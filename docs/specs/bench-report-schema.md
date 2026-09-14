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
| `group`    | string                     | The `criterion::BenchmarkGroup` name verbatim (e.g. `run/oxford-uniform-s1/triangle/TreeTrie/LeapfrogTriejoin`). On disk Criterion 0.8.2 escapes `/` (and `?"\*<>:\|^`) to `_` **and truncates the name to 64 bytes**, so `run/watdiv-stress-100-test-1-prelim/q0000/ColumnTrie/LeapfrogTriejoin` lives at `target/criterion/run_watdiv-stress-100-test-1-prelim_q0000_ColumnTrie_LeapfrogTri/`. Don't compute it — resolve as below. `bench run` refuses a sweep in which two groups would truncate to the same directory. |
| `function` | string                     | Criterion `function_id` (e.g. `space/P` or `iteration`). On disk it is escaped the same way, and gets a `_2`, `_3`… suffix if its directory name was already used by the same `Criterion` instance. Resolve as below. |
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
| `query`          | `join`, `run`            | string           | Query name. `run`: from the YAML `queries:` list (e.g. `"triangle"`). `bench join`: the query file's stem. |
| `benchmark`      | `join`, `run`            | string           | Workload name. `run`: YAML benchmark name (e.g. `"triangle"`, `"watdiv-stress-c1"`). `bench join`: `"adhoc"`. |
| `relation_path`  | `ds`                     | string           | The single relation file passed to `bench ds`. Workspace-relative if invoked from the workspace root. |
| `relation_bytes` | `ds`                     | number (u64)     | On-disk size in bytes (raw, not formatted). Use `relation_size` from `metadata` for the human-readable form. |
| `tuples`         | `ds`, `join`, `run`      | number (usize)   | `ds`: tuples in the single relation. `join`, `run`: total summed across all of the benchmark's relations (workload input size). |
| `arity`          | `ds`                     | number (usize)   | Relation arity. |
| `relations`      | *(removed 2026-09-09)*   | —                | `bench join` used to emit its relation count; it now reports `tuples` like `bench run`. Old reports may still carry it. |
| `verified`       | `run`                    | boolean          | `true` when `--verify` checked the query's result count against the YAML's `expected`; absent otherwise. |
| `queries_per_build` | `ds`, `join`, `run`   | number (u32)     | K for the `end-to-end` metric (`T = build + K × query`), from `--queries-per-build` (default 1). Emitted **only** when `--metrics` includes `end-to-end`, so historical invocations' reports are byte-identical. K never appears in the Criterion function id — the id is the bare `end_to_end` token (prefixed `{ds}/` for `bench ds`). |

## Resolving a `CriterionGroupRef` to filesystem paths

Directory names are lossy (escaped, truncated to 64 bytes, possibly
suffixed), but every `new/benchmark.json` records the untruncated `group_id`
and `function_id`. Index those once and look the pair up:

```python
import json, pathlib

def index(criterion_root="target/criterion"):
    found = {}
    for bench_json in pathlib.Path(criterion_root).glob("*/*/new/benchmark.json"):
        meta = json.loads(bench_json.read_text())
        found.setdefault((meta["group_id"], meta["function_id"]), []).append(bench_json.parent)
    return found

def resolve(group_ref, found):
    dirs = found.get((group_ref["group"], group_ref["function"]), [])
    if len(dirs) != 1:  # 0: missing or overwritten; >1: a stale duplicate
        raise LookupError(group_ref, dirs)
    return dirs[0]
```

Match on **both** ids: two groups sharing a 64-byte prefix share a group
directory, and their functions (`iteration`, `iteration_2`) carry the same
`function_id`. `base/` holds the previous run's copy and is never a
candidate. `kermit_lab.criterion.CriterionIndex` implements this with
diagnostic errors.

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
| 2 (no bump) | 2026-09-09 | `bench join` now runs through the generic runner: it gained `benchmark` (`"adhoc"`), `query` (the query file's stem), and `tuples`, and dropped the redundant `relations` count. No key changed name or type, so `schema_version` stays `2`. |
| 2 (no bump) | 2026-09-09 | Added the `verified` conventional `axes` key, present only when `bench run --verify` checked the query. Additive, so `schema_version` stays `2`. |
