# Parallel builds (`--ds-build tree-trie=parallel:N`)

`TreeTrie` can be built on several threads (issue #94). The build is
morsel-driven in the sense of Leis et al. (*Morsel-Driven Parallelism*,
SIGMOD 2014): the input is cut into small morsels, and whichever thread is
free takes the next unit of work. It produces exactly the trie the serial
build produces, so only the build-timing metrics (`insertion`,
`end_to_end`) can move.

| Mode | `--ds-build` | `ds_build_mode` | Default |
|---|---|---|---|
| `TreeTrieBuildMode::Serial` | `tree-trie=serial` | `"serial"` | ✓ |
| `TreeTrieBuildMode::Parallel(n)` | `tree-trie=parallel:N` | `"parallel:N"` | |

`N` counts every thread the build uses, the calling one included. So
`parallel:1` runs the parallel code on one thread, and against `serial` it
measures what partitioning costs, net of one saving: sorting P partitions
takes about n·log₂P fewer comparisons than one sort of everything, so
`parallel:1` can beat `serial`. N ranges from 1 to 1024 (`Threads::MAX`).

The shared steps live in `kermit-ds/src/morsel.rs` (`scatter`, `dispatch`).
The TreeTrie build is `TreeTrie::build_parallel` in
`kermit-ds/src/ds/tree_trie/implementation.rs`.

## The three steps

```text
parallel_build(tuples, N):
    check arities                                   // the serial checks, on the caller
    splitters = quantiles of a sample of first keys  // duplicates kept; aiming at P = 4·N partitions
    partitions = scatter(tuples, morsels of 16 384)  // step 1, N workers
        // tuple t goes to partition_point(splitters, s <= t[0])
    built = dispatch(partitions):                    // step 2, N workers
        sort the partition; insert its tuples one at a time
        -> (its top-level nodes, its distinct tuples)
    root = []                                        // step 3, the caller
    for (nodes, count) in built, in key order:
        push each node onto root, one at a time
```

- **Partition.** Workers take morsels from a mutex-guarded queue and move
  each tuple, without copying it, into its partition's bucket for that
  morsel. A partition is its buckets in morsel order, so it lists its tuples
  in input order.
- **Build.** Workers take whole partitions from the same kind of queue.
  There are usually more partitions than threads (fewer only when heavy keys
  merge splitters), so a worker that drew a small partition takes another.
- **Assemble.** The calling thread pushes each partition's top-level nodes
  onto the root, in partition order, which is key order.

## Invariant: the parallel trie is the serial trie

Every `parallel:N` build is identical to the serial build of the same
tuples: the same nodes, the same `Vec` capacities, the same `tuple_count`
and `heap_size_bytes`. `parallel_builds_are_identical_to_serial` pins it for
N ∈ {1, 2, 3, 8}.

- Splitters are first keys, so all tuples sharing a first key share a
  partition, and the partitions are first-key ranges, in order.
- Sorted order restricted to a key range is that range sorted, so each
  subtree receives exactly the serial build's sequence of inserts.
- A child list grows one insert at a time in both builds, and `Vec::insert`
  grows a list exactly as `push` does, so capacities match. In fact a
  TreeTrie, capacities included, depends only on its distinct tuples. The
  per-partition sort is there for speed (every insert then appends), not
  for identity.
- The root grows by one push per first key, as the serial build's
  insert-at-the-end does. `extend` or `append` would reserve in bulk and
  break this, so the assemble step pushes.

The splitters, the partition count and the morsel size only decide how work
is spread. None of them can change the trie.

Since the trie cannot show which build made it, each parallel build records
its thread count and partition sizes in a thread-local. kermit-ds's own tests
read the record directly. `kermit`'s tests read it through
`kermit_ds::test_hooks` (the `test-hooks` feature), where
`tree_trie_families_build_with_their_mode` checks that the family's mode
reaches the real build on every route a relation is built.

## Complexity

With `n` tuples of arity `a`, `k` distinct first keys and `N` threads:

| Step | Work | Runs on |
|---|---|---|
| Checks and sampling | O(n), plus sorting a sample of 128–256 keys per partition (fewer for small inputs) | the calling thread |
| Partition | O(n log P) | N workers |
| Build | O(n · a · log n): sorting and inserting, split across partitions | N workers |
| Assemble | O(k) moves | the calling thread |

The sequential share is the checks, the sampling and the assemble step.
With few tuples per first key (`k` close to `n`), the assemble step is a
larger share and the speedup falls. The sample keeps duplicate keys, so the
splitters share out tuples rather than distinct keys. Still, a key's tuples
cannot be split: one dominant first key fills one partition, which caps the
build step at one worker's speed. The build step hands out partitions first
come, first served. Largest-first would balance a little better (about 4%
at 8 and 16 threads on uniform keys, in simulation), so that is a known,
small contributor to the Karp–Flatt fraction.

Two more costs grow with N, both by design. Each parallel step starts its own
workers, so a build pays 2·(N − 1) thread starts and joins. And the calling
thread merges the per-morsel buckets into partitions: (n / 16 384) · 4·N of
them. Both raise the Karp–Flatt fraction as N grows. In the other direction,
the per-partition sort saving (see `parallel:1` above) can make the speedup
superlinear, and the fraction then goes negative.

## Worked micro-example

`parallel:2` over `[3,1] [1,2] [2,9] [1,1]` aims at 8 partitions. The sorted
sample is `1 1 2 3`; its quantiles for 8 partitions are `1 1 1 2 2 3 3`,
which merge to the splitters `[1, 2, 3]`, so there are four partitions:

| Partition | First keys | Tuples (input order) | After sorting and inserting |
|---|---|---|---|
| 0 | `< 1` | — | — |
| 1 | `1` | `[1,2] [1,1]` | `1 → {1, 2}` |
| 2 | `2` | `[2,9]` | `2 → {9}` |
| 3 | `≥ 3` | `[3,1]` | `3 → {1}` |

Pushing the nodes of partitions 1, 2 and 3 in order gives the root `1, 2, 3`:
the trie `from_tuples` builds from the same input.

## Measuring

See `BENCHMARKING.md`, "Scaling: measuring a parallel build", and kermit-lab's
`kl.speedup_table` / `kermit-lab speedup`. Compare build modes within one
binary. The protocol is the spec's "Scaling protocol"; its TreeTrie half is
recorded below.

### Scaling result: TreeTrie (2026-10-05)

These are **glibc** numbers. The binary predates #112, which made jemalloc the
binary's allocator. At 10⁷ tuples the glibc curve is bound by the allocator,
not by the build (see the reading below and "Under jemalloc").

**Setup.**
- **Binary:** built at d3c7945 (sha256 `7713fbf56e851b51…`).
- **Host:** AMD Ryzen 7 7700X, 8 cores and 16 threads (SMT), 32 MB L3, one
  NUMA node; `amd-pstate-epp` with boost on; glibc malloc; Linux 7.1.5.
- **Arms:** `--ds-build tree-trie=serial` and `parallel:{1,2,4,8,16}`, at the
  default Layout (galloping seek).
- **Replicates:** five per arm, arm order rotated one place per replicate,
  replicates outermost.
- **Measurement:** `bench ds -m insertion` at `--sample-size 10
  --measurement-time 3 --warm-up-time 1`, plus `space` in replicate 1.
- **Inputs:**
  - Synthetic relations of 10³–10⁷ tuples. A binary relation draws `a` from
    `U[0, n/10)` and `b` from `U[0, n)`, about ten tuples per first key. A
    unary relation draws from `U[0, n)`, about 63 % distinct.
  - Two WatDiv relations from #91's radix A/B: `friendof` (4.5M tuples,
    39,781 first keys, grouped by first key) and `price` (240K tuples,
    sorted, every key distinct).
