# HashTrie Build by Algorithm 2: Group, Then Recurse, Every Table Sized Once

**Date:** 2026-10-07
**Status:** Design approved in conversation (2026-10-07); implemented
(`docs/superpowers/plans/2026-10-07-hash-trie-algorithm-2-build.md`), on top of
the dependent-optimisations design
(`docs/specs/2026-10-07-dependent-optimisations-design.md`), whose
`Prerequisite` table and `presized:N` spelling it uses. Algorithm 2 lives in
`bulk.rs`, as designed. The plan settled two things the design left open: the
run-wise form of `map` is `HashTable::map_in_runs`, and the presized fill is
given the root's log2 capacity and returns the finished root by value, so no
root is allocated only to be dropped.
**Scope:** HashTrie's default build becomes the paper's Algorithm 2: put a list's
tuples into the buckets of a table, then build each bucket's child from its list.
A new Config value, `child-capacity=tuples`, sizes every child once from its list.
The per-tuple build stays selectable as `--ds-build hash-trie=incremental`. This is
"layer 2" of the paper's build (Freitag et al., VLDB 2020, §3.2.2), issue #107.
**Related:** #105 (the paper-convergence board; this closes its "each table sized
once" and "Algorithm 2" rows), #88 (`root-capacity`), #94 and the presized design
(`docs/specs/2026-10-06-hash-trie-presized-parallel-build-design.md`), #91 (`radix:K`),
#92 (lazy expansion), #101 (layer 3), #113 (`root-capacity=keys`), #66 (the bucket
multiplier).

## Summary

The paper builds a hash trie top-down. Algorithm 2 sizes a table once from the
length of the tuple list it is given, pushes every tuple onto the list in its
bucket, and then recurses into each bucket's list. kermit inserts tuple by tuple
(`insert_at`, `kermit-ds/src/ds/hash_trie/implementation.rs:237`): each tuple walks its
whole path before the next starts, so a child table is created on its first tuple, at
4 buckets, and grows. #88 presized the root; no child can be presized, because no
child knows its final size.

This design:

- makes Algorithm 2 the build process of every HashTrie build mode, under the new
  name `bulk` (the default);
- keeps the per-tuple build, unchanged, as `incremental`;
- adds `--ds-config child-capacity=grow|tuples`, where `tuples` sizes every child once
  from its list and the default `grow` keeps today's sizes.

Under every configuration that exists today, `bulk` builds the array-identical trie,
capacities included. Only build time moves.

## Motivation

- **Parity.** Algorithm 2 (lines 3–12) is the paper's build, and every table in it is
  sized once at 2^⌈log₂(1.25·|L|)⌉ and never grown. kermit's docs credit the
  per-tuple insert as Algorithm 2 (`implementation.rs:64`, `:230`;
  `docs/data-structures/hash-trie.md:365`). It is not: Algorithm 2 groups, then
  recurses.
- **Per-level sizing needs a different process.** A child's size is `|L|`, the number
  of tuples below it, which only a grouping step knows before the child is built.
- **One value, one behaviour.** #107 as filed said "under the Config value, the serial
  build becomes group-then-recurse". The dependent-optimisations design (Amendment 3)
  forbids a BuildMode whose process depends on a Config value. Making Algorithm 2 the
  process of every mode, with the old one under its own name, satisfies the rule
  without a prerequisite on the paper configuration.
- **Readability (Priority 2).** The build then reads like the paper's pseudocode, line
  for line.

## Decisions

