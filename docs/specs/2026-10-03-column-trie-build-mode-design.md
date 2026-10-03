# ColumnTrie Build Modes

**Date:** 2026-10-03
**Status:** Approved design, not yet implemented
**Scope:** Issue #84. `ColumnTrie::from_tuples` stops inserting tuples one
at a time and builds its layers in one pass. The old routine survives as
the `incremental` mode of the first **BuildMode** consumer under
[`optimization-standard.md`](optimization-standard.md), selected with
`--ds-build` and reported as the `ds_build_mode` axis.

## Motivation

`ColumnTrie::from_tuples`
(`kermit-ds/src/ds/column_trie/implementation.rs:317-347`) sorts its input,
then calls `insert` once per tuple. On sorted input every insert appends, so
nothing shifts, but `step_layer` (`:188-246`) still scans each interval from
its start to find the match or the append point, and both are always the
interval's last entry. The build is therefore O(n · a · b), which is
quadratic per parent on wide relations.

On the WatDiv stress-100 sample (60 relations) ColumnTrie loads in about
125 s, against about 1.9 s for TreeTrie and 1.7 s for HashTrie (indicative,
2026-10-02 prescreen at 62f722e). The 2026-09-10 prelim measured `friendof`
alone at 34 s. The cost belongs to the build routine, not to the column
layout, and it dominates ColumnTrie's `insertion` metric.

The user asked for the fix to be an optimisation knob rather than a silent
replacement. Keeping the old routine lets the thesis measure the issue's own
claim (build routine versus layout) and reproduce pre-#84 `insertion`
numbers from the current binary.

## Decisions

