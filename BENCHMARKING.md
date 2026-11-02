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

## Verify before you trust

Before a measurement campaign, run each cell once with `--verify`:

```sh
kermit bench run <name> -i all -a all -m iteration --verify
```

For every query whose YAML entry carries an `expected` cardinality, the
runner executes the query once before timing it and compares the returned
tuple count with `expected`; a mismatch aborts the run, so a benchmark that
declares its answers cannot produce timings for wrong ones. Queries without
`expected` are timed as usual and flagged on stderr as not verified. The
check is opt-in because it costs one query execution per query and cell (on
the build the runner makes anyway), which is unwelcome in a long sweep once
the answers are known to be right.

`bench probe` is the cheaper screen. It takes `bench run`'s selection flags
and never runs Criterion. Per query and cell it builds once and runs once,
timing both, verifies every query that declares `expected` (a mismatch
aborts), and records the exact `space`, the answer count (`answer_rows`)
and the plan (`plan`). Its two times are single runs, so they are cold: use
them to screen queries and estimate a sweep, or to time queries too slow for
Criterion, never beside a Criterion estimate. On the 2026-10-24 sweep's WatDiv
and JOB template 13, every cell, a probe pass took 5.3 minutes where the
Criterion-based screen it replaces (`-m build --verify` at 10 samples / 1 s / 1
s) took 29.1 (`kermit-bench-runs/efficiency-ab-2026-10-26/`).

```sh
kermit bench probe <name> -i all -a all --optimiser cost-based
```

`bench fetch` verifies relation digests for the same reason: a relation that
declares a `sha256` is checked against the download before it is cached and
re-hashed on every `fetch`, so a corrupted or swapped data file is caught
before it is measured.

## What you can measure

`bench ds` and `bench run` accept `--metrics`, defaulting to `build`,
`iteration`, and `space` (the opt-in `end-to-end` phase is *not* in the
default set). They fall into two families. (`bench join` accepts the same
`--metrics` as `bench run`; it is the ad-hoc form of the same measurement.)

