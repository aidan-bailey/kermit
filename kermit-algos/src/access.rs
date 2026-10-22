//! Access extraction: the query-to-views half of the per-query index build.
//!
//! A body atom such as `p(X, c42, X, Y)` is not a relation the executors can
//! descend as written: `c42` is a constant and `X` repeats. Relational
//! algebra reads it as a selection and a projection over `p`,
//! `π_{X,Y}(σ_{col1 = 42 ∧ col0 = col2}(p))`, and that is what an
//! [`Access`] records: the relation, a [`Filter`] (the selection) and the
//! kept columns (the projection) with the variable each one binds. The
//! build copies a relation through such a view once, so the executors see
//! only distinct variables and no constants.
//!
//! [`extract_accesses`] runs on the query after
//! [`crate::rewrite_placeholders`]. Each atom of the access query it returns
//! is named `@<k>` (see [`ACCESS_PREFIX`]) so the optimisers plan over
//! accesses; a name no relation can have, since the parser admits
//! `[A-Za-z][A-Za-z0-9_]*` only.

use {
    crate::const_rewrite::RewriteError,
    kermit_iters::Filter,
    kermit_parser::{JoinQuery, Predicate, Term},
    std::collections::HashMap,
};

/// Prefix of the ids of the atoms of an [`AccessQuery`]: `@0`, `@1`, …, the
/// position of the access in [`AccessQuery::accesses`].
pub const ACCESS_PREFIX: &str = "@";

/// One body atom read as a selection and a projection of its relation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Access {
    /// The relation the atom names.
    pub relation: String,
    /// The constants and column equalities the atom imposes on its rows.
    pub filter: Filter,
    /// The kept source columns, in source order: each variable's first
    /// occurrence in the atom.
    pub columns: Vec<usize>,
    /// The variable each kept column binds, parallel to `columns`.
    pub variables: Vec<String>,
}

/// A query's body as accesses.
#[derive(Clone, Debug)]
pub struct AccessQuery {
    /// The query with its head unchanged. `body[i]` is the atom `@{i}` over
    /// `accesses[i].variables`.
    pub query: JoinQuery,
    /// The body atoms with at least one kept column, in body order.
    pub accesses: Vec<Access>,
    /// The body atoms with none (only constants), in body order. Each is a
    /// test that some row matches, and its row count multiplies the answer.
    pub checks: Vec<Access>,
}

/// Reads every body atom of `query` as an [`Access`].
///
/// Expects [`crate::rewrite_placeholders`] to have run, and constants to be
/// `c<digits>`.
///
/// # Errors
///
/// Returns [`RewriteError::BadAtom`] for the first constant, in body order,
/// that is not `c<digits>`.
///
/// # Panics
///
/// Panics if a body atom still holds a placeholder.
pub fn extract_accesses(query: &JoinQuery) -> Result<AccessQuery, RewriteError> {
    let mut accesses: Vec<Access> = Vec::new();
    let mut checks: Vec<Access> = Vec::new();
    for atom in &query.body {
        let access = access_of(atom)?;
        if access.columns.is_empty() {
            checks.push(access);
        } else {
            accesses.push(access);
        }
    }
    let body = accesses
        .iter()
        .enumerate()
        .map(|(k, access)| Predicate {
            name: format!("{ACCESS_PREFIX}{k}"),
            terms: access.variables.iter().cloned().map(Term::Var).collect(),
        })
        .collect();
    Ok(AccessQuery {
        query: JoinQuery {
            head: query.head.clone(),
            body,
        },
        accesses,
        checks,
    })
}

/// One atom as an [`Access`], column by column.
fn access_of(atom: &Predicate) -> Result<Access, RewriteError> {
    let mut columns: Vec<usize> = Vec::new();
    let mut variables: Vec<String> = Vec::new();
    let constants: Vec<(usize, usize)> = Vec::new();
    let mut equalities: Vec<(usize, usize)> = Vec::new();
    let mut first_column: HashMap<&str, usize> = HashMap::new();
    for (column, term) in atom.terms.iter().enumerate() {
        match term {
            | Term::Var(name) => match first_column.get(name.as_str()) {
                | Some(&first) => equalities.push((first, column)),
                | None => {
                    first_column.insert(name.as_str(), column);
                    columns.push(column);
                    variables.push(name.clone());
                },
            },
            | Term::Atom(_) => unimplemented!("constants: Task 9.2 replaces this arm"),
            | Term::Placeholder => {
                panic!("extract_accesses: placeholder in {atom}; run rewrite_placeholders first")
            },
        }
    }
    Ok(Access {
        relation: atom.name.clone(),
        filter: Filter::new(constants, equalities),
        columns,
        variables,
    })
}

