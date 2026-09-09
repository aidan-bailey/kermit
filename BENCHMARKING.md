# Benchmarking Kermit

This is the **methodology guide** for benchmarking in Kermit: what the numbers
mean, how to get trustworthy ones, and which comparison answers which question.

It is deliberately *not* a command reference — for exact CLI flags, output
paths, and copy-paste recipes see [`USAGE.md`](USAGE.md). For the internal
design of the measurement pipeline see
[`docs/specs/benchmarking-architecture.md`](docs/specs/benchmarking-architecture.md).

Kermit is a research platform for comparing the Leapfrog Triejoin algorithm
across data structures and optimizations. Benchmarks are the evidence; this doc
is about producing evidence that holds up.

## The loop

```
  kermit bench …                          python/kermit-lab/
 ┌──────────────────┐   writes      ┌───────────────────────────┐
 │ run a benchmark  │ ───────────▶  │ kl.load(...) → DataFrame   │ ──▶ figures (PDF/PNG/SVG/PGF)
 │ under Criterion  │               │ kl.plot / presets          │ ──▶ stats (CIs, U-tests)
 └──────────────────┘               └───────────────────────────┘
        │ also writes
        ▼
  bench-runs/<kind>-<millis>.json   ← machine-readable summary (one BenchReport per query)
  target/criterion/<group>/<dir>/   ← per-function samples + estimates (mean, 95% CI)
```

Measurement (Rust + Criterion) and analysis (Python + pandas/matplotlib) are
**decoupled**: the CLI renders no plots. This is deliberate — a single `bench`
invocation pins exactly one `(data_structure, algorithm)` pair, so the
comparative figures the thesis needs are assembled *across* runs in
`kermit-lab`. See
[`docs/specs/2026-05-04-remove-criterion-graphs-design.md`](docs/specs/2026-05-04-remove-criterion-graphs-design.md).

## What you can measure

`bench ds` and `bench run` accept `--metrics`, defaulting to `insertion`,
`iteration`, and `space` (the opt-in `end-to-end` phase is *not* in the
default set). They fall into two families. (`bench join` accepts the same
`--metrics` as `bench run`; it is the ad-hoc form of the same measurement.)

### Time — three phases

Time is split into phases because they stress different things and a single
"total" number hides which one dominates:

| Phase | `--metrics` | What it times | When it matters |
| --- | --- | --- | --- |
| **Insertion** | `insertion` | Building the index from the input tuples (`from_tuples`, which presorts) | Construction-heavy workloads; one-shot queries; comparing build cost of structures |
| **Iteration** | `iteration` | Running the join itself (`open`/`up`/`seek`/`next` traversal) | The worst-case-optimal join's actual cost; the headline number for query performance |
| **End-to-end** | `end-to-end` | One database build **plus K query executions** in a single timed body (`T = build + K × query`, K from `--queries-per-build`, default 1) | Amortisation/crossover questions: which structure wins depends on how many queries run per build |

The end-to-end phase exists because summing `insertion + K × iteration` after
the fact assumes the phases are independent — precisely the assumption a
build-then-query crossover study is testing (e.g. cache effects across the
build→query boundary). Each Criterion sample therefore performs a *fresh*
build (`BatchSize::PerIteration`) followed by K joins. Two caveats:

- In `bench run`, the timed build goes through
  `ExecutionFamily::build_from_tuples`, which routes every relation through
  the same `RelationFamily::build_relation` site the `insertion` phase uses
  — so the build term is the `from_tuples` path in both phases, honouring
  any `--ds-config` values. The end-to-end build additionally pays for
  assembling the relation store (`BTreeMap<String, R>`), so the two numbers
  still are not identical.
- K is recorded in the report's `queries_per_build` axis (and surfaces as a
  DataFrame column), never in the Criterion function id.

In `kermit-lab`, pick a phase with the `phase=` argument (default
`"iteration"`, the join-execution phase). A scaling or bar plot mixes phases
only if you ask it to — each figure is one phase. A crossover figure facets
end-to-end curves by K:

```python
kl.plot(df, kind="line", x="tuples", y="time",
        colour="data_structure", facet="queries_per_build", phase="end_to_end")
```

### Space — deterministic by construction