Per query and cell, every runner measures in one order: one untimed
*describe* build (read for the `index` lines, `space`, verification and a
probe's single runs), `iteration` on that same build, then, with it
dropped, `build` and `end_to_end`. So no other index set is alive while a
build is timed. Lazy HashTrie cells follow the same order, but their
`iteration` builds a fresh set per sample (#92). Every report row carries an
`estimator`: `criterion` for a Criterion estimate, `exact` for `space`, and
`single` for a probe's one-run times.

### Time — three phases

Time is split into phases because they stress different things and a single
"total" number hides which one dominates:

| Phase | `--metrics` | What it times | When it matters |
| --- | --- | --- | --- |
| **Build** | `build` | `bench run` / `bench join`: building the indexes one query reads (each a relation filtered by the query's constants and repeated variables, in plan order) from the catalog's flat tuples, plus its all-constant checks. `bench ds`: building the one relation from its flat tuples in file order. The built structure is freed after the timer stops | Construction-heavy workloads; one-shot queries; comparing build cost of structures |
| **Iteration** | `iteration` | Running the join itself (`open`/`up`/`seek`/`next` traversal). In `bench run` / `bench join`, each result tuple is passed to a counting `black_box` sink and dropped: no result rows are allocated, and memory does not grow with the result. `bench ds` scans its relation the same way: tuples are counted through the sink, never collected. | The worst-case-optimal join's actual cost; the headline number for query performance |
| **End-to-end** | `end-to-end` | One database build **plus K query executions** in a single timed body (`T = build + K × query`, K from `--queries-per-build`, default 1); each execution counts its rows the same way as Iteration | Amortisation/crossover questions: which structure wins depends on how many queries run per build |

Reports with `schema_version` below 3 are not comparable with later ones.
Before v3, `bench run` / `bench join` timed a join that also collected every
result row into memory, `bench ds` timed a scan that collected every tuple,
and the rebuilds, HashTrie's bucket index and TreeTrie's `seek` all differed;
the v3 row of
[`docs/specs/bench-report-schema.md`](docs/specs/bench-report-schema.md)
lists every change. `kermit-lab` refuses to load both kinds together unless
you pass `allow_mixed_schema=True`.
`--verify` counts rows the same way, so verifying a result of billions of
rows needs no more memory than a small one.

Reports with `schema_version` 3 and 4 are not comparable in `insertion`,
`copies`, `end_to_end`, HashTrie's `space` or lazy-expansion HashTrie's
`iteration`. From v4 (#111) every relation's tuples reach every build as
one row-major buffer (`Tuples`) instead of a `Vec` per tuple, and HashTrie
keeps that buffer and stores 4-byte row ids where it stored a `Vec` per
tuple. TreeTrie and ColumnTrie `space` and the joins' `iteration` keep their
meaning, except on lazy-expansion HashTrie cells: their `iteration` runs on
fresh builds and times child expansion, whose Algorithm 2 now groups row
ids and reads rows from the buffer. HashTrie's `bench ds` `iteration` walk
now reads rows through their ids, so compare it across the boundary only
through a measured A/B. The v4 row of
[`docs/specs/bench-report-schema.md`](docs/specs/bench-report-schema.md)
lists the changes, and `kermit-lab` refuses to load v3 and v4 reports
together unless you pass `allow_mixed_schema=True`.

Reports with `schema_version` 5 (2026-10-22) are not comparable with v4 in
`build`, `iteration`, `end_to_end` or `space`, and have no `insertion` or
`copies` metric: `build` replaces them. Relations are read once into a flat
catalog, and each query builds, and times, only the indexes its plan reads,
once, from a borrowed view of those tuples in every build mode. So
`serial` and `bulk` builds now copy inside the timer, as `radix:K`,
`parallel:N` and `presized:N` already did; `iteration` excludes planning;
`space` is one function per query; and the `column_orders` axis is gone,
since `--column-orders` no longer exists and the planner is always free.
The v5 row of
[`docs/specs/bench-report-schema.md`](docs/specs/bench-report-schema.md)
lists the changes, and `kermit-lab` refuses to load v4 and v5 reports
together unless you pass `allow_mixed_schema=True`. There is no A/B against
v4; the first v5 measurement is the next full sweep (#99).

Reports with `schema_version` 6 (2026-10-25) measure what v5 measured:
`space` moved from a Criterion function into the report's `values` list as
an exact value, the axes gain `plan` and `answer_rows`, and `bench probe`
reports (`kind: probe`) are new. `kermit-lab` loads v5 and v6 together.
Within one binary, `iteration` now runs before `build` in every cell. The
measure-layer spec's order check
([`docs/specs/2026-10-25-bench-runner-measure-layer-design.md`](docs/specs/2026-10-25-bench-runner-measure-layer-design.md),
Acceptance 7; run 2026-10-26 in `kermit-bench-runs/order-check-2026-10-26/`)
measured what that changes. It timed `build` after `iteration` against `build`
alone, in 5 alternating replicates, on the 2026-10-24 sweep's WatDiv Criterion
set (123 queries) and JOB template 6, in every cell.

- In five cells there is no effect: the 95% CI of the per-cell geometric-mean
  ratio contains 1.
- WatDiv TreeTrie builds run 0.6% faster after `iteration` (ratio 0.994, CI
  [0.990, 0.998]). That is a real but small order effect, well inside the
  codegen precision bound.

So compare TreeTrie `build` across v5 and v6 only within that bound, as any
two binaries already are.

The end-to-end phase exists because summing `build + K × iteration` after
the fact assumes the phases are independent — precisely the assumption a
build-then-query crossover study is testing (e.g. cache effects across the
build→query boundary). Each Criterion sample therefore performs a *fresh*
build (`BatchSize::PerIteration`) followed by K joins. Two caveats:

- In `bench run`, the timed build is `ExecutionFamily::build_indexes`, the
  same call the `build` phase times, and every index it makes goes through
  `RelationFamily::build_index`, the one construction site, honouring any
  `--ds-config` values and each structure's `--ds-build` mode. So the build
  term is the same work in both phases; a gap between `end_to_end` and
  `build + K × iteration` beyond noise is the build→query interaction this
  phase measures.
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

Space measures `HeapSize::heap_size_bytes()` — heap-allocated bytes only,
not `size_of::<Self>` — of what one describe build produced, read before
anything runs on it, so a lazy HashTrie's `space` is its as-built footprint.
It is exact: since report v6 it is one `values` entry (`estimator: exact`),
not a Criterion function, and kermit-lab draws it with a zero-width
interval, as it drew v5's zero-variance Criterion `space`. `bench probe`
records it too, so one probe pass over a sweep's cells gives every cell's
space.

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
  `--force` to opt in. A LUBM cache from before reproducible entailment is
  refused the same way (`OutdatedCache`). WatDiv/LUBM also have
  generator-level non-determinism (seeds, thread count); the discipline for
  keeping runs comparable is in
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
kermit bench --name tri-sip --report-json bench-runs/tri-sip.json \
  run triangle -i hash-trie -a hash-triejoin --ds-layout-hasher sip    --metrics iteration
kermit bench --name tri-fx --report-json bench-runs/tri-fx.json \
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

`--ds-build` works the same way for the build modes, as `structure=mode`
pairs: ColumnTrie's (`column-trie=bulk` by default, `column-trie=incremental`
for the build before issue #84), HashTrie's (`hash-trie=bulk` by default,
Algorithm 2; `hash-trie=incremental` for the per-tuple build (it requires
`--ds-config child-capacity=grow`, and was `serial` before #107);
`hash-trie=radix:<bits>` for the radix-partitioned build of issue #91,
`hash-trie=parallel:N` for the parallel build of issue #94 and
`hash-trie=presized:N` for the presized build, which requires
`--ds-config root-capacity=tuples`) and
TreeTrie's (`tree-trie=serial` by default, `tree-trie=parallel:N` for the
parallel build of issue #94; see "Scaling" below). For thesis figures,
compare modes **within one binary** — e.g.
`--ds-build column-trie=incremental` against the default.
kermit-lab back-fills `incremental` on pre-#84 ColumnTrie reports and on
HashTrie rows without the axis (and reads a HashTrie `serial` as
`incremental`), and `serial` on pre-#94 TreeTrie reports, but those
rows are for continuity only: reports carry no binary identity, so a
difference between an old row and a new one mixes the build mode with every
other change between the two binaries. A build mode only changes the build,
so `kl.ablation` (and `render-all`) draw the `ds_build_mode` axis for every
time phase: `build` and `end_to_end` move with it, and `iteration` may
(a different slot order, or allocations elsewhere in memory).

The seek strategy of the sorted tries works the same way. Compare
strategies **within one binary**, and give each run a distinct `--name`,
because Criterion group names do not encode axes:

```sh
for seek in linear binary galloping; do
  kermit bench --name seek-$seek --report-json bench-runs/seek-$seek.json \
    run watdiv-stress-100-test-1 -q q0236 -i all -a leapfrog-triejoin \
    --ds-layout-seek $seek --metrics iteration
done
```

`kl.ablation(df, axis="ds_layout_seek")` draws only for `iteration` and
`end_to_end`: every strategy builds the same trie.

The default seek is `galloping` since 2026-10-05; it was `binary` before.
Every report records the strategy that ran in `ds_layout_seek`, so rows from
either side of the switch stay labelled, but compare strategies within one
binary, never across builds (see the precision bound below).

**Codegen precision bound.** Measurement 1 for #80
(`kermit-bench-runs/seek-ab-2026-10-04/`, kept outside the repository;
`analysis.txt`) compared release builds of 0056fe1 and f8f7494 under the
then-default seek, `binary`. The rebuild alone (no algorithm changed
under the default) moved TreeTrie by −2.8% (geomean new/base 0.972) and
ColumnTrie by +2.1% (1.021), while HashTrie, whose code was untouched, stayed
flat (0.999). That is consistent with code-layout effects of the extra
monomorphised strategies, and it moved the TreeTrie/ColumnTrie ratio ~5%. A
within-binary difference between the two sorted tries smaller than that is
not conclusive, and cross-binary comparisons stay forbidden, since reports
carry no binary identity.

Give every run its own `--name`: Criterion group names do not encode
optimisation axes, so two runs that differ only in a `--ds-*` flag write into
the same `target/criterion/` directory, and the later overwrites the earlier's
samples.

```sh
kermit bench --name col-bulk --report-json bench-runs/col-bulk.json \
  ds -r data.parquet -i column-trie -m build
kermit bench --name col-incr --report-json bench-runs/col-incr.json \
  ds -r data.parquet -i column-trie -m build --ds-build column-trie=incremental
```

The full optimization model (Layout / Config / BuildMode, how to add one, and
the bench-axis namespace) is in
[`docs/specs/optimization-standard.md`](docs/specs/optimization-standard.md); a runnable
walkthrough is in
[`python/kermit-lab/notebooks/06_ablation.ipynb`](python/kermit-lab/notebooks/06_ablation.ipynb).

### Scaling: measuring a parallel build

`--ds-build tree-trie=parallel:N` builds a `TreeTrie` on N threads, and
`hash-trie=parallel:N` a `HashTrie` (#94). A TreeTrie's trie is identical
to its `serial` build's, so only `build` and `end_to_end` can move.
HashTrie's `parallel:N` holds `bulk`'s contents and root over a buffer of
its own, since it copies its rows into partition order (#101).
HashTrie's presized build is `--ds-build hash-trie=presized:N`, which
requires `--ds-config root-capacity=tuples`; it copies its rows the same
way and builds an equivalent, not identical, trie, so `iteration` is
measured for both HashTrie builds, and the presized build's speedup
baseline is `bulk` under the same `root-capacity=tuples` config.
Run one arm per thread count within one binary, each with its own
`--name`, then plot the speedup over the single-threaded build
(`kl.speedup_table` picks TreeTrie `serial`, HashTrie `bulk`):

```bash
for mode in serial parallel:1 parallel:2 parallel:4 parallel:8 parallel:16; do
  name="tt-${mode/:/-}"
  kermit bench --name "$name" --report-json "bench-runs/$name.json" \
    ds -r data.parquet -i tree-trie -m build --ds-build "tree-trie=$mode"
done
uv --directory python/kermit-lab run kermit-lab speedup "$PWD"/bench-runs/tt-*.json \
  --criterion-root "$PWD"/target/criterion --out "$PWD"/speedup.pdf
```

`kl.speedup_table(df)` gives the numbers behind the plot (`kermit-lab speedup`
prints the same table): speedup and efficiency per build mode and thread count, with CIs, and
the Karp–Flatt serial fraction.
A flat Karp–Flatt fraction means a fixed sequential share (the assemble
step) limits the build; a rising one means a cost that grows with N: thread
start-up and the merge of per-morsel buckets (both by design), then allocator
contention, memory bandwidth and imbalance. Replicates (one report per run,
each with its own `--name`) are pooled per arm and give bootstrap CIs once
each arm has two.
`parallel:1` against `serial` is the cost of partitioning net of one
saving (P smaller sorts take about n·log₂P fewer comparisons than one big
one), so `parallel:1` can come out ahead. The same saving can make a speedup
superlinear (above N), which makes the Karp–Flatt fraction negative.
For HashTrie, `parallel:1` against `bulk` is the cost of partitioning,
the copy included, with no sort saving: above 1 on grouped inputs,
possibly below on shuffled ones (#101); `radix:2` is the same
partitioning, run serially. `kl.speedup_table(df, baseline="radix:K")`
measures the threads alone. Expect the Karp–Flatt fraction to be highest
where most first-attribute values are distinct, and for unary relations,
because the root merge runs on one thread.
`N = 16` on an 8-core host measures SMT, not more cores. The protocol behind
reported numbers is in
[`docs/specs/2026-10-05-parallel-build-design.md`](docs/specs/2026-10-05-parallel-build-design.md).

## Comparing against external engines

kermit can run DuckDB and Nemo in process on the very queries its own cells run, measured by the same
measure layer: `--engine duckdb,nemo` adds engine cells beside the `-i`/`-a` cells of `bench run`,
`bench probe` and `bench join`. The engines are cargo features that are off by default, so the binary
must be built with them (in the dev shell, `--features engine-duckdb,engine-nemo`):

```sh
cargo run -p kermit --features engine-duckdb,engine-nemo -- bench run triangle -i all -a all \
  --engine duckdb,nemo --metrics build iteration space materialize end-to-end --verify
```

This is the thesis's RQ1.4: a **bounded sanity baseline** against an established peer engine, with
explicit threats to validity, not a head-to-head claim. An engine reads the same `Prepared` query, so
all of them answer the full conjunctive query, and on duplicate-free inputs every count equals the
benchmark's `expected` (`--verify` checks it for each cell).

**The phases differ, so only some pairs compare.** Where an engine does its index work decides which
phase is the same work as kermit's:

| Engine | Compare with kermit on | Why | Not offered |
| --- | --- | --- | --- |
| DuckDB (`duckdb-t{N}`) | `end_to_end` | its hash tables are built inside its `iteration`, while kermit's index work is in `build`, so only the sum is the same work | `space` (it keeps no index) |
| Nemo (`nemo`) | `build`, `materialize`, `space` | it builds sorted tries as `ColumnTrie` does and stores its result | `iteration`, `end_to_end` (its streaming join is private upstream); stderr says so |

`kl.engines()` draws exactly these pairs and no others, and per pair only the kermit rows of the
(benchmark, query) the pair's engine ran. It raises `InsufficientAxesError` when one cell and query
hold more than one row (two optimisers, several HashTrie Layouts, two benchmarks sharing a query
name), so filter the frame first. kermit-lab's six structure figures
(`scaling`, `bar_time`, `bar_space`, `tradeoff`, `dist` and `bar_queries`) draw kermit rows only,
because a generic figure would put an engine beside kermit on a metric that is not a like-for-like
pair; `kl.plot` draws engines too for a reader who picks the metric knowingly: with no `colour` and
a frame holding non-kermit rows it colours by the cell label (`duckdb-t1`, `nemo`,
`data_structure/algorithm` for kermit), and an explicit `colour` wins. `speedup_table` excludes
engine rows. Read the fairness caveats before quoting a
ratio: [`docs/engines/duckdb.md`](docs/engines/duckdb.md) and
[`docs/engines/nemo.md`](docs/engines/nemo.md) (DuckDB plans its own join order, Nemo runs kermit's
plan, and engine rows carry kermit's `plan` axis either way; DuckDB counts duplicate rows and Nemo does
not; on a query with an all-constant atom whose count is 0 an engine cell skips its `build`, which
kermit's cells do not, so that `build` is not like-for-like).

**`materialize` (opt-in, `--metrics materialize`; probes: `--materialize`).** The join, collecting
every full assignment of the executed query, sorted and counted distinct, on kermit's cells and
DuckDB; Nemo stores its result trie and counts it. It is not `iteration`: kermit keeps streaming for
that, because storing results adds a cost roughly equal for every structure and a result such as
SNAP's `path3` does not fit in memory. A query whose `materialize` would exceed
`--materialize-max-bytes` (default 4 GiB) is skipped with a stderr note and a metadata line. Nemo
does not stream, so its guard reads `expected` alone; over the limit nothing materialises, and a run
asked only for `--verify` skips verification for that cell with the same note.

**Threads and variance.** DuckDB runs one thread (`--engine-threads N` for more; every report records
`engine_threads`), the parallelism of kermit's joins and serial builds. Probe numbers are single cold
runs: compare them with other single runs, not with Criterion estimates. `--engine-threads` applies to DuckDB only and is refused without it. Every report records the
engine's version (`engine_version`) and the allocator; do not mix reports across an engine pin move.

## Statistical tooling

When a bar chart isn't enough to claim a difference, `kermit_lab.analysis`
provides five helpers:

| Helper | Signature (key args) | Use it to |
| --- | --- | --- |
| `summary` | `summary(df, *, rows, cols, value="mean_ns")` | Pivot many runs into a readable 2-D table (e.g. DS × size). |
| `compare` | `compare(df, *, baseline, target, group_by="data_structure")` | Pair every baseline row with its target and compute speedup per workload. |
| `speedup_table` | `speedup_table(df, *, phase="build", baseline=None)` | Speedup, efficiency and Karp–Flatt fraction of every `parallel:N` / `presized:N` build over each structure's default build (TreeTrie `serial`, HashTrie `bulk`), with CIs ("Scaling" above). |
| `bootstrap_ratio_ci` | `bootstrap_ratio_ci(a, b, *, ci=0.95, rng=…)` | Percentile-bootstrap CI for `mean(a)/mean(b)` from per-iter samples — "is this speedup real?" |
| `mannwhitney_u` | `mannwhitney_u(a, b)` → `(U, p)` | Non-parametric test that two sample distributions differ; pair with a violin plot. |

`summary`/`compare`/`speedup_table` consume the summary frame from `kl.load`;
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
| External engines (DuckDB, Nemo): phases, fairness, version pins | [`docs/engines/duckdb.md`](docs/engines/duckdb.md), [`docs/engines/nemo.md`](docs/engines/nemo.md) |
| System architecture & data flow | [`ARCHITECTURE.md`](ARCHITECTURE.md) |
