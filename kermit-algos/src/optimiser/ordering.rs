//! Variable ordering shared by all optimisers.

use crate::analysis::QueryAnalysis;

/// Computes a *global attribute order* (GAO): the join keys of `analysis`
/// first, then the payload, each group sorted by `rank`, with the canonical
/// index as the final deterministic tie-break.
///
/// Payload last (#116): a payload variable is carried by one atom, so binding
/// it early only multiplies the search below it, and a `HashTrie` under
/// `--ds-layout-levels keys` has no level for it at all. A query builds each
/// of its indexes with the columns in plan order, so no stored column order
/// constrains the plan: an ordering policy can be slow, never wrong. `rank`
/// maps a variable to an [`Ord`] key and is evaluated at most once per
/// variable, cheap enough for statistics-driven policies.
pub fn order_by_rank<K: Ord>(analysis: &QueryAnalysis, rank: impl Fn(usize) -> K) -> Vec<usize> {
    let mut order: Vec<usize> = (0..analysis.num_vars).collect();
    order.sort_by_cached_key(|&v| (!analysis.join_keys[v], rank(v), v));
    order
}

#[cfg(test)]
mod tests {
    use {super::*, std::cell::Cell};

    /// An analysis of `num_vars` variables whose join keys are `keys`.
    fn analysis(num_vars: usize, keys: &[usize]) -> QueryAnalysis {
        QueryAnalysis {
            num_vars,
            predicate_variables: Vec::new(),
            join_keys: (0..num_vars).map(|v| keys.contains(&v)).collect(),
        }
    }

    #[test]
    fn the_rank_alone_decides_with_the_canonical_index_as_tie_break() {
        let all = analysis(3, &[0, 1, 2]);
        assert_eq!(order_by_rank(&all, |v| v), vec![0, 1, 2]);
        assert_eq!(order_by_rank(&all, |v| 2 - v), vec![2, 1, 0]);
        // Equal ranks fall back to the canonical index.
        assert_eq!(order_by_rank(&all, |_| 0), vec![0, 1, 2]);
        assert_eq!(order_by_rank(&all, |v| v == 0), vec![1, 2, 0]);
        assert_eq!(order_by_rank(&analysis(0, &[]), |v| v), Vec::<usize>::new());
    }

    #[test]
    fn join_keys_go_before_payload_whatever_their_rank() {
        // 1 is the only join key: it goes first even when ranked last.
        let one_key = analysis(3, &[1]);
        assert_eq!(order_by_rank(&one_key, |v| v), vec![1, 0, 2]);
        assert_eq!(order_by_rank(&one_key, |v| 2 - v), vec![1, 2, 0]);
        assert_eq!(order_by_rank(&one_key, |v| usize::from(v == 1)), vec![1, 0, 2]);
    }

    #[test]
    fn rank_is_evaluated_once_per_variable() {
        let calls = Cell::new(0);
        order_by_rank(&analysis(5, &[0, 1, 2, 3, 4]), |v| {
            calls.set(calls.get() + 1);
            v
        });
        assert_eq!(calls.get(), 5);
    }
}
