//! Library interface for the Kermit CLI.
//!
//! Re-exports a curated subset of algorithm and data-structure types from
//! [`kermit_algos`] and [`kermit_ds`], plus the [`db`] module's join entry
//! points (`lftj_join` and `hash_join`) and the [`compute_join`] helper for
//! running joins from raw tuple inputs.

#![deny(missing_docs)]

/// Re-exports of join algorithms from [`kermit_algos`].
pub mod algos {
    pub use kermit_algos::LeapfrogTriejoin;
}

/// Re-exports of relation data structures from [`kermit_ds`].
pub mod ds {
    pub use kermit_ds::{RelationFileExt, TreeTrie};
}

pub mod db;

use {
    crate::db::Catalog,
    kermit_algos::{JoinAlgo, JoinQuery, LexicographicOptimiser, Planner},
    kermit_ds::{Relation, RelationHeader, Tuples},
};

/// Convenience function that builds relations from raw tuple batches and runs a
/// join, returning the result tuples.
///
/// Constructs a synthetic Datalog query from the `variables` and
/// `rel_variables` mappings (variable `v` is written `V{v}`, and input `i` is
/// the relation `R{i}`), loads the batches into a [`db::Catalog`], and runs
/// [`db::prepare`], [`db::build`] and [`db::execute`] through the one body
/// [`db::lftj_join`] and [`db::hash_join`] share, with `R` as the index
/// structure and `JA` as the algorithm. The query is therefore validated first,
/// a variable repeated within one `rel_variables` entry selects the rows whose
/// columns agree, and each result row holds the `variables`, in that order:
/// a variable only the body binds is not part of it.
///
/// An empty batch carries no arity (an empty `Vec` converts to arity 0), so
/// a relation with no rows takes its arity from its atom. An empty
/// `rel_variables` entry stands for the unary atom `R{i}(_)`.
///
/// # Panics
///
/// If the synthetic query cannot run over the inputs: a head variable no
/// relation binds, a repeated head variable, or a batch whose arity is not
/// the length of its `rel_variables` entry. The message is the
/// [`db::JoinError`]'s. Also if `rel_variables` has more entries than
/// `input` has batches: the entry's relation `R{i}` was never loaded, so its
/// atom is an unknown relation.
pub fn compute_join<R, JA>(
    input: Vec<impl Into<Tuples>>, variables: Vec<usize>, rel_variables: Vec<Vec<usize>>,
) -> Vec<Vec<usize>>
where
    R: Relation,
    JA: JoinAlgo<R>,
{
    // Build a synthetic query from the legacy inputs
    // Head: Q(V0, V1, ...)
    let head_vars: Vec<String> = variables.iter().map(|v| format!("V{}", v)).collect();
    // Body: R{i}(V...)
    let mut body_preds: Vec<String> = Vec::new();
    for (i, rv) in rel_variables.iter().enumerate() {
        let var_list = if rv.is_empty() {
            "_".to_string()
        } else {
            rv.iter()
                .map(|v| format!("V{}", v))
                .collect::<Vec<_>>()
                .join(", ")
        };
        body_preds.push(format!("R{}({})", i, var_list));
    }
    let query_str = format!("Q({}) :- {}.", head_vars.join(", "), body_preds.join(", "));
    let query: JoinQuery = query_str.parse().expect("Failed to build JoinQuery");

    // One catalog relation per batch, named as the query's atoms name them.
    // A batch with no rows carries no arity (an empty `Vec` converts to arity
    // 0), so it takes its atom's: one column per term, or one for `_`.
    let relations: Vec<(RelationHeader, Tuples)> = input
        .into_iter()
        .enumerate()
        .map(|(i, tuples)| {
            let tuples: Tuples = tuples.into();
            let tuples = match rel_variables.get(i) {
                | Some(terms) if tuples.is_empty() => Tuples::new(terms.len().max(1)),
                | _ => tuples,
            };
            (
                RelationHeader::new_positional(format!("R{i}"), tuples.arity()),
                tuples,
            )
        })
        .collect();

    // Synthetic queries carry no constants and this helper predates
    // statistics, so plan with the stats-free default optimiser.
    let planner = Planner::new(Box::new(LexicographicOptimiser));
    let catalog = Catalog::new(relations, planner.required_statistics());
    let mut rows = Vec::new();
    db::run_join::<R, JA>(&catalog, &query, &planner, |row| rows.push(row.to_vec()))
        .unwrap_or_else(|error| panic!("compute_join: {error}"));
    rows
}

