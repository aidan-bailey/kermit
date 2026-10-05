# Parallel Builds for TreeTrie and HashTrie

**Date:** 2026-10-05
**Status:** Design approved (brainstormed 2026-10-05); not yet implemented
**Scope:** Issue #94. TreeTrie and HashTrie each gain a `parallel:N` build mode. The
build is morsel-driven, runs on `std::thread::scope`, and produces exactly
the trie the serial build produces. Serial stays the default. A scaling
protocol and a kermit-lab preset make build time against thread count a
reported thesis result.
**Related:** #91 (radix partitioning: it reuses this design's partition
step), #92 (lazy child expansion).

## Motivation

The hash-trie paper (SIGMOD 2020, §3.3.2) lists a parallel build among its
optimisations, and the optimisation standard catalogues it as a Large
BuildMode (`docs/specs/optimization-standard.md`, "Available to add"). The
paper's parallelism follows the morsel-driven model of Leis et al.
(SIGMOD 2014): the input is cut into small morsels, and whichever thread is
free takes the next one.

Every build in kermit is single-threaded today, on a host with 8 cores and
16 hardware threads. The thesis question is how each structure's build
scales with threads. Answering it needs two things:

- Only the build process may vary between arms, so that every other metric
  stays comparable.
- The ceiling on scaling should be predictable from the design, not
  discovered by accident.

## Decisions

| Question | Decision |
|---|---|
| Purpose | A reported scaling result: build time against thread count per structure, relative to the serial build. |
| Structures | TreeTrie and HashTrie. ColumnTrie stays out (an explicit roadmap decision), though the shared partition and build steps would serve it. |
| Scope | Build only. Joins stay single-threaded. |
| Category | **BuildMode**: a process yielding the same shape. `parallel:N` builds the identical trie, capacities included, for every N. The standard's strict rule therefore holds and needs no amendment. |
| Approach | Partition, build, assemble (below). Two alternatives were rejected. Filling HashTrie's root in parallel builds a different layout, so it needs an amendment and lets layout effects reach `iteration` and `space`. A shared trie with concurrent inserts needs locks and changes layout from run to run. |
| Threads | `std::thread::scope` plus `Mutex`-guarded work queues. Safe Rust, no new dependency, and every thread is joined before the build returns. The calling thread is one of the N workers. Threads are not pinned. |
| Modes | `serial` and `parallel:N` with 1 ≤ N ≤ 1024 (`Threads::MAX`, which bounds the threads and per-morsel buckets one build allocates). There is no bare `parallel`: N is always explicit and always recorded. |
| Default | `serial`, which runs today's code unchanged. Every existing measurement stays valid. |
| Small inputs | No fallback to serial below a size threshold. `parallel:N` always runs all three steps, so the axis means what it says; the crossover is measured, not hidden. |
| Partition count and morsel size | Constants with comments, not knobs. They change speed only, never the trie. |
| Mode types | One per structure (`TreeTrieBuildMode`, `HashTrieBuildMode`), following `ColumnTrieBuildMode`. They diverge later: HashTrie's gains `radix:K` with #91, TreeTrie's does not. |
| HashTrie construction | An inherent constructor takes both config and mode. `BuildModeRelation` uses the default config, and `ConfigurableRelation` keeps the serial mode. |
| Schema version | Stays at 3. The axis values are additive, and old TreeTrie and HashTrie rows read as `serial`. |
| Criterion names | Unchanged. Each arm of a scaling run uses its own `--name`, as `BENCHMARKING.md` already requires for ablations. |
| Test dependency | None added. Seeded inputs come from the existing `Lcg` (`kermit-ds/src/test_support.rs`). |
| Delivery | Plan 1: the shared steps, TreeTrie, CLI and kermit-lab. Plan 2: HashTrie. Then the scaling measurement. |

## Background: why the serial builds can be reproduced exactly

**TreeTrie.** `from_tuples` sorts the tuples, then inserts each one with
`insert_into_children`. With sorted input, every new child sorts after its
siblings, so `children.insert(pos, node)` always inserts at `pos == len`. A
child list therefore grows exactly as if built by `push`, one node at a
time.

