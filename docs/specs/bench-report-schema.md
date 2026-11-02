# `BenchReport` JSON schema

**Current schema version:** `6`
**Source of truth:** `kermit/src/bench_report.rs`

## Top-level shape

Every report-producing `bench` subcommand (`join`, `ds`, `run`, `probe`) writes a JSON
**array** of one or more `BenchReport` objects; `list`/`fetch`/`clean`/`gen`
produce no report. The default destination is
`bench-runs/<kind>-<unix-millis>.json` (resolved relative to the invocation's
CWD; the directory is auto-created). Pass `--report-json <PATH>` to override.
The array shape is uniform across `bench join` (one report), `bench ds`
(one report), and `bench run` and `bench probe` (one report per query and
cell). External tools should
always parse a list.

## `BenchReport`

```json
[
  {
    "schema_version": 6,
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
    "criterion_groups": [],
    "values": [
      { "function": "TreeTrie/space", "metric": "space", "estimator": "exact", "value": 96 }
    ]
  }
]
```

### Field catalogue

| Field              | Type                         | Description |
|--------------------|------------------------------|-------------|
| `schema_version`   | u32                          | Currently `6`. Consumers should refuse unknown majors. |
| `kind`             | `"join"` \| `"ds"` \| `"run"` \| `"probe"` | Which `bench` subcommand produced the report. |
| `metadata`         | Array of `{label, value}`    | Human-readable label/value pairs mirroring the stderr block. Both fields are strings (numerics get stringified for stderr alignment). |
| `axes`             | Object (string → JSON value) | Structured axis values for downstream tooling. Numeric axes are kept numeric; alphabetically ordered (`BTreeMap`) so JSON diffs are deterministic. |
| `criterion_groups` | Array of `CriterionGroupRef` | Pointers into `target/criterion/` artefacts written during this invocation. |
| `values`           | Array of `ValueEntry`        | Measurements made without Criterion (v6): the exact `space`, and a probe's single-run times. Always present from v6, possibly empty. |

### `CriterionGroupRef`

| Field      | Type                       | Description |
|------------|----------------------------|-------------|
| `group`    | string                     | The `criterion::BenchmarkGroup` name verbatim (e.g. `run/oxford-uniform-s1/triangle/TreeTrie/LeapfrogTriejoin`). On disk Criterion 0.8.2 escapes `/` (and `?"\*<>:\|^`) to `_` **and truncates the name to 64 bytes**, so `run/watdiv-stress-100-test-1-prelim/q0000/ColumnTrie/LeapfrogTriejoin` lives at `target/criterion/run_watdiv-stress-100-test-1-prelim_q0000_ColumnTrie_LeapfrogTri/`. Don't compute it — resolve as below. `bench run` refuses a sweep in which two groups would truncate to the same directory. |
| `function` | string                     | Criterion `function_id` (e.g. `TreeTrie/space` or `iteration`). On disk it is escaped the same way, and gets a `_2`, `_3`… suffix if its directory name was already used by the same `Criterion` instance. Resolve as below. |
| `metric`   | `"time"` \| `"space"`      | Which Criterion measurement axis this function recorded. |

### `ValueEntry` (v6)

