# Optimization Standard

**Status:** Normative.

## Overview

Kermit classifies all optimizations on data structures and algorithms into one of three structural categories. Each category has a prescribed Rust shape, CLI surface, bench-report axis namespace, and test obligation.

The standard exists so that:
- Adding an optimization follows a recognizable recipe.
- Bench reports across DSes and algorithms are pivot-friendly (consistent axis names).
- Future contributors know where new optimization code lives.

## The three categories

### Layout — compile-time, type-parameterized

A Layout option is a type parameter on the data structure or algorithm. Implementations are typically zero-sized marker types. Each layout combination is a distinct monomorphized type.

- **Rust shape:** `HashTrie<H: HashStrategy>` where `H` implements `LayoutOption`.
- **CLI surface:** `--ds-layout-<dim> <choice>` (e.g., `--ds-layout-hasher fxhash`).
- **Bench axis:** `ds_layout_<dim>` (e.g., `ds_layout_hasher: "fxhash"`).
- **Test obligation:** define a type alias per combination; invoke `define_multiway_join_test_suite!(<alias>, <Algo>)` for each.

### Config — runtime, branch-toggle flags

A Config option is a struct of boolean or enum flags read at hot-path call sites. No monomorphization; one type covers all configurations.

- **Rust shape:** `HashTrieConfig { singleton_pruning: bool, ... }` implementing `ConfigOption`.
- **CLI surface:** `--ds-config <flag>=<value>,<flag>=<value>` (e.g., `--ds-config singleton-pruning=true,lazy-expansion=false`).
- **Bench axis:** `ds_config_<flag>` per flag.
- **Test obligation:** baseline + at least one alternate per flag, via `define_multiway_join_test_suite_with_config!` (specified; macro to be implemented when first Config consumer lands).

### BuildMode — construction-time, output-equivalent

A BuildMode option affects how a structure is constructed but not its in-memory representation. Modes produce equivalent results on the same input.

- **Rust shape:** `BuildMode` trait on an enum; or distinct constructor methods (`from_tuples_parallel`).
- **CLI surface:** `--ds-build <mode>[:<params>]` (e.g., `--ds-build parallel:8`).
- **Bench axis:** `ds_build_mode: "<mode>:<params>"` (single key).
- **Test obligation:** each mode produces output equivalent to the default mode on the same input, via `define_multiway_join_test_suite_for_build_mode!` (specified; macro to be implemented when first BuildMode consumer lands).

## The trait family

Defined in `kermit-iters/src/optimization.rs`:

- `LayoutOption` — marker; provides `NAME` constant.
- `ConfigOption: Default + Clone` — provides `axes()` serialization.
- `BuildMode` — provides `axis_value()` serialization.
- `HasOptimizationAxes` — umbrella trait the bench reporter consumes; returns `BTreeMap<String, serde_json::Value>`.

See [`kermit-iters/src/optimization.rs`](../../kermit-iters/src/optimization.rs) for the full trait definitions with doc comments.

## The naming convention

Axes use a prefix per category:
- Data-structure axes: `ds_layout_*`, `ds_config_*`, `ds_build_mode`.
- Algorithm axes (reserved): `algo_layout_*`, `algo_config_*`, `algo_build_mode`.

Tooling pivots on these prefixes. The convention is normative — deviation breaks downstream analysis.

## Adding an optimization: the recipe

1. **Classify.** Decide which of the three categories the optimization belongs to.
2. **Implement.** Define the relevant Rust types (LayoutOption marker / ConfigOption struct / BuildMode enum). Add them in the DS or algorithm crate.
3. **Wire to the standard.** Update the DS or algorithm's `HasOptimizationAxes` impl to emit the new axis under the correct prefix.
4. **CLI.** Add the corresponding flag / parameter to `kermit/src/main.rs`.
5. **Bench-report.** No change needed — the `.extend()` of `optimization_axes()` picks up the new axis automatically.
6. **Tests.** Add the appropriate test-suite invocation (type alias for Layout; macro variant for Config/BuildMode).
7. **Document.** Add the optimization to the DS or algorithm's per-component doc under an "Optimizations" section.

## Current consumers

- `HashTrie` (Layout dimension: hasher choice) — see [`docs/data-structures/hash-trie.md`](../data-structures/hash-trie.md).
