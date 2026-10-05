//! Constraint-respecting topological ordering shared by all optimisers.

use {
    crate::{analyse, optimiser::CatalogStats},
    kermit_parser::JoinQuery,
    std::{
        cmp::Reverse,
        collections::{BinaryHeap, HashSet},
    },
};

/// Computes a *global attribute order* (GAO): a permutation of the
/// query's variables in which every *pinned* relation's variables appear
/// in physical column order.
///
/// Trie-descending joins bind each relation one physical column per depth,
/// so a relation `r(K, Y)` read in its stored order participates correctly
/// only if its first column `K` is bound before its second column `Y`.
/// `precedence` holds those edges ([`Precedence::for_query`]: one
/// `col[i] -> col[i+1]` edge per adjacent pair of each pinned atom's
/// variables; a variable repeated within one predicate imposes no
/// self-constraint; under `--column-orders any` no atom is pinned and the
/// graph is empty). Kahn's algorithm yields a valid order.
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
/// Panics if the constraints are cyclic (e.g. `r(X, Y), s(Y, X)` under
/// `stored`): answering such a query would require a relation sorted in
/// two different column orders at once, which a single fixed trie order
/// cannot provide. `kermit::db` rejects such a query before planning
/// ([`check_attribute_order`]), and under `any` the graph has no cycle.
pub fn topological_order<K: Ord>(precedence: &Precedence, rank: impl Fn(usize) -> K) -> Vec<usize> {
    let order = precedence.kahn(rank);
    assert_eq!(
        order.len(),
        precedence.num_vars(),
        "query imposes a cyclic global attribute order; the join cannot answer it with a single \
         trie column order per relation"
    );
    order
}

/// The column-order constraint graph: one edge `col[i] -> col[i+1]` per
/// adjacent pair of a pinned relation's variables. The one place those
/// edges are built, shared by [`topological_order`], which orders it,
/// [`check_attribute_order`], which reports a cycle in it, and the
/// cost-based search, which reads its
/// [`predecessor_masks`](Self::predecessor_masks), so none of them can
/// disagree about which plans are valid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Precedence {
    /// `adjacency[v]`: the variables that must be bound after `v`.
    adjacency: Vec<HashSet<usize>>,
    /// Number of distinct variables that must be bound before each one.
    in_degree: Vec<usize>,
}

impl Precedence {
    /// The graph of `query` under the policy `stats` carry: only atoms
    /// [`CatalogStats::is_pinned`] says are pinned add edges. `query` is
    /// the rewritten query, as
    /// [`QueryOptimiser::plan`](super::QueryOptimiser::plan) receives it.
    pub fn for_query(query: &JoinQuery, stats: &CatalogStats) -> Self {
        let analysis = analyse(query);
        let pinned = query
            .body
            .iter()
            .zip(&analysis.predicate_variables)
            .filter(|(atom, _)| stats.is_pinned(&atom.name))
            .map(|(_, vars)| vars);
        Self::build(analysis.num_vars, pinned)
    }

    /// The graph with every atom pinned: what `stored` plans under, and
    /// what [`check_attribute_order`] reports cycles in.
    pub(crate) fn new(num_vars: usize, predicate_variables: &[Vec<usize>]) -> Self {
        Self::build(num_vars, predicate_variables)
    }

    fn build<'v>(num_vars: usize, atoms: impl IntoIterator<Item = &'v Vec<usize>>) -> Self {
        let mut adjacency: Vec<HashSet<usize>> = vec![HashSet::new(); num_vars];
        let mut in_degree: Vec<usize> = vec![0; num_vars];
        for vars in atoms {
            for pair in vars.windows(2) {
                let (earlier, later) = (pair[0], pair[1]);
                if earlier != later && adjacency[earlier].insert(later) {
                    in_degree[later] += 1;
                }
            }
        }
        Self {
            adjacency,
            in_degree,
        }
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

/// Body atoms whose column orders together form a cycle; see
/// [`check_attribute_order`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CyclicAttributeOrder {
    /// Indices into the query's body of the atoms on the cycle, ascending.
    pub atoms: Vec<usize>,
}

