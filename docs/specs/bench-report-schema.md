# `BenchReport` JSON schema

**Current schema version:** `4`
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
    "schema_version": 4,
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
| `schema_version`   | u32                          | Currently `4`. Consumers should refuse unknown majors. |
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

### Time and space function ids

`bench run` / `bench join` write the time functions `insertion` (every
base relation built from its file-order tuples), `iteration` (the
streamed join over the prebuilt engine), `end_to_end` (opt-in: a fresh
build plus K joins) and, under `--column-orders any` when the plan needs
a copy, `copies` (every reordered copy the query needs, permuted and
built through the same `build_relation` as `insertion`; emitted beside
`insertion`, so only when `insertion` is measured). The space functions
are `space/<relation>` per base relation and `space/Index_<π>_<base>` per
copy (e.g. `space/Index_1_0_edge`). Each copy also gets an `index`
metadata line, e.g. `edge (1, 0)`. Under `stored` no `copies` or
`space/Index_*` function is ever written. A query's copies are part of
the engine `iteration` reads and of `end_to_end`'s fresh build.

## Conventional `axes` keys

Each call site populates whichever subset is meaningful for that subcommand.
The keys below are the *committed* vocabulary — adding a new one is a minor
schema change (no version bump unless an existing key changes type or
semantics).

| Key              | Populated by             | JSON type        | Notes |
|------------------|--------------------------|------------------|-------|
| `data_structure` | `join`, `ds`, `run`      | string           | `"TreeTrie"`, `"ColumnTrie"`, `"HashTrie"`. The `IndexStructure::axis_value` string. |
| `algorithm`      | `join`, `run`            | string           | `"LeapfrogTriejoin"`, `"HashTriejoin"`. The `JoinAlgorithm::axis_value` string. |
| `optimiser`      | `join`, `run`            | string           | Query optimiser that planned the join's variable ordering. Values: `"lexicographic"` (default), `"cardinality"`, `"cost-based"`. A `"cost-based"` run's `end_to_end` includes its per-column statistics walk; its `iteration` does not. Emitted by `bench join` and `bench run` (not `bench ds`, which performs no join). |
| `column_orders`  | `join`, `run`            | string           | Column-order policy the join was planned under (`--column-orders`). Values: `"stored"` (default; each relation read in its stored column order) and `"any"` (the planner is free; an atom whose plan disagrees with its stored order reads a per-query reordered copy). Always emitted since #93; kermit-lab back-fills `"stored"` on earlier join reports (rows with an `algorithm`). |
| `allocator`      | `join`, `ds`, `run`      | string           | The allocator the binary runs on: `"jemalloc"` (the default `jemalloc` cargo feature of `kermit`) or `"system"` (built with `--no-default-features`; glibc on Linux). Every timing depends on it (#112), so `BenchReport::new` adds it to every report. Always emitted since #112; kermit-lab back-fills `"system"` on every earlier row. |
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
  (e.g., `ds_layout_hasher`, `ds_layout_pruning`, `ds_layout_expansion`,
  `ds_layout_seek`, `ds_layout_pointer_encoding`).
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
  `bench join` never probe the engine they loaded: `iteration` times one
  join against the relations **in their as-built state**, on a fresh build
  made in untimed setup per sample, and `--verify` runs on a fresh build
  too, so `space` always measures the relations as built. Every other
  structure reuses one engine for `iteration`, which is the same state, so
  every pre-existing cell measures what it did and the schema stays at
  version 3.

Adding new keys under these prefixes does not require a `schema_version`
bump — the `axes` field is an open map.

## Versioning policy

- **Bump `schema_version`** on any breaking change: renaming a field,
  changing a value type, removing a key from `axes` (if external tooling
  pinned to it), or restructuring nesting.
- **Also bump** when what a metric measures changes, even if every field
  keeps its name and type: values on either side are no longer comparable.
  v3 and v4 are such bumps.
- **No bump** for additive changes: new `axes` keys, new optional fields on
  `CriterionGroupRef`, new conventional values for `kind` or `metric`.
- Consumers should refuse to parse if `schema_version` is missing or
  greater than the highest version they know about. kermit-lab also
  refuses to load reports from both sides of version 3, or of version 4,
  in one call: at 3 the `iteration` and `end_to_end` values changed
  meaning, and at 4 `insertion`, `copies`, `end_to_end` and HashTrie's
  `space` did.

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
| 4       | 2026-10-08 | What `insertion`, `copies`, `end_to_end` and HashTrie's `space` measure changed, so they are not comparable with v3, and kermit-lab refuses to load v3 and v4 reports together unless `allow_mixed_schema=True` (#111). Every relation's tuples travel from the reader to every structure's build as one row-major buffer (`Tuples`) instead of a `Vec` per tuple: each `insertion` / `end_to_end` setup clone copies one buffer; TreeTrie and ColumnTrie sort it with one shared `Tuples::sort` and free one buffer where they freed a `Vec` per tuple; `bench run`'s `copies` metric (`--column-orders any`) times `permute_all` plus the copies' builds, which now copy one buffer and build from rows; and HashTrie keeps the buffer, so its `space` counts the buffer (capacity × 8 B), its row-id lists and chains (capacity × 4 B) and its tables, where it counted a `Vec` per tuple. HashTrie's `bench ds` `iteration` still walks the trie, now reading each row through its id, so it may move; #111's measurement records by how much. TreeTrie and ColumnTrie `space` and `bench run` / `bench join` `iteration` keep their meaning (the join's leaf product reads HashTrie chains through `LeafRows`; compare `iteration` within the codegen bound). No field or axis changed. |