**HashTrie.** `from_tuples_with_config` calls `insert_at` once per tuple, in
input order.

- A tuple touches only its first attribute's root entry and the subtrie
  below it.
- For a key that is already present, `HashTable::entry_or_insert_with`
  returns before its resize check (`hash_table.rs:153-157`). So a table's
  final layout (which slot each key occupies, and its capacity) depends only
  on the order in which its *new* keys arrive.
- `grow` rehashes in bucket order, which is itself a function of that
  history.
- Leaf chains are pushed in input order.

**Consequences.**

- A subtrie is a function of its own key's tuples. For TreeTrie that is
  sorted order; for HashTrie, input order, including singleton pruning's
  evict-and-reinsert sequence.
- A root is a function of key order (TreeTrie) or of the order in which its
  keys first appear (HashTrie).
- So the subtries can be built anywhere, as long as each sees its own
  tuples in the right order and the root is assembled in the right order.

## Design

### Shared steps: `kermit-ds/src/morsel.rs`

The module is new and knows nothing about tries. Its comments use the
vocabulary of Leis et al.: morsels, dispatcher.

```rust
/// The N of `parallel:N`: how many threads a parallel build uses, the
/// calling thread included. Only 1 to `Threads::MAX` (1024) are
/// representable.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Threads(NonZeroUsize);

/// Partition step. Moves every tuple into partition `partition_of(tuple)`.
/// `threads` workers take fixed-size morsels from a shared queue. Each
/// tuple keeps its input position, and each partition lists its tuples in
/// input order: its per-morsel segments, in morsel order. No concatenation
/// pass is needed.
pub(crate) fn scatter(
    threads: Threads, tuples: Vec<Vec<usize>>, partitions: usize,
    partition_of: impl Fn(&[usize]) -> usize + Sync,
) -> Vec<Partition>;

/// Build step. Runs `task` on every item. Workers take the next item when
/// they are free, and the results come back in item order.
pub(crate) fn dispatch<T: Send, R: Send>(
    threads: Threads, items: Vec<T>, task: impl Fn(T) -> R + Sync,
) -> Vec<R>;
```

- **Moving tuples without copying.** `scatter` gives each worker a
  disjoint `chunks_mut` morsel and moves tuples out with `std::mem::take`.
  That is safe Rust, and the tuples' heap buffers are not copied.
- **Determinism.** No output depends on scheduling. Partitions are ordered
  by morsel index, and `dispatch` orders its results by item index.
- **Panics.** A panicking worker is joined, and `thread::scope` re-raises
  the panic. It is never swallowed and never deadlocks.
- **Morsel size.** 16 384 tuples. One uncontended lock per 16 384 tuples is
  negligible, and morsels this small still give a mid-sized relation
  several morsels per thread. (Leis et al. use about 100 000.)
- **Partition count.** About 4·N, more partitions than threads, so
  `dispatch` can balance uneven partitions.

`Threads` is re-exported from `kermit-ds`, because the binary constructs
modes from it.

### TreeTrie: `kermit-ds/src/ds/tree_trie/build_mode.rs`

`TreeTrieBuildMode { #[default] Serial, Parallel(Threads) }` implements
`kermit_iters::BuildMode`, with axis values `"serial"` and `"parallel:N"`.
`TreeTrie<S>` implements `BuildModeRelation`. `Relation::from_tuples` stays
the serial build.

`Parallel(n)` runs these steps:

1. **Checks.** Run the same arity checks as `from_tuples`, on the calling
   thread and with the same panic messages. Empty input returns
   `Self::new(header)` without spawning anything.
2. **Splitters.** Stride-sample up to 128 first keys per partition and
   sort the sample, keeping its duplicates. The splitters are the sample's
   P − 1 quantiles, with P = 4·N; repeats are merged, since a key heavier
   than one share repeats and its tuples cannot be split. Because the
   sample keeps duplicates, the splitters share out the tuples, not the
   distinct keys (de-duplicating the sample first put 28% of a skewed
   input in one partition). A tuple's partition is
   `splitters.partition_point(|&s| s <= tuple[0])`. Partitions are
   therefore ordered by key, and all tuples sharing a first key land
   together.
