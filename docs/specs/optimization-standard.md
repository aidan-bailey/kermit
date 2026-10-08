# Optimization Standard

**Status:** Normative. This is the single source of truth for the optimization
standard — both the precise rules (categories, axis namespaces, test
obligations) and the narrative guide with worked examples.

---

## Why this exists

Kermit's earliest benchmarks compared **(data structure × algorithm)** pairs: `TreeTrie + LeapfrogTriejoin` vs `ColumnTrie + LeapfrogTriejoin`, etc. The bench report axes were two-dimensional, and `kermit-lab` could pivot on them cleanly.

Then came the SIGMOD 2020 hash-trie-join paper, whose §3.3 enumerates **seven optimizations** that change how a data structure is built or navigated:

- Pointer tagging (`hash || tagged_ptr` in 16-byte buckets)
- Singleton pruning (collapse single-tuple subtries)
- Lazy child expansion (build inner nodes on first probe)
- Parallel build (morsel-driven multi-thread insert)
- Radix partitioning (pre-partition tuples by hash bucket)
- Choice of hash function (AquaHash, MurmurHash, FxHash, ...)
- Persistent secondary index reuse

The paper's experiments (Table 6, an ablation study) treat these as **a third benchmark axis**: hold (DS, algorithm) fixed, vary one optimization at a time, observe the effect on runtime.

Kermit's old benchmark model couldn't represent that. There was no place in the bench-report JSON to say "this run had singleton pruning enabled," no CLI flag to toggle it, no convention for naming the bench axis. Every contributor adding an optimization would invent their own scheme, and `kermit-lab` would need bespoke parsing for each.

**The optimization standard is the answer.** It formalizes three structural categories of optimization, prescribes their Rust shape, CLI surface, bench-report axis naming, and test obligation. Adding an optimization becomes a recognizable recipe — like adding a new data structure or algorithm before it.

---

## The three categories, by example

### How to classify

Before reaching for a category, ask what *kind* of thing the optimization
is. The rule (adopted 2026-09-08 by Amendment 1 of
[`2026-09-08-singleton-pruning-config-design.md`](2026-09-08-singleton-pruning-config-design.md)):

- **Layout** = a **shape**: which variants, encodings, or hash functions
  exist. Zero cost for non-users, by monomorphisation.
- **Config** = a **value** read on a path the code already takes: a
  threshold, a capacity, a seed. Replacing a constant with it adds no
  branch.