| Field       | Type                     | Description |
|-------------|--------------------------|-------------|
| `function`  | string                   | `space` (`bench run`, `bench join`, `bench probe`), `{ds}/space` (`bench ds`), or `build` / `iteration` (a probe's single runs). |
| `metric`    | `"time"` \| `"space"`    | What the value measures. |
| `estimator` | `"exact"` \| `"single"`  | `exact`: a deterministic quantity (heap bytes, as built). `single`: one run's wall time, timed with an `Instant` around one describe build or one execution. A single run is cold; compare it with other single runs, never with a Criterion estimate. |
| `value`     | number (u64)             | ns for time, bytes for space. |

### Time and space function ids

From v6, `bench run` and `bench join` write, per query, the Criterion time
functions `iteration`, `build` and `end_to_end` (opt-in), in that order:
`iteration` runs on the query's describe build, which is then dropped before
`build` and `end_to_end` are timed. `space`, the heap bytes of the query's
indexes together, read off the describe build before anything runs on it, is
an exact `values` entry, never a Criterion function. `bench ds` writes
`{ds}/iteration`, `{ds}/build` and `{ds}/end_to_end` (opt-in), and the exact
value `{ds}/space`. `bench probe` writes no Criterion function: per query and
cell, the exact `space` and the single-run values `build` and `iteration`.
Each index gets an `index` metadata line, `{id} {relation} filter {filter}
columns {columns:?}: {rows} rows, {bytes} bytes`, where the filter reads `-`
for none and otherwise `col{c}={value}` for a constant and `col{a}=col{b}` for
an equality, comma-separated (for example `#0 edge filter - columns [1, 0]: 4
rows, 96 bytes`, illustrative). A HashTrie index under `--ds-layout-levels
keys` whose trie has levels for fewer columns than it keeps ends its columns
with ` levels {n}` (`#0 r filter - columns [0, 1] levels 1: 4 rows, 272
bytes`, #116); every other line reads as before.

In v5 the time functions ran as `build`, `iteration`, `end_to_end`, and
`space` was a Criterion function (`space`, `{ds}/space`) measured through
`SpaceMeasurement`.

Up to v4, the time functions were `insertion` (every base relation built
from its file-order tuples), `iteration` (the streamed join over the
prebuilt engine), `end_to_end` and, under `--column-orders any` when the
plan needed a copy, `copies` (every reordered copy the query needed,
permuted and built through the same `build_relation` as `insertion`;
emitted beside `insertion`, so only when `insertion` was measured). The
space functions were `space/<relation>` per base relation and
`space/Index_<π>_<base>` per copy (e.g. `space/Index_1_0_edge`), and each
copy also got an `index` metadata line, e.g. `edge (1, 0)`. Under `stored`
no `copies` or `space/Index_*` function was ever written.

## Conventional `axes` keys

Each call site populates whichever subset is meaningful for that subcommand.
The keys below are the *committed* vocabulary — adding a new one is a minor
schema change (no version bump unless an existing key changes type or
semantics).

| Key              | Populated by             | JSON type        | Notes |
|------------------|--------------------------|------------------|-------|
| `data_structure` | `join`, `ds`, `run`      | string           | `"TreeTrie"`, `"ColumnTrie"`, `"HashTrie"`. The `IndexStructure::axis_value` string. |
| `algorithm`      | `join`, `run`            | string           | `"LeapfrogTriejoin"`, `"HashTriejoin"`. The `JoinAlgorithm::axis_value` string. |
| `optimiser`      | `join`, `run`            | string           | Query optimiser that planned the join's variable ordering. Values: `"lexicographic"` (default), `"cardinality"`, `"cost-based"`. A `"cost-based"` run's per-column statistics walk runs when the catalog is read, outside every timer. Emitted by `bench join` and `bench run` (not `bench ds`, which performs no join). |
| `column_orders`  | `join`, `run`            | string           | *Removed in v5* (`--column-orders` no longer exists; the planner is always free). Schema 3 and 4 reports carry the column-order policy the join was planned under: `"stored"` (each relation read in its stored column order) or `"any"` (an atom whose plan disagreed with its stored order read a per-query reordered copy). kermit-lab back-fills `"stored"` only on join rows (rows with an `algorithm`) whose `schema_version` is below 5; a v5 row has no such axis, and labelling it `"stored"` would be false. |
| `index_rows`     | `join`, `run`            | number (usize)   | v5: the rows the query's build copied, `Indexes::rows()`, the sum of its index views' counts (a filtered index counts only the rows that match). Lets build time be normalised per row. Absent from `ds` reports and from reports before v5. |
| `plan`           | `join`, `run`, `probe`   | string           | v6: the plan's variable order by name, joined by `", "` (`Prepared::variable_order`; a `_` appears under its `K<n>` name). For one catalog and query it identifies the whole plan, so two reports of one (benchmark, query) with the same `plan` ran the same build and join. |
| `answer_rows`    | `run`, `probe`           | number (u64)     | v6: the answer's row count from the describe step's count; present whenever it counted (`bench run --verify` on a query with `expected`, and every probe query). |
| `allocator`      | `join`, `ds`, `run`      | string           | The allocator the binary runs on: `"jemalloc"` (the default `jemalloc` cargo feature of `kermit`) or `"system"` (built with `--no-default-features`; glibc on Linux). Every timing depends on it (#112), so `BenchReport::new` adds it to every report. Always emitted since #112; kermit-lab back-fills `"system"` on every earlier row. |
| `query`          | `join`, `run`            | string           | Query name. `run`: from the YAML `queries:` list (e.g. `"triangle"`). `bench join`: the query file's stem. |
| `benchmark`      | `join`, `run`            | string           | Workload name. `run`: YAML benchmark name (e.g. `"triangle"`, `"watdiv-stress-c1"`). `bench join`: `"adhoc"`. |
| `relation_path`  | `ds`                     | string           | The single relation file passed to `bench ds`. Workspace-relative if invoked from the workspace root. |
| `relation_bytes` | `ds`                     | number (u64)     | On-disk size in bytes (raw, not formatted). Use `relation_size` from `metadata` for the human-readable form. |
| `tuples`         | `ds`, `join`, `run`      | number (usize)   | `ds`: tuples in the single relation. `join`, `run`: from v5, the flat row count of the relations the query reads (`Catalog::tuples(name).len()` summed over `Prepared::relations()`), the same for every structure; before v5, the total summed across all of the benchmark's relations (workload input size). |
| `arity`          | `ds`                     | number (usize)   | Relation arity. |
| `relations`      | *(removed 2026-09-09)*   | —                | `bench join` used to emit its relation count; it now reports `tuples` like `bench run`. Old reports may still carry it. |
| `verified`       | `run`, `probe`           | boolean          | `true` when `--verify` checked the query's result count against the YAML's `expected`; absent otherwise. `bench probe` checks every query that declares `expected`. |
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
linear model — up to v5 most often the deterministic, zero-variance space
measurements captured via `SpaceMeasurement`. Consumers must accept `null` for `slope`
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
  (e.g., `ds_layout_hasher`, `ds_layout_pruning`, `ds_layout_expansion`,
  `ds_layout_levels`, `ds_layout_seek`, `ds_layout_pointer_encoding`).
- `ds_config_<flag>` — runtime configuration value on the data structure
  (e.g., `ds_config_load_factor`, `ds_config_root_capacity`,
  `ds_config_child_capacity`).
- `ds_build_mode` — construction-time build mode for the data structure
  (single key; value is a `<mode>[:<params>]` string, e.g., `bulk`,
  `radix:8`). Emitted on every report of a structure with a build mode —
  ColumnTrie, TreeTrie and HashTrie — by the bench family that ran the build
  rather than by the relation, since every build mode builds the same
  structure. ColumnTrie values: `bulk` (default) and `incremental`
  (`--ds-build column-trie=incremental`). TreeTrie values: `serial` (default)
  and `parallel:N` (`--ds-build tree-trie=parallel:N`, N threads in
  1..=1024). HashTrie values: `bulk` (default, Algorithm 2), `incremental`
  (the per-tuple build, `serial` before #107; valid only with
  `ds_config_child_capacity: "grow"`, which the CLI enforces),
  `radix:<bits>` (`--ds-build hash-trie=radix:<bits>`, bits in 1..=16),
  `parallel:N` (`--ds-build hash-trie=parallel:N`, N threads in 1..=1024)
  and `presized:N` (`--ds-build hash-trie=presized:N`, N threads in
  1..=1024; valid only with `ds_config_root_capacity: "tuples"`, which the
  CLI enforces).
- `algo_layout_<dim>`, `algo_config_<flag>`, `algo_build_mode` — analogous
  prefixes for algorithm-level optimizations (reserved; not yet used).

When pivoting bench reports in kermit-lab, downstream code should:
- Treat a missing key as the algorithm/DS default, but only on rows of the
  structure that has the axis; every other structure's rows stay NaN (#85). For
  pre-standard reports predating this change, back-fill, on HashTrie rows,
  `ds_layout_hasher == "sip"` (the historical hash function),
  `ds_layout_pruning == "off"`, `ds_layout_expansion == "eager"` (every
  HashTrie before #92), `ds_config_load_factor == 0.7` (the
  historical constant), `ds_config_child_capacity == "grow"` and
  `ds_build_mode == "incremental"` (HashTrie's per-tuple build, the only one
  before #91); a HashTrie `ds_build_mode` of `serial` reads as `incremental`
  (`RENAMED_AXIS_VALUES` in kermit-lab's `defaults.py`). On ColumnTrie rows,
  back-fill `ds_build_mode == "incremental"` (ColumnTrie's build before issue
  #84; every ColumnTrie report since carries the axis), and, on TreeTrie rows,
  `ds_build_mode == "serial"` (TreeTrie's only build before issue #94).
  `data_structure` has named HashTrie as
  `"HashTrie"` in every report since the `axes` map was added, so the
  scoped fill misses no HashTrie row.
- Group on the relevant prefix to perform ablation analysis.
- `ds_layout_seek` (sorted tries only) is back-filled `"binary"` on
  `ColumnTrie` rows only. `TreeTrie` rows stay missing: its seek was
  linear before 9604293 (#67) and binary after, and a report cannot tell
  which. `HashTrie` has no seek.
- `ds_layout_expansion` (HashTrie only, `"eager"` / `"lazy"`, #92). Under
  `"lazy"` a join builds the children it first reaches, so `bench run` /
  `bench join` never probe the indexes they measure: `iteration` times one
  join against the query's indexes **in their as-built state**, on a fresh
  build made in untimed setup per sample, and `--verify` runs on a fresh
  build too, so `space` always measures the indexes as built. Every other
  structure reuses one set of indexes for `iteration`, which is the same
  state.
- `ds_layout_levels` (HashTrie only, `"all"` / `"keys"`, #116): whether a
  HashTrie index has a level per column (`all`, the default) or levels up to
  its last join-key column only (`keys`), the other columns kept as payload in
  its leaf rows. Under `keys` a query's indexes, and so `build`, `space`,
  `iteration` and the `index` lines, can differ from `all`'s; the answers do
  not. Emitted on `bench run`, `bench probe`, `bench join` and `bench ds`
  reports of HashTrie (`bench ds` always `"all"`: it refuses the flag).
  kermit-lab back-fills `"all"` on HashTrie rows without it.

Adding new keys under these prefixes does not require a `schema_version`
bump — the `axes` field is an open map.

## Versioning policy

- **Bump `schema_version`** on any breaking change: renaming a field,
  changing a value type, removing a key from `axes` (if external tooling
  pinned to it), or restructuring nesting.
- **Also bump** when what a metric measures changes, even if every field
  keeps its name and type: values on either side are no longer comparable.
  v3, v4 and v5 are such bumps.
- **No bump** for additive changes: new `axes` keys, new optional fields on
  `CriterionGroupRef`, new conventional values for `kind` or `metric`.
- Consumers should refuse to parse if `schema_version` is missing or
  greater than the highest version they know about. kermit-lab also
  refuses to load reports from both sides of version 3, of version 4, or
  of version 5, in one call: at 3 the `iteration` and `end_to_end` values
  changed meaning, at 4 `insertion`, `copies`, `end_to_end`, HashTrie's
  `space` and lazy-expansion HashTrie's `iteration` did, and at 5 the build
  metric, `iteration`, `end_to_end` and `space` did (`insertion` and
  `copies` became `build`).
- v6 is a structural bump with no meaning change: `space` moved from
  `criterion_groups` into `values`. kermit-lab loads v5 and v6 reports
  together.

## Change log

| Version | Date       | Change |
|---------|------------|--------|
| 1       | 2026-04-19 | Initial schema (`schema_version`, `kind`, `metadata`, `criterion_groups`). |
| 2       | 2026-05-04 | Added structured `axes: BTreeMap<String, serde_json::Value>` for downstream tooling. `metadata` retained as the human-readable surface. |
| 2 (no bump) | 2026-07-08 | Added the `optimiser` conventional `axes` key (query optimiser that planned the join's variable ordering). Additive — `axes` is an open map, so `schema_version` stays `2`. |
| 2 (no bump) | 2026-07-23 | Added the `queries_per_build` conventional `axes` key and the `end_to_end` time-metric function id (`--metrics end-to-end`). Additive — the key only appears when the metric is requested, so `schema_version` stays `2`. |
| 2 (no bump) | 2026-09-09 | `bench join` now runs through the generic runner: it gained `benchmark` (`"adhoc"`), `query` (the query file's stem), and `tuples`, and dropped the redundant `relations` count. No key changed name or type, so `schema_version` stays `2`. |
| 2 (no bump) | 2026-09-09 | Added the `verified` conventional `axes` key, present only when `bench run --verify` checked the query. Additive, so `schema_version` stays `2`. |
| 3       | 2026-10-02 | What the time metrics measure changed, so values are not comparable with v2, and kermit-lab refuses to load v2 and v3 reports together unless `allow_mixed_schema=True`. The changes on master at this version: `bench run` / `bench join` `iteration` and `end_to_end` time a streamed join whose rows are counted through a `black_box` sink and never materialised, and `--verify` counts the same way (#65); `bench ds` `iteration` / `end_to_end` traverse the relation without materialising it (#79); `insertion` and `end_to_end` rebuild every structure from the file's tuple order, and HashTrie's bucket index gains a per-capacity multiplier, which changes its build and lookup cost (#66); TreeTrie `seek` is a binary search (#67, from `9604293`); LFTJ reuses its per-depth buffers, so it no longer allocates on every `open`/`up` (#83); the LUBM caches are regenerated with a deterministic encoding (#74, from `8048fd7`); the toolchain is nightly 2026-10-01 (`106ad36`). |
| 3 (no bump) | 2026-10-03 | ColumnTrie's `from_tuples` builds in one pass (#84), so its `insertion` and `end_to_end` values drop; every ColumnTrie report now carries `ds_build_mode: "bulk"`, which tells it apart from earlier ColumnTrie rows. Additive, so `schema_version` stays `3`. `--ds-build incremental` restores the old build, and kermit-lab back-fills `incremental` on earlier ColumnTrie rows, so old ColumnTrie rows stay correctly labelled; compare the two modes within one binary. |
| 3 (no bump) | 2026-10-04 | TreeTrie and ColumnTrie reports carry `ds_layout_seek` (#80): `linear`, `binary` (the default) or `galloping`. Under the default every metric times the same `partition_point` search as before, so `schema_version` stays `3`. kermit-lab back-fills `binary` on earlier ColumnTrie rows only; earlier TreeTrie rows stay missing, since a v3 report may predate `9604293`. Compare strategies within one binary (see `BENCHMARKING.md`'s codegen precision bound). |
| 3 (no bump) | 2026-10-05 | The sorted tries' default seek becomes `galloping` (it was `binary`), after #80's strategy comparison. Every report since #80 records the strategy in `ds_layout_seek`, so rows from either side of the switch stay labelled and `schema_version` stays `3`. Earlier rows without the axis are unchanged: the ColumnTrie back-fill is still `binary`, which is what they ran. |
| 3 (no bump) | 2026-10-05 | Every HashTrie report carries `ds_build_mode` (#91): `serial`, the only build before, or `radix:<bits>`. Every mode builds the identical trie, so no metric changes meaning and `schema_version` stays `3`; kermit-lab back-fills `serial` on earlier HashTrie rows. `--ds-build` now takes `structure=mode` pairs (`column-trie=incremental`), and the bare form is rejected. |
| 3 (no bump) | 2026-10-05 | Added the `column_orders` conventional `axes` key (`--column-orders stored\|any`, #93), the `copies` time function, `space/Index_<π>_<base>` space functions and `index` metadata lines, the last three present only under `any` when the plan needs a copy. Additive — under `stored` only the new axis appears — so `schema_version` stays `3`. kermit-lab back-fills `stored` on earlier join rows. |
| 3 (no bump) | 2026-10-05 | TreeTrie reports carry `ds_build_mode` (#94): `serial` (the default, the build every earlier TreeTrie report ran) or `parallel:N` (`--ds-build tree-trie=parallel:N`). Every mode builds the identical trie, so `iteration` and `space` cannot move and `schema_version` stays `3`. kermit-lab back-fills `serial` on earlier TreeTrie rows and derives a numeric `threads` column. Compare build modes within one binary. |
| 3 (no bump) | 2026-10-05 | HashTrie reports can carry `ds_build_mode` `parallel:N` (#94, `--ds-build hash-trie=parallel:N`). Every mode builds the identical trie, so `schema_version` stays `3`; kermit-lab derives `threads` from it as for TreeTrie. |
| 3 (no bump) | 2026-10-06 | Every HashTrie report carries `ds_config_root_capacity` (#88): `"grow"` (the default, the only behaviour before) or `"tuples"` (`--ds-config root-capacity=tuples`, the root sized once from the tuple count). Under `grow` every metric measures the same build as before, so `schema_version` stays `3`; kermit-lab back-fills `"grow"` on earlier HashTrie rows. `tuples` changes the root's capacity, so it may move `space` and `iteration` as well as the build metrics. |
| 3 (no bump) | 2026-10-06 | Every report carries the `allocator` conventional `axes` key and an `allocator` metadata line (#112): the binary now links jemalloc by default (`"jemalloc"`), or the system allocator under `--no-default-features` (`"system"`). Every timing moves with the allocator, but each row records which one it ran on, as with the 2026-10-05 seek-default switch, so `schema_version` stays `3`. kermit-lab back-fills `"system"` on every earlier row (`BINARY_AXIS_DEFAULTS`). Compare allocators within one binary's source, built both ways. |
| 3 (no bump) | 2026-10-07 | HashTrie's presized parallel build is its own `ds_build_mode` value, `presized:N` (`--ds-build hash-trie=presized:N`, requires `--ds-config root-capacity=tuples`); `parallel:N` is the exact merge build under every root capacity. A new value of an existing key, so `schema_version` stays `3`. Reports written before this change carry `parallel:N` with `ds_config_root_capacity: "tuples"` for the presized build (the 2026-10-06 scaling run); kermit-lab does not rewrite them. |
| 3 (no bump) | 2026-10-07 | HashTrie builds by Algorithm 2 (#107): its default `ds_build_mode` is `bulk`, and the per-tuple build is `incremental`, spelled `serial` until now; kermit-lab reads old HashTrie `serial` rows as `incremental`, so they stay correctly labelled. Every HashTrie report gains `ds_config_child_capacity` (`grow` by default). The trie each existing config builds is unchanged, so `space` keeps its meaning, and so does `iteration` over an eagerly expanded trie; `insertion` and `end_to_end` of the default and of `radix:K` / `parallel:N` / `presized:N` now time Algorithm 2, and under `ds_layout_expansion=lazy` `iteration` and `end_to_end` time child expansion, which `resolve` now does by Algorithm 2 rather than `insert_at`. Compare these timings within one binary, as every timing already is. Additive, so `schema_version` stays `3`. |
| 4       | 2026-10-08 | What `insertion`, `copies`, `end_to_end`, HashTrie's `space` and lazy-expansion HashTrie's `iteration` measure changed, so they are not comparable with v3, and kermit-lab refuses to load v3 and v4 reports together unless `allow_mixed_schema=True` (#111). Every relation's tuples travel from the reader to every structure's build as one row-major buffer (`Tuples`) instead of a `Vec` per tuple: each `insertion` / `end_to_end` setup clone copies one buffer; TreeTrie and ColumnTrie sort it with one shared `Tuples::sort` and free one buffer where they freed a `Vec` per tuple; `bench run`'s `copies` metric (`--column-orders any`) times `permute_all` plus the copies' builds, which now copy one buffer and build from rows; and HashTrie keeps the buffer, so its `space` counts the buffer (capacity × 8 B), its row-id lists and chains (capacity × 4 B) and its tables, where it counted a `Vec` per tuple. HashTrie's `bench ds` `iteration` still walks the trie, now reading each row through its id, so it may move; #111's measurement records by how much. TreeTrie and ColumnTrie `space` and `bench run` / `bench join` `iteration` keep their meaning (the join's leaf product reads HashTrie chains through `LeafRows`; compare `iteration` within the codegen bound), except on lazy-expansion HashTrie cells (`ds_layout_expansion=lazy`): their `iteration` runs on fresh builds and times child expansion, and `resolve`'s Algorithm 2 now groups row ids and reads rows from the buffer. HashTriejoin `iteration` on a query that repeats a variable inside one atom also moves: its selection view copies 4-byte row ids per leaf bucket where it cloned tuples. Under `--optimiser cost-based`, HashTrie's column-distinct statistics are now a scan of its buffer (`TupleScan`), timed inside `end_to_end`, which is non-comparable already. No field or axis changed. |
| 4 (no bump) | 2026-10-08 | HashTrie's `radix:K`, `parallel:N` and `presized:N` copy their rows into first-key-hash partition order and keep the copy as the trie's buffer (#101 B1). Their `insertion`, `end_to_end` and, under `--column-orders any`, `copies` now include the copy (`add_index` builds each copy through `build_relation` in the cell's mode, so a radix-family cell's copies include the partition copy too), and their `iteration` reads rows in the new order. The trie's contents and capacities are unchanged, so `space` keeps its meaning. `bulk` and `incremental` are unchanged. A process change within the BuildMode axis, as #107's was, so `schema_version` stays `4`; compare these modes within one binary. |
| 4 (no bump) | 2026-10-14 | Every HashTrie list is a chain threaded through one 4-byte link per row (`ChainLinks`) instead of a `Vec<RowId>` (#101 B2), in every build mode and Layout. HashTrie's `insertion` and `end_to_end` time builds that allocate per table, never per list, and rewrite each row's link once per level; its `space` counts the links (capacity × 4 B) and 24-byte leaf buckets where it counted a heap list per chain and pending list and 32-byte leaf buckets; its `iteration`, the join's leaf product and `bench ds`'s trie walk, walks each chain through its links, one link and one row per step, and under `ds_layout_expansion=lazy` times an expansion that re-threads links; and under `--column-orders any` its `copies` time copies built the same way. The trie's contents, its tables' capacities and every join's output order are unchanged, and no field or axis changed, so `schema_version` stays `4`, as for #107 and #101 B1; compare HashTrie timings and `space` within one binary, or across B2 only through a measured A/B. TreeTrie and ColumnTrie reports are unchanged. |
| 5       | 2026-10-22 | What `insertion`, `copies`, `iteration`, `end_to_end` and `space` measure changed, so they are not comparable with v4, and kermit-lab refuses to load v4 and v5 reports together unless `allow_mixed_schema=True`. Relations load into a flat `Catalog` untimed, and each query builds, and times as `build`, only the indexes its plan reads: each a relation filtered by the query's constants and repeated variables, in plan order, copied exactly once from a borrowed `TupleView` in every build mode (so `serial` and `bulk` now copy inside the timer, as `radix:K`, `parallel:N` and `presized:N` already did, and a build is freed after the timer stops). `insertion` and `copies` are replaced by `build` (`--metrics build`; `copies` is no longer a metric); `iteration` excludes planning, which now runs once per query before any timer, and times the join over plain indexes with no const or selection wrapper; `end_to_end` and `space` are per query, and `space` sums the query's indexes in one function, replacing `space/<relation>` and `space/Index_<π>_<base>`. The `column_orders` axis is removed (`--column-orders` no longer exists; the planner is always free), `index_rows` is new, `tuples` for `join` and `run` is the flat row count of the relations the query reads, and each index adds an `index` metadata line. `bench ds` emits `{ds}/build` where it emitted `{ds}/insertion`. Criterion group names are unchanged. No v4 cell measures the same work under the same name, so there is no A/B against v4; the first v5 measurement is the next authoritative sweep (#99). |
| 6       | 2026-10-25 | The measurements made without Criterion move into a new top-level `values` list (`ValueEntry`: `function`, `metric`, `estimator`, `value`). `space` is an exact `values` entry in every subcommand and never a Criterion function, so `SpaceMeasurement` is gone. `bench probe` (`kind: probe`) is new: per query and cell, one describe build and one execution timed with an `Instant` (single-run `build` and `iteration`), the exact `space`, verification against `expected`, and no Criterion function. New axes: `plan` and `answer_rows`. Per query and cell the runners measure in one order: describe (one untimed build, read for the `index` lines, `space` and verification), `iteration` on that build, then `build` and `end_to_end`; reports are written after every query. No metric changes what it measures, so kermit-lab loads v5 and v6 together. |
| 6 (no bump) | 2026-11-02 | Plans are payload last (#116 step 1): every optimiser orders the join keys (the variables two or more atoms carry) first, so a query with a payload variable can change plan, and so its indexes, on every structure. Rows are told apart by the `plan` axis (v6), as the seek-default and allocator changes were told apart by their axes. |
| 6 (no bump) | 2026-11-02 | Added the `ds_layout_levels` axis (`--ds-layout-levels all\|keys`, #116 step 2) and the `index` line's ` levels n` suffix, present only under `keys`. Additive: an `all` report is unchanged, so `schema_version` stays 6. kermit-lab back-fills `all`. |
