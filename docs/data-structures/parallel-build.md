# Parallel builds (`--ds-build parallel:N`)

`TreeTrie` can be built on several threads (issue #94). The build is
morsel-driven in the sense of Leis et al. (*Morsel-Driven Parallelism*,
SIGMOD 2014): the input is cut into small morsels, and whichever thread is
free takes the next unit of work. It produces exactly the trie the serial
build produces, so only the build-timing metrics (`insertion`,
`end_to_end`) can move.

| Mode | `--ds-build` | `ds_build_mode` | Default |
|---|---|---|---|
| `TreeTrieBuildMode::Serial` | `serial` | `"serial"` | ✓ |
| `TreeTrieBuildMode::Parallel(n)` | `parallel:N` | `"parallel:N"` | |

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
  There are more partitions than threads, so a worker that drew a small
  partition takes another.
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

## Complexity

With `n` tuples of arity `a`, `k` distinct first keys and `N` threads:

| Step | Work | Runs on |
|---|---|---|
| Checks and sampling | O(n · a), plus sorting a sample of 128–256 keys per partition (fewer for small inputs) | the calling thread |
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
binary.

## See also

- [`TreeTrie`](./tree-trie.md), whose Optimizations table lists the mode.
- The design: [`docs/specs/2026-10-05-parallel-build-design.md`](../specs/2026-10-05-parallel-build-design.md).
