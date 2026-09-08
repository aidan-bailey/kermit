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

### Layout — *changes the type*

A **Layout** option changes the type of the data structure itself. Each combination is a distinct compile-time type, with its own monomorphized code path.

> **Concrete example.** HashTrie's hash function (sip vs fxhash) is a Layout. `HashTrie<SipHashStrategy>` and `HashTrie<FxHashStrategy>` are different types — the compiler generates different machine code for each because the inner `H::hash(key)` call resolves to different functions.

| Aspect | Layout |
|---|---|
| Runtime cost | Zero (no branches; monomorphized) |
| Binary size cost | Linear in combinations |
| Type system enforcement | Strong (incompatible layouts won't compile together) |
| Switching at runtime | Impossible (it's compile-time) |
| Bench axis key | `ds_layout_<dim>` |
| Examples (potential) | Hasher choice ✓, pointer encoding, key type (`u32` vs `u64`) |
| Test obligation | Type alias per combination + `define_multiway_join_test_suite!(<alias>, <Algo>, <Optimiser>)` for each |

### Config — *changes a runtime flag*

A **Config** option is a boolean (or enum) field on a config struct that's read at hot-path call sites. One type covers all configurations; the cost is per-call branch evaluation.

> **Concrete example (implemented).** Singleton pruning. `HashTrie::insert_at` reads `config.singleton_pruning` and either allocates a child hash table (normal) or stores the single tuple in place as a `Singleton` node (pruned). The iterator then branches on the node variant it finds, not on the config struct.

| Aspect | Config |
|---|---|
| Runtime cost | One branch per check (microscopic) |
| Binary size cost | Constant |
| Type system enforcement | Weaker (any config is type-compatible with any other) |
| Switching at runtime | Yes (just change the flag) |
| Bench axis key | `ds_config_<flag>` |
| Examples (potential) | Singleton pruning ✓, lazy expansion, load-factor tuning |
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

Today (the hasher Layout and the singleton-pruning Config are implemented):

```bash
# Default — Sip, no pruning
kermit bench run triangle -i hash-trie -a hash-triejoin

# Pick FxHash
kermit bench run triangle -i hash-trie -a hash-triejoin --ds-layout-hasher fxhash

# Both implemented categories on one invocation
kermit bench run triangle -i hash-trie -a hash-triejoin \
    --ds-layout-hasher fxhash \
    --ds-config singleton-pruning=true
```

Hypothetically (when BuildMode gets a consumer):

```bash
kermit bench run triangle -i hash-trie -a hash-triejoin \
    --ds-layout-hasher fxhash \
    --ds-config singleton-pruning=true \
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
its Config axis, so `ds_config_singleton_pruning` appears even at its default):

```json
[
  {
    "schema_version": 2,
    "kind": "ds",
    "axes": {
      "data_structure": "HashTrie",
      "ds_config_singleton_pruning": false,
      "ds_layout_hasher": "fxhash",
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

The keys `ds_layout_hasher` and `ds_config_singleton_pruning` sit alongside the existing axes. The convention is:

| Prefix | Used for | Example |
|---|---|---|
| `ds_layout_<dim>` | DS layout (compile-time) | `ds_layout_hasher: "fxhash"` |
| `ds_config_<flag>` | DS config (runtime) | `ds_config_singleton_pruning: true` |
| `ds_build_mode` | DS build mode | `ds_build_mode: "parallel:8"` |
| `algo_layout_<dim>` | Algorithm layout (reserved) | (none yet) |
| `algo_config_<flag>` | Algorithm config (reserved) | (none yet) |
| `algo_build_mode` | Algorithm build mode (reserved) | (none yet) |

These prefixes are **normative** — downstream tools (`kermit-lab`) rely on them for pivot analysis. A DS that emits `singleton_pruning_enabled` instead of `ds_config_singleton_pruning` breaks the convention and trips up the tooling.

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

### `ConfigOption`

For a struct of runtime flags. The `axes()` method returns `(suffix, value)` pairs; the bench reporter prepends `ds_config_` (or `algo_config_`) to each suffix.

```rust
pub trait ConfigOption: Default + Clone {
    fn axes(&self) -> Vec<(&'static str, serde_json::Value)>;
}
```

Usage (`HashTrie`'s actual config, in [`kermit-ds/src/ds/hash_trie/config.rs`](../../kermit-ds/src/ds/hash_trie/config.rs)):
```rust
#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
pub struct HashTrieConfig {
    pub singleton_pruning: bool,
}

impl ConfigOption for HashTrieConfig {
    fn axes(&self) -> Vec<(&'static str, Value)> {
        vec![("singleton_pruning", Value::Bool(self.singleton_pruning))]
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
one Layout axis plus one `ds_config_*` axis per config flag):
```rust
impl<H: HashStrategy> HasOptimizationAxes for HashTrie<H> {
    fn optimization_axes(&self) -> BTreeMap<String, Value> {
        let mut axes = BTreeMap::new();
        axes.insert("ds_layout_hasher".to_string(), H::NAME.into());
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
with a Config axis implement it — today just `HashTrie<H>`, whose
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

## Walkthrough: adding your first optimization

Suppose you want to add singleton pruning as a Config flag on HashTrie. The recipe:

### 1. Classify

Singleton pruning is a runtime flag, read at build time by `HashTrie::insert_at`; the iterator then branches on the node variant it finds, not on the config struct. It doesn't change the type, and it needs no per-mode constructor. **Category: Config.**

### 2. Define the config struct

In `kermit-ds/src/ds/hash_trie/config.rs`:

```rust
use kermit_iters::ConfigOption;

#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
pub struct HashTrieConfig {
    pub singleton_pruning: bool,
}

impl ConfigOption for HashTrieConfig {
    fn axes(&self) -> Vec<(&'static str, serde_json::Value)> {
        vec![
            ("singleton_pruning", self.singleton_pruning.into()),
        ]
    }
}
```

### 3. Add a config field to HashTrie

```rust
pub struct HashTrie<H: HashStrategy = SipHashStrategy> {
    header: RelationHeader,
    root: HashTrieNode,
    config: HashTrieConfig,  // new
    _hasher: PhantomData<H>,
}
```

Implement `ConfigurableRelation` (`kermit-ds/src/relation.rs`) for it —
`with_config(header, config)` and `from_tuples_with_config(header, config, tuples)` —
and have `Relation::new` / `Relation::from_tuples` call them with
`HashTrieConfig::default()`, so existing behaviour is unchanged.

### 4. Wire the algorithm to read the flag

Inside `HashTrie::insert_at` (or wherever the pruning logic lives), branch on the config flag. The check is a runtime branch; LLVM will hoist or eliminate it depending on context.

### 5. Extend `HasOptimizationAxes`

```rust
impl<H: HashStrategy> HasOptimizationAxes for HashTrie<H> {
    fn optimization_axes(&self) -> BTreeMap<String, Value> {
        let mut axes = BTreeMap::new();
        axes.insert("ds_layout_hasher".to_string(), H::NAME.into());
        for (suffix, value) in self.config.axes() {
            axes.insert(format!("ds_config_{suffix}"), value);
        }
        axes
    }
}
```

### 6. Add CLI surface

In `kermit/src/main.rs`, `ConfigChoices` sits beside `LayoutChoices` as a
flattened clap group carrying `--ds-config`, a comma-separated key=value
string parsed into a `HashTrieConfig`. `validate_config_choices` (mirroring
`validate_layout_choices`) rejects the flag on an index structure with no
Config axis, so a report can never carry a `ds_config_*` axis the structure
ignored. Both groups are wired on `bench ds` and `bench run`.

### 7. Add tests

Use `define_multiway_join_test_suite_with_config!` (`kermit/tests/common/macros.rs`).
It declares the `Configured<Relation, Provider>` alias inside a module of its
own and runs the 11 standard join patterns on it:

```rust
use kermit_ds::{define_config_provider, HashTrieConfig};

define_config_provider!(PruningOn, HashTrieConfig, HashTrieConfig { singleton_pruning: true });
define_multiway_join_test_suite_with_config!(HashTrieSip, HashTriejoin, LexicographicOptimiser, PruningOn);
```

The existing pruning-off invocations stay as the baseline. The DS-level
suites run on `Configured<HashTrieSip, PruningOn>` aliases too —
`hash_trie_test_suite!` in `kermit-ds/tests/hash_trie_tests.rs` and
`parquet_test_suite!` in `kermit-ds/tests/parquet_tests.rs`.

### 8. Document

Update `docs/data-structures/hash-trie.md` § Optimizations § Config flags to list `singleton_pruning` with description and bench-axis value.

### 9. Verify the bench report

`--report-json` is a `bench`-level flag, so it precedes the subcommand:

```bash
kermit bench --report-json /tmp/sp.json ds -i hash-trie \
    --ds-config singleton-pruning=true \
    --relation kermit/tests/fixtures/edge.csv -m space
jq '.[0].axes' /tmp/sp.json
#   "ds_config_singleton_pruning": true,
```

---

## Ablation studies with `kermit-lab`

The whole point of the prefix convention is that ablation analysis becomes a one-liner in pandas. Suppose you have 20 bench-runs JSON files in `bench-runs/`, varying singleton pruning and lazy expansion across the triangle query:

```python
import kermit_lab as kl

df = kl.load("bench-runs/")

# Pivot on a single optimization axis
df[df["query"] == "triangle"] \
    .groupby("ds_config_singleton_pruning")["mean_ns"] \
    .agg(["mean", "std"])
```

Output (illustrative):
```
ds_config_singleton_pruning
False    mean: 1.42e6   std: 8.1e4
True     mean: 6.10e5   std: 4.2e4
```

A 2.3× speedup with singleton pruning enabled, visible in two lines of pandas. The paper's Table 6 ablation is reproducible at this scale.

For multi-dimensional ablation:

```python
df.groupby(["ds_layout_hasher", "ds_config_singleton_pruning"])["mean_ns"].mean().unstack()
```

Gives a 2×2 matrix: hasher choice on one axis, pruning toggle on the other.

---

## Pitfalls and FAQ

### Q: Why categorize? Why not just have "an optimization"?

Because the categories map to **different costs and constraints**:

- Layout has zero runtime cost but multiplies binary size and can't be runtime-switched.
- Config has runtime branch cost but trivial binary impact and dynamic toggling.
- BuildMode has no query-time cost but only affects construction.

Treating them as one would mean picking the worst tradeoff each time. The categories let you pick the right tool.

### Q: My optimization is Config but I want compile-time guarantees about which flags are set.

Use a Layout type parameter that includes the config: `HashTrie<H, C: HashTrieConfigKind>` where `C` is a marker like `WithSingletonPruning` or `NoSingletonPruning`. The cost: combinatorial monomorphization. The benefit: typecheck-time enforcement of compatible Config + Algorithm pairs.

Generally, **prefer Config until typecheck-time enforcement is genuinely needed**. The performance difference between a branch and a monomorphized call is microscopic on modern CPUs.

### Q: What if my optimization spans categories?

Example: a hash function choice (Layout) plus a tuning parameter (Config), like "use FxHash with seed N." Decompose: the hash function family is Layout (`FxHashStrategy`), the seed is Config (`HashTrieConfig::fx_seed: Option<u64>`). Both axes are emitted; `kermit-lab` can pivot on either or both.

### Q: What about algorithm optimizations?

The `algo_layout_*`, `algo_config_*`, `algo_build_mode` prefixes are reserved for algorithm-side optimizations. No current algorithm uses them. When `HashTriejoin` gains an "eager collect vs lazy iterate" toggle, that becomes a Config under `algo_config_*`. Same trait family, different prefix.

### Q: How do I sweep all optimization combinations?

Don't do it as a single CLI invocation — shell-loop instead:

```bash
for hasher in sip fxhash; do
    for sp in true false; do
        kermit bench run triangle -i hash-trie -a hash-triejoin \
            --ds-layout-hasher $hasher \
            --ds-config singleton-pruning=$sp \
            --report-json bench-runs/triangle-$hasher-sp$sp.json
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

This is semantically correct — pre-standard runs were SipHash-only.

---

## Where to look in the code

| What | Where |
|---|---|
| The four traits | [`kermit-iters/src/optimization.rs`](../../kermit-iters/src/optimization.rs) |
| First Layout consumer (hasher choice) | [`kermit-iters/src/hash_strategy.rs`](../../kermit-iters/src/hash_strategy.rs) |
| HashTrie's `HasOptimizationAxes` impl | [`kermit-ds/src/ds/hash_trie/implementation.rs`](../../kermit-ds/src/ds/hash_trie/implementation.rs) |
| CLI dispatch monomorphizing on `H` | [`kermit/src/main.rs`](../../kermit/src/main.rs) (search `run_*_hash<H`) |
| Bench-report axes merge | [`kermit/src/main.rs`](../../kermit/src/main.rs) (search `optimization_axes`) |
| CLI smoke tests | [`kermit/tests/cli_hash_trie_hasher_choice.rs`](../../kermit/tests/cli_hash_trie_hasher_choice.rs), [`kermit/tests/cli_hash_trie_config_choice.rs`](../../kermit/tests/cli_hash_trie_config_choice.rs) |
| First Config consumer (singleton pruning) | [`kermit-ds/src/ds/hash_trie/config.rs`](../../kermit-ds/src/ds/hash_trie/config.rs) |
| Config-injection seam (`ConfigurableRelation`) | [`kermit-ds/src/relation.rs`](../../kermit-ds/src/relation.rs) |
| `Configured` / `ConfigProvider` / `define_config_provider!` | [`kermit-ds/src/configured.rs`](../../kermit-ds/src/configured.rs) |
| Config-aware construction in `bench run` | [`kermit/src/execution.rs`](../../kermit/src/execution.rs) — `ExecutionFamily::build_relation`, with `load` defaulting to read-then-`build_relation` |
| Config join test macro | [`kermit/tests/common/macros.rs`](../../kermit/tests/common/macros.rs) (search `with_config`) |
| Per-DS catalog | [`docs/data-structures/hash-trie.md`](../data-structures/hash-trie.md) § Optimizations |
| Schema axis prefixes | [`docs/specs/bench-report-schema.md`](bench-report-schema.md) § Standard axis prefixes |
| Contributor recipe | [`CLAUDE.md`](../../CLAUDE.md) § "Adding an optimization to a data structure or algorithm" |

---

## What's implemented today, what's available

Two optimizations are implemented:

| Optimization | Category | Where | Paper § |
|---|---|---|---|
| Hasher choice (Sip vs Fx) | Layout | `ds_layout_hasher` | §3.3.1 |
| Singleton pruning | Config | `ds_config_singleton_pruning` | §3.3.1, Fig 5 |

The BuildMode category still has no consumer, so
`define_multiway_join_test_suite_for_build_mode!` lands with the first
BuildMode consumer.

Available to add (each a separate brainstorming → planning → implementation cycle):

| Optimization | Category | Effort | Paper § |
|---|---|---|---|
| Lazy child expansion | Config | Small | §3.3.1, Fig 6 |
| Pointer tagging | Layout | Medium | §3.3.1, Fig 4 |
| Radix partitioning | BuildMode | Medium | §3.3.2 |
| Parallel build | BuildMode | Large | §3.3.2 |
| Algorithm: eager-collect vs lazy-iterate | Config (algo) | Medium | (kermit-specific) |

The framework is in place; the catalog grows as consumers land.
