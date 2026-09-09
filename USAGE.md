# Kermit Usage

Practical examples for the `kermit` CLI and the companion `kermit-lab` Python
tool. All examples run from the workspace root.

## Build

```sh
cargo build --release
alias kermit=./target/release/kermit
```

Or run via cargo: `cargo run --release -- <args>`.

## Joins

### Single-relation self-join

```sh
kermit join \
  --relations kermit/tests/fixtures/edge.csv \
  --query kermit/tests/fixtures/path_query.dl \
  --algorithm leapfrog-triejoin \
  --indexstructure tree-trie
```

`edge.csv` holds `(src, dst)` pairs; `path_query.dl` is

```prolog
path(X, Y, Z) :- edge(X, Y), edge(Y, Z).
```

Output (CSV to stdout, header on first line):

```
X,Y,Z
1,2,3
1,3,4
2,3,4
```

### Multiple relations

Repeat `--relations` for each input file:

```sh
kermit join \
  --relations kermit/tests/fixtures/first.csv \
  --relations kermit/tests/fixtures/second.csv \
  --query kermit/tests/fixtures/intersect_query.dl \
  --algorithm leapfrog-triejoin \
  --indexstructure column-trie
```

### Write to a file instead of stdout

```sh
kermit join … --output results.csv
```

### Pick the index structure

`--indexstructure` accepts `tree-trie` (pointer-based), `column-trie`
(column-oriented), or `hash-trie` (hash-based). The two sorted tries implement
the same `Relation` + `TrieIterable` traits, pair with `-a leapfrog-triejoin`,
and are interchangeable from the CLI's perspective; benchmark to pick one.
`hash-trie` lives in a different trait family (`HashTrieIterable`) and pairs
only with `-a hash-triejoin`. Every command, including the one-shot
`kermit join` / `bench join`, accepts all three valid pairs; an incompatible
pair is rejected as a usage error.

### Pick the query optimiser

`--optimiser` selects the policy that plans the join's variable ordering
(the global attribute order the algorithm descends). It accepts
`lexicographic` (the default — reproduces the historical hardcoded
ordering) or `cardinality` (binds variables from the smallest relations
first). Long-only: `-o` belongs to `--output`.

```sh
kermit join … --optimiser cardinality
```

Every valid ordering yields the same result tuples; the optimiser affects
only execution order and therefore runtime. See
[`docs/optimisers/`](docs/optimisers/) for the per-policy docs.

## Benchmarks

Every `bench` subcommand wraps Criterion. Each invocation writes:

- Per-function Criterion artefacts under `target/criterion/<group>/<dir>/{base,new}/`
  (`benchmark.json`, `estimates.json`, `sample.json`, `tukey.json`).
- A machine-readable JSON summary at `bench-runs/<kind>-<unix-millis>.json`
  (override with `--report-json <PATH>`). See
  `docs/specs/bench-report-schema.md`. It is rewritten after every cell, so a
  `bench run` sweep that fails or is interrupted partway still leaves a valid
  report holding every cell that finished first; the error names the file.

Parent-level options apply to all subcommands:

```sh
kermit bench \
  --name my-group \
  --sample-size 50 \
  --measurement-time 3 \
  --warm-up-time 2 \
  --report-json bench-runs/my-run.json \
  <subcommand> ...
```

`-n`/`--name` controls the Criterion group name. Semantics differ by
subcommand: for `bench ds` it is the full group name (default `ds`); for
`bench join` and `bench run` it is a *prefix* on the auto-generated
`{benchmark}/{query}/{ds}/{algo}` identity (defaults `join` / `run`).

### Benchmark a join (`bench join`)

Benchmark a Datalog query over ad-hoc relation files against a chosen DS + algorithm:

```sh
kermit bench join \
  --relations kermit/tests/fixtures/edge.csv \
  --query kermit/tests/fixtures/path_query.dl \
  --algorithm leapfrog-triejoin \
  --indexstructure tree-trie
```

Pass `-o`/`--output <PATH>` to also write the join's tuples to a CSV file
(useful for verifying correctness alongside the benchmark). `--optimiser
<lexicographic|cardinality>` picks the query optimiser (default:
`lexicographic`); the choice lands in the JSON report's `optimiser` axis.

