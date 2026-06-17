# Space Benchmarks — Criterion Output Specification

**Date:** 2026-03-16

Space benchmarking is implemented entirely in the CLI: `kermit bench ds
<relation> -i <ds> --metrics space` (and `kermit bench run <name> --metrics
space`), routed through `kermit::measurement::SpaceMeasurement`. There is no
standalone benchmark binary; this spec describes the Criterion artefacts those
commands write. See [`bench-report-schema.md`](bench-report-schema.md) for the
machine-readable report that points at these artefacts.

> **Historical note.** An earlier design proposed a standalone `kermit-ds`
> benchmark binary with `Exponential`/`Factorial` tuple generators. That binary
> was never built; the `bench ds`/`bench run --metrics space` CLI path
> superseded it. Function IDs of the form `Space/Exponential/...` do not exist.

## Directory Layout

Criterion writes output to `target/criterion/` with the following structure:

```
target/criterion/
├── {group}/                          # bench ds: "ds" (overridable via --name)
│   └── {directory_name}/             # e.g. TreeTrie_space
│       ├── new/                      # latest run
│       │   ├── estimates.json
│       │   ├── sample.json
│       │   ├── benchmark.json
│       │   └── tukey.json
│       ├── base/                     # previous run (after second run)
│       │   ├── estimates.json
│       │   ├── sample.json
│       │   ├── benchmark.json
│       │   └── tukey.json
│       └── change/                   # regression data (after second run)
│           └── estimates.json
```

**Group** is the Criterion group name. For `bench ds` it defaults to `ds`
(overridable via `--name`); for `bench run` it is
`{prefix}/{benchmark}/{query}/{ds}/{algo}` (prefix defaults to `run`).

**directory_name** is the function ID with `/` replaced by `_`. The space
function ID is `{IndexStructure}/space` — the `IndexStructure` Debug string is
one of `ColumnTrie`, `HashTrie`, `TreeTrie` — so e.g. `TreeTrie/space` becomes
`TreeTrie_space` on disk. Read the canonical segment from each subdir's
`benchmark.json:directory_name` rather than computing it.

On each run, the previous `new/` is rotated to `base/` and a `change/`
directory is created with regression estimates.

## benchmark.json

Identifies the benchmark. This is the primary metadata file.

```json
{
    "group_id": "ds",
    "function_id": "TreeTrie/space",
    "value_str": null,
    "throughput": {
        "Elements": 27
    },
    "full_id": "ds/TreeTrie/space",
    "directory_name": "ds/TreeTrie_space",
    "title": "ds/TreeTrie/space"
}
```

| Field | Type | Description |
|-------|------|-------------|
| `group_id` | string | Benchmark group name (`ds` for `bench ds`; the data structure is the function prefix) |
| `function_id` | string | Benchmark function name within the group (`{IndexStructure}/space`) |
| `value_str` | string \| null | Optional parameter string (unused) |
| `throughput` | object \| null | Throughput config; `{"Elements": n}` where `n` is the tuple count of the benchmarked relation (the `27` above is illustrative) |
| `full_id` | string | `{group_id}/{function_id}` |
| `directory_name` | string | Filesystem path segment (slashes replaced with underscores) |
| `title` | string | Display name (same as `full_id`) |

## estimates.json

Statistical estimates for the measurement. All values are in **bytes** (the
unit of `SpaceMeasurement::Value`).

```json
{
    "mean": {
        "confidence_interval": {
            "confidence_level": 0.95,
            "lower_bound": 1664.0,
            "upper_bound": 1664.0
        },
        "point_estimate": 1664.0,
        "standard_error": 0.0
    },
    "median": {
        "confidence_interval": {
            "confidence_level": 0.95,
            "lower_bound": 1664.0,
            "upper_bound": 1664.0
        },
        "point_estimate": 1664.0,
        "standard_error": 0.0
    },
    "median_abs_dev": {
        "confidence_interval": {
            "confidence_level": 0.95,
            "lower_bound": 0.0,
            "upper_bound": 0.0
        },
        "point_estimate": 0.0,
        "standard_error": 0.0
    },
    "slope": {
        "confidence_interval": {
            "confidence_level": 0.95,
            "lower_bound": 1664.0,
            "upper_bound": 1664.0
        },
        "point_estimate": 1664.0,
        "standard_error": 0.0
    },
    "std_dev": {
        "confidence_interval": {
            "confidence_level": 0.95,
            "lower_bound": 0.0,
            "upper_bound": 0.0
        },
        "point_estimate": 0.0,
        "standard_error": 0.0
    }
}
```

