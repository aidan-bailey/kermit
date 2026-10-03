# Sorted-Trie Seek Strategies

**Date:** 2026-10-03
**Status:** Design for review (checkpoint 1); no code yet
**Scope:** Issue #80. `TreeTrie` and `ColumnTrie` take their seek algorithm
as a **Layout** type parameter `S: SeekStrategy`, under
[`optimization-standard.md`](optimization-standard.md). There are three
strategies: `linear`, `binary` (the default, today's code) and `galloping`.
The flag `--ds-layout-seek` selects one, and reports record it as the
`ds_layout_seek` axis. Each strategy has one implementation, shared by both
tries.

**Sequencing (user, 2026-10-02).** The authoritative sweep runs on today's
binary seek, the same algorithm in both sorted tries, and #80 follows as a
separate comparison (issue comment). Code starts after #84 phase 2 is on
master and lands after the sweep. This spec is written against #84's phase-2
shapes: `DsChoices`, `SortedTrie::ColumnTrie { build }`,
`SortedTrieRelation::{kind, build}`, and kermit-lab's `SCOPED_AXIS_DEFAULTS`
and ablation guard (`git show
aidanb/84:docs/specs/2026-10-03-column-trie-build-mode-design.md`).

## Motivation

Issue #67 (9604293) replaced `TreeTrie`'s linear seek with `ColumnTrie`'s
binary search, `partition_point` over the siblings not yet passed. That
closed 10–197x gaps on 31 of 99 WatDiv queries. But queries whose seeks
mostly move one or two siblings got slower. Indicative timings, taken under
load: q0236 ~1.9x and q0167 ~1.35x slower, while q0056 sped up to 0.69x. A
binary search costs O(log r) over the r remaining siblings even when the
answer is the next one; the scan cost O(d) in the distance d moved.

A galloping (exponential) search costs O(log d): cheap for short seeks and
logarithmic for long ones. Veldhuizen's LFTJ analysis assumes this cost for
its amortised O(1 + log(N/m)) seek bound, and `tree-trie.md` already notes
that neither trie meets that bound.

Hard-coding any one strategy repeats #67's problem: it confounds the
structure comparison with the strategy comparison. If the strategy is a
Layout of both sorted tries, with one shared implementation each, the thesis
can vary the two independently. One strategy on both tries isolates the
layout, and one trie under three strategies isolates the seek.

The issue's last criterion ("default decided before the next sweep") is
superseded by the sequencing decision: the default stays `binary`. Changing
it is a separate, later decision, informed by measurement 2 below.

## Decisions

| Question | Decision |
|---|---|
| Category | **Layout**, because it changes code shape rather than a value. A runtime switch would put a branch in every seek, a tax on runs that never change strategy. A type parameter monomorphises it away, and `TreeTrie<BinarySeek>` compiles to today's `partition_point` call. |
| Strategies | `linear` (a scan), `binary` (`partition_point`) and `galloping` (doubling probes from the current position, then a binary search inside the bracket they find). |
| Default | `binary`. `TreeTrie` and `ColumnTrie` default `S = BinarySeek`, so their unparameterised names still mean today's code. This follows the `HashTrie<H = SipHashStrategy, P = NoPruning>` precedent. |
| Shared code | `SeekStrategy::partition_point(remaining, below)`, one implementation per strategy, in `kermit-ds/src/seek.rs`. It computes **only** the offset. Each iterator keeps the logic around that call unchanged (see "What a strategy may not touch"). |
| Strategy contract | For every strategy and input, `S::partition_point(remaining, below) == remaining.partition_point(below)`. Strategies differ only in which elements they probe. |
| Galloping schedule | Stateless per seek. It probes offset 0, then 1, 2, 4, … until a probe fails or passes the end, then runs `partition_point` inside the bracket. **Open question 1.** |
| `linear` | Supported. **Open question 2.** |
| Axis and flag | Axis `ds_layout_seek`, values `"linear"` / `"binary"` / `"galloping"`. Flag `--ds-layout-seek linear\|binary\|galloping`. |
| Axis source | The relation, through `HasOptimizationAxes` on `TreeTrie<S>` / `ColumnTrie<S>`. A Layout describes the structure that exists. (#84 draws the same line from the other side: a BuildMode comes from the family.) |
| Commands | `bench run`, `bench join` and `kermit join` accept the flag with `tree-trie`, `column-trie` or `all`, and reject it with `hash-trie`. `bench ds` rejects it outright, because none of its metrics calls `seek`. |
| `#[derive(IntoTrieIter)]` | Generalised to accept type parameters after `'a`, so it can derive for `TreeTrieIter<'a, S>` and `ColumnTrieIter<'a, S>`. |
| Schema version | Stays at 3. A new axis is non-breaking, and under the default every metric measures what it measured before. |
| kermit-lab back-fill | Scoped by structure. ColumnTrie rows without the axis become `binary`, which is true of every report ever written. TreeTrie and HashTrie rows stay NaN. |
| Applicable phases | #84's build-only guard becomes a general map from axis to applicable phases. `ds_layout_seek` applies to `iteration` and `end_to_end`. |
| Test dependency | None added. The randomised tests reuse the hand-rolled LCG. |

## Design: `kermit-ds`

### The strategy (`kermit-ds/src/seek.rs`)

```rust
/// Compile-time seek strategy of a sorted trie: how `seek` finds the least
/// upper bound among the siblings it has not yet passed. Bench axis
/// `ds_layout_seek`.
pub trait SeekStrategy: LayoutOption + Copy + Debug + Default + 'static {
    /// The number of leading elements of `remaining` that satisfy `below`,
    /// exactly as [`slice::partition_point`] returns it.
    ///
    /// `below` must hold on a prefix of `remaining` and on nothing after it,
    /// as `|k| k < target` does on a sorted slice. The result is then the
    /// offset of the least upper bound, or `remaining.len()` if there is
    /// none. Strategies differ only in which elements they probe.
    fn partition_point<T, P: FnMut(&T) -> bool>(remaining: &[T], below: P) -> usize;
}
```

The method is named `partition_point` so that the call site reads like the
code it replaces, and so that the contract is the one Rust readers already
know.

Three zero-sized markers, each deriving `Clone, Copy, Debug, Default,
PartialEq, Eq` and implementing `LayoutOption`:

| Marker | `NAME` | `partition_point` | Probes for a seek that moves `d` of `n` remaining |
|---|---|---|---|
| `LinearSeek` | `"linear"` | `remaining.iter().position(\|x\| !below(x)).unwrap_or(remaining.len())` | `min(d + 1, n)` |
| `BinarySeek` | `"binary"` | `remaining.partition_point(below)`, today's call | `⌈log₂ n⌉ + 1` |
| `GallopingSeek` | `"galloping"` | see below | 1 if `d = 0`; at most `2⌈log₂ d⌉ + 2` otherwise |

```rust
impl SeekStrategy for GallopingSeek {
    fn partition_point<T, P: FnMut(&T) -> bool>(remaining: &[T], mut below: P) -> usize {
        // A seek usually lands close by, so probe the current position first.
        match remaining.first() {
            | Some(first) if below(first) => {},
            | _ => return 0,
        }
        // Gallop: double `bound` while it is still below. Invariant: `below`
        // holds at offset `bound / 2`.
        let mut bound = 1;
        while bound < remaining.len() && below(&remaining[bound]) {
            bound *= 2;
        }
        // The answer lies in (bound / 2, min(bound, len)]; binary-search the
        // elements strictly between.
        let lo = bound / 2 + 1;
        let hi = bound.min(remaining.len());
        lo + remaining[lo..hi].partition_point(below)
    }
}
```

`lo <= hi` always holds. The loop doubles only while `bound < len`, so
`bound / 2 < len`. The doubling cannot overflow, since `bound < len <=
isize::MAX` before each doubling. The binary search inside the bracket is
the standard library's, so `binary` and `galloping` share it.

**Location.** `PruningPolicy` is the precedent: it lives in `kermit-ds`
because only the structure uses it. `HashStrategy` lives in `kermit-iters`
only because `kermit-algos` hashes constants with it, and no algorithm
needs a seek strategy. `lib.rs` re-exports `SeekStrategy`, `LinearSeek`,
`BinarySeek` and `GallopingSeek` beside the other DS vocabulary.
`IndexStructure` is unchanged: a strategy is a Layout of a structure, not a
structure.

### The tries become generic

```rust
pub struct TreeTrie<S: SeekStrategy = BinarySeek> {
    header: RelationHeader,
    children: Vec<TrieNode>,
    tuple_count: usize,
    _seek: PhantomData<S>,
}
```

`ColumnTrie<S: SeekStrategy = BinarySeek>` gains the same field. Every impl
becomes `impl<S: SeekStrategy> … for TreeTrie<S>`, covering `Relation`,
`JoinIterable`, `Projectable`, `HeapSize`, `Cardinality`, `TrieIterable`
and `Display`, plus #84's `BuildModeRelation` on `ColumnTrie`. Projection
keeps `S`, because `project_via_trie_iter` returns `Self`.
`heap_size_bytes` is unchanged, since `PhantomData` is zero-sized and owns
no heap.

Both structs declare the bound on the type, as `HashTrie` does, so that the
derived `IntoIterator` impl satisfies `TrieIteratorWrapper<IT:
TrieIterator>` (see "The derive").

The iterators become `TreeTrieIter<'a, S: SeekStrategy>` and
`ColumnTrieIter<'a, S: SeekStrategy>`. `S` reaches them only through `trie:
&'a TreeTrie<S>` (resp. `ColumnTrie<S>`), so they gain no field. Each seek
changes in exactly one line:

```rust
// TreeTrieIter::seek
self.sibling_idx += S::partition_point(&siblings[self.sibling_idx..], |n| n.key() < seek_key);

// ColumnTrieIter::seek
let offset = S::partition_point(remaining, |&k| k < seek_key);
```

The comments above each line change to describe the strategy contract
rather than a binary search.

### What a strategy may not touch

Everything around the offset is load-bearing and stays unchanged:

- **`TreeTrieIter::seek`.** This covers the `at_end` early return and the
  backward-seek panic (`current_key > seek_key`). It also covers the
  stack-top update, made only when the seek lands on a key. Off the end,
  the stack top stays on the last positioned node, so `open` after `at_end`
  still descends from it. LFTJ's open-after-`at_end` discipline depends on
  this (CLAUDE.md gotcha; `docs/algorithms/leapfrog-triejoin.md`).
- **`ColumnTrieIter::seek`.** This covers the `at_end` early return,
  `slice_offset += offset` and `!self.at_end()`. `ColumnTrie` has never
  panicked on a backward seek, and it still won't: for a target at or below
  the current key, every strategy returns 0, so the iterator stays put as
  it does today.

Every strategy returns the offset `partition_point` would, and that is
proven at the strategy level. So the iterator state after any seek is
identical under every strategy, and the existing contract tests carry over
without per-strategy reasoning.

### Inference at call sites (trap)

Rust does not fall back to a defaulted type parameter in expression
position. Once `S` exists, `let t = TreeTrie::from_tuples(…);
t.trie_iter()` fails with E0283, "type annotations needed for
`TreeTrie<_>`". So does a binding that only ever reaches a generic
function. A later use that names `TreeTrie`, such as `Vec<&TreeTrie>`, a
return type or a struct field, pins `S`. A minimal rustc probe confirmed
all three cases on 2026-10-03.

The fix is the HashTrie precedent: annotate the binding as `let t: TreeTrie
= …`, as `hash_trie_iter.rs`'s tests do (`let trie: HashTrie =
HashTrie::from_tuples(…)`). Logic does not change. About 100
constructor sites name the plain types, in these places:

- the doc examples on both structs and in `relation.rs`, which are
  doctests;
- `tree_trie/tests.rs` and the column-trie test module;
- the test modules of `kermit-algos/src/sorted/{leapfrog_triejoin,
  selection, iter_kind}.rs`;
- `kermit/src/db.rs`, `kermit/src/db/validation.rs` and
  `kermit/tests/{watdiv_correctness, subject_position_constant,
  lubm_mini_oracle, lubm_cardinalities}.rs`.

Most sites are pinned by a later use, and the compiler lists the rest.
Touching `kermit-algos` is a Priority-6 exception. Only `#[cfg(test)]`
modules change, and only by type annotations, so no measured path moves.
Test macros invoked with an alias are unaffected, because an alias fixes
the parameter.

### The derive

Today `#[derive(IntoTrieIter)]` requires exactly one generic parameter, the
lifetime `'a`. Trybuild UI tests from #60 pin that rule, including
`reject_type_param.rs`. Those tests pinned the macro's existing behaviour;
they did not record a decision against generics. The new rule:

- The first parameter must be the lifetime `'a`.
- Any further parameters must be type parameters.
- The impl is emitted through `syn::Generics::split_for_impl`, so the type
  parameters' bounds and the struct's where-clause carry over.
- Still rejected: no parameters, a first parameter other than `'a`, a
  second lifetime, and a const parameter. The `compile_error!` text changes
  to state the new rule.

`reject_type_param.rs` becomes a positive test in
`derive_into_trie_iter.rs`, a mock iterator generic over a strategy-like
parameter. A new UI case rejects a second lifetime.

The alternative was to hand-write `IntoIterator` for both iterators (eight
lines each). It is rejected because recipe step 3 ("apply
`#[derive(IntoTrieIter)]`") would then stop holding for any generic sorted
structure.

### Axes

A helper in `seek.rs`, `pub(crate) fn seek_axes<S: SeekStrategy>() ->
BTreeMap<String, Value>`, returns `{"ds_layout_seek": S::NAME}`. Both tries
implement `HasOptimizationAxes` through it, so the key is spelt once.

## Design: `kermit` binary

The design below assumes #84 phase 2 has landed.

### The CLI choice (`kermit/src/options.rs`)

```rust
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum SeekChoice {
    Linear,
    #[default]
    Binary,
    Galloping,
}
```

The following follow `HasherChoice` / `PruningChoice` exactly:
`SeekChoice::from_layout_name`, `seek_of::<S: SeekStrategy>() ->
SeekChoice` (the counterpart of `hasher_of`), and a
`seek_choices_round_trip_through_layout_names` guard test.
`LayoutChoices` gains `#[arg(long = "ds-layout-seek", value_name = "SEEK",
value_enum)] sorted_trie_seek: Option<SeekChoice>`, with `_resolved` and
`_explicit` accessors.

### Validation

`validate_layout_choices` today treats every layout flag as hash-only: one
`applies` test covers both flags. It becomes per-flag. Hasher and pruning
apply to `hash-trie | all`, and seek applies to `tree-trie | column-trie |
all`. For example, `--ds-layout-seek is only valid with --indexstructure
tree-trie or column-trie (or all); got --indexstructure HashTrie`. Under #84
this runs inside `DsChoices::resolve`, so no command can resolve without
it. Today `load_query_runner` hand-duplicates the hash-only check for
`kermit join`. Wherever a copy of that check survives #84, it gets the same
per-flag rule, so `kermit join` and `bench join` reject exactly what `bench
run` rejects.

`run_ds_bench_command` rejects an explicit `--ds-layout-seek` before
resolving:

> `--ds-layout-seek has no effect on bench ds: none of its metrics calls
> seek (insertion builds the relation; iteration and end_to_end scan it
> with open/next/up; space measures it). Time a join with bench run or
> bench join.`

Verified: `TrieIteratorWrapper::advance` uses only `open`, `next` and `up`,
and neither build calls the iterator. Accepting the flag would produce
reports that differ in `ds_layout_seek` but time identical code: a fake
ablation.

### `DsChoices` and the cell

`DsChoices { hasher, pruning, seek, config, build }`. `resolve` reads
`seek` from `LayoutChoices`.

```rust
pub enum SortedTrie {
    TreeTrie { seek: SeekChoice },
    ColumnTrie { seek: SeekChoice, build: ColumnTrieBuildMode },
}
```

`for_pair`, `for_structure` and `Sweep::expand` attach `seek` to both sorted
cells, as they attach `config` to the hash cell only. Each structure's
variant carries the choices that pick its concrete type, as
`HashHtj { hasher, pruning, config }` does. `index_structure()` matches
`TreeTrie { .. }` / `ColumnTrie { .. }`.

### `SortedTrieRelation` and the families

`SortedTrieRelation` gains the supertrait `HasOptimizationAxes`, and its two
impls become generic:

```rust
impl<S: SeekStrategy> SortedTrieRelation for TreeTrie<S> {
    type BuildMode = ();
    fn kind((): ()) -> SortedTrie { SortedTrie::TreeTrie { seek: seek_of::<S>() } }
    // build, build_mode_axes: unchanged from #84
}

impl<S: SeekStrategy> SortedTrieRelation for ColumnTrie<S> {
    type BuildMode = ColumnTrieBuildMode;
    fn kind(build: ColumnTrieBuildMode) -> SortedTrie {
        SortedTrie::ColumnTrie { seek: seek_of::<S>(), build }
    }
    // build, build_mode_axes: unchanged from #84
}
```

`execution()` therefore re-derives the seek label from the type that ran,
as `HashTrieFamily` does with `hasher_of::<H>()`. A dispatch arm that
monomorphised the wrong strategy would report it. Today
`SortedTrieFamily::optimization_axes` returns an empty map; it becomes
`rel.optimization_axes()`. From #80 on, every sorted-trie report (`bench
ds`, `bench run` and `bench join`) carries `ds_layout_seek`.

### Dispatch: `with_sorted_trie_layout!`

```rust
macro_rules! with_sorted_trie_layout {
    ($seek:expr, | $S:ident | $body:expr) => {
        match $seek {
            | $crate::options::SeekChoice::Linear => {
                type $S = ::kermit_ds::LinearSeek;
                $body
            },
            | $crate::options::SeekChoice::Binary => {
                type $S = ::kermit_ds::BinarySeek;
                $body
            },
            | $crate::options::SeekChoice::Galloping => {
                type $S = ::kermit_ds::GallopingSeek;
                $body
            },
        }
    };
}
```

This is the one place a seek choice becomes a type. Its call sites are the
two sorted arms of each of `dispatch_run_bench`, `dispatch_ds_bench` and
`load_query_runner`:

```rust
| Execution::TrieLftj(SortedTrie::TreeTrie { seek }) => with_sorted_trie_layout!(seek, |S| {
    run_benchmark(&TrieLftj::<kermit_ds::TreeTrie<S>>::new(optimiser, ()), workload, settings)
}),
| Execution::TrieLftj(SortedTrie::ColumnTrie { seek, build }) => with_sorted_trie_layout!(seek, |S| {
    run_benchmark(&TrieLftj::<kermit_ds::ColumnTrie<S>>::new(optimiser, build), workload, settings)
}),
```

`dispatch_ds_bench` goes through the macro too, even though `bench ds` only
ever passes `Binary`. The rejection is CLI policy, not a type invariant, and
one dispatch shape reads more easily than a special case.

The cost is three monomorphisations of each generic runner per sorted
structure (`HashTrie` already has four). It is a separate macro from
`with_hash_trie_layout!` because the two products share no dimension: a
sorted cell has no hasher, and a hash cell has no seek.

## Keeping old and new reports apart

**`schema_version` stays at 3.** `ds_layout_seek` is a new key. Under the
default, every metric times the same code as before, which measurement 1
checks. Reports from before #80 therefore stay loadable beside new ones.

**Back-fill (trap A).** #80 adds one entry to #84's `SCOPED_AXIS_DEFAULTS`:

```python
# ColumnTrie's seek has been a binary search (`partition_point`) since
# 525c99f (2026-03-02), before the first JSON report writer (f76344d,
# 2026-04-26), so every ColumnTrie report without the axis ran `binary`.
("ds_layout_seek", "ColumnTrie"): "binary",
```

Verified 2026-10-03: 525c99f is an ancestor of f76344d. Every version of
`ColumnTrieIter::seek` since 525c99f calls `partition_point`.

The entry is not in the unscoped `AXIS_DEFAULTS`. That fill ignores
structure and would stamp `binary` on HashTrie rows. The same flaw already
stamps `ds_layout_hasher = "sip"` on TreeTrie rows, and #84 lists it as out
of scope.

**TreeTrie rows stay NaN.** TreeTrie's seek was linear until 9604293 (#67,
2026-10-02 12:43) and binary afterwards. Reports carry no binary identity,
and `schema_version` cannot stand in for one: the v3 bump (100f9d4, #65)
does not descend from 9604293, so a v3 binary may still have the linear
seek. "Missing means binary" would therefore mislabel the 2026-09-10
prelim's TreeTrie rows.

One consequence: the authoritative sweep's TreeTrie rows are NaN as well,
although 62f722e contains 9604293 and so ran binary. Within one sweep's
frame that is harmless. An analyst who needs the label can set it from the
run directory's provenance, on that run's reports only. The strategy
comparison never needs it, because trap C rules out cross-binary
comparisons.

**`linear` is the pre-#67 algorithm, not the pre-#67 code.** The old loop
re-derived `at_end()` and the sibling list on every step. `LinearSeek` makes
the same comparisons in a `position` scan, then writes the stack top once.
Pre-#67 TreeTrie numbers stay incomparable with anything new.

**Applicable phases (trap B).** #84 Task 9 adds `_BUILD_ONLY_AXES` and
`_BUILD_PHASES` to `presets.py`, and makes `ablation` refuse `ds_build_mode`
outside build phases. #80 replaces the pair with one map instead of adding
a second special case:

```python
# The time phases an optimisation axis can affect; an axis absent from this
# map can affect every phase. `ablation` refuses an axis on a phase outside
# its set: two values there time the same code, so any difference between
# them is noise or binary drift.
AXIS_PHASES: dict[str, frozenset[str]] = {
    # A BuildMode changes how a structure is built, never the structure (#84).
    "ds_build_mode": frozenset({"insertion", "end_to_end"}),
    # A seek strategy changes how a built trie is searched; no build calls
    # seek (#80).
    "ds_layout_seek": frozenset({"iteration", "end_to_end"}),
}
```

The `InsufficientAxesError` message names the axis and its applicable
phases. `render_all` already logs that error as a skip. A future axis whose
effect is confined to some phases needs only an entry here, so no other
part of kermit-lab learns axis semantics.

`bench ds`'s `iteration` and `end_to_end` time a scan, not a join, so
`ds_layout_seek` cannot affect them either. The CLI rejection keeps
`bench ds` rows single-valued (`binary`), so the map needs no
report-kind dimension.

**Criterion directories.** Group names do not encode axes. Two runs that
differ only in `--ds-layout-seek` therefore overwrite each other's
`target/criterion/` directories unless their `--name` differs. The same
already holds for the hasher, pruning, load factor and build mode, and
BENCHMARKING.md's ablation note gains the seek flag.

## Measurement

The two measurements answer different questions. Never pool their runs.

### 1. The default is unchanged (acceptance criterion 2)

- **Baseline.** The commit #80's first code commit sits on, exported with
  `git archive <base> | tar -x -C $SCRATCH/kermit-base` (never `git worktree
  add`). Today that commit is 5bd78e7; once #84 phase 2 is merged in, it is
  that merge.
- **Builds.** Release builds of both trees, made inside `nix develop`. Both
  binaries run directly from outside the repo, one at a time, and never
  alongside cargo.
- **Workload.** From `watdiv-stress-100-test-1-prelim`: q0236, q0167 and
  q0056, #67's sensitive queries. Also `lubm-reference` (14 queries) with
  `--verify`.
- **Cells.** TreeTrie and ColumnTrie under LFTJ with the lexicographic
  optimiser, timing `iteration`. HashTrie under HTJ is the control, since
  its code path is untouched.
- **Replicates.** At least 5 per arm, with arm order alternating, and a
  distinct `--name` per (arm, replicate), e.g. `seek-ab-base-r1`.
- **Pass.** Answers are identical (`--verify`, and equal result counts
  across arms on WatDiv). For each query, the new/base median ratio lies
  inside the baseline's own replicate spread. A systematic shift beyond it
  on both sorted tries, and not on the control, blocks landing until
  explained.

### 2. The strategy comparison (the issue's purpose; trap C)

- **One binary.** All strategies run from the #80 head and are compared
  inside it, never across builds. Reports carry no binary identity, so a
  cross-build comparison is unrecoverable after the fact.
- **Probe set.**
  - q0236, q0167 and q0056;
  - the 31 WatDiv queries of #67's table, those where TreeTrie was ≥10x
    slower than ColumnTrie in the 2026-09-10 prelim. Derive the list from
    `kermit-bench-runs/watdiv-stress-100-prelim-2026-09-10/reports/`, and
    record it in the run's README;
  - `lubm-reference`.
- **Arms.** {linear, binary, galloping} × {TreeTrie, ColumnTrie}, under
  LFTJ with the lexicographic optimiser, timing `iteration`. Run
  `bench run <bench> -q <query>` once per query.
- **Replicates.** At least 5, rotating the strategy order between
  replicates (l-b-g, b-g-l, g-l-b, …), with a distinct `--name` per
  (strategy, replicate). This machine shows ~1.3x run-to-run spread.
- **Output.** For each query and structure: the median and spread of each
  strategy. For each structure: the geomean of galloping/binary and of
  linear/binary.
- **Where.** The runs live outside the repo, in
  `kermit-bench-runs/seek-strategies-<date>/`, with a README naming the
  commit.
- **Purpose.** This is evidence for the issue. #80 does not change the
  default.

## Out of scope

- **Changing the default.** That is a later decision, informed by
  measurement 2.
- **An adaptive gallop** that remembers its last stride across seeks. It
  needs iterator state, so the binary instantiation would no longer compile
  to today's code.
- **`ColumnTrieIter::up`'s linear scan.** `column-trie.md` already lists it.
- **TreeTrie's build-time search** (`insert_into_children`). It is not
  `seek`, and no strategy reaches it.
- **HashTrie** (exact-match lookup), and the singleton and selection
  iterators in `kermit-algos` (one key each). None of them searches.
- **kermit-lab's unscoped back-fill flaw**, as #84 also leaves it.
- **The BuildMode × seek cross product in tests.** A build mode yields
  identical arrays under every strategy, and no strategy runs during a
  build. `ColumnTrie`'s `incremental` mode therefore stays tested under the
  default seek only. The Config precedent is the same: `HalfFull` runs on
  `HashTrieSip`/`HashTrieFx`, not on the pruned aliases.

## Testing

**Strategy level** (`kermit-ds/src/seek.rs`, inline tests):

- **Exhaustive.** For every strictly increasing slice over keys `0..10`
  (1024 subsets), every start offset and every target in `0..=10`, all
  three strategies equal `slice::partition_point`. The targets include a
  backward one (below the current key) and one past the end. Under miri,
  the key range shrinks to `0..6`.
- **Randomised.** Using the LCG: lengths up to 4096 including 0, with and
  without duplicates, random starts and targets. Each case includes target
  == current key (expected offset 0) and a target past the end (expected
  `len`).
- **Probe counts.** These count calls to the predicate, so they are
  deterministic and run under miri:
  - linear makes exactly `min(d + 1, n)` probes;
  - binary makes at most `bitlen(n) + 1`;
  - galloping makes exactly 1 when `d = 0`, and at most `2·bitlen(d) + 2`
    otherwise;
  - an empty slice takes 0 probes and returns 0.

  These pin each strategy's complexity, which the equality tests cannot
  see: a galloping search that degenerated into a scan would still return
  the right offsets.
- **Names.** `NAME` is pinned to `"linear"`, `"binary"` and `"galloping"`,
  because these strings are report values.
- **LCG.** #84's `Lcg`, now in the column-trie test module, moves to
  `kermit-ds/src/test_support.rs` (`#[cfg(test)] pub(crate)`) and is shared.

**Structure level:**

- **Zero tax.** `size_of` is equal across the three strategies for
  `TreeTrie<S>`, `ColumnTrie<S>` and both iterators. `heap_size_bytes` is
  equal for the same tuples. This is the pruning precedent's growth guard.
- **TreeTrie unit tests.** `seek_backward_panics` and
  `open_after_failed_seek_descends_from_last_positioned_node` in
  `tree_trie/tests.rs` run under every strategy. They pin the code a
  strategy may not touch.
- **Aliases.** In `kermit-ds/tests/trie_tests.rs` and `parquet_tests.rs`,
  six aliases replace the two plain invocations: `TreeTrieLinear`,
  `TreeTrieBinary`, `TreeTrieGalloping`, `ColumnTrieLinear`,
  `ColumnTrieBinary` and `ColumnTrieGalloping`. This follows the HashTrie
  precedent of invoking aliases only.
- **New fixture in `trie_seek_tests!`.**
  `seek_varied_distances_land_on_least_upper_bound` builds one parent with
  1000 children and seeks at Fibonacci distances (1, 2, 3, 5, 8, …) through
  the real iterator. That exercises long gallops and bracket edges at a
  size miri can run.
- **Seek-cost gate.** `seek_cost_is_independent_of_distance` becomes
  `seek_cost_matches_the_strategy`, keeping its calibration (interleaved
  batches, fastest of 5).
  - It reads `ds_layout_seek` from the relation's `optimization_axes()`,
    which derives from the type, so an alias cannot be mislabelled.
  - It asserts far/near `< 10` for binary and galloping, #67's bound.
  - It asserts `>= 10` for linear. That inverse assertion is the gate:
    linear fails the sublinear bound by design. If `LinearSeek` ever
    stopped being O(d), the linear arm of every ablation would measure the
    wrong thing.

**Join level.** `kermit/tests/join_tests.rs` invokes each of the six
aliases under LFTJ with both optimisers. That is 12 invocations × 16
patterns = 192 tests, replacing today's 4 sorted invocations (64 tests).

**Allocation.** `kermit/tests/result_allocation.rs` gains
`*_lftj_allocates_independently_of_result_size` and `*_of_descent_count`
cells for linear and galloping on both tries: 8 cells; the existing ones
cover binary. The scan cells are not multiplied, because the scan never
seeks.

**Real data.** This is the depth hook. CLAUDE.md's failed-descent lesson
applies: 3–5-tuple fixtures never exercise long seeks. `lubm_mini_oracle.rs`
(CI, needs no Java), `watdiv_correctness.rs` (CI) and
`lubm_cardinalities.rs` (needs Java) become generic over the relation. Each
runs `TreeTrie` under all three strategies.

**CLI.** `kermit/tests/cli_sorted_trie_layout_seek.rs` mirrors
`cli_hash_trie_layout_pruning.rs`:

- `bench run` with `-i tree-trie` and with `-i column-trie` records
  `binary` by default, and `galloping` with `--ds-layout-seek galloping`;
- `bench run -i all -a all --ds-layout-seek linear` puts `linear` on both
  sorted cells and no `ds_layout_seek` on the HashTrie cell;
- the flag is rejected with `-i hash-trie` (in `bench run`, `bench join`
  and `kermit join`), and on `bench ds` with any `-i`;
- a `bench ds -i tree-trie` report carries `ds_layout_seek: "binary"`;
- `bench join` records the strategy, and `kermit join` prints identical CSV
  under all three strategies.

**Unit tests in the binary crate:**

- `seek_choices_round_trip_through_layout_names`, and `seek_of::<S>()` for
  each marker;
- `DsChoices::resolve` accept and reject cases;
- `execution_axes_round_trip_through_for_pair` and
  `for_structure_agrees_with_for_pair` over all six sorted cells;
- `TrieLftj::<TreeTrie<GallopingSeek>>::new(…).execution()` names
  `Galloping`.

**kermit-derive.** The positive generic test, the updated UI cases and
their `.stderr` texts.

**kermit-lab:**

- `test_defaults.py`: a ColumnTrie NaN becomes `binary`; TreeTrie and
  HashTrie NaN stay NaN; an explicit value is untouched.
- `test_render_all.py` and `test_presets.py`: the seek ablation is drawn on
  `iteration` and skipped on `insertion`; #84's build-mode cases still pass.
- `test_contract.py`, the real-binary test: `bench join -i tree-trie
  --ds-layout-seek galloping` loads with `ds_layout_seek == "galloping"`.

**Mutation checks.** Each commits first, applies an exact edit and confirms
both that the named test fails and that the mutant actually applied. It then
reverses the edit, never with `git checkout`, and re-runs the test to see
it pass.

| Mutant | Must be caught by |
|---|---|
| Galloping's `hi = bound.min(len)` becomes `(bound - 1).min(len)` | The exhaustive equality test |
| Galloping skips the offset-0 probe (`lo` starts at 1) | The exhaustive test (target == current) |
| `LinearSeek` delegates to `partition_point` | Probe counts, and `seek_cost_matches_the_strategy` on the linear aliases; the equality tests alone pass it |
| `TreeTrieIter::seek` hard-codes `partition_point`, ignoring `S` | `seek_cost_matches_the_strategy` (`TreeTrieLinear`) |
| The `Galloping` arm of `with_sorted_trie_layout!` binds `BinarySeek` | The CLI test, because the axis is re-derived from the type |

## Verification (gate)

All of the following must pass:

- `cargo test` over the whole workspace, including doctests;
- `cargo clippy --all-targets` with `-Dwarnings`, and `cargo doc --workspace`
  with `-Dwarnings`;
- `MIRIFLAGS=-Zmiri-disable-isolation cargo miri test` for `kermit-ds`,
  `kermit-algos` and `kermit-derive` (whose trybuild tests are already
  ignored under miri);
- kermit-lab's pytest, with `KERMIT_BIN` set.

Cargo runs in the foreground with `CARGO_BUILD_JOBS=2`. Formatting runs only
through `nix develop --command cargo fmt --all`, after
`nix flake update rust-overlay` if CI's fmt disagrees.

## Documentation

- **New `docs/data-structures/seek-strategies.md`**, shared by both tries:
  - the contract (`partition_point`) and the three pseudocodes;
  - the probe-bound table;
  - a worked micro-example: one sibling list, one short and one long seek,
    with every probe listed per strategy;
  - why the strategy is a Layout, and the LFTJ bound;
  - what a strategy may not touch.

  It complements `seek.rs`'s rustdoc and is not a structure page.
- **`tree-trie.md` and `column-trie.md`.** The `seek` complexity row lists
  each strategy. In `tree-trie.md`, the paragraph saying neither trie
  gallops is rewritten. Each page gains an "Optimizations" section (axis,
  flag, default, aliases, link); `column-trie.md`'s sits beside #84's
  build-mode subsection.
- **`optimization-standard.md`:**
  - the Layout examples: the first Layout on the sorted tries and the third
    overall;
  - walkthrough step 3 names `with_sorted_trie_layout!` as the sorted
    counterpart;
  - the "implemented today" table and the "Where to look" rows;
  - the stale "11-pattern suite" becomes 16 patterns.
- **`bench-report-schema.md`.** The `ds_layout_seek` axis, and the scoped
  back-fill with the reason TreeTrie stays NaN.
- **`CLAUDE.md`:**
  - Priorities item 1: the six sorted aliases;
  - the trait hierarchy: `SeekStrategy`;
  - recipe step 3: the derive accepts type parameters after `'a`;
  - optimisation recipe step 4: `with_sorted_trie_layout!`;
  - the "sweeps are cells" gotcha: sorted cells carry `seek`;
  - a one-line gotcha on the E0283 inference trap.
- **Other docs:**
  - `ARCHITECTURE.md`: the `HasOptimizationAxes` implementors and the
    `Execution` variants;
  - `BENCHMARKING.md`: compare seek strategies within one binary, with
    distinct `--name`s;
  - `USAGE.md`: the flag and its rejection on `bench ds`;
  - `kermit-derive`'s crate docs: the new rule.

## Commit sequence

| # | Commit |
|---|---|
| 1 | `feat(kermit-derive): IntoTrieIter accepts type parameters after 'a (#80)` |
| 2 | `feat(kermit-ds): linear, binary and galloping seek strategies (#80)`: `seek.rs`, the shared LCG and the strategy tests |
| 3 | `feat(kermit-ds): sorted tries take their seek strategy as a Layout (#80)`: generic tries and iterators, axes, inference annotations, zero-tax tests, six-alias DS suites, the seek-cost gate |
| 4 | `feat(kermit): --ds-layout-seek selects the sorted tries' seek strategy (#80)`: `SeekChoice`, validation, `DsChoices.seek`, cells, `SortedTrieRelation`, the macro, dispatch, the `bench ds` rejection, the CLI test |
| 5 | `test(kermit): every seek strategy through the join, allocation and real-data suites (#80)` |
| 6 | `feat(kermit-lab): ds_layout_seek back-fill and per-axis applicable phases (#80)` |
| 7 | `docs: the sorted tries' seek-strategy Layout (#80)` |

Mutation checks run on commits 2, 3 and 4. Commits are plain, with no
amends and no pushes.

## Coordination

- **#84 phase 2** (`aidanb/84`) introduces `DsChoices`,
  `SortedTrie::ColumnTrie { build }`, `SortedTrieRelation::{kind, build}`,
  `SCOPED_AXIS_DEFAULTS` and the ablation guard. #80's code starts after
  #84 phase 2 is on master, and merges `origin/master` in first (never
  rebase). Expect textual overlap in:
  - `options.rs`, `execution.rs`, `bench/{run,ds}.rs` and `main.rs`;
  - `trie_tests.rs` and `join_tests.rs`;
  - `defaults.py` and `presets.py`;
  - `CLAUDE.md` and the optimisation standard.
- **The sweep.** It is at 62f722e: iteration and space are done, and the
  insertion re-run is pending #84. #80 lands after the sweep. Its default
  changes no metric's code, but the user's sequencing stands.
- **Checkpoints.** A supervising session reviews at three points: this
  spec and its plan; after commit 4, when the A/B becomes possible; and
  before any merge or push.

## Open questions

These are for the user, routed through the supervisor. The spec assumes the
recommended answers.

1. **Galloping schedule.** Recommended: a stateless doubling gallop from the
   current position (probes at 0, 1, 2, 4, …), then a binary search in the
   bracket. This is the textbook exponential search (Bentley and Yao, 1976),
   and it gives the O(log d) bound Veldhuizen assumes. The alternatives are
   another base, such as 4 (fewer gallop probes, wider brackets, the same
   bound), or a gallop that remembers its last stride, which adds iterator
   state and is listed under Out of scope.
2. **Keep `linear`.** Recommended: yes. The issue asks for it, and it is the
   pre-#67 algorithm, so the thesis can show #67's effect from one binary.
   q0236 and q0167 are where it wins. It costs two more aliases (64 join
   tests, plus two seek-cost tests at about 1 s each in debug builds), and
   its numbers are not the pre-#67 code's numbers.
3. **Measurement 2's scope when #80 lands.** Recommended: the probe set (34
   WatDiv and 14 LUBM queries), with a sweep-scale comparison as a separate
   run if galloping wins broadly.

## Acceptance (issue #80)

| Criterion | Where it is met |
|---|---|
| `--ds-layout-seek` selects the strategy for both sorted tries, and reports carry `ds_layout_seek` | Commit 4 and its CLI test |
| The default (`binary`) leaves answers and timings unchanged | By construction (`BinarySeek::partition_point` is `slice::partition_point`, with no new field); measurement 1 |
| The test suite is extended per the standard | Commits 2, 3 and 5 |
| The default is decided before the next authoritative sweep | Superseded (user, 2026-10-02): it stays `binary`, and measurement 2 informs any later change |