/// Checks that `query` admits a global attribute order under `stored`,
/// i.e. that [`topological_order`] over every atom's column-order edges
/// would not panic on it, and otherwise names the body atoms on one cycle
/// of those constraints. Under `--column-orders any` no atom is pinned, so
/// there is nothing to check and `kermit::db` skips this.
///
/// Each atom is matched one column at a time, left to right, so `r(X, Y)`
/// needs `X` bound before `Y`; `r(X, Y), s(Y, X)` then needs both orders
/// at once. That is a limitation of trie-descending joins over relations
/// stored in one column order, not malformed input.
///
/// Call it on the query the executor will run — after the const,
/// placeholder and selection rewrites — since those decide the variables
/// each atom carries. The rewrites keep body atoms in place (the const
/// rewrite only appends unary predicates, which carry no constraint), so
/// the reported indices also locate the atoms in the original query.
///
/// # Errors
///
/// [`CyclicAttributeOrder`] when the constraints are cyclic.
pub fn check_attribute_order(query: &JoinQuery) -> Result<(), CyclicAttributeOrder> {
    let analysis = analyse(query);
    let precedence = Precedence::new(analysis.num_vars, &analysis.predicate_variables);
    let order = precedence.kahn(|v| v);
    if order.len() == analysis.num_vars {
        return Ok(());
    }

    // Every variable Kahn left behind still has a left-behind predecessor
    // (its in-degree never reached zero), so walking predecessors from any
    // of them must revisit a variable; the revisited stretch is a cycle.
    let mut emitted = vec![false; analysis.num_vars];
    for &v in &order {
        emitted[v] = true;
    }
    let mut predecessor: Vec<Option<usize>> = vec![None; analysis.num_vars];
    for (earlier, laters) in precedence.adjacency.iter().enumerate() {
        for &later in laters {
            if !emitted[earlier] && !emitted[later] {
                predecessor[later] = Some(earlier);
            }
        }
    }
    let mut position: Vec<Option<usize>> = vec![None; analysis.num_vars];
    let mut path: Vec<usize> = Vec::new();
    let mut v = (0..analysis.num_vars)
        .find(|&v| !emitted[v])
        .expect("a short Kahn order leaves a variable behind");
    let cycle_start = loop {
        if let Some(at) = position[v] {
            break at;
        }
        position[v] = Some(path.len());
        path.push(v);
        v = predecessor[v].expect("a left-behind variable has a left-behind predecessor");
    };
    // `path` runs against the edges: each step goes to a predecessor.
    let cycle = &path[cycle_start..];
    let edges: HashSet<(usize, usize)> = (0..cycle.len())
        .map(|i| (cycle[(i + 1) % cycle.len()], cycle[i]))
        .collect();
    let atoms = analysis
        .predicate_variables
        .iter()
        .enumerate()
        .filter(|(_, vars)| vars.windows(2).any(|w| edges.contains(&(w[0], w[1]))))
        .map(|(i, _)| i)
        .collect();
    Err(CyclicAttributeOrder {
        atoms,
    })
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::optimiser::{CatalogStats, ColumnOrderPolicy},
        kermit_parser::JoinQuery,
    };

    #[test]
    fn identity_rank_reproduces_lexicographic_order() {
        // Triangle: R(0,1), S(1,2), T(0,2) — edges 0->1, 1->2, 0->2.
        let preds = vec![vec![0, 1], vec![1, 2], vec![0, 2]];
        assert_eq!(topological_order(&Precedence::new(3, &preds), |v| v), vec![
            0, 1, 2
        ]);
    }

    #[test]
    fn constraint_forces_late_canonical_variable_first() {
        // r(K, Y) with K canonical index 1, Y index 0: edge 1 -> 0.
        // The subject-position-constant shape — K must come first.
        let preds = vec![vec![1, 0], vec![1]];
        assert_eq!(topological_order(&Precedence::new(2, &preds), |v| v), vec![
            1, 0
        ]);
    }

    #[test]
    fn rank_breaks_ties_among_ready_variables() {
        // Star: R(0,1), S(0,2). After 0, both 1 and 2 are ready; the rank
        // function prefers 2.
        let preds = vec![vec![0, 1], vec![0, 2]];
        let order = topological_order(&Precedence::new(3, &preds), |v| {
            if v == 2 {
                0
            } else {
                v + 1
            }
        });
        assert_eq!(order, vec![0, 2, 1]);
    }

    fn stats(q: &JoinQuery, policy: ColumnOrderPolicy) -> CatalogStats {
        CatalogStats::for_query(q, policy, |_| None)
    }

    /// Under `stored`, `for_query` builds the same graph as `new` over
    /// every atom.
    #[test]
    fn under_stored_every_atom_adds_its_edges() {
        let q: JoinQuery = "Q(X, Y, Z) :- R(X, Y), S(Y, Z), T(X, Z).".parse().unwrap();
        let analysis = analyse(&q);
        let from_query = Precedence::for_query(&q, &stats(&q, ColumnOrderPolicy::Stored));
        assert_eq!(
            from_query,
            Precedence::new(analysis.num_vars, &analysis.predicate_variables)
        );
        assert!(!from_query.is_free());
        assert_eq!(from_query.predecessor_masks().unwrap(), vec![
            0b000, 0b001, 0b011
        ]);
    }

    /// Under `any` nothing is pinned: no edges, every order valid, and a
    /// query that is cyclic under `stored` orders fine.
    #[test]
    fn under_any_no_atom_adds_an_edge() {
        let q: JoinQuery = "Q(X, Y) :- edge(X, Y), edge(Y, X).".parse().unwrap();
        let precedence = Precedence::for_query(&q, &stats(&q, ColumnOrderPolicy::Any));
        assert!(precedence.is_free());
        assert_eq!(precedence.num_vars(), 2);
        assert_eq!(precedence.predecessor_masks().unwrap(), vec![0, 0]);
        assert_eq!(topological_order(&precedence, |v| v), vec![0, 1]);
        // The rank alone decides.
        assert_eq!(topological_order(&precedence, |v| 1 - v), vec![1, 0]);
    }

    fn check(q: &str) -> Result<(), CyclicAttributeOrder> {
        check_attribute_order(&q.parse::<JoinQuery>().unwrap())
    }

    #[test]
    fn acyclic_queries_pass_the_check() {
        assert_eq!(check("Q(X, Y, Z) :- R(X, Y), S(Y, Z), T(X, Z)."), Ok(()));
        // Subject-position constant shape: K before Y is a constraint, not
        // a cycle.
        assert_eq!(check("Q(Y) :- r(K, Y), Const_c5(K)."), Ok(()));
    }

    #[test]
    fn opposite_column_orders_are_a_cycle() {
        assert_eq!(
            check("Q(X, Y) :- edge(X, Y), edge(Y, X)."),
            Err(CyclicAttributeOrder {
                atoms: vec![0, 1]
            })
        );
    }

    #[test]
    fn a_longer_cycle_names_every_atom_on_it() {
        assert_eq!(
            check("Q(X) :- r(X, Y), s(Y, Z), t(Z, X)."),
            Err(CyclicAttributeOrder {
                atoms: vec![0, 1, 2]
            })
        );
    }

    /// `u` hangs off the cycle (it needs `Y` before `W`) but is not on it.
    #[test]
    fn atoms_off_the_cycle_are_not_named() {
        assert_eq!(
            check("Q(X) :- u(Y, W), r(X, Y), s(Y, X)."),
            Err(CyclicAttributeOrder {
                atoms: vec![1, 2]
            })
        );
    }

    /// A cycle can pass through a variable that only one atom carries, as
    /// the placeholder rewrite's fresh variables do: `r(X, K0, Y)` needs
    /// `X` before `Y` through `K0`.
    #[test]
    fn a_cycle_through_a_fresh_variable_is_found() {
        assert_eq!(
            check("Q(X) :- r(X, K0, Y), s(Y, X)."),
            Err(CyclicAttributeOrder {
                atoms: vec![0, 1]
            })
        );
    }

    #[test]
    fn predecessor_masks_mirror_the_edges() {
        // Triangle: R(0,1), S(1,2), T(0,2) — edges 0->1, 1->2, 0->2.
        let preds = vec![vec![0, 1], vec![1, 2], vec![0, 2]];
        let masks = Precedence::new(3, &preds).predecessor_masks().unwrap();
        assert_eq!(masks, vec![0b000, 0b001, 0b011]);
    }

    #[test]
    fn predecessor_masks_stop_at_64_variables() {
        let preds: Vec<Vec<usize>> = (0..65).map(|v| vec![v]).collect();
        assert!(Precedence::new(65, &preds).predecessor_masks().is_none());
        let masks = Precedence::new(64, &preds[..64])
            .predecessor_masks()
            .unwrap();
        assert_eq!(masks.len(), 64);
    }

    #[test]
    #[should_panic(expected = "cyclic global attribute order")]
    fn cyclic_constraints_panic() {
        // r(X, Y), s(Y, X): edges 0->1 and 1->0.
        let preds = vec![vec![0, 1], vec![1, 0]];
        topological_order(&Precedence::new(2, &preds), |v| v);
    }
}
