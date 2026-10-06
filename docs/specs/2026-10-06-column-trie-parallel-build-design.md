# A Parallel Build for ColumnTrie

**Date:** 2026-10-06
**Status:** Design approved (brainstormed 2026-10-06); not yet implemented
**Scope:** Issue #103. ColumnTrie gains a `parallel:N` build mode, the
morsel-driven build TreeTrie got in #94, producing exactly the trie the
`bulk` build produces. `bulk` stays the default and is the baseline. The
#94 scaling protocol, run on ColumnTrie, adds a third build-scaling curve
to the thesis.
**Related:** #94 (the parallel-build design this follows,
`docs/specs/2026-10-05-parallel-build-design.md`), #84 (the `bulk` build
and ColumnTrie's BuildMode), #102 (a thread count that never reached a
test).

## Motivation

The #94 design left ColumnTrie out, following a roadmap decision of
2026-10-05 that listed its parallel build as unplanned. On 2026-10-06 the
user reversed that decision: the thesis reports how each structure's build
scales with threads, and two of the three structures leave the comparison
incomplete. ColumnTrie is also the instructive contrast. TreeTrie
allocates one node per key and its 1e7 curve plateaus at about 1.8×, with
the allocator among the suspects. ColumnTrie stores flat arrays, so its
workers allocate a few large buffers each, not millions of small ones.

## Decisions

| Question | Decision |
|---|---|
| Purpose | A reported scaling result, the third curve beside TreeTrie and HashTrie. Not a per-phase timing breakdown, and not a tuned production build. |
| Approach | Partition, build, assemble on the calling thread: #94's three steps. Rejected: a parallel sort followed by the serial `from_sorted` pass (the O(n·a) pass would stay serial and cap the curve), and a fully parallel assemble that writes straight into the final arrays (two parallel phases and offset bookkeeping, for a share that is a sequential copy). That one can follow if Karp–Flatt points at the assemble step. |
| Category | **BuildMode**. `parallel:N` builds the `bulk` trie exactly, capacities included (rung 3 of the BuildMode rule), so it does not depend on the rung-2 amendment pending on `aidanb/hash-trie-parallel`. |
| Mode | `ColumnTrieBuildMode::Parallel(Threads)`, spelled `parallel:N`, 1 ≤ N ≤ 1024 (`Threads::MAX`). `bulk` stays the default; `incremental` is unchanged. |
| Baseline | `bulk`. kermit-lab's speedup defaults to each structure's default build mode, so one call draws all three curves. |
| Shared steps | `morsel::scatter` and `morsel::dispatch`, unchanged. `first_key_splitters` moves from `tree_trie/implementation.rs` to `morsel.rs`, unchanged (see "Moving the splitters"). |
| Constants | `MORSEL_TUPLES`, `PARTITIONS_PER_THREAD` and `SPLITTER_SAMPLES_PER_PARTITION` as TreeTrie uses them. They change speed only, never the trie. |
| Parsing | A `FromStr` per structure, as TreeTrie and HashTrie have. Factoring out the shared `parallel:<threads>` parsing would touch both siblings, so it is a separate change. |
| Schema version | Stays at 3. `parallel:N` is a new value of an existing axis. |
| Tracking | Issue #103, referencing #94, so #94 can close without waiting on ColumnTrie. |

## Background: why `bulk` can be reproduced exactly

`from_tuples_with_build_mode` checks every tuple's arity, sorts the tuples
with a hand-rolled lexicographic comparator, and hands them to
`from_sorted`. That pass only appends. For each tuple it finds the
*divergence depth*, the number of leading keys it shares with its
predecessor, and from that depth down it pushes one key onto every layer,
opening a child interval (a push onto `interval`) at every depth deeper
than the divergence depth. The root layer's single interval is pushed once, before
the first tuple.

**Contents.** Split the sorted input into first-key ranges, in key order.
The first tuple of each range has divergence depth 0 in the serial pass,
since its first key differs from its predecessor's; it pushes one key onto
every layer and opens an interval at every depth ≥ 1. In a separate
`from_sorted` over just its range, the same tuple has no predecessor and
does the same. Every later tuple of the range has the same predecessor in
both builds. So each layer's pushes are the per-range pushes, range after
range, except the root interval, which each per-range build pushes once
and the serial build pushes once in all. Duplicates are adjacent after
sorting and never span two ranges, so `tuple_count` is the sum of the
ranges' counts.

