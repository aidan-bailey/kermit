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
//! Intended to run before [`crate::extract_accesses`], which expects it to
//! have run.

use kermit_parser::{JoinQuery, Predicate, Term};

/// Rewrites `query.body` so that no atom contains a placeholder: each `_`
/// becomes its own fresh variable `K<i>`.
///
/// Fresh variables start past the highest existing `K<n>` in the query,
/// and each is distinct, so access extraction never mistakes one for a
/// repeated variable.
///
/// # Head asymmetry
///
/// **Only body atoms are rewritten.** A head placeholder names no output
/// column; `kermit::db` rejects it before any rewrite runs.
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

/// Highest `n` among existing `K<n>` variable names in `query`, if any. A
/// fresh name starts past it, so it never collides with a user's own `K0`,
/// `K1`, …; scanning the head too keeps a head-only `K3` from being reused.
pub(crate) fn highest_k_index(query: &JoinQuery) -> Option<usize> {
    let scan = |p: &Predicate| -> Option<usize> {
        p.terms
            .iter()
            .filter_map(|t| match t {
                | Term::Var(name) => name.strip_prefix('K').and_then(|r| r.parse::<usize>().ok()),
                | _ => None,
            })
            .max()
    };
    query
        .body
        .iter()
        .chain(std::iter::once(&query.head))
        .filter_map(scan)
        .max()
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::{analyse, extract_accesses},
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

    /// A constant beside a placeholder stays a constant: the placeholder
    /// takes the first fresh name, and access extraction keeps its column
    /// while the constant becomes a filter.
    #[test]
    fn composes_before_access_extraction_with_a_constant() {
        let out = rewrite_placeholders(parse("Q(X) :- r(c5, _, X)."));
        let extracted = extract_accesses(&out).unwrap();
        let access = &extracted.accesses[0];
        assert_eq!(access.columns, vec![1, 2]);
        assert_eq!(access.variables, vec!["K0".to_string(), "X".to_string()]);
        assert_eq!(access.filter.constants(), &[(0, 5)]);
    }

    /// A placeholder between two occurrences of one variable is neither a
    /// source nor a repeat for the equality that extraction records.
    #[test]
    fn composes_before_access_extraction_with_a_repeated_variable() {
        let out = rewrite_placeholders(parse("Q(X) :- r(X, _, X)."));
        let names: Vec<&str> = out.body[0].terms.iter().map(var).collect();
        assert_eq!(names, vec!["X", "K0", "X"]);
        let extracted = extract_accesses(&out).unwrap();
        let access = &extracted.accesses[0];
        assert_eq!(access.columns, vec![0, 1]);
        assert_eq!(access.variables, vec!["X".to_string(), "K0".to_string()]);
        assert_eq!(access.filter.equalities(), &[(0, 2)]);
    }
}