3. **Partition.** `scatter` with that function. The input positions are
   dropped, because TreeTrie sorts anyway.
4. **Build.** `dispatch` over the partitions. Each partition:
   - sorts its tuples with the serial build's hand-rolled comparator, so a
     comparison costs the same in both builds (the derived order is the
     same but measurably cheaper, which would favour the parallel arm);
   - inserts them with `insert_into_children` into a local `Vec<TrieNode>`;
   - counts the `true` returns, which are the distinct tuples.
5. **Assemble.** Start from `Self::new(header)` and push each partition's
   nodes onto the root in partition order, **one push at a time**.
   `extend` and `append` reserve in bulk and would change the root's
   capacity. `tuple_count` is the sum of the partition counts.

**Why the trie is identical:**

- Each subtree receives exactly the serial insert sequence for its keys,
  because sorted order restricted to a key range is that range sorted.
- The root grows by one push per new key, as the serial
  insert-at-the-end does.
- Every `Vec` therefore has the serial build's length and capacity.
- In fact a TreeTrie, capacities included, depends only on its set of
  distinct tuples: `Vec::insert` grows a child list exactly as `push`
  does, wherever it inserts. Only the key-range partitions and the
  one-at-a-time root pushes in key order are load-bearing; the
  per-partition sort is there for speed (every insert then appends).

**The serial code is untouched.** The parallel build lives beside it in
`implementation.rs` and calls `insert_into_children` unchanged.
TreeTrie's serial build allocates a fresh `Vec` per level per tuple
(`key_iter.collect()`), and the parallel build inherits that cost on
purpose, so `parallel:1` against `serial` measures the partitioning
process. That comparison includes one saving: sorting P partitions takes
about n·log₂P fewer comparisons than one sort of everything, so
`parallel:1` can beat `serial`.

### HashTrie: `kermit-ds/src/ds/hash_trie/build_mode.rs`

`HashTrieBuildMode { #[default] Serial, Parallel(Threads) }` has the same
axis values. Construction works like this:

- `HashTrie::from_tuples_with(header, config, mode, tuples)` is an inherent
  method and the one place that dispatches on the mode.
- `BuildModeRelation::from_tuples_with_build_mode` calls it with
  `HashTrieConfig::default()`.
- `ConfigurableRelation::from_tuples_with_config` keeps its serial body
  unchanged.

`Parallel(n)` runs these steps:

1. **Checks.** As for TreeTrie. Empty input returns `with_config(header,
   config)`.
2. **Partition.** Partition by the top log₂P bits of `H::hash(tuple[0])`,
   with P the next power of two at or above 4·N. The top bits are used
   because FxHash mixes its low bits poorly. Input positions are kept.
3. **Build.** `dispatch` over the partitions. Each partition:
   - creates a throwaway local root (`make_root(arity)`);
   - walks its tuples in input order. If the local root has no entry for
     `h = H::hash(tuple[0])`, it records `(position, h)`. Then it calls
     `insert_at(&mut local_root, 0, arity, tuple, config.load_factor)`;
   - once all its tuples are in, maps each record to its local slot
     (`index_of`);
   - consumes the root's table with a new crate-internal
     `HashTable::into_slots(self) -> Vec<Option<(u64, V)>>`, in bucket
     order.
4. **Assemble.**
   - k-way merge the partitions' record lists by position, using a
     `BinaryHeap`. Each list is already sorted.
   - For each record, take its slot and
     `entry_or_insert_with(hash, load_factor, || value)` it into the real
     root.
   - `tuple_count` is the number of input tuples (HashTrie counts the
     multiset).

**Why the trie is identical:**

- Each child is built by the same `insert_at` calls, in the same order, as
  in the serial build. That includes creating and unpruning `Singleton`s,
  which happens at the root for that key exactly as it would serially.