**Capacities.** `heap_size_bytes` sums every `data` and `interval`
capacity, and the serial build reaches those capacities by pushing one
element at a time. An assemble step that builds its arrays with
`with_capacity(len)` or a bulk `extend` from empty gets the exact length
instead, so it must grow each final `Vec` as pushes would. The same
constraint led TreeTrie's assemble to push root nodes one at a time; there
it costs O(k), here pushing would cost O(n·a) per-element pushes on the
calling thread.

## Design

### The build: `ColumnTrie::build_parallel`

In `kermit-ds/src/ds/column_trie/implementation.rs`, beside `from_sorted`:

```text
build_parallel(header, threads, morsel_tuples, tuples):
    (arity checks already ran in from_tuples_with_build_mode)
    no tuples, or arity 0      → Self::new(header)       // what bulk returns for both
    splitters  = first_key_splitters(tuples, PARTITIONS_PER_THREAD · N)
    partitions = scatter(threads, tuples, morsel_tuples, splitters.len() + 1,
                         |t| splitters.partition_point(|&s| s <= t[0]))   // step 1
    record (N, partition sizes)                          // test builds only
    built = dispatch(threads, partitions, |partition|    // step 2
        sort the partition with bulk's comparator; from_sorted(it))
    assemble(header, built)                              // step 3, calling thread
```

`from_tuples_parallel(header, threads, tuples)` calls it with
`MORSEL_TUPLES`; tests call `build_parallel` with small morsels to cut small
inputs into many morsels, as TreeTrie's tests do.

**Assemble.** For each depth `d`, with `base_d(p)` the number of keys
partitions before `p` put at depth `d`:

- `data_d` = every partition's `data_d`, in partition order;
- `interval_d` for `d ≥ 1` = every partition's `interval_d` entries, in
  partition order, each plus `base_d(p)`;
- `interval_0` = a single `0`, pushed, when any partition stored a tuple.
  Each partition's own root interval is dropped.
- `tuple_count` = the sum of the partitions' counts.

Each final `Vec` is first grown to the capacity pushes would have left it
with (next section), then filled with `extend_from_slice` (and, for
intervals, `extend` over the offset entries), which never reallocates
within that capacity. Each partition's scratch layers are dropped once
copied. Partitions that stored nothing contribute nothing.

### Growing a `Vec` the way pushes do

A private helper, along the lines of

```rust
/// An empty `Vec` whose capacity is what pushing `len` elements one at a
/// time leaves: grown by `reserve(capacity + 1)`, which takes the same
/// amortised step as a push into a full `Vec`, until it can hold `len`.
fn grown_like_pushes(len: usize) -> Vec<usize>
```

walks std's own growth sequence in O(log len) allocations, without
hard-coding the doubling rule or std's minimum capacity. std does not
promise a growth strategy, so a test pins the helper to the real thing
(see Testing): if std ever changes how `reserve` or `push` grows, the test
fails rather than the build drifting from `bulk`.

The serial build's own pushes are untouched. `from_sorted`'s doc comment
("never pre-size these `Vec`s") still holds for it; the parallel build
sizes its arrays only through this helper.

### The mode: `ColumnTrieBuildMode`

In `kermit-ds/src/ds/column_trie/build_mode.rs`:

- A third variant, `Parallel(Threads)`, documented as the morsel-driven
  build on N threads, the calling thread included
  (`docs/data-structures/parallel-build.md`).
- The `clap::ValueEnum` derive goes: a variant carrying data cannot derive
  it. In its place a `FromStr` with a `ParseColumnTrieBuildModeError`,
  modelled on `TreeTrieBuildMode`'s. It accepts `bulk`, `incremental` and
  `parallel:<threads>`. Every rejection names what was wrong and ends with
  the accepted forms, the thread range included.
- `axis_value` returns `"parallel:N"` for the new variant.
- `axis_values_match_clap_value_names` becomes a round trip through
  `FromStr` and a malformed-input table, copied from TreeTrie's tests.

### Dispatch: `from_tuples_with_build_mode`

