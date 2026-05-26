//! Hash Trie Join — worst-case-optimal multi-way join over hash tries.
//!
//! Implements the probe phase from §3.2.3 of the SIGMOD 2020 paper
//! "Combining Worst-Case Optimal and Traditional Binary Join Processing".
//! Coordinates one [`HashTrieIterator`] per body predicate, descending in
//! lockstep variable by variable. At each depth it picks the iterator with
//! the smallest hash table (`argmin size`), iterates that table's hashes,
//! and probes the rest via [`HashTrieIterator::lookup`]. At the leaf level
//! it cross-products the participating iterators' tuple chains and
//! verifies the actual join condition (hash collisions can produce false
//! positives at any inner level).

use {
    crate::join_algo::JoinAlgo,
    kermit_iters::{HashTrieIterable, HashTrieIterator},
    kermit_parser::{JoinQuery, Term},
    std::collections::HashMap,
};

/// Indexes the variables in a query for the hash-trie-join algorithm.
///
/// Duplicate of [`crate::leapfrog_triejoin::build_variable_index`] (per
/// CLAUDE.md scope discipline — don't touch LFTJ during this work).
/// Refactor to a shared helper is a separate follow-up.
fn build_variable_index(query: &JoinQuery) -> (Vec<usize>, Vec<Vec<usize>>) {
    let mut var_to_index: HashMap<String, usize> = HashMap::new();
    let mut next_index: usize = 0;

    let register_var =
        |name: &str, map: &mut HashMap<String, usize>, next: &mut usize| {
            *map.entry(name.to_string()).or_insert_with(|| {
                let idx = *next;
                *next += 1;
                idx
            })
        };

    for t in &query.head.terms {
        if let Term::Var(ref vname) = t {
            let _ = register_var(vname, &mut var_to_index, &mut next_index);
        }
    }
    for pred in &query.body {
        for t in &pred.terms {
            if let Term::Var(ref vname) = t {
                let _ = register_var(vname, &mut var_to_index, &mut next_index);
            }
        }
    }

    let variable_ordering: Vec<usize> = (0..var_to_index.len()).collect();

    let mut predicate_variables: Vec<Vec<usize>> = Vec::with_capacity(query.body.len());
    for pred in &query.body {
        let mut vars_for_pred: Vec<usize> = Vec::new();
        for t in &pred.terms {
            if let Term::Var(ref vname) = t {
                if let Some(idx) = var_to_index.get(vname) {
                    vars_for_pred.push(*idx);
                }
            }
        }
        predicate_variables.push(vars_for_pred);
    }

    (variable_ordering, predicate_variables)
}

/// Build `variable_to_iter_map[i] = predicate indices that carry the i-th
/// variable in `variable_ordering`. Identical shape to LFTJ's inline
/// construction.
fn build_variable_to_iter_map(
    variable_ordering: &[usize], predicate_variables: &[Vec<usize>],
) -> Vec<Vec<usize>> {
    variable_ordering
        .iter()
        .map(|v| {
            predicate_variables
                .iter()
                .enumerate()
                .filter_map(|(i, vars)| if vars.contains(v) { Some(i) } else { None })
                .collect()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variable_index_triangle() {
        let query: JoinQuery = "Q(X, Y, Z) :- R(X, Y), S(Y, Z), T(X, Z)."
            .parse()
            .unwrap();
        let (ordering, predicate_vars) = build_variable_index(&query);
        assert_eq!(ordering, vec![0, 1, 2]);
        assert_eq!(predicate_vars, vec![vec![0, 1], vec![1, 2], vec![0, 2]]);
    }

    #[test]
    fn variable_to_iter_map_triangle() {
        let pred_vars = vec![vec![0, 1], vec![1, 2], vec![0, 2]];
        let map = build_variable_to_iter_map(&[0, 1, 2], &pred_vars);
        assert_eq!(map, vec![vec![0, 2], vec![0, 1], vec![1, 2]]);
    }
}
