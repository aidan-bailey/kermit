# HashTrie Parallel Build, Presized Root: the Paper's Design

**Date:** 2026-10-06
**Status:** Design approved (brainstormed 2026-10-06); waits for #88
**Scope:** HashTrie's `parallel:N` gains a second process, used when #88's tuple-count
root sizing is on. The input is partitioned into contiguous regions of the presized
root, and each worker inserts every tuple of its regions once, straight into the root.
This is layer 1 of the paper's build (Freitag et al., VLDB 2020, §3.2.2 and §3.3.2).
Under the default config, `parallel:N` keeps today's exact build.
**Related:** #94 (parallel builds; `docs/specs/2026-10-05-parallel-build-design.md`,
and its § Amendment 2), #88 (initial-capacity hint), #101 (why partitioned builds lose
locality).

## Motivation

The HashTrie half of the #94 scaling run (2026-10-06,
`kermit-bench-runs/hash-trie-scaling-2026-10-06/`; preliminary, replicates 1–3) shows
the merge into the root as `parallel:N`'s limit. That merge runs on the calling thread,
which inserts D root keys one at a time in first-appearance order, D being the number
of distinct first-attribute hashes. Karp–Flatt serial fractions:

| Relation | Speedup at N=16 | Karp–Flatt (N ≥ 8) |
|---|---|---|
| unary-1e7 | 1.00× | ≈ 1.0 |
| price (every first key distinct) | 0.91× | 1.1–1.6 |
| binary-1e7 (about 10 tuples per first key) | 4.09× | 0.19 |

Amendment 2 (2026-10-06) relaxed the BuildMode rule to "the same contents and
capacities". Slot placement may now differ, so the root can be filled in parallel. The
user chose the paper's design for this, integrated into `parallel:N` rather than added
as a separate mode.

## Decisions

| Question | Decision |
|---|---|
| Design | The paper's (layer 1): partition by root region, then one insert per tuple into the worker's regions of a presized root. The alternatives considered were scratch roots followed by a region fill ("A"), sort-grouping, and one shared root with concurrent inserts. A was superseded by this design. The other two were rejected: the shared root is nondeterministic and cache-hostile, and sort-grouping is risky and is a later optimisation |
| Mode | No new mode. `parallel:N` picks its process from the config: this design under #88's tuple-count root sizing, today's exact merge otherwise |
| Default config | Unchanged. `parallel:N` stays array-identical to serial, and the 2026-10-06 scaling run stays valid for it |
| Layout across N | The same for every N: fixed-size regions, input order within each region, the tail in input-position order |
| Root step | A mirrored root step beside `insert_at`; `insert_at` itself is untouched (Priority 6). The two fold into one shared helper at layer 2, when the serial build is restructured anyway |
| Region size | 4096 buckets, a constant with a comment, as `MORSEL_TUPLES` is. Tests take it as a parameter |
| Layers 2 and 3 | Wanted eventually, out of scope here: per-level sizing (Algorithm 2's recursion), and partitioned contiguous tuple storage (§3.3.2, #101) |

## Background

**The paper sizes first.** Algorithm 2 allocates each table once, at
2^⌈log₂(1.25·|L|)⌉ buckets for its |L| input tuples, and never grows it. A key's
bucket is the top bits of its hash. Because the root's size is known before any insert,
partitioning the input by the hash's top bits is partitioning by root region (§3.3.2:
two-pass radix partitioning, morsel-driven).

