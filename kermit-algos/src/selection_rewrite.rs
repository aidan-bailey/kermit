//! Selection rewrite for variables repeated inside one body atom.
//!
//! Worst-case-optimal join algorithms (Veldhuizen 2014, Freitag et al.
//! SIGMOD 2020) assume that each variable occurs at most once per atom, so
//! that one trie level binds one variable. A query such as
//! `Q(X) :- r(X, X).` violates that: `X` names both columns of `r`, and an
//! executor that registers `r` once "for `X`" opens only its first column.
//!
//! Relational algebra resolves this with a selection: `r(X, X)` is
//! `σ_{$1 = $2}(r)` read as `r'(X, X')` with `X'` functionally bound to
//! `X`. This pass performs that rewrite on the query text — every
//! second-and-later occurrence of a variable within one body atom becomes
//! a fresh body-only variable, and the atom is renamed to a synthetic
//! `Select_<n>_<base>` predicate carrying the column equalities. The
//! caller (`kermit::db::run_join`) backs each such predicate with an
//! equality-selection view over the base relation
//! ([`crate::sorted::EqualitySelectionTrieIter`] /
//! [`crate::hash::EqualitySelectionHashTrieIter`]), so neither executor
//! ever sees a repeated variable. Intended to run immediately after
//! [`crate::rewrite_atoms`] and before [`crate::JoinAlgo::join_iter`].

use {
    crate::const_rewrite::highest_k_index,
    kermit_parser::{JoinQuery, Term},
    std::collections::HashMap,
};

/// Prefix of the synthetic predicate names introduced by
/// [`rewrite_repeated_variables`] (e.g. `Select_0_r`). Load-bearing
/// beyond this module: `kermit::db::run_join` recognises selection views
/// by this prefix and resolves their statistics to the base relation.
pub const SELECTION_PREDICATE_PREFIX: &str = "Select_";

/// Returns `true` iff `name` names a synthetic selection-view predicate
/// introduced by [`rewrite_repeated_variables`].
pub fn is_selection_predicate(name: &str) -> bool { name.starts_with(SELECTION_PREDICATE_PREFIX) }

/// One column equality `σ_{col_source = col_repeat}` on a body atom.
///
/// Both fields are physical term positions within the atom (counting
/// placeholders and constants). `source < repeat` always holds, and
/// `source` is always the *first* occurrence of the variable in the atom,
/// so a trie descent has bound the source column before it reaches the
/// repeat column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColumnEquality {
    /// Column of the variable's first occurrence.
    pub source: usize,
    /// Column of a later occurrence, now carrying a fresh variable.
    pub repeat: usize,
}

/// A selection view the rewrite introduced: the synthetic predicate name
/// the rewritten query uses, the base relation it selects from, and the
/// equalities the view enforces.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectionSpec {
    /// Synthetic body-predicate name, e.g. `Select_0_r`.
    pub name: String,
    /// The relation the atom originally named, e.g. `r`.
    pub relation: String,
    /// The equalities to enforce, in column order of the repeat.
    pub equalities: Vec<ColumnEquality>,
}