- The real root receives its new keys in serial order. Equal 64-bit hashes
  share a root entry, and they also share a partition, because the
  partition is a function of the hash.
- Moved tuples, chains and tables keep their capacities.

**Only one shared internal changes.** `HashTable::into_slots` is new, and
the serial path never calls it.

### Placement under the optimisation standard

- **Category.** BuildMode, axis `ds_build_mode`.
- **Who emits the axis.** The family that ran the build:
  `SortedTrieRelation::build_mode_axes` for TreeTrie, and the `HashHtj`
  family for HashTrie. It is present on every TreeTrie and HashTrie report
  from now on, as it is on ColumnTrie's.
- **Catalogue.** The "Parallel build" row moves to the landed table for
  both structures.

### `kermit` binary

- **`BuildChoice`.** A value parser replaces ColumnTrie's enum in
  `BuildChoices` and accepts `bulk | incremental | serial | parallel:N`.
  - `parallel:0`, `parallel:1025`, a bare `parallel` and `parallel:x` are
    usage errors that show the form `parallel:N` with 1 ≤ N ≤ 1024.
  - An N above the core count (up to 1024) is allowed and recorded,
    because oversubscription is a legitimate point on the curve.
- **`DsFlag::structures`.** Its `--ds-build` row depends on the value:
  - `bulk` and `incremental` belong to ColumnTrie.
  - `serial` and `parallel:N` belong to TreeTrie and HashTrie.

  So `-i column-trie --ds-build parallel:8` is rejected. With `-i all`,
  each structure takes the value only if it has it, and the others keep
  their defaults. `--ds-layout-seek` already behaves this way.
- **`DsChoices.build`.** It becomes `BuildModes { column, tree, hash }`,
  resolved from the one choice.
- **Execution cells.**
  - `SortedTrie::TreeTrie { seek, build }`. `SortedTrieRelation for
    TreeTrie<S>` gets `type BuildMode = TreeTrieBuildMode`; `build_with`
    calls `from_tuples_with_build_mode`; `build_mode_axes` emits the axis.
  - `Execution::HashHtj { hasher, pruning, config, build }`. The family's
    `build_relation` calls `HashTrie::from_tuples_with`.
- **Unchanged:**
  - `kermit join` still takes no `--ds-build`, since a build mode can't
    change answers.
  - `bench ds` reaches the mode through `RelationFamily::build_relation`,
    as ColumnTrie's mode does today.

### kermit-lab

- **`defaults.py`.** The structure-scoped registry gains
  `("ds_build_mode", "TreeTrie"): "serial"` and
  `("ds_build_mode", "HashTrie"): "serial"`.
- **`kl.load`.** It derives a numeric `threads` column: N for
  `parallel:N`, empty otherwise.
- **`kl.speedup(df, phase="insertion", baseline="serial")`.** A new preset
  and CLI subcommand. Per structure and relation, it reports against N:
  - speedup (baseline time divided by `parallel:N` time) and efficiency,
    with bootstrap confidence intervals from `bootstrap_ratio_ci`;
  - the ideal-scaling line;
  - a table of Karp–Flatt serial fractions.
- **Phase limits.** The existing `AXIS_PHASES` entry already confines
  `ds_build_mode` to `insertion` and `end_to_end`.

## Error handling and edge cases

- **Arity mismatch.** Checked on the calling thread before any spawn, with
  the serial panic messages, so workers never see bad input.
- **Empty input.** The same fresh trie as serial, with no threads spawned.
- **Little or skewed input.** Fewer tuples than partitions, a single
  distinct first key, or N greater than the available work: idle workers
  find the queue empty and return.
- **Arity 1.** The same three steps. TreeTrie's root children are leaves;
  HashTrie's root is a leaf table whose values are chains.
- **Panics in workers.** Bugs only. `thread::scope` joins every thread and
  re-raises the panic.
- **Memory.** The partitions hold each tuple's 24-byte `Vec` header plus an
  8-byte position until the build step consumes them. The input's own
  header array is freed after the partition step.

