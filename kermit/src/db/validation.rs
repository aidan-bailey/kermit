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
//! they are total. Callers that read a catalog first or time the join
//! (`kermit join`, `bench run`) also call [`validate_query`] up front,
//! against headers read without the data ([`kermit_ds::read_csv_header`] /
//! [`kermit_ds::read_parquet_header`]), so a malformed query fails before
//! anything is built or timed.

use {
    kermit_algos::{
        extract_accesses, rewrite_placeholders, JoinQuery, RewriteError, StatisticsLevel,
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
/// in. Every variant but [`MissingStatistics`](Self::MissingStatistics) means
/// the query itself is malformed.
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
    /// The query optimiser reads statistics the catalog was not built with.
    /// A library-usage error, not a malformed query: build the
    /// [`Catalog`](super::Catalog) at the optimiser's
    /// [`required_statistics`](kermit_algos::QueryOptimiser::required_statistics),
    /// as the CLI always does.
    MissingStatistics {
        /// What the optimiser reads.
        required: StatisticsLevel,
        /// What the catalog gathered.
        available: StatisticsLevel,
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
            | JoinError::MissingStatistics {
                required,
                available,
            } => write!(
                f,
                "the query optimiser reads {required}, but the database gathered only \
                 {available}; build the database at the optimiser's required statistics"
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
/// Implemented for the [`Catalog`](super::Catalog) the join entry points
/// take, and for a slice of headers read before any relation is built, so a
/// query can be checked without paying for the build.
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

/// Checks that `query` can run over a store holding `relations`.
///
/// The checks run in a fixed order and the first failure is returned:
///
/// 1. every head term is a variable, and none repeats;
/// 2. for each body atom, in order: the store holds the relation, and the atom
///    has one term per column;
/// 3. every head variable appears in the body;
/// 4. every constant is a dictionary ID (`c<digits>`), which access extraction
///    ([`extract_accesses`]) decides.
///
/// Every relation name is acceptable: indexes carry ids of their own, so no
/// name is reserved, and the order an atom binds its columns in is the
/// plan's to choose, never an error.
///
/// Costs `O(#atoms + #head terms²)` lookups plus one pass over the query;
/// it never reads a relation's tuples.
///
/// # Errors
///
/// The [`JoinError`] for the first failed check.
pub fn validate_query(
    query: &JoinQuery, relations: &(impl RelationArities + ?Sized),
) -> Result<(), JoinError> {
    check_head_terms(query)?;
    check_body_atoms(query, relations)?;
    check_head_bound(query)?;
    check_constants(query)
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

/// Access extraction parses each constant as `c<digits>`, so asking it is the
/// check. It expects the placeholders rewritten, as `prepare` does.
fn check_constants(query: &JoinQuery) -> Result<(), JoinError> {
    extract_accesses(&rewrite_placeholders(query.clone()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use {super::*, crate::db::Catalog, kermit_ds::Tuples};

    /// `edge/2` and `r/3`, as headers read before any build.
    fn headers() -> Vec<RelationHeader> {
        vec![
            RelationHeader::new("edge", vec!["src".into(), "dst".into()]),
            RelationHeader::new("r", vec!["a".into(), "b".into(), "c".into()]),
        ]
    }

    fn validate(q: &str) -> Result<(), JoinError> {
        validate_query(&q.parse().unwrap(), headers().as_slice())
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
            // Opposite column orders are a plan's concern, never a validation
            // error: each atom becomes an index in the plan's order.
            "Q(X, Y) :- edge(X, Y), edge(Y, X).",
            "Q(X) :- r(X, _, Y), edge(Y, X).",
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

    /// `Const_`, `Select_` and `Index_` were reserved for the predicates the
    /// join synthesised. Indexes now carry ids no relation name can take, so a
    /// name with those prefixes is an ordinary one: unknown unless the store
    /// holds it.
    #[test]
    fn names_with_the_old_synthetic_prefixes_are_ordinary() {
        for (q, relation) in [
            ("Q(X) :- Const_c5(X).", "Const_c5"),
            ("Q(X) :- Select_0_edge(X, Y).", "Select_0_edge"),
            ("Q(X, Y) :- Index_1_0_edge(X, Y).", "Index_1_0_edge"),
        ] {
            assert_eq!(
                validate(q),
                Err(JoinError::UnknownRelation {
                    relation: relation.into(),
                    known: vec!["edge".into(), "r".into()],
                }),
                "{q}"
            );
        }
        let mut held = headers();
        held.push(RelationHeader::new("Const_c5", vec!["a".into()]));
        let query: JoinQuery = "Q(X) :- Const_c5(X).".parse().unwrap();
        assert_eq!(validate_query(&query, held.as_slice()), Ok(()));
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

    /// Checks run head first, then atom by atom, then the head's variables,
    /// then the constants, so the first problem in that order is the one
    /// reported.
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
        assert!(matches!(
            validate("Q(X) :- edge(X, cfoo), nope(X)."),
            Err(JoinError::UnknownRelation { .. })
        ));
        assert!(matches!(
            validate("Q(Y) :- edge(X, cfoo)."),
            Err(JoinError::UnboundHeadVariable { .. })
        ));
    }

    /// A catalog and the headers its relations were read from accept and
    /// reject the same queries with the same error.
    #[test]
    fn headers_and_a_catalog_agree() {
        let catalog = Catalog::new(
            headers()
                .into_iter()
                .map(|header| {
                    let empty = Tuples::new(header.arity());
                    (header, empty)
                })
                .collect(),
            StatisticsLevel::TupleCounts,
        );
        for q in [
            "Q(X, Y) :- edge(X, Y).",
            "Q(X, Y) :- nope(X, Y).",
            "Q(X) :- edge(X).",
            "Q(X, Y) :- edge(X, Y), edge(Y, X).",
            "Q(X) :- edge(X, cfoo).",
        ] {
            let query: JoinQuery = q.parse().unwrap();
            assert_eq!(
                validate_query(&query, &catalog),
                validate_query(&query, headers().as_slice()),
                "{q}"
            );
        }
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

    /// The variants that stay keep the exact text callers and tests match.
    #[test]
    fn the_remaining_variants_keep_their_messages() {
        let message = |error: JoinError| error.to_string();
        assert_eq!(
            message(JoinError::NonVariableHeadTerm {
                term: "c5".into()
            }),
            "head term `c5` is not a variable: every head term must be a variable bound by the \
             body (head placeholders and constants are not supported)"
        );
        assert_eq!(
            message(JoinError::RepeatedHeadVariable {
                variable: "X".into()
            }),
            "head variable `X` appears more than once: each head variable may name only one \
             output column"
        );
        assert_eq!(
            message(JoinError::UnboundHeadVariable {
                variable: "Y".into()
            }),
            "head variable `Y` does not appear in the body, so nothing binds it"
        );
        let bad = RewriteError::BadAtom("cfoo".into());
        assert_eq!(
            message(JoinError::MalformedConstant(bad.clone())),
            bad.to_string()
        );
        assert_eq!(
            message(JoinError::MissingStatistics {
                required: StatisticsLevel::ColumnDistinct,
                available: StatisticsLevel::TupleCounts,
            }),
            "the query optimiser reads per-column distinct counts, but the database gathered only \
             tuple counts; build the database at the optimiser's required statistics"
        );
    }
}