| Question | Decision |
|---|---|
| Process | Algorithm 2 for every build mode (`bulk`, `radix:K`, `parallel:N`, `presized:N`, and lazy expansion). Rejected: a new mode for Algorithm 2 with a Config that requires it, leaving the default untouched ("A"); it needed a grouped twin of every parallel mode and left Amendment 2's reference undefined under the new Config |
| Old process | Kept, unchanged, as `hash-trie=incremental` (ColumnTrie's #84 precedent), so pre-#107 numbers can be reproduced and the two processes compared within one binary |
| Mode names | `bulk` (default) and `incremental`, ColumnTrie's pair. HashTrie no longer accepts `serial`; kermit-lab reads old HashTrie `serial` rows as `incremental`. No schema bump |
| Config surface | A new axis, `child-capacity=grow\|tuples` (`ds_config_child_capacity`, default `grow`), independent of `root-capacity` |
| Child formula | Algorithm 2, line 3, exactly: no 4-bucket floor for children, so a one-tuple child (pruning off) gets 2 buckets. The root keeps #88's 4-bucket floor |
| Prerequisite | `incremental` requires `child-capacity=grow` |
| Pending-list capacities | `bulk` matches `insert_at`'s, so `space` cannot move |
| Code home | `kermit-ds/src/ds/hash_trie/bulk.rs`, beside `radix.rs` and `parallel.rs`. `insert_at` stays in `implementation.rs` |

## Background

### Algorithm 2 (VLDB 2020, §3.2.2)

```text
 1 function build(i, L)
 2   if i ≤ n then
 3     M ← allocateHashtable(2^⌈log2(1.25·|L|)⌉)
 4     while L is not empty do
 5       t ← pop next tuple from L
 6       B ← lookupBucket(M, h_i(π_vi(t)))
 7       push t onto the linked list stored in B
 8     i_next ← index of the next attribute in E_j
 9     foreach populated bucket B in M do
10       L_next ← extract linked list stored in B
11       M_next ← build(i_next, L_next)
12       store M_next in B
13     return M
14   else
15     return L
```

With lazy child expansion, "we only create the root nodes of the hash tries in the
build phase, and create any nested hash tables on-demand when they are accessed for
the first time" (§3.3.1). In Umbra the lists of line 7 are threaded through an 8-byte
chain pointer reserved in every materialised tuple (§3.3.2).

### kermit today

- `from_tuples_with_config` presizes the root (#88) and calls `insert_at` once per
  tuple. A fresh `Inner` bucket gets a `Singleton` (pruning on), an `Unexpanded`
  child holding `vec![tuple]` (lazy, pruning off), or a 4-bucket table; a second tuple
  below a `Singleton` unprunes it.
- `radix:K` and `parallel:N` build scratch roots by `insert_at`
  (`radix::build_scratch_root`) and merge their entries into the real root in
  first-appearance order. `presized:N` inserts straight into regions of the presized
  root through two mirrored root steps (`insert_at_leaf_root_in_run`,
  `insert_at_inner_root_in_run`), then `insert_at` below.
- Lazy `resolve` builds a child by `expand_level`: `insert_at` over its pending list.
- `heap_size_bytes` counts every list's capacity (`tuple_list_heap_bytes`).

## Design

### `bulk.rs`: Algorithm 2

Two functions, written against the pseudocode:

```rust
/// Algorithm 2: the table at `depth` over the list `L`.
fn build(depth, list: Vec<Vec<usize>>, log2_capacity: u32, config) -> HashTrieNode<P, E> {
    let mut m = HashTable::with_log2_capacity(log2_capacity);          // line 3
    for t in list {                                                     // lines 4–7
        m.entry_or_insert_with(H::hash(t[depth]), lf, Vec::new).push(t);
    }
    if is_leaf_depth(depth, arity) { return Leaf(m); }                  // line 15
    Inner(m.map(|l| child(depth + 1, l, config)))                       // lines 8–12
}

/// The node a bucket's list becomes.
fn child(depth, list, config) -> HashTrieNode<P, E> {
    // P::ENABLED && list.len() == 1 → Singleton(the tuple)              (§3.3.1)
    // E::LAZY                        → Unexpanded(list), capacity fixed (below)
    // otherwise                      → build(depth, list, config.child_log2_capacity(list.len()))
}
```

- At the leaf depth the bucket lists are the chains: the table of line 3 is returned
  with its lists, which is line 15 applied one level down.
- A one-tuple list becomes a `Singleton` directly, so `bulk` never unprunes.
- `from_tuples_with_config` becomes `build(0, tuples, config.root_log2_capacity(n))`.
- `insert_at` is unchanged. It serves `incremental` and `Relation::insert`.

### `HashTable::map`

`HashTable<A>::map(self, f: FnMut(A) -> B) -> HashTable<B>` keeps `log2_capacity`,
`len`, and every key's hash in its bucket, and replaces each value by `f(value)`, in
bucket order. It is line 12, "store M_next in B": a table of lists cannot become a
table of children in place, since the value types differ in size, and under eager
expansion no node variant holds a list. A run-wise form maps contiguous bucket runs on
separate workers, for `presized:N` (below).

### Why the default trie is unchanged

A table's final layout (capacity, and which key sits in which bucket) is a function
of its starting capacity, its load factor and the sequence of distinct keys it
receives: `HashTable::entry_or_insert_with` returns an existing entry before its
resize check. Under `child-capacity=grow` every table starts where the per-tuple build
starts it (the root by `root-capacity`, children at 4 buckets). Every list keeps input
order, so each table receives its distinct keys in the order `insert_at` sends them:
the root sees the input, and a child sees the subsequence of tuples below it. Pruning
agrees: an unprune re-inserts the evicted tuple first, so the child receives its
tuples in input order there too. Chains are pushed from an empty `Vec` in input order
by both builds, so their capacities agree.

### Pending-list capacities

`insert_at` creates a lazy child's pending list as `vec![tuple]` (capacity 1, pruning
off) or, after an unprune, as `Vec::with_capacity(2)` (pruning on). A bucket list
grown from empty jumps to capacity 4. From three tuples up both reach the same
capacities, under std's amortised doubling with its minimum non-zero capacity of 4 for
24-byte elements. So `child` shrinks a one-tuple pending list (pruning off) to
capacity 1, and a two-tuple pending list (pruning on) to capacity 2. Without this,
`space` would move, and a lazy, unpruned trie on `price` (one tuple per first key)
would pay 4× per pending list. The rule follows std's growth policy; the array-level
identity test pins it, so a change in std shows up there rather than in a report.

### Lazy expansion

`resolve` builds an `Unexpanded` child by `build(depth, pending, …)`. The child's own
children come out `Unexpanded` or `Singleton`, so one expansion builds one level, as
the paper describes. Under `child-capacity=tuples` the expanded table is sized from
the pending list's length. The expansion process changes for every lazy trie, built by
any mode; the expanded table does not (above).

### `child-capacity`

| Value | Child tables |
|---|---|
| `grow` (default) | start at 4 buckets and double as keys arrive: today |
| `tuples` | sized once from `\|L\|`, the length of the list they are built from: the smallest `p ≥ 1` with `\|L\| · 100 ≤ 2^p · percent`. At `load-factor=0.8` that is the paper's 2^⌈log₂(1.25·\|L\|)⌉ |

- Distinct keys cannot outnumber tuples, so under `tuples` no child grows during the
  build.
- `HashTrieConfig::child_log2_capacity(len)` sits beside `root_log2_capacity`. The
  shared `hash_table::log2_capacity_for` takes its floor as a parameter (2 for the
  root, 1 for children), so the two rules read side by side.
- After the build, a child that `Relation::insert` creates starts at 4 buckets under
  eager expansion, as #88's root does for a trie created empty; under lazy expansion it
  is sized from its pending list when a probe first reaches it. So under `tuples`, an
  eager and a lazy trie that `insert` has changed since their build may differ in
  capacity (joins and benches never insert after building).
- `HashTrieConfig::axes` gains `("child_capacity", "grow" | "tuples")`.

### Build modes

| Mode | Spelling | Process | Equivalence with `bulk` |
|---|---|---|---|
| `Bulk` (default) | `hash-trie=bulk` | `build(0, tuples, …)` | the reference |
| `Incremental` | `hash-trie=incremental` | one `insert_at` per tuple, in input order: the build named `serial` before #107, code unchanged | array-identical (rung 3) |
| `Radix(K)` | `radix:K` | partition; each scratch root by `build`'s grouping step, recording the input position at which each key first appears, then `map` to children; merge in first-appearance order | array-identical, every config |
| `Parallel(N)` | `parallel:N` | the same on N threads | array-identical, every config |
| `Presized(N)` | `presized:N` | phase 1: workers push each tuple onto its bucket's list in their runs of a presized table of lists (cut into the root's regions), the deferred tail on the calling thread; phase 2 (arity ≥ 2): workers map their runs' lists to children with `child` | rung 2, as today |