## Testing

1. **`morsel.rs` unit tests.**
   - `scatter` delivers every tuple exactly once, in input order within each
     partition, for N ∈ {1, 2, 3, 8} and morsels that don't divide the
     input evenly.
   - `dispatch` returns results in item order.
   - Empty input, and N greater than the number of items.
2. **Identity tests, at array level with capacities included.**
   - Inputs: seeded `Lcg` tuples; arities 1–4; duplicates; one dominant
     first key; empty and single-tuple inputs.
   - Check: `parallel:N` against serial, for N ∈ {1, 2, 3, 8}.
   - TreeTrie: a recursive helper compares keys, child-list lengths and
     capacities, `tuple_count` and `heap_size_bytes`.
   - HashTrie: compares node kinds, table capacities, every slot in order
     (hash and value), chain contents, order and capacity, `tuple_count`
     and `heap_size_bytes`. Covered for every Layout alias, including the
     pruned ones and the colliding `Mod10` hasher, which forces many keys
     to share root hashes.
   - Under Miri: one tiny case per structure with N = 2; the rest are
     `cfg_attr(miri, ignore = "...")`.
3. **Structure-level suites** through `BuiltWith<…, Parallel2>` aliases.
   - TreeTrie: `relation_trie_test_suite!` and `parquet_test_suite!`.
   - HashTrie: `hash_trie_test_suite!` and the Parquet suite.
     `BuiltWith` gains the `HashTrieIterable` forwarding impl it lacks
     today.
4. **Join level.** `define_multiway_join_test_suite_for_build_mode!` with a
   `Parallel2` provider, for TreeTrie and all four HashTrie aliases
   (`HashTrieSip`, `HashTrieFx`, `HashTrieSipPruned`, `HashTrieFxPruned`),
   under both optimisers.
5. **CLI.**
   - Parse errors, and per-structure rejections: `bulk` on TreeTrie,
     `parallel:N` on ColumnTrie.
   - `-i all` routing, and the report's `ds_build_mode` values.
   - A `bench run --ds-build parallel:2 --verify` smoke test.
6. **kermit-lab.**
   - The `threads` derivation and the `serial` default.
   - `kl.speedup` on a fixture.
   - The real-binary contract test gains a `--ds-build parallel:2` run.

## Scaling protocol

**What gets measured.**

- **Primary:** `insertion` per relation, via `bench ds` on single large
  relations. It gives the cleanest curve, with no join in the way.
- **Secondary:** `insertion` and `end_to_end`, via `bench run` on the
  sweep's workloads. They show what the speedup is worth inside a whole
  query.
- **Arms:** `serial`, then `parallel:1`, `:2`, `:4`, `:8` and `:16`, for
  TreeTrie and HashTrie at their default Layouts. One extra HashTrie curve
  runs with pruning on, since pruning changes the build work.
- **Relations:** sizes from below the crossover up to the largest that fit
  comfortably in RAM. Both unary and binary relations, because the
  predicted ceiling (the sequential assemble step) differs between them.

**How it runs.**

- One binary serves every arm, so a rebuild's codegen drift doesn't enter
  the comparison.
- At least 5 replicates per arm, with the arm order rotated each replicate.
- A quiet host: other sessions paused and their pause acknowledged, and
  `ps` sampled for 60 s before the run.
- A distinct `--name` per arm.
- Generated data goes in a scratch `XDG_CACHE_HOME`.
- `scripts/parallel-build-scaling.sh` runs the matrix. It writes reports to
  a run directory outside the repo, with an `env.txt` recording the CPU,
  the frequency governor and boost state, the kernel and the commit.
- `space` is measured once per arm as a check. It is identical by
  construction, so any difference is a bug.

**Analysis.**

- Speedup and efficiency against N, with confidence intervals.
- `parallel:1 ÷ serial`: the partitioning overhead net of the sort saving
  above, so it can come out below 1.
