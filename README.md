# Kermit &emsp; [![Build Status]][actions] [![dependency status](https://deps.rs/repo/github/aidan-bailey/kermit/status.svg)](https://deps.rs/repo/github/aidan-bailey/kermit) [![Latest Version]][crates.io]

[Build Status]: https://img.shields.io/github/actions/workflow/status/aidan-bailey/kermit/build.yml?branch=master
[actions]: https://github.com/aidan-bailey/kermit/actions?query=branch%3Amaster
[Latest Version]: https://img.shields.io/crates/v/kermit.svg
[crates.io]: https://crates.io/crates/kermit

*Kermit* is a library containing data structures, iterators and algorithms related to [relational algebra](https://en.wikipedia.org/wiki/Relational_algebra), primarily for the purpose of research and benchmarking. It is currently in early stages of development and as such all builds and releases should be considered unstable.

It is being written primarily to provide a platform for my Masters thesis.
The scope of which (preliminarily) encompassing benchmarking the [Leapfrog Triejoin algorithm](https://arxiv.org/abs/1210.0481) over a variety of data structures.
I intend to design Kermit in an easily-extensible way, allowing for the possibility of benchmarking other algorithms and datastructures in the future.

Rust was chosen as the project language for two main reasons:
1. The [Knowledge-Based Systems group](https://iccl.inf.tu-dresden.de/web/Wissensbasierte_Systeme/en) at [TU Dresden](https://tu-dresden.de/) is developing a new Rust-based rule engine [Nemo](https://github.com/knowsys/nemo), which I'm hoping the knowledge and implementions developed during this Masters will prove useful for. I strongly recommend checking Nemo out. Not only is it a very promising project, it is one of most beautiful, pedantically managed repositories I've come across.
2. I wanted an excuse to write Rust with actual purpose.

My objective is to write entirely safe, stable, and hopefully idiomatic Rust the whole way through. I am very interested in how much one can maintain readibility (and sanity) while striving to achieve this.

## Usage

Given a relation stored as a CSV file (`edge.csv`):

```csv
src,dst
1,2
2,3
3,4
1,3
```

And a Datalog query file (`query.dl`):

```prolog
path(X, Y, Z) :- edge(X, Y), edge(Y, Z).
```

Run a join with the `kermit` CLI:

```sh
kermit join \
  --relations edge.csv \
  --query query.dl \
  --algorithm leapfrog-triejoin \
  --indexstructure tree-trie
```

Output (CSV to stdout):

```
1,2,3
1,3,4
2,3,4
```

Use `--output results.csv` to write to a file instead. Multiple relation files can be provided by repeating the `--relations` flag. `kermit join` supports the two sorted-trie structures `tree-trie` and `column-trie` (both paired with `leapfrog-triejoin`); the third structure, `hash-trie`, is exercised via `bench run -i hash-trie -a hash-triejoin` (see [Benchmarking](#benchmarking)).

An optional `--optimiser <lexicographic|cardinality>` flag picks the query optimiser that plans the join's variable ordering (default: `lexicographic`, which reproduces the historical ordering; `cardinality` binds variables from the smallest relations first). See [`docs/optimisers/`](docs/optimisers/).

## Benchmarking

All benchmarking is driven through the CLI. Each `bench` subcommand wraps Criterion and writes its raw measurement JSON to `target/criterion/`.

### Named benchmarks (`bench run`)

Run benchmarks declared in `benchmarks/*.yml`. Each YAML defines one or more named queries and the relations they need. Some benchmarks are *declarative generators* (e.g. `watdiv-basic`, `lubm-reference`) — instead of pointing at downloadable files they declare a `generator:` block and `bench run` materialises the data on first invocation.

```sh
kermit bench run triangle \
  --indexstructure tree-trie \
  --algorithm leapfrog-triejoin

# Sweep every benchmark over every compatible (index, algorithm) cell.
# Only three pairings are valid — (tree-trie|column-trie, leapfrog-triejoin)
# and (hash-trie, hash-triejoin) — and `all` on either side skips the
# incompatible ones, announcing each skip on stderr.
kermit bench run --all -i all -a all
```

Useful flags:
- `--query <NAME>` — run a single named query from the benchmark (default: all queries).
- `--all` — run every benchmark in `benchmarks/`.
- `-i <ds>` / `-a <algo>` — pick the index structure / join algorithm. Only the three compatible pairings above are supported; `all` on either side expands to the valid cells and skips the rest, while naming a single incompatible pair is a usage error.
- `--metrics insertion iteration space` — pick which metrics to measure (default: all three). The opt-in `end-to-end` metric times one database build plus K query executions per Criterion sample (`T = build + K × query`); it is never in the default set.
- `--queries-per-build <K>` — K for the `end-to-end` metric (default 1); recorded in the report's `queries_per_build` axis. Ignored by other metrics.
- `--optimiser <lexicographic|cardinality>` — pick the query optimiser planning each join's variable ordering (default: `lexicographic`). The choice is recorded in the JSON report's `optimiser` axis, making optimiser comparisons a third benchmark dimension alongside `-i`/`-a`.
- `--force` — regenerate a declarative-generator benchmark when its cached `meta.json` no longer matches the YAML's `spec_hash` (otherwise drift is a hard error).

Available benchmarks include `triangle`, the `oxford-uniform-s{1..6}` / `oxford-zipf-s{1..6}` Oxford DSI suites, and the `watdiv-stress-{100,1000}-{warmup,test-1..5}` WatDiv suites. Run `kermit bench list` for the full set.

### Generate fresh benchmarks

For ad-hoc data outside the committed YAMLs, `bench gen` drives the vendored RDF generators directly:

```sh
kermit bench gen watdiv --scale 10 --tag dev   # → watdiv-stress-10-dev
kermit bench gen lubm   --scale 1  --tag dev   # → lubm-1-dev (JDK 8 required)
```

Both write into `~/.cache/kermit/benchmarks/<name>/` and become discoverable to `bench run/list`. WatDiv uses the committed vendored binary at `kermit-rdf/vendor/watdiv/bin/Release/watdiv`; LUBM requires JDK 8 on PATH for the vendored jar. See [`docs/benchmarks/WATDIV.md`](docs/benchmarks/WATDIV.md), [`docs/benchmarks/LUBM.md`](docs/benchmarks/LUBM.md), and [`USAGE.md`](USAGE.md) for full flag listings.

### Manage benchmark cache

```sh
kermit bench list                # list all benchmarks (and which are cached)
kermit bench fetch [<NAME>]      # download a benchmark's data files (default: all)
kermit bench clean [<NAME>]      # remove cached data files (default: all)
```

Data is cached in `~/.cache/kermit/benchmarks/` on Linux.

### Data structure benchmark (`bench ds`)

Benchmark a specific data structure against a single relation file:

```sh
kermit bench ds \
  --relation data.csv \
  --indexstructure tree-trie \
  --metrics insertion iteration space
```

`--metrics` defaults to `insertion iteration space`; the opt-in `end-to-end` metric (with `--queries-per-build <K>`) times one build plus K full-trie iterations per Criterion sample.

### Join benchmark (`bench join`)

Benchmark join execution (query only, against a pre-built database) on data files:

```sh
kermit bench join \
  --relations edge.csv \
  --query query.dl \
  --algorithm leapfrog-triejoin \
  --indexstructure tree-trie
```

Supported index structures (for `bench join`): `tree-trie`, `column-trie` — both via `leapfrog-triejoin`. (`hash-trie` is only available through `bench run`/`bench ds`, paired with `hash-triejoin`.) Supported metrics (on `bench ds`/`bench run`): `insertion`, `iteration`, `space`, plus the opt-in `end-to-end`.

### JSON reports for tooling

Every `bench` invocation writes a machine-readable JSON summary alongside the human-readable stderr output and Criterion's per-function artefacts. The default path is `bench-runs/<kind>-<unix-millis>.json` (the directory is auto-created and gitignored at the workspace root). Pass `--report-json <PATH>` to override.

```sh
kermit bench run triangle -i tree-trie -a leapfrog-triejoin
# → writes bench-runs/run-1714828215123.json (timestamp varies)

kermit bench --report-json /tmp/triangle.json \
  run triangle -i tree-trie -a leapfrog-triejoin
# → writes /tmp/triangle.json
```

The schema (currently v2) is documented in `docs/specs/bench-report-schema.md`.

## Analysis and plotting

Criterion's auto-generated SVG/HTML output is disabled. Benchmark results are explored through [`python/kermit-lab/`](python/kermit-lab/), a [uv](https://docs.astral.sh/uv/)-managed notebook-first analysis library that loads `--report-json` output plus Criterion's per-function JSON into pandas DataFrames. Seven plot shapes return inline `matplotlib` Figures; pivot/comparison/stats helpers ship alongside. The CLI is preserved as a thin wrapper. Start with [`python/kermit-lab/notebooks/00_full_timeline.ipynb`](python/kermit-lab/notebooks/00_full_timeline.ipynb) for an end-to-end walkthrough that generates its own data, runs the bench sweep, and produces inline plots.

```sh
# one-time install
cd python/kermit-lab && uv sync

# notebook workflow (preferred)
uv run --with jupyter jupyter lab notebooks/

# CLI shortcut for headless renders
uv run kermit-lab render-all ../../bench-runs/*.json --out-dir ../../plots/
```

See [`python/kermit-lab/README.md`](python/kermit-lab/README.md) for the public Python API (`load`, `summary`, `compare`, `bootstrap_ratio_ci`, `mannwhitney_u`) and CLI subcommands (`plot`, `scaling`, `bar-time`, `bar-space`, `tradeoff`, `dist`, `bar-queries`, `ablation`, `render-all`).

## Contributing

Thanks for taking an interest! Perhaps after I've finished my thesis.

## License

This repository, as is customary with Rust projects, is duel-licensed under the [MIT](https://github.com/aidan-bailey/kermit/blob/master/LICENSE-MIT.txt) and [Apache-V2](https://github.com/aidan-bailey/kermit/blob/master/LICENSE-APACHE.txt) licenses.

