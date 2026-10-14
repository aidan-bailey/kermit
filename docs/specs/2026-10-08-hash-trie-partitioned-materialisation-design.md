# HashTrie Partitioned Materialisation: the Radix Family Copies Rows into Partition Order

**Date:** 2026-10-08
**Status:** Design approved in conversation (2026-10-08), section by section. Planned in
`docs/superpowers/plans/2026-10-08-hash-trie-partitioned-materialisation.md` and
implemented (2026-10-14); see Amendment 1 for where the code differs from this text.
**Scope:** Issue #101, layer 3, first half ("B1"). HashTrie's three partitioning build modes
copy their rows into first-key-hash partition order. They are `radix:K`, `parallel:N` and
`presized:N`, which today partition row ids and leave the rows in arrival order. The copy
is the paper's materialisation step (§3.3.2), so every later read of a partition's rows is
local. The second half ("B2"), designed after this one lands and is measured, threads
Algorithm 2's lists through a chain link per row (the paper's Figure 3) on top of these
partitions.
**Related:**
- #101: why partitioned builds keep the input-order penalty; layer 3.
- #105: the paper-convergence board.
- #111: flat tuple batches, landed d746b97; this design builds on its `Tuples` storage.
- #107: Algorithm 2.
- #94: the parallel and presized builds.
- Amendment 2 in `docs/specs/2026-10-05-parallel-build-design.md`.
- The #101 locality run: `kermit-bench-runs/hash-trie-locality-2026-10-08/`.

## Summary

`radix:K`, `parallel:N` and `presized:N` already decide a row's partition from its first
key's hash, then scatter row ids. This design scatters the rows themselves, through one new
helper, `morsel::partition_rows`, a classic histogram-and-scatter radix partition, run on N
threads when the mode has them:
- **The copy becomes the buffer.** The partition-ordered copy becomes the trie's buffer, and
  the input batch is freed.
- **Each partition is contiguous.** A partition's rows are a contiguous range of that buffer,
  in input order.
- **The modes' later steps are unchanged:** scratch roots, children by Algorithm 2, the
  merge, `presized:N`'s regions and tail.
- **Every read is local.** Every row those steps read lies in one partition's range.

`bulk` and `incremental` keep arrival order, so one binary measures the effect.

## Motivation

- **#101's locality run (2026-10-08) located the input-order penalty in tuple memory.** It
  separated the order rows are received from the order they were allocated, on friendof with
  15 uncontended replicates.
  - `bulk` pays 2.32× on a shuffled file, and 3.07× when rows arrive grouped but sit
    scattered in memory.
  - `radix:12` pays 2.13× on a shuffled file, and 1.70× when it receives rows shuffled but
    they sit grouped in memory. Its residual is the histogram pass's random reads.
  - DRAM fills rise from 0.44 to about 2.1 per tuple.
- **#111 changed the cost of each read, not where rows sit.** It replaced per-tuple `Vec`s
  with one buffer, which removed the allocations but left rows in arrival order.
  Partitioning only ids, as the radix family does today, cannot make a partition's reads
  local.
- **The paper partitions the tuples themselves.** §3.3.2: "As part of this materialization
  step, the tuples are partitioned based on the hash values of the first join key attribute.
  This ensures that tuples with similar join key hash values reside in physically close
  memory locations which is critical to achieve acceptable cache performance during the
  remainder of the build and probe phases."
