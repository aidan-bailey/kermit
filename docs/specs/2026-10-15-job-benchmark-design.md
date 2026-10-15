# Join Order Benchmark Generator (`kind: job`)

**Date:** 2026-10-15
**Status:** Design approved (brainstormed 2026-10-15, revising the
2026-10-05 brainstorm that filed #97)
**Scope:** Issue #97. The Join Order Benchmark (Leis et al., "How Good Are
Query Optimizers, Really?", VLDB 2015) becomes a generator benchmark: all
113 queries (33 templates) over IMDb, with each alias's filters applied
during generation, so kermit runs JOB's real joins.
**Related:** #95 (H2O, owns the shared generator layer, the
`kermit-generate` crate, and its streaming writer), #96 (SNAP, owns kermit-bench's `download_verified`), #81
(cost-based optimiser) and #93 (`--column-orders any`), which JOB evaluates;
#99 (the next sweep).

## Motivation

kermit's runnable workloads are RDF-derived (LUBM, WatDiv) plus the
`triangle` sanity check. JOB is the standard workload for join ordering:
real data, real selectivities, and join orders known to be hard. It is the
natural evaluation of #81's cost-based optimiser and #93's `any` policy,
and complements #95 (acyclic H2O joins) and #96 (cyclic SNAP patterns).

Filtering a single table commutes with an inner join and every optimiser
applies it before joining, so applying the filters ahead of time keeps
JOB's join problem intact. Only evaluating the filter expressions moves to
generation, done by DuckDB as an external tool, the way LUBM uses `java`.

## Changes from #97 as filed

A scan of the 113 pinned query files and of today's master corrected these
points of the issue:

| #97 said | This design | Why |
|---|---|---|
| Split the WHERE clause at top-level ANDs, respecting parentheses and quotes | Parse with the `sqlparser` crate and flatten the AST's `AND` tree | 24 of 113 queries contain `BETWEEN x AND y`, which a top-level-AND splitter cuts in half |
| One variable order per template ("all variants share the same joins") | One order per query, by first appearance among the join conditions; identical relations shared by key | Template 8's variants rename aliases (`n1`/`an1`, `n`/`an`, `a1`); per-query text order gives 0 column-order conflicts across all 113 queries |
| Relation = join columns, then `id` | `id` appended only when it is not already a join column | 589 alias occurrences join on their own `id`; the rule as written would store it twice |
| The rule's head unspecified | Head = every join variable in order | The parser needs at least one head term; a full head makes the lexicographic optimiser follow the text order |
| `RelationSink::write` buffers each relation (~1.1 GB peak) | DuckDB streams CSV; kermit writes Parquet batch by batch | User decision: generation is out of core end to end |
| Download into the raw cache (helper unspecified) | #96's `download_verified` | #96 builds the streaming, digest-checked download (user decision) |
| Builds on whichever of #95/#96 lands `kermit/src/generate/` | Builds on #95's new `kermit-generate` crate | User decisions: H2O owns the layer, and it is a workspace crate, not a binary module |

Premises re-checked and unchanged: `bench run` still loads every relation of
a definition (`kermit/src/bench/run.rs:113`), so benchmarks stay per
template; `read_parquet` still reads only Int64 columns, so every column is
cast to `BIGINT`; both IMDb mirrors serve 1,263,193,115 bytes;
`gregrahn/join-order-benchmark` is still at
`a39603662e023e449cb2121997a5034df9e02ebf`.

## Workload

- **One benchmark per template.** `generator: { kind: job, template: <1–33> }`
  generates `job-<N>` with every variant (`job-6` = 6a–6f). `bench run`
  loads every relation of a definition, so one benchmark holding all 113
  queries' relations would load all of them at once.
- **Query names** are JOB's (`6a`).
- **`expected`** is DuckDB's `COUNT(*)` of the original query: a bag count,
  matching kermit's `_`.
- **Deviations from published JOB:** the `MIN(…)` aggregates become a count
  of joined rows, and rows whose join key is NULL are excluded, as an SQL
  equi-join excludes them. A filters-stripped "JOB-shape" variant is out of
  scope.

Facts from the scan that the translator relies on: 4–17 aliases and 2–10
join variables per query; no condition joins two aliases by anything other
than `=`, no OR spans two aliases, and every condition names an alias; no
alias has two columns in one join variable, so no query needs the selection
rewrite.

## Translator (`kermit-generate/src/job/translate.rs`, pure Rust)

**Parsing.** `sqlparser` (PostgreSQL dialect) parses each query. The
translator reads the `FROM` aliases, then flattens the WHERE clause's
top-level `BinaryOp::And` tree; `BETWEEN … AND …` is one node. Each conjunct
is classified by the `alias.column` identifiers it contains:

