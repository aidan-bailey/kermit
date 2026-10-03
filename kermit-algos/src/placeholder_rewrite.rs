//! Placeholder rewrite: every `_` in a body atom becomes a fresh variable.
//!
//! A placeholder occupies a column but binds nothing, so it is exactly a
//! variable that appears nowhere else in the query: `r(X, _, Y)` is
//! `r(X, K0, Y)`. Both executors assume that each body atom carries one
//! variable per column — the sorted iterators open one physical level per
//! variable, and Hash Triejoin expects every iterator at its leaf once
//! the last variable is bound — so a short variable list shifts the
//! columns after a middle placeholder onto the wrong level, and leaves a
//! trailing one unreached (issue #73). Rewriting each placeholder to a
//! fresh body-only variable restores one variable per column, which makes
//! a placeholder mean exactly what an unused named variable means: the
//! result keeps one row per matching tuple (bag semantics), and the
//! fresh column is dropped when the result is projected to the head.
//! Intended to run after [`crate::rewrite_atoms`] and before
//! [`crate::rewrite_repeated_variables`].

use {
    crate::const_rewrite::highest_k_index,
    kermit_parser::{JoinQuery, Term},
};

/// Rewrites `query.body` so that no atom contains a placeholder: each `_`
/// becomes its own fresh variable `K<i>`.
///
/// Fresh variables share the `K<n>` counter with [`crate::rewrite_atoms`]
/// and [`crate::rewrite_repeated_variables`] (all three start past the
/// highest existing `K<n>`), and each is distinct, so the selection
/// rewrite that follows never mistakes one for a repeat.
///
/// # Head asymmetry
///
/// As with the other rewrites, **only body atoms are rewritten**. A head
/// placeholder names no output column; `kermit::db` rejects it before
/// any rewrite runs.
pub fn rewrite_placeholders(mut query: JoinQuery) -> JoinQuery {
    let mut next_k = highest_k_index(&query).map_or(0, |n| n + 1);
    for term in query.body.iter_mut().flat_map(|pred| &mut pred.terms) {
        if matches!(term, Term::Placeholder) {
            *term = Term::Var(format!("K{next_k}"));
            next_k += 1;
        }
    }
    query
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::{analyse, rewrite_atoms, rewrite_repeated_variables, ColumnEquality},
        kermit_parser::Term,
    };

    fn parse(q: &str) -> JoinQuery { q.parse().unwrap() }

    fn var(t: &Term) -> &str {
        match t {
            | Term::Var(n) => n,
            | other => panic!("expected a variable, got {other:?}"),
        }
    }

    #[test]
    fn no_placeholders_is_identity() {
        let q = parse("Q(X, Y) :- p(X, Y), r(Y, c5).");
        assert_eq!(rewrite_placeholders(q.clone()), q);
    }

    #[test]
    fn trailing_placeholder_becomes_a_fresh_variable() {
        let out = rewrite_placeholders(parse("Q(X) :- edge(X, _)."));
        assert_eq!(out.body[0].name, "edge");
        assert_eq!(var(&out.body[0].terms[0]), "X");
        assert_eq!(var(&out.body[0].terms[1]), "K0");
    }

    /// The column after a middle placeholder keeps its position: `Y` still
    /// names the third column, not the second.
    #[test]
    fn middle_placeholder_keeps_later_columns_in_place() {
        let out = rewrite_placeholders(parse("Q(X, Y) :- r(X, _, Y)."));
        let names: Vec<&str> = out.body[0].terms.iter().map(var).collect();
        assert_eq!(names, vec!["X", "K0", "Y"]);
    }

    #[test]
    fn every_occurrence_gets_its_own_variable() {
        let out = rewrite_placeholders(parse("Q(X) :- r(X, _, _), s(_, X)."));
        assert_eq!(var(&out.body[0].terms[1]), "K0");
        assert_eq!(var(&out.body[0].terms[2]), "K1");
        assert_eq!(var(&out.body[1].terms[0]), "K2");
    }

    #[test]
    fn fresh_variables_avoid_existing_k_names() {
        let out = rewrite_placeholders(parse("Q(K3) :- r(K3, _)."));
        assert_eq!(var(&out.body[0].terms[1]), "K4");
    }

    /// Head terms describe the output, so only the body is rewritten; the
    /// caller rejects a head placeholder before any rewrite runs.
    #[test]
    fn head_placeholders_are_not_rewritten() {
        let out = rewrite_placeholders(parse("Q(_) :- r(X)."));
        assert!(matches!(out.head.terms[0], Term::Placeholder));
    }

    /// After the rewrite, each atom carries one variable per column, which
    /// is what both executors assume of a body atom.
    #[test]
    fn every_atom_carries_one_variable_per_column() {
        let out = rewrite_placeholders(parse("Q(X, Y) :- r(X, _, Y), s(_, _)."));
        let analysis = analyse(&out);
        assert_eq!(analysis.predicate_variables, vec![vec![0, 2, 1], vec![
            3, 4
        ]]);
    }

    /// The const rewrite runs first and takes `K0`; the placeholder takes
    /// the next index from the shared counter.
    #[test]
    fn composes_after_the_const_rewrite() {
        let (q, _) = rewrite_atoms(parse("Q(X) :- r(c5, _, X).")).unwrap();
        let out = rewrite_placeholders(q);
        let names: Vec<&str> = out.body[0].terms.iter().map(var).collect();
        assert_eq!(names, vec!["K0", "K1", "X"]);
    }

    /// A placeholder between two occurrences of one variable is neither a
    /// source nor a repeat for the selection rewrite that follows.
    #[test]
    fn composes_before_the_selection_rewrite() {
        let q = rewrite_placeholders(parse("Q(X) :- r(X, _, X)."));
        let (out, specs) = rewrite_repeated_variables(q);
        let names: Vec<&str> = out.body[0].terms.iter().map(var).collect();
        assert_eq!(names, vec!["X", "K0", "K1"]);
        assert_eq!(specs[0].equalities, vec![ColumnEquality {
            source: 0,
            repeat: 2,
        }]);
    }
}
