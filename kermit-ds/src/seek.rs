//! Seek strategies: the Layout dimension of the sorted tries,
//! [`TreeTrie`](crate::TreeTrie) and [`ColumnTrie`](crate::ColumnTrie).
//!
//! A sorted-trie iterator's `seek(target)` moves to the least upper bound
//! of `target` among the siblings it has not yet passed. *Where* that bound
//! sits is a [`slice::partition_point`] question over the remaining
//! siblings. *How* to answer it is the strategy:
//!
//! | Strategy | Probes for a seek that moves `d` of `n` remaining siblings |
//! |---|---|
//! | [`LinearSeek`] | `min(d + 1, n)`: a scan, cheapest for one-step seeks |
//! | [`BinarySeek`] | `⌈log₂ n⌉ + 1`, whatever `d` is |
//! | [`GallopingSeek`] | 1 if `d = 0`, at most `2⌈log₂ d⌉ + 2` otherwise |
//!
//! Every strategy returns exactly what `partition_point` returns, so an
//! iterator's state after a seek is the same under all three; only the cost
//! differs. The strategy is a type parameter rather than a runtime value
//! because a runtime switch would put a branch in every seek. Under the
//! default, [`BinarySeek`], both tries compile to the `partition_point`
//! call they made before the parameter existed. Bench axis
//! `ds_layout_seek`; see `docs/data-structures/seek-strategies.md`.

use {
    kermit_iters::LayoutOption,
    serde_json::Value,
    std::{collections::BTreeMap, fmt::Debug},
};

/// Compile-time seek strategy of a sorted trie: how its iterator's `seek`
/// finds the least upper bound among the siblings it has not yet passed.
///
/// Implementors are zero-sized markers, so a trie pays nothing for the
/// strategies it does not use.
pub trait SeekStrategy: LayoutOption + Copy + Debug + Default + 'static {
    /// The number of leading elements of `remaining` that satisfy `below`,
    /// exactly as [`slice::partition_point`] returns it.
    ///
    /// `below` must hold on a prefix of `remaining` and on nothing after it,
    /// as `|k| k < target` does on a sorted slice. The result is then the
    /// offset of the least upper bound: `remaining.len()` if there is none,
    /// and 0 if the first element is already not below. Strategies differ
    /// only in which elements they probe.
    fn partition_point<T, P: FnMut(&T) -> bool>(remaining: &[T], below: P) -> usize;
}

/// Scans forward one element at a time: O(d) for a seek that moves `d`
/// positions. The cheapest strategy when seeks move one or two siblings, and
/// the algorithm `TreeTrie` used before issue #67.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LinearSeek;

/// Binary-searches everything not yet passed: O(log n) in the `n` remaining
/// siblings, however far the seek moves. The default, and what both sorted
/// tries did before the strategy became a parameter.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BinarySeek;

/// Gallops from the current position, probing offsets 0, 1, 2, 4, … until
/// one is no longer below, then binary-searches the bracket the last two
/// probes found: O(log d) for a seek that moves `d` positions. This is the
/// cost Veldhuizen's LFTJ analysis assumes for its amortised
/// O(1 + log(N/m)) seek bound.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GallopingSeek;

impl LayoutOption for LinearSeek {
    const NAME: &'static str = "linear";
}

impl LayoutOption for BinarySeek {
    const NAME: &'static str = "binary";
}

impl LayoutOption for GallopingSeek {
    const NAME: &'static str = "galloping";
}

impl SeekStrategy for LinearSeek {
    fn partition_point<T, P: FnMut(&T) -> bool>(remaining: &[T], mut below: P) -> usize {
        remaining
            .iter()
            .position(|element| !below(element))
            .unwrap_or(remaining.len())
    }
}

impl SeekStrategy for BinarySeek {
    fn partition_point<T, P: FnMut(&T) -> bool>(remaining: &[T], below: P) -> usize {
        remaining.partition_point(below)
    }
}

impl SeekStrategy for GallopingSeek {
    fn partition_point<T, P: FnMut(&T) -> bool>(remaining: &[T], mut below: P) -> usize {
        // A seek usually lands close by, so probe the current position
        // first: a seek that does not move costs one probe.
        if remaining.is_empty() || !below(&remaining[0]) {
            return 0;
        }
        // Gallop: double `bound` while it is still below. Invariant: `below`
        // holds at offset `bound / 2`. The loop doubles only while
        // `bound < len <= isize::MAX`, so the doubling cannot overflow.
        let mut bound = 1;
        while bound < remaining.len() && below(&remaining[bound]) {
            bound *= 2;
        }
        // The answer lies in (bound / 2, min(bound, len)]; binary-search the
        // elements strictly between. `lo <= hi`: the loop ran only while
        // `bound / 2 < len`.
        let lo = bound / 2 + 1;
        let hi = bound.min(remaining.len());
        lo + remaining[lo..hi].partition_point(below)
    }
}

/// The `ds_layout_seek` axis of a sorted trie seeking with `S`, shared by
/// both tries' `HasOptimizationAxes` impls so the key is spelt once.
pub(crate) fn seek_axes<S: SeekStrategy>() -> BTreeMap<String, Value> {
    BTreeMap::from([(
        "ds_layout_seek".to_string(),
        Value::String(<S as LayoutOption>::NAME.to_string()),
    )])
}

#[cfg(test)]
mod tests {
    use {
        super::{BinarySeek, GallopingSeek, LinearSeek, SeekStrategy},
        crate::test_support::Lcg,
        kermit_iters::LayoutOption,
        std::cell::Cell,
    };