Today it checks arities, sorts, then matches on the mode. The parallel arm
must branch after the checks and before the whole-input sort, since it
sorts per partition. So the function gains one early `if let
Parallel(threads) = mode { return Self::from_tuples_parallel(…) }` after
the arity loop. The sort and the `Bulk` / `Incremental` match below it
stay as they are, so those modes keep today's code path and their
measurements stay valid. The parallel arm's per-partition sort repeats
bulk's hand-rolled comparator, with the comment TreeTrie's parallel arm
carries: the comparison then costs the same in every arm, and `parallel:N`
against `bulk` measures the build process, not a cheaper sort.

### Moving the splitters

`first_key_splitters` and `SPLITTER_SAMPLES_PER_PARTITION` live in
`tree_trie/implementation.rs` as private items, with two tests
(`splitters_are_increasing_first_keys`, `splitters_share_out_the_tuples`).
They move, with their tests and doc comments, to `morsel.rs` as
`pub(crate)`, and TreeTrie imports them. Nothing in the moved code changes.
This touches a sibling, which Priority 6 asks to avoid. The alternatives
are worse: ColumnTrie importing from `tree_trie` couples the two
structures, and a copy would let the two builds' partitioning drift apart.
TreeTrie's trie, its tests and its measurements are unaffected; a rebuild
moves sorted-trie timings by up to ±3% regardless (the codegen precision
bound), and every speedup is measured within one binary.

### Test hook

As TreeTrie does, each parallel build pushes `(threads, partition sizes)`
onto a thread-local, under `#[cfg(any(test, feature = "test-hooks"))]`.
`kermit-ds`'s tests read it directly; `kermit_ds::test_hooks` gains
`take_column_trie_parallel_builds()` for `kermit`'s tests. Only
`kermit`'s dev-dependency enables the feature, so benchmarked builds never
record.

### `kermit` binary

- **`--ds-build` parsing** (`kermit/src/options.rs`): the `column-trie`
  arm becomes `mode.parse()`, mapping the error to `--ds-build
  column-trie: {why}`, like its two siblings. `column_trie_mode_names`
  goes; `bare_mode_hint` asks `FromStr`. The `--ds-build` help text lists
  `parallel:N` for `column-trie`.
- **Cells**: `SortedTrie::ColumnTrie { seek, build }` and
  `SortedTrieFamily<R> { build }` already carry a `ColumnTrieBuildMode`,
  and `build_mode_axes` reports its `axis_value`. No change in
  `execution.rs` beyond the route test.
- `--ds-build column-trie=parallel:N` thereby works on `bench ds`,
  `bench run` and `bench join`. `kermit join` takes no `--ds-build`, as
  today.

### kermit-lab

- `threads_of` already reads `parallel:N` from any structure.
- `speedup_table` and `kl.speedup` take `baseline="serial"` today, which no
  ColumnTrie row carries. The default becomes `baseline=None`, meaning each
  row's structure's default build mode, from one registry: `TreeTrie` →
  `serial`, `HashTrie` → `serial`, `ColumnTrie` → `bulk`. An explicit
  `baseline=` still applies to every structure. The `kermit-lab speedup`
  CLI follows.
- That registry is separate from the back-fill defaults in
  `SCOPED_AXIS_DEFAULTS`, which answer a different question: a ColumnTrie
  report without the axis predates #84 and still reads as `incremental`.

## Error handling and edge cases

- **Arity.** The existing loop in `from_tuples_with_build_mode` runs first,
  so a wrong arity panics with `bulk`'s message under every mode.
- **No tuples / nullary tuples.** `bulk` stores nothing for either; the
  parallel build returns `Self::new(header)` before partitioning and
  records nothing, as TreeTrie's does.
- **One dominant first key.** All its tuples share a partition, so one
  worker builds it; correct, slower, and a documented ceiling (as for
  TreeTrie).
- **Empty partitions.** Merged splitters can leave partitions empty; they
  are skipped at assembly.
- **Worker panics** reach the caller through `run_workers`, unchanged.
- **Memory.** Assembly holds the scratch layers and the final arrays at
  once, about twice the layers' size at the peak, less as each partition's
  scratch is dropped. The input tuples, which dwarf the layers, are
  consumed inside the workers.

## Testing

