//! Variable ordering shared by all optimisers.

/// Computes a *global attribute order* (GAO): the query's `num_vars`
/// variables sorted by `rank`, with the canonical index as the final
/// deterministic tie-break.
///
/// A query builds each of its indexes with the columns in plan order, so no
/// relation's stored column order constrains the plan and every permutation
/// is valid: an ordering policy can be slow, never wrong. `rank` maps a
/// variable to an [`Ord`] key and is evaluated at most once per variable —
/// cheap enough for expensive statistics-driven policies.
pub fn order_by_rank<K: Ord>(num_vars: usize, rank: impl Fn(usize) -> K) -> Vec<usize> {
    let mut order: Vec<usize> = (0..num_vars).collect();
    order.sort_by_cached_key(|&v| (rank(v), v));
    order
}

#[cfg(test)]
mod tests {
    use {super::*, std::cell::Cell};

    #[test]
    fn the_rank_alone_decides_with_the_canonical_index_as_tie_break() {
        assert_eq!(order_by_rank(3, |v| v), vec![0, 1, 2]);
        assert_eq!(order_by_rank(3, |v| 2 - v), vec![2, 1, 0]);
        // Equal ranks fall back to the canonical index.
        assert_eq!(order_by_rank(3, |_| 0), vec![0, 1, 2]);
        assert_eq!(order_by_rank(3, |v| v == 0), vec![1, 2, 0]);
        assert_eq!(order_by_rank(0, |v| v), Vec::<usize>::new());
    }

    #[test]
    fn rank_is_evaluated_once_per_variable() {
        let calls = Cell::new(0);
        order_by_rank(5, |v| {
            calls.set(calls.get() + 1);
            v
        });
        assert_eq!(calls.get(), 5);
    }
}
