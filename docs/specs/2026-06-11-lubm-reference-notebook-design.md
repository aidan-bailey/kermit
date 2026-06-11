# LUBM Reference Comparison Notebook — Design

**Date:** 2026-06-11
**Status:** Approved (brainstorming session)
**Artifacts:** `benchmarks/lubm-reference.yml`, `python/kermit-lab/notebooks/07_lubm_reference_comparison.ipynb`

## Goal

A supervisor-ready reference benchmark, built as a Jupyter notebook that runs
every step live — data generation, correctness verification, benchmark sweep,
statistical analysis — and compares all three index structures. The notebook
is both a demonstration artifact (walk a supervisor through it) and a
reproducible measurement procedure (rerun it for thesis numbers).

## Decisions

These were made interactively during brainstorming:

| Decision | Choice | Rationale |
|---|---|---|
| Workload | LUBM(1, 0), all 14 queries | Canonical academic benchmark with published reference cardinalities (paper Table 3); generation needs only JDK 8, which the nix flake provides. WatDiv rejected: requires the locally-built, gitignored `watdiv` binary, weakening reproducibility. Static benchmarks (`triangle`, `oxford-*`) rejected: their ZivaHub URLs are placeholders. |
| Analysis depth | Full suite | Six figures + two tables (see Analysis section). |
| Execution model | Live end-to-end | Notebook cells invoke `cargo run --release -- bench …` via subprocess, streaming output (the `run_phase` pattern proven in `00_full_timeline.ipynb`). |
| Benchmark identity | Committed declarative YAML | `benchmarks/lubm-reference.yml` with a `generator:` block makes the reference benchmark a named, durable artifact — `bench run lubm-reference` works for anyone, independent of the notebook. |

## The comparison is configuration-vs-configuration

`HashTrie` implements `HashTrieIterable`, not `TrieIterable`, so it cannot run
LeapfrogTriejoin; `(HashTrie, HashTriejoin)` is the only valid pairing
(`is_compatible`, `kermit/src/main.rs:134`). The honest framing — stated
explicitly in the notebook's narrative — is three *system configurations*:

1. TreeTrie + LeapfrogTriejoin
2. ColumnTrie + LeapfrogTriejoin
3. HashTrie + HashTriejoin

The notebook enumerates these three explicitly — one `bench run` per
configuration. (`-i all -a all` cannot be used: the CLI's cross-product
expansion does not skip incompatible pairs; it panics at
`kermit/src/db.rs:313` on `(TreeTrie, HashTriejoin)`. The `is_compatible`
check at `kermit/src/main.rs:~134` only validates the top-level selector
pair, not the expansion. Discovered during implementation review; the CLI
fix is filed as a separate change per scope discipline.)

## Artifact 1: `benchmarks/lubm-reference.yml`

```yaml
name: lubm-reference
description: "Reference comparison: LUBM(1,0), all 14 queries, all data structures"
generator:
  kind: lubm
  scale: 1
```

Omitting `queries:` selects all 14 LUBM queries. Scale is pinned to 1 because
the published reference cardinalities only hold at LUBM(1, 0) — the pipeline
only emits `expected/q*.csv` at scale 1 (`kermit/src/main.rs:1745-1749`).
Materialisation happens automatically on first `bench run lubm-reference` via
`kermit/src/materialize.rs::materialize`, with spec-hash caching; reruns
short-circuit. Spec drift requires explicit `--force` (never auto-regenerates).

## Artifact 2: the notebook

Location: `python/kermit-lab/notebooks/07_lubm_reference_comparison.ipynb`
(continues the existing `00`–`06` numbering). Markdown narration before every
code cell explains *what* the step does and *why* it is trustworthy — the
notebook doubles as methodology documentation for the thesis.

### Phase 0 — Preflight

- Resolve the repo root by walking up to `Cargo.toml`; `os.chdir` there
  (pattern from `00_full_timeline.ipynb` cell 1).