#[cfg(test)]
mod compute_join_tests {
    use {
        super::*,
        kermit_algos::{HashTriejoin, LeapfrogTriejoin},
        kermit_ds::{HashTrie, TreeTrie},
    };

    type Rows = Vec<Vec<usize>>;

    /// `compute_join` under both families; they must agree. Returns the rows
    /// sorted.
    fn joined(input: Vec<Rows>, variables: Vec<usize>, rel_variables: Vec<Vec<usize>>) -> Rows {
        let mut sorted = compute_join::<TreeTrie, LeapfrogTriejoin>(
            input.clone(),
            variables.clone(),
            rel_variables.clone(),
        );
        let mut hashed = compute_join::<HashTrie, HashTriejoin>(input, variables, rel_variables);
        sorted.sort();
        hashed.sort();
        assert_eq!(sorted, hashed);
        sorted
    }

    fn chain_input() -> Vec<Rows> {
        vec![vec![vec![1, 2], vec![1, 3], vec![2, 3]], vec![
            vec![2, 7],
            vec![3, 8],
            vec![4, 9],
        ]]
    }

    #[test]
    fn a_chain_joins_on_both_families() {
        let rows = joined(chain_input(), vec![0, 1, 2], vec![vec![0, 1], vec![1, 2]]);
        assert_eq!(rows, vec![vec![1, 2, 7], vec![1, 3, 8], vec![2, 3, 8]]);
    }

    /// `variables` is the head: a variable only the body binds (`V1` here) is
    /// not in the rows. The helper used to return it after the head's columns.
    #[test]
    fn the_variables_are_the_output_columns() {
        let rows = joined(chain_input(), vec![0, 2], vec![vec![0, 1], vec![1, 2]]);
        assert_eq!(rows, vec![vec![1, 7], vec![1, 8], vec![2, 8]]);
    }

    /// A variable repeated within one entry is a selection. The helper used
    /// to skip the rewrite that handles it, so the entry had to avoid it.
    #[test]
    fn a_repeated_variable_selects_the_diagonal() {
        let input = vec![vec![vec![1, 1], vec![1, 2], vec![3, 3]]];
        let rows = joined(input, vec![0], vec![vec![0, 0]]);
        assert_eq!(rows, vec![vec![1], vec![3]]);
    }

    /// An empty batch converts to arity 0, but its atom has two terms: the
    /// relation takes its arity from the atom, and the join is empty.
    #[test]
    fn an_empty_relation_takes_its_arity_from_its_atom() {
        let input = vec![Vec::new(), vec![vec![2, 3]]];
        let rows = joined(input, vec![0, 1, 2], vec![vec![0, 1], vec![1, 2]]);
        assert!(rows.is_empty());
    }

    /// The query is validated now, so a head variable no relation binds is an
    /// error carrying the validation message, not an executor panic.
    #[test]
    #[should_panic(expected = "head variable `V5` does not appear in the body")]
    fn an_unbound_head_variable_panics_with_the_error() {
        let input: Vec<Rows> = vec![vec![vec![1, 2]]];
        let _ = compute_join::<TreeTrie, LeapfrogTriejoin>(input, vec![0, 5], vec![vec![0, 1]]);
    }

    /// An empty variable entry is the unary atom `R{i}(_)`, and its empty
    /// batch takes arity 1 for it (`.max(1)`), not the arity 0 an empty
    /// `Vec` converts to, which the atom would reject. The join is empty.
    #[test]
    fn an_empty_batch_with_an_empty_entry_is_a_unary_relation() {
        let input = vec![Vec::new(), vec![vec![2, 3]]];
        let rows = joined(input, vec![0, 1], vec![vec![], vec![0, 1]]);
        assert!(rows.is_empty());
    }

    /// An entry with no batch names a relation that was never loaded.
    #[test]
    #[should_panic(expected = "unknown relation \"R1\"")]
    fn an_entry_without_a_batch_panics_as_an_unknown_relation() {
        let input: Vec<Rows> = vec![vec![vec![1, 2]]];
        let _ = compute_join::<TreeTrie, LeapfrogTriejoin>(input, vec![0, 1], vec![
            vec![0, 1],
            vec![1],
        ]);
    }

    #[test]
    #[should_panic(expected = "has arity 3")]
    fn an_input_whose_arity_disagrees_with_its_atom_panics() {
        let input: Vec<Rows> = vec![vec![vec![1, 2, 3]]];
        let _ = compute_join::<HashTrie, HashTriejoin>(input, vec![0, 1], vec![vec![0, 1]]);
    }
}
