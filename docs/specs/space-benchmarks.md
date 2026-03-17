# Space Benchmarks — Criterion Output Specification

**Date:** 2026-03-16

## Directory Layout

Criterion writes output to `target/criterion/` with the following structure:

```
target/criterion/
├── {group}/                          # e.g. TreeTrie, ColumnTrie
│   └── {benchmark}/                  # e.g. Space_Exponential_3_27
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

**Group** is the data structure name (`TreeTrie`, `ColumnTrie`).

**Benchmark** is the function ID with `/` replaced by `_`
(e.g. `Space/Exponential/3/27` → `Space_Exponential_3_27`).

On each run, the previous `new/` is rotated to `base/` and a `change/`
directory is created with regression estimates.

## benchmark.json

Identifies the benchmark. This is the primary metadata file.

```json
{
    "group_id": "TreeTrie",
    "function_id": "Space/Exponential/3/27",
    "value_str": null,
    "throughput": {
        "Elements": 27
    },
    "full_id": "TreeTrie/Space/Exponential/3/27",
    "directory_name": "TreeTrie/Space_Exponential_3_27",
    "title": "TreeTrie/Space/Exponential/3/27"
}
```

| Field | Type | Description |
|-------|------|-------------|
| `group_id` | string | Benchmark group name (data structure) |
| `function_id` | string | Benchmark function name within the group |
| `value_str` | string \| null | Optional parameter string (unused) |
| `throughput` | object \| null | Throughput config; `{"Elements": n}` where `n` is tuple count |
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

The number of entries equals the configured `sample_size` (10 for space
benchmarks, minus the warm-up sample).

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

The benchmark function ID `Space/{Generator}/{param}/{n}` maps to the
filesystem as `Space_{Generator}_{param}_{n}`.

| Function ID | Directory | Tuple count |
|-------------|-----------|-------------|
| `Space/Exponential/1/1` | `Space_Exponential_1_1` | 1 |
| `Space/Exponential/2/4` | `Space_Exponential_2_4` | 4 |
| `Space/Exponential/3/27` | `Space_Exponential_3_27` | 27 |
| `Space/Exponential/4/256` | `Space_Exponential_4_256` | 256 |
| `Space/Exponential/5/3125` | `Space_Exponential_5_3125` | 3125 |
| `Space/Factorial/1/1` | `Space_Factorial_1_1` | 1 |
| `Space/Factorial/2/2` | `Space_Factorial_2_2` | 2 |
| `Space/Factorial/3/6` | `Space_Factorial_3_6` | 6 |
| `Space/Factorial/4/24` | `Space_Factorial_4_24` | 24 |
| `Space/Factorial/5/120` | `Space_Factorial_5_120` | 120 |
| `Space/Factorial/6/720` | `Space_Factorial_6_720` | 720 |
| `Space/Factorial/7/5040` | `Space_Factorial_7_5040` | 5040 |
| `Space/Factorial/8/40320` | `Space_Factorial_8_40320` | 40320 |
| `Space/Factorial/9/362880` | `Space_Factorial_9_362880` | 362880 |

Groups: `TreeTrie`, `ColumnTrie` (28 benchmark directories total).
