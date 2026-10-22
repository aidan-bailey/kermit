//! Variable ordering shared by all optimisers.

use std::{
    cmp::Reverse,
    collections::{BinaryHeap, HashSet},
};

/// Computes a *global attribute order* (GAO): a permutation of the query's
/// variables in which every edge of `precedence` points forward.
///
/// A query builds each of its indexes with the columns in plan order, so no
/// relation's stored column order constrains the plan. [`Precedence::free`],
/// the graph every optimiser plans under, has no edge, and every
/// permutation is valid. Kahn's algorithm yields a valid order for any
/// acyclic graph.
///
/// `rank` maps a candidate variable to an [`Ord`] key; among the variables
/// whose predecessors are all emitted (the *ready set*), the smallest key is
/// emitted next, with the canonical index as the final deterministic
/// tie-break. Because candidates are restricted to the ready set, **any**
/// rank function yields a valid order — an ordering policy can be slow,
/// never wrong. `rank` is evaluated once per variable, as it becomes ready
/// — cheap enough for expensive statistics-driven policies.
///
/// # Panics
///
/// Panics if the edges are cyclic, which no graph the optimisers build is.
pub fn topological_order<K: Ord>(precedence: &Precedence, rank: impl Fn(usize) -> K) -> Vec<usize> {
    let order = precedence.kahn(rank);
    assert_eq!(
        order.len(),
        precedence.num_vars(),
        "precedence edges are cyclic; no variable order satisfies them"
    );
    order
}

/// The ordering-constraint graph: an edge `u -> v` requires `u` bound before
/// `v`. Every optimiser plans under [`Precedence::free`], which has none.
/// The edge machinery stays because [`topological_order`] orders by it and
/// the cost-based search reads its
/// [`predecessor_masks`](Self::predecessor_masks), so neither can disagree
/// about which plans are valid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Precedence {
    /// `adjacency[v]`: the variables that must be bound after `v`.
    adjacency: Vec<HashSet<usize>>,
    /// Number of distinct variables that must be bound before each one.
    in_degree: Vec<usize>,
}

impl Precedence {
    /// The graph over `num_vars` variables with no edge: every order is
    /// valid, and the ranking alone decides.
    pub fn free(num_vars: usize) -> Self {
        Self {
            adjacency: vec![HashSet::new(); num_vars],
            in_degree: vec![0; num_vars],
        }
    }

    /// The graph with one edge `col[i] -> col[i+1]` per adjacent pair of
    /// each atom's variables, a variable repeated within one atom imposing
    /// no self-constraint. The shape a stored column order used to impose;
    /// kept for the tests that exercise Kahn's algorithm with edges.
    #[cfg(test)]
    fn with_edges(num_vars: usize, predicate_variables: &[Vec<usize>]) -> Self {
        let mut precedence = Self::free(num_vars);
        for vars in predicate_variables {
            for pair in vars.windows(2) {
                let (earlier, later) = (pair[0], pair[1]);
                if earlier != later && precedence.adjacency[earlier].insert(later) {
                    precedence.in_degree[later] += 1;
                }
            }
        }
        precedence
    }

    /// The number of variables the graph orders.
    pub fn num_vars(&self) -> usize { self.in_degree.len() }

    /// Whether the graph has no edge at all, so every order is valid.
    pub fn is_free(&self) -> bool { self.in_degree.iter().all(|&d| d == 0) }

    /// The variables each variable must follow, as one bitmask per variable:
    /// bit `u` of entry `v` is set when `u -> v` is an edge. A set of bound
    /// variables `S` can bind `v` next exactly when `masks[v] & !S == 0`.
    /// `None` past 64 variables, which a `u64` cannot index.
    pub fn predecessor_masks(&self) -> Option<Vec<u64>> {
        let num_vars = self.num_vars();
        if num_vars > 64 {
            return None;
        }
        let mut masks = vec![0u64; num_vars];
        for (earlier, laters) in self.adjacency.iter().enumerate() {
            for &later in laters {
                masks[later] |= 1u64 << earlier;
            }
        }
        Some(masks)
    }

