# Kermit Usage

Practical examples for the `kermit` CLI and the companion `kermit-plot` Python
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

`--indexstructure` accepts `tree-trie` (pointer-based) or `column-trie`
(column-oriented). Both implement the same `Relation` + `TrieIterable` traits
and are interchangeable from the CLI's perspective; benchmark to pick one.

## Benchmarks

Every `bench` subcommand wraps Criterion. Each invocation writes:

- Per-function Criterion artefacts under `target/criterion/<group>/<dir>/{base,new}/`
  (`benchmark.json`, `estimates.json`, `sample.json`, `tukey.json`).
- A machine-readable JSON summary at `bench-runs/<kind>-<unix-millis>.json`
  (override with `--report-json <PATH>`). See
  `docs/specs/bench-report-schema.md`.

Parent-level options apply to all subcommands:

```sh
kermit bench \
  --sample-size 50 \
  --measurement-time 3 \
  --warm-up-time 2 \
  --report-json bench-runs/my-run.json \
  <subcommand> ...
```

### Benchmark a join (`bench join`)

End-to-end timing of a Datalog query against a chosen DS + algorithm:

```sh
kermit bench join \
  --relations kermit/tests/fixtures/edge.csv \
  --query kermit/tests/fixtures/path_query.dl \
  --algorithm leapfrog-triejoin \
  --indexstructure tree-trie
```

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

### Manage cached data (`bench list` / `fetch` / `clean`)

```sh
kermit bench list                    # show all benchmarks; mark cached ones
kermit bench fetch oxford-uniform-s1 # download data files for one benchmark
kermit bench fetch                   # download every benchmark's data
kermit bench clean oxford-uniform-s1 # remove cached files for one benchmark
kermit bench clean                   # wipe all cached benchmark data
```

Cache lives at `~/.cache/kermit/benchmarks/` on Linux.

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

## Plotting (`kermit-plot`)

The Rust CLI deliberately doesn't render plots — Criterion's auto-plots are
disabled. Thesis-quality figures come from the Python tool at
`scripts/kermit-plot/`, which consumes one or many `--report-json` outputs
and the Criterion artefacts they reference.

### One-time setup

```sh
cd scripts/kermit-plot
python -m venv venv && source venv/bin/activate
pip install -e '.[test]'
```

This registers a `kermit-plot` console script.

### Render a single shape

```sh
kermit-plot scaling     bench-runs/*.json --out plots/scaling.pdf
kermit-plot bar-time    bench-runs/*.json --query triangle --out plots/bar-time.pdf
kermit-plot bar-space   bench-runs/*.json --out plots/bar-space.pdf
kermit-plot tradeoff    bench-runs/*.json --out plots/tradeoff.pdf
kermit-plot dist        bench-runs/*.json --out plots/dist.pdf
kermit-plot bar-queries bench-runs/*.json --ds TreeTrie --algo LeapfrogTriejoin --out plots/bar-queries.pdf
```

`--format` overrides the file extension's default (`pdf`, `png`, `svg`, `pgf`).

### Render every applicable shape (`render-all`)

```sh
kermit-plot render-all bench-runs/*.json --out-dir plots/
```

Shapes that lack the necessary axes (e.g. only one `tuples` value → no
`scaling.pdf`) are skipped with an info-level log.

### NixOS

The pip-installed numpy / matplotlib wheels expect a glibc-style runtime
loader. The Nix dev shell handles this — `nix develop` exports an
`LD_LIBRARY_PATH` covering `libstdc++.so.6` and `libz.so.1` so any venv
activated inside the shell can load numpy's C extensions:

```sh
nix develop
source venv/bin/activate
kermit-plot render-all bench-runs/*.json --out-dir plots/
```

If you're not using `nix develop`, set the env var manually before activating
the venv:

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

# 3. Run the triangle query at each scale, against both DS implementations.
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
cd scripts/kermit-plot
source venv/bin/activate
kermit-plot scaling ../../bench-runs/triangle-*.json \
  --out ../../plots/triangle-scaling.pdf
```

The result is a log-log line plot with one line per `(data_structure,
algorithm)` over `tuples`, error bars, and the canonical Wong / Okabe-Ito
palette.