- Assert `cargo` and `java` are on PATH, failing loudly with a pointer to
  `nix develop`.
- Define the `run_phase` subprocess helper (streamed output, rolling tail
  buffer, timing) — copied from `00_full_timeline.ipynb`.
- A `PROFILE` knob at the top:
  - `"quick"`: `--sample-size 10 --measurement-time 1 --warm-up-time 1`
    (live-demo runtime, ~10–20 min cold including build and generation)
  - `"full"`: `--sample-size 50 --measurement-time 3 --warm-up-time 1`
    (thesis-grade numbers; markdown states the expected runtime cost)

### Phase 1 — Verify correctness before timing anything

- Run `cargo test -p kermit --test lubm_cardinalities --release` via
  `run_phase`. This test generates LUBM(1, 0) end-to-end into a tempdir and
  asserts all 14 query result counts match the published cardinalities — the
  load-bearing oracle for the entailment rule set and the join engine
  together. It is deliberately independent of the benchmark cache: it
  validates the pipeline, not a cached snapshot.
- Markdown: timing numbers from an engine returning wrong results are
  worthless; this phase is why the figures below can be trusted. Note it may
  be skipped on reruns.

### Phase 2 — Benchmark sweep (materialises on first run)

Materialisation has no standalone CLI trigger — `bench fetch` only downloads
static relation URLs (`ensure_cached`); only the `bench run` path calls
`materialize::materialize` (`kermit/src/main.rs:1541`). So the first sweep
run materialises LUBM(1, 0) inline, and the notebook makes that visible
rather than pretending it is a separate command:

- `bench list` cell before the sweep — shows `lubm-reference  not generated`.
- The sweep invocation (below); on a clean cache its first minutes of
  streamed output are the generation pipeline: vendored jar → N-Triples →
  Univ-Bench TBox entailment → per-predicate partition → parquet +
  dictionary → `benchmark.yml` + `expected/*.csv` + `meta.json`. Markdown
  narrates these stages and the provenance trail (jar SHA-256 and spec hash
  in `meta.json`). Cache location:
  `~/.cache/kermit/benchmarks/lubm-reference/`.
- `bench list` cell after — status now `cached`; reruns skip generation.
- Render `~/.cache/kermit/benchmarks/lubm-reference/expected/q*.csv` as a
  pandas table of the 14 reference cardinalities (cross-referencing Phase 1).

One invocation per valid configuration (metrics listed explicitly even
though they match the default, because this is a reference document):

```
for (ds, algo) in [(tree-trie, leapfrog-triejoin), (column-trie, leapfrog-triejoin),
                   (hash-trie, hash-triejoin)]:
  cargo run --release -- bench \
    --sample-size <profile> --measurement-time <profile> --warm-up-time <profile> \
    --report-json bench-runs/lubm-reference-sweep-<profile>-<ds>-<algo>.json \
    run lubm-reference -i <ds> -a <algo> \
    --metrics insertion iteration space
```

The per-config reports are loaded together via a glob (`kl.load` accepts
glob patterns); each config's time and space rows share a `source_path`, so
`kl.tradeoff`'s merge still works.

This is a deliberate simplification of the approved 7-phase outline: the
original separate time/space phases merge into one sweep per configuration
because `--metrics` accepts all three values in a single run
(`num_args = 1..`, `kermit/src/main.rs:308-316`). Markdown
explains the three metrics: insertion (build time), iteration (query
execution time), space (`heap_size_bytes()` via the custom Criterion
`SpaceMeasurement`).

### Phase 3 — Load into pandas

```python
import kermit_lab as kl
kl.apply_style()
df = kl.load("bench-runs/lubm-reference-sweep.json", criterion_root="target/criterion")
```

Show the schema, row count, and a pivot confirming the three configurations ×
14 queries × metrics/phases are all present (an explicit completeness check —
a missing combination should be visible here, not discovered in a figure).

