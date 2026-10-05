# Parallel builds (`--ds-build tree-trie=parallel:N`, `hash-trie=parallel:N`)

`TreeTrie` and `HashTrie` can each be built on several threads (issue
#94). The build is morsel-driven in the sense of Leis et al. (*Morsel-Driven
Parallelism*, SIGMOD 2014): the input is cut into small morsels, and
whichever thread is free takes the next unit of work. It produces exactly
the trie the serial build produces, so only the build-timing metrics
(`insertion`, `end_to_end`) can move.

| Mode | `--ds-build` | `ds_build_mode` | Default |
|---|---|---|---|
| `TreeTrieBuildMode::Serial` | `tree-trie=serial` | `"serial"` | ✓ |
| `TreeTrieBuildMode::Parallel(n)` | `tree-trie=parallel:N` | `"parallel:N"` | |
| `HashTrieBuildMode::Serial` | `hash-trie=serial` | `"serial"` | ✓ |
| `HashTrieBuildMode::Parallel(n)` | `hash-trie=parallel:N` | `"parallel:N"` | |

`N` counts every thread the build uses, the calling one included. So
`parallel:1` runs the parallel code on one thread. For TreeTrie, against
`serial` it measures what partitioning costs, net of one saving: sorting P
partitions takes about n·log₂P fewer comparisons than one sort of
everything, so `parallel:1` can beat `serial`. HashTrie has no such saving
(see [HashTrie](#hashtrie)). N ranges from 1 to 1024 (`Threads::MAX`).

The shared steps live in `kermit-ds/src/morsel.rs` (`scatter`, `dispatch`).
The TreeTrie build is `TreeTrie::build_parallel` in
`kermit-ds/src/ds/tree_trie/implementation.rs`.
The HashTrie build is `parallel::fill_root` in
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

The HashTrie build is the radix build of #91
([`hash-trie.md`](./hash-trie.md#build-modes), `radix.rs`) with its first
two steps on N threads.

### The three steps

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

### Invariant: the parallel trie is the serial trie

Every `parallel:N` build is identical to the serial build of the same
tuples, bucket for bucket and capacity for capacity, under every Layout and
load factor. `parallel_builds_the_serial_trie_*` in `parallel.rs` pins it
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

### Complexity

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

### Worked micro-example

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

## Measuring

See `BENCHMARKING.md`, "Scaling: measuring a parallel build", and kermit-lab's
`kl.speedup_table` / `kermit-lab speedup`. Compare build modes within one
binary. For HashTrie, `kl.speedup_table(df, baseline="radix:K")`
measures the parallel build against the single-threaded partitioned one.

## See also

- [`TreeTrie`](./tree-trie.md), whose Optimizations table lists the mode.
- [`HashTrie`](./hash-trie.md), whose Build modes section lists the mode.
- The design: [`docs/specs/2026-10-05-parallel-build-design.md`](../specs/2026-10-05-parallel-build-design.md).
