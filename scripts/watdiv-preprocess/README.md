# watdiv-preprocess

One-time preprocessor that turns WatDiv inputs into kermit benchmark artifacts.

## Inputs

- `watdiv.10M.nt` — N-Triples dataset (~1.5 GB, 10.9M triples).
- `watdiv-stress-100/*.sparql` + `watdiv-stress-1000/*.sparql` — query workload.
- `watdiv-stress-*/*.desc` (optional) — expected cardinality sidecars.

## Outputs

- `dict.json`, `dict.parquet` — shared URI → `usize` dictionary.
- `<predicate>.parquet` — one file per predicate URI (two `UInt64` columns `s`, `o`).
- `watdiv-stress-<size>-<stem>.yml` — one kermit `BenchmarkDefinition` per SPARQL file.
- `expected.json` — per-query expected cardinalities harvested from `.desc` files.

## Usage

```bash
python -m venv venv && source venv/bin/activate
pip install -e .
watdiv-preprocess \
    --input ../../watdiv-data \
    --output ./out \
    --base-url https://zivahub.uct.ac.za/ndownloader/files
```

Parquet files must be uploaded to the location matching `--base-url` before the
generated benchmarks will run — the preprocessor only writes URL strings.

## Scope

BGP-only SELECT queries. `FILTER`, `OPTIONAL`, `UNION`, and subqueries fail loudly.
