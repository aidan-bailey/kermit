//! The checks a query must pass before either join family runs it.
//!
//! `kermit-parser` checks syntax only, and nothing downstream checked what
//! a query *means* against the relations it runs over, so each malformed
//! shape took whatever path the executors' internals allowed and the two
//! families diverged: an unbound head variable panicked in LFTJ but gave
//! no rows under Hash Triejoin, a wrong-arity atom gave a plausible empty
//! result, and an unknown relation panicked (issue #78). [`validate_query`]
//! turns each such shape into one [`JoinError`], identical for every
//! structure.
//!
//! The entry points in [`crate::db`] run the same checks on every join, so
//! they are total. Callers that build an expensive relation store first or
//! time the join (`kermit join`, `bench run`) also call [`validate_query`]
//! up front, against headers read without the data
//! ([`kermit_ds::read_csv_header`] / [`kermit_ds::read_parquet_header`]),
//! so a malformed query fails before anything is built or timed.

use {
    kermit_algos::{
        check_attribute_order, is_const_predicate, is_index_predicate, is_selection_predicate,
        rewrite_atoms, rewrite_placeholders, rewrite_repeated_variables, ColumnOrderPolicy,
        ConstSpec, JoinQuery, RewriteError, SelectionSpec, StatisticsLevel, CONST_PREDICATE_PREFIX,
        INDEX_PREDICATE_PREFIX, SELECTION_PREDICATE_PREFIX,
    },
    kermit_ds::{Relation, RelationHeader},
    kermit_parser::Term,
    std::{
        collections::{BTreeMap, BTreeSet},
        fmt,
    },
};

/// Why a query cannot run over a relation store.
///
/// One variant per check; [`validate_query`] documents the order they run
/// in. Every variant but [`CyclicAttributeOrder`](Self::CyclicAttributeOrder),
/// [`MissingStatistics`](Self::MissingStatistics) and
/// [`MissingIndex`](Self::MissingIndex) means the query itself is
/// malformed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JoinError {
    /// A head term is a placeholder or a constant. The join projects to the
    /// head by keeping the first `head.len()` columns of each result row,
    /// which are the head's columns only when every head term is a distinct
    /// variable; head constants are not supported.
    NonVariableHeadTerm {
        /// The term as written, e.g. `_` or `c5`.
        term: String,
    },
    /// A variable names more than one head column (`Q(X, X)`).
    RepeatedHeadVariable {
        /// The repeated variable.
        variable: String,
    },
    /// A body atom names a relation with a prefix reserved for the
    /// predicates the join synthesises (`Const_`, `Select_`, `Index_`).
    ReservedRelationName {
        /// The relation name as written.
        relation: String,
    },
    /// A body atom names a relation the store does not hold.
    UnknownRelation {
        /// The relation name as written.
        relation: String,
        /// Every relation the store holds, sorted.
        known: Vec<String>,
    },
    /// A body atom has a different number of terms than its relation has
    /// columns. Constants and placeholders count: each fills one column.
    ArityMismatch {
        /// The atom as written, e.g. `edge(X, Y, Z)`.
        atom: String,
        /// The relation it names.
        relation: String,
        /// How many terms the atom has.
        terms: usize,
        /// How many columns the relation has.
        arity: usize,
    },
    /// A constant is not a dictionary ID of the form `c<digits>`.
    MalformedConstant(RewriteError),
    /// A head variable appears in no body atom, so nothing binds it.
    UnboundHeadVariable {
        /// The unbound variable.
        variable: String,
    },
    /// The body atoms need their relations' columns in contradictory
    /// orders under `--column-orders stored`. A limitation of reading each
    /// relation in one column order, not malformed input: each atom is
    /// matched one column at a time, left to right, so
    /// `edge(X, Y), edge(Y, X)` would need `edge` sorted both ways at
    /// once. `--column-orders any` lifts it by reading one of the atoms
    /// through a reordered copy.
    CyclicAttributeOrder {
        /// The atoms on one such cycle, as written, in body order.
        atoms: Vec<String>,
    },
    /// The query optimiser reads statistics the database was not built
    /// with. A library-usage error, not a malformed query: build the
    /// [`Database`](super::Database) at the optimiser's
    /// [`required_statistics`](kermit_algos::QueryOptimiser::required_statistics),
    /// as the CLI always does.
    MissingStatistics {
        /// What the optimiser reads.
        required: StatisticsLevel,
        /// What the database gathered.
        available: StatisticsLevel,
    },
    /// The plan reads an atom through a reordered copy the database does
    /// not hold. A library-usage error: build the copies
    /// [`Database::required_indexes`](super::Database::required_indexes)
    /// names and [`add_index`](super::Database::add_index) them before
    /// joining, as the CLI and the test harness do.
    MissingIndex {
        /// The copy's name, e.g. `Index_1_0_edge`.
        index: String,
        /// The relation it would be built from.
        base: String,
    },
}

