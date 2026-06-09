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

    let register_var = |name: &str, map: &mut HashMap<String, usize>, next: &mut usize| {
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
                .filter_map(|(i, vars)| {
                    if vars.contains(v) {
                        Some(i)
                    } else {
                        None
                    }
                })
                .collect()
        })
        .collect()
}

/// Verify a candidate result tuple's join condition and construct the
/// output tuple in `variable_ordering` order. Returns `None` if any
/// variable mentioned by two or more predicates has inconsistent values
/// in the candidate (a hash false positive).
///
/// `candidate[k]` is one tuple from the k-th participating relation's
/// leaf chain. `predicate_variables[k]` lists the variable indices carried
/// by the k-th relation (in attribute order). `arity` is the number of
/// distinct variables in the query.
fn verify_and_construct(
    candidate: &[&Vec<usize>], predicate_variables: &[Vec<usize>], arity: usize,
) -> Option<Vec<usize>> {
    let mut result: Vec<Option<usize>> = vec![None; arity];
    for (rel_idx, tuple) in candidate.iter().enumerate() {
        for (col, &var_idx) in predicate_variables[rel_idx].iter().enumerate() {
            let v = tuple[col];
            match result[var_idx] {
                | None => result[var_idx] = Some(v),
                | Some(existing) if existing != v => return None,
                | _ => {},
            }
        }
    }
    Some(result.into_iter().map(Option::unwrap).collect())
}

/// Algorithm 3 from the paper. Recursively descends through attribute
/// positions; emits all verified result tuples into `output`.
///
/// **Contract.** `enumerate(i, ...)` is called with each iterator in
/// `variable_to_iter_map[i]` positioned at depth `i - 1` (or pre-root for
/// `i == 0`). The function descends each one level (to depth `i`), scans
/// at depth `i`, recurses on matches, then ascends back. This way the
/// caller's stack is unchanged on return.
fn enumerate<IT: HashTrieIterator>(
    i: usize, arity: usize, iters: &mut [IT], predicate_variables: &[Vec<usize>],
    variable_to_iter_map: &[Vec<usize>], output: &mut Vec<Vec<usize>>,
) {
    if i == arity {
        emit_leaf(iters, predicate_variables, arity, output);
        return;
    }

    let i_join = &variable_to_iter_map[i];
    if i_join.is_empty() {
        return; // no relation carries this variable
    }

    // Descend every participating iterator to depth i. Track how many we
    // successfully opened so we can match `up` calls on early return.
    let mut opened = 0;
    let mut descend_ok = true;
    for &idx in i_join {
        if iters[idx].open() {
            opened += 1;
        } else {
            descend_ok = false;
            break;
        }
    }

    if descend_ok {
        let i_scan = *i_join
            .iter()
            .min_by_key(|&&idx| iters[idx].size())
            .expect("i_join non-empty");

        while !iters[i_scan].at_end() {
            let h = iters[i_scan]
                .key()
                .expect("scan iterator not at end => key Some");

            // Probe the other iterators in i_join for this hash.
            let mut all_match = true;
            for &idx in i_join {
                if idx == i_scan {
                    continue;
                }
                if !iters[idx].lookup(h) {
                    all_match = false;
                    break;
                }
            }

            if all_match {
                enumerate(
                    i + 1,
                    arity,
                    iters,
                    predicate_variables,
                    variable_to_iter_map,
                    output,
                );
            }

            iters[i_scan].next();
        }
    }

    // Ascend back to the parent depth, matching the descend count so
    // partial-open failures are symmetric.
    for &idx in &i_join[..opened] {
        let _ = iters[idx].up();
    }
}

/// Algorithm 3 lines 16–19. Cross-product the leaf chains of every
/// iterator and emit each verified candidate.
fn emit_leaf<IT: HashTrieIterator>(
    iters: &[IT], predicate_variables: &[Vec<usize>], arity: usize, output: &mut Vec<Vec<usize>>,
) {
    let chains: Vec<&[Vec<usize>]> = iters
        .iter()
        .map(|it| {
            it.leaf_tuples()
                .expect("at leaf level for every participating iter")
        })
        .collect();
    if chains.iter().any(|c| c.is_empty()) {
        return;
    }

    let mut cursor: Vec<usize> = vec![0; chains.len()];
    loop {
        let candidate: Vec<&Vec<usize>> = chains.iter().zip(&cursor).map(|(c, &i)| &c[i]).collect();
        if let Some(result) = verify_and_construct(&candidate, predicate_variables, arity) {
            output.push(result);
        }
        if !advance_cursor(&mut cursor, &chains) {
            break;
        }
    }
}

/// Advance a mixed-base cursor over the chain lengths. Returns `false`
/// once every position has been exhausted (overflow off the high end).
fn advance_cursor(cursor: &mut [usize], chains: &[&[Vec<usize>]]) -> bool {
    let mut k = 0;
    loop {
        if k >= cursor.len() {
            return false;
        }
        cursor[k] += 1;
        if cursor[k] < chains[k].len() {
            return true;
        }
        cursor[k] = 0;
        k += 1;
    }
}

/// Entry point for the hash-trie-join algorithm.
///
/// Implements [`JoinAlgo`] for any [`HashTrieIterable`] data structure.
/// See the module docs for the algorithm overview.
pub struct HashTriejoin {}