`bench join` runs through the same runner as `bench run`: it records the
`insertion`, `iteration` and `space/<relation>` metrics by default (`-m`
selects; `end-to-end` is opt-in, with `--queries-per-build K` setting its K,
default 1), accepts
`--ds-config` alongside the `--ds-layout-*` flags, and writes its Criterion
output under `{--name|join}/adhoc/{query-file-stem}/{ds}/{algo}`. The
report's `benchmark` axis is `adhoc` and `query` is the query file's stem.

### Benchmark a data structure (`bench ds`)

Insertion / iteration / heap-size over a single relation file:

```sh
kermit bench ds \
  --relation kermit/tests/fixtures/edge.csv \
  --indexstructure tree-trie \
  --metrics insertion iteration space
```

`--metrics` defaults to all three. To benchmark a single metric:

```sh
kermit bench ds -r data.csv -i column-trie -m space
```

The opt-in `end-to-end` metric (not in the default set) times one build plus
K full-trie iterations in a single Criterion body; K comes from
`--queries-per-build` (default 1) and is stamped into the report's
`queries_per_build` axis:

```sh
kermit bench ds -r data.csv -i tree-trie -m end-to-end --queries-per-build 4
```

### Run a named benchmark (`bench run`)

Each YAML in `benchmarks/` declares one or more named queries plus the
relations they need. Run one:

```sh
kermit bench run triangle \
  --indexstructure tree-trie \
  --algorithm leapfrog-triejoin
```

Run a single query inside a benchmark:

```sh
kermit bench run oxford-uniform-s3 \
  --query triangle \
  -i tree-trie -a leapfrog-triejoin
```

Run every benchmark:

```sh
kermit bench run --all -i tree-trie -a leapfrog-triejoin
```

`bench run` also accepts `--metrics` (same shape as `bench ds`); defaults to
all three. To benchmark only space:

```sh
kermit bench run triangle -i tree-trie -a leapfrog-triejoin -m space
```

The opt-in `end-to-end` metric times one database build **plus K query
executions** per Criterion sample (`T = build + K × query`, fresh build every
sample). K comes from `--queries-per-build` (default 1) and lands in the
report's `queries_per_build` axis — see `BENCHMARKING.md` § "What you can
measure" for when to use it and why it is not the sum of `insertion` and
`iteration`:

```sh
kermit bench run triangle -i tree-trie -a leapfrog-triejoin \
  -m end-to-end --queries-per-build 4
```

It also accepts `--optimiser <lexicographic|cardinality>` (default:
`lexicographic`), stamped into each report's `optimiser` axis — sweep it the
same way as the DS/algorithm axes to compare ordering policies:

```sh
kermit bench run triangle -i tree-trie -a leapfrog-triejoin --optimiser lexicographic
kermit bench run triangle -i tree-trie -a leapfrog-triejoin --optimiser cardinality
```

Pass `--verify` to check answers before timing them. For every cell, the
query is run once and its returned tuple count is compared with the
`expected` value declared in the benchmark YAML; a mismatch aborts the run
with `verification failed: ...` (reports already written for earlier cells
are kept), and a match stamps `verified: yes` into the stderr metadata and
`verified: true` into the report's `axes`. A query with no `expected` value
is still timed but noted on stderr as `not verified`. Without the flag
nothing is checked:

```sh
kermit bench run triangle -i all -a all -m iteration --verify
```

### Sweep all index structures and algorithms

`-i` and `-a` accept `all` as a value (in addition to the concrete
variants). `bench ds` honours `-i all` as a sweep over every index
structure (it has no algorithm axis). `bench run` takes the Cartesian
product of both axes, but see the caveat below — the product is *not*
filtered for compatible (index structure, algorithm) pairs. `bench join`
does *not* support `all` — it shares its argument struct with the one-shot
`kermit join` command.

```sh
# Sweep the two sorted-family DS variants on a single benchmark + algorithm.
kermit bench run triangle -i tree-trie -a leapfrog-triejoin
kermit bench run triangle -i column-trie -a leapfrog-triejoin
```