impl fmt::Display for JoinError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            | JoinError::NonVariableHeadTerm {
                term,
            } => write!(
                f,
                "head term `{term}` is not a variable: every head term must be a variable bound \
                 by the body (head placeholders and constants are not supported)"
            ),
            | JoinError::RepeatedHeadVariable {
                variable,
            } => write!(
                f,
                "head variable `{variable}` appears more than once: each head variable may name \
                 only one output column"
            ),
            | JoinError::ReservedRelationName {
                relation,
            } => write!(
                f,
                "relation name {relation:?} is reserved: the prefixes {CONST_PREDICATE_PREFIX:?}, \
                 {SELECTION_PREDICATE_PREFIX:?} and {INDEX_PREDICATE_PREFIX:?} name predicates \
                 the join synthesises from constants, repeated variables and reordered copies"
            ),
            | JoinError::UnknownRelation {
                relation,
                known,
            } => write!(
                f,
                "query body references unknown relation {relation:?}; known relations: {known:?}"
            ),
            | JoinError::ArityMismatch {
                atom,
                relation,
                terms,
                arity,
            } => write!(
                f,
                "atom `{atom}` has {terms} term(s), but relation {relation:?} has arity {arity}"
            ),
            | JoinError::MalformedConstant(e) => write!(f, "{e}"),
            | JoinError::UnboundHeadVariable {
                variable,
            } => write!(
                f,
                "head variable `{variable}` does not appear in the body, so nothing binds it"
            ),
            | JoinError::CyclicAttributeOrder {
                atoms,
            } => write!(
                f,
                "unsupported query under --column-orders stored: atoms {} need their relations' \
                 columns in contradictory orders. Each body atom is matched one column at a time, \
                 left to right (subject before object for an RDF triple), so it fixes the order \
                 its variables are bound in, and these atoms fix opposite orders. This is a \
                 limitation of reading each relation in its stored column order, not an error in \
                 the query; `--column-orders any` answers it over a reordered copy",
                atoms
                    .iter()
                    .map(|a| format!("`{a}`"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            | JoinError::MissingStatistics {
                required,
                available,
            } => write!(
                f,
                "the query optimiser reads {required}, but the database gathered only \
                 {available}; build the database at the optimiser's required statistics"
            ),
            | JoinError::MissingIndex {
                index,
                base,
            } => write!(
                f,
                "the plan reads {base:?} through the reordered copy {index:?}, which the database \
                 does not hold; build the copies `required_indexes` names before joining"
            ),
        }
    }
}

impl std::error::Error for JoinError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            | JoinError::MalformedConstant(e) => Some(e),
            | _ => None,
        }
    }
}

impl From<RewriteError> for JoinError {
    fn from(e: RewriteError) -> Self { JoinError::MalformedConstant(e) }
}

/// What validation needs from a relation store: each relation's arity,
/// by name.
///
/// Implemented for the store the join entry points take, and for a slice
/// of headers read before any relation is built, so a query can be
/// checked without paying for the build.
pub trait RelationArities {
    /// The arity of the relation named `relation`, if the store holds one.
    fn arity(&self, relation: &str) -> Option<usize>;

    /// Every relation name, sorted and distinct. Read only to report an
    /// unknown relation.
    fn relation_names(&self) -> Vec<String>;
}

impl<R: Relation> RelationArities for BTreeMap<String, R> {
    fn arity(&self, relation: &str) -> Option<usize> {
        self.get(relation).map(|r| r.header().arity())
    }

    fn relation_names(&self) -> Vec<String> { self.keys().cloned().collect() }
}

/// Headers keyed by [`RelationHeader::name`], the key the CLI stores each
/// built relation under. As in that store, the last header with a given
/// name wins.
impl RelationArities for [RelationHeader] {
    fn arity(&self, relation: &str) -> Option<usize> {
        self.iter()
            .rev()
            .find(|h| h.name() == relation)
            .map(RelationHeader::arity)
    }

    fn relation_names(&self) -> Vec<String> {
        self.iter()
            .map(|h| h.name().to_string())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }
}