- **The presized fold.** Phase 1 replaces both mirrored root steps (about 80 lines that
  copy `insert_at`'s decisions) with one "push onto the list" step for both arities, as
  the presized design anticipated ("the two fold into one shared helper at layer 2").
  For arity 1 there is no phase 2: the lists are the chains.
- **Why `presized:N` stays rung 2.** Phase 1 places the root's keys exactly as today's
  presized root step does (the same regions, the same order within each, the same
  tail). A deferred key's tuples are all deferred, so its list, filled by the tail, is
  in input order too. Every child is therefore built from the list `bulk` builds it
  from.
- **`Relation::insert`** is not a build mode and keeps `insert_at`.

### Prerequisite

One row beside the dependent-optimisations design's `PresizedBuildNeedsPresizedRoot`:

- `IncrementalBuildNeedsGrowingChildren`: `--ds-build hash-trie=incremental requires
  --ds-config child-capacity=grow`. `incremental` cannot size a child it creates on its
  first tuple. The constructor asserts the same condition, with the same sentence.

### Complexity and memory

With n tuples and arity a:

| | `bulk` | `incremental` |
|---|---|---|
| Hashes and tuple moves | n per level, O(n · a) | the same |
| Allocations | one list per inner bucket, per level (a one-tuple list too, freed again when pruning makes it a `Singleton`), plus the tables | the tables |
| Transient memory | one grouping array per table (about 32 B a bucket), alive until its `map` finishes | none |
| Access order | one bucket's subtree at a time | each tuple's whole path, interleaved |

The largest transient array is the root's: about 512 MiB at 2²⁴ buckets, the presized
root of a 10⁷-tuple relation at load factor 0.7. `presized:N` holds the same: its table
of lists and the mapped node table coexist until its children phase ends, 32 B a bucket
more than the presized build before #107, and equal to `bulk`'s peak. Umbra avoids the list allocations and
the transient arrays with its intrusive chain pointer, which needs contiguous tuple
storage (#101).

## Parity with the paper

| Element | Paper | Parity |
|---|---|---|
| Group into bucket lists, then recurse per bucket | Algorithm 2 | ✓ the default build, `bulk` |
| Each table sized once at 2^⌈log₂(1.25·\|L\|)⌉, never grown | Algorithm 2, line 3 | ✓ under `root-capacity=tuples,child-capacity=tuples,load-factor=0.8`; children exactly, the root with a 4-bucket minimum that differs only at n ≤ 1 |
| The leaf level returns its lists unchanged | line 15 | ✓ the leaf table's lists are its chains |
| Lists threaded through an 8-byte chain pointer in each tuple | §3.3.2 | ✗ a `Vec` per list; deferred to #101 (layer 3) |
| Recursion over the populated buckets of M | line 9 | ✓ bucket order |
| Lazy expansion builds a nested table on first access | §3.3.1 | ✓ one level of `build` per expansion, sized from its list under `tuples` |
| Singleton pruning | §3.3.1 | ✓ a one-tuple list becomes a `Singleton` directly |
| The input partitioned by the first attribute's hash, morsel-driven, before Algorithm 2 runs | §3.3.2 | ✓ as #94 layer 1 |
| Partitions as contiguous runs of the root's regions, each grouped by one worker, with a deferred tail | not described: §3.3.2 partitions for locality and does not say how Algorithm 2 runs on threads | — kermit's. The thesis must not present it as Umbra's |
| How the recursion is spread across threads | not described | — kermit's: phase 2 maps each region's lists on a worker. The thesis must not present it as Umbra's |
| Bucket index = the hash's top bits | §3.3.1 | ✗ the #66 multiplier. #105 asked to revisit it once no table grows; under the parity configuration none does, so the revisit is unblocked (out of scope here) |

**The closest-to-paper configuration** after this design:

```text
--ds-config root-capacity=tuples,child-capacity=tuples,load-factor=0.8
--ds-build hash-trie=presized:N --ds-layout-pruning on --ds-layout-expansion lazy
```

It still differs in tuple storage and list threading (#101), bucket layout (#106) and
the bucket index (#66). Results should be reported at 0.8 for parity and at 0.7 as
kermit's default.

## Placement under the standard

- **`bulk`, `incremental`:** BuildMode values on `ds_build_mode`. They build the
  array-identical trie under every config both accept, which is stronger than
  Amendment 2 requires.
- **`child-capacity`:** a Config value. It replaces the constant 4 at child creation on
  the path every `bulk` build takes, as `root-capacity` replaced the root's. Non-users
  pay nothing: under `grow` the function returns the constant.
- **One value, one behaviour:** `bulk` is Algorithm 2 under every config; `incremental`
  is the per-tuple build and is valid only under `child-capacity=grow`;
  `child-capacity=tuples` sizes children from their lists under every mode that
  accepts it.

## CLI, reports and kermit-lab

- **`--ds-config child-capacity=grow|tuples`**, a new key in
  `ConfigChoices::HASH_TRIE_KEYS` (`kermit/src/options.rs:628`), beside `load-factor`
  and `root-capacity`.
- **`--ds-build hash-trie=bulk|incremental|radix:K|parallel:N|presized:N`.**
  `hash-trie=serial` is rejected: "serial was renamed incremental in #107 (the
  per-tuple build); the default is now bulk (Algorithm 2)". TreeTrie's
  `serial` is unchanged.
- **Reports, no schema bump.** Every HashTrie report gains `ds_config_child_capacity`,
  and its `ds_build_mode` takes the values above. Both changes are additive except the
  rename, which kermit-lab maps unambiguously because `serial` is never written for
  HashTrie again. `docs/specs/bench-report-schema.md` gains a history row. It also
  records that `radix:K`, `parallel:N` and `presized:N` keep their labels while their
  subtries now come from Algorithm 2: their `insertion` is comparable across #107 only
  within one binary, as every timing already is.
- **kermit-lab:**
  - `defaults.py`: a HashTrie row without `ds_build_mode` reads as `incremental` (it
    was `serial`); without `ds_config_child_capacity`, as `grow`.
  - A new `RENAMED_VALUES` map, applied on load: HashTrie `ds_build_mode: "serial"`
    reads as `"incremental"`. It rewrites present values, which the back-fill cannot.
  - `speedup_table`'s baseline defaults to each structure's default single-threaded
    mode (TreeTrie `serial`, HashTrie `bulk`) instead of the literal `"serial"`.
  - `threads_of` returns `None` for `bulk` and `incremental`, as for ColumnTrie's.

## Testing

1. **`HashTable::map`:** capacity, `len`, every key's bucket and hash are preserved;
   values are mapped in bucket order; the run-wise form equals the serial one.
2. **`bulk` ≡ `incremental`,** array-level through `identity.rs`'s assertions (table and
   list capacities, `heap_size_bytes`, `tuple_count`): the load factors ×
   `root-capacity` {grow, tuples} × all 8 Layouts × arities 1–4, on the shared inputs
   with duplicates and full-hash collisions.
3. **Modes under `child-capacity=tuples`:** the radix and parallel identity tests
   (`radix_builds_the_serial_trie_*`, renamed `…_bulk_trie_*`, and their parallel
   counterparts) and the presized equivalence tests
   (`presized_parallel_builds_are_equivalent_*`) loop `child-capacity` as well.
4. **No table grows under `tuples`:** a walk asserts that every child's capacity is
   `child_log2_capacity` of the tuple count below it. A table starts at that capacity
   and growth only enlarges it, so equality proves it never grew. Lazy tries are
   walked after a full probe has expanded them.
5. **Lazy expansion:** the trace-equivalence tests in
   `kermit-ds/tests/hash_trie_tests.rs` (lazily expanded tables equal eager ones) run
   under both `child-capacity` values.
6. **The prerequisite:** a `#[should_panic(expected = "requires child-capacity=grow")]`
   constructor test; a CLI test pinning the rejection; the `Prerequisite::ALL` guard
   in `options.rs` reaches the new row (`incremental` under `grow` and under
   `tuples`).
7. **CLI** (`kermit/tests/cli_hash_trie_build_mode.rs`): `hash-trie=serial` is rejected
   with the hint; a `bench ds` report carries `ds_build_mode: "bulk"` and
   `ds_config_child_capacity`; `bench run --verify` passes on triangle under the
   closest-to-paper configuration, eager and lazy.
8. **Suites:**
   - `kermit-ds`: the structure and Parquet suites under a `child-capacity=tuples`
     provider (the `NinetyPercent` precedent) and an `incremental` alias;
   - `kermit/tests/join_tests.rs`: `define_multiway_join_test_suite_with_config!` for
     `child-capacity=tuples` (the `HalfFull` precedent);
     `define_multiway_join_test_suite_for_build_mode!` for `incremental` (the `Radix2`
     precedent); stacked cells `BuiltWith<Configured<…, ChildTuples>, …>` under
     `radix:2` and `parallel:2`, and one paper cell (`root=tuples, child=tuples, lf
     0.8` under `presized:2`); all under both optimisers, with their `AnyOrders` rows;
   - existing providers, aliases and tests named after `serial` are renamed for `bulk`.
9. **kermit-lab pytest:** the rename map, the new defaults, the per-structure speedup
   baseline, `threads_of`.
10. **Mutants, before landing.** Each must turn a test red: `child` ignoring the
    config; the pending-list capacity fix dropped; a bucket list reversed; presized
    phase 2 mapping a neighbour's run.
11. **Miri:** the identity matrices roughly double. Large loops are trimmed under Miri,
    as the presized SipHash matrix was, so `kermit-ds` stays inside CI's 30-minute
    Miri limit.

## Measurement

- **Smoke, before landing (a checkpoint).** One run per arm, jemalloc binary,
  `bench ds` `insertion`: `bulk` vs `incremental` at the default config on unary-1e7,
  binary-1e6 and `friendof`; `child-capacity=tuples` vs `grow` under `bulk` on
  `friendof`. A `bulk` regression is reported to the user before anything lands.
- **After landing,** replicated (≥ 5 replicates, alternating arm order, one binary, a
  distinct `--name` per run):
  1. `bulk` vs `incremental` `insertion` on the 12 relations of the 2026-10-06 run, plus
     a shuffled `friendof` (bears on #101's locality hypothesis);
  2. `child-capacity=tuples` vs `grow`: `insertion`, `space` and `iteration`, at
     load factors 0.8 and 0.7, under `bulk` and `presized:16`;
  3. lazy `iteration`: expansion changed process in every mode, so this A/B is across
     binaries (the perf comparison recipe, TreeTrie as the control).
- Results are recorded in `docs/data-structures/hash-trie.md` and posted to #107 and
  #105. Re-running the thesis sweep's HashTrie `insertion` belongs to #99.

## Out of scope

- `child-capacity=keys`: #113's tuples-vs-keys question, one level down.
- #101: contiguous partitioned tuple storage and intrusive chain pointers.
- The #66 revisit, which this design unblocks.
- #106 (bucket layout) and the root's 4-bucket floor.
- TreeTrie and ColumnTrie (Priority 6).

## Sequencing

1. The dependent-optimisations design lands on master (spec e78be41, plan a0f194b, on
   `aidanb/dependent-optimisations`).
2. This branch merges origin/master, then implements in phases, each gated:
   - **P1:** `bulk.rs` (`build`, `child`), `HashTable::map`, lazy `resolve` through
     `build`, and the `Bulk`/`Incremental` rename. The identity tests pass with no new
     Config.
   - **P2:** `child-capacity` and its prerequisite.
   - **P3:** the modes follow: scratch roots by `build`, the presized fold and its
     phase 2.
   - **P4:** the CLI, kermit-lab and the suites.
   - **P5:** the docs, the mutants, and the smoke checkpoint.

## Acceptance

- [ ] Under every config `incremental` accepts, `bulk` builds the array-identical trie,
      capacities and pending lists included, for every Layout.
- [ ] Under `child-capacity=tuples`, no table grows during `from_tuples` (test 4).
- [ ] Every mode builds the equivalent of `bulk` under every config it accepts:
      array-identical for `radix:K` and `parallel:N`, rung 2 for `presized:N`.
- [ ] `incremental` without `child-capacity=grow` is rejected by the CLI and the
      constructor; `hash-trie=serial` is rejected with the hint.
- [ ] The suites, the CLI tests, the kermit-lab tests and Miri pass.
- [ ] The smoke checkpoint is reported before landing; the replicated measurement
      follows it.
- [ ] The docs below are updated, the three wrong Algorithm 2 credits are fixed, and
      #105's rows are updated.

## Documentation

- **`docs/data-structures/hash-trie.md`:** Construction (Algorithm 2 mapped to `build`
  and `child`), the Build modes table (`bulk`, `incremental`), the Config section
  (`child-capacity`), complexity, and the parity rows.
- **`docs/data-structures/parallel-build.md`** § HashTrie: `presized:N`'s two phases.
- **`docs/specs/optimization-standard.md`:** the implemented-today rows (`bulk` /
  `incremental`, `child-capacity`, the prerequisite).
- **`docs/specs/bench-report-schema.md`:** the history row.
- **`CLAUDE.md`:** the `HashTrieBuildMode` bullet, the `--ds-build` and `--ds-config`
  examples, and the "Config values are build-time for `HashTrie`" gotcha.
- **Code:** the doc comments at `implementation.rs:64` and `:230` stop crediting the
  per-tuple insert as Algorithm 2.
- **#105:** tick "each table sized once" and "Algorithm 2", with the closest-to-paper
  configuration; mark the #66 row unblocked.