### Phase 4 — Analysis

Six figures (each a `matplotlib.figure.Figure` from `kermit_lab`) and two
tables, each preceded by markdown saying what question it answers:

| # | Artifact | API | Question |
|---|---|---|---|
| F1 | Per-query iteration-time bars, 14 queries × 3 configs, log-y | `kl.plot(kind="bar", x="query", colour="data_structure", logy=True)` or `kl.bar_queries` | Which configuration answers each query fastest? |
| F2 | Insertion-time bars per structure | `kl.plot(kind="bar", x="data_structure", phase="insertion")` | What does building each index cost? |
| F3 | Heap-space bars | `kl.bar_space(df)` | What does each index cost in memory? |
| F4 | Space-vs-time tradeoff scatter | `kl.tradeoff(df)` | The thesis figure: is any structure Pareto-dominant? |
| F5 | Violin distributions for headline query q9 | `kl.dist(df, samples=kl.load_samples(...))` | Are the q9 timing differences real or noise? |
| T1 | Ratio table, TreeTrie+LFTJ as baseline, with bootstrap CIs | `kl.compare(...)` + `kl.bootstrap_ratio_ci(...)` | How much faster/slower, with uncertainty? |
| T2 | Mann-Whitney U on q9 per config pair | `kl.mannwhitney_u(...)` | Is the headline difference statistically significant? |
| F6 | Closing summary pivot heat/bars | `kl.summary(df, rows="query", cols="data_structure")` | One-glance overview for the meeting. |

q9 is the headline query: the most join-heavy of the 14 (largest body atom
count), where structural differences between the tries should be most
visible. If implementation finds q2 discriminates better, swapping is fine —
the spec requirement is *one* deep-dive query with distribution + significance
treatment, named and justified in the markdown.

Closing markdown section, "Reading the results": how to interpret the
figures, explicit caveats (single scale, single machine, dictionary-encoded
usize keys, config-vs-config framing), and pointers to
`docs/benchmarks/LUBM.md` and the per-structure docs in
`docs/data-structures/`.

## Out of scope

- **Scaling over dataset size.** Reference cardinalities only hold at scale 1,
  and the `oxford-*` scaling series has placeholder download URLs. A scaling
  notebook is a natural follow-up once those URLs are live.
- **WatDiv.** Gitignored vendored binary → not "runs anywhere".
- **Optimization-axis ablations** (hasher choice, etc.) — `06_ablation.ipynb`
  territory; this notebook compares the three structures at their defaults.
- **No changes to Rust code, existing notebooks, or existing benchmarks.**
  The only repo deltas are the new YAML and the new notebook.

## Risks and mitigations

- **Runtime surprises during a live demo.** Mitigated by the `PROFILE` knob,
  spec-hash caching (generation runs once), and `run_phase`'s streamed output
  (no silent multi-minute cells).
- **JDK presence.** Preflight asserts `java` on PATH with a clear message;
  the flake provides JDK 8.
- **Stale cache after spec edits.** `materialize` errors with `SpecDrift`
  rather than silently reusing; the notebook's markdown documents
  `bench run --force lubm-reference` as the recovery path.
- **Criterion sample availability for F5.** `kl.load_samples` needs
  `sample.json` under `target/criterion/`; written by default — no extra
  flags required.

## Acceptance criteria

1. The three per-configuration `bench run lubm-reference` invocations
   materialise on first run and produce criterion output for exactly
   3 configurations × 14 queries.
2. The notebook executes top-to-bottom under `nix develop` on a clean cache
   (quick profile) without manual intervention.
3. The cardinality verification phase passes (all 14 match Table 3).
4. All six figures and both tables render with all three configurations
   present.
5. Rerunning the notebook skips generation (cache hit) and completes
   substantially faster.
6. `bench list` shows `lubm-reference` with correct generator status
   (`not generated` / `cached` / `stale`).