`-i all` expands to all three index structures (`column-trie`, `hash-trie`,
`tree-trie`) and `-a all` expands to both concrete algorithms (`hash-triejoin`,
`leapfrog-triejoin`); the selectors are wired up so adding a new `IndexStructure`
or `JoinAlgorithm` variant automatically joins the sweep. **However, the
cross-product currently does *not* filter incompatible pairs**, so the only
benchmark runs that are both valid and correctly labelled are the three matched
configurations:

```sh
kermit bench run triangle -i tree-trie -a leapfrog-triejoin
kermit bench run triangle -i column-trie -a leapfrog-triejoin
kermit bench run triangle -i hash-trie -a hash-triejoin
```

Avoid `-i all -a all` (it panics on the first incompatible pair, e.g.
`(column-trie, hash-triejoin)`) and `-i all -a leapfrog-triejoin` (it reaches
the `(hash-trie, leapfrog-triejoin)` arm, which silently runs hash-triejoin yet
labels the report's algorithm axis `LeapfrogTriejoin`).

### Declarative generator benchmarks

A benchmark YAML may declare a `generator:` block instead of `relations:`
/ `queries:`:

```yaml
# benchmarks/watdiv-100.yml
name: watdiv-100
generator:
  kind: watdiv
  scale: 100
```

On `bench run watdiv-100`, kermit hashes the spec, looks under
`~/.cache/kermit/benchmarks/watdiv-100/`, and either reuses the cached
data on a `spec_hash` match or invokes the underlying `kermit-rdf`
pipeline (the same one `bench gen watdiv|lubm` uses) on first run.

If the YAML's parameters change after a benchmark has been cached, the
next `bench run` aborts with a `SpecDrift` error rather than silently
spending minutes regenerating a multi-gigabyte dataset. Pass `--force`
to opt in:

```sh
kermit bench run watdiv-100 -i tree-trie -a leapfrog-triejoin --force
```

`bench list` distinguishes `not generated` / `cached` / `stale` for
generator benchmarks (vs. `cached` / `not cached` for static ones). See
[`benchmarks/README.md`](benchmarks/README.md) for the full schema.

### Manage cached data (`bench list` / `fetch` / `clean`)

```sh
kermit bench list                    # show all benchmarks; mark cached ones
kermit bench fetch oxford-uniform-s1 # download data files for one benchmark
kermit bench fetch                   # download every benchmark's data
kermit bench clean oxford-uniform-s1 # remove cached files for one benchmark
kermit bench clean                   # wipe all cached benchmark data
```

Cache lives at `~/.cache/kermit/benchmarks/` on Linux.

`bench fetch` also re-hashes every relation that declares a `sha256` in its
YAML (cached downloads and committed `path:` files alike) and reports
`Verified N relation(s).`, or `No integrity hashes declared.` when the
benchmark pins nothing. A download whose digest does not match is rejected
before anything is written to the cache. `bench run` never hashes relation files.

### Generate a fresh WatDiv benchmark (`bench gen watdiv`)

Drives the vendored `watdiv` binary to synthesize a new RDF dataset + stress
queries, dropped into the cache so subsequent `bench run` invocations pick it
up. The generated benchmark is named `watdiv-stress-<scale>-<tag>`; `--tag`
must contain a non-numeric character so it cannot collide with the committed
snapshot names under `benchmarks/`.

```sh
kermit bench gen watdiv --scale 10 --tag dev
# → ~/.cache/kermit/benchmarks/watdiv-stress-10-dev/
kermit bench run watdiv-stress-10-dev -i tree-trie -a leapfrog-triejoin
```

Tunables (all optional): `--max-query-size` (default 5),
`--query-count` per template (20), `--constants-per-query` (2),
`--allow-join-vertex` (false), `--watdiv-bin <PATH>` to override the
vendored binary, `--output-dir <PATH>` to write outside the default cache
(generated benchmarks placed there are NOT auto-discovered by
`bench list/fetch/run`), and `--no-bwrap` to skip the bwrap sandbox
(requires host `/usr/share/dict/words`).

On NixOS, run inside `nix develop` so the vendored binary's `libstdc++`
loader and `bubblewrap` are on PATH.

### Generate a fresh LUBM benchmark (`bench gen lubm`)

Drives the vendored `lubm-uba.jar` to synthesize an RDF dataset for a
given university scale, runs Univ-Bench TBox forward-chaining
entailment, and emits the 14 LUBM queries (paper Appendix A). Output
lands at `~/.cache/kermit/benchmarks/lubm-<scale>-<tag>/` where
subsequent `bench run` invocations pick it up. Pick a `--tag` value that
won't collide with any committed snapshot name.

```sh
kermit bench gen lubm --scale 1 --tag dev
# → ~/.cache/kermit/benchmarks/lubm-1-dev/
kermit bench run lubm-1-dev -i tree-trie -a leapfrog-triejoin
```

JDK 8 must be on PATH — the Nix dev shell provides `pkgs.jdk8`;
otherwise install `openjdk-8-jre` (or equivalent). The jar emits two
`<>` document-self triples per file that strict N-Triples parsers
reject; the pipeline filters them out at extraction time.

Tunables (all optional): `--seed N` (default 0, matches LUBM-UBA's
documented default), `--start-index N` (default 0), `--threads N`
(default 1, set higher to parallelise the jar at the cost of
reproducibility), `--lubm-jar <PATH>` to override the vendored jar
(also `KERMIT_LUBM_JAR`), `--ontology <URL>` to override the canonical
Univ-Bench TBox URL (only useful if you've mirrored the ontology), and
`--output-dir <PATH>` to write outside the default cache (generated
benchmarks placed there are NOT auto-discovered by
`bench list/fetch/run`).

LUBM(1, 0) reference cardinalities (paper Table 3) are written into the
generated `benchmark.yml` as each query's `expected` only for scale 1,
seed 0 and start index 0; at other settings the queries still run but
carry no `expected`, so `bench run --verify` reports them as not
verified.

## JSON reports

The default report path is `bench-runs/<kind>-<unix-millis>.json` resolved
relative to the invocation's CWD. The directory is auto-created and
gitignored.

```sh
kermit bench run triangle -i tree-trie -a leapfrog-triejoin
# → bench-runs/run-1714828215123.json
```

Override the path:

```sh
kermit bench --report-json /tmp/triangle.json \
  run triangle -i tree-trie -a leapfrog-triejoin
```

The shape is always a JSON array of `BenchReport` objects (one per query for
`bench run`, exactly one for `bench join` / `bench ds`). Schema is documented
in [`docs/specs/bench-report-schema.md`](docs/specs/bench-report-schema.md);
the source of truth is `kermit/src/bench_report.rs`.

## Analysis and plotting (`kermit-lab`)

The Rust CLI deliberately doesn't render plots — Criterion's auto-plots are
disabled. Benchmark exploration happens in `python/kermit-lab/`, a
notebook-first analysis library managed with [uv](https://docs.astral.sh/uv/)
that loads `--report-json` output and Criterion artefacts into pandas
DataFrames. `kl.load` surfaces every report axis as a column — including the
optimization axes `ds_layout_*` / `ds_config_*` / `ds_build_mode`, so any of
them can be plotted. A general `kl.plot(df, kind=…, x=…, colour=…, facet=…)`
engine binds any axis to any visual channel; the named shapes (`scaling`,
`bar_time`, `bar_space`, `tradeoff`, `dist`, `bar_queries`, `ablation`) are
presets over it, each returning an inline `matplotlib.figure.Figure`.
`summary` / `compare` / `bootstrap_ratio_ci` / `mannwhitney_u` provide pivots
and stats. The CLI subcommands listed below are thin wrappers over the same
Python API. For *which* comparison to run and how to read the results, see
[`BENCHMARKING.md`](BENCHMARKING.md).

### One-time setup

```sh
cd python/kermit-lab
uv sync --group test
```

This creates `.venv/` with all dependencies pinned to `uv.lock`. Run the CLI
via `uv run kermit-lab …` or activate the venv (`source .venv/bin/activate`)
to call `kermit-lab` directly.

### Render a single shape

```sh
uv run kermit-lab scaling     bench-runs/*.json --out plots/scaling.pdf
uv run kermit-lab bar-time    bench-runs/*.json --query triangle --out plots/bar-time.pdf
uv run kermit-lab bar-space   bench-runs/*.json --out plots/bar-space.pdf
uv run kermit-lab tradeoff    bench-runs/*.json --out plots/tradeoff.pdf
uv run kermit-lab dist        bench-runs/*.json --out plots/dist.pdf
uv run kermit-lab bar-queries bench-runs/*.json --ds TreeTrie --algo LeapfrogTriejoin --out plots/bar-queries.pdf
uv run kermit-lab ablation    bench-runs/*.json --axis ds_layout_hasher --out plots/ablation.pdf
```

The output format is determined by the `--out` suffix (`pdf`, `png`, `svg`,
or `pgf`).

### The general `plot` subcommand

The presets above are configurations of one engine. Drive it directly to bind
any axis — including an optimization axis — to any channel:

```sh
uv run kermit-lab plot bench-runs/*.json \
  --kind bar --y time \
  --x ds_config_singleton_pruning \
  --colour data_structure --facet query \
  --out plots/ablation-pruning.pdf
```

`--kind` is one of `bar`/`line`/`scatter`/`violin`; `--x`/`--colour`/`--style`/
`--facet` take column names (`tuples`, `data_structure`, `query`, or any
`ds_*`/`algo_*` optimization axis), `--y` is `time` or `space`, and repeatable
`--filter k=v` holds axes fixed. See [`BENCHMARKING.md`](BENCHMARKING.md) for
the ablation workflow.

### Render every applicable shape (`render-all`)

```sh
uv run kermit-lab render-all bench-runs/*.json --out-dir plots/
uv run kermit-lab render-all bench-runs/*.json --out-dir plots/ --format png
```

`render-all` does not take `--out` (it generates many files); pass
`--format {pdf,png,svg,pgf}` to pick the output format for every emitted
plot (default: `pdf`).

Shapes that lack the necessary axes (e.g. only one `tuples` value → no
`scaling.pdf`) are skipped with an info-level log.

### NixOS

The numpy / matplotlib wheels installed by uv expect a glibc-style runtime
loader. The Nix dev shell handles this — `nix develop` exports an
`LD_LIBRARY_PATH` covering `libstdc++.so.6` and `libz.so.1` so the `.venv/`
created by uv can load numpy's C extensions:

```sh
nix develop
cd python/kermit-lab
uv sync
uv run kermit-lab render-all ../../bench-runs/*.json --out-dir ../../plots/
```

If you're not using `nix develop`, set the env var manually before invoking uv:

```sh
export LD_LIBRARY_PATH=/run/current-system/sw/share/nix-ld/lib
```

## End-to-end: scaling plot from scratch

```sh
# 1. Build the CLI.
cargo build --release
alias kermit=./target/release/kermit

# 2. Fetch the Oxford uniform suite (six scale points).
for s in 1 2 3 4 5 6; do kermit bench fetch oxford-uniform-s$s; done

# 3. Run the triangle query at each scale, against the two sorted-family DS
#    implementations (tree-trie and column-trie); hash-trie is omitted because
#    it requires hash-triejoin rather than leapfrog-triejoin.
for s in 1 2 3 4 5 6; do
  for ds in tree-trie column-trie; do
    kermit bench run oxford-uniform-s$s \
      --query triangle \
      -i $ds -a leapfrog-triejoin \
      --report-json bench-runs/triangle-$ds-s$s.json
  done
done

# 4. Render the scaling plot. (Inside `nix develop` on NixOS — the dev
#    shell already exports the LD_LIBRARY_PATH that numpy needs.)
cd python/kermit-lab
uv sync
uv run kermit-lab scaling ../../bench-runs/triangle-*.json \
  --out ../../plots/triangle-scaling.pdf
```

The result is a log-log line plot with one line per `(data_structure,
algorithm)` over `tuples`, error bars, and the canonical Wong / Okabe-Ito
palette.