#[cfg(test)]
mod tests {
    use {super::*, crate::rewrite_placeholders};

    fn parse(q: &str) -> JoinQuery { q.parse().expect("parse") }

    fn extract(q: &str) -> AccessQuery { extract_accesses(&parse(q)).expect("extract") }

    fn body_text(access_query: &AccessQuery) -> Vec<String> {
        access_query
            .query
            .body
            .iter()
            .map(ToString::to_string)
            .collect()
    }

    fn vars(names: &[&str]) -> Vec<String> { names.iter().map(ToString::to_string).collect() }

    #[test]
    fn plain_atoms_keep_every_column_unfiltered() {
        let out = extract("Q(X, Y, Z) :- R(X, Y), S(Y, Z), T(X, Z).");
        assert_eq!(out.accesses.len(), 3);
        assert!(out.checks.is_empty());
        for access in &out.accesses {
            assert!(access.filter.is_none());
            assert_eq!(access.columns, vec![0, 1]);
        }
        assert_eq!(out.accesses[1].relation, "S");
        assert_eq!(out.accesses[1].variables, vars(&["Y", "Z"]));
        assert_eq!(body_text(&out), vec!["@0(X, Y)", "@1(Y, Z)", "@2(X, Z)"]);
    }

    #[test]
    fn the_head_is_unchanged() {
        let q = parse("Q(Z, X) :- R(X, Y), S(Y, Z).");
        let out = extract_accesses(&q).expect("extract");
        assert_eq!(out.query.head, q.head);
    }

    #[test]
    fn a_self_join_is_two_accesses_of_one_relation() {
        let out = extract("Q(X, Z) :- edge(X, Y), edge(Y, Z).");
        assert_eq!(out.accesses.len(), 2);
        assert_eq!(out.accesses[0].relation, "edge");
        assert_eq!(out.accesses[1].relation, "edge");
        assert_eq!(body_text(&out), vec!["@0(X, Y)", "@1(Y, Z)"]);
    }

    #[test]
    fn a_repeated_variable_becomes_an_equality_with_its_first_column() {
        let out = extract("Q(X) :- r(X, X).");
        let access = &out.accesses[0];
        assert_eq!(access.columns, vec![0]);
        assert_eq!(access.variables, vars(&["X"]));
        assert_eq!(access.filter, Filter::new(vec![], vec![(0, 1)]));
        assert_eq!(body_text(&out), vec!["@0(X)"]);
    }

    #[test]
    fn a_triple_repeat_is_two_equalities_with_the_first_column() {
        let out = extract("Q(X, Y) :- r(X, X, Y, X).");
        let access = &out.accesses[0];
        assert_eq!(access.columns, vec![0, 2]);
        assert_eq!(access.variables, vars(&["X", "Y"]));
        assert_eq!(access.filter.equalities(), &[(0, 1), (0, 3)]);
        assert!(access.filter.constants().is_empty());
    }

    #[test]
    fn two_repeated_variables_are_two_classes() {
        let out = extract("Q(X, Y) :- r(X, Y, X, Y).");
        let access = &out.accesses[0];
        assert_eq!(access.columns, vec![0, 1]);
        assert_eq!(access.filter.equalities(), &[(0, 2), (1, 3)]);
    }

    #[test]
    fn a_repeat_in_one_atom_does_not_touch_the_next_atom() {
        let out = extract("Q(X, Y) :- r(X, X), s(X, Y).");
        assert_eq!(out.accesses[0].filter.equalities(), &[(0, 1)]);
        assert!(out.accesses[1].filter.is_none());
        assert_eq!(out.accesses[1].columns, vec![0, 1]);
    }

    #[test]
    fn rewritten_placeholders_are_kept_columns() {
        // `r(X, _, X)` is `r(X, K0, X)`: the placeholder keeps its column
        // (bag semantics, #78) and the repeat is still an equality.
        let q = rewrite_placeholders(parse("Q(X) :- r(X, _, X)."));
        let out = extract_accesses(&q).expect("extract");
        let access = &out.accesses[0];
        assert_eq!(access.columns, vec![0, 1]);
        assert_eq!(access.variables, vars(&["X", "K0"]));
        assert_eq!(access.filter, Filter::new(vec![], vec![(0, 2)]));
    }

    #[test]
    #[should_panic(expected = "placeholder")]
    fn an_unrewritten_placeholder_panics() { let _ = extract_accesses(&parse("Q(X) :- r(X, _).")); }
}
