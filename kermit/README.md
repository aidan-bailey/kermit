# kermit

Top-level crate of the Kermit workspace. Ships two things:

- a small **library** surface re-exporting the most useful types from [`kermit-algos`](../kermit-algos) and [`kermit-ds`](../kermit-ds), plus the [`db`](src/db.rs) join entry points that bridge a parsed Datalog query to a relation store.
- the **`kermit` CLI** — a `clap`-based binary with two top-level subcommands (`join` and `bench`), backed by [Criterion](https://github.com/bheisler/criterion.rs) for benchmark execution.

See the workspace [`README.md`](../README.md) for a broader introduction and the [`ARCHITECTURE.md`](../ARCHITECTURE.md) for the design rationale.

## Library surface

- [`db::lftj_join`](src/db.rs) — sorted-family join over a `BTreeMap<String, R>` of `TrieIterable` relations, generic in the algorithm.
- [`db::hash_join`](src/db.rs) — hash-family join over a `BTreeMap<String, HashTrie<H>>` under `HashTriejoin`.
- [`db::JoinFamily`](src/db.rs) — the trait both share, abstracting how a family wraps a relation and a constant for its algorithm.
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
- `kermit bench gen watdiv --scale N --tag STR ...` — synthesize a fresh WatDiv dataset and stress queries on the fly via the vendored generator.
- `kermit bench gen lubm --scale N --tag STR ...` — synthesize a fresh LUBM dataset on the fly (requires JDK 8 on `PATH` for the vendored UBA jar).
- `kermit bench list` — print available benchmark names.
- `kermit bench fetch [NAME]` — pre-download the data files for a benchmark.
- `kermit bench clean [NAME]` — remove cached data files.

Every `kermit bench` invocation also writes a machine-readable JSON report. By default it lands at `bench-runs/<kind>-<unix-millis>.json` (with `<kind>` one of `join`/`ds`/`run`); pass `--report-json <PATH>` on the `bench` subcommand to override. The on-disk layout is documented in [`../docs/specs/bench-report-schema.md`](../docs/specs/bench-report-schema.md), and the source of truth for the schema is [`src/bench_report.rs`](src/bench_report.rs).

On Linux the benchmark cache lives at `~/.cache/kermit/benchmarks/`.

Full help: `kermit --help`, or `kermit bench <subcommand> --help`. Deeper recipes live in [`../USAGE.md`](../USAGE.md).

## Adding an index structure or algorithm

- **New data structure** — add the type in [`kermit-ds`](../kermit-ds), extend `IndexStructure`, and wire up `run_ds_bench` / `run_benchmark` in [`src/main.rs`](src/main.rs).
- **New join algorithm** — add the type in [`kermit-algos`](../kermit-algos), extend `JoinAlgorithm`, and add its cell to `Execution::for_pair` in `src/execution.rs` (the compiler then flags every dispatch `match` to extend).
