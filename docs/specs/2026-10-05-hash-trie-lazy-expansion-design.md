# HashTrie Lazy Child Expansion

**Date:** 2026-10-05
**Status:** Approved design (checkpoint 1, 2026-10-05); no code yet
**Scope:** Issue #92. `HashTrie` gains a third **Layout** type parameter,
`E: ExpansionPolicy`, under
[`optimization-standard.md`](optimization-standard.md). `EagerExpansion`
(the default, today's code) builds every level at construction.
`LazyExpansion` builds only the root and keeps each child below it as its
raw tuples until a probe first reaches it (SIGMOD 2020 §3.3.1, Figure 6).
The flag `--ds-layout-expansion eager|lazy` selects one, and reports record
it as the `ds_layout_expansion` axis.

## Motivation

A hash trie built eagerly pays for every subtrie, including those no join
ever reaches. In the paper's model a hash trie is built per query, so the
build is on the query's critical path and skipping untouched subtries is a
direct saving. Kermit keeps relations as persistent indexes and times their
construction separately (`insertion`) from the join (`iteration`). Lazy
expansion moves work between those two metrics, which is precisely why its
measurement needs care (see "Bench methodology").

The catalogue in `optimization-standard.md` lists the optimization as a
Medium Layout. `hash-trie.md` § Deferred follow-ups explains why it is a
Layout: an unexpanded node is a node state, and eager tries should not
carry it.

## Decisions

| Question | Decision |
|---|---|
| Category | **Layout.** It changes which node states exist. A runtime switch would put the unexpanded state, and a branch on it, into every eager trie. |
| Representation | **Move on expansion** (user, 2026-10-05; option A). `HashTrieNode::Unexpanded(E::Pending<P>)`. Under `LazyExpansion` the payload is `Box<LazyChild<P>>` holding `pending: RefCell<Vec<Vec<usize>>>` and `built: OnceCell<HashTrieNode<P, LazyExpansion>>`. Expansion `mem::take`s `pending` into the build, so no tuple is stored twice. Under `EagerExpansion` the payload is the uninhabited `Never`. |
| What is lazy | The root is always built at construction, as in the paper. Every child below it starts unexpanded. Expansion builds **one level**, and its children come out unexpanded (Figure 6). |
| Interaction with pruning | With `SingletonPruning`, one tuple below a bucket is a `Singleton` and two or more is `Unexpanded`. A `Singleton` never expands. Without pruning, one tuple below a bucket is `Unexpanded([t])`. |
| Who expands | Only `HashTrieIter::open`, through `&self` (`OnceCell::get_or_init`). Every read-only walk (`collect_tuples`, `for_each_tuple`, `heap_size_bytes`, `project`, Parquet) reads `built` if present, otherwise `pending`, and never expands. |
| `iteration` methodology | **Cold** (user, 2026-10-05). For a lazy family each timed join runs on a freshly built engine, so it pays its own expansion. Eager cells are unchanged. |
| `space` methodology | **As built only** (user, 2026-10-05). `space/<rel>` always measures a relation that no probe has reached. A per-query expanded footprint is out of scope. |
| Axis and flag | Axis `ds_layout_expansion`, values `"eager"` / `"lazy"`. Flag `--ds-layout-expansion eager\|lazy`, default `eager`. |
| Axis source | The relation, through `HasOptimizationAxes` on `HashTrie<H, P, E>`, like the other two HashTrie Layouts. |
| Commands | `kermit join`, `bench join`, `bench run` and `bench ds` accept the flag with `hash-trie` or `all`, and reject it with `tree-trie` / `column-trie` (a `DsFlag` row). `bench ds` accepts it because its metrics differ under lazy: a cheaper build, a smaller as-built footprint, and a walk over pending lists. |
| Dispatch | `with_hash_trie_layout!` goes from 4 flat arms to 8. A nested form waits for a fourth dimension. |
| `Sync` | Eager tries stay `Sync`: their payload is `Never` as a whole type, not a `Never` field beside cells. Lazy tries are `!Sync` (still `Send`). Nothing in the workspace shares a relation across threads. |
| Schema version | Stays at 3. A new axis is non-breaking, and every existing cell measures what it measured before. |
| kermit-lab back-fill | `("ds_layout_expansion", "HashTrie"): "eager"` in `SCOPED_AXIS_DEFAULTS`, true of every HashTrie report ever written. No `AXIS_PHASES` entry: the axis can affect every phase, including `space`. |

## Design: `kermit-ds`

### The policy (`kermit-ds/src/ds/hash_trie/expansion.rs`)

This file mirrors `pruning.rs`: a closed policy trait whose associated types
are uninhabited under the default.

```rust
/// Compile-time expansion policy of a `HashTrie`: whether a child below the
/// root is built at construction or on the first probe that reaches it.
/// Bench axis `ds_layout_expansion`.
pub trait ExpansionPolicy: LayoutOption + Copy + Default + 'static {
    /// What a `HashTrieNode::Unexpanded` holds: `Box<LazyChild<P>>` when
    /// lazy, the uninhabited `Never` when eager.
    type Pending<P: PruningPolicy>: PendingChild<P, Self>;
    /// Folded by the compiler: `insert_at`'s lazy branches are
    /// `if E::LAZY { … }`.
    const LAZY: bool;
}

pub struct EagerExpansion; // NAME "eager", Pending<P> = Never, LAZY = false
pub struct LazyExpansion;  // NAME "lazy",  Pending<P> = Box<LazyChild<P>>, LAZY = true

pub struct LazyChild<P: PruningPolicy> {
    /// The tuples below this bucket, in insertion order. Emptied (moved
    /// into the build) by expansion.
    pending: RefCell<Vec<Vec<usize>>>,
    /// The table this level would have held, once a probe has reached it.
    built: OnceCell<HashTrieNode<P, LazyExpansion>>,
}
```

`PendingChild<P, E>` is the private trait both payloads implement, with
`Never`'s methods all `match *self {}`:

- `from_tuples(Vec<Vec<usize>>) -> Self`;
- `push(&mut self, tuple)`, which is only legal while unexpanded;
- `built(&self) -> Option<&HashTrieNode<P, E>>` and its `_mut` twin;
- `pending(&self) -> Ref<'_, Vec<Vec<usize>>>`;
- `expand(&self, build: impl FnOnce(Vec<Vec<usize>>) -> HashTrieNode<P, E>) -> &HashTrieNode<P, E>`.

`expand` is `built.get_or_init(|| build(mem::take(&mut *pending.borrow_mut())))`.
The returned reference borrows `self`, so it lives as long as the trie,
which is what the iterator's `&'a` frames need.

The payload is boxed so that a lazy node is the same size as an eager one:
a `Box` is one pointer, which is smaller than the `HashTable` the other
variants hold.

**Compiler risk.** `HashTrieNode<P, E>` holds `E::Pending<P>`, which under
`LazyExpansion` holds `HashTrieNode<P, LazyExpansion>` again: a recursive
type through a generic associated type and a `Box`. rustc should accept it,
since the recursion goes through a pointer. If normalisation or auto-trait
evaluation overflows, the fallback is
`Unexpanded(E::Token, Box<LazyChild<P, E>>)` with `Token = Never` under
eager. The variant is still uninhabited and omitted from the layout, but
auto traits look at every field, so eager tries would become `!Sync`. That
is harmless today. Record which form landed in `hash-trie.md`.

### The node (`node.rs`)

`HashTrieNode<P, E>` gains a fourth variant, `Unexpanded(E::Pending<P>)`.
The forwarding accessors (`buckets_len`, `next_occupied`, `hash_at`,
`index_of`, `len`) panic on it through a `#[cold]` helper, as they do on
`Singleton` today. A table frame never holds an `Unexpanded` node, just as
it never holds a `Singleton`.

### Insert (`implementation.rs`, `insert_at`)

The eager path is unchanged: every new branch is behind `E::LAZY`, which is
a constant. In the `Inner` arm, at depth `d`, with the child at `d + 1`:

| Bucket state | Eager (today) | Lazy |
|---|---|---|
| absent, pruning on | `Singleton(t)` | `Singleton(t)` |
| absent, pruning off | new table, recurse | `Unexpanded([t])`, stop |
| `Singleton(a)` | unprune: new table, re-insert `a` then `t` | `Unexpanded([a, t])`, stop |
| `Unexpanded`, not built | — | `pending.push(t)`, stop |
| `Unexpanded`, built | — | recurse into `built_mut()` at `d + 1` |
| table | recurse | (never: a lazy bucket holds `Singleton` or `Unexpanded`) |

So under lazy, insert does O(1) work below the root, except into a subtrie
a probe has already expanded. The `Leaf` arm is unchanged: the root of a
unary relation is a `Leaf`, and no level below a leaf exists to defer.

### Expansion

```rust
/// Builds the table a lazy child at `depth` would have held, from its
/// pending tuples, by the same `insert_at` construction uses.
fn expand_level(tuples: Vec<Vec<usize>>, depth, arity, load_factor) -> HashTrieNode<P, E>
```

This creates `new_table(is_leaf_depth(depth, arity))` and calls `insert_at`
on it for each tuple in order. Because the policy is lazy, those inserts
create the next level's children as `Unexpanded` / `Singleton`, which gives
one-level expansion with no extra code. The trie's configured load factor
is passed through, so a configured lazy trie expands under the same cap it
was built with.

**Equivalence invariant.** A child's pending list holds its tuples in
insertion order, which is also the order eager construction inserts them
into that child (an eager unprune re-inserts the evicted tuple first, and a
lazy one lists it first). Expansion inserts them through the same
`insert_at` with the same load factor, and linear probing places keys
deterministically by insertion order. So **an expanded lazy table is
identical to the eager table at the same position** in what the iterator
observes: the same occupied buckets, in the same order, with the same hashes
and chains. Only the bucket *values* differ, since a lazy child is a boxed
`Unexpanded` rather than an inline node. The trace-equivalence test (below) pins this. It
is what makes eager-vs-lazy timings a comparison of one variable.

### Iterator (`hash_trie_iter.rs`)

`frame_for(child, depth)` gains one arm: `Unexpanded(p)` becomes
`frame_for(p.expand(|ts| HashTrie::expand_level(ts, depth, arity, lf)), depth)`.
The built node is always a table, so this recursion is one step. `depth`
is what `open` already computes. `arity` and `lf` come from the iterator's
`trie: &'a HashTrie` reference. `Frame`, `Descent`, `key`, `next`,
`lookup`, `size`, `up` and `leaf_tuples` are unchanged. Several iterators
over one trie share expansions: whichever reaches a child first builds it,
and the rest see `built`.

### Read-only walks

- **`visit_at` / `collect_at`** (behind `for_each_tuple`, `collect_tuples`,
  `project`, Parquet round-trips): on `Unexpanded`, recurse into `built` if
  present, otherwise visit each tuple of `pending()`. Tuple *order*
  therefore differs between eager and as-built lazy tries. It was already
  hash-dependent and unspecified, and tests compare multisets.
- **`node_heap_bytes`**: on `Unexpanded`, count `size_of::<LazyChild<P>>()`
  (the box), plus the built node's heap bytes if expanded, otherwise the
  pending vector's capacity × `size_of::<Vec<usize>>()` plus each tuple's
  capacity × `size_of::<usize>()`. `mem::take` leaves a zero-capacity
  vector, so an expanded node counts no stale pending bytes.
- **`Cardinality`** is unchanged: `tuple_count` is a counter.

### Panics (internal invariants)

- **Re-entrancy.** If a visitor closure passed to `for_each_tuple` opens an
  iterator on the same trie and reaches an unexpanded node currently being
  visited, `borrow_mut` panics with `BorrowMutError`. That is a clear panic,
  not a wrong answer, and no caller in the workspace does it. Document it on
  `for_each_tuple`.
- **Panics during expansion.** `expand_level` panics only on allocation
  failure. A panic inside `get_or_init` would leave `pending` emptied and
  `built` unset. Document this; no recovery is attempted.

### Axes

`optimization_axes` adds `ds_layout_expansion` = `<E as LayoutOption>::NAME`.
The module and type docs gain the third Layout dimension in the same words
the first two use.

## Design: `kermit` binary

### CLI (`kermit/src/options.rs`)

- `ExpansionChoice { Eager (default), Lazy }` is a `clap::ValueEnum` in
  `LayoutChoices`, flag `--ds-layout-expansion`.
- `DsFlag::LayoutExpansion` has the row `&[IndexStructure::HashTrie]`, so
  `validate_*` and `resolve_sweep` reject it on sorted-only selections
  exactly as they do `--ds-layout-pruning`.
- `expansion_of::<E>()` derives the label from `E` (beside `hasher_of` /
  `pruning_of`).
- `with_hash_trie_layout!($hasher, $pruning, $expansion, |$H, $P, $E| $body)`
  has 8 flat arms. Its three callers are `dispatch_run_bench`,
  `dispatch_ds_bench` and `load_query_runner`.

### Execution (`kermit/src/execution.rs`)

- `Execution::HashHtj` gains `expansion: ExpansionChoice`, in the same two
  roles its siblings document: the request, and the label re-derived from
  `E`.
- `HashTrieFamily<H, P, E>` and `HashHtj<H, P, E>`. Their `Rel` is
  `HashTrie<H, P, E>`.
- `ExecutionFamily` gains a **required** associated constant:

  ```rust
  /// Whether running a join can change the relations' state (a lazy
  /// HashTrie expands what the join reaches). When true, `bench run` never
  /// probes the loaded engine: every probe runs on a fresh build, so the
  /// engine stays as built for `space`.
  const JOIN_MUTATES: bool;
  ```

  `TrieLftj` states `false` and `HashHtj` states `E::LAZY`. It has no
  default, in keeping with `build_relation`: every family says what it
  does.

### Bench methodology (`kermit/src/bench/run.rs`, `run_benchmark`)

The rule is that **a family with `JOIN_MUTATES` never probes the loaded
engine.** Concretely:

1. **Inputs are kept.** `rebuilds` also holds when
   `F::JOIN_MUTATES && (Iteration ∈ metrics || verify)`, so `build_inputs`
   exist to rebuild from.
2. **`iteration` is cold.** When `F::JOIN_MUTATES`, the setup is
   `(family.build_from_tuples(build_inputs.clone()), query.clone())` and the
   routine is `|(engine, q)| { let n = family.count(&engine, q)…; (engine, n) }`
   with `BatchSize::PerIteration`.
   - Returning the engine moves its drop **out of the timed region**:
     Criterion drops routine outputs after measuring.
   - `PerIteration` keeps one fresh engine alive at a time. `SmallInput`
     would hold `iters / 10` of them.

   Eager families take today's code unchanged.
3. **`--verify` runs on a fresh build** when `F::JOIN_MUTATES`.
4. **`space` is unchanged.** It measures `F::relations(&engine)`, which
   points 2 and 3 guarantee no probe has reached. `--metrics space` and
   `--metrics iteration,space` therefore report the same bytes.
5. **`insertion`** is unchanged: under lazy it times a root-only build.
   **`end_to_end`** is unchanged: each sample builds fresh and runs
   `queries_per_build` queries. The first pays expansion and the rest run
   warm, so `queries_per_build` > 1 shows the amortisation.

Put the probe-engine choice in one small function. Its tests pin the rule:
a family with `JOIN_MUTATES` never receives the shared engine. Its
`build_from_tuples` call count in the `iteration` and verify paths equals
its probe count. `bench join` shares `run_benchmark` and gets this for
free. `kermit join` runs one query once and is unaffected.

**Defining `iteration`.** `bench-report-schema.md` defines `iteration` as
"one join against the relations in their as-built state". For every
existing cell, reusing the engine already meets this definition, which is
why the schema version stays at 3.

**Known confound.** Eager `iteration` reuses one engine, which stays
cache-resident across samples. Lazy `iteration` joins on an engine built
just before the timed region. The eager-vs-lazy `iteration` gap therefore
includes a cache-state term as well as expansion. Measurement 2 bounds it
with a control.

### Interaction with #81

#81 (cost-based optimiser, on branch `aidanb/81`, unlanded) gathers
`column_distinct` statistics when an optimiser asks. If it reads relations
through `for_each_tuple`, it does not expand. If it reads them through
`hash_trie_iter`, it expands the shared engine before `space` runs, which
breaks point 4. Whichever change lands second checks this and routes
statistics through a non-expanding walk.

## kermit-lab

- `SCOPED_AXIS_DEFAULTS[("ds_layout_expansion", "HashTrie")] = "eager"`,
  with a comment matching its neighbours.
- No `AXIS_PHASES` entry, because the axis affects all four phases.
- Check whether the real-binary contract test or any axis list enumerates
  layout axes, and extend it if so.

## Testing

### `kermit-ds` unit tests

- **Size pins (acceptance 1).**
  - `node_does_not_grow_under_the_expansion_policy`: the eager node's size
    equals that of the node without the variant, measured as pruning's pin
    does.
  - The eager `Frame` is unchanged: extend `off_frame_is_the_bare_table_pair`
    across `E`.
  - Also pin that a lazy node is the same size as an eager one (the box).
- **Construction shape.** Under lazy, the root's children are all
  `Unexpanded` (pruning off), or `Singleton` / `Unexpanded` by tuple count
  (pruning on). No node below depth 1 exists before a probe.
- **One-level expansion.** After one `open`, `open`, the opened child is
  built and its children are unexpanded.
- **Insert after partial expansion.** Insert into an expanded child and
  into an unexpanded one. Then `collect_tuples` is the input multiset, and
  a full walk equals eager's.
- **Heap accounting.** `heap_size_bytes` of an as-built lazy trie is the
  sum the formula gives. After full expansion it is the eager trie's heap
  plus one `LazyChild` box per expanded child.
- **Policy file.** Names are the axis values, `LAZY` matches the marker,
  and `Never` is uninhabited (mirroring `pruning.rs`'s tests).

### Trace equivalence (the strongest check)

For each hasher (including colliding `Mod10`), pruning value and load
factor, build eager and lazy tries from the same tuples. Drive both with
the same scripted probe sequence of `open` / `next` / `lookup` / `up`,
including dead-end lookups, and assert identical `key`, `size`, `at_end`
and `leaf_tuples` at every step. Because expansion reproduces the eager
table exactly, this is equality, not multiset comparison. Use a randomised
script with the existing hand-rolled LCG, plus fixed scripts for:

- expanding a sibling after `up`;
- two iterators interleaved on one lazy trie, where one expands and the
  other then descends into the expanded child.

### DS-layer suites (`kermit-ds/tests/`)

Six lazy aliases run `hash_trie_test_suite!`:

- `HashTrieSipLazy`, `HashTrieFxLazy`, `HashTrieMod10Lazy`;
- `…PrunedLazy` for each of the three.

They also run `parquet_test_suite!` in `parquet_tests.rs`, with the same
alias set as today's pruned aliases there. Add one Config-on-lazy suite,
`Configured<HashTrieSipLazy, NinetyPercent>`, because expansion is a new
site that reads the load factor.

### Join layer (`kermit/tests/`)

- **`join_tests.rs`** (acceptance 2): four aliases, `HashTrieSipLazy`,
  `HashTrieFxLazy`, `HashTrieSipPrunedLazy` and `HashTrieFxPrunedLazy`, each
  × `HashTriejoin` × {Lexicographic, Cardinality}. That makes 8 join-layer
  aliases in all. Add one `define_multiway_join_test_suite_with_config!`
  invocation over `HashTrieSipLazy` with `HalfFull`.
- **`result_allocation.rs`**: add lazy cells. The file already runs one
  unmeasured join first, so the measured join is warm and both checks
  (result size and descent count) hold unchanged. A cold lazy join
  allocates per expanded node, by design. Say so in the file's issue list.
- **Other `Execution` sweeps.** Any test that iterates every `Execution` or
  Layout cell (e.g. `cli_query_errors.rs`, `lubm_mini_oracle.rs`) picks up
  the new cells, or gets them added. The plan enumerates these.

### CLI and bench

- **`cli_hash_trie_layout_expansion.rs`**, modelled on
  `cli_hash_trie_layout_pruning.rs`: the flag reaches the report axis on
  each accepting command, and it is rejected with sorted-only `-i`.
- **The probe-engine unit tests** described above.
- **The `space` regression** (acceptance 3): `bench join` with
  `-i hash-trie --ds-layout-expansion lazy` reports the same `space` bytes
  under `--metrics space` and `--metrics iteration,space --verify`.
- `execution.rs`'s existing label and dispatch tests extend across `E`.

## Measurement

Follow the perf A/B recipe: `oxford-uniform-s3` (not `triangle`), TreeTrie
as the control, at least 5 replicates in alternating arm order, distinct
`--name` per run, and a scratch `XDG_CACHE_HOME` if anything must be
generated.

1. **Eager at parity.** Compare `eager` against the pre-change binary on
   `insertion`, `iteration` and `space`. Space should be byte-identical;
   times should fall within the TreeTrie control's spread (the codegen
   precision bound of about ±3 % applies).
2. **The lazy effect.** For `eager` vs `lazy` × pruning {off, on} under
   Sip, record:
   - `insertion`;
   - as-built `space`;
   - cold `iteration`;
   - `end_to_end` at `queries_per_build` = 1 and 10.

   **Confound control.** Once, using a local patch that is never committed,
   force eager through the cold `iteration` path. This bounds the
   cache-state term in the eager-vs-lazy `iteration` gap. Record the bound
   next to the result.

The record goes in `hash-trie.md`'s Layout entry, as the pruning entry does.

## Out of scope

- A per-query expanded-footprint metric (`space_expanded/<rel>`). The user
  chose as-built only; file it separately if the thesis needs it.
- A warm `iteration` variant (`iteration_warm`).
- Pointer tagging (the paper's way of telling expanded from unexpanded
  apart). It is unplanned.
- Parallel or thread-safe expansion. Lazy tries are `!Sync`, and parallel
  probing is a later design discussion.
- Any change to `HashTriejoin`, `TreeTrie` or `ColumnTrie` (Priorities
  item 6).

## Documentation

- **`docs/data-structures/hash-trie.md`.**
  - Add a "Lazy child expansion (`ds_layout_expansion`)" entry under the
    Layout flags, covering CLI, choices, type level, axis values, test
    aliases, measured effect, and the `RefCell` / `OnceCell` representation
    with its `!Sync` note.
  - Remove the Deferred follow-ups paragraph.
  - Extend the representation, invariants and complexity sections: insert
    is O(1) below the root under lazy, and the first `open` of a child
    costs O(k) for its k tuples.
  - Add a micro-example of an unexpanded child.
  - Update the "See also" alias list.
- **`docs/specs/optimization-standard.md`.** Move the catalogue row to the
  landed table (six optimizations, four Layout dimensions). Point the
  "Examples (potential)" row at the landed one.
- **`docs/specs/bench-report-schema.md`.** Add the `ds_layout_expansion`
  axis and the as-built definition of `iteration`.
- **`CLAUDE.md`.** The HashTrie variants grow from four to eight aliases
  (hasher × pruning × expansion), and `with_hash_trie_layout!` now has
  three dimensions. Add a run line to the Build Commands block. Add a
  Gotcha on the `JOIN_MUTATES` rule.
- **`docs/algorithms/hash-triejoin.md`.** One sentence: the algorithm is
  unchanged, and `open` may expand a child.

## Commit sequence

1. `feat(kermit-ds)`: the `ExpansionPolicy` file, the node variant, insert,
   expansion, the iterator arm, walks, heap accounting and axes, with the
   unit and trace tests. The type gains `E` with an eager default, so every
   existing caller still compiles.
2. `test(kermit-ds)`: the lazy DS-layer suites and Parquet aliases.
3. `feat(cli)`: `ExpansionChoice`, `DsFlag`, the 8-arm macro, the
   `Execution` field, `HashHtj<H, P, E>`, `JOIN_MUTATES`, and the
   `run_benchmark` probe-engine rule with its tests.
4. `test(kermit)`: join-layer aliases, the allocation cells, the CLI smoke
   test and the `space` regression.
5. `feat(kermit-lab)`: the back-fill default.
6. `docs`: everything in Documentation, plus the measurement record once
   it has been taken.

Each commit passes the CI gate on its own: `cargo test`, clippy with
`-Dwarnings`, `fmt --check` inside `nix develop`, `cargo doc`, miri, and
the Python job.

## Acceptance (issue #92)

- [ ] Eager node and iterator-frame sizes are unchanged, pinned the way
      `node_does_not_grow_under_the_pruning_policy` pins pruning.
- [ ] All 8 join-layer aliases run the 16 standard patterns under both
      optimisers. The DS-layer `hash_trie_test_suite!` covers each lazy
      alias, including probes that expand a node mid-iteration.
- [ ] The `iteration` / `space` methodology (cold / as built) is
      implemented, pinned by tests and documented.
- [ ] `hash-trie.md` gains a Layout entry, and the catalogue row moves to
      the landed table.
