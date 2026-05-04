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

Use `--output results.csv` to write to a file instead. Multiple relation files can be provided by repeating the `--relations` flag. Both `tree-trie` and `column-trie` index structures are supported.

## Benchmarking

All benchmarking is driven through the CLI. Each `bench` subcommand wraps Criterion and writes its raw measurement JSON to `target/criterion/`.

### Named benchmarks (`bench run`)

Run benchmarks declared in `benchmarks/*.yml`. Each YAML defines one or more named queries and the relations they need.

```sh
kermit bench run triangle \
  --indexstructure tree-trie \
  --algorithm leapfrog-triejoin
```

Useful flags:
- `--query <NAME>` — run a single named query from the benchmark (default: all queries).
- `--all` — run every benchmark in `benchmarks/`.
- `--metrics insertion iteration space` — pick which metrics to measure (default: all three).

Available benchmarks include `triangle`, the `oxford-uniform-s{1..6}` / `oxford-zipf-s{1..6}` Oxford DSI suites, and the `watdiv-stress-{100,1000}-{warmup,test-1..5}` WatDiv suites. Run `kermit bench list` for the full set.

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

`--metrics` defaults to `insertion iteration space`.

### Join benchmark (`bench join`)

Benchmark end-to-end join execution on data files:

```sh
kermit bench join \
  --relations edge.csv \
  --query query.dl \
  --algorithm leapfrog-triejoin \
  --indexstructure tree-trie
```

Supported index structures: `tree-trie`, `column-trie`. Supported metrics: `insertion`, `iteration`, `space`.

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

## Plotting

Criterion's auto-generated SVG/HTML output is disabled — thesis-quality plots are rendered separately by [`scripts/kermit-plot/`](scripts/kermit-plot/), a Python tool consuming `--report-json` output plus Criterion's per-function JSON artefacts.

```sh
# one-time install
python -m venv venv && source venv/bin/activate
pip install -e scripts/kermit-plot

# render every applicable plot shape across one or many runs
kermit-plot render-all bench-runs/*.json --out-dir plots/
```

See [`scripts/kermit-plot/README.md`](scripts/kermit-plot/README.md) for the full subcommand list (`scaling`, `bar-time`, `bar-space`, `tradeoff`, `dist`, `bar-queries`).

## Contributing

Thanks for taking an interest! Perhaps after I've finished my thesis.

## License

This repository, as is customary with Rust projects, is duel-licensed under the [MIT](https://github.com/aidan-bailey/kermit/blob/master/LICENSE-MIT.txt) and [Apache-V2](https://github.com/aidan-bailey/kermit/blob/master/LICENSE-APACHE.txt) licenses.

