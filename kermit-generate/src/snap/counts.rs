//! The count oracle: the exact answer count of each SNAP query, computed from
//! the parsed edge list alone.
//!
//! `--verify` compares a join's count against the `expected` this module
//! produces, so it must be right in ways the joins cannot be wrong. It reads
//! only the parser's `Tuples` and shares no code with `kermit-algos` or with
//! any of `kermit-ds`'s tries: a bug in a seek, an intersection or a descent
//! cannot also be in the number it is checked against.
//!
//! The graph is a CSR (compressed sparse row) adjacency, sized by the largest
//! vertex id plus one, since ids are sparse. With `A` the adjacency matrix and
//! `d(v)` the degree of `v`, the counts are:
//!
//! | query | count | computed as |
//! |---|---|---|
//! | `path3` | `1ᵀA³1` | `Σ d(b)·d(c)` over rows `(b, c)` |
//! | `triangle` | `6 × triangles` | oriented sorted intersections |
//! | `cycle4` | `tr(A⁴)` | `Σ_v Σ_w (A²)_vw²` from a reusable tally |
//! | `clique4` | `24 × 4-cliques` | oriented sorted intersections, twice |
//!
//! Arithmetic is checked `u64`. An overflow is an error: a wrong `expected`
//! is worse than none.

use {
    anyhow::{bail, ensure},
    kermit_bench::SnapQuery,
    kermit_ds::Tuples,
    std::cmp::Ordering,
};

/// The exact answer count of each query in `queries` (same order) over
/// `edges`, a symmetric, self-loop-free, duplicate-free arity-2 batch (what
/// `parse_edge_list` produces). It shares no code with the joins.
///
/// # Errors
///
/// Fails if `edges` is not arity 2, or if a count overflows `u64`.
pub fn expected_counts(edges: &Tuples, queries: &[SnapQuery]) -> anyhow::Result<Vec<u64>> {
    ensure!(
        edges.arity() == 2,
        "the edge batch has arity {}, expected 2",
        edges.arity()
    );
    let graph = Graph::from_rows(edges);
    let mut counts = Vec::with_capacity(queries.len());
    for &query in queries {
        let count = match query {
            | SnapQuery::Triangle => triangle(&graph),
            | SnapQuery::Path3 => path3(&graph),
            | SnapQuery::Cycle4 => cycle4(&graph),
            | SnapQuery::Clique4 => clique4(&graph),
        };
        let Some(count) = count else {
            bail!("the {} count overflows u64", query.name());
        };
        counts.push(count);
    }
    Ok(counts)
}

/// A graph as a CSR adjacency: the neighbours of `v`, sorted, are
/// `neighbours[offsets[v]..offsets[v + 1]]`.
struct Graph {
    /// `vertices + 1` offsets into `neighbours`.
    offsets: Vec<usize>,
    /// Every adjacency list, one after another, each sorted ascending.
    neighbours: Vec<usize>,
}

impl Graph {
    /// Builds the graph whose adjacency lists are the rows' second values
    /// grouped by their first: count degrees, prefix-sum them into offsets,
    /// fill, then sort each list. The input holds both directions of every
    /// edge, so each list is complete.
    fn from_rows(edges: &Tuples) -> Self {
        let vertices = edges
            .rows()
            .map(|row| row[0].max(row[1]) + 1)
            .max()
            .unwrap_or(0);

        let mut degrees = vec![0usize; vertices];
        for row in edges.rows() {
            degrees[row[0]] += 1;
        }
        let mut offsets = Vec::with_capacity(vertices + 1);
        let mut running = 0;
        offsets.push(running);
        for degree in &degrees {
            running += degree;
            offsets.push(running);
        }

        let mut next = offsets[..vertices].to_vec();
        let mut neighbours = vec![0usize; edges.len()];
        for row in edges.rows() {
            neighbours[next[row[0]]] = row[1];
            next[row[0]] += 1;
        }
        for list in offsets.windows(2) {
            neighbours[list[0]..list[1]].sort_unstable();
        }
        Graph {
            offsets,
            neighbours,
        }
    }

    /// The number of vertices: the largest id plus one.
    fn vertices(&self) -> usize { self.offsets.len() - 1 }