- two aliases compared by `=`: a join;
- one alias: a filter on that alias;
- anything else (a non-equality between aliases, an OR across aliases, a
  condition naming no alias): an error naming the query and the condition
  (#78).

**Variables.** Union-find over `alias.column` merges the join conditions
into variables. Each query orders its variables by the position of the
first join condition that mentions them, in text order. The order is
neutral by design: it uses no data, so under `--column-orders stored` JOB
measures one fixed order, and optimiser comparisons belong under `any`
(JOB.md says so). A variable is named after the first alias, in text order,
whose `id` it contains (`K`, `T`, `MI_IDX1`), or `<ALIAS>_<COLUMN>` when it
contains no `id`.

**Relations.** An alias's columns are its join columns in variable order,
then `id` unless `id` is already a join column. The `id` keeps every row
distinct after filtering and projection, so the sorted tries (sets) and
HashTrie (a multiset) agree on one `expected`. A relation's sharing key is
(table, ordered columns, filter text): the alias's filters with each
`alias.col` rewritten to `col` in the AST, printed by sqlparser (which also
normalises whitespace), joined by `AND` in text order. Within a template, one file per key:

- unfiltered: `<table>`;
- filtered: `<table>_<alias>_<variant letters>`, with the alias of the first
  query (in variant order) that uses it.

**Rule.** Each query's Datalog is `Q(<every join variable, in order>) :-
<one atom per alias, in FROM order>.`, with `_` for each carried `id`.

Template 6, nine files for six queries:

```
6a: Q(K, T, N) :- cast_info(T, N, _), keyword_k_ace(K), movie_keyword(K, T, _),
                  name_n_abcde(N), title_t_a(T).

movie_keyword(keyword_id, movie_id, id)   unfiltered            6a–6f
cast_info(movie_id, person_id, id)        unfiltered            6a–6f
keyword_k_ace(id)                         keyword = '…'         6a, 6c, 6e
keyword_k_bdf(id)                         keyword IN (…)        6b, 6d, 6f
name_n_abcde(id)                          name LIKE '%Downey%…' 6a–6e
name(id)                                  unfiltered            6f
title_t_a / title_t_bc / title_t_def(id)  production_year > …
```

Across all 33 templates this gives 466 relation files.

## DuckDB stages (`kermit-generate/src/job/{raw,duckdb}.rs`)

**Finding DuckDB.** `KERMIT_DUCKDB_BIN`, then `duckdb` on PATH; the flake
adds `pkgs.duckdb`. Without it, generation fails with an error pointing at
`nix develop`. `meta.json` records `duckdb --version`.

**Session settings.** Every call sets `memory_limit = '4GB'`,
`temp_directory` inside the raw cache, and `preserve_insertion_order =
true`, and opens the database read-only (except the build). DuckDB's join,
sort and aggregate operators spill to disk past the limit, instead of
claiming its default 80% of RAM beside a timing run.

**Raw cache** (`~/.cache/kermit/imdb/`; shared by every template; no
`benchmark.yml`, so discovery ignores it):

- `imdb.tgz` through #96's `download_verified`, trying
  `https://bonsai.cedardb.com/job/imdb.tgz` then
  `http://event.cwi.nl/da/job/imdb.tgz`; its SHA-256 is pinned during
  implementation.
- `schema.sql` and the 113 queries from `gregrahn/join-order-benchmark` at
  `a39603662e023e449cb2121997a5034df9e02ebf`, each checked against a
  SHA-256 committed in kermit. The repository has no licence, so kermit
  pins the files rather than committing them.
- `imdb.duckdb`: `schema.sql`, then each CSV loaded with the dialect stated
  in full, `(FORMAT csv, HEADER false, DELIMITER ',', QUOTE '"', ESCAPE '\',
  ALLOW_QUOTED_NULLS false, AUTO_DETECT false)`, then each table's row count
  checked against a pinned table. The dump was written by PostgreSQL's CSV
  format, which escapes quotes with backslashes (the default dialect shifts
  fields into the wrong columns) and reads a quoted `""` as the empty string
  and only an unquoted empty field as NULL; DuckDB's default reads `""` as
  NULL and sniffs any option left unset, and neither change moves a row
  count, so the check would not catch them. JOB's reference counts are
  PostgreSQL's. The extracted CSVs are deleted after loading.
- Built in a temporary directory and renamed into place, so an interrupted
  build is never reused; a build that loses the rename to a concurrent one
  is discarded.

**Filter** (one DuckDB process per distinct relation):

```sql
COPY (SELECT CAST(movie_id AS BIGINT), CAST(person_id AS BIGINT), CAST(id AS BIGINT)
      FROM cast_info
      WHERE <filter text> AND movie_id IS NOT NULL AND person_id IS NOT NULL
      ORDER BY id) TO '/dev/stdout' (FORMAT csv)
```

kermit parses stdout into `Tuples` batches of 65,536 rows and hands each to
the relation's writer from #95's sink (`sink.open(&header)`, then
`write_batch(&batch)` per batch). It calls `finish()` only after DuckDB
exits 0 with an empty stderr; otherwise the error names the relation and quotes DuckDB's
stderr, and no `<name>.parquet` is left. Memory is one batch in kermit and
at most `memory_limit` in DuckDB. `ORDER BY id` fixes the row order, which
DuckDB's parallel scan does not promise and on which build cost (#66) and
byte-identical regeneration depend. Because kermit writes every byte,
regeneration is byte-identical across DuckDB versions as long as DuckDB
returns the same rows.

**Count** (one DuckDB call per query): the query's AST with its `MIN(…)`
projection replaced by `COUNT(*)`, printed and run. The result is the
query's `expected`, an oracle independent of kermit's executors.

**Cost.** The raw cache needs about 5 GB of disk and a few minutes to build;
each template then takes seconds to a couple of minutes. The run phase, not
generation, sets the memory ceiling: `cast_info` holds about 36M rows, and
HashTrie may not fit for the `cast_info` templates on a 30 GB host. Measure
and document, don't fix here.

## Integration

- **Spec.** `GeneratorSpec::Job { template: u8 }` in kermit-bench; `validate`
  rejects templates outside 1–33; `spec_hash` covers `template`. No `scale`.
- **Module.** `kermit-generate/src/job/` in #95's `kermit-generate` crate,
  registered by one `pub mod job;` line in its `lib.rs`: `mod.rs` (the
  generator, implementing `kermit_generate::Generator` with `KIND = "job"`:
  `generate(&self, &mut RelationSink)` writes the relations and returns the
  queries with their `expected` and the typed `meta.json` extras),
  `translate.rs`, `raw.rs`, `duckdb.rs`. The crate is
  `#![deny(missing_docs)]`, so every `pub` item is documented, with no
  intra-doc links to binary items (`materialize`, `ExecutionFamily`, …).
  Imports name the trait through `kermit_generate`, since
  `kermit_rdf::generator` has its own `Generator`.
- **`materialize::dispatch`** (in the binary) gets a `Job` arm calling
  `kermit_generate::run`.
- **CLI.** `bench gen job --template N` with #95's shared `GenTarget`
  flattened in (`--tag` required, `--output-dir` optional); it writes
  `job-{N}-{tag}`.
- **Committed.** `benchmarks/job-1.yml` … `job-33.yml`. Cache hit, drift,
  `--force` and `bench list` come from the shared meta, as for LUBM.
- **`meta.json` extras.** The template, the `imdb.tgz` SHA-256, the query
  commit and per-file SHA-256s, `duckdb --version` of the generation and of
  the raw-cache build (`duckdb_version`, `raw_duckdb_version`; they differ
  when a cache built by an older DuckDB is reused), and `sources`: per
  relation `{relation, table, filter}`, joined by name to the common
  `relations` list, which already records each relation's columns, rows and
  SHA-256. `filter` is the query's own predicate on that alias. Every
  relation also drops rows with a NULL in a join column, since an SQL
  equi-join never matches them; that is one fixed rule over the join
  columns `relations` already lists, stated here and in `JobSource`'s docs
  rather than repeated per relation. No extras field reuses a common name (`relations`, `columns`, …),
  since #95's `GeneratedMeta` flattens the extras beside them.
- **New dependency.** `sqlparser` in `kermit-generate/Cargo.toml` (never
  `kermit`'s).
- **What #97 uses from #95**, agreed 2026-10-15 and specified in #95's
  `docs/specs/2026-10-15-h2o-benchmark-design.md` (4e0beac on
  `aidanb/h2o`), Part 1:
  - `RelationSink::open(&RelationHeader) -> RelationWriter`, with
    `write_batch(&Tuples)` and `finish()`; `RelationSink::write` is `open`
    plus one batch plus `finish`, one code path.
  - Underneath, `kermit_ds::ParquetWriter`, which re-chunks rows into fixed
    2^16-row buffers before parquet-rs sees them, since parquet-rs cuts data
    pages relative to each input batch. A file's bytes therefore do not
    depend on how its rows were batched (pinned by #95's tests).
  - Writes go to `<name>.parquet.part`, renamed on `finish` and removed when
    the writer is dropped unfinished; `finish` records rows and SHA-256.

## Sequencing

1. **After #95's push 1** (the `kermit-generate` crate, landing on master
   alone), merged in from `origin/master`: the translator and its tests; the
   DuckDB session runner; the raw-cache build from a local `imdb.tgz`; the
   count stage. The `pub mod job;` line is JOB's only edit to the layer's
   files, a one-line add/add conflict with #96 at most.
2. **After #95's `GenTarget`** (push 2, or #96's phase 2 if first): the
   filter stage on the streaming writer, `kermit_generate::run`, the spec,
   CLI, `materialize` arm and the 33 YAMLs.
3. **After #96's phase 1** (which does not wait on #95, so this step may
   come before step 2): downloads through `download_verified`, which prints
   progress on stderr for bodies over 64 MiB; until then the raw cache takes
   a local file.
4. **Last:** the acceptance run and JOB.md's measured memory table.

The phases are development order on `aidanb/job`, not separate landings:
JOB lands on master once, after phase 3 and its 113-count check (the memory
table may follow as a docs commit), because a JOB that cannot both download
and dispatch is not usable. `kermit-generate` is a library crate, so its
`pub` items are not dead code before the dispatch arm exists, and every
branch commit passes `cargo clippy --all-targets` under `-Dwarnings`
without any lint attribute.

`bench run --all` will materialise all 33 templates (first run: the 1.26 GB
download and the raw-cache build). Whether the next sweep includes JOB is
#99's decision.

## Testing

- **Spec** (CI): `kind: job` parses; templates 0 and 34 are rejected;
  `spec_hash` changes with the template.
Unit tests (spec aside) live in `kermit-generate`; the pipeline test, which
counts through kermit's join entry points, lives in `kermit/tests/job_generate.rs`
(as #95's `kermit/tests/h2o_generate.rs`), since `kermit-generate` cannot
depend on `kermit`.

- **Translator** (CI, no network), on hand-written JOB-shaped SQL: `BETWEEN`
  stays one filter; OR inside parentheses; quoted strings containing
  `' AND '` and dots; `LIKE`, `NOT LIKE`, `IN`, `IS NULL`; self-joins; chain
  merging; text-order variables; `id` appended only when not a join column;
  the sharing key including column order; naming; head and rule text;
  renamed aliases still sharing relations; one error per unsupported shape.
- **Translator on the real 113** (`kermit/tests/job_translate.rs`, where
  `kermit-algos` is already a dependency, so `kermit-generate` gains no
  dev-dependency; needs network and fetches the pinned files from GitHub at
  test time, in CI too; skips without network, saying why on stderr): every condition classifies; every rule passes
  `check_attribute_order`; every relation and variable name is a
  kermit-parser identifier (an ASCII letter, then letters, digits or `_`),
  which #95's sink also enforces; golden counts of 466 relation files and 0
  column-order conflicts.
- **Pipeline on a committed fixture** (needs DuckDB; skips without it, as the
  LUBM tests do without Java; CI installs no DuckDB, so it runs only under
  `nix develop`). Tiny IMDb-shaped CSVs with NULL join keys,
  backslash-escaped quotes, and rows removed by `LIKE`, `IN`, ranges and
  `BETWEEN`, plus a few fixture queries (the real JOB files cannot be
  committed). Checked:
  - relation contents and `ORDER BY id` order;
  - every query's count on every cell × optimiser × `stored`/`any`, through
    `kermit::db`'s entry points, equals DuckDB's `COUNT(*)`;
  - regeneration is byte-identical;
  - a raw-cache row-count mismatch is an error, and an interrupted raw-cache
    build is not reused;
  - a DuckDB failure mid-stream leaves no `.parquet`.

## Docs

- `docs/benchmarks/JOB.md`: sources and citations (Leis et al. 2015; the IMDb
  mirrors; the pinned query commit; IMDb's non-commercial licence); what is
  kept and what changes; the translation rules, carried ids, text-order
  variables (and why `stored` is one fixed order), relation naming and
  sharing; why benchmarks are per template; DuckDB's settings, disk and
  time; the measured memory ceiling per template and structure.
- `benchmarks/README.md`: the `job` kind, and the common-fields table's
  `generator.scale` row stops saying "required for every kind" (#96 makes
  the same edit; whichever lands first does it).
- CLAUDE.md: a Benchmark Reference Docs entry; `bench gen job`; gotchas for
  DuckDB on PATH or `KERMIT_DUCKDB_BIN`, the shared raw cache, the streamed
  handoff, `ORDER BY id`, carried ids and text-order variables.
- `flake.nix`: `pkgs.duckdb`.

## Acceptance criteria

- `benchmarks/job-1.yml` … `job-33.yml` generate on `bench run`; the raw
  cache is built once and shared; cache hit, drift, `--force` and
  `bench list` behave as for LUBM.
- `bench gen job --template N --tag T` works.
- For all 113 queries, kermit's count equals DuckDB's `COUNT(*)`, and
  `bench run job-N -i all -a all --verify` passes on every cell that fits in
  memory; JOB.md lists the cells that don't, with measured sizes.
- Regenerating a template gives byte-identical relations.
- The tests above pass: spec and translator in CI, the DuckDB-gated tests
  locally under `nix develop`.
