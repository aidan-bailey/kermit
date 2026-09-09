# `BenchReport` JSON schema

**Current schema version:** `2`
**Source of truth:** `kermit/src/bench_report.rs`

## Top-level shape

Every report-producing `bench` subcommand (`join`, `ds`, `run`) writes a JSON
**array** of one or more `BenchReport` objects; `list`/`fetch`/`clean`/`gen`
produce no report. The default destination is
`bench-runs/<kind>-<unix-millis>.json` (resolved relative to the invocation's
CWD; the directory is auto-created). Pass `--report-json <PATH>` to override.
The array shape is uniform across `bench join` (one report), `bench ds`
(one report), and `bench run` (one report per query). External tools should
always parse a list.

## `BenchReport`

```json
[
  {
    "schema_version": 2,
    "kind": "ds",
    "metadata": [
      { "label": "data structure", "value": "TreeTrie" },
      { "label": "relation",       "value": "kermit/tests/fixtures/edge.csv" },
      { "label": "relation size",  "value": "24 B" },
      { "label": "tuples",         "value": "4" },
      { "label": "arity",          "value": "2" }
    ],
    "axes": {
      "arity": 2,
      "data_structure": "TreeTrie",
      "relation_bytes": 24,
      "relation_path": "kermit/tests/fixtures/edge.csv",
      "tuples": 4
    },
    "criterion_groups": [
      { "group": "ds", "function": "TreeTrie/space", "metric": "space" }
    ]
  }
]
```

### Field catalogue

| Field              | Type                         | Description |
|--------------------|------------------------------|-------------|
| `schema_version`   | u32                          | Currently `2`. Consumers should refuse unknown majors. |
| `kind`             | `"join"` \| `"ds"` \| `"run"` | Which `bench` subcommand produced the report. |
| `metadata`         | Array of `{label, value}`    | Human-readable label/value pairs mirroring the stderr block. Both fields are strings (numerics get stringified for stderr alignment). |
| `axes`             | Object (string → JSON value) | Structured axis values for downstream tooling. Numeric axes are kept numeric; alphabetically ordered (`BTreeMap`) so JSON diffs are deterministic. |
| `criterion_groups` | Array of `CriterionGroupRef` | Pointers into `target/criterion/` artefacts written during this invocation. |

### `CriterionGroupRef`

| Field      | Type                       | Description |
|------------|----------------------------|-------------|
| `group`    | string                     | The `criterion::BenchmarkGroup` name verbatim (e.g. `run/oxford-uniform-s1/triangle/TreeTrie/LeapfrogTriejoin`). On disk, Criterion flattens any `/` to `_` so this group lives at `target/criterion/run_oxford-uniform-s1_triangle_TreeTrie_LeapfrogTriejoin/`. |
| `function` | string                     | Criterion `function_id` (e.g. `space/P` or `iteration`). On disk, `/` is again replaced with `_` — read each candidate subdir's `benchmark.json:directory_name` to resolve to the actual filesystem path. |
| `metric`   | `"time"` \| `"space"`      | Which Criterion measurement axis this function recorded. |

## Conventional `axes` keys

Each call site populates whichever subset is meaningful for that subcommand.
The keys below are the *committed* vocabulary — adding a new one is a minor
schema change (no version bump unless an existing key changes type or
semantics).

| Key              | Populated by             | JSON type        | Notes |
|------------------|--------------------------|------------------|-------|
| `data_structure` | `join`, `ds`, `run`      | string           | `"TreeTrie"`, `"ColumnTrie"`, `"HashTrie"`. The `IndexStructure::axis_value` string. |
| `algorithm`      | `join`, `run`            | string           | `"LeapfrogTriejoin"`, `"HashTriejoin"`. The `JoinAlgorithm::axis_value` string. |
| `optimiser`      | `join`, `run`            | string           | Query optimiser that planned the join's variable ordering. Values: `"lexicographic"` (default), `"cardinality"`. Emitted by `bench join` and `bench run` (not `bench ds`, which performs no join). |
| `query`          | `run`                    | string           | Query name from the YAML `queries:` list (e.g. `"triangle"`). |
| `benchmark`      | `run`                    | string           | YAML benchmark name (e.g. `"triangle"`, `"watdiv-stress-c1"`). |
| `relation_path`  | `ds`                     | string           | The single relation file passed to `bench ds`. Workspace-relative if invoked from the workspace root. |
| `relation_bytes` | `ds`                     | number (u64)     | On-disk size in bytes (raw, not formatted). Use `relation_size` from `metadata` for the human-readable form. |
| `tuples`         | `ds`, `run`              | number (usize)   | `ds`: tuples in the single relation. `run`: total summed across all of the benchmark's relations (workload input size). |
| `arity`          | `ds`                     | number (usize)   | Relation arity. |
| `relations`      | `join`                   | number (usize)   | Count of relation files passed to `bench join`. |
| `queries_per_build` | `ds`, `run`           | number (u32)     | K for the `end-to-end` metric (`T = build + K × query`), from `--queries-per-build` (default 1). Emitted **only** when `--metrics` includes `end-to-end`, so historical invocations' reports are byte-identical. K never appears in the Criterion function id — the id is the bare `end_to_end` token (prefixed `{ds}/` for `bench ds`). |

