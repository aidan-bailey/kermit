# WatDiv stress workload: seed empty relations for absent predicates

Resolves [#63](https://github.com/aidan-bailey/kermit/issues/63).

## Problem

`kermit-rdf/tests/e2e_watdiv.rs::watdiv_sf1_pipeline_succeeds_and_produces_expected_artifacts`
fails in roughly 13 % of isolated runs with

```
UnsupportedSparql("predicate URI not in partition map: <IRI>")
```

WatDiv draws data (`-d`) and queries (`-s`/`-q`) as independent invocations
of the model. Rare predicates live in `<pgroup>` blocks with p < 1 over small
entity populations, so at scale 1 a predicate can appear in a generated query
while drawing zero triples in the generated data. `partition::partition`
builds `predicate_map` only from triples that exist, and
`translate_query` hard-errors on any query predicate outside that map.

Commit 3be1e97 already solved this for the **Basic** workload by seeding an
empty relation per absent query predicate (`seed_missing_predicates` in
`kermit-rdf/src/pipeline.rs`). The **Stress** workload was excluded on the
assumption that generated templates could only reference predicates present
in the data. The measurements in #63 show that assumption is false. The
earlier "race between concurrent tests" diagnosis was also wrong: each
generation runs in a fresh staging dir and reads the binary's stdout, and the
failure reproduces with zero concurrency.

## Decision

Seed empty relations for absent query predicates in **both** WatDiv
workloads. An absent predicate becomes an empty relation, so the affected
query translates and yields an empty result instead of aborting the pipeline.
The query set stays exactly what WatDiv produced.

Alternatives rejected:

- **Drop the query and warn.** Query count would vary run-to-run and Basic
  and Stress would treat the same event differently.
- **Filter templates against the data before `-q`.** Requires threading the
  data predicate set back into the driver and editing WatDiv's own output.

## Changes

### `kermit-rdf/src/pipeline.rs`

- `WatdivGenerator::seed_relations` calls `seed_missing_predicates`
  unconditionally.
- Delete `Workload::seeds_missing_predicates`. `Workload` remains because it
  still selects the `meta.json` `kind` string; its type-level and variant doc
  comments lose the seeding clauses.
- Reword the doc comment on `seed_missing_predicates`: the cause is that
  WatDiv draws data and queries independently from the model, so any workload
  can reference a predicate that drew zero triples.

### `kermit-rdf/src/generator.rs`

- The `Generator::seed_relations` hook doc cites "the WatDiv generator"
  rather than "the WatDiv basic workload". LUBM keeps the no-op default; its
  data is deterministic and its 14 queries are fixed.

### Tests

1. **Unit test for `seed_missing_predicates`** in the `pipeline.rs` test
   module. Build a `Partitioned` by hand with one relation, write a temp
   `.sparql` file whose BGP references that predicate plus two absent ones,
   one of which sanitises to a name colliding with the existing relation.
   Assert: each absent predicate gains an empty relation, a `predicate_map`
   entry, and a dictionary ID; the colliding one gets the `_<id>` suffix; the
   present relation and its tuples are unchanged. Deterministic, no bwrap or
   watdiv needed.
2. **`e2e_watdiv.rs`**: the assertion that `meta.relation_count` equals the
   count from re-partitioning `raw/data.nt` becomes `>=`, with a comment that
   seeded relations are absent from the raw data by construction.
3. **`e2e_watdiv.rs`**: new check that every relation the emitted
   `benchmark.yml` declares has a matching `<name>.parquet` in the output
   dir. `yaml_emit::write_benchmark_yaml` derives the `relations` list from
   the predicates the Datalog queries use, so this is exactly the set that
   seeding must cover. Load the YAML with `kermit_bench`'s benchmark
   definition type (already a dependency of `kermit-rdf`); no parser needed.

### Docs

- `docs/benchmarks/WATDIV.md`: retitle "Absent predicates in the basic
  workload" to "Absent predicates" and rewrite it to cover both workloads,
  including the per-run probability at scale 1 and the note that this is
  effectively unobservable at scale 100 and above.
- `docs/benchmarks/WATDIV.md` § Determinism: add a paragraph recording that
  WatDiv seeds from the wall clock at one-second granularity (200
  back-to-back `-d` runs produced 20 distinct datasets, in contiguous
  identical blocks), and the resulting rule: independent generations must be
  spaced more than one second apart, and `-d`/`-q` stages that fall in the
  same second share a seed.
- `CLAUDE.md` WatDiv on-the-fly gotcha: one sentence stating that both
  workloads seed empty relations for query predicates absent from the data.

## Out of scope

- The translator keeps its hard error; an unknown predicate after seeding is
  a genuine bug.
- No changes to LUBM, the driver, the sandbox, or the CLI.
- No cross-process locking or test serialisation.
- No `--seed` support in the vendored binary (listed under Future work in
  `WATDIV.md`).

## Verification

`cargo test -p kermit-rdf` for the unit test; the e2e test run 15 times with
`sleep 1.2` between runs inside `nix develop`, expecting 15 passes; the
standard CI gate (`clippy -Dwarnings`, `fmt --check`, `doc`).

Commit: `fix(rdf): seed empty relations for absent predicates in the stress
workload too`, closing #63.