- **#105's parallel-build row is still open.** It reads: "tuples stay in file order,
  partitioned in one pass into per-morsel buffers rather than two passes into contiguous
  storage (#101)".

## Decisions

Made in conversation, 2026-10-08:

1. **Two specs: B1 (this one, partitioned materialisation), then B2 (chain links).** Each
   lands and is measured on its own, so each effect is attributed.
   - B1 targets #101's locality penalty.
   - B2 targets the per-chain `Vec`, which #107 measured at 1.62× on `price`.
   - Links come second because walking a chain is only local once rows are partitioned,
     and because parallel workers can write links safely only to their own partition's
     contiguous region.
2. **Only the radix family materialises.**
   - The modes: `radix:K`, `parallel:N` and `presized:N`, which already partition.
   - `bulk` and `incremental` keep arrival order with no copy.
   - One binary measures the effect (`bulk` against `radix:K`, as #101 did).
   - This is a process change within the BuildMode axis, not a new axis.
   - Making a radix-family mode the default waits for the measurement.
3. **`radix:K` and `parallel:N` keep `bulk`'s root.**
   - The scatter records each copied row's input position, `origin`, 4 bytes per row, freed
     after the build.
   - The merge still inserts root keys in first-appearance order, so the root stays bucket
     for bucket `bulk`'s.
   - Only tuple placement changes, which keeps the A/B clean.
4. **One partitioning round.**
   - The paper adapts Balkesen et al.'s two-pass radix partitioning, whose second pass keeps
     each pass's fan-out within TLB limits.
   - #101's H4 found no fan-out cost at kermit's scale: on grouped input `radix:16` is within
     1.5 % of `radix:12`, with equal page walks (0.14 per tuple).
   - So one round of fan-out 2^K is kermit's choice, documented as such.
5. **TreeTrie keeps `scatter_rows` and its per-partition gather.**
   - Switching it to `partition_rows` would change a sibling's timings (Priority 6).
   - It is a follow-up.

## Background

After #111 (d746b97) HashTrie owns a `Tuples` buffer in arrival order. Its chains are
`Vec<RowId>`, a pruned singleton is a `RowId`, and a lazy pending list is a `Vec<RowId>`.

| Mode | Partition step today (all on the first key's hash) | Then |
| --- | --- | --- |
| `radix:K` | `radix::partition`: a histogram, then ids pushed into 2^K `Vec<RowId>` | a scratch root per partition (Algorithm 2), merged into the real root in first-appearance order |
| `parallel:N` | `morsel::scatter_rows` on N threads, into P = 4N partitions rounded up to a power of two, as per-morsel id segments | `dispatch` builds the scratch roots; a k-way merge on first appearance |
| `presized:N` | `scatter_rows` by home-bucket run, so each partition is a run of root regions | `fill_runs` pushes ids into a presized table of lists; a deferred tail sorted by id; children built run by run |

Each partition lists its ids in input order, so every mode's chains equal `bulk`'s.
`radix:K` and `parallel:N` are array-identical to `bulk`. `presized:N` is equivalent: its
root has the same capacity, occupied buckets and total displacement, and every subtrie is
identical.

## Design

### `morsel::partition_rows`

```rust
/// A batch's rows copied into partition order. Partition `p`'s rows are the
/// rows `offsets[p]..offsets[p + 1]` of `tuples`, in input order.
pub(crate) struct Partitioned {
    /// The partition-ordered copy; its capacity equals the input buffer's.
    pub(crate) tuples: Tuples,
    /// `partitions + 1` prefix sums of the partition sizes.
    pub(crate) offsets: Vec<usize>,
    /// `origin[new id]` is the copied row's position in the input.
    pub(crate) origin: Vec<RowId>,
}

pub(crate) fn partition_rows(
    threads: Threads, tuples: &Tuples, partitions: usize,
    partition_of: impl Fn(&[usize]) -> usize + Sync,
) -> Partitioned;
```

**The steps:**
1. **Histogram.** Morsels of `MORSEL_TUPLES` rows are dealt to `threads` workers through
   `run_workers` and `take_next`, as `scatter_rows` deals them. Each worker counts its
   morsels' rows per partition.
2. **Offsets.** Prefix sums over (partition, morsel), in that order, give every pair its own
   destination range. `offsets` keeps the per-partition totals.
3. **Scatter.**
   - The output `data` and `origin` buffers are allocated once.
   - They are split by `split_at_mut` into the (partition, morsel) ranges, grouped by morsel.
   - Each worker takes a morsel and copies each of its rows to its partition's cursor within
     that morsel's range, writing `origin` alongside.
   - Ranges are disjoint, so safe Rust hands them to threads with no atomics.
   - Reads are sequential; writes go to one cursor per partition.

**Properties:**
- **Stable.** Within a partition, rows keep input order: morsels are ordered within each
  partition, and rows within each morsel.
- **Deterministic.** The output is the same byte for byte for any number of threads, given
  the same partition count.
- **Capacity.** The copy reserves the input data buffer's capacity, so `space` does not
  depend on the mode.
- **Freed after.** The caller drops the input batch once the copy is made.
- **One thread** is the classic single-threaded radix scatter: `run_workers` runs the work on
  the calling thread.
- **Arity.** Every caller hashes `row[0]`, so `partition_rows` requires arity ≥ 1. HashTrie
  rejects nullary relations.

### The three modes

The partitioned fill takes the batch by value and returns the copy, which becomes the trie's
buffer:

```rust
fn from_tuples_partitioned(
    header: RelationHeader, config: HashTrieConfig, tuples: Tuples,
    fill: impl FnOnce(Tuples, u32, usize) -> (Tuples, HashTrieNode<P, E>),
) -> Self
```

**`radix:K`:**
- `partition_rows` on one thread (`Threads::new(1)`), with `1 << K` partitions and the top K bits of `H::hash(row[0])`,
  replaces `radix::partition`.
- Each non-empty partition's range is grouped into a scratch root and its children built by
  Algorithm 2, as today, now reading partition-local rows.
- `first_seen` records `origin[new id]` for a key's first row. A partition keeps input order,
  so that is the key's first input position.
- `insert_in_first_appearance_order` sorts by it, so the root is `bulk`'s, bucket for bucket.

**`parallel:N`:**
- `partition_rows(N, …, P = 4N rounded up to a power of two, same function)` replaces
  `scatter_rows`, so the copy runs on N threads.
- `dispatch` hands each non-empty partition's range to a worker for its scratch root.
- `merge_in_first_appearance_order` keys on `origin`, so the root is `bulk`'s.

**`presized:N`:**
- `partition_rows` uses the existing home-bucket-run function, so partition p's rows are
  exactly its run of root regions.
- `fill_runs` pushes each partition's id range.
- **The deferred tail is sorted by `origin`.** Sorting by new id would order the tail
  partition-major. The partition count depends on N, so the root's placement of tail keys
  would vary with N. Sorting by `origin` keeps today's input order, so the trie stays the
  same for every N.
- Children are built run by run from partition-local ids.

**`ParallelBuild` test-hook records:** partition sizes come from `offsets`; their meaning is
unchanged.

**Unchanged:**
- `bulk` and `incremental`: the batch is moved in, in arrival order.
- Lazy expansion: pending ids are now partition-local in the radix family.
- `Relation::insert`: rows append after the partitions.
- Chains, which stay `Vec<RowId>` (B2).
- `scatter_rows` and TreeTrie.

### Equivalence

**What stays the same, and what becomes equivalent:**
- The radix family's buffer is a permutation of `bulk`'s that depends on K or N.
- These modes are therefore equivalent to `bulk` under Amendment 2, not array-identical.
  Every chain, singleton and pending list holds the same tuples in the same order, under
  different ids.
- Capacities and `space` are identical.
- The root of `radix:K` and `parallel:N` is identical to `bulk`'s.
- `presized:N`'s root stays equivalent, as today, and the same for every N.

## Parity with the paper

| Element (VLDB 2020 §3.3.1–3.3.2, Fig. 3) | After #111 | After B1 | After B2 |
| --- | --- | --- | --- |
| Tuples in one contiguous buffer, fixed-length layout | ✓ | ✓ | ✓ |
| Tuples partitioned on the first join key's hash as they are materialised | ✗ arrival order | ✓ in `radix:K`, `parallel:N` and `presized:N`; ✗ in `bulk` (the default) and `incremental` | ✓ |
| Partitioning morsel-driven, on N threads | ids only | ✓ the copy itself (`parallel:N`, `presized:N`) | ✓ |
| Two-pass radix partitioning (Balkesen et al.) | ✗ | ✗ one round, kermit's (Decision 4) | ✗ |
| A leaf is a list threaded through an 8-byte chain pointer per tuple | ✗ | ✗ a `Vec<RowId>` per chain | ✓ |

## Placement under the standard

This is a process change inside the existing BuildMode axis. The modes' names, flags and axes
do not change.

**No schema bump.** The radix family's `insertion` now includes the copy, and its `iteration`
sees the new placement. #107 changed `radix:K`'s process without a schema bump, and B1
follows it. Records measured before this change get dated notes in the docs, and reports
already name their binary.

## Testing

**A content-level identity helper,** `identity::assert_same_contents(a, b)`, compares two
tries node by node:
- every table: the same capacity, the same occupied buckets and the same hashes;
- every chain, singleton and pending list: the same tuples in the same order, each id
  resolved through its own trie's buffer;
- the buffers: the same multiset of rows, at the same capacity.

`assert_same_trie` (array-level) stays for `bulk` against `incremental`.
`assert_equivalent_trie` (`presized:N`'s root rule) uses the content comparison below the
root.

**Changed tests:**
- `radix_builds_the_bulk_trie_*` and `parallel_builds_the_bulk_trie_*` become content-level.
  Each also asserts that the root is bucket for bucket `bulk`'s.
- `presized_parallel_builds_are_equivalent_*` compare subtries by content.
- `presized_parallel_builds_are_the_same_for_every_n_on_dense_roots` becomes content-level.
- A new test shows the deferred tail is pushed in input order, by `origin`, at N = 1 and
  N = 4.

**New `partition_rows` tests:**
- `offsets` are the prefix sums of the partition sizes;
- partition p's rows are exactly those `partition_of` maps to p;
- rows keep input order within a partition;
- `origin` is a permutation mapping each copied row to its input row;
- N threads produce byte-identical output to one thread for the same partition count;
- the empty batch;
- the copy's capacity equals the input's.

**Pinning the copy:** for each radix-family mode, the trie's buffer equals `partition_rows`'
output. A build that skipped the copy would pass every content test, so this test is what
makes the change observable.

**Unchanged:**
- The join suites, the lazy and pruning invariants and `result_allocation.rs`.
- `tuple_allocation.rs`'s HashTrie cells still hold their bounds. The per-partition id lists
  give way to one copy buffer plus `origin`, so the counts fall. The `parallel:1` and
  `presized:1` cells re-derive their bound comments.

**Mutation checks:**
- the copy skipped, so the buffer stays in arrival order;
- the presized tail sorted by new id instead of `origin`, run at N = 1 and N = 4;
- the scatter's within-partition order broken.

## Measurement

After landing, a replicated A/B in the #107 style: 5 replicates, a distinct `--name` per
step, one binary per side built from `git archive` in its own target dir, and the quiet gate
plus host sampler.

**A, the effect within one binary:**
- Modes: `bulk`, `radix:12`, `radix:16`, `parallel:8`, and `presized:8` with
  `root-capacity=tuples`.
- Metrics: `bench ds` `insertion` and `iteration`, plus `space`, which must be identical
  across modes.
- Inputs: friendof grouped and file-shuffled, `price`, and binary-1e7 sorted and in file
  order.
- **Expected on shuffled input:**
  - `radix:12` falls from its 2.13× order penalty to near its grouped time plus the copy;
  - it beats `bulk`, which pays 2.32×.
- **Expected on grouped input:** the copy makes the radix family slightly slower than
  `bulk`.

**B, the #101 locality harness re-run with `radix:12`:**
- The GS and SS cells should fall to about GG plus the copy, because the copy undoes any
  allocation order.
- This answers #101's open question with the paper's mechanism in place.

**C, combined with #111's own A/B,** which has not yet run:
- three binaries: 89771a9 (before #111), d746b97 (#111) and B1;
- one run then records both changes' effects.

## Out of scope

- B2: chain links, removing the per-chain `Vec`, and `emit_leaf` cursors.
- Moving TreeTrie onto `partition_rows`.
- Making a radix-family mode the default; that decision follows the measurement.
- Two-pass partitioning.
- `--ds-config partition-bits`.
- #106.

## Acceptance

1. `radix:K`, `parallel:N` and `presized:N` store the partition-ordered copy as their buffer,
   pinned by a test.
2. Every mode is content-equivalent to `bulk`.
   - The root of `radix:K` and `parallel:N` is bucket for bucket `bulk`'s.
   - `presized:N`'s root is equivalent, and its trie is the same for every N.
3. `space` is identical across modes.
4. The CI gate passes: test, clippy, fmt, doc and python.
5. The docs and the #105 board rows are updated.

## Documentation

- **`docs/data-structures/hash-trie.md`:**
  - the radix, parallel and presized sections;
  - the Construction table;
  - the parity rows;
  - dated notes on records that predate this change.
- **`docs/data-structures/parallel-build.md`:** the HashTrie steps and their worked
  examples.
- **CLAUDE.md:** the `BuildModeRelation` bullet. The radix family copies rows into partition
  order and is equivalent rather than array-identical; `presized:N` was already equivalent.
- **ARCHITECTURE.md:** the HashTrie construction summary.
- **The #105 board:**
  - The parallel-build row's "still open: tuples stay in file order" closes.
  - The materialisation row is met by the radix family, with chain pointers B2's.

## Amendment 1 (2026-10-14): as built

Only where the code differs from the text above:

- **`partition_rows` takes `morsel_rows`.** The signature is
  `partition_rows(threads, tuples, morsel_rows, partitions, partition_of)`. `parallel:N` and
  `presized:N` pass `MORSEL_TUPLES`; `radix:K` passes one morsel for the whole batch,
  `tuples.len().max(1)`, since it runs on one thread and a count and a range per
  (partition, morsel) pair would only cost.
- **Partition numbers are `u16`.** The histogram keeps each row's partition number as a
  `u16` between the two passes, so `partitions <= 1 << 16` is asserted (`radix:16` is the
  most any build uses). The copy and `origin` come from zeroed allocations (`alloc_zeroed`),
  since safe Rust cannot hand out uninitialised ranges.
- **`partition_rows` is total at arity 0.** The text's "requires arity >= 1" does not hold:
  a nullary row is an empty slice with an id and an input position, which the function
  documents and a test pins.
- **Every mode frees `origin` once nothing reads it.** `presized:N` frees `origin` and
  `offsets` after its tail; `radix:K` and `parallel:N` free them right after the scratch
  roots are built, before the merge, since each key's first input position is already in
  its `first_seen` entry. None holds `origin` until the build returns.
