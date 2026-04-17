//! Library interface for the Kermit CLI.
//!
//! Re-exports a curated subset of algorithm and data-structure types from
//! [`kermit_algos`] and [`kermit_ds`], plus the [`db`] module's database
//! abstraction (`DB` trait and `DatabaseEngine`) and the [`compute_join`]
//! helper for running joins from raw tuple inputs.

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
    kermit_algos::{JoinAlgo, JoinQuery},
    kermit_ds::Relation,
    std::collections::HashMap,
};

/// Convenience function that builds relations from raw tuple vectors and runs a
/// join, returning the result tuples.
///
/// Constructs a synthetic Datalog query from the `variables` and
/// `rel_variables` mappings, builds one `R` per input relation, and executes
/// the join via `JA`.
pub fn compute_join<R, JA>(
    input: Vec<Vec<Vec<usize>>>, variables: Vec<usize>, rel_variables: Vec<Vec<usize>>,
) -> Vec<Vec<usize>>
where
    R: Relation,
    JA: JoinAlgo<R>,
{
    let relations: Vec<_> = input
        .into_iter()
        .map(|tuples| {
            let k = if tuples.is_empty() {
                0
            } else {
                tuples[0].len()
            };
            R::from_tuples(k.into(), tuples)
        })
        .collect();

    // Build a synthetic query and datastructure map from the legacy inputs
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

    // Map datastructures by the synthetic names
    let mut ds_map: HashMap<String, &R> = HashMap::new();
    for (i, rel) in relations.iter().enumerate() {
        ds_map.insert(format!("R{}", i), rel);
    }

    JA::join_iter(query, ds_map).collect()
}