    /// `d(v)`.
    fn degree(&self, v: usize) -> usize { self.offsets[v + 1] - self.offsets[v] }

    /// The sorted neighbours of `v`.
    fn neighbours_of(&self, v: usize) -> &[usize] {
        &self.neighbours[self.offsets[v]..self.offsets[v + 1]]
    }

    /// Whether the edge `u - v` points from `u` to `v` in the degree
    /// orientation: `u → v` iff `(d(u), u) < (d(v), v)`, a total order on the
    /// vertices. Each triangle and each 4-clique has exactly one ordering
    /// consistent with it, so counting along oriented edges counts each once.
    fn precedes(&self, u: usize, v: usize) -> bool { (self.degree(u), u) < (self.degree(v), v) }

    /// The oriented graph: `N⁺(u)` is the neighbours `v` of `u` with
    /// `u → v`. Filtering keeps each list sorted, and orienting towards the
    /// higher degree keeps `|N⁺(v)|` within `O(√m)`.
    fn oriented(&self) -> Graph {
        let mut offsets = Vec::with_capacity(self.offsets.len());
        let mut neighbours = Vec::new();
        offsets.push(0);
        for u in 0..self.vertices() {
            neighbours.extend(
                self.neighbours_of(u)
                    .iter()
                    .copied()
                    .filter(|&v| self.precedes(u, v)),
            );
            offsets.push(neighbours.len());
        }
        Graph {
            offsets,
            neighbours,
        }
    }
}

/// Calls `found` on each value in both sorted slices, ascending: a merge,
/// `O(|a| + |b|)`.
fn for_each_common(a: &[usize], b: &[usize], mut found: impl FnMut(usize)) {
    let (mut i, mut j) = (0, 0);
    while i < a.len() && j < b.len() {
        match a[i].cmp(&b[j]) {
            | Ordering::Less => i += 1,
            | Ordering::Greater => j += 1,
            | Ordering::Equal => {
                found(a[i]);
                i += 1;
                j += 1;
            },
        }
    }
}

/// `|a ∩ b|` for sorted slices.
fn intersection_size(a: &[usize], b: &[usize]) -> usize {
    let mut size = 0;
    for_each_common(a, b, |_| size += 1);
    size
}

/// 3-step walks, `1ᵀA³1 = Σ_{(b,c)} d(b) · d(c)` over the rows `(b, c)`: a
/// walk `A - B - C - D` is a row `(b, c)` for its middle step, any of the
/// `d(b)` neighbours of `b` for `A` and any of the `d(c)` neighbours of `c`
/// for `D`. Vertices may repeat.
fn path3(graph: &Graph) -> Option<u64> {
    let mut total: u64 = 0;
    for b in 0..graph.vertices() {
        let d_b = graph.degree(b) as u64;
        for &c in graph.neighbours_of(b) {
            let walks = d_b.checked_mul(graph.degree(c) as u64)?;
            total = total.checked_add(walks)?;
        }
    }
    Some(total)
}

/// `6 × Σ_{u → v} |N⁺(u) ∩ N⁺(v)|`: a triangle `a → b → c` (with `a → c`)
/// is found once, at the oriented edge `(a, b)` as the common out-neighbour
/// `c`. The 6 is its orderings as the answers `(A, B, C)` of the rule.
fn triangle(graph: &Graph) -> Option<u64> {
    let out = graph.oriented();
    let mut closed: u64 = 0;
    for u in 0..out.vertices() {
        let out_u = out.neighbours_of(u);
        for &v in out_u {
            let common = intersection_size(out_u, out.neighbours_of(v));
            closed = closed.checked_add(common as u64)?;
        }
    }
    closed.checked_mul(6)
}

/// Closed 4-step walks, `tr(A⁴) = Σ_v Σ_w (A²)_vw²`, not `8 ×` the
/// 4-cycles: walks that double back (`A = C` or `B = D`) count too.
///
/// `(A²)_vw` is the number of 2-step walks `v → u → w`. For each `v` the
/// tally adds 1 at `w` for every such walk, the squares of the touched
/// entries are summed, and only those entries are reset. So `A²` is never
/// formed, and the work is `Σ_u d(u)²`.
fn cycle4(graph: &Graph) -> Option<u64> {
    let mut tally = vec![0u64; graph.vertices()];
    let mut touched: Vec<usize> = Vec::new();
    let mut total: u64 = 0;
    for v in 0..graph.vertices() {
        for &u in graph.neighbours_of(v) {
            for &w in graph.neighbours_of(u) {
                if tally[w] == 0 {
                    touched.push(w);
                }
                tally[w] += 1;
            }
        }
        for w in touched.drain(..) {
            let walks = tally[w];
            tally[w] = 0;
            total = total.checked_add(walks.checked_mul(walks)?)?;
        }
    }
    Some(total)
}

