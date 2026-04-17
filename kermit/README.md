# kermit

Top-level crate of the Kermit workspace. Ships two things:

- a small **library** surface re-exporting the most useful types from [`kermit-algos`](../kermit-algos) and [`kermit-ds`](../kermit-ds), plus the [`DB`](src/db.rs) trait that bridges a parsed Datalog query to an indexed relation store.
- the **`kermit` CLI** — a `clap`-based binary with two top-level subcommands (`join` and `bench`), backed by [Criterion](https://github.com/bheisler/criterion.rs) for benchmark execution.

See the workspace [`README.md`](../README.md) for a broader introduction and the [`ARCHITECTURE.md`](../ARCHITECTURE.md) for the design rationale.

## Library surface

- [`db::DB`](src/db.rs) — object-safe trait erasing the concrete `Relation` / `JoinAlgo` types so the CLI can hold `Box<dyn DB>`.
- [`db::DatabaseEngine`](src/db.rs) — the sole implementation, parameterised by the chosen data structure and join algorithm.
- [`db::instantiate_database`](src/db.rs) — dispatches on the CLI enums (`IndexStructure`, `JoinAlgorithm`) to construct a `Box<dyn DB>`.
- [`compute_join`](src/lib.rs) — helper that builds relations from raw tuple vectors and runs a join end-to-end.
- `algos::LeapfrogTriejoin` and `ds::{RelationFileExt, TreeTrie}` re-exports for downstream consumers.

## CLI

### Run a join

```sh
kermit join \
  --relations edge.csv \
  --query query.dl \
  --algorithm leapfrog-triejoin \
  --indexstructure tree-trie
```

Writes result tuples as CSV to stdout (or `--output` if given).

### Benchmarks

All benchmarking is driven through `kermit bench`:

- `kermit bench join ...` — Criterion-time a single join query.
- `kermit bench ds ...` — measure insertion, iteration, and heap-space for a specific index structure against a single relation file.
- `kermit bench run <NAME> ...` — run one of the YAML-declared benchmarks from [`benchmarks/`](../benchmarks).
- `kermit bench list` — print available benchmark names.
- `kermit bench fetch [NAME]` — pre-download the data files for a benchmark.
- `kermit bench clean [NAME]` — remove cached data files.

On Linux the benchmark cache lives at `~/.cache/kermit/benchmarks/`.

Full help: `kermit --help`, or `kermit bench <subcommand> --help`.

## Adding an index structure or algorithm

- **New data structure** — add the type in [`kermit-ds`](../kermit-ds), extend `IndexStructure`, and wire up `run_ds_bench` / `run_benchmark` in [`src/main.rs`](src/main.rs).
- **New join algorithm** — add the type in [`kermit-algos`](../kermit-algos), extend `JoinAlgorithm`, and add a match arm in [`db::instantiate_database`](src/db.rs).