1. **Identity** (`column_trie/implementation.rs`):
   `parallel_builds_are_identical_to_bulk`. N ∈ {1, 2, 3, 8}; morsel
   sizes from 1 up to larger than the input; seeded `Lcg` inputs of arity
   1–4 with duplicates, one dominant first key, a single first key, and
   inputs that leave partitions empty. Compared with the existing
   `assert_identical` (arrays, capacities, `tuple_count`,
   `heap_size_bytes`). Smaller under Miri.
2. **Edge cases:** empty input, nullary tuples, a single tuple, and a wrong
   arity under `Parallel`, panicking with `bulk`'s message.
3. **The capacity helper:** `grown_like_pushes(len).capacity()` equals the
   capacity of a `Vec<usize>` after `len` pushes, for every `len` in
   0..=4096 and for powers of two ±1 up to 2²⁰ (one reference `Vec`
   pushed step by step; fewer lengths under Miri); and its length is 0.
4. **The record:** each build's partition sizes sum to n, and
   `morsel::take_worker_runs` shows N workers ran.
5. **Parsing** (`build_mode.rs`): round trip for `bulk`, `incremental`,
   `parallel:{1, 2, 8, 1024}`; the malformed-input table (`""`,
   `parallel`, `parallel:`, `parallel:0`, `parallel:1025`, `serial`,
   `radix:8`, `Bulk`, …); the default stays `Bulk`.
6. **Structure suites** (`kermit-ds/tests/trie_tests.rs`,
   `parquet_tests.rs`): `relation_trie_test_suite!` and
   `parquet_test_suite!` through `BuiltWith<ColumnTrie, ColumnParallel2>`
   and a `ColumnParallel3` alias, so a second thread count reaches the
   suites (the #102 lesson).
7. **Join suites** (`kermit/tests/join_tests.rs`):
   `define_multiway_join_test_suite_for_build_mode!(ColumnTrie,
   LeapfrogTriejoin, {LexicographicOptimiser, CardinalityOptimiser,
   CostBasedOptimiser}, ColumnParallel2)`, beside the `Incremental` block.
   The provider is `ColumnParallel2`, clear of TreeTrie's `Parallel2` and
   HashTrie's `HashParallel2`.
8. **Routes** (`kermit/src/execution.rs`):
   `column_trie_families_build_with_their_mode`, mirroring
   `tree_trie_families_build_with_their_mode`: every route that builds a
   ColumnTrie relation, `add_index` included, reaches the parallel build
   with its thread count. Mutation checks: a family that ignores its mode
   must fail it.
9. **CLI** (`kermit/tests/cli_column_trie_build_mode.rs`, `options.rs`
   units): `column-trie=parallel:2` recorded as `ds_build_mode` on
   `bench ds`, `bench run` and `bench join`; a `bench run --verify` smoke;
   an `-i all` sweep carries the mode only to ColumnTrie cells; the bare
   `parallel:8` hint names `column-trie=`; any test that pinned
   `parallel:N` as rejected on ColumnTrie flips.
10. **kermit-lab:** the baseline registry; `speedup_table` on a fixture
    mixing ColumnTrie `bulk` / `parallel:N` rows with TreeTrie `serial` /
    `parallel:N` rows; the real-binary contract test gains a
    `column-trie=parallel:2` run.

## Scaling protocol

The #94 protocol, as the TreeTrie half ran it
(`kermit-bench-runs/tree-trie-scaling-2026-10-05/README.md`), with
ColumnTrie and the `bulk` baseline:

- **Arms:** `--ds-build column-trie={bulk, parallel:1, parallel:2,
  parallel:4, parallel:8, parallel:16}`, default Layout (galloping seek).
  Five replicates, replicate i running the arms rotated by i − 1,
  replicates outermost. A distinct `--name` per (relation, arm,
  replicate).
- **Primary:** `bench ds -m insertion`, plus `space` in replicate 1 as the
  identity check. Inputs: the TreeTrie half's `data/` (unary and binary,
  1e3 to 1e7, fixed seeds) and WatDiv `friendof` and `price`, the same
  files.
- **Secondary:** `bench run lubm-reference`, ColumnTrie/LFTJ, `-m
  insertion end-to-end iteration`, `--verify` in replicate 1. `iteration`
  is there because the TreeTrie half left open whether a build mode moves
  it (heap placement). Here the calling thread allocates every final
  array, so no placement effect is expected; measuring it gives the
  contrast.