- **Statistics:** speedup is mean `serial` over mean `parallel:N`, 5 runs a
  side, with the 95 % bootstrap CI (`kl.speedup_table`).
- **Quiet host:** every invocation waited for one. Contention was judged on
  the load *besides* the benchmark, since a parallel arm alone keeps up to 16
  cores busy. The 9 invocations that overlapped a peer session's Miri were
  re-run.
- **Run directory:** `kermit-bench-runs/tree-trie-scaling-2026-10-05/`
  (README, `env.txt`, `analysis.txt`, `figures/`).

HashTrie's half needs plan 2 and will run on another binary, so compare
speedups *within* a structure, never absolute times across the two halves.

`space` was identical across all six arms for all 12 relations, from
32,768 bytes to 470,503,040, as the invariant above requires.

| Relation | `serial` | `:1` | `:2` | `:4` | `:8` [95 % CI] | `:16` |
|---|---|---|---|---|---|---|
| unary 10³ | 0.033 ms | 0.73 | 0.38 | 0.27 | 0.17 [0.16, 0.17] | 0.10 |
| unary 10⁴ | 0.54 ms | 0.86 | 1.00 | 0.96 | 0.91 [0.90, 0.92] | 0.63 |
| unary 10⁵ | 7.8 ms | 0.89 | 1.33 | 1.64 | 1.87 [1.85, 1.88] | 1.14 |
| unary 10⁶ | 177 ms | 1.04 | 1.99 | 2.85 | 3.37 [3.15, 3.62] | 2.37 |
| unary 10⁷ | 2.18 s | 0.67 | 1.11 | 1.65 | 1.77 [1.75, 1.79] | 1.87 |
| binary 10³ | 0.044 ms | 0.81 | 0.49 | 0.36 | 0.23 [0.22, 0.23] | 0.13 |
| binary 10⁴ | 0.72 ms | 0.95 | 1.16 | 1.49 | 1.36 [1.33, 1.39] | 0.89 |
| binary 10⁵ | 9.5 ms | 0.98 | 1.52 | 2.07 | 2.56 [2.53, 2.58] | 1.65 |
| binary 10⁶ | 181 ms | 1.09 | 2.07 | 2.92 | 3.36 [3.25, 3.48] | 2.62 |
| binary 10⁷ | 2.89 s | 1.01 | 1.67 | 1.78 | 1.87 [1.85, 1.89] | 1.92 |
| `price` | 17 ms | 0.74 | 1.52 | 2.23 | 2.54 [2.29, 2.97] | 1.10 |
| `friendof` | 516 ms | 0.91 | 1.69 | 2.87 | 3.99 [3.91, 4.05] | 2.22 |

