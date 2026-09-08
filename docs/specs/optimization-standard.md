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
cap ✓, an initial-capacity hint, a hash seed.

### Layout — *changes the type*

A **Layout** option changes the type of the data structure itself. Each combination is a distinct compile-time type, with its own monomorphized code path.

> **Concrete example.** HashTrie's hash function (sip vs fxhash) is a Layout. `HashTrie<SipHashStrategy>` and `HashTrie<FxHashStrategy>` are different types — the compiler generates different machine code for each because the inner `H::hash(key)` call resolves to different functions.
>
> **Second concrete example.** Singleton pruning is HashTrie's other Layout dimension: `HashTrie<H, NoPruning>` and `HashTrie<H, SingletonPruning>` differ in which node variants exist at all, because the policy's `Payload` associated type is uninhabited when pruning is off.

| Aspect | Layout |
|---|---|
| Runtime cost | Zero (no branches; monomorphized) |
| Binary size cost | Linear in combinations |
| Type system enforcement | Strong (incompatible layouts won't compile together) |
| Switching at runtime | Impossible (it's compile-time) |
| Bench axis key | `ds_layout_<dim>` |
| Examples (potential) | Hasher choice ✓, singleton pruning ✓, pointer encoding, lazy expansion |
| Test obligation | Type alias per combination + `define_multiway_join_test_suite!(<alias>, <Algo>, <Optimiser>)` for each |

### Config — *changes a runtime value*

A **Config** option is a *value* field on a config struct, read on a code path that already exists. One type covers all configurations; substituting the field for a constant adds no branch, so a run at the default pays what it paid before the axis existed.

> **Concrete example (implemented).** The load-factor cap. `HashTable::entry_or_insert_with` already tests `(len + 1) * DEN > capacity * NUM` on every insert to decide whether to double; `NUM`/`DEN` come from `HashTrieConfig::load_factor` instead of two `const`s. `HashTrie` holds the config and passes the cap down through `insert_at`, so nothing is stored per table and space is unchanged.

| Aspect | Config |
|---|---|
| Runtime cost | None beyond the comparison the code already performed |
| Binary size cost | Constant |
| Type system enforcement | Weaker (any config is type-compatible with any other) |
| Switching at runtime | Yes (just change the value) |
| Bench axis key | `ds_config_<flag>` |
| Examples (potential) | Load-factor cap ✓, initial capacity, hash seed |
| Test obligation | Baseline + ≥1 alternate per flag via `define_multiway_join_test_suite_with_config!` ([`kermit/tests/common/macros.rs`](../../kermit/tests/common/macros.rs)) |

### BuildMode — *changes how the structure is built*

A **BuildMode** changes the construction process but leaves the resulting in-memory representation unchanged. Modes produce *equivalent results* on the same input — they're a perf knob, not a semantic change.

> **Concrete example (hypothetical).** Parallel build. `HashTrie::from_tuples_parallel(tuples, threads=8)` and `HashTrie::from_tuples(tuples)` both yield the same trie on the same input, just one uses morsel-driven concurrent insertion.

| Aspect | BuildMode |
|---|---|
| Runtime cost | None at query time (build-only) |
| Binary size cost | One constructor variant per mode |
| Type system enforcement | Weak (mode is just a parameter) |
| Output equivalence | Required (must produce same trie as default) |
| Bench axis key | `ds_build_mode` (single key with mode + params) |
| Examples (potential) | Parallel build, radix partitioning, presorted input |
| Test obligation | Each mode output-equivalent to the default via `define_multiway_join_test_suite_for_build_mode!` (macro lands with the first BuildMode consumer) |

---

## What this looks like at the CLI

Today (two Layout dimensions — hasher and pruning — plus the load-factor
Config are implemented):

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
```

Hypothetically (when BuildMode gets a consumer):

```bash
kermit bench run triangle -i hash-trie -a hash-triejoin \
    --ds-layout-hasher fxhash \
    --ds-layout-pruning on \
    --ds-config load-factor=0.5 \
    --ds-build parallel:8
```

Each category has its own flag namespace — `--ds-layout-<dim>` for Layout, `--ds-config <flag>=<value>,...` for Config (a single flag with comma-separated key=value pairs), `--ds-build <mode>[:<params>]` for BuildMode.

When a chosen DS doesn't have a given Layout dimension, the CLI rejects the flag at parse time:

```
$ kermit bench ds -i tree-trie --ds-layout-hasher fxhash
Error: --ds-layout-hasher is only valid with --indexstructure hash-trie (or all); got --indexstructure TreeTrie
```

---

## What the bench report looks like

The optimization choices flow into the existing `axes` field of `BenchReport`. Here's what you see today for a HashTrie + FxHash run (HashTrie always emits
both Layout axes and its Config axis, so `ds_layout_pruning` and
`ds_config_load_factor` appear even at their defaults):

```json
[
  {
    "schema_version": 2,
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
| `ds_build_mode` | DS build mode | `ds_build_mode: "parallel:8"` |
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

Usage (hypothetical):
```rust
enum HashTrieBuildMode {
    Serial,
    Parallel { threads: usize },
    Radix    { bits: u32 },
}

impl BuildMode for HashTrieBuildMode {
    fn axis_value(&self) -> String {
        match self {
            Self::Serial               => "serial".to_string(),
            Self::Parallel { threads } => format!("parallel:{threads}"),
            Self::Radix    { bits }    => format!("radix:{bits}"),
        }
    }
}
```

### `HasOptimizationAxes`

The umbrella trait the bench reporter consumes. A DS or algorithm implements it; the impl composes results from the other three trait methods under the correct key prefixes.

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
never carry a `ds_config_*` axis the structure ignored. Both groups are wired
on `bench ds` and `bench run`.

### 7. Add tests

Use `define_multiway_join_test_suite_with_config!` (`kermit/tests/common/macros.rs`).
It declares the `Configured<Relation, Provider>` alias inside a module of its
own and runs the 11 standard join patterns on it:

```rust
use kermit_ds::{define_config_provider, HashTrieConfig, LoadFactor};

define_config_provider!(HalfFull, HashTrieConfig, HashTrieConfig {
    load_factor: LoadFactor::percent(50).unwrap(),
});
define_multiway_join_test_suite_with_config!(HashTrieSip, HashTriejoin, LexicographicOptimiser, HalfFull);
```

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
`ValueEnum`, default `off`), listed in `validate_layout_choices` so it is
rejected on non-`hash-trie` selectors. Dispatch does **not** repeat the
product: `with_hash_trie_layout!(hasher, pruning, |H, P| …)` in
`kermit/src/options.rs` expands the hasher × pruning cells once, and both
`dispatch_run_bench` and `dispatch_ds_bench` call it. **Adding a third
Layout dimension means adding arms to that macro and nowhere else** — do
not reach for a runtime enum inside the structure, which would reintroduce
the tax the Layout category exists to avoid.

`Execution::HashHtj { hasher, pruning, config }`
(`kermit/src/execution.rs`) records the cell, so the bench report cannot
name a Layout it did not run.

### 4. The test obligation: one alias per combination × the whole suite

Each Layout combination is a distinct type alias, and each alias runs the
full 11-pattern suite under every compatible algorithm and optimiser
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

### Q: What about algorithm optimizations?

The `algo_layout_*`, `algo_config_*`, `algo_build_mode` prefixes are reserved for algorithm-side optimizations. No current algorithm uses them. When `HashTriejoin` gains an "eager collect vs lazy iterate" toggle, that becomes a Config under `algo_config_*`. Same trait family, different prefix.

### Q: How do I sweep all optimization combinations?

Don't do it as a single CLI invocation — shell-loop instead:

```bash
for hasher in sip fxhash; do
    for pruning in off on; do
        for lf in 0.5 0.7 0.9; do
            kermit bench run triangle -i hash-trie -a hash-triejoin \
                --ds-layout-hasher $hasher \
                --ds-layout-pruning $pruning \
                --ds-config load-factor=$lf \
                --report-json bench-runs/triangle-$hasher-$pruning-lf$lf.json
        done
    done
done
```

Then load all of them in `kermit-lab` and pivot.

### Q: Does adding a new axis break existing bench reports?

No. `BenchReport.axes` is an open map; downstream tooling treats missing keys as defaults. The `schema_version` stays at `2` — adding new keys is non-breaking.

For old reports that predate the standard, back-fill defaults in `kermit-lab`:

```python
df["ds_layout_hasher"] = df.get("ds_layout_hasher", "sip")
```

This is semantically correct — pre-standard runs were SipHash-only. `kermit_lab.defaults` does exactly this for every axis, back-filling `ds_layout_pruning = "off"` and `ds_config_load_factor = 0.7` (the historical constant) as well.

---

## Where to look in the code

| What | Where |
|---|---|
| The four traits | [`kermit-iters/src/optimization.rs`](../../kermit-iters/src/optimization.rs) |
| First Layout consumer (hasher choice) | [`kermit-iters/src/hash_strategy.rs`](../../kermit-iters/src/hash_strategy.rs) |
| Second Layout consumer (pruning policy) | [`kermit-ds/src/ds/hash_trie/pruning.rs`](../../kermit-ds/src/ds/hash_trie/pruning.rs) |
| HashTrie's `HasOptimizationAxes` impl | [`kermit-ds/src/ds/hash_trie/implementation.rs`](../../kermit-ds/src/ds/hash_trie/implementation.rs) |
| CLI dispatch monomorphizing on the Layout cell | [`kermit/src/options.rs`](../../kermit/src/options.rs) (`with_hash_trie_layout!` — the one place Layout dimensions multiply) |
| Bench-report axes merge | [`kermit/src/main.rs`](../../kermit/src/main.rs) (search `optimization_axes`) |
| CLI smoke tests | [`kermit/tests/cli_hash_trie_hasher_choice.rs`](../../kermit/tests/cli_hash_trie_hasher_choice.rs), [`kermit/tests/cli_hash_trie_layout_pruning.rs`](../../kermit/tests/cli_hash_trie_layout_pruning.rs), [`kermit/tests/cli_hash_trie_config_choice.rs`](../../kermit/tests/cli_hash_trie_config_choice.rs) |
| First Config consumer (load-factor cap) | [`kermit-ds/src/ds/hash_trie/config.rs`](../../kermit-ds/src/ds/hash_trie/config.rs) |
| The classification rule and why pruning moved | [`docs/specs/2026-09-08-singleton-pruning-config-design.md`](2026-09-08-singleton-pruning-config-design.md) § Amendment 1 |
| Config-injection seam (`ConfigurableRelation`) | [`kermit-ds/src/relation.rs`](../../kermit-ds/src/relation.rs) |
| `Configured` / `ConfigProvider` / `define_config_provider!` | [`kermit-ds/src/configured.rs`](../../kermit-ds/src/configured.rs) |
| Config-aware construction in `bench run` | [`kermit/src/execution.rs`](../../kermit/src/execution.rs) — `ExecutionFamily::build_relation`, with `load` defaulting to read-then-`build_relation` |
| Config join test macro | [`kermit/tests/common/macros.rs`](../../kermit/tests/common/macros.rs) (search `with_config`) |
| Per-DS catalog | [`docs/data-structures/hash-trie.md`](../data-structures/hash-trie.md) § Optimizations |
| Schema axis prefixes | [`docs/specs/bench-report-schema.md`](bench-report-schema.md) § Standard axis prefixes |
| Contributor recipe | [`CLAUDE.md`](../../CLAUDE.md) § "Adding an optimization to a data structure or algorithm" |

---

## What's implemented today, what's available

Three optimizations are implemented — two Layout dimensions and one Config
value:

| Optimization | Category | Where | Paper § |
|---|---|---|---|
| Hasher choice (Sip vs Fx) | Layout | `ds_layout_hasher` | §3.3.1 |
| Singleton pruning (off/on) | Layout | `ds_layout_pruning` | §3.3.1, Fig 5 |
| Load-factor cap | Config | `ds_config_load_factor` | (kermit-specific) |

The BuildMode category still has no consumer, so
`define_multiway_join_test_suite_for_build_mode!` lands with the first
BuildMode consumer.

Available to add (each a separate brainstorming → planning → implementation cycle):

| Optimization | Category | Effort | Paper § |
|---|---|---|---|
| Lazy child expansion | Layout (an unexpanded node is a node state) | Medium | §3.3.1, Fig 6 |
| Pointer tagging | Layout | Medium | §3.3.1, Fig 4 |
| Initial capacity hint | Config | Small | (kermit-specific) |
| Hash seed | Config | Small | (kermit-specific) |
| Radix partitioning | BuildMode | Medium | §3.3.2 |
| Parallel build | BuildMode | Large | §3.3.2 |
| Algorithm: skip-levels short-circuit | Config (algo) | Medium | §3.3.1 |
| Algorithm: eager-collect vs lazy-iterate | Config (algo) | Medium | (kermit-specific) |

The framework is in place; the catalog grows as consumers land.