| Estimate | Description | Space benchmarks note |
|----------|-------------|----------------------|
| `mean` | Arithmetic mean of per-iteration values | Equals `heap_size_bytes()` exactly |
| `median` | Median of per-iteration values | Identical to mean (deterministic) |
| `median_abs_dev` | Median absolute deviation | Always 0.0 (zero variance) |
| `slope` | Slope of linear regression (iters vs total) | Equals mean (perfectly linear) |
| `std_dev` | Standard deviation | Always 0.0 (zero variance) |

Each estimate contains:

| Field | Type | Description |
|-------|------|-------------|
| `point_estimate` | f64 | Measured value in bytes |
| `standard_error` | f64 | Standard error of the estimate |
| `confidence_interval.confidence_level` | f64 | Always 0.95 |
| `confidence_interval.lower_bound` | f64 | Lower bound in bytes |
| `confidence_interval.upper_bound` | f64 | Upper bound in bytes |

**Key field for consumers:** `mean.point_estimate` — this is the heap size in
bytes for the data structure under the given input.

Because space measurement is deterministic, all five estimates collapse to the
same value, `standard_error` is 0.0, and confidence intervals have equal
bounds. This is expected behaviour, not a bug.

## sample.json

Raw sample data collected during the benchmark run.

```json
{
    "sampling_mode": "Linear",
    "iters": [
        32768.0,
        65536.0
    ],
    "times": [
        54525952.0,
        109051904.0
    ]
}
```

| Field | Type | Description |
|-------|------|-------------|
| `sampling_mode` | string | Always `"Linear"` for `iter_custom` benchmarks |
| `iters` | f64[] | Iteration counts for each sample |
| `times` | f64[] | **Total** measurement value for that sample (not per-iteration) |

Despite the field name `times`, these are **byte totals** (not durations).
Criterion uses this name regardless of the `Measurement` type.

The per-iteration value is `times[i] / iters[i]`. For space benchmarks this
quotient is constant across all samples (e.g. `54525952 / 32768 = 1664`).

The number of entries equals the configured `sample_size` (set via
`--sample-size`, default 100, minimum 10), minus the warm-up sample.

## tukey.json

Tukey fence thresholds for outlier classification.

```json
[1664.0, 1664.0, 1664.0, 1664.0]
```

Four values: `[low_severe, low_mild, high_mild, high_severe]`. Samples outside
the mild fences are classified as mild outliers; outside the severe fences as
severe outliers.

For deterministic data all four values are identical (equal to the measurement
value), so no samples are classified as outliers.

## change/estimates.json

Regression estimates comparing `new/` against `base/`. Only present after the
second run of a benchmark.

```json
{
    "mean": {
        "confidence_interval": {
            "confidence_level": 0.95,
            "lower_bound": 0.0,
            "upper_bound": 0.0
        },
        "point_estimate": 0.0,
        "standard_error": 0.0
    },
    "median": {
        "confidence_interval": {
            "confidence_level": 0.95,
            "lower_bound": 0.0,
            "upper_bound": 0.0
        },
        "point_estimate": 0.0,
        "standard_error": 0.0
    }
}
```

Values are **fractional change** (not absolute bytes). A `point_estimate` of
0.0 means no change; 0.1 would mean a 10% increase.

Only `mean` and `median` are present (no `slope`, `std_dev`, or
`median_abs_dev`).

## Benchmark ID Mapping

Each benchmarked data structure produces exactly one space function, with ID
`{IndexStructure}/space`, which maps to the filesystem as `{IndexStructure}_space`.
The `IndexStructure` Debug string is one of `ColumnTrie`, `HashTrie`, `TreeTrie`.

| Function ID | Directory | Tuple count |
|-------------|-----------|-------------|
| `ColumnTrie/space` | `ColumnTrie_space` | tuple count of the benchmarked relation |
| `HashTrie/space` | `HashTrie_space` | tuple count of the benchmarked relation |
| `TreeTrie/space` | `TreeTrie_space` | tuple count of the benchmarked relation |

One `{IndexStructure}/space` function is produced per data structure
benchmarked. Under `bench ds` these all land in the `ds` group; under
`bench run` the group is `{prefix}/{benchmark}/{query}/{ds}/{algo}`.