impl<DS> JoinAlgo<DS> for HashTriejoin
where
    DS: HashTrieIterable,
{
    fn join_iter(
        query: JoinQuery, datastructures: HashMap<String, &DS>,
    ) -> impl Iterator<Item = Vec<usize>> {
        let (variable_ordering, predicate_variables) = build_variable_index(&query);
        let mut iters: Vec<_> = query
            .body
            .iter()
            .map(|pred| {
                datastructures
                    .get(&pred.name)
                    .expect("Missing datastructure for predicate name")
                    .hash_trie_iter()
            })
            .collect();
        let variable_to_iter_map =
            build_variable_to_iter_map(&variable_ordering, &predicate_variables);
        let mut output = Vec::new();
        enumerate(
            0,
            variable_ordering.len(),
            &mut iters,
            &predicate_variables,
            &variable_to_iter_map,
            &mut output,
        );
        output.into_iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variable_index_triangle() {
        let query: JoinQuery = "Q(X, Y, Z) :- R(X, Y), S(Y, Z), T(X, Z).".parse().unwrap();
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

    #[test]
    fn verify_constructs_when_shared_var_agrees() {
        // R(X, Y), S(Y, Z) with X=1, Y=2, Z=3.
        // predicate_variables = [[0, 1], [1, 2]]
        // arity = 3
        let r_tuple = vec![1, 2];
        let s_tuple = vec![2, 3];
        let candidate: Vec<&Vec<usize>> = vec![&r_tuple, &s_tuple];
        let pv = vec![vec![0, 1], vec![1, 2]];
        assert_eq!(
            verify_and_construct(&candidate, &pv, 3),
            Some(vec![1, 2, 3])
        );
    }

    #[test]
    fn verify_rejects_when_shared_var_disagrees() {
        // R(X, Y), S(Y, Z) but Y differs in R (=2) vs S (=99)
        let r_tuple = vec![1, 2];
        let s_tuple = vec![99, 3];
        let candidate: Vec<&Vec<usize>> = vec![&r_tuple, &s_tuple];
        let pv = vec![vec![0, 1], vec![1, 2]];
        assert_eq!(verify_and_construct(&candidate, &pv, 3), None);
    }

    #[test]
    fn verify_passes_no_shared_var() {
        // Two disjoint unary predicates R(X), S(Y).
        let r_tuple = vec![1];
        let s_tuple = vec![2];
        let candidate: Vec<&Vec<usize>> = vec![&r_tuple, &s_tuple];
        let pv = vec![vec![0], vec![1]];
        assert_eq!(verify_and_construct(&candidate, &pv, 2), Some(vec![1, 2]));
    }

    #[test]
    fn enumerate_unary_intersection() {
        use kermit_ds::{HashTrie, Relation};
        // Explicit `HashTrie` annotation pins the default `H = SipHashStrategy`
        // since the local bindings escape into `Vec<_>` iter values that
        // would otherwise leave `H` ambiguous.
        let r: HashTrie = HashTrie::from_tuples(1.into(), vec![vec![1], vec![2], vec![3]]);
        let s: HashTrie = HashTrie::from_tuples(1.into(), vec![vec![2], vec![3], vec![4]]);
        let mut iters = vec![r.hash_trie_iter(), s.hash_trie_iter()];
        // Inline the same setup the JoinAlgo entry point does.
        let predicate_variables = vec![vec![0], vec![0]];
        let variable_to_iter_map = vec![vec![0, 1]];
        let mut output = Vec::new();
        enumerate(
            0,
            1,
            &mut iters,
            &predicate_variables,
            &variable_to_iter_map,
            &mut output,
        );
        output.sort();
        assert_eq!(output, vec![vec![2], vec![3]]);
    }

    #[test]
    fn join_algo_unary_intersection() {
        use kermit_ds::{HashTrie, Relation};
        let r = HashTrie::from_tuples(1.into(), vec![vec![1], vec![2], vec![3]]);
        let s = HashTrie::from_tuples(1.into(), vec![vec![2], vec![3], vec![4]]);
        let query: JoinQuery = "Q(X) :- R(X), S(X).".parse().unwrap();
        let mut ds: HashMap<String, &HashTrie> = HashMap::new();
        ds.insert("R".to_string(), &r);
        ds.insert("S".to_string(), &s);
        let mut out: Vec<Vec<usize>> = HashTriejoin::join_iter(query, ds).collect();
        out.sort();
        assert_eq!(out, vec![vec![2], vec![3]]);
    }

    #[test]
    fn join_algo_triangle() {
        use kermit_ds::{HashTrie, Relation};
        let r = HashTrie::from_tuples(2.into(), vec![vec![1, 2], vec![2, 3], vec![3, 1]]);
        let s = HashTrie::from_tuples(2.into(), vec![vec![2, 3], vec![3, 1], vec![1, 2]]);
        let t = HashTrie::from_tuples(2.into(), vec![vec![1, 3], vec![2, 1], vec![3, 2]]);
        let query: JoinQuery = "Q(X, Y, Z) :- R(X, Y), S(Y, Z), T(X, Z).".parse().unwrap();
        let mut ds: HashMap<String, &HashTrie> = HashMap::new();
        ds.insert("R".to_string(), &r);
        ds.insert("S".to_string(), &s);
        ds.insert("T".to_string(), &t);
        let mut out: Vec<Vec<usize>> = HashTriejoin::join_iter(query, ds).collect();
        out.sort();
        assert_eq!(out, vec![vec![1, 2, 3], vec![2, 3, 1], vec![3, 1, 2]]);
    }
}
