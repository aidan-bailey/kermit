# Parallel builds (`--ds-build tree-trie=parallel:N`, `hash-trie=parallel:N`, `hash-trie=presized:N`)

`TreeTrie` and `HashTrie` can each be built on several threads (issue
#94). The build is morsel-driven in the sense of Leis et al. (*Morsel-Driven
Parallelism*, SIGMOD 2014): the input is cut into small morsels, and
whichever thread is free takes the next unit of work. `parallel:N`
produces exactly the trie the serial build produces under the same config,
so only the build-timing metrics (`insertion`, `end_to_end`) can move.
HashTrie's presized build (`hash-trie=presized:N`, which requires
`--ds-config root-capacity=tuples`) builds an equivalent trie whose root
keys may sit in other buckets, so `iteration` is measured for it too
(Amendment 2).

| Mode | `--ds-build` | `ds_build_mode` | Default |
|---|---|---|---|
| `TreeTrieBuildMode::Serial` | `tree-trie=serial` | `"serial"` | ✓ |
| `TreeTrieBuildMode::Parallel(n)` | `tree-trie=parallel:N` | `"parallel:N"` | |
| `HashTrieBuildMode::Serial` | `hash-trie=serial` | `"serial"` | ✓ |
| `HashTrieBuildMode::Parallel(n)` | `hash-trie=parallel:N` | `"parallel:N"` | |
| `HashTrieBuildMode::Presized(n)` | `hash-trie=presized:N` (requires `root-capacity=tuples`) | `"presized:N"` | |

