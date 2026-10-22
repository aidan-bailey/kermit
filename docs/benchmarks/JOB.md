# JOB (Join Order Benchmark)

The Join Order Benchmark is a join-ordering workload introduced by Leis, Gubichev, Mirchev,
Boncz, Kemper and Neumann (2015). Its 113 multi-join queries, 33 query structures with 2–6
variants that differ only in their selections, run over a real May 2013 snapshot of the
Internet Movie Database (IMDb, 21 tables), whose correlations and skew defeat the uniformity
and independence assumptions that synthetic benchmarks share with query optimisers. Kermit
generates it as a benchmark: each query's per-alias filters are applied once, by DuckDB,
during generation, and the remaining equi-joins run as a Datalog rule on kermit's engines.
That keeps JOB's real data, real selectivities and hard join orders, and gives the cost-based
optimiser (#81) and the free per-query index orders (#93, 2026-10-22) a real workload to be judged on.

JOB is the relational counterpart of the RDF benchmarks: LUBM and WatDiv derive binary
predicates from triples, while JOB's relations come from a relational schema, with several
join columns each and one benchmark per query template. For a side-by-side comparison with
other benchmarks in the suite, see [`README.md`](README.md#cross-benchmark-comparison).

## How to run

JOB needs two things outside the workspace. **DuckDB**: `KERMIT_DUCKDB_BIN` if set, else
`duckdb` on `PATH`, which `nix develop` provides (`pkgs.duckdb`); without it generation fails
with an error naming both. **The IMDb raw cache**, built once on first use and shared by every
template (see [Output layout](#output-layout)): 114 small files from the pinned query
repository, then a 1.26 GB download through the pinned mirrors, then a build into
`imdb.duckdb` of about 30 s, with download progress on stderr.

### On-the-fly generation

```bash
kermit bench gen job --template N --tag STR [--output-dir PATH]
```

Produces `~/.cache/kermit/benchmarks/job-{template}-{tag}/` runnable via the unmodified
`kermit bench run`. The bench name is collision-checked against committed YAMLs at runtime.

| Flag | Default | Notes |
|------|---------|-------|
| `--template` | required | JOB template, 1–33; `job-6-dev` holds queries 6a–6f |
| `--tag` | required | Suffix on the bench name; pick a value that won't collide with a committed spec |
| `--output-dir` | `~/.cache/kermit/benchmarks` | Custom output dirs are NOT auto-discovered by `bench list/fetch/run` |

DuckDB is chosen by the environment, not a flag: `KERMIT_DUCKDB_BIN=/path/to/duckdb`.

### Declarative YAML spec (commit-and-run)

The 33 templates are committed as `benchmarks/job-1.yml` … `benchmarks/job-33.yml`.
`bench run <name>` materialises the data on first invocation:

```yaml
# benchmarks/job-6.yml
name: job-6
description: "Join Order Benchmark, template 6 (6a-6f)"
generator:
  kind: job
  template: 6
```

- `kind: job` and `template` (1–33) are both required; `template` outside 1–33 is rejected by
  `BenchmarkDefinition::validate` with `job generator template must be in 1..=33`.
- There is no `scale`: the data is the fixed IMDb dump, and the template picks the queries.
- Editing `template` and re-running errors with `SpecDrift` — pass `--force` to opt into
  regenerating. `--force` rebuilds that benchmark only; the raw cache is untouched. See
  `benchmarks/README.md` for the full schema.

**Why one benchmark per template.** `bench run` reads every relation a definition's queries
name into one flat catalog before it runs any query, so one benchmark holding all 113
queries' relations would hold all of them at once, `cast_info` (about 36 M rows) among them.
`job-6` holds the relations of 6a–6f only; `bench run job-6 -q 6c` selects one query.

```bash
kermit bench run job-6 -i all -a all --verify
```

`--verify` runs each query once and aborts on a count that differs from its `expected`, which
is DuckDB's `COUNT(*)` (see [Determinism](#determinism)). `bench run --all` materialises all
33 templates.

## Pipeline

```
imdb.tgz (mirror 1, then mirror 2)            schema.sql + 113 query files
        │ download_verified (SHA-256 pinned)         │ download_verified (pinned commit)
        │ tar -xzf → 21 CSVs; imdb.tgz deleted       │
        ▼                                            │
 duckdb: schema.sql, then one COPY per table         │
 in PostgreSQL's CSV dialect, pinned in full;        │
 row counts checked against the pins                 │
        │                                            │
        ▼                                            ▼
   imdb.duckdb                         queries/<template><letter>.sql
        └──────── raw cache, built once, shared by all 33 templates ────────┐
                                                                            │
                          translate::parse_query (sqlparser) ◄──────────────┤
                          translate::plan_template                          │
                                   │                                        │
                                   ▼                                        │
              per template: relations (table, columns, filter)              │
                            + one Datalog rule per query                    │
                                   │                                        │
        ┌──────────────────────────┴───────────────┐                        │
        ▼                                          ▼                        │
 per relation: duckdb                       per query: duckdb ◄─────────────┘
   COPY (SELECT CAST(c AS BIGINT), …          SELECT COUNT(*) of the
         FROM table                           original query
         WHERE <filter> AND k IS NOT NULL            │
         ORDER BY id) TO '/dev/stdout'               │ becomes the query's `expected`
        │ CSV on stdout → Tuples batches             │
        │ of 65,536 rows                             │
        ▼                                            │
 RelationWriter → <relation>.parquet × K             │
        │                                            │
        └──────────────────┬─────────────────────────┘
                           ▼
              benchmark.yml (+ expected per query) + meta.json
```

Generation is out of core end to end: DuckDB streams, kermit writes Parquet batch by batch,
and no relation is ever held whole. Only the pure-Rust translator reads the SQL; DuckDB is
the only thing that evaluates it. The 114 small files are fetched before the dump, so a stale
query pin fails before the long download.

Each table loads with `COPY <table> FROM '<csv>' (FORMAT csv, HEADER false, DELIMITER ',',
QUOTE '"', ESCAPE '\', ALLOW_QUOTED_NULLS false, AUTO_DETECT false)`: PostgreSQL's CSV
format, which wrote the dump. The pin is defensive. Left to its defaults, DuckDB sniffs every
unset option from a sample, and it reads a quoted `""` as NULL where PostgreSQL reads an empty
string. The real dump has no empty string in any of its 49 `VARCHAR` columns (its 57.7 M NULLs
are all in nullable columns), so today it loads the same under either reading; the pin keeps
that true of any dump. The archive also holds a `schematext.sql`, a different schema; kermit
loads only the pinned `schema.sql` from the query repository.

### DuckDB sessions

Every DuckDB call sets `memory_limit = '4GB'`, a `temp_directory` of its own,
`preserve_insertion_order = true` and `enable_progress_bar = false`, and opens the database
read-only, except the raw-cache build. DuckDB's join, sort and aggregate operators spill to
disk past the limit, instead of claiming their default 80% of RAM beside a timing run. The
spill directory is a fresh `duckdb.tmp/<pid>-<seq>` in the raw cache, removed when the call
ends: DuckDB names its spill files by size alone and deletes every `duckdb_temp_*` file in its
directory on exit, so two processes sharing one directory would corrupt each other's spills. The
progress bar is off because DuckDB's CLI may draw one after about two seconds: on stderr it
would fail the empty-stderr check below, and on stdout it would corrupt the row stream. A
`COPY … TO '/dev/stdout'` is trusted only if DuckDB exits 0 with an empty stderr; otherwise the
error names the relation and quotes stderr, and no new `<name>.parquet` is written.

The count oracle is the original query with its projection replaced by `COUNT(*)` and every
identifier double-quoted. DuckDB's keywords are not PostgreSQL's: JOB's `aka_title AS at`
(15a–15d) is valid PostgreSQL, where `AT` is non-reserved, but DuckDB refuses a bare `at`.

## Output layout

```
~/.cache/kermit/benchmarks/job-{N}/            (job-{N}-{tag} for bench gen)
  meta.json                  GeneratedMeta — kind = "job"; written last, the cache's commit marker
  benchmark.yml              kermit BenchmarkDefinition: every variant of template N as a query,
                             each with `expected`
  <relation>.parquet × K     one per distinct (table, columns, filter); Int64 columns; rows in
                             ascending id order

~/.cache/kermit/imdb/                          the shared raw cache; not a benchmark
  imdb.duckdb                the 21 IMDb tables
  queries/1a.sql … 33c.sql   the 113 pinned queries
  provenance.json            imdb_sha256, query_commit, a SHA-256 per query file, and the DuckDB
                             version that built the cache; written last
  duckdb.tmp/                parent of each DuckDB call's own spill directory, <pid>-<seq>
```

`meta.json` carries #95's common fields (`schema_version`, `kind`, `tag`, `generated_at_utc`,
`spec_hash`, `relation_count`, `query_count`, and `relations`: each relation's columns, rows and
SHA-256), plus JOB's:
`template`, `imdb_sha256`, `query_commit`, `query_sha256` (this template's files and
`schema.sql`), `duckdb_version` (the generation's), `raw_duckdb_version` (the one that built the
raw cache, from its `provenance.json`), and `sources` (per relation `{relation, table, filter}`,
joined by name to `relations`).

The raw cache has no `benchmark.yml` and no `meta.json`, so benchmark discovery ignores it. It
sits beside the `benchmarks/` cache, not in it: `bench clean job-6` and `bench clean --all`
remove only `~/.cache/kermit/benchmarks`, so the raw cache survives both. Delete
`~/.cache/kermit/imdb/` by hand to force a re-download and rebuild.

**When the pins change.** Every generation first compares a complete raw cache's
`provenance.json` with the pins in the binary: the dump's SHA-256, the query commit and every
query file's SHA-256. If any differs, generation stops with an error naming the field, the
cache's value, the pinned value and the directory to delete. Kermit never deletes or rebuilds
the cache itself, the same rule as a generator's `SpecDrift`, which waits for `--force`. The
DuckDB version that built the cache is recorded but not compared: it is not a pin. Two other
states are refused the same way, naming the directory to delete: an `imdb` directory without
`provenance.json` (an unfinished cache, which the build could not rename over), and a
`provenance.json` that cannot be read. All three are refused before anything is downloaded. A
cache another process finished first is checked like any other. A refused or failed raw cache
leaves behind the empty `job-N-<tag>` directory `bench gen job` had made, which discovery
ignores (it has no `meta.json`).

The cache is built in two staging directories beside it, `imdb.fetch-<pid>-<seq>` (the
downloads and the extracted CSVs) and `imdb.building-<pid>-<seq>` (the database), and renamed
into place, so an interrupted build is never reused. The tarball is deleted as soon as it is
extracted, and a failed run removes both directories. A run stopped outright (Ctrl-C,
`SIGKILL`) leaves them behind under the pid of a process that is gone: any `imdb.fetch-*` or
`imdb.building-*` directory whose process no longer runs is safe to delete. Two first runs at
once each download; the later to finish is discarded.

## Inference / preprocessing

Pure Rust in `kermit-generate/src/job/translate.rs`: `sqlparser` (PostgreSQL dialect) parses
each query, and `plan_template` turns one template's variants into relations and rules. The
translator works on the parsed syntax tree, never on the query text.

| Rule kind | Examples | Required by |
|-----------|----------|-------------|
| Flatten the `AND` tree | `a AND b AND c` becomes three conjuncts; `x BETWEEN 1 AND 2` stays one node | 24 of the 113 queries use `BETWEEN`, which a text splitter would cut in half |
| Classify each conjunct | `mk.movie_id = t.id`: a **join** (two aliases, `=`). `t.production_year > …`: a **filter** on `t`. A non-equality between aliases, an OR across aliases, or a condition naming no alias: an **error** naming the query and the condition | All 113: none raises one, and every condition names an alias |
| Merge joins into variables | `mk.movie_id = t.id` and `mk.movie_id = ci.movie_id` make one variable over `mk.movie_id`, `t.id`, `ci.movie_id` | 4–17 aliases and 2–10 variables per query |
| Order the variables | By the position of the first join condition that mentions them, in text order | All 113; 0 column-order conflicts |
| Name the variables | After the first alias, in text order, whose `id` the variable contains (`K`, `T`, `MI_IDX1`); `<ALIAS>_<COLUMN>` when it contains no `id` | All 113 |
| Build each alias's relation | Its join columns in variable order, then the table's `id`, unless `id` is already a join column | 589 alias occurrences join on their own `id` |
| Exclude NULL join keys | `… AND movie_id IS NOT NULL` on every join column | Any nullable join column, such as `cast_info.person_role_id` (an SQL equi-join never matches NULL) |
| Share relations | Equal (table, ordered columns, filter text) within a template is one file | 465 files for 113 queries across 33 templates |
| Cast | Every column is `CAST(… AS BIGINT)` | DuckDB prints integer text for the row stream to parse; the files hold Int64 columns |

The rest of the translation, in the order it matters when reading a rule:

**Variables are ordered by text.** The order uses no data: it is the order in which each
query's join conditions first mention a variable, and the relation files give each alias's
columns in that order. Every index is built in plan order (per-query index builds,
2026-10-22), so no stored column order restricts any optimiser: the optimisers choose among
all orders, and comparisons between them are free to differ on every query. Before that, the
default `--column-orders stored` pinned every atom to its stored column order, so a query ran
in the order it was written, JOB measured one neutral order, and optimiser comparisons
belonged under `--column-orders any`.

**The `id` is carried.** An alias's `id` rides along as a `_` in the rule. It keeps filtered
rows distinct, so the sorted tries (which are sets) and HashTrie (a multiset) agree on one
`expected` count. When `id` is itself a join column (`t.id = mk.movie_id`), it is stored once,
as that column. A `_` means an unused named variable, so a carried `id` costs a column, not a
row.

**Relation names.** Unfiltered: `<table>`. Filtered: `<table>_<alias>_<letters>`, where
`<alias>` is the alias of the first query (in variant order) that uses the relation and
`<letters>` are the variant letters of every query that does. Names match `[a-z][a-z0-9_]*`,
which the writer enforces. The sharing key includes the column order, and the filter text is
sqlparser's printout of the alias's conjuncts with `alias.col` rewritten to `col`, joined by
`AND` in text order, so whitespace differences vanish. Template 8's variants rename aliases
(`n1`/`an1`, `n`/`an`, `a1`) and still share, because the key never contains an alias.
Relations are shared only within a template: each `job-N` directory is self-contained.

**The rule.** `Q(<every join variable, in order>) :- <one atom per FROM alias, in FROM order>.`
The full head makes the lexicographic optimiser follow the text order. No query needs the
selection rewrite: no alias has two columns in one join variable.

## Workload reference

The 113 queries are not reproduced here: the query repository carries no licence (see
[Vendored generator](#vendored-generator)). They are fetched and checked by SHA-256 at
generation time. Each query becomes one rule in its template's `benchmark.yml`, named as JOB
names it (`6a`).

### Template 6, as emitted

Nine relation files for six queries (from the design; `translate.rs`'s `template_6_shapes`
pins this shape on JOB-shaped SQL):

```
6a: Q(K, T, N) :- cast_info(T, N, _), keyword_k_ace(K), movie_keyword(K, T, _),
                  name_n_abcde(N), title_t_a(T).

movie_keyword(keyword_id, movie_id, id)   unfiltered            6a–6f
cast_info(movie_id, person_id, id)        unfiltered            6a–6f
keyword_k_ace(id)                         keyword = '…'         6a, 6c, 6e
keyword_k_bdf(id)                         keyword IN (…)        6b, 6d, 6f
name_n_abcde(id)                          name LIKE '%…%'       6a–6e
name(id)                                  unfiltered            6f
title_t_a / title_t_bc / title_t_def(id)  production_year > …
```

Across all 33 templates this gives 465 relation files.

### The 33 templates

Counted from the pinned `FROM` clauses; the alias count is the number of atoms in each rule,
and it is the same for every variant of a template. Cardinalities are not listed: they are
DuckDB's `COUNT(*)` for the dump actually in use, written to each query's `expected` in
`benchmark.yml`.

| Template | Variants | Aliases | Template | Variants | Aliases | Template | Variants | Aliases |
|----------|----------|---------|----------|----------|---------|----------|----------|---------|
| 1 | a–d | 5 | 12 | a–c | 8 | 23 | a–c | 11 |
| 2 | a–d | 5 | 13 | a–d | 9 | 24 | a–b | 12 |
| 3 | a–c | 4 | 14 | a–c | 8 | 25 | a–c | 9 |
| 4 | a–c | 5 | 15 | a–d | 9 | 26 | a–c | 12 |
| 5 | a–c | 5 | 16 | a–d | 8 | 27 | a–c | 12 |
| 6 | a–f | 5 | 17 | a–f | 7 | 28 | a–c | 14 |
| 7 | a–c | 8 | 18 | a–c | 7 | 29 | a–c | 17 |
| 8 | a–d | 7 | 19 | a–d | 10 | 30 | a–c | 12 |
| 9 | a–d | 8 | 20 | a–c | 10 | 31 | a–c | 11 |
| 10 | a–c | 7 | 21 | a–c | 9 | 32 | a–b | 6 |
| 11 | a–d | 8 | 22 | a–d | 11 | 33 | a–c | 14 |

Self-joins occur: a table may appear under several aliases (`info_type AS it1`, `info_type AS
it2`), each becoming its own relation, or one shared file when their columns and filters are equal.

### What is kept, what changes

Kept: the schema, the May 2013 data, JOB's 33 × (2–6) query structure, every join and every
filter exactly as written, and JOB's query names.

Changed:

- **`MIN(…)` becomes a count.** The published queries wrap every projected attribute in `MIN`
  (the paper's footnote 4: to keep communication from dominating). Kermit counts the joined
  rows instead, `COUNT(*)`, the bag cardinality that a `_` gives.
- **NULL join keys are excluded**, as an SQL equi-join excludes them.
- **Filters are evaluated at generation, not at join time.** A filter on one table commutes
  with an inner join and every optimiser applies it before joining, so JOB's join problem is
  intact; only what evaluates the filter expressions moves, to DuckDB.
- **The head lists every join variable**, not the SELECT list.
- **Keys are IMDb's own integer ids**, not dictionary ids.
- **A filters-stripped "JOB-shape" variant is out of scope.**

## Determinism

A JOB benchmark is reproducible from its spec and its inputs: generating the same template
from the same dump and the same query commit twice yields byte-identical relations. There is no
random number generator, and no clock reaches the data (`generated_at_utc` is in `meta.json`
only).

- **Row order is fixed by `ORDER BY id`.** DuckDB's parallel scan promises no order, and build
  cost (#66) and byte-identical regeneration both depend on one. Build cost depends on row
  order; answers never do.
- **Kermit writes every byte.** The Parquet writer re-chunks rows into fixed 2^16-row buffers,
  so a file's bytes do not depend on how its rows were batched, and regeneration is
  byte-identical across DuckDB versions as long as DuckDB returns the same rows. The bytes are
  stable for a fixed kermit build only (the Parquet writer's `created_by`).
- **`expected` is an oracle independent of kermit's executors:** DuckDB's `COUNT(*)` of the
  original query.
- **Identification after the fact:** `meta.json` records the IMDb dump's SHA-256, the query
  commit, the SHA-256 of `schema.sql` and of each of the template's query files, `duckdb --version` (of the generation and of the
  raw-cache build), and `sources`.
- **The pins are not in the spec hash.** `spec_hash` covers only `template`, so a change to
  `pins.rs` (the dump's digest, the query commit or a query file's digest) changes the output
  under a fixed spec, and a `job-N` cache generated before it would still pass as a hit. Such a
  change therefore follows the generator-cache rule in `CLAUDE.md`: bump `META_SCHEMA_VERSION`
  (`kermit-rdf/src/generator.rs`) and add a `job` floor to `OUTDATED_BEFORE`, so that old caches
  show as `stale`; users are never relied on to pass `--force`.

## Practical scale ceiling

Generation is not the limit. The **raw cache** is built once. Measured on 2026-10-16 with
DuckDB 1.4.4: the dump is 1,263,193,115 bytes (about five minutes to download here), its 21
CSVs are 3.88 GB, and they load in about 30 s into a 2.65 GB `imdb.duckdb`, which is what the
finished cache keeps. The tarball is deleted once extracted, so the first build peaks near
6.5 GB on disk (the CSVs beside the growing database), and the staging directory goes once
the database is in place. Each template then takes 1.3–7.6 s (measured on 2026-10-17 with a
release build, the raw cache in place): 1.3–3.1 s for most, 4.1–4.8 s for templates 3, 18–20
and 26, and 6.6–7.6 s for templates 6–8, 16 and 17, which read all 36 M rows of `cast_info`
and peak at 2.4–2.5 GB resident. The first template generated also builds the raw cache: job-1 took 854 s, of which
its own generation was about a second.

The **run phase** sets the memory ceiling. `cast_info` holds about 36 M rows and `movie_info`
15 M (Leis et al., §2.1), so the templates that read all of `cast_info` need the most memory,
HashTrie the most of all. The table below records, per template, the output
size (the bytes of its relation files) and, for each structure, the peak resident memory of
its verify run, or the failure. It was measured on 2026-10-17 on a 30 GB host with a release
build (jemalloc), one process per template and structure:
`bench run job-N -i <structure> -a <algorithm> --verify -m space`, the structure's default
layout and build, each process in its own `systemd-run --user --scope` with `MemoryMax=14G`
and no swap, its peak taken from GNU time's `%M`. Every query of every cell that finished
matched its `expected`.

The table predates per-query index builds (2026-10-22): a cell then built every relation of the template before any query, and now builds only the indexes each query reads, freeing them after its timers, so the peaks are expected to differ. The next sweep (#99) re-measures it.

| Template | Output size | Peak memory (tree-trie) | Peak memory (column-trie) | Peak memory (hash-trie) | Notes |
|----------|-------------|-------------------------|---------------------------|-------------------------|-------|
| job-1 | 84.7 MB | 0.6 GiB | 0.2 GiB | 0.9 GiB | 1a–1d |
| job-2 | 129.2 MB | 1.3 GiB | 0.3 GiB | 1.9 GiB | 2a–2d |
| job-3 | 98.5 MB | 1.0 GiB | 0.3 GiB | 1.5 GiB | 3a–3c |
| job-4 | 105.9 MB | 1.2 GiB | 0.3 GiB | 1.8 GiB | 4a–4c |
| job-5 | 55.6 MB | 0.6 GiB | 0.2 GiB | 0.9 GiB | 5a–5c |
| job-6 | 791.9 MB | 6.6 GiB | 1.8 GiB | 9.4 GiB | 6a–6f |
| job-7 | 735.8 MB | 6.7 GiB | 1.9 GiB | 8.9 GiB | 7a–7c |
| job-8 | 808.0 MB | 11.5 GiB | 2.7 GiB | OOM (>14 GiB) | 8a–8d; hash-trie killed by the cap within 15 s, twice |
| job-9 | 176.0 MB | 1.5 GiB | 0.3 GiB | 2.4 GiB | 9a–9d |
| job-10 | 103.3 MB | 1.0 GiB | 0.3 GiB | 1.7 GiB | 10a–10c |
| job-11 | 137.7 MB | 1.8 GiB | 0.3 GiB | 2.6 GiB | 11a–11d |
| job-12 | 331.9 MB | 3.2 GiB | 0.8 GiB | 4.9 GiB | 12a–12c |
| job-13 | 325.7 MB | 3.4 GiB | 0.8 GiB | 5.2 GiB | 13a–13d |
| job-14 | 120.0 MB | 1.6 GiB | 0.3 GiB | 2.3 GiB | 14a–14c |
| job-15 | 149.0 MB | 1.8 GiB | 0.4 GiB | 2.7 GiB | 15a–15d |
| job-16 | 877.3 MB | 7.5 GiB | 1.9 GiB | 10.9 GiB | 16a–16d |
| job-17 | 858.5 MB | 7.4 GiB | 1.9 GiB | 10.7 GiB | 17a–17f |
| job-18 | 381.2 MB | 3.2 GiB | 0.7 GiB | 4.9 GiB | 18a–18c |
| job-19 | 400.0 MB | 3.9 GiB | 0.9 GiB | 5.6 GiB | 19a–19d |
| job-20 | 551.4 MB | 5.9 GiB | 1.3 GiB | 9.2 GiB | 20a–20c |
| job-21 | 136.6 MB | 1.5 GiB | 0.3 GiB | 2.3 GiB | 21a–21c |
| job-22 | 194.6 MB | 2.5 GiB | 0.5 GiB | 3.8 GiB | 22a–22d |
| job-23 | 138.5 MB | 1.8 GiB | 0.4 GiB | 2.6 GiB | 23a–23c |
| job-24 | 173.2 MB | 1.9 GiB | 0.4 GiB | 2.8 GiB | 24a–24b |
| job-25 | 142.1 MB | 1.5 GiB | 0.3 GiB | 2.2 GiB | 25a–25c |
| job-26 | 565.6 MB | 6.2 GiB | 1.3 GiB | 9.5 GiB | 26a–26c |
| job-27 | 128.5 MB | 1.5 GiB | 0.3 GiB | 2.3 GiB | 27a–27c |
| job-28 | 127.0 MB | 1.6 GiB | 0.3 GiB | 2.3 GiB | 28a–28c |
| job-29 | 243.6 MB | 2.7 GiB | 0.5 GiB | 4.2 GiB | 29a–29c |
| job-30 | 156.8 MB | 1.6 GiB | 0.3 GiB | 2.4 GiB | 30a–30c |
| job-31 | 227.0 MB | 2.1 GiB | 0.4 GiB | 3.3 GiB | 31a–31c |
| job-32 | 79.3 MB | 0.9 GiB | 0.3 GiB | 1.3 GiB | 32a–32b |
| job-33 | 132.8 MB | 1.7 GiB | 0.3 GiB | 2.5 GiB | 33a–33c |

The notes name each template's queries. 98 of the 99 cells finished, in 28–264 s each, and
verified 335 query counts. The one cell that does not fit
is HashTrie on template 8, killed by the cap before it finished building; a re-run was killed
the same way. Template 8 is the only template whose unfiltered `cast_info` atom has four columns,
`cast_info(N1, T, RT, _)`, one level more than the same 685 MB relation in templates 6, 7, 16
and 17, whose HashTrie cells fit in 8.9–10.9 GiB. Across the table, ColumnTrie peaks at
0.18–0.29 times TreeTrie's memory and HashTrie at 1.33–1.66 times it. See
[Future work](#future-work).

## Vendored generator

Nothing from JOB is committed. The query repository carries no licence and the IMDb dump is
1.26 GB, so kermit pins both by SHA-256 in `kermit-generate/src/job/pins.rs` and fetches them
on first use.

| Field | Value |
|-------|-------|
| Query source | <https://github.com/gregrahn/join-order-benchmark> (a mirror, not maintained by the original authors) |
| Commit | `a39603662e023e449cb2121997a5034df9e02ebf` |
| Files | `schema.sql` and the 113 queries `1a.sql` … `33c.sql`: 114 files, each pinned by SHA-256. `fkindexes.sql` is not used |
| Query licence | None: the repository has no licence file |
| IMDb dump | `imdb.tgz`, 1,263,193,115 bytes, the CSVs of May 2013 (21 tables); SHA-256 `25f9d893c54f903366e0c263f88db0d429dbc2b159d4987ebc1e203242a7e988` (`IMDB_SHA256` in `pins.rs`) |
| Mirrors, tried in order | <https://bonsai.cedardb.com/job/imdb.tgz>, then <http://event.cwi.nl/da/job/imdb.tgz> |
| IMDb licence | Non-commercial use (Leis et al. §2.1); the terms are at <http://www.imdb.com/interfaces> |
| Row counts | One per table, pinned in `IMDB_TABLE_ROWS` (`cast_info` 36,244,344); the build refuses a table that differs |
| DuckDB | External tool: `pkgs.duckdb` in the flake, or `KERMIT_DUCKDB_BIN` |
| Local path | `~/.cache/kermit/imdb/` (the platform cache dir) |
| Committed | Digests and URLs only |

Kermit downloads the dump from the mirrors and never commits it or any file derived from it.
Both mirrors served the same bytes when the digest was pinned (2026-10-16); whichever is
used, the pinned digest is checked.

Changing a pin is a deliberate step: fetch the new file, `sha256sum` it into `pins.rs`, measure
the new table row counts with the DuckDB CLI, bump `META_SCHEMA_VERSION` and add a `job` floor
to `OUTDATED_BEFORE` (see [Determinism](#determinism)), delete `~/.cache/kermit/imdb/`, and
regenerate every template with `--force`. Until the old cache is deleted, every `job`
generation stops with the error described under [Output layout](#output-layout). Timings from
either side of a pin change are not comparable.

## Tests

| Test | What it validates | Gate |
|------|-------------------|------|
| `kermit-generate/src/job/translate.rs` unit tests | Parsing and classifying JOB-shaped SQL: `BETWEEN`, OR in parentheses, quoted `AND` and dots, `LIKE`/`IN`/`IS NULL`, self-joins; variable order and names; `id` carried only when not a join column; relation sharing and naming; one error per unsupported shape | None |
| `kermit-generate/src/job/pins.rs` unit tests | Pinned file names are unique and their digests 64-character lowercase hex; query URLs carry the commit; the IMDb digest and row counts are measured values, not placeholders | None |
| `kermit-generate/src/job/duckdb.rs` unit tests | Quoting of literals and identifiers; batching and order of streamed rows; a private spill directory per call, removed afterwards; stderr or a non-zero exit fails a stream even after rows, and errors quote stderr; read-only sessions | The batching, spill-directory and quoting tests need nothing, and the failure tests use a `/bin/sh` stand-in; the rest need `duckdb` on `PATH` or `KERMIT_DUCKDB_BIN` |
| `kermit-generate/src/job/raw.rs` unit tests | The raw-cache build on a fixture (row-count mismatch leaves no cache; a leftover staging directory is not reused; provenance is written last; a lost rename adopts the winner; PostgreSQL's CSV dialect, on a sample that would mislead DuckDB's sniffer); `ensure`'s mirror fallback, queries-before-dump order, staging cleanup and a first run under a missing cache parent directory; a complete cache built from other pins, an unfinished `imdb` directory and an unreadable `provenance.json` are each refused before any fetch, naming the directory (the first two also checked to be left in place); a cache adopted after a lost rename is checked against the pins too; an extra non-SQL `schematext.sql` in the archive is ignored | The build and `ensure` tests need `duckdb`; the mirror loop, staging guard, pins and provenance-comparison tests need nothing |
| `kermit-bench` `definition.rs` tests | `kind: job` parses; templates 0 and 34 are rejected; `spec_hash` changes with the template | None |
| `kermit/tests/job_generate.rs` | The fixture pipeline, also over CSVs loaded in reverse order (so a missing `ORDER BY` shows): filtered rows in id order; kermit's count equals DuckDB's `COUNT(*)` on every structure × optimiser; byte-identical regeneration; a failed filter leaves no file for the failing relation, no `.part`, no `benchmark.yml` and no `meta.json` (and, when it is the template's first relation, no relation at all); a provenance missing a query's digest fails before anything is written; `meta.json` records sources and versions; every committed `job-N.yml`'s description names its template's variants | `duckdb` available, except the YAML-description test |
| `kermit/tests/cli_job_gen.rs` | `bench gen job` and `bench run job-1` through the real binary over a fixture raw cache, with the platform cache redirected so nothing is written under `~/.cache/kermit`: a template outside 1–33, a missing DuckDB and a tag that makes an invalid benchmark name fail before any directory is created; `bench gen job` writes `job-N-<tag>`; it refuses a raw cache built from other pins and leaves it unchanged; `bench run job-1` materialises the committed YAML, verifies every count, then hits the cache, reports drift and rebuilds under `--force` | The first three run anywhere and need no DuckDB; the rest need Linux (`XDG_CACHE_HOME`) and `duckdb` |
| `kermit/tests/job_translate.rs` | The real 113 queries (fetched and digest-checked at test time): every rule parses and passes `kermit::db::validate_query` against its template's relations, the check `bench run` applies; every relation name is parser-valid; 33 templates, 113 rules and 465 relation files; template 12's three variants share one `company_name` relation | Network to `raw.githubusercontent.com` (CI too); skips only when the first fetch cannot connect; a digest mismatch fails; about 40 s cold (114 serial fetches); not miri |

The translator test on real queries lives in the `kermit` crate because it checks each rule
with `kermit::db::validate_query`, which `kermit-generate` cannot reach; the fixture pipeline
lives there for the same reason (it counts through `kermit::db`). The fixture pipeline is the load-bearing correctness check for the
filter stage and the translation; finding it green is the precondition for relying on a
`job-N` benchmark. CI installs no DuckDB, so the DuckDB-gated tests run only under `nix
develop`.

## References

- V. Leis, A. Gubichev, A. Mirchev, P. Boncz, A. Kemper, T. Neumann. *How Good Are Query
  Optimizers, Really?* PVLDB 9(3), 204–215, 2015.
  <http://www.vldb.org/pvldb/vol9/p204-leis.pdf>
- The authors' own copy of the queries, as the paper links it:
  <http://www-db.in.tum.de/~leis/qo/job.tgz>
- Pinned query repository: <https://github.com/gregrahn/join-order-benchmark>
- IMDb data and terms: <http://www.imdb.com/interfaces>
- DuckDB: <https://duckdb.org>
- `sqlparser` crate: <https://crates.io/crates/sqlparser>
- Design: `docs/specs/2026-10-15-job-benchmark-design.md`
- Module: `kermit-generate/src/job/` (`translate.rs`, `duckdb.rs`, `raw.rs`, `pins.rs`)
- Sibling benchmark references: [`LUBM.md`](LUBM.md), [`WATDIV.md`](WATDIV.md),
  [`SNAP.md`](SNAP.md), [`H2O.md`](H2O.md)

## Future work

- **Cells that do not fit.** One cell runs out of the 14 GiB cap on this host: HashTrie on
  template 8 (see the memory table). Two HashTrie layouts might bring it under the cap, and
  neither is measured. Singleton pruning (`--ds-layout-pruning on`) stores a one-row subtrie
  as one row id, and 31.4 M of the 33.3 M (person, movie) pairs in `cast_info(N1, T, RT, _)`
  (94 %) have one row. Lazy expansion (`--ds-layout-expansion lazy`) builds a child only when a probe
  reaches it.
- **A filters-stripped "JOB-shape" variant.** Dropping the filters changes the join problem
  (much larger inputs), so it would be a second workload with its own `expected`, not a flag.
- **An outdated-cache floor for `job`.** `kind: job` is new, so `OUTDATED_BEFORE` has no `job`
  entry yet. The spec hash covers only `template`: when the translator's or the filter stage's
  output changes for a fixed template, or a pin changes (see [Determinism](#determinism)), add
  the first one (`MetaHeader::outdated_reason`) rather than letting old caches pass as hits.
- **Aggregate-faithful counting.** Kermit counts joined rows; reproducing JOB's `MIN(…)`
  projections would need aggregation in the engine, which is out of scope.