| Question | Decision |
|---|---|
| Category | **BuildMode**: a process yielding the same shape. Both modes produce identical arrays and capacities, so no query-time code pays for the knob. ColumnTrie is the first BuildMode consumer. |
| Modes | `incremental` (today's sort, then one `insert` per tuple; build routine unchanged) and `bulk` (one pass over the sorted tuples). |
| Default | `bulk`. Plain `Relation::from_tuples`, every test, and every bench run without `--ds-build` get the fix. |
| Phasing | One branch, two phases. Phase 1 (bulk build, equivalence test, fixed `ds_build_mode: "bulk"` axis) can land alone, so the authoritative sweep's deferred `insertion` re-run isn't held up. Phase 2 adds the knob. |
| Axis source | The bench family that ran the build, not the relation. The relation is identical under both modes, so it cannot know. |
| Axis timing | Phase 1 already stamps `ds_build_mode: "bulk"` on ColumnTrie reports, so the kermit-lab back-fill rule ("a ColumnTrie row without the axis was built `incremental`") is never falsified by a bulk-built row. |
| Schema version | Stays at 3. See "Keeping old and new reports apart". |
| Test dependency | None added. The randomised test reuses the existing hand-rolled LCG (no `proptest` / `rand`). |
| `kermit join` | Gets no `--ds-build`, for the same reason it has no `--ds-config`: a build mode cannot change answers. |
| Authoritative re-run | Issue criterion 4 (re-run the sweep's `insertion` step for all three structures) is **not** part of this work; the supervising session takes it up with the user. |

## Design: phase 1

### The bulk build

`from_tuples` keeps its prologue and replaces the insert loop with a
one-pass builder:

```rust
fn from_tuples(header: RelationHeader, mut tuples: Vec<Vec<usize>>) -> Self {
    if tuples.is_empty() { return Self::new(header); }
    // assert every tuple's arity (TreeTrie's precedent, tree_trie/implementation.rs:144)
    // sort: unchanged hand-rolled lexicographic comparator
    Self::from_sorted(header, tuples)
}

/// Builds the layers from lexicographically sorted tuples in one pass:
/// append keys layer by layer, and open a new child interval wherever the
/// prefix changes.
fn from_sorted(header: RelationHeader, sorted: Vec<Vec<usize>>) -> Self {
    let mut trie = Self::new(header);
    let arity = trie.header.arity();
    if let Some(root) = trie.layers.first_mut() { root.open_interval(); }
    let mut previous: Option<Vec<usize>> = None;
    for tuple in sorted {
        // Depth at which this tuple leaves its predecessor's path;
        // `arity` means it is a duplicate.
        let diverge = previous.as_deref().map_or(0, |p| common_prefix_len(p, &tuple));
        if diverge == arity { continue; }
        for depth in diverge..arity {
            // The key just pushed one layer up is new, so its children start here.
            if depth > diverge { trie.layers[depth].open_interval(); }
            trie.layers[depth].push_key(tuple[depth]);
        }
        trie.tuple_count += 1;
        previous = Some(tuple);
    }
    trie
}
```

`ColumnTrieLayer` gains two push-only mutators, `push_key` (`data.push`) and
`open_interval` (`interval.push(data.len())`). Its doc comment, which says all
mutation goes through `insert_key_and_shift_intervals` and `add_interval`,
lists them too. `insert`, `insert_all`, `step_layer` and `add_interval` are
untouched.

Properties the design relies on:

- **Same arrays.** On sorted input, today's `step_layer` always takes the
  append branch with `insert_pos == data.len()`, and `add_interval(i)` always
  takes `i == interval.len()`. So the old build performs exactly these pushes
  in exactly this order. The first tuple pushes interval `0` on every layer,
  which the root `open_interval` plus the `depth > diverge` rule reproduce.
- **Same capacities.** `push` and `Vec::insert` both grow through the same
  one-step amortised path, so a Vec grown one element at a time from empty
  has a capacity determined by its final length alone. `heap_size_bytes`
  sums capacities, so it is unchanged. The build must therefore never use
  `Vec::with_capacity`, `reserve`, `extend` from a sized iterator, or
  `shrink_to_fit`.
- **Arity 0.** The first tuple has `diverge == 0 == arity` and is skipped,
  so `from_tuples(0.into(), vec![vec![]; 3])` yields no layers and
  `tuple_count == 0`, as today. No special case.
- **Peak memory.** The loop takes tuples by value and keeps only the
  predecessor, so each tuple's `Vec` is freed as the loop passes it, like
  today's consuming loop.
- **Complexity.** O(n · a) for the build after the O(n · a · log n) sort.

The layout is canonical: each layer's `data` is every parent's sorted
children, concatenated in parent order, so the arrays depend only on the
tuple *set*. Incremental `insert` in any order produces the same arrays.
The equivalence test pins this, and `column-trie.md` states it as an
invariant.

### The `ds_build_mode` axis in phase 1

A new `RelationFamily::build_mode_axes(&self) -> BTreeMap<String, Value>`
(`kermit/src/execution.rs`, default empty) runs alongside the existing
`optimization_axes(rel)`. Both report-assembly sites add its result
unconditionally: `bench/ds.rs` (next to `:180`) and `bench/run.rs` (next to
`:120-123`, which reads axes off `relations.first()` and so emits nothing for
a workload with no relations).

In phase 1 `SortedTrieRelation` gains a `build_mode_axes()` hook (default
empty). ColumnTrie's returns `{"ds_build_mode": "bulk"}`, and
`SortedTrieFamily<R>` delegates to it. TreeTrie and HashTrie reports carry no
such key.

Layout and Config axes describe the structure that exists, so they come from
the relation. A build mode describes the process, and the structure is the
same either way, so only the code that ran the build can report it. The
separate method follows the standard's own category boundary.

## Design: phase 2

### `kermit-ds`

**The mode** (`kermit-ds/src/ds/column_trie/build_mode.rs`, re-exported from
`ds/mod.rs` and `lib.rs`):

```rust
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, clap::ValueEnum)]
pub enum ColumnTrieBuildMode {
    Incremental,
    #[default]
    Bulk,
}
impl kermit_iters::BuildMode for ColumnTrieBuildMode { /* "incremental" | "bulk" */ }
```

`kermit-ds` already depends on clap (`IndexStructure` derives `ValueEnum`). A
guard test pins `axis_value()` to the clap value names.

**The construction seam** (`kermit-ds/src/relation.rs`, beside
`ConfigurableRelation`):

```rust
pub trait BuildModeRelation: Relation {
    type BuildMode: kermit_iters::BuildMode + Copy + Default;
    fn from_tuples_with_build_mode(
        header: RelationHeader, mode: Self::BuildMode, tuples: Vec<Vec<usize>>,
    ) -> Self;
}
```

ColumnTrie is the only implementer. Its `Relation::from_tuples` becomes
`from_tuples_with_build_mode(header, Default::default(), tuples)`. Both modes
share the prologue (empty check, arity check, sort). `Incremental` then runs
today's `insert` loop; this is where phase 1's test oracle moves. `Bulk` runs
`from_sorted`. The up-front arity check adds one O(n) pass to `Incremental`,
which is negligible beside its O(n · a · b) build and gives both modes the
same panic message on mixed-length input.

**The test wrapper** (`kermit-ds/src/built_with.rs`): `BuiltWith<R, P>`, a
`BuildModeProvider<M>` trait (`fn build_mode() -> M`) and a
`define_build_mode_provider!(Name, ModeTy, expr)` macro, mirroring
`Configured<R, P>` / `ConfigProvider` / `define_config_provider!`
(`kermit-ds/src/configured.rs`). `BuiltWith` overrides `new` and
`from_tuples` to inject the mode, derefs to `R`, and forwards `JoinIterable`,
`Projectable`, `HeapSize`, `Cardinality` and `TrieIterable`. It does not
implement `BuildModeRelation`, so the marker cannot drift from the value.

`Configured` cannot be reused: it requires `ConfigurableRelation` with a
`ConfigOption`, and would file the axis under `ds_config_*`. Unlike
`Configured`, `BuiltWith` needs no rule that `project` preserves the mode.
Projection rebuilds through the default mode, and because modes produce
identical arrays, losing the mode changes nothing observable.

### `kermit` binary: preparatory refactor

The resolved `--ds-*` choices travel as one value,
`DsChoices { hasher, pruning, config, build }` (`kermit/src/options.rs`).
`DsChoices::resolve(selector, &layout, &config, &build)` validates and
resolves in one step, so no command can resolve without validating. It
replaces the separate hasher / pruning / config parameters of
`Execution::for_pair`, `Execution::for_structure`, `Sweep::expand`,
`dispatch_ds_bench` (9 parameters today, under
`#[allow(clippy::too_many_arguments)]`) and `load_query_runner`. It is its own
commit with no behaviour change: hash-trie runs are identical, and no
measured code moves.

### `kermit` binary: the cell, the family, the axis

**The cell carries the mode**, following `HashHtj { hasher, pruning, config }`:

```rust
pub enum SortedTrie { TreeTrie, ColumnTrie { build: ColumnTrieBuildMode } }
```

`SortedTrieRelation::KIND` (a `const`, which cannot hold a runtime value) is
replaced by:

- `type BuildMode: Copy + Default`
- `fn kind(build: Self::BuildMode) -> SortedTrie`
- `fn build(header, build: Self::BuildMode, tuples) -> Self`
- `fn build_mode_axes(build: Self::BuildMode) -> BTreeMap<String, Value>`

TreeTrie's impl uses `type BuildMode = ()` and calls plain `from_tuples`, so
TreeTrie's code isn't touched. ColumnTrie's calls
`from_tuples_with_build_mode` and returns `{"ds_build_mode": build.axis_value()}`.

**The family.** `SortedTrieFamily<R>` gains `build: R::BuildMode`.
`execution()` returns `Execution::TrieLftj(R::kind(self.build))`, so the
report's identity axes still come from the cell (issue #56).
`build_relation` calls `R::build(header, self.build, tuples)`. That is the
one construction site: `load`, `load_with_tuples`, `build_from_tuples` and
the `insertion` closures all reach it, so `insertion` times the mode the
axis names. `build_mode_axes(&self)` delegates to `R::build_mode_axes(self.build)`.
`TrieLftj::new(optimiser, build)` threads it through.

`for_pair`, `for_structure` and `Sweep::expand` attach `build` to the
ColumnTrie cell only, as they attach `config` to the hash cell only.

### `kermit` binary: CLI

A `BuildChoices { ds_build: Option<ColumnTrieBuildMode> }` group
(`--ds-build bulk|incremental`) is flattened into `bench ds`, `bench run` and
`bench join`. `validate_build_choices` (in `DsChoices::resolve`) accepts
`column-trie` and `all`, and rejects anything else with
"`--ds-build is only valid with --indexstructure column-trie (or all); got --indexstructure TreeTrie`",
mirroring `validate_config_choices`. Under `-i all` the mode reaches only the
ColumnTrie cell. `bench join --output` builds its second relation set
through `load_query_runner`, which receives the mode like it receives the
config.

### Keeping old and new reports apart

`schema_version` stays at 3. The repo's rule is to bump whenever what a
metric measures changes, so that incomparable values can't be mixed
unnoticed. Here the axis does that job:

- `insertion` still measures the `from_tuples` build. The axis records which
  build, and the kermit-lab back-fill labels the old rows.
- `space` is unchanged by construction (identical capacities, pinned by the
  equivalence test).
- A v4 would stop kermit-lab loading new rows alongside every pre-#84 row,
  which loses the one cross-version comparison the thesis wants: old
  `incremental` against new `bulk` on the same data. The perf fixes for #67
  and #83 didn't bump either.

kermit-lab (`python/kermit-lab/kermit_lab/defaults.py`) gains a
structure-scoped registry next to `AXIS_DEFAULTS`. A missing
`ds_build_mode` is filled with `"incremental"` only on rows whose
`data_structure` is `"ColumnTrie"`, and only when the column is present
(matching the existing rule). TreeTrie and HashTrie rows stay NaN, because
they have no build-mode axis.

`render_all` draws an ablation figure for every optimisation column with two
or more values, on its default `iteration` phase. With old rows back-filled
`incremental` beside new `bulk` ones, that would chart an iteration-time
"build-mode ablation" — but both modes build the same trie, so the difference
is noise or binary drift. `presets.ablation` therefore refuses
`ds_build_mode` on any phase other than `insertion` and `end_to_end`, raising
`InsufficientAxesError`, which `render_all` already logs as a skip
(supervisor review, checkpoint 1). The back-filled rows are for continuity;
thesis comparisons of the two builds should come from one binary
(`--ds-build incremental` against the default), since reports carry no binary
identity. BENCHMARKING.md says so.

## Out of scope

- **TreeTrie's build.** It also sorts and then inserts, but loads in about
  1.9 s; it is not a distortion (Priority 6).
- **Faster incremental `insert`.** A binary search or last-key fast path in
  `step_layer` would help incremental users, but it changes the
  `incremental` mode, which must stay the pre-#84 routine.
- **`insert_all` on an empty trie** routing through the bulk path.
- **`ColumnTrieIter::up`'s linear scan** (`column-trie.md` already lists it).
- **kermit-lab's unscoped back-fill.** `apply_axis_defaults` fills every
  row of a present column regardless of structure, so TreeTrie rows already
  get `ds_layout_hasher = "sip"`. Pre-existing; can be filed separately.
- **Criterion directory collisions.** Group names and function ids don't
  encode axes, so two runs that differ only in `--ds-build` overwrite each
  other's `target/criterion/` directories unless their `--name` differs.
  This already holds for the hasher and the load factor. BENCHMARKING.md's
  ablation loop gets a note; the naming scheme doesn't change.
- **Issue criterion 4**, the authoritative `insertion` re-run.

## Testing

**Equivalence (phase 1, then reshaped in phase 2).** Inline in
`column_trie/implementation.rs`, so it can read the private `layers[i].data`
and `.interval`:

- The LCG in `random_inserts_round_trip_to_sorted_deduped_input` moves into a
  small test helper shared by both tests.
- Grid: several seeds × arity 1–4 × key ranges {2, 5, 50} ×
  n ∈ {0, 1, 2, 17, 200}. Small key ranges make duplicates and long shared
  prefixes common. Inputs are generated unsorted. Under `cfg(miri)` the grid
  shrinks to a couple of seeds and small n.
- For each case, against the oracle (today's sort-then-insert routine in
  phase 1; the `Incremental` mode in phase 2): per layer, `data` and
  `interval` contents **and** `capacity()`; then `tuple_count` and
  `heap_size_bytes`.
- A second assertion inserts the same tuples unsorted and checks the arrays
  match, pinning the canonical-layout invariant.
- Hand-written cases: all duplicates; empty input; arity 0 (no layers,
  count 0); a `#[should_panic]` test for mixed-length tuples.

**Mutation check.** After committing, break the bulk build twice (drop the
duplicate skip; change `depth > diverge` to `>=`), confirm the test fails
each time and that the mutant applied, undo the exact edit (never
`git checkout`), and re-run the test to see it pass.

**Phase 1 axis.** A CLI test that ColumnTrie reports from `bench ds` and
`bench run` carry `ds_build_mode: "bulk"`, and TreeTrie and HashTrie reports
carry no such key.

**Phase 2.**

- `kermit-ds/tests/trie_tests.rs` and `parquet_tests.rs`: a
  `ColumnTrieIncremental = BuiltWith<ColumnTrie, Incremental>` alias through
  `relation_trie_test_suite!` and `parquet_test_suite!` (the
  `HashTrieSipDense` precedent).
- `define_multiway_join_test_suite_for_build_mode!(Rel, Algo, Opt, Provider)`
  in `kermit/tests/common/macros.rs`, the same shape as
  `define_multiway_join_test_suite_with_config!` (`:414-441`): one module per
  invocation, `type <Rel><Provider> = kermit_ds::BuiltWith<Rel, Provider>`,
  then `define_multiway_join_test_suite!`. It delegates rather than copying
  the pattern list, so patterns added to the base suite (#78) reach it
  automatically. The standard's earlier sketch was `(Rel, Algo, Mode)`; the
  optimiser parameter is added, as on every suite since optimisers landed.
- Invocations in `kermit/tests/join_tests.rs`: ColumnTrie × LeapfrogTriejoin
  × {Lexicographic, Cardinality} with an `Incremental` provider. The existing
  plain-ColumnTrie invocations already cover the `bulk` default.
- `kermit/tests/result_allocation.rs` gets no new cells: it measures after
  the build, and both modes produce identical tries.
- `kermit/tests/cli_column_trie_build_mode.rs`: `bench ds` records `bulk` by
  default and `incremental` with the flag; the flag is rejected on tree-trie
  and hash-trie; `bench run -i all -a all --ds-build incremental` puts the
  axis only on ColumnTrie rows; `bench join` records the mode.
- Unit tests for `DsChoices::resolve`, and the guard test pinning
  `axis_value()` to the clap names.
- kermit-lab: `test_defaults.py` (ColumnTrie NaN becomes `incremental`;
  other structures stay NaN; explicit `bulk` untouched); a column-trie
  `bench ds` case in the real-binary contract test `test_contract.py`; and
  `test_render_all.py` over mixed old/new ColumnTrie rows (no
  `ablation-ds_build_mode` on `iteration`, one on `insertion`; the preset
  raises `InsufficientAxesError` for `iteration`).

## Verification

- **Gate, at the end of each phase:** `cargo test -p kermit-ds`, the
  ColumnTrie join suites and `result_allocation`, the full `cargo test`,
  `cargo clippy --all-targets` with `-Dwarnings`, `cargo doc` with
  `-Dwarnings`, `MIRIFLAGS=-Zmiri-disable-isolation cargo miri test -p kermit-ds`,
  and (phase 2) kermit-lab's pytest with `KERMIT_BIN` set. Cargo runs in the
  foreground with `CARGO_BUILD_JOBS=2`; formatting only via
  `nix develop --command cargo fmt --all`.
- **Evidence, phase 1:** one indicative before-and-after
  `bench ds -i column-trie --metrics insertion` on WatDiv `friendof` (34 s at
  the 2026-09-10 prelim), both binaries built in `nix develop`, run outside
  the repo, following the A/B recipe (distinct `--name` per run).
- **Evidence, phase 2:** the same run with `--ds-build incremental` should
  land near the pre-#84 timing, showing the old routine survived intact.

## Documentation

- `docs/data-structures/column-trie.md` (phase 1): the `from_tuples(n)`
  complexity row becomes O(n · a) after the O(n · a · log n) sort; a short
  construction paragraph with the worked example traced through divergence
  depths (0, 1, 0 for `{(1,2),(1,3),(2,4)}`); the canonical-layout invariant;
  "expensive inserts" narrowed to `insert`, here and in the struct's doc
  comment. Phase 2 adds an "Optimizations" section (modes, default, axis,
  flag, when `incremental` is useful), in the shape of `hash-trie.md`'s.
- `docs/specs/optimization-standard.md`: the BuildMode section's example
  becomes ColumnTrie; the CLI section's "hypothetical" BuildMode becomes
  real; the `BuildMode` trait example; a `BuildModeRelation` / `BuiltWith`
  section beside `ConfigurableRelation` / `Configured`; the "implemented
  today" table and the macro sentence; the stale "Bench-report axes merge |
  main.rs" row in "Where to look" (the code is in `bench/run.rs`,
  `bench/ds.rs` and `execution.rs`).
- `docs/specs/bench-report-schema.md`: the `ds_build_mode` axis and the
  structure-scoped back-fill.
- `CLAUDE.md`: Priorities item 1 (the macro is no longer "not yet
  implemented"); the trait hierarchy (`BuildModeRelation`); Testing Patterns
  (the new macro); the optimisation recipe's steps 2, 4 and 5; the "sweeps
  are cells" gotcha (`TrieLftj(SortedTrie)` now carries a mode).
- `ARCHITECTURE.md` (the `Execution` variants; the stale claim that HashTrie
  is the only `HasOptimizationAxes` implementor), `BENCHMARKING.md`
  (`build_relation` honours `--ds-build`; the ablation loop and the
  `--name` note) and `USAGE.md` (the flag).

## Commit sequence

| # | Phase | Commit |
|---|---|---|
| 1 | 1 | `perf(kermit-ds): ColumnTrie builds from sorted tuples in one pass (#84)`, with the equivalence test |
| 2 | 1 | `feat(bench): ColumnTrie reports carry ds_build_mode (#84)`, the `build_mode_axes` hook and its CLI test |
| 3 | 1 | `docs(column-trie): the one-pass build (#84)` |
| 4 | 2 | `refactor(kermit): resolved --ds-* choices travel as one DsChoices` |
| 5 | 2 | `feat(kermit-ds): ColumnTrieBuildMode, BuildModeRelation and BuiltWith (#84)` |
| 6 | 2 | `feat(kermit): --ds-build selects ColumnTrie's build mode (#84)` |
| 7 | 2 | `test(kermit): define_multiway_join_test_suite_for_build_mode! (#84)` |
| 8 | 2 | `feat(kermit-lab): back-fill ds_build_mode for pre-#84 ColumnTrie rows (#84)` |
| 9 | 2 | `docs: ColumnTrie is the first BuildMode consumer (#84)` |

Mutation checks run on commits 1, 2 and 6. Plain commits; no amend, no push.

## Coordination

- A supervising session reviews at three checkpoints: the implementation
  plan before any code, after each phase's commits, and before any merge or
  push.
- The #78 session (`aidanb/78`) is adding placeholder patterns to
  `define_multiway_join_test_suite!` and editing CLAUDE.md. Both branches
  touch `kermit/tests/common/macros.rs` and CLAUDE.md, so expect a textual
  conflict. Whichever lands second merges `origin/master` in (never rebase)
  and re-runs the gate.

## Acceptance (issue #84)

| Criterion | Where it is met |
|---|---|
| `from_tuples` produces exactly the same `data` and `interval` arrays, checked by a property test on random inputs including duplicates and empty relations | The equivalence test (phase 1), which also compares capacities |
| `heap_size_bytes` unchanged for the same input | The same test; guaranteed by push-only construction |
| `column-trie.md`'s complexity table reflects the new build | Commit 3 |
| Re-run the authoritative re-benchmark's `insertion` step for all three structures | Out of scope here; taken up by the supervising session with the user. Phase 1's axis keeps those rows correctly labelled whenever it runs. |