**kermit grows.** A table starts at 4 buckets and doubles whenever the load factor would
be exceeded. The root's final size is therefore a function of D, known only after
grouping, and the bucket index is the top p bits of `hash × MULTIPLIERS[p]` (#66). #88
adds a non-default Config that sizes the root from the tuple count n: the smallest power
of two ≥ 4 that holds n keys under the load factor. Since D ≤ n, a `from_tuples` build
then never grows the root, and its size is a function of (n, load factor), known before
partitioning. That is the precondition for this design.

## Design

### Data flow

```text
parallel:N under tuple-count root sizing:
  checks (from_tuples_partitioned, as now); no tuples → the empty root, no workers
  p = root capacity exponent for (n, load factor)       // #88
  root = a table of 2^p buckets, allocated once         // #88; it does not grow here
  regions: fixed blocks of REGION_BUCKETS = 4096 buckets (the whole table if smaller)
  P = 4·N rounded up to a power of two, capped at the region count;
      partition k is a contiguous run of regions
  partitions = scatter(tuples, by the top log₂P bits of hash × MULTIPLIERS[p])
      // morsel-driven; each partition keeps input order and input positions
  root.with_runs(P, REGION_BUCKETS, |runs|
      dispatch over (partition k, run k), N workers:
          for each tuple, in input order:
              insert_at_root_in_run(run, tuple)
                  Ok      → the subtrie is built below by insert_at(child, depth 1, …)
                  Overflow → defer (position, tuple)
          → this worker's deferred tuples)
  caller: sort the deferred tuples by input position; insert each with the
          ordinary root insert_at (no growth: the root is presized)
  tuple_count = n
```

### `HashTable` primitives (crate-internal)

- **`with_runs(parts, region_buckets, f)`** lends `f` the bucket array as `parts`
  contiguous `BucketRun`s (`chunks_mut`), each owned by one worker. When `f` returns,
  it adds the runs' insert counts to `len`. Scoping this way, `len` cannot be left
  inconsistent, which would silently break the load-factor check on a later
  `Relation::insert`.
- **`BucketRun::entry_or_insert_with(hash, default) -> Result<&mut V, Overflow>`**
  probes from the key's home bucket (the table's own index at its capacity), and only
  within the `region_buckets`-sized region holding that home.
  - It returns the entry if found.
  - It inserts `default()` at the first empty bucket.
  - It returns `Overflow` on reaching the region's end, the table's last bucket
    included. It never wraps and never grows.
- The presized constructor and the capacity function come from #88.

### The root step

`HashTrie::insert_at_root_in_run` sits beside `insert_at`, so the two read side by side.

- **Arity 1:** the root is a `Leaf` table, so the step is
  `run.entry_or_insert_with(hash, Vec::new)?.push(tuple)`.
- **Arity ≥ 2:** the same decisions as `insert_at`'s root level.
  - A new key becomes a `Singleton` (pruning on), an `Unexpanded` child (lazy, pruning
    off) or a fresh child table.
  - An existing key unprunes, appends to its pending list, or descends.
  - Then the unchanged `insert_at(child, depth 1, …)` takes over.

The step mirrors about 30 lines of `insert_at`. The equivalence tests below are the
guard against drift.

### Why it is correct

- **Linear probing stays valid.** Every bucket between a key's home and its slot is
  occupied: inside a region by construction, and for a deferred key because the tail
  probes normally from its home. Nothing is deleted.
- **Subtries are serial's.** A key's tuples reach `insert_at` in input order, all inside
  its region or all in the tail. Once a key has been deferred it cannot be found inside
  its region, so every later tuple with that key probes to the region's end and is
  deferred too. Every subtrie, chain, singleton and pending list is therefore built by
  the serial build's calls, in its order.
- **The root matches serial in everything but slots.** Under the same config, serial
  presizes the root identically and inserts its keys in input order with no growth. The
  parallel root equals sequential insertion in the order (every region's keys, then the
  deferred keys). By linear probing's order-independence (Knuth, TAOCP §6.4) it occupies
  the same set of buckets with the same total displacement. Keys differ from serial's
  slots only where deferred keys shift them near region boundaries.
- **Same for every N.** Regions are fixed-size, and all of a region's tuples fall in one
  partition, in input order. So each region's contents, its order and the deferred set
  are independent of N, and the tail is sorted by input position.
- **Under Amendment 2.** The trie holds the same contents with the same capacities as
  serial under the same config, so `space` cannot move. Root slots may differ, so
  `iteration` is measured.

### Complexity

With n tuples, D distinct first-attribute hashes and N threads:

| Step | Work | Runs on |
|---|---|---|
| Checks, sizing | O(n) | the calling thread |
| Partition | O(n) hashes and moves | N workers |
| Region inserts (and every subtrie below) | O(n · a) expected, once per tuple | N workers |
| Tail | the deferred tuples (a small fraction of n at load factors ≤ 0.7) | the calling thread |

The scratch roots and the D-key merge are gone. The sequential remainder is the checks,
`scatter`'s merge of per-morsel buckets, and the tail.

## Placement under the standard

- **Category:** still a BuildMode, axis `ds_build_mode = "parallel:N"`.
- **The new rule:** a mode may pick its process by Config value, as long as within each
  config it builds the equivalent of serial under that config.
  `docs/specs/optimization-standard.md` gains a sentence to that effect.
- **Reports are never ambiguous:** the `ds_config_*` axis that #88 adds sits beside
  `ds_build_mode`, so each row names exactly one process. No schema bump.

## CLI and kermit-lab

