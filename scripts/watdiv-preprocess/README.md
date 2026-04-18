# watdiv-preprocess

One-time preprocessor that turns N-Triples + SPARQL workloads into kermit benchmark
artifacts. Ships with two CLI drivers — a WatDiv-specific one that preserves the
original behaviour, and a generic driver for arbitrary workloads.

## Outputs (both drivers)

- `dict.json`, `dict.parquet` — shared URI → `usize` dictionary.
- `<predicate>.parquet` — one file per predicate URI (two `Int64` columns `s`, `o`).
- `<parent-dir-name>-<sparql-stem>.yml` — one kermit `BenchmarkDefinition` per SPARQL file.
- `expected.json` — optional; per-query expected cardinalities (see driver-specific notes).

Parquet files must be uploaded to the location matching `--base-url` before generated
benchmarks will run — the preprocessor only writes URL strings.

## Install

```bash
python -m venv venv && source venv/bin/activate
pip install -e .
```

This registers two console scripts: `watdiv-preprocess` and `sparql-preprocess`.

## WatDiv driver (`watdiv-preprocess`)

Expects the canonical WatDiv layout: `watdiv.10M.nt` at `--input`, plus
`watdiv-stress-{100,1000}/*.sparql` subdirs. Harvests `.desc` sidecars into
`expected.json` automatically.

```bash
watdiv-preprocess \
    --input ../../watdiv-data \
    --output ./out \
    --base-url https://zivahub.uct.ac.za/ndownloader/files
```

Flags:

- `--input` — dir containing `watdiv.10M.nt` and `watdiv-stress-*/` subdirs.
- `--output` — artifacts dir (created if missing).
- `--base-url` — URL prefix written into generated YAMLs.
- `--nt-name` — NT filename under `--input` (default `watdiv.10M.nt`).

## Generic driver (`sparql-preprocess`)

Makes no assumptions about directory layout. Caller supplies the NT path explicitly
and one or more glob patterns selecting SPARQL files.

```bash
sparql-preprocess \
    --input /path/to/workload \
    --nt /path/to/data.nt \
    --sparql-glob 'queries/*.sparql' \
    --output ./out \
    --base-url https://example.com/data
```

Flags:

- `--input` — root dir; `--sparql-glob` patterns are resolved relative to it.
- `--nt` — N-Triples file (can live anywhere).
- `--output`, `--base-url` — same as WatDiv driver.
- `--sparql-glob` — repeatable glob pattern; default `**/*.sparql` if omitted.
- `--expected-json` — optional pre-computed `expected.json` to copy into `--output`.
  The driver does not harvest `.desc` files — produce this externally if you want it.

## Core library

The shared orchestration lives in `watdiv_preprocess.pipeline.run_pipeline`. Both
drivers are thin argparse wrappers around it. Use it directly from Python when
the CLI doesn't fit — e.g. invoking the pipeline from a notebook or from another
driver with non-argparse argument handling.

## Scope

BGP-only SELECT queries. `FILTER`, `OPTIONAL`, `UNION`, and subqueries fail loudly.

Literal objects in the NT input are dictionary-encoded and partitioned alongside
URIs — every term position maps to a `usize` key, so predicates whose objects
are literals (e.g. `ogp:title`) produce valid Parquet files just like URI-only
predicates.

URI constants referenced by a query but absent from the NT input do **not** fail
loudly. The translator assigns them a fresh dictionary ID so the resulting
Datalog rule is well-formed; the query evaluates to the empty result at run
time because the synthetic `c<id>` never appears in any per-predicate Parquet
(kermit's const-atom rewriter turns it into a singleton trie iterator that
joins to nothing). This matches SPARQL semantics: a triple pattern with an
unseen URI is a no-match, not an error. If the dictionary grew during
translation, `dict.json` and `dict.parquet` are re-emitted at the end of the
pipeline so the on-disk artifacts stay in sync with the `c<id>` atoms baked
into the YAMLs.