Space measures `HeapSize::heap_size_bytes()` — heap-allocated bytes only, not
`size_of::<Self>`. It is **deterministic**: the same built relation returns the
same byte count every iteration, so Criterion records zero variance and the
95% CI is width-zero. That is expected, not a bug; figures still draw the CI
(width zero) by convention. The mechanics and the `iter_custom`/`black_box`
trap are documented in
[`docs/specs/space-benchmarks.md`](docs/specs/space-benchmarks.md).

## Getting trustworthy numbers

### Sampling controls

The parent `bench` flags tune Criterion's sampling (see `USAGE.md` for syntax):

- `--sample-size` — number of measured samples. More samples → tighter CIs,
  longer runs.
- `--measurement-time` / `--warm-up-time` — seconds Criterion spends measuring
  and warming caches/branch-predictors. Too-short warm-up biases the first
  samples; too-short measurement inflates variance.

Defaults are tuned for a quick signal. For numbers you will publish or compare,
raise `--sample-size` and `--measurement-time` until the 95% CI is narrow
relative to the effect you are claiming. A 3% speedup with overlapping CIs is
not a speedup.

### Confidence intervals are the point

Every Criterion function writes `estimates.json` with a point estimate **and a
95% confidence interval**. `kermit-lab` reads both — its plots draw error bars
straight from `mean_lo`/`mean_hi`, and the stats helpers (below) quantify
whether two distributions actually differ. Always read the interval, never just
the mean.

### Reproducibility

- **Keys are dictionary-encoded `usize`** and inputs are fixed files, so a
  static benchmark is deterministic across runs given the same data.
- **Generator benchmarks** (WatDiv, LUBM) hash their spec; a cached dataset is
  reused on a `spec_hash` match. Changing the YAML's parameters makes the next
  `bench run` abort with `SpecDrift` rather than silently regenerate — pass
  `--force` to opt in. WatDiv/LUBM also have generator-level non-determinism
  (seeds, thread count); the discipline for keeping runs comparable is in
  [`docs/benchmarks/WATDIV.md`](docs/benchmarks/WATDIV.md) and
  [`docs/benchmarks/LUBM.md`](docs/benchmarks/LUBM.md).
- **Don't move two variables at once.** Because benchmarks compare structures
  against each other, silently changing a sibling structure or algorithm
  mid-study invalidates prior measurements (Priority 6 in
  [`CLAUDE.md`](CLAUDE.md)). Re-run the whole comparison set when the platform
  changes under it.

## Which comparison answers which question

Map the question you have to the figure/stat that answers it. All plot shapes
are presets over the general `kl.plot` engine; the analysis helpers live in
`kermit_lab.analysis`. Full API: the
[kermit-lab README](python/kermit-lab/README.md).

| Question | Vary | Hold fixed | Tool |
| --- | --- | --- | --- |
| Does it scale? | input size (`tuples`) | DS, algorithm, query | `kl.scaling` (log-log time vs tuples) |
| Which DS is faster here? | data structure | query, size | `kl.bar_time(df, query=…)` + `kl.compare` / `kl.mannwhitney_u` |
| How does one DS handle different workloads? | query | DS, algorithm | `kl.bar_queries(df, ds=[…], algo=[…])` |
| Space vs time? | DS | query | `kl.tradeoff` (scatter) |
| Run-to-run spread? | — | DS, query | `kl.dist` (violin) + `kl.mannwhitney_u` |
| **Does an optimization help?** | **one optimization axis** | DS, query, all other axes | `kl.ablation(df, axis=…)` (see below) |

### Ablation: measuring an optimization

The optimization standard records *which optimization was active* during a run
as axes on each report, under three prefixes — `ds_layout_*` (compile-time
layout), `ds_config_*` (runtime flag), and `ds_build_mode` (construction-time
mode). `kl.load` surfaces every such axis as a DataFrame column, so an
**ablation** holds the structure and workload fixed and varies exactly one
optimization axis to isolate its effect.

Because most optimization flags pin a single value per run (e.g.
`--ds-layout-hasher` picks one hasher), you produce one report per setting and
load them together:

```sh
# two runs, same workload, different hasher → one ablation dataset
kermit bench --report-json bench-runs/tri-sip.json \
  run triangle -i hash-trie -a hash-triejoin --ds-layout-hasher sip    --metrics iteration
kermit bench --report-json bench-runs/tri-fx.json \
  run triangle -i hash-trie -a hash-triejoin --ds-layout-hasher fxhash --metrics iteration
```

```python
import kermit_lab as kl
df = kl.load("bench-runs/tri-*.json")
kl.discover_opt_columns(df)            # ['ds_layout_hasher']
kl.ablation(df, axis="ds_layout_hasher")          # the preset
# …or bind the axis to any channel via the general engine:
kl.plot(df, kind="bar", x="ds_layout_hasher", y="time",
        colour="data_structure", facet="query")
```

The full optimization model (Layout / Config / BuildMode, how to add one, and
the bench-axis namespace) is in
[`docs/specs/optimization-standard.md`](docs/specs/optimization-standard.md); a runnable
walkthrough is in
[`python/kermit-lab/notebooks/06_ablation.ipynb`](python/kermit-lab/notebooks/06_ablation.ipynb).

## Statistical tooling

When a bar chart isn't enough to claim a difference, `kermit_lab.analysis`
provides four helpers:

| Helper | Signature (key args) | Use it to |
| --- | --- | --- |
| `summary` | `summary(df, *, rows, cols, value="mean_ns")` | Pivot many runs into a readable 2-D table (e.g. DS × size). |
| `compare` | `compare(df, *, baseline, target, group_by="data_structure")` | Pair every baseline row with its target and compute speedup per workload. |
| `bootstrap_ratio_ci` | `bootstrap_ratio_ci(a, b, *, ci=0.95, rng=…)` | Percentile-bootstrap CI for `mean(a)/mean(b)` from per-iter samples — "is this speedup real?" |
| `mannwhitney_u` | `mannwhitney_u(a, b)` → `(U, p)` | Non-parametric test that two sample distributions differ; pair with a violin plot. |

`summary`/`compare` consume the summary frame from `kl.load`;
`bootstrap_ratio_ci`/`mannwhitney_u` consume per-iteration arrays from
`kl.load_samples` (the `per_iter_ns` column). The notebooks
[`03_compare_ds.ipynb`](python/kermit-lab/notebooks/03_compare_ds.ipynb) and
[`05_distributions.ipynb`](python/kermit-lab/notebooks/05_distributions.ipynb)
show both end-to-end.

## A minimal study, start to finish

1. **Decide the question** and what varies vs. stays fixed (table above).
2. **Run** the benchmark set, one report per cell of the comparison, with a
   stable `--sample-size`/`--measurement-time` across all of them (`USAGE.md`).
3. **Load** every report together: `df = kl.load("bench-runs/*.json")`.
4. **Plot** the matching shape and **read the CIs**; reach for `compare` /
   `bootstrap_ratio_ci` / `mannwhitney_u` before claiming a difference.
5. **Re-run the whole set** if the platform changed under it — never mix
   measurements taken across a sibling change.

`USAGE.md` ends with a complete worked example (a six-point scaling plot from a
clean checkout).

## Where to look next

| For… | See |
| --- | --- |
| Exact CLI flags, output paths, recipes | [`USAGE.md`](USAGE.md) |
| The `BenchReport` JSON schema (axes, criterion_groups) | [`docs/specs/bench-report-schema.md`](docs/specs/bench-report-schema.md) |
| Measurement-pipeline internals | [`docs/specs/benchmarking-architecture.md`](docs/specs/benchmarking-architecture.md) |
| Space measurement mechanics | [`docs/specs/space-benchmarks.md`](docs/specs/space-benchmarks.md) |
| Optimization axes (Layout/Config/BuildMode) | [`docs/specs/optimization-standard.md`](docs/specs/optimization-standard.md) |
| The plotting engine design | [`docs/specs/2026-06-10-optimization-axis-graphs-design.md`](docs/specs/2026-06-10-optimization-axis-graphs-design.md) |
| Analysis/plotting API + notebooks | [`python/kermit-lab/README.md`](python/kermit-lab/README.md) |
| Benchmark catalogue, WatDiv, LUBM | [`docs/benchmarks/README.md`](docs/benchmarks/README.md) |
| System architecture & data flow | [`ARCHITECTURE.md`](ARCHITECTURE.md) |