/// `24 × Σ_{u → v} Σ_{w ∈ W} |N⁺(w) ∩ W|` with `W = N⁺(u) ∩ N⁺(v)`: a
/// 4-clique `a → b → c → d` is found once, at the oriented edge `(a, b)`, with
/// `c ∈ W` and `d ∈ N⁺(c) ∩ W`. The 24 is its orderings as the answers
/// `(A, B, C, D)` of the rule.
fn clique4(graph: &Graph) -> Option<u64> {
    let out = graph.oriented();
    let mut common: Vec<usize> = Vec::new();
    let mut found: u64 = 0;
    for u in 0..out.vertices() {
        let out_u = out.neighbours_of(u);
        for &v in out_u {
            common.clear();
            for_each_common(out_u, out.neighbours_of(v), |w| common.push(w));
            for &w in &common {
                let fourth = intersection_size(out.neighbours_of(w), &common);
                found = found.checked_add(fourth as u64)?;
            }
        }
    }
    found.checked_mul(24)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A symmetric arity-2 batch: each pair as (a, b) then (b, a), as
    /// `parse_edge_list` produces.
    fn symmetric(pairs: &[(usize, usize)]) -> Tuples {
        let mut edges = Tuples::with_capacity(2, 2 * pairs.len());
        for &(a, b) in pairs {
            edges.push(&[a, b]);
            edges.push(&[b, a]);
        }
        edges
    }

    fn k4() -> Tuples { symmetric(&[(0, 1), (0, 2), (0, 3), (1, 2), (1, 3), (2, 3)]) }

    /// A xorshift64 generator, so the random graphs are seeded without a
    /// `rand` dependency.
    struct XorShift(u64);

    impl XorShift {
        fn step(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }

        /// A value in `0..bound`.
        fn below(&mut self, bound: u64) -> u64 { self.step() % bound }
    }

    /// The rule's atoms as pairs of variable positions (A = 0, B = 1, C = 2,
    /// D = 3), and its number of variables. Spec § Workload, atom for atom.
    fn atoms(query: SnapQuery) -> (usize, &'static [(usize, usize)]) {
        match query {
            // edge(A, B), edge(B, C), edge(A, C)
            | SnapQuery::Triangle => (3, &[(0, 1), (1, 2), (0, 2)]),
            // edge(A, B), edge(B, C), edge(C, D)
            | SnapQuery::Path3 => (4, &[(0, 1), (1, 2), (2, 3)]),
            // edge(A, B), edge(B, C), edge(C, D), edge(A, D)
            | SnapQuery::Cycle4 => (4, &[(0, 1), (1, 2), (2, 3), (0, 3)]),
            // edge(A, B), edge(A, C), edge(A, D), edge(B, C), edge(B, D),
            // edge(C, D)
            | SnapQuery::Clique4 => (4, &[(0, 1), (0, 2), (0, 3), (1, 2), (1, 3), (2, 3)]),
        }
    }

    /// Every assignment of `arity` variables to vertices below `n`, as an
    /// odometer over base-`n` digits.
    fn assignments(n: usize, arity: usize) -> impl Iterator<Item = Vec<usize>> {
        (0..n.pow(arity as u32)).map(move |mut code| {
            (0..arity)
                .map(|_| {
                    let digit = code % n;
                    code /= n;
                    digit
                })
                .collect()
        })
    }

    /// The rule evaluated by brute force: the number of assignments of
    /// vertices to variables under which every atom is an edge of `adj`.
    fn brute_force(adj: &[Vec<bool>], query: SnapQuery) -> u64 {
        let (arity, atoms) = atoms(query);
        assignments(adj.len(), arity)
            .filter(|vars| atoms.iter().all(|&(x, y)| adj[vars[x]][vars[y]]))
            .count() as u64
    }

    #[test]
    fn k4_counts() {
        // 6 x 4 triangles; 12 rows x 3 x 3 walks; tr(A^4) = 4 x 3^2 + 12 x 2^2;
        // 24 x 1 four-clique.
        assert_eq!(expected_counts(&k4(), &SnapQuery::ALL).unwrap(), [
            24, 108, 84, 24
        ]);
    }

    #[test]
    fn c4_counts() {
        // No triangle; 8 rows x 2 x 2 walks; tr(A^4) = 4 x 2^2 + 4 x 2^2 (the
        // diagonal and the opposite pairs); no four-clique.
        let c4 = symmetric(&[(0, 1), (1, 2), (2, 3), (3, 0)]);
        assert_eq!(expected_counts(&c4, &SnapQuery::ALL).unwrap(), [
            0, 32, 32, 0
        ]);
    }

    #[test]
    fn sparse_ids_are_counted() {
        // Arrays are sized by the largest id plus one, and ids are sparse.
        // K3: 6 triangle rows; 3 x 2^3 walks; tr(A^4) = 2^4 + 1 + 1 over the
        // eigenvalues 2, -1, -1, which is 18; no four-clique.
        let k3 = symmetric(&[(5, 700_000), (700_000, 1_200_000), (5, 1_200_000)]);
        assert_eq!(expected_counts(&k3, &SnapQuery::ALL).unwrap(), [
            6, 24, 18, 0
        ]);
    }

    #[test]
    fn orientation_follows_degree_then_id() {
        // A star: the hub has the lowest id but the highest degree, so every
        // edge points at it and its own list is empty. Orienting by id alone
        // would give the hub all five.
        let star = symmetric(&[(0, 1), (0, 2), (0, 3), (0, 4), (0, 5)]);
        let out = Graph::from_rows(&star).oriented();
        assert!(out.neighbours_of(0).is_empty());
        for leaf in 1..=5 {
            assert_eq!(out.neighbours_of(leaf), [0]);
        }

        // A triangle: equal degrees, so the id breaks every tie.
        let k3 = symmetric(&[(0, 1), (0, 2), (1, 2)]);
        let out = Graph::from_rows(&k3).oriented();
        assert_eq!(out.neighbours_of(0), [1, 2]);
        assert_eq!(out.neighbours_of(1), [2]);
        assert!(out.neighbours_of(2).is_empty());
    }

    #[test]
    fn random_graphs_match_a_brute_force_join() {
        let mut nonzero = [0usize; 4];
        for case in 1..=50u64 {
            let mut rng = XorShift(case.wrapping_mul(0x9E37_79B9_7F4A_7C15));
            let n = 2 + rng.below(8) as usize;
            let density = 10 + rng.below(90);
            let candidates: Vec<(usize, usize)> = (0..n)
                .flat_map(|x| (x + 1..n).map(move |y| (x, y)))
                .collect();
            let mut adj = vec![vec![false; n]; n];
            let mut pairs = Vec::new();
            for (x, y) in candidates {
                if rng.below(100) < density {
                    adj[x][y] = true;
                    adj[y][x] = true;
                    pairs.push((x, y));
                }
            }

            let counts = expected_counts(&symmetric(&pairs), &SnapQuery::ALL).unwrap();
            for (i, query) in SnapQuery::ALL.into_iter().enumerate() {
                assert_eq!(
                    counts[i],
                    brute_force(&adj, query),
                    "case {case}, {}, edges {pairs:?}",
                    query.name()
                );
                nonzero[i] += usize::from(counts[i] > 0);
            }
        }
        // The comparison must not be vacuous: every query is non-zero
        // somewhere.
        assert!(nonzero.iter().all(|&cases| cases > 0), "{nonzero:?}");
    }

    #[test]
    fn an_empty_query_list_counts_nothing() {
        assert!(expected_counts(&k4(), &[]).unwrap().is_empty());
        assert!(expected_counts(&Tuples::new(2), &[]).unwrap().is_empty());
    }

    #[test]
    fn a_non_binary_batch_is_rejected() {
        let err = expected_counts(&Tuples::new(3), &SnapQuery::ALL).unwrap_err();
        assert!(err.to_string().contains("arity 3"), "{err}");
    }
}