- No new flags. The design runs under `--ds-config <#88's key>=tuples --ds-build
  hash-trie=parallel:N`; #88's spec names the key.
- kermit-lab needs no change: `threads_of` already reads `parallel:N`.

## Testing

1. **`HashTable` units** (`with_runs`, `BucketRun`):
   - finds an existing key;
   - inserts at the first empty bucket;
   - returns `Overflow` exactly at the region's end, never wrapping, at the table's
     last bucket too;
   - `len` equals the runs' sum;
   - after the runs and the tail, `get` and `index_of` find every key;
   - property: the runs plus the tail give the same occupied buckets and the same total
     displacement as sequential insertion into the same presized table.
2. **Equivalence matrix** under tuple-count sizing:
   - `parallel:N` against serial: below the root at array level, matched by hash; the
     root map-level, with the same capacity, `len`, occupied set and total
     displacement; `heap_size_bytes`; `tuple_count`;
   - `parallel:N` against `parallel:1`, whole trie at array level;
   - the matrix: 12 Layouts (Sip/Fx/Mod10 × pruning × lazy) × arity 1–4 × load factors
     50/70/95 × the shared inputs plus large and skewed ones × N ∈ {1, 2, 3, 8} ×
     morsels {7, 16 384};
   - a test-only region size, so small inputs span many regions and overflow, and a
     crafted hasher that homes keys at a region's last bucket to force overflow
     (asserted through the record).
3. **The record:** the test-hooks entry becomes a struct holding the thread count, the
   partition sizes, the path (exact or presized) and the deferred count. The reach test
   gains presized rows at N = 2 and 3; the binary's six-route test gains a presized
   case.
4. **The default config:** the existing exact-path tests pass unchanged.
5. **Suites:**
   - the structure suites and the 16 join patterns under (tuple-count sizing ×
     `parallel:2`): 3 Layouts (as the radix precedent) × 3 optimisers, plus `AnyOrders`
     rows;
   - test types: `BuildModeRelation` for `Configured`, routed through
     `from_tuples_with_config_and_build_mode`, so `BuiltWith<Configured<…>, …>`
     composes;
   - CLI: one `bench ds` report carries both axes, and `bench run --verify` passes on
     triangle, eager and lazy.
6. **Miri:** the Sip matrix at N = 2 with small regions and morsels; the rest
   `cfg_attr(miri, ignore)`.

## Measurement (after landing)

- **Protocol:** one binary, the #94 protocol and the 2026-10-06 run's scripts.
- **The presized curve:** `serial` and `parallel:{1,2,4,8,16}`, both under tuple-count
  sizing, on the same 12 relations, with `iteration` measured too (Amendment 2).
- **Comparisons:**
  - presized `parallel:N` against presized `serial` is this design's scaling result;
  - against the default-config curve of 2026-10-06, the effect of removing the serial
    merge;
  - presized `serial` against default `serial` is #88's own effect, kept out of the
    parallel speedup.

## Out of scope

- **Layer 2:** per-level sizing, which restructures the serial build; the mirrored root
  step then folds into a shared helper.
- **Layer 3:** partitioned contiguous tuple storage (#101).
- `parallel:N` under the default config.
- Any change to `radix:K`: it presizes through #88 like serial.
- TreeTrie. Its 1e7 plateau is being profiled separately (tree-trie-profile session).

## Sequencing

1. **#88** (hint-size-88 session): its spec's decision section is confirmed against the
   precondition above, then it lands.
2. **`aidanb/hash-trie-parallel`** (plan 2 and Amendment 2) lands, on the user's word.
3. **This design** is implemented on a fresh merge of origin/master.

## Acceptance

- [ ] Under tuple-count sizing, `parallel:N` builds the equivalent of serial: the same
      contents, capacities, occupied buckets and total displacement; array-identical
      below the root; array-identical across N ∈ {1, 2, 3, 8}; for every Layout.
- [ ] Under the default config, `parallel:N` is unchanged.
- [ ] The suites, the CLI tests and Miri pass.
- [ ] `insert_at` and `from_tuples_with_config` are untouched.
- [ ] The docs listed below are updated, and the standard gains its Config-dependent
      process sentence.
- [ ] The presized curve is measured, with `iteration`.

## Documentation

- **`docs/data-structures/parallel-build.md`** § HashTrie: both paths, the root
  guarantee, complexity, and a worked example with an overflow.
- **`docs/data-structures/hash-trie.md`:** the Build modes row, and the cross-reference
  to #88's Config flag.
- **`docs/specs/optimization-standard.md`:** the sentence on modes that pick their
  process by config.
