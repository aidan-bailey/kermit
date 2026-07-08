//! Constraint-respecting topological ordering shared by all optimisers.

use std::{
    cmp::Reverse,
    collections::{BinaryHeap, HashSet},
};

/// Computes a *global attribute order* (GAO): a permutation of
/// `0..num_vars` in which every relation's variables appear in physical
/// column order.
///
/// Trie-descending joins bind each relation one physical column per depth,
/// so a relation `r(K, Y)` participates correctly only if its first column
/// `K` is bound before its second column `Y`. Each relation contributes
/// edges `col[i] -> col[i+1]` (a variable repeated within one predicate
/// imposes no self-constraint); Kahn's algorithm yields a valid order.
///
/// `rank` maps a candidate variable to an [`Ord`] key; among the variables
/// whose constraints are currently satisfied (the *ready set*), the
/// smallest key is emitted next, with the canonical index as the final
/// deterministic tie-break. Because candidates are restricted to the ready
/// set, **any** rank function yields a valid order — an ordering policy
/// can be slow, never wrong. `rank` is evaluated once per variable, as it
/// becomes ready — cheap enough for expensive statistics-driven policies.
///
/// # Panics
///
/// Panics if the constraints are cyclic (e.g. `r(X, Y), s(Y, X)`):
/// answering such a query would require a relation sorted in two different
/// column orders at once, which a single fixed trie order cannot provide.
pub fn topological_order<K: Ord>(
    num_vars: usize, predicate_variables: &[Vec<usize>], rank: impl Fn(usize) -> K,
) -> Vec<usize> {
    let mut adjacency: Vec<HashSet<usize>> = vec![HashSet::new(); num_vars];
    let mut in_degree: Vec<usize> = vec![0; num_vars];

    for vars in predicate_variables {
        for pair in vars.windows(2) {
            let (earlier, later) = (pair[0], pair[1]);
            if earlier != later && adjacency[earlier].insert(later) {
                in_degree[later] += 1;
            }
        }
    }

    let mut ready: BinaryHeap<Reverse<(K, usize)>> = (0..num_vars)
        .filter(|&v| in_degree[v] == 0)
        .map(|v| Reverse((rank(v), v)))
        .collect();
    let mut order = Vec::with_capacity(num_vars);
    while let Some(Reverse((_, v))) = ready.pop() {
        order.push(v);
        for &w in &adjacency[v] {
            in_degree[w] -= 1;
            if in_degree[w] == 0 {
                ready.push(Reverse((rank(w), w)));
            }
        }
    }

    assert_eq!(
        order.len(),
        num_vars,
        "query imposes a cyclic global attribute order; the join cannot answer it with a single \
         trie column order per relation"
    );
    order
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_rank_reproduces_lexicographic_order() {
        // Triangle: R(0,1), S(1,2), T(0,2) — edges 0->1, 1->2, 0->2.
        let preds = vec![vec![0, 1], vec![1, 2], vec![0, 2]];
        assert_eq!(topological_order(3, &preds, |v| v), vec![0, 1, 2]);
    }

    #[test]
    fn constraint_forces_late_canonical_variable_first() {
        // r(K, Y) with K canonical index 1, Y index 0: edge 1 -> 0.
        // The subject-position-constant shape — K must come first.
        let preds = vec![vec![1, 0], vec![1]];
        assert_eq!(topological_order(2, &preds, |v| v), vec![1, 0]);
    }

    #[test]
    fn rank_breaks_ties_among_ready_variables() {
        // Star: R(0,1), S(0,2). After 0, both 1 and 2 are ready; the rank
        // function prefers 2.
        let preds = vec![vec![0, 1], vec![0, 2]];
        let order = topological_order(3, &preds, |v| {
            if v == 2 {
                0
            } else {
                v + 1
            }
        });
        assert_eq!(order, vec![0, 2, 1]);
    }

    #[test]
    #[should_panic(expected = "cyclic global attribute order")]
    fn cyclic_constraints_panic() {
        // r(X, Y), s(Y, X): edges 0->1 and 1->0.
        let preds = vec![vec![0, 1], vec![1, 0]];
        topological_order(2, &preds, |v| v);
    }
}