/// A validated query, rewritten into the form the executors run, plus the
/// synthetic predicates the rewrites introduced.
pub(super) struct Prepared {
    /// The query after the const, placeholder and selection rewrites.
    pub(super) query: JoinQuery,
    /// One entry per rewritten constant.
    pub(super) const_specs: Vec<ConstSpec>,
    /// One entry per atom that repeated a variable.
    pub(super) selection_specs: Vec<SelectionSpec>,
}

/// Validates `query` against `relations` under `column_orders`, then
/// rewrites a copy of it for the executors. The one path both
/// [`validate_query`] and the join entry points take, so they accept
/// exactly the same queries.
pub(super) fn prepare(
    query: &JoinQuery, relations: &(impl RelationArities + ?Sized),
    column_orders: ColumnOrderPolicy,
) -> Result<Prepared, JoinError> {
    check_head_terms(query)?;
    check_body_atoms(query, relations)?;
    check_head_bound(query)?;

    // Const, then placeholder, then selection: the selection pass then sees
    // a body of variables only, and all three share one fresh-variable
    // counter, so no fresh variable is ever mistaken for a repeat.
    let (rewritten, const_specs) = rewrite_atoms(query.clone())?;
    let rewritten = rewrite_placeholders(rewritten);
    let (rewritten, selection_specs) = rewrite_repeated_variables(rewritten);

    // Under `stored` every atom binds its columns left to right, so the
    // atoms must admit one order per relation; under `any` the planner is
    // free and the orientation rewrite reads a disagreeing atom through a
    // reordered copy, so there is nothing to check. The rewrites decide
    // which variables each atom carries, so the check runs on their
    // output; they keep body atoms in place, so the indices it reports
    // locate the atoms as the user wrote them.
    if column_orders == ColumnOrderPolicy::Stored {
        check_attribute_order(&rewritten).map_err(|cycle| JoinError::CyclicAttributeOrder {
            atoms: cycle
                .atoms
                .iter()
                .map(|&i| query.body[i].to_string())
                .collect(),
        })?;
    }

    Ok(Prepared {
        query: rewritten,
        const_specs,
        selection_specs,
    })
}

/// Checks that `query` can run over a store holding `relations` under
/// `column_orders`.
///
/// The checks run in a fixed order and the first failure is returned:
///
/// 1. every head term is a variable, and none repeats;
/// 2. for each body atom, in order: its relation name is not reserved, the
///    store holds the relation, and the atom has one term per column;
/// 3. every head variable appears in the body;
/// 4. every constant is a dictionary ID (`c<digits>`);
/// 5. under `stored`, the atoms admit one column order per relation.
///
/// Costs `O(#atoms + #head terms²)` lookups plus the query rewrites; it
/// never reads a relation's tuples.
///
/// # Errors
///
/// The [`JoinError`] for the first failed check.
pub fn validate_query(
    query: &JoinQuery, relations: &(impl RelationArities + ?Sized),
    column_orders: ColumnOrderPolicy,
) -> Result<(), JoinError> {
    prepare(query, relations, column_orders).map(|_| ())
}

fn check_head_terms(query: &JoinQuery) -> Result<(), JoinError> {
    let terms = &query.head.terms;
    for (i, term) in terms.iter().enumerate() {
        let Term::Var(variable) = term else {
            return Err(JoinError::NonVariableHeadTerm {
                term: term.to_string(),
            });
        };
        // Quadratic in the head's width, which is tiny, and allocation-free:
        // this runs inside every join.
        if terms[..i].contains(term) {
            return Err(JoinError::RepeatedHeadVariable {
                variable: variable.clone(),
            });
        }
    }
    Ok(())
}

fn check_body_atoms(
    query: &JoinQuery, relations: &(impl RelationArities + ?Sized),
) -> Result<(), JoinError> {
    for atom in &query.body {
        if is_const_predicate(&atom.name)
            || is_selection_predicate(&atom.name)
            || is_index_predicate(&atom.name)
        {
            return Err(JoinError::ReservedRelationName {
                relation: atom.name.clone(),
            });
        }
        let arity = relations
            .arity(&atom.name)
            .ok_or_else(|| JoinError::UnknownRelation {
                relation: atom.name.clone(),
                known: relations.relation_names(),
            })?;
        if atom.terms.len() != arity {
            return Err(JoinError::ArityMismatch {
                atom: atom.to_string(),
                relation: atom.name.clone(),
                terms: atom.terms.len(),
                arity,
            });
        }
    }
    Ok(())
}