- **Discipline:** one release binary for every arm; the quiet gate; the
  run-local sampler that subtracts the benchmark's own cores; contended
  steps re-run. The run starts after the HashTrie scaling half has
  finished.
- **Analysis:** kermit-lab's `speedup_table` (speedup, efficiency,
  Karp–Flatt), space identity across arms, crossover size. Speedups are
  compared within a structure only; each half comes from its own binary.

## Out of scope

- A parallel assemble step. If Karp–Flatt shows the caller's copy
  dominating, it can follow as its own measured change.
- A per-phase timing breakdown (partition, sort and fill, assemble).
- A parallel `incremental` mode.
- A shared `parallel:<threads>` parser for the three structures.
- Parallel joins, thread pinning, NUMA placement, allocator changes, a
  persistent thread pool (as in #94).

## Documentation

- **`docs/data-structures/parallel-build.md`:** a ColumnTrie section: the
  three steps and how its assemble differs from TreeTrie's, the identity
  argument as an invariant (contents and capacities), the assemble row of
  the complexity table (O(n·a) copy on the calling thread), and the
  micro-example's ColumnTrie layers. After the run, "Scaling result:
  ColumnTrie".
- **`docs/data-structures/column-trie.md`:** `parallel:N` in its
  Optimizations section.
- **`docs/specs/optimization-standard.md`:** the BuildMode examples and
  catalogue rows.
- **`docs/specs/2026-10-05-parallel-build-design.md`:** its out-of-scope
  line for ColumnTrie points here.
- **`docs/specs/bench-report-schema.md`:** the new `ds_build_mode` value.
- **`BENCHMARKING.md`:** the scaling section names `bulk` as ColumnTrie's
  baseline.
- **`CLAUDE.md` and `ARCHITECTURE.md`:** the `BuildModeRelation` bullet,
  the Priority 1 precedents, a build-command example, the axis values.
- **kermit-lab `README.md`:** the per-structure baseline.

## Delivery

- **Phase 1, `kermit-ds`** (can start now: it touches `column_trie/`,
  `morsel.rs`, one import in `tree_trie/`, `test_hooks.rs` and the
  `kermit-ds` test suites): the splitter move, `grown_like_pushes`,
  `build_parallel`, the mode and its parsing, the record and hook, tests
  1–6.
- **Phase 2, the binary and kermit-lab** (after `aidanb/hash-trie-parallel`
  lands on master, then merged in): `options.rs`, the join suites, the
  route test, the CLI tests, kermit-lab, the documentation. Tests 7–10.
- **Measurement:** the driver script and the run, then the published
  result in `parallel-build.md` and on #103.

## Coordination

- **`aidanb/hash-trie-parallel`** (unpushed as of 2026-10-06) edits the
  same files Phase 2 does: `options.rs` (`bare_mode_hint`),
  `test_hooks.rs`, `join_tests.rs`, `parallel-build.md`, the standard,
  `CLAUDE.md`, and kermit-lab's `presets.py` / `speedup_table` (its
  Amendment 2 commit 721159c). It lands first; this branch merges
  origin/master in (never rebases) before Phase 2.
- **The HashTrie scaling half** is running on this host; no benchmark or
  release build of this branch runs until it finishes.

## Acceptance

- [ ] `parallel:N` builds the `bulk` trie exactly (arrays, capacities,
  `tuple_count`, `heap_size_bytes`) for N ∈ {1, 2, 3, 8}, and Miri is
  clean.
- [ ] The structure suites and the join suites pass for the new mode under
  all three optimisers, and the route test sees the parallel build on
  every route.
- [ ] The CLI, the report axis and kermit-lab work as designed, and `bench
  run --ds-build column-trie=parallel:2 --verify` passes.
- [ ] The diff leaves `from_sorted`, `from_sorted_by_insertion` and the
  `bulk` / `incremental` sort path untouched, and moves
  `first_key_splitters` without changing it.
- [ ] The scaling protocol has run, and its results are published with
  `env.txt`: speedup, efficiency, crossover and Karp–Flatt, plus the
  `iteration` comparison.