    /// `S`'s offset for `target` in `remaining`, through the predicate a
    /// trie iterator's `seek` uses.
    fn seek_with<S: SeekStrategy>(remaining: &[usize], target: usize) -> usize {
        S::partition_point(remaining, |&k| k < target)
    }

    /// A strategy's offset for a target, as `seek_with` computes it.
    type SeekFn = fn(&[usize], usize) -> usize;

    /// Each strategy by name, so a failure names the culprit.
    const STRATEGIES: [(&str, SeekFn); 3] = [
        ("linear", seek_with::<LinearSeek>),
        ("binary", seek_with::<BinarySeek>),
        ("galloping", seek_with::<GallopingSeek>),
    ];

    fn assert_agrees(slice: &[usize], start: usize, target: usize) {
        let remaining = &slice[start..];
        let want = remaining.partition_point(|&k| k < target);
        for (name, seek) in STRATEGIES {
            assert_eq!(
                seek(remaining, target),
                want,
                "{name}: {slice:?}[{start}..], target {target}"
            );
        }
    }

    /// Every strictly increasing slice over a small key range (each subset,
    /// in order: a sibling list has no duplicates), from every start offset,
    /// for every target from below the current key to past the last. This
    /// covers a target equal to the current key (offset 0), a backward
    /// target (offset 0) and one past the end (offset `len`).
    #[test]
    fn strategies_agree_with_partition_point_exhaustively() {
        const KEYS: usize = if cfg!(miri) {
            6
        } else {
            10
        };
        for subset in 0u32..(1 << KEYS) {
            let slice: Vec<usize> = (0..KEYS).filter(|k| subset & (1 << k) != 0).collect();
            for start in 0..=slice.len() {
                for target in 0..=KEYS {
                    assert_agrees(&slice, start, target);
                }
            }
        }
    }

    /// Long slices, with and without duplicates (the contract holds on any
    /// sorted slice), so a gallop crosses brackets of many sizes.
    #[test]
    fn strategies_agree_with_partition_point_on_random_sorted_slices() {
        let mut rng = Lcg(0x5EE4_5EE4_5EE4_5EE4);
        let cases = if cfg!(miri) {
            20
        } else {
            500
        };
        let max_len = if cfg!(miri) {
            64
        } else {
            4096
        };
        for case in 0..cases {
            let len = rng.next_usize() % max_len;
            // A spread of 1 makes duplicates common.
            let spread = 1 + rng.next_usize() % 8;
            let mut slice: Vec<usize> = (0..len)
                .map(|_| rng.next_usize() % (len * spread + 1))
                .collect();
            slice.sort_unstable();
            if case % 2 == 0 {
                slice.dedup();
            }
            let start = rng.next_usize() % (slice.len() + 1);
            let last = slice.last().copied().unwrap_or(0);
            let current = slice.get(start).copied().unwrap_or(0);
            for target in [rng.next_usize() % (last + 2), current, last + 1] {
                assert_agrees(&slice, start, target);
            }
        }
    }

    /// How many times `S` evaluates the predicate to place `target`.
    fn probes<S: SeekStrategy>(remaining: &[usize], target: usize) -> usize {
        let count = Cell::new(0);
        S::partition_point(remaining, |&k| {
            count.set(count.get() + 1);
            k < target
        });
        count.get()
    }

    /// Bits needed to write `n`, i.e. `⌊log₂ n⌋ + 1` (0 for 0): an upper
    /// bound on `⌈log₂ n⌉` that needs no float.
    fn bit_length(n: usize) -> usize { (usize::BITS - n.leading_zeros()) as usize }

    /// Pins each strategy's *cost*, which the agreement tests cannot see: a
    /// galloping search that degenerated into a scan would still return the
    /// right offsets. Probe counts are exact, so this also runs under miri.
    #[test]
    fn probe_counts_match_each_strategys_bound() {
        let n = if cfg!(miri) {
            64
        } else {
            1024
        };
        // Even keys: the least upper bound of `2 * d` sits `d` positions ahead.
        let slice: Vec<usize> = (0..n).map(|k| 2 * k).collect();
        for d in 0..=n {
            let target = 2 * d;
            assert_eq!(
                probes::<LinearSeek>(&slice, target),
                (d + 1).min(n),
                "linear, d = {d}"
            );
            let binary = probes::<BinarySeek>(&slice, target);
            assert!(
                binary <= bit_length(n) + 1,
                "binary, d = {d}: {binary} probes"
            );
            let galloping = probes::<GallopingSeek>(&slice, target);
            if d == 0 {
                assert_eq!(galloping, 1, "galloping must stop at the current key");
            } else {
                assert!(
                    galloping <= 2 * bit_length(d) + 2,
                    "galloping, d = {d}: {galloping} probes"
                );
            }
        }
        assert_eq!(probes::<LinearSeek>(&[], 1), 0);
        assert_eq!(probes::<BinarySeek>(&[], 1), 0);
        assert_eq!(probes::<GallopingSeek>(&[], 1), 0);
    }

    /// These strings are `ds_layout_seek` report values: renaming one splits
    /// every ablation across the rename.
    #[test]
    fn layout_names_are_pinned() {
        assert_eq!(<LinearSeek as LayoutOption>::NAME, "linear");
        assert_eq!(<BinarySeek as LayoutOption>::NAME, "binary");
        assert_eq!(<GallopingSeek as LayoutOption>::NAME, "galloping");
    }
}