fn check_head_bound(query: &JoinQuery) -> Result<(), JoinError> {
    for term in &query.head.terms {
        if !query.body.iter().any(|atom| atom.terms.contains(term)) {
            return Err(JoinError::UnboundHeadVariable {
                variable: term.to_string(),
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `edge/2` and `r/3`, as headers read before any build.
    fn headers() -> Vec<RelationHeader> {
        vec![
            RelationHeader::new("edge", vec!["src".into(), "dst".into()]),
            RelationHeader::new("r", vec!["a".into(), "b".into(), "c".into()]),
        ]
    }

    fn validate(q: &str) -> Result<(), JoinError> {
        validate_query(
            &q.parse().unwrap(),
            headers().as_slice(),
            ColumnOrderPolicy::Stored,
        )
    }

    fn validate_any(q: &str) -> Result<(), JoinError> {
        validate_query(
            &q.parse().unwrap(),
            headers().as_slice(),
            ColumnOrderPolicy::Any,
        )
    }

    #[test]
    fn well_formed_queries_pass() {
        for q in [
            "Q(X, Y) :- edge(X, Y).",
            "Q(X) :- edge(X, Y).",
            "Q(X) :- edge(X, _).",
            "Q(X, Y) :- r(X, _, Y).",
            "Q(X) :- edge(X, c5).",
            "Q(X) :- edge(X, X).",
            "Q(X, Y, Z) :- edge(X, Y), edge(Y, Z), edge(X, Z).",
        ] {
            assert_eq!(validate(q), Ok(()), "{q}");
        }
    }

    #[test]
    fn unknown_relation_lists_the_known_ones() {
        let err = validate("Q(X, Y) :- nope(X, Y).").unwrap_err();
        assert_eq!(err, JoinError::UnknownRelation {
            relation: "nope".into(),
            known: vec!["edge".into(), "r".into()],
        });
        assert_eq!(
            err.to_string(),
            r#"query body references unknown relation "nope"; known relations: ["edge", "r"]"#
        );
    }

    #[test]
    fn synthetic_prefixes_are_reserved() {
        for (q, relation) in [
            ("Q(X) :- Const_c5(X).", "Const_c5"),
            ("Q(X) :- Select_0_edge(X, Y).", "Select_0_edge"),
            ("Q(X, Y) :- Index_1_0_edge(X, Y).", "Index_1_0_edge"),
        ] {
            assert_eq!(
                validate(q),
                Err(JoinError::ReservedRelationName {
                    relation: relation.into()
                }),
                "{q}"
            );
        }
    }

    #[test]
    fn too_many_terms_is_an_arity_mismatch() {
        let err = validate("Q(X, Y, Z) :- edge(X, Y, Z).").unwrap_err();
        assert_eq!(err, JoinError::ArityMismatch {
            atom: "edge(X, Y, Z)".into(),
            relation: "edge".into(),
            terms: 3,
            arity: 2,
        });
        assert_eq!(
            err.to_string(),
            r#"atom `edge(X, Y, Z)` has 3 term(s), but relation "edge" has arity 2"#
        );
    }

    #[test]
    fn too_few_terms_is_an_arity_mismatch() {
        assert!(matches!(
            validate("Q(X) :- edge(X)."),
            Err(JoinError::ArityMismatch {
                terms: 1,
                arity: 2,
                ..
            })
        ));
    }

    /// A constant or a placeholder fills one column, like a variable.
    #[test]
    fn constants_and_placeholders_count_towards_arity() {
        assert!(matches!(
            validate("Q(X) :- edge(X, _, c5)."),
            Err(JoinError::ArityMismatch {
                terms: 3,
                arity: 2,
                ..
            })
        ));
    }

    #[test]
    fn malformed_constant_is_reported() {
        assert_eq!(
            validate("Q(X) :- edge(X, cfoo)."),
            Err(JoinError::MalformedConstant(RewriteError::BadAtom(
                "cfoo".into()
            )))
        );
    }

    #[test]
    fn unbound_head_variable_is_reported() {
        assert_eq!(
            validate("Q(X, Y) :- edge(X, Z)."),
            Err(JoinError::UnboundHeadVariable {
                variable: "Y".into()
            })
        );
    }

    #[test]
    fn repeated_head_variable_is_reported() {
        assert_eq!(
            validate("Q(X, X) :- edge(X, Y)."),
            Err(JoinError::RepeatedHeadVariable {
                variable: "X".into()
            })
        );
    }

    #[test]
    fn head_placeholders_and_constants_are_reported() {
        for (q, term) in [
            ("Q(_) :- edge(X, Y).", "_"),
            ("Q(c5, X) :- edge(X, Y).", "c5"),
        ] {
            assert_eq!(
                validate(q),
                Err(JoinError::NonVariableHeadTerm {
                    term: term.into()
                }),
                "{q}"
            );
        }
    }

    #[test]
    fn opposite_column_orders_are_a_limitation() {
        let err = validate("Q(X, Y) :- edge(X, Y), edge(Y, X).").unwrap_err();
        assert_eq!(err, JoinError::CyclicAttributeOrder {
            atoms: vec!["edge(X, Y)".into(), "edge(Y, X)".into()],
        });
        let message = err.to_string();
        assert!(message.contains("limitation"), "{message}");
        assert!(message.contains("left to right"), "{message}");
    }

    /// The cycle runs through the placeholder's fresh variable, but the
    /// error quotes the atoms as written.
    #[test]
    fn a_cycle_through_a_placeholder_quotes_the_written_atoms() {
        assert_eq!(
            validate("Q(X) :- r(X, _, Y), edge(Y, X)."),
            Err(JoinError::CyclicAttributeOrder {
                atoms: vec!["r(X, _, Y)".into(), "edge(Y, X)".into()],
            })
        );
    }

    /// Checks run head first, then atom by atom, so the first problem in
    /// reading order is the one reported.
    #[test]
    fn the_first_failed_check_wins() {
        assert!(matches!(
            validate("Q(X, X) :- nope(Y)."),
            Err(JoinError::RepeatedHeadVariable { .. })
        ));
        assert!(matches!(
            validate("Q(X) :- edge(X, Y, Z), nope(X)."),
            Err(JoinError::ArityMismatch { .. })
        ));
        assert!(matches!(
            validate("Q(X, W) :- nope(X)."),
            Err(JoinError::UnknownRelation { .. })
        ));
    }

    /// A store and the headers its relations were built from accept and
    /// reject the same queries with the same error.
    #[test]
    fn headers_and_built_relations_agree() {
        use kermit_ds::TreeTrie;
        let store: BTreeMap<String, TreeTrie> = headers()
            .into_iter()
            .map(|h| (h.name().to_string(), TreeTrie::from_tuples(h, vec![])))
            .collect();
        for q in [
            "Q(X, Y) :- edge(X, Y).",
            "Q(X, Y) :- nope(X, Y).",
            "Q(X) :- edge(X).",
            "Q(X, Y) :- edge(X, Y), edge(Y, X).",
        ] {
            let query: JoinQuery = q.parse().unwrap();
            for policy in [ColumnOrderPolicy::Stored, ColumnOrderPolicy::Any] {
                assert_eq!(
                    validate_query(&query, &store, policy),
                    validate_query(&query, headers().as_slice(), policy),
                    "{q}"
                );
            }
        }
    }

    #[test]
    fn under_any_opposite_column_orders_are_accepted() {
        assert_eq!(validate_any("Q(X, Y) :- edge(X, Y), edge(Y, X)."), Ok(()));
        assert_eq!(validate_any("Q(X) :- r(X, _, Y), edge(Y, X)."), Ok(()));
        // Every other check still runs.
        assert!(matches!(
            validate_any("Q(X, Y) :- edge(X, Z)."),
            Err(JoinError::UnboundHeadVariable { .. })
        ));
    }

    #[test]
    fn the_cyclic_message_names_the_flag_that_lifts_it() {
        let err = validate("Q(X, Y) :- edge(X, Y), edge(Y, X).").unwrap_err();
        let message = err.to_string();
        assert!(message.contains("--column-orders any"), "{message}");
        assert!(message.contains("limitation"), "{message}");
        let missing = JoinError::MissingIndex {
            index: "Index_1_0_edge".into(),
            base: "edge".into(),
        };
        assert!(missing.to_string().contains("required_indexes"));
    }

    #[test]
    fn the_last_header_with_a_name_wins() {
        let headers = vec![
            RelationHeader::new("edge", vec!["a".into()]),
            RelationHeader::new("edge", vec!["a".into(), "b".into()]),
        ];
        assert_eq!(headers.as_slice().arity("edge"), Some(2));
        assert_eq!(headers.as_slice().relation_names(), vec!["edge"]);
    }
}
