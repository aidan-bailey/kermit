# Dependent Optimisations: Prerequisites Between Axes

**Date:** 2026-10-07
**Status:** design, approved 2026-10-07; implemented on branch `aidanb/dependent-optimisations` (as-implemented notes below)
**Amends:** [`optimization-standard.md`](optimization-standard.md) (Amendment 3)
**Retrofits:** the presized parallel build of
[`2026-10-06-hash-trie-presized-parallel-build-design.md`](2026-10-06-hash-trie-presized-parallel-build-design.md)

## Summary

The optimisation standard treats every axis as independent: a flag is checked
against the structure that has its axis, the test obligation is per axis, and
the sweep FAQ loops over a full Cartesian product. Nothing can say that one
optimisation depends on another. One dependent optimisation has nevertheless
landed: `--ds-build hash-trie=parallel:N` runs the exact radix-merge build
under `--ds-config root-capacity=grow` and the paper's presized build under
`root-capacity=tuples`. Two processes share one axis value, and a report's
`ds_build_mode` column cannot be read without the config column beside it.

This design adds one rule and one mechanism:

- **One value, one behaviour.** An axis value names exactly one shape, value
  or process, whatever every other axis is set to. An optimisation whose
  behaviour would differ under another axis is two optimisations, and each
  gets its own value.
- **Prerequisites.** The only dependency the standard admits is "value `v`
  of axis B is valid only when axis A has a value in set S". Prerequisites
  live in one table beside `DsFlag::structures`, the CLI rejects a run that
  violates one, and every dependent value is tested as a stacked cell.

The presized build is retrofitted to the rule: it becomes
`--ds-build hash-trie=presized:N`, which requires `root-capacity=tuples`.
`parallel:N` is then one build under every config.

## Goal

