# `BenchReport` JSON schema

**Current schema version:** `2`
**Source of truth:** `kermit/src/bench_report.rs`

## Top-level shape

Every `kermit bench` invocation writes a JSON **array** of one or more
`BenchReport` objects, regardless of subcommand. The default destination is
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
| `group`    | string                     | First path segment under `target/criterion/`. Matches `criterion::BenchmarkGroup` name. |
| `function` | string                     | Criterion `function_id` (e.g. `TreeTrie/iteration`). On disk, `/` is replaced with `_` — read each candidate subdir's `benchmark.json:directory_name` to resolve to the actual filesystem path. |
| `metric`   | `"time"` \| `"space"`      | Which Criterion measurement axis this function recorded. |

## Conventional `axes` keys

Each call site populates whichever subset is meaningful for that subcommand.
The keys below are the *committed* vocabulary — adding a new one is a minor
schema change (no version bump unless an existing key changes type or
semantics).

| Key              | Populated by             | JSON type        | Notes |
|------------------|--------------------------|------------------|-------|
| `data_structure` | `join`, `ds`, `run`      | string           | `"TreeTrie"`, `"ColumnTrie"`. Matches the `IndexStructure` `Debug` repr. |
| `algorithm`      | `join`, `run`            | string           | `"LeapfrogTriejoin"`. Matches the `JoinAlgorithm` `Debug` repr. |
| `query`          | `run`                    | string           | Query name from the YAML `queries:` list (e.g. `"triangle"`). |
| `benchmark`      | `run`                    | string           | YAML benchmark name (e.g. `"triangle"`, `"watdiv-stress-c1"`). |
| `relation_path`  | `ds`                     | string           | The single relation file passed to `bench ds`. Workspace-relative if invoked from the workspace root. |
| `relation_bytes` | `ds`                     | number (u64)     | On-disk size in bytes (raw, not formatted). Use `relation_size` from `metadata` for the human-readable form. |
| `tuples`         | `ds`, `run`              | number (usize)   | `ds`: tuples in the single relation. `run`: total summed across all of the benchmark's relations (workload input size). |
| `arity`          | `ds`                     | number (usize)   | Relation arity. |
| `relations`      | `join`                   | number (usize)   | Count of relation files passed to `bench join`. |

## Resolving a `CriterionGroupRef` to filesystem paths

```python
import json, pathlib

def resolve(group_ref, criterion_root="target/criterion"):
    group_dir = pathlib.Path(criterion_root) / group_ref["group"]
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
common cases but Criterion's own escaping rules apply for special characters,
so prefer reading the file.

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
