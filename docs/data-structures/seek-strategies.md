# Seek strategies (`TreeTrie<S>`, `ColumnTrie<S>`)

Both sorted tries take their seek algorithm as a Layout type parameter,
`S: SeekStrategy` (`kermit-ds/src/seek.rs`). Each strategy has one
implementation, shared by both tries, so the same strategy on both tries
isolates the layout, and one trie under three strategies isolates the
seek (issue #80).

| Strategy | `--ds-layout-seek` | `ds_layout_seek` | Default |
|---|---|---|---|
| `LinearSeek` | `linear` | `"linear"` | |
| `BinarySeek` | `binary` | `"binary"` | |
| `GallopingSeek` | `galloping` | `"galloping"` | ✓ |

## Default

`galloping` has been the default since 2026-10-05, replacing `binary`, on
the strength of #80's strategy comparison. That run measured all three
strategies inside one release binary, with 5 replicates
(`kermit-bench-runs/seek-strategies-2026-10-04/analysis.txt`). The table
gives each strategy's time as a geomean of medians, relative to binary:

| Workload | Structure | galloping/binary | linear/binary |
|---|---|---|---|
| WatDiv probe set (34 queries) | TreeTrie | 0.805 | 9.152 |
| WatDiv probe set (34 queries) | ColumnTrie | 0.835 | 9.912 |
| `lubm-reference` (14 queries) | TreeTrie | 0.934 | 1.900 |
| `lubm-reference` (14 queries) | ColumnTrie | 0.913 | 1.942 |

On WatDiv, galloping was faster than binary on 23 of 34 queries for each
structure, with the replicate ranges disjoint. It was slower on q0065
(TreeTrie 1.172, ColumnTrie 1.086), q0129 (ColumnTrie 1.016) and q0185
(TreeTrie 1.058). Name `TreeTrie<BinarySeek>` / `ColumnTrie<BinarySeek>`, or
pass `--ds-layout-seek binary`, to run the earlier default.

## Contract

`seek(target)` moves the iterator to the least upper bound of `target`
among the siblings it has not yet passed. Its offset is the number of
remaining siblings whose key is below `target`, which is exactly what
`slice::partition_point` returns. A strategy is one function:

```rust
fn partition_point<T, P: FnMut(&T) -> bool>(remaining: &[T], below: P) -> usize;
```

Every strategy must return what `remaining.partition_point(below)`
returns, and the strategies differ only in which elements they probe. An
iterator's state after a seek is therefore identical under every strategy.

## What a strategy may not touch

The strategy computes the offset and nothing else. Around it, each
iterator keeps the logic LFTJ depends on:

- `TreeTrieIter` keeps its `at_end` guard and backward-seek panic, and
  moves the stack top only when the seek lands on a key. Off the end, the
  stack top stays on the last positioned node, so `open` after `at_end`
  still descends from it (see `tree-trie.md`).
- `ColumnTrieIter` keeps its `at_end` guard and advances `slice_offset`
  by the offset.

## The three strategies

```text
linear(remaining, below):
    i = 0
    while i < len and below(remaining[i]): i += 1
    return i

binary(remaining, below):
    return partition_point(remaining, below)      // std's binary search

galloping(remaining, below):
    if len == 0 or not below(remaining[0]): return 0
    bound = 1
    while bound < len and below(remaining[bound]): bound *= 2
    lo = bound / 2 + 1;  hi = min(bound, len)      // answer in (bound/2, hi]
    return lo + partition_point(remaining[lo..hi], below)
```

| Strategy | Probes for a seek that moves `d` of `n` remaining | Best when |
|---|---|---|
| linear | `min(d + 1, n)` | seeks move one or two siblings |
| binary | `⌈log₂ n⌉ + 1` | seeks jump far, uniformly |
| galloping | 1 if `d = 0`, at most `2⌈log₂ d⌉ + 2` | both: O(1) short seeks, O(log d) long ones |

Veldhuizen's LFTJ analysis assumes a seek costs O(log N), amortised
O(1 + log(N/m)) over m visited keys. Binary meets the first bound;
galloping meets both. `seek.rs` pins these bounds by counting probes
(`probe_counts_meet_each_strategy_bound`): linear's and galloping's as
tabled, and binary's, which is std's own search, as `⌊log₂ n⌋ + 2`, the
tabled bound or one more. `seek_hands_the_strategy_only_the_unpassed_siblings`
/ `…_keys` pin that each iterator hands its strategy only the siblings it
has not yet passed. Through the real iterators, `seek_cost_matches_the_strategy`
(in `trie_seek_tests!`) times a seek across 4096 siblings against a one-step
seek: the ratio must stay below 10x under `binary` and 32x under
`galloping`, and reach 32x under `linear`.

## Worked micro-example

Take sixteen siblings with keys `2, 4, 6, …, 32`, and the iterator on the
first (key 2).

| Seek | Lands on | `d` | linear probes | binary probes | galloping probes |
|---|---|---|---|---|---|
| `seek(4)` | 4 (offset 1) | 1 | 2: keys 2, 4 | 5 | 2: offsets 0, 1 |
| `seek(27)` | 28 (offset 13) | 13 | 14: keys 2 … 28 | 5 | 9: offsets 0, 1, 2, 4, 8, then 4 inside offsets 9–15 |

With a thousand siblings (keys `2, 4, …, 2000`), binary costs 11 probes
for both seeks. Linear still costs 2 and 14. Galloping costs 2 and 10, one
more than here, because its gallop now probes offset 16 rather than
stopping at the end of the list. That is the trade-off issue #67 exposed.
Binary removed TreeTrie's 10–197x losses on high-fan-out WatDiv queries,
but it made queries dominated by one-step seeks slower (q0236 ~1.9x, q0167
~1.35x; indicative).

## Why a Layout

A runtime switch would add a branch to every seek, a tax on runs that
never change strategy (the shape/value rule in
[`optimization-standard.md`](../specs/optimization-standard.md)). As a
type parameter, the strategy is monomorphised: each instantiation compiles
only its own search, and plain `TreeTrie` / `ColumnTrie` mean
`GallopingSeek`. The trie gains only a
zero-sized `PhantomData<S>`, which `seek_strategy_adds_no_state` pins.

## Measuring

- **Compare within one binary.** Compare strategies inside one binary
  (`--ds-layout-seek`), with a distinct `--name` per run. Reports carry no
  binary identity, so a cross-build comparison cannot be recovered later.
  Even within one binary, small TreeTrie-versus-ColumnTrie differences
  are not conclusive; see the codegen precision note in
  [`BENCHMARKING.md`](../../BENCHMARKING.md).
- **`bench ds` rejects the flag.** None of its metrics calls `seek`.
- **kermit-lab.** kermit-lab back-fills a missing `ds_layout_seek` on
  ColumnTrie rows as `binary`, which is true of every report ever written.
  It leaves TreeTrie rows unlabelled, because TreeTrie's seek was linear
  before 9604293 (#67).
- **Ablation phases.** `kl.ablation(df, axis="ds_layout_seek")` draws only
  for `iteration` and `end_to_end`.