Every number the thesis reports must be readable from its axes alone. A row
with `ds_build_mode: "parallel:8"` should name one build to a reader or a
kermit-lab pivot, without a second column or the prose. The standard already
guarantees this per axis: a flag the structure ignores is rejected (#86), so a
report never carries an axis that did nothing. Dependencies between
optimisations are the one place the guarantee breaks today.

## The rule

An optimisation's behaviour is a function of its own axis value. Concretely:

- A Layout marker's code is the same under every Config and BuildMode.
- A Config value is read on the same path under every Layout and BuildMode.
- A BuildMode runs the same process under every Layout and Config. Where a
  process cannot run under some config, that config is a prerequisite, not a
  branch.

The sentence the presized design promised the standard, "a mode may pick its
process by Config value", is withdrawn. It was written into the standard:
`optimization-standard.md`'s BuildMode table said a mode may choose its
process by a Config value (added in 64b8823, from that design's
§ Placement), and this branch removes it. The BuildMode
equivalence rule (Amendment 2) is unchanged: every mode builds the same
contents and capacities as `serial` *under the same config*.

### What a prerequisite can express

A prerequisite is a row `(dependent value, required axis, allowed values)`.
It can say:

- a value needs another value (`presized:N` needs `root-capacity=tuples`);
- a value needs one of several values, since the allowed set may have more
  than one member (a knob that needs "a parallel build" names both parallel
  modes);
- a value in one category needs a value in another (BuildMode on Config here;
  Config on Layout, Config on BuildMode and the rest are the same row shape);
- a conflict, written as a prerequisite on the values that remain allowed;
- a chain, by evaluating every row against the one resolved `DsChoices`,
  which needs no transitive machinery.

A prerequisite applies only on the structure that has both axes, because both
flags pass the per-structure check first.

### What it deliberately cannot express

- **Conditional behaviour.** Forbidden by the rule; split the value instead.
- **Implication.** No flag sets another flag. A missing prerequisite is a
  usage error that names the flag to add. Implying would be the first time a
  flag changed another flag's reported value, and the user should state every
  axis they run.
- **Dependence on a bench-level choice**: the algorithm, the optimiser,
  `--column-orders`. Those are cell validity, owned by `Execution`, not axes.
- **Dependence on the data** ("radix needs more than K tuples"). That is a
  fallback inside the build, and the BuildMode rule already requires the
  result to be the same either way.

## Rust shape

### The table (`kermit/src/options.rs`)

One enum, one variant per prerequisite, beside `DsFlag`:

```rust
/// A `--ds-*` value that is valid only under another: the one table of
/// prerequisites between optimisation axes (optimization-standard.md
/// § Dependencies). A row applies on the structure that has both axes.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum Prerequisite {
    /// `--ds-build hash-trie=presized:N` fills a root sized before any
    /// tuple arrives, so it needs `--ds-config root-capacity=tuples`.
    PresizedBuildNeedsPresizedRoot,
}

impl Prerequisite {
    /// Every row, for the check and the guard tests.
    pub(crate) const ALL: &'static [Prerequisite] = &[Self::PresizedBuildNeedsPresizedRoot];

    /// `Some(violation)` when `choices` selects the dependent value without
    /// its prerequisite.
    pub(crate) fn violated(self, choices: &DsChoices) -> Option<Violation> {
        match self {
            | Self::PresizedBuildNeedsPresizedRoot => match choices.build.hash_trie {
                | HashTrieBuildMode::Presized(threads)
                    if choices.config.root_capacity != RootCapacity::Tuples =>
                {
                    Some(Violation {
                        dependent: format!("--ds-build hash-trie=presized:{}", threads.get()),
                        requires: "--ds-config root-capacity=tuples",
                        actual: format!(
                            "root-capacity={}{}",
                            choices.config.root_capacity.axis_value(),
                            if choices.config.root_capacity == RootCapacity::default() {
                                " (the default)"
                            } else {
                                ""
                            }
                        ),
                    })
                },
                | _ => None,
            },
        }
    }
}

/// What a violated [`Prerequisite`] reports.
pub(crate) struct Violation {
    /// The flag and value the user gave: `--ds-build hash-trie=presized:8`.
    pub dependent: String,
    /// The flag and value it needs: `--ds-config root-capacity=tuples`.
    pub requires: &'static str,
    /// What the required axis resolved to: `root-capacity=grow (the default)`.
    pub actual: String,
}
```

An enum rather than a slice of closures so the compiler flags an unhandled
variant, and so a row is readable as one `match` arm with its reason in its
doc comment. Adding a prerequisite is one variant, one arm, one entry in
`ALL`.

### The check (`DsChoices::resolve`)

`DsChoices::resolve` already validates the three flag groups against `-i` and
then resolves them. The prerequisite check runs last, on the resolved value:

```rust
let choices = Self { … };
if let Some(v) = Prerequisite::ALL.iter().find_map(|p| p.violated(&choices)) {
    anyhow::bail!("{} requires {}; got {}", v.dependent, v.requires, v.actual);
}
Ok(choices)
```

The message for the retrofit:

```text
--ds-build hash-trie=presized:8 requires --ds-config root-capacity=tuples; got root-capacity=grow (the default)
```

Every command resolves through `DsChoices::resolve` (`kermit join`, `bench
join`, `bench ds`, `bench run`), so the check reaches all of them without a
second call site. `resolve_sweep` (`bench/run.rs`) needs no change: a
prerequisite is about values, not cells, and `-a` cannot change a value.
`kermit join` takes neither `--ds-config` nor `--ds-build`, so no
prerequisite can arise there today; it still runs the check, so a future
flag it does take is covered.

### The library boundary (`kermit-ds`)

The CLI is the only place that can reject a run, but the constructor is a
public API and a test can call it with any pair. A dependent value under a
missing prerequisite is a broken invariant at the constructor: it panics on
the same condition, with a message naming the prerequisite
(`hash-trie=presized:N requires root-capacity=tuples; got
root-capacity=<v>`), as `from_tuples` already panics on an arity
mismatch (Priorities item 5: internal panics on broken invariants are
acceptable; the CLI makes them unreachable). For the retrofit, in
`HashTrie::from_tuples_with_config_and_build_mode`:

```rust
| HashTrieBuildMode::Presized(threads) => {
    assert_eq!(
        config.root_capacity,
        RootCapacity::Tuples,
        "hash-trie=presized:{} requires root-capacity=tuples; got root-capacity={}",
        threads.get(),
        config.root_capacity.axis_value(),
    );
    Self::from_tuples_partitioned(header, config, tuples, |root, arity, tuples| {
        parallel::fill_presized_root::<H, P, E>(root, arity, tuples, threads, config.load_factor)
    })
},
```

and the `Parallel` arm loses its `match config.root_capacity`: it calls
`parallel::fill_root` under every config.

A `Result` here would force every `from_tuples_with_build_mode` caller (the
families, `Configured`, `BuiltWith`, the test macros) to handle an error no
CLI run can produce; the panic keeps the seam total for them and the CLI
total for users.

### Layout prerequisites

No Layout depends on a Layout today. When one does, the table row is
mandatory (it produces the usage error), and the implementer may additionally
make the combination unrepresentable by a trait bound, as `NoPruning` makes
`Singleton` uninhabited. A bound alone is not enough: `with_hash_trie_layout!`
expands every combination, so a rejected combination needs the CLI check to
run before the macro's arm is reached. The first consumer decides how its arm
is written; the standard only requires the row and the test.

## CLI surface

No new flag. The retrofit adds one value to an existing flag:

```text
--ds-build hash-trie=serial|radix:<bits>|parallel:<threads>|presized:<threads>
```

- `presized:N`, N in 1..=1024, the same bound and parser rule as
  `parallel:N` (digits only, `Threads::new`).
- `HashTrieBuildMode` gains `Presized(Threads)`; `FromStr` and `axis_value`
  gain the arm; the `expected …` clause of the parse error lists it.
- `BuildChoices`'s doc comment and `bench --help` text list it.

## Bench report and kermit-lab

The report changes nothing structural. `ds_build_mode` gains the value
`presized:N` on HashTrie rows; `ds_config_root_capacity` is already emitted
on every HashTrie row. No `schema_version` bump: a new value of an existing
key, like `radix:8` and `parallel:N` before it. `bench-report-schema.md`'s
`ds_build_mode` entry and its history table gain the value.

kermit-lab:

- `frame.threads_of` recognises `presized:N` as well as `parallel:N`, so the
  `threads` column covers the presized curve, and `frame.build_of` names a
  threaded mode's build (`parallel` or `presized`). The speedup's baseline
  stays `serial`, and its case keys include `ds_config_root_capacity`, so
  `presized:N` is compared with `serial` under `tuples`, which is the
  comparison the scaling record makes. The case keys alone do not keep the
  arms apart: `analysis.speedup_table` keys its arms by build mode and
  threads and emits `ds_build_mode` in each row, since by `threads` alone
  `parallel:4` and `presized:4` under `tuples` would have been pooled. The
  `speedup` preset draws one line per build and titles a presized-only
  figure "Presized build speedup".
- No mirror of the prerequisite table. A report is written by a binary that
  already rejected every invalid pair, so kermit-lab never sees one.
- No rewrite of old values. `SCOPED_AXIS_DEFAULTS` back-fills *missing* axes
  only; it never changes a value a report carries. Reports written before the
  split that carry `parallel:N` with `root_capacity = "tuples"` timed the
  presized build. The one such run is
  `kermit-bench-runs/hash-trie-presized-scaling-2026-10-06/`; the scaling
  record in `parallel-build.md` says so beside its command lines.

## Test obligation

For every prerequisite row:

1. **The dependent value runs the full suite as a stacked cell.** The
   prerequisite is a type (`Configured<R, Provider>` for a Config, a Layout
   alias for a Layout), and the dependent value is applied on top
   (`BuiltWith<…, Provider>` for a BuildMode, `Configured<…, Provider>` for a
   Config): 16 patterns under every compatible algorithm and optimiser, via
   the existing macros. The presized block in `join_tests.rs` is already this
   shape (`Configured<HashTrieSip, PresizedRoot>` under a build-mode
   provider); it changes its provider from `HashParallel2` to `HashPresized2`.
2. **The CLI rejects the dependent value without its prerequisite**, pinning
   the message, in the structure's `cli_*` test file
   (`cli_hash_trie_build_mode.rs` for the retrofit).
3. **The constructor panics without the prerequisite**, pinned with
   `#[should_panic(expected = "requires root-capacity=tuples")]`, in the
   structure's crate.
4. **Every row in `Prerequisite::ALL` is reachable**: a guard test in
   `options.rs` builds, for each variant, a `DsChoices` that violates it and
   one that satisfies it, so a row whose `violated` arm can never fire is
   caught. The retrofit's two cases are `presized:2` under `grow` and under
   `tuples`.
5. **The dependent value's own category obligation** is unchanged: a
   BuildMode still needs its equivalence test (`presized_parallel_builds_are_*`
   in `parallel.rs`, which already test the presized fill under `tuples`).

## Retrofit of the presized build

Renames `parallel:N` under `root-capacity=tuples` to `presized:N`. No
algorithm changes; `fill_presized_root` and `fill_root` are untouched.

| Where | Change |
|---|---|
| `kermit-ds/src/ds/hash_trie/build_mode.rs` | `Presized(Threads)` variant; `FromStr` arm sharing `parallel`'s digit rule; `axis_value` → `presized:N`; round-trip and pinned-value tests |
| `kermit-ds/src/ds/hash_trie/implementation.rs` | `from_tuples_with_config_and_build_mode`: `Parallel` arm always `fill_root`; new `Presized` arm asserts `Tuples` then `fill_presized_root`; `from_tuples_partitioned` doc |
| `kermit-ds/src/ds/hash_trie/parallel.rs` | module doc: two modes, not one mode under two configs; `root_capacity_selects_the_parallel_path` becomes `presized_build_reaches_its_own_path`, `presized_build_requires_a_presized_root` (`#[should_panic]`) and `parallel_build_merges_under_every_root_capacity`; `ParallelBuild.deferred` stays `Option` (`Some` iff presized) |
| `kermit-ds/src/ds/hash_trie/identity.rs` | comments naming the mode |
| `kermit-ds/src/configured.rs` | the `BuiltWith<Configured<…, Presized>, TwoThreads>` test uses `Presized(2)` |
| `kermit-ds/tests/hash_trie_tests.rs`, `parquet_tests.rs` | `*PresizedParallel2` aliases build with `Presized(2)` |
| `kermit-ds/src/test_hooks.rs` | doc: `take_hash_trie_parallel_builds` records `parallel:N` and `presized:N` builds |
| `kermit/src/options.rs` | `Prerequisite`, `Violation`, the check in `DsChoices::resolve`, the guard test; `--ds-build` docs list `presized:<threads>` |
| `kermit/src/execution.rs` | `hash_trie_families_build_with_their_parallel_mode` selects the presized path by mode, not config |
| `kermit/tests/join_tests.rs` | `HashPresized2` provider; the "BuildMode × Config" block uses it; the comment states the prerequisite |
| `kermit/tests/cli_hash_trie_build_mode.rs` | `cli_bench_ds_records_a_presized_parallel_build` and `cli_bench_run_verifies_presized_parallel_builds` pass `presized:N` with `root-capacity=tuples`; new `cli_rejects_presized_without_a_presized_root` pins the message; the malformed-modes test gains `presized` without a count |
| `python/kermit-lab/kermit_lab/frame.py` | `threads_of` accepts `presized:N`; `tests/test_frame.py` row |
| `docs/specs/optimization-standard.md` | Amendment 3 (below); BuildMode examples row; `--ds-build` CLI example; implemented-today table row `HashTrie presized build (presized:N, requires root-capacity=tuples)`; "Where to look" row for the table |
| `docs/specs/bench-report-schema.md` | `ds_build_mode` values; history row (no bump) |
| `docs/data-structures/hash-trie.md` | § Build modes table: a `Presized(N)` row with its prerequisite; the "presized parallel build" paragraph and the test list |
| `docs/data-structures/parallel-build.md` | § HashTrie: two modes; § The presized build titled by mode; the scaling record's commands note the pre-split spelling |
| `docs/specs/2026-10-06-hash-trie-presized-parallel-build-design.md` | a dated note at the top: superseded in spelling by this design; § Placement's rule withdrawn |
| `CLAUDE.md` | the `--ds-build` example line and the `BuildModeRelation` / `HashTrieBuildMode` bullet; the `test-hooks` bullet |

Records: the scaling result of 2026-10-06 was measured at b882bd6 with
`--ds-build hash-trie=parallel:N --ds-config root-capacity=tuples`. Its
numbers stand; its command lines are annotated, not re-run.

## Amendment 3 to the optimisation standard

To be added under a new `## Dependencies between optimisations` section after
the three categories, and referenced from "How to classify":

> **One value, one behaviour.** An axis value names exactly one shape, value
> or process, whatever every other axis is set to. If an optimisation's
> behaviour would differ under another axis, it is two optimisations; give
> each its own value. (Adopted 2026-10-07; the presized parallel build, which
> had been `parallel:N` under `root-capacity=tuples`, became `presized:N`.)
>
> **Prerequisites.** The only dependency the standard admits: value `v` of
> axis B is valid only when axis A has a value in set S. Declare it as a
> variant of `Prerequisite` in `kermit/src/options.rs`, the one table of
> prerequisites, which `DsChoices::resolve` checks after resolving every
> `--ds-*` flag. A violation is a usage error naming the flag to add; no flag
> ever implies another. The constructor asserts the same condition. The
> dependent value is tested as a stacked cell (`BuiltWith<Configured<…>>` and
> the like) through the full suite, the CLI rejection is pinned, and a guard
> test reaches every row.

The BuildMode table's "Examples" row becomes "… HashTrie serial / radix:K /
parallel:N / presized:N (requires `root-capacity=tuples`) ✓".

## Out of scope

- A sweep syntax that expands prerequisites automatically. The sweep FAQ's
  shell loop is unchanged; an invalid pair in the loop fails fast with the
  usage error.
- Rewriting the values of existing reports.
- Any change to `fill_presized_root`, `fill_root`, the equivalence
  guarantees, or the paper-parity board (#105); the presized build's row on
  the board is spelled `presized:N` from now on, nothing else moves.

## As implemented (2026-10-07)

What landed differs from the sketches above in these places:

- `Violation` derives `Clone, Debug, PartialEq, Eq`, so tests compare it
  whole. The guard test `every_prerequisite_is_reachable` is row-generic:
  each row supplies a violating and a satisfying `DsChoices` and the
  `Violation` it must report, and every row is checked not to fire on the
  default `DsChoices`. Its sibling `ds_choices_resolve_rejects_a_violated_prerequisite`
  pins the message through `DsChoices::resolve`, for a default and an
  explicit `root-capacity=grow`.
- `Prerequisite::ALL`'s doc warns that nothing checks it against the enum,
  so a variant missing there is never checked.
- The parser has one arm per threaded mode, both over a shared
  `parse_threads`.
- The test aliases are `HashTrieSipPresized2`, `HashTrieSipLazyPresized2` and
  `HashTrieFxPrunedPresized2` (not `*PresizedParallel2`), built with a
  `HashPresized2` provider; `join_tests.rs` also runs `HashTrieSipPresized2`
  under `AnyOrders`.
- Files touched beyond the retrofit table: `hash_table.rs` (comments naming
  the mode); `relation.rs` (the `BuildModeRelation` panic doc);
  `hash_trie/mod.rs` and `ds/mod.rs`, which widen the parallel-build drain's
  cfg to `any(test, feature = "test-hooks")`; `configured.rs`, whose
  stacked-marker test now asserts the presized record; `BENCHMARKING.md`;
  `USAGE.md`; kermit-lab's `analysis.py`, `presets.py` and `README.md`; and
  `CLAUDE.md`'s Priority 1 sentence, recipe step and gotcha.
- The 2026-10-06 design's note also supersedes its § CLI and kermit-lab.