- The crossover size, where `parallel:N` first beats `serial`.
- The Karp–Flatt serial fraction e = (1/S − 1/N) / (1 − 1/N) per N.
  - A flat e means the sequential assemble step is the limit, as
    predicted; it should be highest for unary HashTries.
  - An e that rises with N points elsewhere: allocator contention, memory
    bandwidth or partition imbalance.
  - No timers go into the build.

**Threats to validity**, to be stated with the results:

- Boost clocks: one thread runs faster than each of eight.
- 16 threads are 8 cores with SMT.
- Workers allocate nodes concurrently, so allocator contention may limit
  scaling.
- The host has a single NUMA node, so morsel-driven parallelism's NUMA
  placement is not exercised.

## Out of scope

- Parallel joins (the probe side).
- ColumnTrie's parallel build.
- A parallel fill of HashTrie's root. If Karp–Flatt shows the root
  dominating, it could become a separate, non-identical mode, behind an
  amendment to the standard.
- Thread pinning and NUMA placement.
- Allocator changes.
- A persistent thread pool shared across builds. Each build spawns its own
  scoped threads, and that cost, tens of microseconds per thread, is part
  of the measured build.
- Criterion group names that encode axes.
- TreeTrie's per-level allocation in `insert_into_children`. Fixing it
  would be a serial-build change (Priority 6).

## Documentation

- **`docs/data-structures/parallel-build.md`** (new, shared by both tries
  like `seek-strategies.md`): the three steps, the identity argument as an
  invariant, the complexity of each step, and a worked micro-example.
- **`tree-trie.md` and `hash-trie.md`:** an "Optimizations → Build mode"
  section each.
- **`docs/specs/optimization-standard.md`:** the catalogue row moves to the
  landed table, and the BuildMode examples are updated.
- **`docs/specs/bench-report-schema.md`:** the new `ds_build_mode` values.
- **`BENCHMARKING.md`:** a "Scaling" section next to "Ablation", with the
  protocol and the distinct-`--name` rule.
- **`CLAUDE.md`:**
  - `BuildModeRelation` implementers;
  - the `--ds-build` row of `DsFlag::structures`;
  - a build-command example;
  - the BuildMode test-obligation line.

## Delivery

- **Plan 1:**
  - `morsel.rs` and its tests;
  - `TreeTrieBuildMode` and the TreeTrie identity tests;
  - the TreeTrie `BuiltWith` suites and join suites;
  - `BuildChoice`, `DsFlag` and the cells in the binary;
  - kermit-lab;
  - documentation for everything above.
- **Plan 2:**
  - `HashTable::into_slots`;
  - `HashTrieBuildMode`, `from_tuples_with` and the HashTrie identity tests;
  - `BuiltWith`'s `HashTrieIterable` forwarding and the HashTrie suites;
  - the `HashHtj` build field;
  - documentation.
- **Measurement:** the driver script, the run, and the published results.

## Coordination

- **#91 (radix partitioning)** reuses `scatter`. Whichever lands second
  builds on the other's `morsel.rs`.
- **#92 (lazy child expansion).** The build step moves finished children
  between threads, so values must be `Send`. `OnceCell` is `Send` (it is
  only `!Sync`), and no trie is ever shared between threads.
- **Other sessions** also edit `optimization-standard.md`'s catalogue.
  Conflicts there are textual and get resolved on merge.

## Acceptance

- [ ] `parallel:N` builds tries identical to serial at array level,
  capacities included:
  - [ ] for N ∈ {1, 2, 3, 8};
  - [ ] for both structures;
  - [ ] for every HashTrie Layout alias, including the pruned ones and
    `Mod10`.
- [ ] The structure-level and join-level suites pass for the new modes,
  and Miri is clean.
- [ ] The CLI, report axis and kermit-lab changes work as designed, and
  `bench run --ds-build parallel:2 --verify` passes.
- [ ] The diff leaves the serial bodies of `from_tuples`,
  `from_tuples_with_config`, `insert_at` and `insert_into_children`
  untouched.
- [ ] The scaling protocol has run, and its results are published with
  `env.txt`: speedup, efficiency, overhead, crossover and Karp–Flatt
  fractions.