    /// Kahn's algorithm, emitting the smallest-`rank` ready variable first.
    /// The result is shorter than `num_vars` exactly when the constraints
    /// are cyclic: the variables on and after a cycle never become ready.
    fn kahn<K: Ord>(&self, rank: impl Fn(usize) -> K) -> Vec<usize> {
        let num_vars = self.in_degree.len();
        let mut in_degree = self.in_degree.clone();
        let mut ready: BinaryHeap<Reverse<(K, usize)>> = (0..num_vars)
            .filter(|&v| in_degree[v] == 0)
            .map(|v| Reverse((rank(v), v)))
            .collect();
        let mut order = Vec::with_capacity(num_vars);
        while let Some(Reverse((_, v))) = ready.pop() {
            order.push(v);
            for &w in &self.adjacency[v] {
                in_degree[w] -= 1;
                if in_degree[w] == 0 {
                    ready.push(Reverse((rank(w), w)));
                }
            }
        }
        order
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_free_graph_orders_by_rank_with_the_canonical_index_as_tie_break() {
        let free = Precedence::free(3);
        assert!(free.is_free());
        assert_eq!(free.num_vars(), 3);
        assert_eq!(topological_order(&free, |v| v), vec![0, 1, 2]);
        // The rank alone decides.
        assert_eq!(topological_order(&free, |v| 2 - v), vec![2, 1, 0]);
        // Equal ranks fall back to the canonical index.
        assert_eq!(topological_order(&free, |_| 0), vec![0, 1, 2]);
    }

    #[test]
    fn a_free_graph_has_no_predecessors() {
        assert_eq!(Precedence::free(3).predecessor_masks().unwrap(), vec![
            0, 0, 0
        ]);
        assert_eq!(
            topological_order(&Precedence::free(0), |v| v),
            Vec::<usize>::new()
        );
    }

    #[test]
    fn identity_rank_reproduces_lexicographic_order() {
        // Triangle: R(0,1), S(1,2), T(0,2) — edges 0->1, 1->2, 0->2.
        let preds = vec![vec![0, 1], vec![1, 2], vec![0, 2]];
        let graph = Precedence::with_edges(3, &preds);
        assert!(!graph.is_free());
        assert_eq!(topological_order(&graph, |v| v), vec![0, 1, 2]);
    }

    #[test]
    fn an_edge_forces_a_late_canonical_variable_first() {
        // r(K, Y) with K canonical index 1, Y index 0: edge 1 -> 0.
        let preds = vec![vec![1, 0], vec![1]];
        assert_eq!(
            topological_order(&Precedence::with_edges(2, &preds), |v| v),
            vec![1, 0]
        );
    }

    #[test]
    fn rank_breaks_ties_among_ready_variables() {
        // Star: R(0,1), S(0,2). After 0, both 1 and 2 are ready; the rank
        // function prefers 2.
        let preds = vec![vec![0, 1], vec![0, 2]];
        let order = topological_order(&Precedence::with_edges(3, &preds), |v| {
            if v == 2 {
                0
            } else {
                v + 1
            }
        });
        assert_eq!(order, vec![0, 2, 1]);
    }

    #[test]
    fn predecessor_masks_mirror_the_edges() {
        // Triangle: R(0,1), S(1,2), T(0,2) — edges 0->1, 1->2, 0->2.
        let preds = vec![vec![0, 1], vec![1, 2], vec![0, 2]];
        let masks = Precedence::with_edges(3, &preds)
            .predecessor_masks()
            .unwrap();
        assert_eq!(masks, vec![0b000, 0b001, 0b011]);
    }

    #[test]
    fn predecessor_masks_stop_at_64_variables() {
        assert!(Precedence::free(65).predecessor_masks().is_none());
        let masks = Precedence::free(64).predecessor_masks().unwrap();
        assert_eq!(masks.len(), 64);
    }

    #[test]
    #[should_panic(expected = "precedence edges are cyclic")]
    fn cyclic_edges_panic() {
        // r(X, Y), s(Y, X): edges 0->1 and 1->0.
        let preds = vec![vec![0, 1], vec![1, 0]];
        topological_order(&Precedence::with_edges(2, &preds), |v| v);
    }
}