- **BuildMode** = a **process** yielding the same shape.
- An optimisation that would behave differently under another axis is **two** optimisations, each a value of its own; see [Dependencies](#dependencies-between-optimisations).

The discriminating question is the **non-user tax**: *would a run that
leaves this optimization off pay anything for its existence?* If yes — an
extra enum variant to match on, an extra field to carry, an extra branch in
a probe loop — it is a shape, and it belongs in a type parameter where the
off instantiation compiles back to the code that predates it. A value knob
whose off state still costs something is a Layout wearing a Config's
clothes.

This rule was learned the hard way. Singleton pruning shipped as a Config
first, and pruning-*off* runs measured ~10 % slower on join iteration than
the pre-pruning `HashTrie` — ~4 % for the third `HashTrieNode` variant and
~5–6 % for the two-variant iterator frame. **It was reclassified as a
Layout** (`ds_layout_pruning`), with an uninhabited `Singleton` payload
under `NoPruning` so the variant disappears from the enum. The earlier
version of this document used pruning as its Config example; that example
was wrong, and the walkthrough below is now built around the load-factor
cap instead. Do not re-derive the old classification from the paper's
framing: §3.3's optimisations are physical-layout choices, so under this
rule **none of the paper's seven optimisations is a Config**. The honest
Config candidates on `HashTrie` are its tuning constants — the load-factor
cap ✓, the root capacity ✓ (#88), the child capacity ✓ (#107), a hash seed.

### Layout — *changes the type*

A **Layout** option changes the type of the data structure itself. Each combination is a distinct compile-time type, with its own monomorphized code path.

> **Concrete example.** HashTrie's hash function (sip vs fxhash) is a Layout. `HashTrie<SipHashStrategy>` and `HashTrie<FxHashStrategy>` are different types — the compiler generates different machine code for each because the inner `H::hash(key)` call resolves to different functions.
>
> **Second concrete example.** Singleton pruning is HashTrie's other Layout dimension: `HashTrie<H, NoPruning>` and `HashTrie<H, SingletonPruning>` differ in which node variants exist at all, because the policy's `Payload` associated type is uninhabited when pruning is off.
>
> **Third concrete example.** The sorted tries' seek strategy is a Layout of two structures at once: `TreeTrie<S>` and `ColumnTrie<S>` share one implementation per `S: SeekStrategy` (`linear`, `binary`, `galloping`), so the same strategy on both tries isolates the layout. Every strategy returns what `slice::partition_point` returns; only the probes differ.

| Aspect | Layout |
|---|---|
| Runtime cost | Zero (no branches; monomorphized) |
| Binary size cost | Linear in combinations |
| Type system enforcement | Strong (incompatible layouts won't compile together) |
| Switching at runtime | Impossible (it's compile-time) |
| Bench axis key | `ds_layout_<dim>` |
| Examples (potential) | Hasher choice ✓, singleton pruning ✓, seek strategy ✓, lazy expansion ✓, pointer encoding |
| Test obligation | Type alias per combination + `define_multiway_join_test_suite!(<alias>, <Algo>, <Optimiser>)` for each |

### Config — *changes a runtime value*

A **Config** option is a *value* field on a config struct, read on a code path that already exists. One type covers all configurations; substituting the field for a constant adds no branch, so a run at the default pays what it paid before the axis existed.

> **Concrete example (implemented).** The load-factor cap. `HashTable::entry_or_insert_with` already tests `(len + 1) * DEN > capacity * NUM` on every insert to decide whether to double; `NUM`/`DEN` come from `HashTrieConfig::load_factor` instead of two `const`s. `HashTrie` holds the config and passes the cap down through `insert_at`, so nothing is stored per table and space is unchanged.

> **Second concrete example (implemented).** The root capacity (#88). The root table's starting capacity was the constant 4 buckets; `HashTrieConfig::root_capacity` replaces it with a value read once per build: 4 under `grow`, or under `tuples` the smallest power of two that holds the build's tuple count under the load factor (Algorithm 2, line 3, applied to the root). One `match` per trie construction, no per-insert branch, no new node variant.

> **Third concrete example (implemented).** The child capacity (#107). A child table's starting capacity was the constant 4 buckets; `HashTrieConfig::child_capacity` replaces it with a value read once per child: 4 under `grow`, or under `tuples` the smallest power of two that holds the child's list under the load factor (Algorithm 2, line 3). Only a build that groups before it recurses knows a list's length, so the per-tuple `incremental` build has the prerequisite `child-capacity=grow`.

| Aspect | Config |
|---|---|
| Runtime cost | None beyond the comparison the code already performed |
| Binary size cost | Constant |
| Type system enforcement | Weaker (any config is type-compatible with any other) |
| Switching at runtime | Yes (just change the value) |
| Bench axis key | `ds_config_<flag>` |
| Examples (potential) | Load-factor cap ✓, root capacity ✓, child capacity ✓, hash seed |
| Test obligation | Baseline + ≥1 alternate per flag via `define_multiway_join_test_suite_with_config!` ([`kermit/tests/common/macros.rs`](../../kermit/tests/common/macros.rs)) |

### BuildMode — *changes how the structure is built*

A **BuildMode** changes the construction process, not the structure: every mode builds the same contents with the same capacities, so answers and `space` cannot change. Placement that the structure itself leaves free, such as the slot a key takes in a hash table, may differ between modes (Amendment 2, 2026-10-06). Modes are a perf knob, not a semantic change.

> **Concrete example (implemented).** ColumnTrie's build. `ColumnTrie::from_tuples_with_build_mode(header, ColumnTrieBuildMode::Incremental, tuples)` sorts the tuples and inserts them one at a time (the build before issue #84, O(n · a · b)); `ColumnTrieBuildMode::Bulk`, the default, builds every layer in one pass (O(n · a)). Both build identical arrays with identical capacities, which an array-level test pins.

| Aspect | BuildMode |
|---|---|
| Runtime cost | None at query time (build-only) |
| Binary size cost | One constructor variant per mode |
| Type system enforcement | Weak (mode is just a parameter) |
| Output equivalence | Required: every mode builds an *equivalent* structure — the same contents **and** the same capacities at every level, hence the same `HeapSize` — so `space` cannot move. Placement the structure leaves free (a key's slot in a hash table) may differ, and every mode's allocations land at addresses of their own, so `iteration` and `end_to_end` can move and are measured per mode, never assumed unchanged. A sorted trie leaves no placement free, so for it equivalence is array-level identity. Amendment 2 (2026-10-06, [`2026-10-05-parallel-build-design.md`](2026-10-05-parallel-build-design.md) § Amendment 2) relaxed this from array-level identity for every structure. Equivalence is to the structure's single-threaded build under the same config (TreeTrie `serial`, HashTrie `bulk`). A mode runs one process under every config; a mode that cannot run under some config declares the config it needs as a prerequisite rather than branching on it (HashTrie's `presized:N` requires `root-capacity=tuples`, and its `incremental` requires `child-capacity=grow`; see [Dependencies](#dependencies-between-optimisations)). |
| Bench axis key | `ds_build_mode` (single key with mode + params) |
| Examples | ColumnTrie bulk / incremental ✓, TreeTrie serial / parallel:N ✓, HashTrie bulk / incremental (requires `child-capacity=grow`) / radix:K / parallel:N / presized:N (requires `root-capacity=tuples`) ✓ |
| Test obligation | Each non-default mode via `define_multiway_join_test_suite_for_build_mode!` ([`kermit/tests/common/macros.rs`](../../kermit/tests/common/macros.rs)), plus an equivalence test over every mode: the same contents and capacities at every level and the same `HeapSize`. Array-level where the structure fixes placement, as it does for every landed mode but HashTrie's `presized:N`; map-level (each hash table compared by hash, not by slot) where it leaves placement free, as `presized:N` does at the root |

---

## Dependencies between optimisations

**One value, one behaviour** (Amendment 3, 2026-10-07,
[`2026-10-07-dependent-optimisations-design.md`](2026-10-07-dependent-optimisations-design.md)).
An axis value names exactly one shape, value or process, whatever every other
axis is set to. If an optimisation's behaviour would differ under another
axis, it is two optimisations; give each its own value. The presized parallel
build is the precedent: it had been `parallel:N` under `root-capacity=tuples`
and became `presized:N`.

**Prerequisites.** The only dependency the standard admits: value `v` of axis
B is valid only when axis A has a value in set S. Declare it as a variant of
`Prerequisite` in [`kermit/src/options.rs`](../../kermit/src/options.rs), the
one table of prerequisites, which `DsChoices::resolve` checks after resolving
every `--ds-*` flag. A violation is a usage error naming the flag to add; no
flag ever implies another:

```
$ kermit bench run triangle -i hash-trie -a hash-triejoin --ds-build hash-trie=presized:8
Error: --ds-build hash-trie=presized:8 requires --ds-config root-capacity=tuples; got root-capacity=grow (the default)
```

The constructor asserts the same condition (`HashTrie::from_tuples_with_config_and_build_mode`
panics), so a library caller cannot reach the dependent process without its
prerequisite either. The report needs nothing new: both axes are already
columns, and a report can only carry a pair the binary accepted.

Today's rows are both HashTrie's: `presized:N` requires
`root-capacity=tuples`, and `incremental` requires `child-capacity=grow`
(#107: the per-tuple build creates a child on its first tuple, before the
child's list, and so its size, is known).

A prerequisite can name one value or several, cross categories (a BuildMode on
a Config, a Config on a Layout, …), express a conflict (a prerequisite on the
values that remain allowed) and chain (every row is checked against the one
resolved `DsChoices`). It cannot express conditional behaviour (split the
value), implication (state every axis), dependence on the algorithm or
optimiser (that is cell validity, owned by `Execution`), or dependence on the
data (a fallback inside the build, which the BuildMode rule already covers).

**Adding a prerequisite** takes four edits in `kermit/src/options.rs`: a
`Prerequisite` variant whose doc says why; its arm in `Prerequisite::violated`;
its entry in `Prerequisite::ALL` (nothing checks that list against the enum, so
a variant missing there is never checked); and its fixtures in the guard test
`every_prerequisite_is_reachable` (a violating and a satisfying `DsChoices`,
and the `Violation` it must report). Then assert the same condition where the
dependent value is built.

**Test obligation** for a dependent value: the full suite on a stacked cell
(`BuiltWith<Configured<R, Prereq>, Mode>` and the like) under every compatible
algorithm and optimiser; the CLI rejection pinned in the structure's `cli_*`
test; the constructor's panic pinned; and the guard-test fixtures above. A
Layout that depends on a Layout may also be made unrepresentable by a trait
bound. `with_hash_trie_layout!` still expands every combination, so the
forbidden combination's arm then becomes an unreachable arm, guarded by the
CLI check that runs first; the row and the CLI check are still required.

---

## What this looks like at the CLI

Every implemented optimization has its own flag; [What's implemented
today](#whats-implemented-today-whats-available) lists them. The HashTrie
Layout and Config flags, for example:

```bash
# Default — Sip, no pruning, load factor 0.7
kermit bench run triangle -i hash-trie -a hash-triejoin

# Pick FxHash
kermit bench run triangle -i hash-trie -a hash-triejoin --ds-layout-hasher fxhash

# Both implemented categories on one invocation
kermit bench run triangle -i hash-trie -a hash-triejoin \
    --ds-layout-hasher fxhash \
    --ds-layout-pruning on \
    --ds-config load-factor=0.5

# The paper's sizing: every table sized once, the root for its tuples and each
# child for its list (#88, #107)
kermit bench run triangle -i hash-trie -a hash-triejoin \
    --ds-config root-capacity=tuples,child-capacity=tuples,load-factor=0.8
```

The BuildMode category has three consumers: ColumnTrie (#84), HashTrie (#91) and TreeTrie (#94). `--ds-build` takes `structure=mode` pairs:

```bash
# The pre-#84 ColumnTrie build, to reproduce its insertion numbers
kermit bench run triangle -i column-trie -a leapfrog-triejoin --ds-build column-trie=incremental

# HashTrie's per-tuple build (`serial` before #107), to reproduce its insertion numbers
kermit bench run triangle -i hash-trie -a hash-triejoin --ds-build hash-trie=incremental

# HashTrie's radix-partitioned build (#91), 2^8 partitions
kermit bench run triangle -i hash-trie -a hash-triejoin --ds-build hash-trie=radix:8

# TreeTrie's morsel-driven parallel build (#94), on 8 threads
kermit bench run triangle -i tree-trie -a leapfrog-triejoin --ds-build tree-trie=parallel:8

# HashTrie's presized build (#94; hash partitioning per §3.3.2, regions kermit's): requires the presized root of #88
kermit bench run triangle -i hash-trie -a hash-triejoin --ds-config root-capacity=tuples --ds-build hash-trie=presized:8
```

Each category has its own flag namespace — `--ds-layout-<dim>` for Layout, `--ds-config <flag>=<value>,...` for Config (a single flag with comma-separated key=value pairs), `--ds-build <structure>=<mode>[:<params>],...` for BuildMode (one pair per structure, since each structure has its own modes).

When a chosen DS doesn't have a flag's axis, the CLI rejects the flag before anything runs:

```
$ kermit bench ds -i tree-trie --ds-layout-hasher fxhash
Error: --ds-layout-hasher is only valid with --indexstructure hash-trie (or all); got --indexstructure TreeTrie
```

`-i all` passes that check for every flag, but in `bench run` the `-a` selector can then leave the sweep without a cell that has the axis. That is rejected too (#86), so a flag is never silently ignored:

```
$ kermit bench run triangle -i all -a leapfrog-triejoin --ds-config load-factor=0.5
Error: --ds-config applies only to hash-trie, but --algorithm LeapfrogTriejoin leaves this sweep no hash-trie cell; it runs only (ColumnTrie, LeapfrogTriejoin), (TreeTrie, LeapfrogTriejoin). The flag would be silently ignored.
```

Both checks read one table, `DsFlag::structures` in `kermit/src/options.rs`, which lists the structures that have each flag's axis.

---

## What the bench report looks like

The optimization choices flow into the existing `axes` field of `BenchReport`. Here's what you see today for a HashTrie + FxHash run (HashTrie always emits
both Layout axes and its Config axis, so `ds_layout_pruning` and
`ds_config_load_factor` appear even at their defaults):

```json
[
  {
    "schema_version": 4,
    "kind": "ds",
    "axes": {
      "data_structure": "HashTrie",
      "ds_config_load_factor": 0.7,
      "ds_layout_hasher": "fxhash",
      "ds_layout_pruning": "off",
      "arity": 2,
      "relation_bytes": 24,
      "relation_path": "kermit/tests/fixtures/edge.csv",
      "tuples": 4
    },
    "criterion_groups": [
      { "group": "ds", "function": "HashTrie/space", "metric": "space" }
    ]
  }
]
```

The keys `ds_layout_hasher`, `ds_layout_pruning` and `ds_config_load_factor` sit alongside the existing axes. The convention is:

| Prefix | Used for | Example |
|---|---|---|
| `ds_layout_<dim>` | DS layout (compile-time) | `ds_layout_hasher: "fxhash"`, `ds_layout_pruning: "on"` |
| `ds_config_<flag>` | DS config (runtime) | `ds_config_load_factor: 0.5` |
| `ds_build_mode` | DS build mode | `ds_build_mode: "bulk"` |
| `algo_layout_<dim>` | Algorithm layout (reserved) | (none yet) |
| `algo_config_<flag>` | Algorithm config (reserved) | (none yet) |
| `algo_build_mode` | Algorithm build mode (reserved) | (none yet) |

These prefixes are **normative** — downstream tools (`kermit-lab`) rely on them for pivot analysis. A DS that emits `load_factor_cap` instead of `ds_config_load_factor` breaks the convention and trips up the tooling.

---

## The trait family

Four traits in [`kermit-iters/src/optimization.rs`](../../kermit-iters/src/optimization.rs):

### `LayoutOption`

A marker trait. Implementing types are typically zero-sized structs. The `NAME` constant flows into bench-report axes and CLI selectors.

```rust
pub trait LayoutOption {
    const NAME: &'static str;
}
```

Usage:
```rust
struct SipHashStrategy;
impl LayoutOption for SipHashStrategy {
    const NAME: &'static str = "sip";
}
```

A Layout dimension is often more than a marker: it carries the shape it
selects as associated types. `PruningPolicy`
([`kermit-ds/src/ds/hash_trie/pruning.rs`](../../kermit-ds/src/ds/hash_trie/pruning.rs))
is the precedent — its `Payload` is uninhabited in the off instantiation,
which is what makes the off instantiation free:

```rust
pub trait PruningPolicy: LayoutOption + Copy + Default + 'static {
    /// What a `HashTrieNode::Singleton` holds. `Vec<usize>` when pruning
    /// is on; the uninhabited `Never` when it is off.
    type Payload: SingletonPayload;
    /// The iterator frame standing in for a pruned level.
    type Frame<'a>: SingletonFrame<'a>;
    /// Folded by the compiler: `insert_at`'s prune branches are
    /// `if P::ENABLED { … }`.
    const ENABLED: bool;
}

pub enum Never {}   // `impl SingletonPayload for Never { … match *self {} … }`
```

### `ConfigOption`

For a struct of runtime flags. The `axes()` method returns `(suffix, value)` pairs; the bench reporter prepends `ds_config_` (or `algo_config_`) to each suffix.

```rust
pub trait ConfigOption: Default + Clone {
    fn axes(&self) -> Vec<(&'static str, serde_json::Value)>;
}
```

Usage (`HashTrie`'s actual config, in [`kermit-ds/src/ds/hash_trie/config.rs`](../../kermit-ds/src/ds/hash_trie/config.rs)):
```rust
/// An exact percentage in `1..=99`, so the resize test stays integer
/// arithmetic: `(len + 1) * 100 > capacity * percent`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LoadFactor { percent: u8 }   // Default: 70

#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
pub struct HashTrieConfig {
    pub load_factor: LoadFactor,
}

impl ConfigOption for HashTrieConfig {
    fn axes(&self) -> Vec<(&'static str, Value)> {
        vec![("load_factor", Value::from(self.load_factor.as_f64()))]
    }
}
```

### `BuildMode`

For a construction-time mode value. The `axis_value()` method returns a single string formatted as `"<mode>[:<params>]"`.

```rust
pub trait BuildMode {
    fn axis_value(&self) -> String;
}
```

Usage (`ColumnTrieBuildMode`, in
[`kermit-ds/src/ds/column_trie/build_mode.rs`](../../kermit-ds/src/ds/column_trie/build_mode.rs)):
```rust
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, clap::ValueEnum)]
pub enum ColumnTrieBuildMode {
    Incremental,
    #[default]
    Bulk,
}

impl BuildMode for ColumnTrieBuildMode {
    fn axis_value(&self) -> String {
        match self {
            Self::Incremental => "incremental",
            Self::Bulk => "bulk",
        }
        .to_string()
    }
}
```

Unlike a Layout or Config axis, the `ds_build_mode` axis is **not** emitted by the relation's `HasOptimizationAxes`: every mode builds an equivalent relation, so the relation cannot know. The bench family that ran the build reports it, through `RelationFamily::build_mode_axes` (`kermit/src/execution.rs`).

### `HasOptimizationAxes`

The umbrella trait the bench reporter consumes. A DS or algorithm implements it; the impl composes results from the Layout and Config trait methods under the correct key prefixes. A BuildMode axis is not composed here: the bench family that ran the build reports it (see `BuildMode` above).

```rust
pub trait HasOptimizationAxes {
    fn optimization_axes(&self) -> BTreeMap<String, serde_json::Value>;
}
```

Usage (`HashTrie`'s actual impl, in
[`kermit-ds/src/ds/hash_trie/implementation.rs`](../../kermit-ds/src/ds/hash_trie/implementation.rs) —
one `ds_layout_*` axis per Layout parameter plus one `ds_config_*` axis per
config value):
```rust
impl<H: HashStrategy, P: PruningPolicy> HasOptimizationAxes for HashTrie<H, P> {
    fn optimization_axes(&self) -> BTreeMap<String, Value> {
        let mut axes = BTreeMap::new();
        axes.insert("ds_layout_hasher".to_string(), <H as LayoutOption>::NAME.into());
        axes.insert("ds_layout_pruning".to_string(), <P as LayoutOption>::NAME.into());
        for (suffix, value) in self.config.axes() {
            axes.insert(format!("ds_config_{suffix}"), value);
        }
        axes
    }
}
```

The composition is mechanical — the trait family does the heavy lifting.

### `ConfigurableRelation` and `Configured<R, P>` (kermit-ds)

`Relation::new` / `from_tuples` have no parameter for a config value, so
`kermit_ds::ConfigurableRelation` ([`kermit-ds/src/relation.rs`](../../kermit-ds/src/relation.rs))
adds `with_config` / `from_tuples_with_config` / `config()`. Only structures
with a Config axis implement it — today just `HashTrie<H, P>`, whose
`Relation::new` / `from_tuples` delegate to it with
`HashTrieConfig::default()`.

`kermit_ds::Configured<R, P>` ([`kermit-ds/src/configured.rs`](../../kermit-ds/src/configured.rs))
wraps an `R: ConfigurableRelation` with a zero-sized
`P: ConfigProvider<R::Config>` so macro suites can name a configured relation
as one type; declare `P` with `kermit_ds::define_config_provider!`. The
wrapper deliberately does **not** implement `ConfigurableRelation` itself:
its config-carrying constructors would take an arbitrary value that `P`
cannot vouch for. The data structure never sees the marker — its config stays
a runtime value, which is what makes it Config rather than Layout.

### `BuildModeRelation` and `BuiltWith<R, P>` (kermit-ds)

`kermit_ds::BuildModeRelation` ([`kermit-ds/src/relation.rs`](../../kermit-ds/src/relation.rs))
adds `from_tuples_with_build_mode(header, mode, tuples)`. Every mode must build
an equivalent relation (the same contents and capacities; see the BuildMode
rule above), and `Relation::from_tuples` must use the default mode.
`TreeTrie`, `ColumnTrie` and `HashTrie` implement it, and `Configured<R, P>`
forwards it to the relation it wraps. HashTrie also has a Config, and
each trait's constructor fixes the other axis to its default, so it adds one
inherent constructor that takes both,
`from_tuples_with_config_and_build_mode`. A mode with a Config prerequisite
can only be built through it: `from_tuples_with_build_mode` uses the default
config, so it panics for HashTrie's `presized:N`, which requires
`root-capacity=tuples` (see [Dependencies](#dependencies-between-optimisations)).

`kermit_ds::BuiltWith<R, P>` ([`kermit-ds/src/built_with.rs`](../../kermit-ds/src/built_with.rs))
wraps an `R: BuildModeRelation` with a zero-sized
`P: BuildModeProvider<R::BuildMode>`, declared with
`kermit_ds::define_build_mode_provider!`, so macro suites can name a mode as a
type. Unlike `Configured`, it needs no rule that `project` preserve the mode:
projection rebuilds through the default mode, and every mode builds an
equivalent relation. It forwards `HasOptimizationAxes` to the wrapped relation, as
`Configured` does, so a wrapped `ColumnTrie<S>` still reports its Layout axis
(`ds_layout_seek`); the build mode stays a family axis.

---

## Walkthrough: adding a Config value

Suppose you want to make HashTrie's load-factor cap tunable. The recipe:

### 1. Classify

The cap is a *value* the code already reads: `HashTable::entry_or_insert_with`
compares `(len + 1) * DEN > capacity * NUM` on every insert, with `NUM`/`DEN`
supplied as `const`s. Replacing the constants with a field adds no branch, so
a run at the default pays exactly what it paid before. **Category: Config.**

(Contrast with singleton pruning, which *was* this document's Config example
and is now a Layout: it adds an enum variant, so pruning-off runs paid for it.
See [How to classify](#how-to-classify).)

### 2. Define the config struct

In `kermit-ds/src/ds/hash_trie/config.rs`:

```rust
use kermit_iters::ConfigOption;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LoadFactor { percent: u8 }

impl LoadFactor {
    /// A cap of `percent` %, which must lie in `1..=99`.
    pub fn percent(percent: u8) -> Result<Self, InvalidLoadFactor> { … }
    pub fn numerator(self) -> usize { usize::from(self.percent) }
    pub fn denominator(self) -> usize { 100 }
    pub fn as_f64(self) -> f64 { f64::from(self.percent) / 100.0 }
}

#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
pub struct HashTrieConfig {
    pub load_factor: LoadFactor,      // Default: 70 %
}

impl ConfigOption for HashTrieConfig {
    fn axes(&self) -> Vec<(&'static str, serde_json::Value)> {
        vec![("load_factor", serde_json::Value::from(self.load_factor.as_f64()))]
    }
}
```

Keep the value exact (a percentage, not an `f64`) so the code that reads it
does not change kind — the resize test stays integer arithmetic.

### 3. Add a config field to HashTrie

```rust
pub struct HashTrie<H: HashStrategy = SipHashStrategy, P: PruningPolicy = NoPruning> {
    header: RelationHeader,
    root: HashTrieNode<P>,
    config: HashTrieConfig,  // new
    …
}
```

Implement `ConfigurableRelation` (`kermit-ds/src/relation.rs`) for it —
`with_config(header, config)` and `from_tuples_with_config(header, config, tuples)` —
and have `Relation::new` / `Relation::from_tuples` call them with
`HashTrieConfig::default()`, so existing behaviour is unchanged.

### 4. Read the value where the constant was

`HashTrie::insert_at` passes `self.config.load_factor` into
`HashTable::entry_or_insert_with`, which uses it in the comparison it already
made. No new branch, and nothing is stored per table — so space is unchanged
and the non-user tax is zero.

### 5. Extend `HasOptimizationAxes`

```rust
impl<H: HashStrategy, P: PruningPolicy> HasOptimizationAxes for HashTrie<H, P> {
    fn optimization_axes(&self) -> BTreeMap<String, Value> {
        let mut axes = BTreeMap::new();
        axes.insert("ds_layout_hasher".to_string(), <H as LayoutOption>::NAME.into());
        axes.insert("ds_layout_pruning".to_string(), <P as LayoutOption>::NAME.into());
        for (suffix, value) in self.config.axes() {
            axes.insert(format!("ds_config_{suffix}"), value);
        }
        axes
    }
}
```

### 6. Add CLI surface

In `kermit/src/options.rs`, `ConfigChoices` sits beside `LayoutChoices` as a
flattened clap group carrying `--ds-config`, a comma-separated key=value
string parsed into a `HashTrieConfig`. Add the key to `HASH_TRIE_KEYS`
(`["load-factor"]`) and a match arm that parses the value — `parse_load_factor`
accepts a decimal in the open interval (0, 1) with at most two decimal places
and maps it onto `LoadFactor::percent`, so a bad value is a usage error naming
the range. `validate_config_choices` (mirroring `validate_layout_choices`)
rejects the flag on an index structure with no Config axis, so a report can
never carry a `ds_config_*` axis the structure ignored. Which structures have
one is `DsFlag::Config`'s row in `DsFlag::structures`, so a Config value on a
second structure adds that structure to the row. Both groups are wired on
`bench ds` and `bench run`.

### 7. Add tests

Use `define_multiway_join_test_suite_with_config!` (`kermit/tests/common/macros.rs`).
It declares the `Configured<Relation, Provider>` alias inside a module of its
own and runs the 16 standard join patterns on it:

```rust
use kermit_ds::{define_config_provider, HashTrieConfig, LoadFactor};

define_config_provider!(HalfFull, HashTrieConfig, HashTrieConfig {
    load_factor: LoadFactor::percent(50).unwrap(),
    ..HashTrieConfig::default()
});
define_multiway_join_test_suite_with_config!(HashTrieSip, HashTriejoin, LexicographicOptimiser, HalfFull);
```

A config struct literal names the fields it sets and takes the rest from
`..HashTrieConfig::default()`, so adding a value does not break every
provider.

The default-config invocations stay as the baseline (standard: baseline + ≥1
alternate per flag). The DS-level suites run on a configured alias too —
`HashTrieSipDense = Configured<HashTrieSip, NinetyPercent>` in
`kermit-ds/tests/hash_trie_tests.rs`, and the matching `parquet_test_suite!`
in `kermit-ds/tests/parquet_tests.rs`. Add unit tests for the value itself:
the constructor's range, and that a table resizes exactly when `(len + 1)`
crosses the configured cap.

### 8. Document

Update `docs/data-structures/hash-trie.md` § Optimizations § Config flags to
list `load_factor` with its default, where it is read, and its bench-axis
value.

### 9. Verify the bench report

`--report-json` is a `bench`-level flag, so it precedes the subcommand:

```bash
kermit bench --report-json /tmp/lf.json ds -i hash-trie \
    --ds-config load-factor=0.5 \
    --relation kermit/tests/fixtures/edge.csv -m space
jq '.[0].axes' /tmp/lf.json
#   "ds_config_load_factor": 0.5,
```

---

## Walkthrough: adding a Layout dimension

Singleton pruning is the worked example, and it is short because the shape
does the work.

### 1. Define the marker trait, with an uninhabited off payload

A Layout dimension is a trait bound on a new type parameter. Give the trait
associated types for everything the shape adds, and make the off
implementation's versions **uninhabited**:

```rust
pub trait PruningPolicy: LayoutOption + Copy + Default + 'static {
    type Payload: SingletonPayload;      // Vec<usize> on, Never off
    type Frame<'a>: SingletonFrame<'a>;  // SingletonFrameOn on, Never off
    const ENABLED: bool;
}

pub enum Never {}                        // no values exist

pub struct NoPruning;         // LayoutOption::NAME = "off"
pub struct SingletonPruning;  // LayoutOption::NAME = "on"
```

`HashTrieNode<P>` then carries `Singleton(P::Payload)` and the iterator's
frame enum carries `Singleton(P::Frame<'a>)`. Under `NoPruning` neither
variant can be constructed and every arm handling them is dead code.
rustc omits uninhabited variants when it computes a layout, so in practice
the enums are laid out as they were before the dimension existed — an
optimisation rustc performs, not a language guarantee, so pin it with size
tests (below). The non-user tax is zero, which is the whole point of
choosing Layout. Gate the
build-time branches on `P::ENABLED`, a `const` the compiler folds.

### 2. Thread the parameter and emit the axis

`HashTrie<H, P>` gains the parameter with a default (`P = NoPruning`), and
its `HasOptimizationAxes` impl emits `ds_layout_pruning` from
`<P as LayoutOption>::NAME`.

### 3. Add the CLI flag, and multiply the dimensions in exactly one place

`LayoutChoices` gains `--ds-layout-pruning <off|on>` (a `PruningChoice`
`ValueEnum`, default `off`). The flag gets a `DsFlag` variant, listed in
`LayoutChoices::given` and given a row in `DsFlag::structures` naming
`hash-trie`, so it is rejected on non-`hash-trie` selectors and on a `bench
run` sweep that `-a` leaves without a hash-trie cell. Dispatch does **not** repeat the
product: `with_hash_trie_layout!(hasher, pruning, |H, P| …)` in
`kermit/src/options.rs` expands the hasher × pruning cells once, and
`dispatch_run_bench`, `dispatch_ds_bench` and `load_query_runner` (`main.rs`)
all call it. **Adding a third
Layout dimension means adding arms to that macro and nowhere else** — do
not reach for a runtime enum inside the structure, which would reintroduce
the tax the Layout category exists to avoid. That rule is per structure
family. The sorted tries' seek strategy has its own `with_sorted_trie_layout!`
beside it, because the two products share no dimension.

`Execution::HashHtj { hasher, pruning, config }`
(`kermit/src/execution.rs`) records the cell, so the bench report cannot
name a Layout it did not run.

### 4. The test obligation: one alias per combination × the whole suite

Each Layout combination is a distinct type alias, and each alias runs the
full 16-pattern suite under every compatible algorithm and optimiser
(Priorities item 1):

```rust
type HashTrieSip       = HashTrie<SipHashStrategy>;
type HashTrieFx        = HashTrie<FxHashStrategy>;
type HashTrieSipPruned = HashTrie<SipHashStrategy, SingletonPruning>;
type HashTrieFxPruned  = HashTrie<FxHashStrategy, SingletonPruning>;

define_multiway_join_test_suite!(HashTrieSipPruned, HashTriejoin, LexicographicOptimiser);
define_multiway_join_test_suite!(HashTrieSipPruned, HashTriejoin, CardinalityOptimiser);
// … one pair per alias
```

The DS-level `hash_trie_test_suite!` and `parquet_test_suite!` take the same
aliases. Add two `#[test]` size assertions — a growth guard,
`size_of::<HashTrieNode<NoPruning>>() == size_of::<HashTrieNode<SingletonPruning>>()`
(`node_does_not_grow_under_the_pruning_policy` in `implementation.rs`),
and the actual elision witness,
`size_of::<Frame<NoPruning>>() == size_of::<(&HashTrieNode<NoPruning>, usize)>()`
(`off_frame_is_the_bare_table_pair` in `hash_trie_iter.rs`), which shows the
off frame really is the pre-pruning pair — plus a CLI smoke test that the
flag lands in the report (`kermit/tests/cli_hash_trie_layout_pruning.rs`).

---

## Walkthrough: adding a BuildMode

ColumnTrie's build (`ColumnTrieBuildMode`) is the worked example. A BuildMode
is a process, not a shape: the structure gains no type parameter, and its axis
is never read off the relation.

1. **Classify.** Every mode must build an equivalent structure: the same
   contents and the same capacities, hence the same `HeapSize`. A mode that
   changes capacities is not a BuildMode (a capacity knob is a Config). A mode
   may place differently only where the structure leaves placement free
   (Amendment 2). A mode that can run only under another axis's value
   declares a prerequisite, and a process that would differ under another
   axis is two modes (see [Dependencies](#dependencies-between-optimisations)).
2. **Define the mode** (`kermit-ds/src/ds/<name>/build_mode.rs`): a
   `Default + clap::ValueEnum` enum whose `BuildMode::axis_value` is the clap name.
   A mode that carries a value (HashTrie's `Radix(RadixBits)`, TreeTrie's
   `Parallel(Threads)`) cannot derive `clap::ValueEnum`; it implements
   `FromStr` over its `axis_value` strings instead, which
   `BuildChoices::resolved` calls (step 6).
3. **Implement `BuildModeRelation`**, and make `Relation::from_tuples` call it
   with the default mode. TreeTrie keeps `from_tuples` as its serial build and
   dispatches in `from_tuples_with_build_mode` instead.
4. **Pin equivalence** with a test over every mode: contents and capacities
   at every level, and `HeapSize`. Test at array level where placement is fixed
   (`bulk_and_incremental_builds_are_identical`), and as a map of maps where it
   is free.
5. **Carry the mode on the cell and family.** Implement `SortedTrieRelation`'s
   `BuildMode`, `kind`, `build_with` and `build_mode_axes`; the `SortedTrie`
   variant and `TrieLftj::new(build, planner)` hold it, and the axis comes
   from `RelationFamily::build_mode_axes`.
6. **Add the CLI.** `--ds-build` takes `structure=mode` pairs (`BuildChoices`,
   `BuildModes`, `DsChoices.build`). A new consumer:
   - adds its key to `BuildChoices::STRUCTURES` and a field to `BuildModes`;
   - adds a match arm in `BuildChoices::resolved` and one in
     `DsFlag::structures` (`DsFlag::Build(IndexStructure)`);
   - adds a `build` slot to its `Execution` cell;
   - adds a `Prerequisite` row if a mode is valid only under another axis's
     value (see [Dependencies](#dependencies-between-optimisations)).
7. **Test it.** `kermit_ds::define_build_mode_provider!` plus
   `define_multiway_join_test_suite_for_build_mode!` per non-default mode and
   optimiser, `BuiltWith` aliases in `kermit-ds/tests/`, a CLI smoke test, and
   a spy test that the mode reaches the build (no output shows it).
8. **kermit-lab.** Add a `SCOPED_AXIS_DEFAULTS` entry if pre-axis rows came
   from a known mode. The `ablation` preset already limits `ds_build_mode` to
   `insertion` / `end_to_end` and drops rows of structures without the axis.

---

## Ablation studies with `kermit-lab`

The whole point of the prefix convention is that ablation analysis becomes a one-liner in pandas. Suppose you have 20 bench-runs JSON files in `bench-runs/`, varying the load-factor cap and the pruning Layout across the triangle query:

```python
import kermit_lab as kl

df = kl.load("bench-runs/")

# Pivot on a single optimization axis
df[df["query"] == "triangle"] \
    .groupby("ds_config_load_factor")["mean_ns"] \
    .agg(["mean", "std"])
```

Output (illustrative):
```
ds_config_load_factor
0.5    mean: 1.31e6   std: 7.4e4
0.7    mean: 1.42e6   std: 8.1e4
0.9    mean: 1.67e6   std: 9.6e4
```

The effect of a tuning value, visible in two lines of pandas. The paper's Table 6 ablation is reproducible at this scale.

For multi-dimensional ablation:

```python
df.groupby(["ds_layout_hasher", "ds_layout_pruning"])["mean_ns"].mean().unstack()
```

Gives a 2×2 matrix: hasher choice on one axis, the pruning Layout on the other. Swap either for `ds_config_load_factor` to cross a Layout with the Config.

---

## Pitfalls and FAQ

### Q: Why categorize? Why not just have "an optimization"?

Because the categories map to **different costs and constraints**:

- Layout has zero runtime cost but multiplies binary size and can't be runtime-switched.
- Config has runtime branch cost but trivial binary impact and dynamic toggling.
- BuildMode has no query-time cost but only affects construction.

Treating them as one would mean picking the worst tradeoff each time. The categories let you pick the right tool.

### Q: My optimization is Config but I want compile-time guarantees about which values are set.

Lift it to a Layout type parameter — a marker per value, as `PruningPolicy` does. The cost: combinatorial monomorphization and one more arm in `with_hash_trie_layout!`. The benefit: typecheck-time enforcement, and no non-user tax.

**Check the classification first.** If the knob changes a *shape* — which variants exist, which fields are carried — it was never a Config, and lifting it is the fix rather than a luxury; that is exactly what happened to singleton pruning. If it really is a value on an existing path, a Config stays the cheaper answer: a value substituted for a constant costs nothing at runtime, so the only thing a Layout buys you there is the type system.

### Q: What if my optimization spans categories?

Example: a hash function choice (Layout) plus a tuning parameter (Config), like "use FxHash with seed N." Decompose along the shape/value line: the hash function family is a shape, so Layout (`FxHashStrategy`); the seed is a value read where a constant sat, so Config (hypothetically, `HashTrieConfig::fx_seed: Option<u64>` — no seed axis exists today). Both axes are emitted; `kermit-lab` can pivot on either or both.

### Q: My optimization only works when another one is on.

Declare the prerequisite (see [Dependencies](#dependencies-between-optimisations)). If instead it *works differently* when the other one is on, it is two optimizations: give the dependent process its own value, as `presized:N` is to `parallel:N`.

### Q: What about algorithm optimizations?

The `algo_layout_*`, `algo_config_*`, `algo_build_mode` prefixes are reserved for algorithm-side optimizations. No current algorithm uses them. When `HashTriejoin` gains an "eager collect vs lazy iterate" toggle, that becomes a Config under `algo_config_*`. Same trait family, different prefix.

### Q: How do I sweep all optimization combinations?

Don't do it as a single CLI invocation — shell-loop instead:

```bash
for hasher in sip fxhash; do
    for pruning in off on; do
        for lf in 0.5 0.7 0.9; do
            kermit bench \
                --name triangle-$hasher-$pruning-lf$lf \
                --report-json bench-runs/triangle-$hasher-$pruning-lf$lf.json \
                run triangle -i hash-trie -a hash-triejoin \
                --ds-layout-hasher $hasher \
                --ds-layout-pruning $pruning \
                --ds-config load-factor=$lf
        done
    done
done
```

`--name` and `--report-json` are `bench`-level flags, so they go before `run`.
Give every run its own `--name`: Criterion group names do not encode
optimization axes, so runs that differ only in a `--ds-*` flag would overwrite
each other's samples under `target/criterion/`. Then load all of the reports in
`kermit-lab` and pivot.

### Q: Does adding a new axis break existing bench reports?

No. `BenchReport.axes` is an open map; downstream tooling treats missing keys as defaults. The `schema_version` does not change — adding new keys is non-breaking.

For old reports that predate the standard, back-fill defaults in `kermit-lab`, on the rows of the structure that has the axis:

```python
on_hash_trie = df["data_structure"] == "HashTrie"
df.loc[on_hash_trie, "ds_layout_hasher"] = df.loc[on_hash_trie, "ds_layout_hasher"].fillna("sip")
```

This is semantically correct — pre-standard HashTrie runs were SipHash-only. Scope matters: TreeTrie and ColumnTrie have no hasher, and a fill over every row labels them `sip`, so an ablation charts them as a `sip` bar beside HashTrie's (#85). `kermit_lab.defaults` keeps every default in one structure-scoped registry, `SCOPED_AXIS_DEFAULTS`, keyed by `(axis, data_structure)`. On HashTrie rows it back-fills `ds_layout_hasher = "sip"`, `ds_layout_pruning = "off"`, `ds_config_load_factor = 0.7` (the historical constant), `ds_config_child_capacity = "grow"` and `ds_build_mode = "incremental"` (the per-tuple build, `serial` until #107); a HashTrie `ds_build_mode` of `serial` reads as `incremental` (`RENAMED_AXIS_VALUES`). On ColumnTrie rows it back-fills `ds_build_mode = "incremental"`, since ColumnTrie built tuple by tuple before issue #84, and `ds_layout_seek = "binary"`. TreeTrie gets no seek default: its seek was linear before issue #67, and a report cannot tell which side of #67 it came from. A new axis's default goes in the same registry, under the structure that has the axis.

---

## Where to look in the code

| What | Where |
|---|---|
| The four traits | [`kermit-iters/src/optimization.rs`](../../kermit-iters/src/optimization.rs) |
| First Layout consumer (hasher choice) | [`kermit-iters/src/hash_strategy.rs`](../../kermit-iters/src/hash_strategy.rs) |
| Second Layout consumer (pruning policy) | [`kermit-ds/src/ds/hash_trie/pruning.rs`](../../kermit-ds/src/ds/hash_trie/pruning.rs) |
| Third Layout consumer (seek strategy, both sorted tries) | [`kermit-ds/src/seek.rs`](../../kermit-ds/src/seek.rs) |
| HashTrie's `HasOptimizationAxes` impl | [`kermit-ds/src/ds/hash_trie/implementation.rs`](../../kermit-ds/src/ds/hash_trie/implementation.rs) |
| CLI dispatch monomorphizing on the Layout cell | [`kermit/src/options.rs`](../../kermit/src/options.rs) (`with_hash_trie_layout!` — the one place Layout dimensions multiply) |
| Sorted Layout dispatch | [`kermit/src/options.rs`](../../kermit/src/options.rs) (`with_sorted_trie_layout!`) |
| The prerequisite table | [`kermit/src/options.rs`](../../kermit/src/options.rs) (`Prerequisite`, checked in `DsChoices::resolve`) |
| Bench-report axes merge | [`kermit/src/bench/run.rs`](../../kermit/src/bench/run.rs) and [`kermit/src/bench/ds.rs`](../../kermit/src/bench/ds.rs) (search `optimization_axes` / `build_mode_axes`), from the families in [`kermit/src/execution.rs`](../../kermit/src/execution.rs) |
| CLI smoke tests | [`kermit/tests/cli_hash_trie_hasher_choice.rs`](../../kermit/tests/cli_hash_trie_hasher_choice.rs), [`kermit/tests/cli_hash_trie_layout_pruning.rs`](../../kermit/tests/cli_hash_trie_layout_pruning.rs), [`kermit/tests/cli_hash_trie_layout_expansion.rs`](../../kermit/tests/cli_hash_trie_layout_expansion.rs), [`kermit/tests/cli_hash_trie_config_choice.rs`](../../kermit/tests/cli_hash_trie_config_choice.rs), [`kermit/tests/cli_sorted_trie_layout_seek.rs`](../../kermit/tests/cli_sorted_trie_layout_seek.rs) |
| First Config consumer (load-factor cap) | [`kermit-ds/src/ds/hash_trie/config.rs`](../../kermit-ds/src/ds/hash_trie/config.rs) |
| Second Config consumer (root capacity) | [`kermit-ds/src/ds/hash_trie/config.rs`](../../kermit-ds/src/ds/hash_trie/config.rs) (`RootCapacity`), [`hash_table.rs`](../../kermit-ds/src/ds/hash_trie/hash_table.rs) (`log2_capacity_for`) |
| Third Config consumer (child capacity) | [`kermit-ds/src/ds/hash_trie/config.rs`](../../kermit-ds/src/ds/hash_trie/config.rs) (`ChildCapacity`, `child_log2_capacity`), [`bulk.rs`](../../kermit-ds/src/ds/hash_trie/bulk.rs) (`build_child_table`) |
| The classification rule and why pruning moved | [`docs/specs/2026-09-08-singleton-pruning-config-design.md`](2026-09-08-singleton-pruning-config-design.md) § Amendment 1 |
| Why BuildMode equivalence is contents and capacities, not array identity | [`docs/specs/2026-10-05-parallel-build-design.md`](2026-10-05-parallel-build-design.md) § Amendment 2 |
| Why an axis value means one behaviour, and prerequisites | [`2026-10-07-dependent-optimisations-design.md`](2026-10-07-dependent-optimisations-design.md) |
| Config-injection seam (`ConfigurableRelation`) | [`kermit-ds/src/relation.rs`](../../kermit-ds/src/relation.rs) |
| `Configured` / `ConfigProvider` / `define_config_provider!` | [`kermit-ds/src/configured.rs`](../../kermit-ds/src/configured.rs) |
| Config-aware construction in `bench run` | [`kermit/src/execution.rs`](../../kermit/src/execution.rs) — `ExecutionFamily::build_relation`, with `load` defaulting to read-then-`build_relation` |
| Config join test macro | [`kermit/tests/common/macros.rs`](../../kermit/tests/common/macros.rs) (search `with_config`) |
| First BuildMode consumer (ColumnTrie build) | [`kermit-ds/src/ds/column_trie/build_mode.rs`](../../kermit-ds/src/ds/column_trie/build_mode.rs) |
| Second BuildMode consumer (HashTrie radix build) | [`kermit-ds/src/ds/hash_trie/build_mode.rs`](../../kermit-ds/src/ds/hash_trie/build_mode.rs), [`radix.rs`](../../kermit-ds/src/ds/hash_trie/radix.rs) |
| BuildMode seam and test wrapper | [`kermit-ds/src/relation.rs`](../../kermit-ds/src/relation.rs) (`BuildModeRelation`), [`kermit-ds/src/built_with.rs`](../../kermit-ds/src/built_with.rs) |
| BuildMode join test macro | [`kermit/tests/common/macros.rs`](../../kermit/tests/common/macros.rs) (search `for_build_mode`) |
| Per-DS catalog | [`docs/data-structures/hash-trie.md`](../data-structures/hash-trie.md), [`tree-trie.md`](../data-structures/tree-trie.md) and [`column-trie.md`](../data-structures/column-trie.md) § Optimizations; [`seek-strategies.md`](../data-structures/seek-strategies.md) for the sorted tries' shared seek Layout |
| Schema axis prefixes | [`docs/specs/bench-report-schema.md`](bench-report-schema.md) § Standard axis prefixes |
| Contributor recipe | [`CLAUDE.md`](../../CLAUDE.md) § "Adding an optimization to a data structure or algorithm" |

---

## What's implemented today, what's available

Thirteen optimizations are implemented — four Layout dimensions, three Config
values and six BuildModes:

| Optimization | Category | Where | Paper § |
|---|---|---|---|
| Hasher choice (Sip vs Fx) | Layout | `ds_layout_hasher` | §3.3.1 |
| Singleton pruning (off/on) | Layout | `ds_layout_pruning` | §3.3.1, Fig 5 |
| Lazy child expansion (eager/lazy) | Layout | `ds_layout_expansion` | §3.3.1, Fig 6 |
| Seek strategy (linear / binary / galloping) | Layout | `ds_layout_seek` | (kermit-specific; LFTJ §3) |
| Load-factor cap | Config | `ds_config_load_factor` | (kermit-specific) |
| Root capacity (grow / tuples) | Config | `ds_config_root_capacity` | §3.2.2, Alg. 2 line 3 (root only; issue #88) |
| HashTrie child capacity (grow / tuples) | Config | `ds_config_child_capacity` | §3.2.2, Algorithm 2 line 3 (issue #107) |
| ColumnTrie build (bulk / incremental) | BuildMode | `ds_build_mode` | (kermit-specific, issue #84) |
| HashTrie Algorithm 2 build (bulk, the default; incremental is the per-tuple one) | BuildMode | `ds_build_mode` | §3.2.2 (issue #107) |
| HashTrie radix-partitioned build (bulk / incremental / radix:K) | BuildMode | `ds_build_mode` | §3.3.2 hash partitioning only; the scratch roots and the merge are kermit's (issue #91) |
| TreeTrie build (serial / parallel:N) | BuildMode | `ds_build_mode` | §3.3.2 (morsel-driven; issue #94) |
| HashTrie parallel build (bulk / parallel:N) | BuildMode | `ds_build_mode` | §3.3.2 morsel-driven partitioning; the scratch-root build and the merge are kermit's (exact; issue #94) |
| HashTrie presized build (presized:N; requires root-capacity=tuples) | BuildMode | `ds_build_mode` | §3.3.2 hash partitioning, §3.2.2 Algorithm 2; the regions, the tail and the run-wise children are kermit's (issues #94, #107, Amendment 3) |

`define_multiway_join_test_suite_for_build_mode!` landed with the first
BuildMode consumer, ColumnTrie's build, and covers every consumer.

Available to add (each a separate brainstorming → planning → implementation cycle):

| Optimization | Category | Effort | Paper § |
|---|---|---|---|
| Pointer tagging | Layout | Medium | §3.3.1, Fig 4 |
| Hash seed | Config | Small | (kermit-specific) |
| Algorithm: skip-levels short-circuit (shelved, #90) | Layout (algo) | Medium | (kermit-specific) |
| Algorithm: eager-collect vs lazy-iterate | Config (algo) | Medium | (kermit-specific) |

The framework is in place; the catalog grows as consumers land.