The `:1` column is the partitioning overhead net of the per-partition sort
saving. Efficiency is speedup ÷ N: about 1.0 at `:2` on the 10⁶ relations,
0.42 there at `:8`, and 0.50 for `friendof` at `:8`.

| Crossover (smallest size whose CI is above 1) | `:1` | `:2` | `:4` | `:8` | `:16` |
|---|---|---|---|---|---|
| binary | 10⁶ | 10⁴ | 10⁴ | 10⁴ | 10⁵ |
| unary | 10⁶ | 10⁵ | 10⁵ | 10⁵ | 10⁵ |

| Karp–Flatt e | `:2` | `:4` | `:8` | `:16` |
|---|---|---|---|---|
| unary 10⁵ | 0.51 | 0.48 | 0.47 | 0.87 |
| unary 10⁶ | 0.01 | 0.13 | 0.20 | 0.38 |
| unary 10⁷ | 0.80 | 0.48 | 0.50 | 0.50 |
| binary 10⁵ | 0.32 | 0.31 | 0.30 | 0.58 |
| binary 10⁶ | −0.04 | 0.12 | 0.20 | 0.34 |
| binary 10⁷ | 0.20 | 0.42 | 0.47 | 0.49 |
| `price` | 0.32 | 0.26 | 0.31 | 0.91 |
| `friendof` | 0.19 | 0.13 | 0.14 | 0.42 |