/// Rewrites `query.body` so that no variable occurs twice within one atom.
///
/// For each body atom, every occurrence of a variable after its first
/// becomes a fresh variable `K<i>` and contributes one [`ColumnEquality`].
/// An atom that gained at least one equality is renamed to
/// `Select_<n>_<base>` (with `n` the index of its [`SelectionSpec`]) so
/// that `r(X, X), r(X, Y)` yields two distinct views of `r`.
///
/// Fresh variables share the `K<n>` counter with [`crate::rewrite_atoms`]
/// (both start past the highest existing `K<n>`), so the two rewrites
/// compose in either order. Run this one **second**: after the const
/// rewrite the body is atom-free, and the const rewrite's fresh variables
/// are distinct by construction, so this pass never mistakes one for a
/// repeat.
///
/// # Head asymmetry
///
/// As with [`crate::rewrite_atoms`], **only body atoms are rewritten**. A
/// head such as `Q(X, X)` describes the output shape and is left alone.
///
/// # Placeholders
///
/// `_` terms are neither sources nor repeats. Column positions are
/// physical, so `r(X, _, X)` yields the equality `(0, 2)`.
pub fn rewrite_repeated_variables(mut query: JoinQuery) -> (JoinQuery, Vec<SelectionSpec>) {
    let mut next_k = highest_k_index(&query).map_or(0, |n| n + 1);
    let mut specs: Vec<SelectionSpec> = Vec::new();

    for pred in &mut query.body {
        let mut first_occurrence: HashMap<String, usize> = HashMap::new();
        let mut equalities: Vec<ColumnEquality> = Vec::new();
        for (col, term) in pred.terms.iter_mut().enumerate() {
            let Term::Var(name) = term else {
                continue;
            };
            match first_occurrence.get(name.as_str()) {
                | None => {
                    first_occurrence.insert(name.clone(), col);
                },
                | Some(&source) => {
                    *term = Term::Var(format!("K{next_k}"));
                    next_k += 1;
                    equalities.push(ColumnEquality {
                        source,
                        repeat: col,
                    });
                },
            }
        }
        if equalities.is_empty() {
            continue;
        }
        let relation = std::mem::take(&mut pred.name);
        pred.name = format!("{SELECTION_PREDICATE_PREFIX}{}_{relation}", specs.len());
        specs.push(SelectionSpec {
            name: pred.name.clone(),
            relation,
            equalities,
        });
    }

    (query, specs)
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::rewrite_atoms,
        kermit_parser::Predicate,
    };

    fn parse(q: &str) -> JoinQuery { q.parse().unwrap() }

    fn var(t: &Term) -> &str {
        match t {
            | Term::Var(n) => n,
            | other => panic!("expected a variable, got {other:?}"),
        }
    }

    fn eq(source: usize, repeat: usize) -> ColumnEquality {
        ColumnEquality {
            source,
            repeat,
        }
    }

    #[test]
    fn no_repeats_is_identity() {
        let q = parse("Q(X, Y) :- p(X, Y), r(Y, Z).");
        let (out, specs) = rewrite_repeated_variables(q.clone());
        assert_eq!(out, q);
        assert!(specs.is_empty());
    }

    #[test]
    fn diagonal_binary_introduces_one_fresh_var_and_one_equality() {
        let (out, specs) = rewrite_repeated_variables(parse("Q(X) :- r(X, X)."));
        assert_eq!(out.body.len(), 1);
        assert_eq!(out.body[0].name, "Select_0_r");
        assert_eq!(var(&out.body[0].terms[0]), "X");
        assert_eq!(var(&out.body[0].terms[1]), "K0");
        assert_eq!(specs, vec![SelectionSpec {
            name: "Select_0_r".into(),
            relation: "r".into(),
            equalities: vec![eq(0, 1)],
        }]);
    }

    #[test]
    fn every_repeat_points_at_first_occurrence() {
        let (out, specs) = rewrite_repeated_variables(parse("Q(X) :- r(X, X, X)."));
        assert_eq!(var(&out.body[0].terms[1]), "K0");
        assert_eq!(var(&out.body[0].terms[2]), "K1");
        assert_eq!(specs[0].equalities, vec![eq(0, 1), eq(0, 2)]);
    }

    #[test]
    fn interleaved_repeats() {
        let (_, specs) = rewrite_repeated_variables(parse("Q(X, Y) :- r(X, Y, X, Y)."));
        assert_eq!(specs[0].equalities, vec![eq(0, 2), eq(1, 3)]);
    }

    #[test]
    fn repeats_across_predicates_are_joins_not_selections() {
        let q = parse("Q(X) :- p(X), r(X).");
        let (out, specs) = rewrite_repeated_variables(q.clone());
        assert_eq!(out, q);
        assert!(specs.is_empty());
    }

    #[test]
    fn only_the_repeating_occurrence_is_renamed() {
        let (out, specs) = rewrite_repeated_variables(parse("Q(X, Y) :- r(X, X), r(X, Y)."));
        assert_eq!(out.body[0].name, "Select_0_r");
        assert_eq!(out.body[1].name, "r");
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].relation, "r");
    }

    #[test]
    fn two_selected_atoms_get_distinct_names_and_fresh_vars() {
        let (out, specs) = rewrite_repeated_variables(parse("Q(X, Y) :- r(X, X), s(Y, Y)."));
        assert_eq!(out.body[0].name, "Select_0_r");
        assert_eq!(out.body[1].name, "Select_1_s");
        assert_eq!(var(&out.body[0].terms[1]), "K0");
        assert_eq!(var(&out.body[1].terms[1]), "K1");
        assert_eq!(specs.len(), 2);
    }

    #[test]
    fn fresh_vars_avoid_existing_k_names() {
        let (out, _) = rewrite_repeated_variables(parse("Q(K3) :- r(K3, K3)."));
        assert_eq!(var(&out.body[0].terms[1]), "K4");
    }

    #[test]
    fn head_repeats_are_not_rewritten() {
        let (out, specs) = rewrite_repeated_variables(parse("Q(X, X) :- r(X)."));
        assert_eq!(out.head.terms.len(), 2);
        assert_eq!(var(&out.head.terms[0]), "X");
        assert_eq!(var(&out.head.terms[1]), "X");
        assert!(specs.is_empty());
    }

    #[test]
    fn placeholders_are_ignored_but_count_as_columns() {
        let (out, specs) = rewrite_repeated_variables(parse("Q(X) :- r(X, _, X)."));
        assert!(matches!(out.body[0].terms[1], Term::Placeholder));
        assert_eq!(specs[0].equalities, vec![eq(0, 2)]);
    }

    #[test]
    fn composes_with_const_rewrite() {
        let (q, const_specs) = rewrite_atoms(parse("Q(X) :- r(X, c5, X).")).unwrap();
        let (out, specs) = rewrite_repeated_variables(q);
        assert_eq!(const_specs, vec![("Const_c5".into(), 5)]);
        // Const rewrite took K0; the selection's fresh variable is K1.
        assert_eq!(out.body[0].name, "Select_0_r");
        assert_eq!(var(&out.body[0].terms[1]), "K0");
        assert_eq!(var(&out.body[0].terms[2]), "K1");
        assert_eq!(specs[0].equalities, vec![eq(0, 2)]);
        // The synthetic const predicate is unary, so never selected.
        assert_eq!(out.body[1].name, "Const_c5");
    }

    #[test]
    fn atoms_are_neither_sources_nor_repeats() {
        // Hand-built: the parser would normally be preceded by rewrite_atoms.
        let q = JoinQuery {
            head: Predicate {
                name: "Q".into(),
                terms: vec![Term::Var("X".into())],
            },
            body: vec![Predicate {
                name: "r".into(),
                terms: vec![
                    Term::Atom("c1".into()),
                    Term::Var("X".into()),
                    Term::Atom("c1".into()),
                    Term::Var("X".into()),
                ],
            }],
        };
        let (out, specs) = rewrite_repeated_variables(q);
        assert!(matches!(out.body[0].terms[2], Term::Atom(ref s) if s == "c1"));
        assert_eq!(specs[0].equalities, vec![eq(1, 3)]);
    }
}