## Resolving a `CriterionGroupRef` to filesystem paths

```python
import json, pathlib

def resolve(group_ref, criterion_root="target/criterion"):
    # Criterion flattens slashes in the group name to underscores on disk.
    group_dir = pathlib.Path(criterion_root) / group_ref["group"].replace("/", "_")
    for candidate in group_dir.iterdir():
        if not candidate.is_dir():
            continue
        bench_json = candidate / "new" / "benchmark.json"
        if not bench_json.exists():
            continue
        with bench_json.open() as f:
            meta = json.load(f)
        if meta["function_id"] == group_ref["function"]:
            return candidate / "new"
    raise FileNotFoundError(group_ref)
```

The `directory_name` in each subdir's `benchmark.json` is the canonical
mapping; computing it locally (slash-to-underscore replacement) works for
common cases but Criterion's own escaping rules apply for other special
characters, so prefer reading the file when in doubt.

## Null-valued estimates

Criterion writes `"slope": null` for measurements where it can't fit a
linear model — most often deterministic / zero-variance space measurements
captured via `SpaceMeasurement`. Consumers must accept `null` for `slope`
(and, by extension, `median_abs_dev` may report identical bounds with
`standard_error: 0.0`). The other estimates (`mean`, `median`, `std_dev`)
are always populated.

Inside the resolved `new/` directory:

- `estimates.json` — point estimate + 95% CIs for `mean`, `median`,
  `median_abs_dev`, `slope`, `std_dev`. Units match the measurement (ns for
  time, bytes for space).
- `sample.json` — `{sampling_mode, iters[], times[]}`. Per-batch raw data;
  `times[i]` is *total* over `iters[i]` iterations, so per-iter is
  `times[i] / iters[i]`. Sufficient for violin / box plots without needing
  `raw.csv`.
- `benchmark.json` — `{group_id, function_id, value_str, throughput,
  full_id, directory_name, title}`. Identifies the function.
- `tukey.json` — Tukey's fences (low / mild low / mild high / high) for
  outlier detection. Array of 4 floats.

A `base/` directory exists alongside `new/` after the second run — it holds
the previous run's data for Criterion's compare-against-baseline mode.
Plotting tools should read `new/`.

## Standard axis prefixes

Beyond the conventional keys (`data_structure`, `algorithm`, `query`, etc.),
the optimization standard ([`optimization-standard.md`](optimization-standard.md))
adds three prefixes for axes that capture which optimizations were active
during a benchmark run. These prefixes are **normative** — kermit-lab
tooling relies on them for cross-DS comparison.

- `ds_layout_<dim>` — compile-time layout choice on the data structure
  (e.g., `ds_layout_hasher`, `ds_layout_pruning`, `ds_layout_pointer_encoding`).
- `ds_config_<flag>` — runtime configuration value on the data structure
  (e.g., `ds_config_load_factor`).
- `ds_build_mode` — construction-time build mode for the data structure
  (single key; value is a `<mode>[:<params>]` string, e.g., `parallel:8`).
- `algo_layout_<dim>`, `algo_config_<flag>`, `algo_build_mode` — analogous
  prefixes for algorithm-level optimizations (reserved; not yet used).

When pivoting bench reports in kermit-lab, downstream code should:
- Treat missing keys as the algorithm/DS default. For pre-standard reports
  predating this change, back-fill `ds_layout_hasher == "sip"` (the
  historical hash function for HashTrie), `ds_layout_pruning == "off"`, and
  `ds_config_load_factor == 0.7` (the historical constant).
- Group on the relevant prefix to perform ablation analysis.

Adding new keys under these prefixes does not require a `schema_version`
bump — the `axes` field is an open map.

## Versioning policy

- **Bump `schema_version`** on any breaking change: renaming a field,
  changing a value type, removing a key from `axes` (if external tooling
  pinned to it), or restructuring nesting.
- **No bump** for additive changes: new `axes` keys, new optional fields on
  `CriterionGroupRef`, new conventional values for `kind` or `metric`.
- Consumers should refuse to parse if `schema_version` is missing or
  greater than the highest version they know about.

## Change log

| Version | Date       | Change |
|---------|------------|--------|
| 1       | 2026-04-19 | Initial schema (`schema_version`, `kind`, `metadata`, `criterion_groups`). |
| 2       | 2026-05-04 | Added structured `axes: BTreeMap<String, serde_json::Value>` for downstream tooling. `metadata` retained as the human-readable surface. |
| 2 (no bump) | 2026-07-08 | Added the `optimiser` conventional `axes` key (query optimiser that planned the join's variable ordering). Additive — `axes` is an open map, so `schema_version` stays `2`. |
| 2 (no bump) | 2026-07-23 | Added the `queries_per_build` conventional `axes` key and the `end_to_end` time-metric function id (`--metrics end-to-end`). Additive — the key only appears when the metric is requested, so `schema_version` stays `2`. |