`N` counts every thread the build uses, the calling one included. So
`parallel:1` runs the parallel code on one thread. For TreeTrie, against
`serial` it measures what partitioning costs, net of one saving: sorting P
partitions takes about n·log₂P fewer comparisons than one sort of
everything, so `parallel:1` can beat `serial`. HashTrie has no such saving
(see [HashTrie](#hashtrie)). N ranges from 1 to 1024 (`Threads::MAX`).

The shared steps live in `kermit-ds/src/morsel.rs` (`scatter`, `dispatch`).
The TreeTrie build is `TreeTrie::build_parallel` in
`kermit-ds/src/ds/tree_trie/implementation.rs`.
The HashTrie builds are `parallel::fill_root` (`parallel:N`) and
`parallel::fill_presized_root` (`presized:N`), both in
`kermit-ds/src/ds/hash_trie/parallel.rs`.

## TreeTrie

### The three steps

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

### Invariant: the parallel trie is the serial trie

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

### Complexity

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

### Worked micro-example

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

## HashTrie

HashTrie has two parallel builds, each its own mode:

- `parallel:N` is the **exact build**: the radix build of #91
  ([`hash-trie.md`](./hash-trie.md#build-modes), `radix.rs`) with its first
  two steps on N threads, under every root capacity. It builds the serial
  trie, bucket for bucket.
- `presized:N` is the **presized build** (the paper's, §3.3.2), described
  [below](#the-presized-build-presizedn). It requires
  `root-capacity=tuples` (#88), since it fills a root presized from the
  tuple count, and it builds an equivalent trie.

### The three steps (exact build)

```text
parallel_build(tuples, N):
    check arities                                     // the serial check, on the caller
    if no tuples: return the empty root               // no worker started
    P = 4·N rounded up to a power of two; b = log₂ P
    partitions = scatter(tuples, morsels of 16 384)   // step 1, N workers
        // tuple t goes to partition H(t[0]) >> (64 − b), with its position
    lists = dispatch(non-empty partitions):            // step 2, N workers
        build a scratch root by insert_at, in input order,
            noting (position, hash) whenever it gains a key
        take its entries out in that order
        -> [(first position, hash, subtrie or chain)]
    root = empty                                       // step 3, the caller
    k-way merge the lists by first position:
        insert each (hash, value) into root
```

- **Partition.** As for TreeTrie, but by the top b bits of the first
  attribute's hash (the radix build's rule; FxHash mixes its low bits
  poorly), so there are no splitters to sample, and each tuple keeps its
  input position.
- **Build.** A worker builds a whole partition into a scratch root of the
  real root's kind, by the serial build's own `insert_at`, then moves the
  scratch root's entries out in the order their keys arrived.
- **Merge.** The calling thread inserts every entry into the real root in
  the order its key first appeared in the input. Each list is already in
  that order, so a k-way merge over at most P lists suffices. (The radix
  build, single-threaded, sorts its entries instead.)

### Invariant (exact build)

Every `parallel:N` build is identical to the serial build of the same
tuples, bucket for bucket and capacity for capacity, under every Layout,
load factor and root capacity. `parallel_builds_the_serial_trie_*` in
`parallel.rs` pins it
for N ∈ {1, 2, 3, 8}, with morsels of 7 tuples and of 16 384 (three
morsels, two of them full, in `…_on_large_and_skewed_inputs`).

- A table's final layout depends only on the order in which its *new* keys
  arrive, because `HashTable::entry_or_insert_with` returns an existing
  entry before its resize check.
- `scatter` keeps each partition in input order, so each root key's subtrie
  is built by the serial build's `insert_at` calls, on the same tuples in
  the same order. That covers every `Singleton` and unprune, every chain,
  every capacity and, under lazy expansion, every pending list.
- The merge inserts the root's keys in first-appearance order, the serial
  order. Equal 64-bit hashes share a root entry and also a partition,
  because the partition is a function of the hash.

Finished subtries move from a worker to the caller, so every Layout's nodes
are `Send` (the policies' associated types are bounded so); lazy tries are
still `!Sync`, and no trie is ever shared between threads. The record of
parallel builds works as TreeTrie's does, except that it also lists the
empty partitions the build skips:
`hash_trie_families_build_with_their_parallel_mode` reads it through
`kermit_ds::test_hooks::take_hash_trie_parallel_builds`.

### Complexity (exact build)

With `n` tuples of arity `a`, `D` distinct first-attribute hashes and `N`
threads (`P` = 4·N rounded up to a power of two):

| Step | Work | Runs on |
|---|---|---|
| Checks | O(n) | the calling thread |
| Partition | O(n) hashes and moves | N workers |
| Build | O(n · a) expected probes and inserts, split across partitions, plus O(D) to take the entries out | N workers |
| Merge | O(D log P) heap operations and D root inserts (expected O(1) each, plus the root's resizes) | the calling thread |

The sequential share is the checks, the merge of per-morsel buckets
((n / 16 384) · P of them) and the merge into the root. It grows with D/n:
when the first attribute is a key (D = n) every root insert happens on one
thread, and a unary relation is the extreme, since its whole trie is the
root. Thread starts are as for TreeTrie, 2·(N − 1) per build. Partition
sizes follow the hash, so balance comes from P > N rather than from
splitters, and a dominant first key still fills one partition.

`parallel:1` against `serial` is what partitioning costs on one thread, as
`radix:2` measures it: #91 found the radix build slower than `serial` on
inputs grouped by their first attribute, and faster only on shuffled ones.
There is no sort saving to offset it, unlike TreeTrie's, so expect
`parallel:1` to be slower than `serial` on grouped inputs.

### Worked micro-example (exact build)

`parallel:1` over `[3,1] [1,2] [2,9] [1,1]` (positions 0–3) has P = 4
partitions, chosen by the top two bits of each first attribute's hash. Say
those bits are `10` for 3 and 1, and `01` for 2:

| Partition | Tuples @ position | Scratch root's keys, by arrival | Entries out |
|---|---|---|---|
| 1 (`01`) | `[2,9]`@2 | 2 | (2, h(2), {9}) |
| 2 (`10`) | `[3,1]`@0, `[1,2]`@1, `[1,1]`@3 | 3, 1 | (0, h(3), {1}), (1, h(1), {2, 1}) |

The merge takes positions 0, 1 and 2 in turn, inserting 3, then 1, then 2:
the order in which the serial build first meets them, so the root's buckets
are the serial root's.

### The presized build (presized:N)

When the root is presized, its capacity is known before any tuple arrives,
so workers can fill it directly. The root is cut into fixed regions of
`REGION_BUCKETS` = 4096 buckets (the whole table if it is smaller), and each
partition is a contiguous run of regions.

```text
presized:N (requires root-capacity=tuples):
    check arities; no tuples → the empty root, no worker started
    root = 2^p buckets, p = config.root_log2_capacity(n)    // #88; never grows here
    P = 4·N rounded up to a power of two, capped at the region count
    partitions = scatter(tuples, morsels of 16 384)        // step 1, N workers
        // tuple t goes to partition home_bucket(H(t[0]), p) >> (p − log₂ P),
        // with its position: partition k is run k of the root
    root.with_runs(P, REGION_BUCKETS, |runs|               // step 2, N workers
        dispatch over (partition k, run k):
            for each tuple, in input order:
                insert_at_{leaf,inner}_root_in_run(run k, tuple)
                    found or inserted → below the root, insert_at as usual
                    probe reached its region's end → defer (position, tuple))
    sort the deferred tuples by position                  // step 3, the caller
    insert each by the ordinary insert_at
```

The root step (`insert_at_leaf_root_in_run` / `insert_at_inner_root_in_run`
in `implementation.rs`) is `insert_at`'s depth-0 arm, decision for
decision, probing one `BucketRun` instead of the whole table. A `BucketRun`
(`hash_table.rs`) probes from a key's home bucket to the end of the home's
region and never wraps or grows. The deferred tail is kermit's mechanism:
the paper does not say how a probe that crosses a partition's end is
handled.

**Why it is correct** (Amendment 2's equivalence, pinned by
`presized_parallel_builds_are_*` in `parallel.rs`):

- **Linear probing stays valid.** Every bucket between a key's home and its
  slot is occupied: inside a region by construction, and for a deferred key
  because the tail probes normally from its home. Nothing is deleted.
- **Subtries are serial's.** A key's tuples reach `insert_at` in input
  order, all inside its region or all in the tail: once a key is deferred
  it cannot be found in its region, so its later tuples are deferred too.
  Every subtrie, chain, singleton and pending list is built by the serial
  build's calls, in its order.
- **The root matches serial in everything but slots.** Serial presizes the
  root the same way and inserts its keys in input order without growing.
  The parallel root equals sequential insertion in another order (each
  region's keys, then the deferred keys), so by linear probing's order
  independence (Knuth, TAOCP §6.4) it occupies the same buckets with the
  same total displacement. Keys sit in other slots only where a deferred
  key and a later region's key compete for a bucket.
- **The same for every N.** Regions have a fixed size and all of a
  region's tuples fall in one partition, in input order, so what is
  deferred does not depend on N, and the tail is sorted by position.
  `presized_parallel_builds_are_the_same_for_every_n_on_dense_roots`
  checks this where N changes how regions are grouped into partitions.

**Complexity.** The scratch roots and the merge of D keys are gone:

| Step | Work | Runs on |
|---|---|---|
| Checks, sizing | O(n) | the calling thread |
| Partition | O(n) hashes and moves | N workers |
| Region inserts, and every subtrie below | O(n · a) expected, one insert per tuple | N workers |
| Tail | the deferred tuples, sorted and inserted | the calling thread |

The deferred share grows with the load factor and shrinks with the region
size; at 4096-bucket regions it is a small fraction of n. Each region's
buckets stay within one worker's cache.

**Worked example.** `presized:2` over `[1,a] [2,b] [3,c] [1,d]` (positions
0–3) under `root-capacity=tuples`: n = 4 at 70 % gives an 8-bucket root.
Real regions are 4096 buckets; for the example, take 4-bucket regions, so
there are 2 regions and 2 runs. Say the keys' home buckets are 3 for key 1,
3 for key 2, and 5 for key 3.

| Run | Tuples (position) | Step | Bucket |
|---|---|---|---|
| 0 (buckets 0–3) | `[1,a]`@0 | key 1 is new: its child table, then `a` | 3 |
| 0 | `[2,b]`@1 | key 2 probes 3 (taken), and 4 is past its region's end | deferred |
| 1 (buckets 4–7) | `[3,c]`@2 | key 3 is new | 5 |
| 0 | `[1,d]`@3 | key 1 is found at 3: `d` goes into its child | 3 |

The tail then inserts `[2,b]` by ordinary probing from its home, bucket 3.
Bucket 3 is taken and bucket 4 is free, so key 2 lands at 4. Serial would
have put key 1 at 3, key 2 at 4 and key 3 at 5: the same occupied buckets,
and here even the same slots. They differ only when a deferred key and a
later region's key compete for the same bucket.

## Measuring

See `BENCHMARKING.md`, "Scaling: measuring a parallel build", and kermit-lab's
`kl.speedup_table` / `kermit-lab speedup`. Compare build modes within one
binary. For HashTrie, `kl.speedup_table(df, baseline="radix:K")`
measures the parallel build against the single-threaded partitioned one.
The presized curve compares `presized:N` with `serial`, both under
`root-capacity=tuples`, at load factors 0.8 (the paper's) and 0.7
(kermit's default), and measures `iteration` as well, since the presized
build may place root keys in other buckets.
The protocol is the spec's "Scaling protocol"; its TreeTrie half is
recorded below.

### Scaling result: TreeTrie (2026-10-05)

These are **glibc** numbers. The binary predates #112, which made jemalloc the
binary's allocator. At 10⁷ tuples the glibc curve is bound by the allocator,
not by the build (see the reading below and "TreeTrie under jemalloc").

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
  - If an `iteration` A/B confirms it, a build mode can move `iteration`
    even when it builds the identical trie, which is why Amendment 2 has
    `iteration` measured per mode. That A/B has not been run.

**Threats to validity** (the spec's list): boost clocks favour one thread;
16 threads are 8 cores with SMT; the allocator, which at 10⁷ under glibc
sets the curve (above); a single NUMA node; the synthetic keys are uniform,
so partition imbalance is barely exercised.

### Scaling result: TreeTrie under jemalloc (#112, 2026-10-06)

The `kermit` binary links jemalloc by default since #112. These TreeTrie
rows compare the #112 source built both ways, with the protocol above at
`serial`, `:8` and `:16` (speedup over the same build's `serial`; the median
of each step's 10 builds, then the median over replicates):

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

### Scaling result: HashTrie, presized and grown (2026-10-06)

The presized build against the default config, on the jemalloc binary of #112.
An earlier HashTrie run on glibc measured the default config alone
(`kermit-bench-runs/hash-trie-scaling-2026-10-06/`); its numbers are not
comparable with these, since glibc capped every parallel build.

**Setup.**
- **Binary:** origin/master b882bd6 (#88, the presized build, #112), default
  features, so jemalloc 5.3.1 (sha256 `540ef597af70c26b…`).
- **Host, inputs, statistics and quiet gate:** as for TreeTrie above, with the
  same 12 relations. Contention was judged on the load besides the benchmark;
  no invocation overlapped any, so none was re-run.
- **Curves:** `--ds-config root-capacity=tuples,load-factor=0.8` (the paper's
  sizing), `root-capacity=tuples,load-factor=0.7`, and
  `root-capacity=grow,load-factor=0.7` (the default config). Each has arms
  `serial` and `parallel:{1,2,4,8,16}`, default Layout. At b882bd6 the
  presized arms were spelled `--ds-build hash-trie=parallel:N` under
  `root-capacity=tuples`; since 2026-10-07 that build is
  `--ds-build hash-trie=presized:N` (still under `root-capacity=tuples`,
  which is now required). The `grow` curve's `parallel:N` arms were the
  exact build then and still are, so only the `tuples` curves' reports carry
  the old spelling: `ds_build_mode: parallel:N` beside
  `ds_config_root_capacity: tuples`.
- **Measurement:** `bench ds -m insertion iteration` at `--sample-size 10
  --measurement-time 3 --warm-up-time 1`, plus `space` in replicate 1. Five
  replicates; within each, per relation, the three curves and the arms in an
  order rotated one place per replicate, so the curves share the host's drift.
- **Run directory:** `kermit-bench-runs/hash-trie-presized-scaling-2026-10-06/`
  (README, `env.txt`, `analysis.txt`, `figures/`).

`space` was identical across the arms of every curve for all 12 relations, as
Amendment 2 requires. Presizing by the tuple count costs heap where first keys
repeat: 1.54× the grown trie's bytes on `friendof` and 1.3–1.5× on the binary
relations, but 1.00× on unary 10⁷ and `price`, whose grown roots reach the
same size.

Insertion, presized at load factor 0.8 (speedup over the same curve's
`serial`):

| Relation | `serial` | `:1` | `:2` | `:4` | `:8` [95 % CI] | `:16` |
|---|---|---|---|---|---|---|
| unary 10³ | 0.021 ms | 0.74 | 0.37 | 0.14 | 0.12 [0.11, 0.12] | 0.06 |
| unary 10⁴ | 0.228 ms | 0.73 | 0.70 | 0.45 | 0.44 [0.43, 0.45] | 0.42 |
| unary 10⁵ | 3.3 ms | 0.85 | 1.00 | 1.30 | 1.43 [1.37, 1.49] | 1.61 |
| unary 10⁶ | 80.5 ms | 1.01 | 1.32 | 1.72 | 2.05 [2.02, 2.08] | 2.14 |
| unary 10⁷ | 939 ms | 0.99 | 1.47 | 2.12 | 2.46 [2.43, 2.49] | 2.68 |
| binary 10³ | 0.049 ms | 0.89 | 0.59 | 0.24 | 0.22 [0.21, 0.23] | 0.15 |
| binary 10⁴ | 0.600 ms | 0.89 | 0.86 | 0.84 | 0.79 [0.78, 0.79] | 0.77 |
| binary 10⁵ | 9.4 ms | 0.96 | 1.30 | 1.65 | 2.35 [2.09, 2.60] | 2.30 |
| binary 10⁶ | 206 ms | 1.40 | 1.91 | 2.59 | 3.21 [3.17, 3.26] | 3.67 |
| binary 10⁷ | 2.78 s | 1.10 | 1.77 | 2.79 | 3.88 [3.71, 4.07] | 4.86 |
| `price` | 33.9 ms | 0.99 | 1.20 | 1.52 | 1.95 [1.93, 1.96] | 2.22 |
| `friendof` | 413 ms | 0.89 | 1.22 | 1.64 | 1.92 [1.90, 1.95] | 2.18 |

Insertion, the default config (`grow`, load factor 0.7):

| Relation | `serial` | `:1` | `:2` | `:4` | `:8` [95 % CI] | `:16` |
|---|---|---|---|---|---|---|
| unary 10³ | 0.024 ms | 0.36 | 0.22 | 0.13 | 0.10 [0.10, 0.11] | 0.06 |
| unary 10⁴ | 0.319 ms | 0.38 | 0.43 | 0.32 | 0.32 [0.32, 0.33] | 0.29 |
| unary 10⁵ | 4.4 ms | 0.48 | 0.58 | 0.68 | 0.72 [0.70, 0.74] | 0.72 |
| unary 10⁶ | 82.6 ms | 0.56 | 0.75 | 0.90 | 0.99 [0.97, 1.00] | 1.00 |
| unary 10⁷ | 1.46 s | 0.49 | 0.71 | 0.92 | 1.03 [1.02, 1.03] | 1.07 |
| binary 10³ | 0.045 ms | 0.68 | 0.42 | 0.26 | 0.20 [0.20, 0.20] | 0.12 |
| binary 10⁴ | 0.604 ms | 0.75 | 0.84 | 0.72 | 0.80 [0.80, 0.80] | 0.74 |
| binary 10⁵ | 7.8 ms | 0.82 | 1.12 | 1.57 | 2.15 [2.13, 2.17] | 2.45 |
| binary 10⁶ | 164 ms | 1.14 | 1.63 | 2.52 | 3.35 [3.31, 3.40] | 4.01 |
| binary 10⁷ | 2.64 s | 0.97 | 1.62 | 2.77 | 4.08 [4.04, 4.13] | 5.04 |
| `price` | 40.7 ms | 0.57 | 0.76 | 0.82 | 0.90 [0.89, 0.91] | 0.92 |
| `friendof` | 295 ms | 0.77 | 1.21 | 2.02 | 2.97 [2.92, 3.02] | 3.90 |

The presized build against the default config in absolute time: the grown
build's time over the presized build's, both at load factor 0.7 (above 1, the
presized build is faster; the `serial` column is #88's own effect):

| Relation | `serial` | `:1` | `:8` | `:16` [95 % CI] |
|---|---|---|---|---|
| unary 10⁵ | 0.78 | 1.42 | 1.33 | 1.38 [1.36, 1.40] |
| unary 10⁶ | 1.02 | 1.91 | 2.18 | 2.31 [2.30, 2.33] |
| unary 10⁷ | 1.54 | 3.12 | 3.73 | 3.89 [3.86, 3.91] |
| binary 10⁵ | 0.63 | 0.79 | 0.52 | 0.48 [0.47, 0.48] |
| binary 10⁶ | 0.79 | 0.96 | 0.76 | 0.72 [0.71, 0.72] |
| binary 10⁷ | 0.97 | 1.07 | 0.89 | 0.91 [0.89, 0.94] |
| `price` | 1.21 | 2.08 | 2.62 | 2.92 [2.90, 2.94] |
| `friendof` | 0.72 | 0.85 | 0.48 | 0.40 [0.40, 0.41] |

Iteration (Amendment 2 has it measured, since the presized build may place
root keys in other buckets), at load factor 0.7:

| Relation | presized `:16` over presized `serial` | grown over presized, `serial` | grown over presized, `:16` |
|---|---|---|---|
| unary 10⁶ | 1.03 | 0.72 | 0.74 |
| unary 10⁷ | 1.05 | 1.00 | 1.07 |
| binary 10⁶ | 1.17 | 0.69 | 0.80 |
| binary 10⁷ | 1.20 | 0.76 | 0.92 |
| `price` | 1.05 | 1.01 | 1.05 |
| `friendof` | 1.08 | 0.70 | 0.77 |

**Reading.**
- **The distinct-key ceiling is gone.** Where first keys rarely repeat, the
  grown build's root merge kept `parallel:N` at or below 1× (unary 10⁷ 1.07×,
  `price` 0.92× at `:16`; Karp–Flatt 0.93–1.10). Presized, each worker fills
  its own regions of the root, and the same relations reach 2.2–2.7× at `:16`
  (Karp–Flatt 0.33–0.43). Unary crosses 1× at 10⁵ from `:4` on, where the
  grown build crossed only at 10⁷. In absolute time unary 10⁷ builds 3.9×
  faster than under the default config, `price` 2.9×.
- **Where first keys repeat, presizing costs more than it gains.** The root is
  sized by the tuple count (the paper's |L| = n), not by the distinct keys, so
  `friendof` gets 2²³ buckets for 39,781 keys and binary 10⁶ about ten times
  the buckets it fills. Presized `serial` is then slower (`friendof` 28 %,
  binary 10⁶ 21 %), and so is presized `parallel:16` against the grown build's
  (`friendof` 2.5×, binary 10⁶ 1.4×), whose exact build already parallelises
  these inputs well. Without repeats it is faster: no rehash, unary 10⁷
  `serial` 1.54×.
- **Load factor 0.8 against 0.7:** within 2 % at 10⁶ and 10⁷, where both
  usually round to the same power-of-two root; at 10⁵, 0.8 halves the root and
  is 1.7–2.2× faster.
- **Iteration:** the grown curve is flat across N, as identical tries must be.
  Presized tries built in parallel iterate up to 1.2× faster than presized
  `serial` ones on binary relations, plausibly because each worker allocates
  its regions' subtries together, which the scan then reads in order (not
  profiled). The sparse presized root also makes iteration 20–30 % slower than
  over the grown trie where keys repeat; unary 10⁷ and `price` are equal.
- **Small inputs:** below the crossover, thread starts cost more than the
  build, as for TreeTrie.

**Threats to validity:** as for TreeTrie, plus that `bench ds` times the build
alone. A query's `end_to_end` adds the iteration differences above.

## See also

- [`TreeTrie`](./tree-trie.md), whose Optimizations table lists the mode.
- [`HashTrie`](./hash-trie.md), whose Build modes section lists the mode.
- The design: [`docs/specs/2026-10-05-parallel-build-design.md`](../specs/2026-10-05-parallel-build-design.md).