Inside whole queries (`bench run lubm-reference`, TreeTrie with LFTJ, the
geometric mean over the 14 queries of each query's speedup):

| | `:1` | `:2` | `:4` | `:8` | `:16` |
|---|---|---|---|---|---|
| `insertion` | 0.96 | 1.23 | 1.48 | 1.34 | 0.89 |
| `end_to_end` | 1.12 | 1.39 | 1.60 | 1.49 | 1.00 |

**Reading.**
- **Peak:** the build is fastest at about 10⁶ tuples: 3.4× on 8 threads,
  and 4.0× on `friendof`, whose input arrives grouped by first key.
- **Plateau at 10⁷: glibc's allocator (#112).** Both arities flatten to
  about 1.8–1.9×. Every insert frees its input tuple at the leaf, on a
  worker thread, but the calling thread allocated those tuples (in
  Criterion's setup clone), so glibc returns all 10⁷ chunks to the main
  arena. Profiled on binary 10⁷ at `:16` (2026-10-06):
  - The dispatch phase (sort, insert, frees) takes 1.44 s of a ~1.5 s
    build. Scatter takes 27 ms, the merge 16 ms.
  - The workers spend 52 % of their cycles in glibc's fastbin push (a
    contended `lock cmpxchg`) and sleep on the arena mutex 68 % of the
    time.
  - The calling thread, worker 0, spends 90 % of its dispatch cycles
    consolidating the freed chunks under that mutex.

  The same binary under jemalloc reaches 7.5× (binary) and 4.5× (unary)
  at `:16`, so neither DRAM traffic nor a sequential step is the cap.
  Inserting allocates nothing per level: `collect()` reuses the tuple's
  buffer.
- **`parallel:1` at 0.67 for unary 10⁷ is a glibc timing artefact.** A
  unary build makes no small allocation, so a serial build's 10⁷ frees
  wait in glibc's fastbins and are consolidated in Criterion's *untimed*
  setup. `parallel:1`'s per-partition allocations trigger that
  consolidation inside the timed build. Both spend the same CPU per step,
  and under jemalloc unary `:1` is 0.90. Unary 10⁷ `serial` therefore
  understates the build by about 1 s here.
- **Assemble is not the cap at these sizes.**
  - Unary (k ≈ 0.63 n) and binary (k = n / 10) reach the same speedup at 10⁶
    and at 10⁷.
  - At 10⁷ the assemble step moves at most about 200 MB of nodes, an
    estimated ≤ 0.1 s of a 1.2 s build.
  - Binary 10⁷'s Karp–Flatt rises with N, which the spec reads as a cost that
    grows with N rather than a fixed sequential step.
- **16 threads:** slower than 8 everywhere but 10⁷, where both are bound
  by glibc (above). The extra 8 threads are SMT siblings, not cores.
- **Small inputs:** below the crossover, the 2·(N − 1) thread starts cost
  more than the whole serial build.
- **Open question:**
  - On `lubm-reference`, `parallel:1` builds about 4 % slower than `serial`,
    yet `end_to_end` (one build plus one query) is 12 % faster on every one of
    the 14 queries.
  - The trie is identical, so the suspect is heap placement: workers allocate
    nodes from their own arenas, apart from the freed input tuples. But `:1`
    starts no worker threads, so glibc's deferred consolidation (above) is
    the likelier suspect. Neither is tested, and nor is the effect under
    jemalloc.
  - If an `iteration` A/B confirms it, a build mode can move `iteration`,
    which the BuildMode rule assumes it cannot. That A/B has not been run.

**Threats to validity** (the spec's list): boost clocks favour one thread;
16 threads are 8 cores with SMT; the allocator, which at 10⁷ under glibc
sets the curve (above); a single NUMA node; the synthetic keys are uniform,
so partition imbalance is barely exercised.

### Under jemalloc (#112, 2026-10-06)

The `kermit` binary links jemalloc by default since #112. These rows compare
the #112 source built both ways, with the protocol above at `serial`, `:8`
and `:16` (speedup over the same build's `serial`; the median of each step's
10 builds, then the median over replicates):

| Relation | Build | `serial` | `:8` | `:16` |
|---|---|---|---|---|
| binary 10⁷ | `--no-default-features` (glibc) | 2.90 s | 1.88 | 1.89 |
| binary 10⁷ | default (jemalloc) | **2.39 s** | **5.08** | **7.21** |
| unary 10⁷ | `--no-default-features` (glibc) | 2.20 s | 1.74 | 1.87 |
| unary 10⁷ | default (jemalloc) | **2.07 s** | **4.19** | **4.59** |

- **Replicates:** three per cell, less nine invocations that overlapped other
  sessions' Miri and test runs (other load above two cores). That leaves
  binary 10⁷'s glibc `:8` and `:16` with one replicate each and five cells
  with two.
- **Cross-check:** the same arms with jemalloc and mimalloc preloaded into the
  2026-10-05 binary (three replicates each) agree. jemalloc gives 7.46× and
  4.53× at `:16`, mimalloc 6.59× and 4.19×.
- **Run directory:** `kermit-bench-runs/tree-trie-profile-2026-10-06/`
  (`FINDINGS.md`; `analysis.txt` holds the preload matrix, `analysis-verify.txt`
  these rows).
- **Serial got faster too:** 17 % less time on binary 10⁷. glibc spent ~17 %
  of the serial step's cycles in its free-chunk bookkeeping (`unlink_chunk`,
  `_int_malloc`, `malloc_consolidate`).
- **What stops unary near 4.6×** (8 → 16 adds little) is not measured.
  The candidate is the sequential assemble step, about 6.3M node pushes onto
  a 268 MB root, which the design predicted for unary inputs.

## See also

- [`TreeTrie`](./tree-trie.md), whose Optimizations table lists the mode.
- The design: [`docs/specs/2026-10-05-parallel-build-design.md`](../specs/2026-10-05-parallel-build-design.md).
