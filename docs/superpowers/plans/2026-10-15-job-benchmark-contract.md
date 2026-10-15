# Names contract: #97, the JOB generator

Design session's contract for drafting `docs/superpowers/plans/2026-10-15-job-benchmark.md`.
Spec: `docs/specs/2026-10-15-job-benchmark-design.md` at da09595 on `aidanb/job`. Read it
first. This contract fixes every JOB name, signature, file and constant. A design decision it
does not make goes back to the design session in your report; do not invent one.

Worktree: `/tb/Source/Academia/kermit/.loom/worktrees/aidanb/job_18dea468f74c13c3`. Branch
`aidanb/job` (never switch, amend or push).

**Host rule for drafters:** do NOT run cargo (build, test, check, clippy, doc, fmt) or DuckDB
on real data. A timing run (#101 replicate chain) is live until `FULL_EXIT`. Read source
only. No filesystem-wide `find`.

## Upstream names (do not rename)

**#95's layer** (contract `git show ffc516c:docs/superpowers/plans/2026-10-15-generate-layer-contract.md`,
spec `git show 41537a4:docs/specs/2026-10-15-h2o-benchmark-design.md`). Not on master yet;
quote these exactly and nothing else from the crate:

- `kermit_generate::{Generator, Generated, RelationSink, RelationWriter, RelationRecord,
  GeneratedMeta, run, output_dir, OutputDir}`.
- `trait Generator { const KIND: &'static str; type Extras: Serialize + DeserializeOwned;
  fn target(&self) -> kermit_rdf::generator::Target<'_>; fn generate(&self, sink: &mut
  RelationSink) -> anyhow::Result<Generated<Self::Extras>>; }`
- `struct Generated<E> { pub description: String, pub queries:
  Vec<kermit_bench::QueryDefinition>, pub extras: E }`
- `RelationSink::open(&mut self, &RelationHeader) -> anyhow::Result<RelationWriter<'_>>`
  (rejects names not matching `[a-z][a-z0-9_]*` and duplicates), `RelationSink::write`,
  `RelationSink::raw_dir`, `RelationSink::records`.
- `RelationWriter::write_batch(&mut self, &Tuples) -> anyhow::Result<()>`,
  `RelationWriter::finish(self) -> anyhow::Result<()>`. A writer dropped unfinished leaves no
  file and records nothing.
- `kermit_generate::run(&G) -> anyhow::Result<GeneratedMeta<G::Extras>>`: deletes the old
  `meta.json`, calls `generate`, rejects extras keys in `COMMON_FIELDS` (`schema_version`,
  `kind`, `tag`, `generated_at_utc`, `relation_count`, `query_count`, `spec_hash`,
  `relations`), checks every query parses and names recorded relations, writes
  `benchmark.yml` and then `meta.json`.
- `kermit_rdf::generator::Target<'a> { out_dir: &'a Path, bench_name: &'a str, tag: &'a str,
  spec_hash: Option<&'a str> }` (pub fields, `Copy`).
- The crate root `kermit-generate/src/lib.rs` gets one line from #97: `pub mod job;`.
- `GenTarget` (binary, `kermit/src/main.rs`, lands with #95 push 2 or #96 phase 2):
  `#[command(flatten)] target: GenTarget`, fields `tag: String`, `output_dir:
  Option<PathBuf>`, method `fn prepare(&self, kind: &str, bench_name: &str) ->
  anyhow::Result<PathBuf>`.

**#96's download** (spec `aidanb/snap`, f300850 and later):
`kermit_bench::cache::download_verified(url: &str, dest: &Path, sha256: &str) -> Result<(),
BenchError>`. Streams into `<dest>.part` while hashing, renames on a match. A digest mismatch
is `BenchError::Integrity`; a non-200, WAF challenge or empty body is
`BenchError::UnusableDownload`; a network failure or a 30 s stall is `BenchError::Download`.
Prints progress on stderr past 64 MiB. Tests must not match on its error text.

## Facts of the tree (verified 2026-10-15)

- kermit-bench: `GeneratorSpec` is `#[serde(tag = "kind", rename_all = "kebab-case")]`
  (definition.rs:50). `validate_generator` (definition.rs:424) checks each kind and returns
  `BenchError::Invalid { name, reason }`. `GeneratorSpec::spec_hash` hashes the spec's YAML.
  `QueryDefinition { name: String, description: String, query: String, expected:
  Option<u64> }`. `cache::base_cache_dir()` = `<platform cache>/kermit/benchmarks`.
- kermit binary: `materialize::dispatch(spec, bench_name, out_dir, spec_hash)`
  (materialize.rs:258) matches every `GeneratorSpec` and passes `tag: bench_name` in each
  `Target`. `GenSubcommand` (main.rs:392); `run_gen_lubm` (main.rs:1112) is the CLI
  precedent.
- `kermit_iters::Tuples` (re-exported by kermit-ds): `with_capacity(arity, rows)`,
  `push(&[usize])`, `len()`, `arity()`, `clear` does not exist (make a fresh batch).
  `kermit_ds::RelationHeader::new(name, attrs: Vec<String>)`.
- Tests that need an external tool skip with a stderr line when it is missing:
  `java_available()` in `kermit/tests/lubm_cardinalities.rs:49` is the precedent.
- `kermit/tests/lubm_mini_oracle.rs` is the precedent for counting a generated benchmark
  through `kermit::db` (`cardinality_mismatches`, line 207).
- `flake.nix` `devShells.default.buildInputs` (line ~28) lists `pkgs.bubblewrap`,
  `pkgs.jdk8` with a comment each.
- sqlparser 0.63.0 (published 2026-09-13) is in the cargo registry:
  `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/sqlparser-0.63.0/`. Read it for
  exact APIs: `Parser::parse_sql(&PostgreSqlDialect {}, sql)`; `Statement::Query`;
  `SetExpr::Select`; `TableFactor::Table { name: ObjectName(Vec<ObjectNamePart>), alias:
  Option<TableAlias { explicit, name, columns }>, .. }`; `Expr::{BinaryOp, CompoundIdentifier,
  Between, Nested, ..}`; `visit_expressions_mut` (feature `visitor`, `ast/visitor.rs:706`).
- DuckDB in the flake's nixpkgs is 1.4.4 (`nix shell --inputs-from . nixpkgs#duckdb`).
  Verified on a scratch table: SQL errors exit 1 with the message on stderr; `-readonly`
  refuses writes (exit 1); `COPY (…) TO '/dev/stdout' (FORMAT csv, HEADER false)` streams
  rows; `-csv -noheader -c "SELECT COUNT(*) …"` prints the bare number; `COPY t FROM 'x.csv'
  (FORMAT csv, HEADER false, ESCAPE '\\')` reads backslash-escaped quotes. `--version`
  prints `v1.4.4 (Andium) 6ddac802ff`.
- `imdb.tgz` holds its CSVs at the top level (`aka_name.csv`, …), one per `schema.sql` table
  (21 tables).

## Stories

| Story | Phase | Title | Drafter |
|---|---|---|---|
| J1 | 1 | Crate wiring, `translate::parse_query` | A |
| J2 | 1 | `translate::plan_template` | A |
| J3 | 1 | `pkgs.duckdb` in the flake; `duckdb.rs` | B |
| J4 | 1 | `pins.rs`, `raw.rs`, the committed fixture, the IMDb measurements | B |
| J5 | 2 | `JobGenerator` (filter and count stages), `kermit/tests/job_generate.rs` | C |
| J6 | 2 | `GeneratorSpec::Job`, the `dispatch` arm, `bench gen job`, the 33 YAMLs | C |
| J7 | 3 | `raw::ensure` downloads; `kermit/tests/job_translate.rs` | D |
| J8 | 4 | Docs: `JOB.md`, `benchmarks/README.md`, CLAUDE.md | D |
| J9 | 4 | Acceptance and the memory table (design session, user-gated) | — |

Phase 1 starts after #95's push 1 is on master and merged into `aidanb/job`. Phase 2 needs
`GenTarget`; phase 3 needs `download_verified`. JOB lands once, after J7.

## Files

```text
kermit-generate/Cargo.toml          + sqlparser = { version = "0.63.0", features = ["visitor"] }
kermit-generate/src/lib.rs          + pub mod job;  (and one crate-docs line naming JOB)
kermit-generate/src/job/mod.rs      module docs, `pub mod`s, JobGenerator, JobExtras, JobSource
kermit-generate/src/job/translate.rs  parse_query, plan_template, TranslateError (J1, J2)
kermit-generate/src/job/duckdb.rs   Duckdb (J3)
kermit-generate/src/job/pins.rs     URLs, commit, 114 file digests, 21 row counts (J4)
kermit-generate/src/job/raw.rs      RawCache, build, extract_tgz, default_raw_dir (J4); ensure (J6 stub, J7 full)
kermit-generate/tests/fixtures/job/ schema.sql, csv/<table>.csv, queries/<name>.sql, rows.txt (J4)
kermit/tests/job_generate.rs        fixture pipeline, DuckDB-gated (J5)
kermit/tests/job_translate.rs       the real 113, network-gated (J7)
kermit-bench/src/definition.rs      GeneratorSpec::Job (J6)
kermit/src/materialize.rs           dispatch arm, run_job (J6)
kermit/src/main.rs                  GenSubcommand::Job, run_gen_job (J6)
benchmarks/job-1.yml … job-33.yml   (J6)
flake.nix                           + pkgs.duckdb (J3)
docs/benchmarks/JOB.md, benchmarks/README.md, CLAUDE.md (J8)
```

Every `pub` item in kermit-generate is documented (`#![deny(missing_docs)]`); no intra-doc
link names a binary item. Doc comments backtick identifiers (clippy `doc_markdown`).

## J1: crate wiring and `parse_query`

`kermit-generate/Cargo.toml`: `sqlparser = { version = "0.63.0", features = ["visitor"] }`.
`lib.rs`: `pub mod job;`. `job/mod.rs` (J1 creates it with module docs and
`pub mod translate;`; J3/J4/J5 add `pub mod duckdb; pub mod pins; pub mod raw;` and the
generator).

`job/translate.rs`:

```rust
/// One `alias.column` reference.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ColumnRef { pub alias: String, pub column: String }

/// One `FROM` entry, `table AS alias`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Alias { pub name: String, pub table: String }

/// A JOB query, parsed and its WHERE conjuncts classified.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobQuery {
    pub name: String,                       // "6a"
    pub aliases: Vec<Alias>,                // FROM order
    pub joins: Vec<(ColumnRef, ColumnRef)>, // text order, as written (left, right)
    pub filters: Vec<(String, String)>,     // (alias, conjunct printed with `alias.` stripped), text order
    pub count_sql: String,                  // the statement with its projection replaced by COUNT(*)
}

pub fn parse_query(name: &str, sql: &str) -> Result<JobQuery, TranslateError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TranslateError {
    Parse { query: String, message: String },          // sqlparser's error text
    NotOneSelect { query: String },                    // not exactly one `SELECT … FROM … WHERE …`
    UnsupportedFrom { query: String, item: String },   // a FROM item that is not `table AS alias`
    DuplicateAlias { query: String, alias: String },
    UnknownAlias { query: String, alias: String },     // `x.col` with x not in FROM
    CrossAliasCondition { query: String, condition: String }, // ≥ 2 aliases, not `a.x = b.y`
    NoAliasCondition { query: String, condition: String },
    UnjoinedAlias { query: String, alias: String },    // (J2) an alias in no join condition
    NameClash { name: String },                        // (J2) two relations or variables share a name
}
impl fmt::Display for TranslateError { … }   // each names the query and the condition/item
impl std::error::Error for TranslateError {}
```

Rules:
- Parse with `PostgreSqlDialect`. Exactly one `Statement::Query` whose body is a plain
  `SetExpr::Select` with a `selection`, no joins in `from` (`TableWithJoins.joins` empty),
  every `from` item `TableFactor::Table` with a single-part name and an alias; else the
  matching error.
- Flatten `selection` through `BinaryOp { op: And }` (left before right). `Nested` is not
  flattened, so `(a OR b)` stays one conjunct and `BETWEEN … AND …` is one node.
- Classify a conjunct by the aliases of its `CompoundIdentifier`s (two parts: alias,
  column). Aliases are compared as written (JOB's are lowercase).
  - `CompoundIdentifier = CompoundIdentifier` with two different aliases: a join.
  - Exactly one alias: a filter. Its text is the conjunct with every `alias.col` rewritten to
    `col` (`visit_expressions_mut` on a clone), printed by `Display`.
  - Two or more aliases otherwise: `CrossAliasCondition`. None: `NoAliasCondition`.
- `count_sql`: the parsed statement with `projection` replaced by one `COUNT(*)` item,
  printed by `Display` (no trailing `;`).

J1 tests (inline): `between_is_one_filter`; `or_in_parentheses_is_one_filter`;
`quoted_and_and_dots_do_not_split` (`mi.info LIKE '%a.b AND c%'`); `like_not_like_in_is_null`;
`self_join_aliases_are_distinct` (`info_type AS it1, info_type AS it2`);
`join_chain_is_three_joins`; `filter_text_strips_the_alias`; `count_sql_replaces_min`; one
test per error variant J1 raises.

## J2: `plan_template`

```rust
/// One relation file of a template.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelationPlan {
    pub name: String,          // "movie_keyword", "keyword_k_ace"
    pub table: String,
    pub columns: Vec<String>,  // join columns in variable order, then "id" unless already present
    pub join_columns: usize,   // how many leading columns are join columns (NULLs excluded)
    pub filter: String,        // conjuncts joined by " AND " in text order; "" when unfiltered
    pub queries: Vec<String>,  // the queries using it, in input order
}

/// One query's rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RulePlan { pub query: String, pub datalog: String, pub count_sql: String }

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplatePlan { pub relations: Vec<RelationPlan>, pub rules: Vec<RulePlan> }

/// `queries` are one template's variants in letter order.
pub fn plan_template(queries: &[JobQuery]) -> Result<TemplatePlan, TranslateError>;
```

Per query:
- Variables: union-find over the joins' `ColumnRef`s. Position = index of the first join
  (text order) mentioning a member.
- Variable name: walk the joins in text order, left then right; the first member whose
  column is `id` gives its alias uppercased (`k` → `K`, `mi_idx1` → `MI_IDX1`); if none,
  the first member gives `ALIAS_COLUMN` uppercased. Two variables of one query with one name
  → `NameClash`.
- An alias with no join column → `UnjoinedAlias`.
- Relation key = (table, columns, filter). One `RelationPlan` per distinct key across the
  template, in first-use order (query order, then FROM order).
- Naming: unfiltered → `table`; filtered → `{table}_{alias}_{letters}`, `alias` from the first
  query using the key, `letters` the variant letters (`name` minus leading digits) of every
  query using it, in order. Two keys with one name → `NameClash`.
- Rule: `Q({head}) :- {body}.`, `head` = variable names in position order joined by `", "`,
  body = one atom per alias in FROM order joined by `", "`, atom = `{relation}({terms})`, terms
  = each column's variable name, `_` for a carried `id`.

J2 tests: `template_6_shapes` (hand-written 6a/6b-style SQL → the example in the spec);
`variables_follow_text_order`; `id_is_appended_only_when_not_joined`;
`shared_relation_keeps_column_order_in_its_key`; `filtered_name_lists_variant_letters`;
`renamed_aliases_still_share` (template-8 style); `variable_named_after_id_alias`;
`fallback_variable_name`; `unjoined_alias_is_an_error`; `relation_name_clash_is_an_error`;
`every_relation_name_matches_sink_rule` (`[a-z][a-z0-9_]*`).

## J3: the flake and `duckdb.rs`

`flake.nix`: `pkgs.duckdb` in `buildInputs`, with a comment: runs the JOB generator's
filter and count stages; required by `bench gen job` and the DuckDB-gated tests.

```rust
pub const DUCKDB_BIN_ENV: &str = "KERMIT_DUCKDB_BIN";
pub const MEMORY_LIMIT: &str = "4GB";
pub const BATCH_ROWS: usize = 1 << 16;

/// A located DuckDB CLI.
#[derive(Debug, Clone)]
pub struct Duckdb { bin: PathBuf, version: String }

impl Duckdb {
    /// `$KERMIT_DUCKDB_BIN`, else `duckdb` on PATH; runs `--version`. The error says to
    /// enter `nix develop` or set `KERMIT_DUCKDB_BIN`.
    pub fn locate() -> anyhow::Result<Duckdb>;
    pub fn at(bin: PathBuf) -> anyhow::Result<Duckdb>;   // same, for an explicit path
    pub fn version(&self) -> &str;                        // first line of `--version`, trimmed
    /// Read-write: runs `sql` against `database`, for the raw-cache build.
    pub fn execute(&self, database: &Path, temp_dir: &Path, sql: &str) -> anyhow::Result<()>;
    /// Read-only: runs a query returning one non-negative integer.
    pub fn query_u64(&self, database: &Path, temp_dir: &Path, sql: &str) -> anyhow::Result<u64>;
    /// Read-only: streams `COPY ({select}) TO '/dev/stdout' (FORMAT csv, HEADER false)`,
    /// parsing each line into `arity` integers and calling `on_batch` with batches of
    /// `BATCH_ROWS` rows (the last one shorter, none when empty). Returns the row count.
    /// Succeeds only if DuckDB exits 0 with an empty stderr; else the error quotes stderr.
    pub fn stream_rows(
        &self, database: &Path, temp_dir: &Path, select: &str, arity: usize,
        on_batch: &mut dyn FnMut(&Tuples) -> anyhow::Result<()>,
    ) -> anyhow::Result<u64>;
}
```

- Every call prefixes `SET memory_limit='4GB'; SET temp_directory='<temp_dir>';
  SET preserve_insertion_order=true;` and runs `duckdb [-readonly] <database> -csv -noheader
  -c "<prelude> <sql>"`.
- `stream_rows` drains stderr on a thread while reading stdout; a line that does not parse
  is an error naming its line number; if `on_batch` errors, kill the child and return that
  error.

J3 tests (inline, skip with a stderr line when `Duckdb::locate()` fails):
`stream_rows_batches_and_orders` (`SELECT range AS a, range * 2 AS b FROM range(70000) ORDER
BY a`: two batches, 65,536 then 4,464 rows, in order); `stream_rows_empty_result_calls_nothing`;
`sql_error_quotes_stderr`; `readonly_refuses_writes`; `query_u64_reads_a_count`;
`locate_honours_the_env_override` (env set to a non-existent path → error naming it).

## J4: `pins.rs`, `raw.rs`, the fixture

`job/pins.rs`:
```rust
pub const IMDB_URLS: [&str; 2] = ["https://bonsai.cedardb.com/job/imdb.tgz", "http://event.cwi.nl/da/job/imdb.tgz"];
pub const IMDB_SHA256: &str = "…";            // measured in J4
pub const QUERY_COMMIT: &str = "a39603662e023e449cb2121997a5034df9e02ebf";
pub const QUERY_BASE_URL: &str = "https://raw.githubusercontent.com/gregrahn/join-order-benchmark";
pub const QUERY_FILES: [(&str, &str); 114] = [ /* (file, sha256), schema.sql first, then 1a.sql … 33c.sql in version order */ ];
pub const IMDB_TABLE_ROWS: [(&str, u64); 21] = [ /* measured in J4, schema order */ ];
pub fn query_url(file: &str) -> String;       // "{QUERY_BASE_URL}/{QUERY_COMMIT}/{file}"
```
The 114 digests (computed 2026-10-15 from a clone at `a396036`) are in
`/tmp/claude-1000/-tb-Source-Academia-kermit--loom-worktrees-aidanb-job-18dea468f74c13c3/a2d71fa8-3211-4812-8912-f34c5ea9fe78/scratchpad/job-pins.txt` as `sha256sum` output; the plan copies them verbatim.

`job/raw.rs`:
```rust
pub const RAW_DIR_NAME: &str = "imdb";
/// `<platform cache>/kermit/imdb`, beside `base_cache_dir()`'s `benchmarks`.
pub fn default_raw_dir() -> anyhow::Result<PathBuf>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RawProvenance {
    pub imdb_sha256: String,
    pub query_commit: String,
    pub query_sha256: BTreeMap<String, String>,   // file → digest
    pub duckdb_version: String,                   // the build's
}

pub struct BuildInputs<'a> {
    pub csv_dir: &'a Path,          // <table>.csv per table
    pub schema_sql: &'a Path,
    pub queries_dir: &'a Path,      // <t><letter>.sql files, copied into the cache
    pub table_rows: &'a [(&'a str, u64)],
    pub provenance: RawProvenance,
}

/// A complete raw cache: `imdb.duckdb`, `queries/`, `provenance.json` (written last).
#[derive(Debug, Clone)]
pub struct RawCache { dir: PathBuf }
impl RawCache {
    pub fn open(dir: &Path) -> Option<RawCache>;  // Some iff <dir>/provenance.json exists
    pub fn dir(&self) -> &Path;
    pub fn database(&self) -> PathBuf;            // <dir>/imdb.duckdb
    pub fn temp_dir(&self) -> PathBuf;            // <dir>/duckdb.tmp
    pub fn provenance(&self) -> anyhow::Result<RawProvenance>;
    /// `(name, sql)` of `<dir>/queries/<template><letter>.sql`, in letter order.
    pub fn template_queries(&self, template: u8) -> anyhow::Result<Vec<(String, String)>>;
}

/// Builds in `<dir>.building-<pid>` and renames it to `dir`. Loads `schema.sql`, then each
/// table with `COPY <table> FROM '<csv>' (FORMAT csv, HEADER false, ESCAPE '\\')`, checks
/// every row count against `table_rows` (a mismatch is an error naming the table, both
/// counts, and leaves no `dir`), copies the queries, writes `provenance.json`, renames.
/// If `dir` appeared meanwhile, the staging dir is removed and `dir` is opened.
pub fn build(duckdb: &Duckdb, dir: &Path, inputs: &BuildInputs<'_>) -> anyhow::Result<RawCache>;

/// `tar -xzf <tgz> -C <into>`.
pub fn extract_tgz(tgz: &Path, into: &Path) -> anyhow::Result<()>;

/// Phase 2 stub (J6): `RawCache::open(dir)` or an error saying the IMDb cache is missing.
/// J7 replaces the error with the download and build.
pub fn ensure(duckdb: &Duckdb, dir: &Path) -> anyhow::Result<RawCache>;
```

Fixture `kermit-generate/tests/fixtures/job/` (hand-written, not JOB's text):
- `schema.sql`: `title`, `kind_type`, `keyword`, `movie_keyword`, `name`, `cast_info`,
  `info_type`, `movie_info`, with JOB's column names for the columns used.
- `csv/<table>.csv`: a few dozen rows in all, including NULL join keys (`cast_info.person_id`,
  `movie_info.info_type_id`), a backslash-escaped quote in `title.title`, rows removed by
  `LIKE`, `IN`, `BETWEEN` and a range, and a self-join on `info_type`.
- `queries/1a.sql`, `1b.sql`, `2a.sql`: JOB-shaped (`SELECT MIN(…) … FROM … WHERE …;`),
  covering those filters, a self-join (`it1`, `it2`) and a shared unfiltered relation.
- `rows.txt`: `table rows` per line; the tests read it into `table_rows`.

`build` loads exactly the tables `schema.sql` creates (read back from
`information_schema.tables`); each needs `<csv_dir>/<table>.csv` and a `table_rows` entry, and
a missing one is an error naming the table.

J4 measurement steps (after `FULL_EXIT`, with the user's go-ahead for the 1.26 GB download):
download `imdb.tgz` into a scratch directory (not `~/.cache/kermit`), `sha256sum` it into
`IMDB_SHA256`; extract it and load the 21 CSVs into a scratch database with the DuckDB CLI
directly (the plan gives the script: `schema.sql`, one `COPY … (FORMAT csv, HEADER false,
ESCAPE '\\')` per table, then `SELECT COUNT(*)` per table), and pin the counts in
`IMDB_TABLE_ROWS`.

J4 tests (inline in raw.rs, DuckDB-gated): `builds_the_fixture`; `row_count_mismatch_leaves_no_cache`;
`leftover_staging_dir_is_not_reused`; `template_queries_are_in_letter_order`;
`provenance_round_trips`; `open_needs_provenance_last`. Plus `pins.rs`:
`query_files_are_unique_and_hex` and `query_url_has_the_commit` (no DuckDB).

## J5: `JobGenerator`

`job/mod.rs`:
```rust
pub const TEMPLATES: RangeInclusive<u8> = 1..=33;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobSource { pub relation: String, pub table: String, pub filter: String }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobExtras {
    pub template: u8,
    pub imdb_sha256: String,
    pub query_commit: String,
    pub query_sha256: BTreeMap<String, String>,   // this template's files and schema.sql
    pub duckdb_version: String,                   // the generation's
    pub sources: Vec<JobSource>,
}

pub struct JobGenerator<'a> {
    pub template: u8,
    pub raw: &'a RawCache,
    pub duckdb: &'a Duckdb,
    pub target: Target<'a>,
}
impl Generator for JobGenerator<'_> { const KIND: &'static str = "job"; type Extras = JobExtras; … }

/// The SELECT the filter stage streams for one relation.
pub fn filter_select(relation: &RelationPlan) -> String;
```

- `filter_select`: `SELECT CAST("c" AS BIGINT) AS "c", … FROM "<table>" WHERE <filter> AND
  "c1" IS NOT NULL AND … ORDER BY "id"`, NULL checks on the first `join_columns` columns,
  the `<filter> AND` part omitted when the filter is empty.
- `generate`: `template_queries` → `parse_query` each → `plan_template` → for each relation,
  `sink.open(&RelationHeader::new(name, columns))`, `stream_rows(…, &mut |b| w.write_batch(b))`,
  `w.finish()` → for each rule, `query_u64(count_sql)` → `QueryDefinition { name, description:
  format!("JOB {name}, filters applied at generation"), query: datalog, expected:
  Some(count) }`. `description` of the benchmark: `format!("Join Order Benchmark template
  {template} (Leis et al., VLDB 2015) over IMDb, filters applied at generation")`.
- A template outside `TEMPLATES` or with no query files is an error.

J5 tests: unit `filter_select_shapes` (filtered, unfiltered, id-only). Integration
`kermit/tests/job_generate.rs` (DuckDB-gated, builds the fixture raw cache in a tempdir,
generates templates 1 and 2 with `kermit_generate::run` into a tempdir):
`relations_hold_filtered_rows_in_id_order`; `counts_match_duckdb_on_every_cell` (every cell ×
every optimiser × `stored`/`any`, through `kermit::db`, as `lubm_mini_oracle.rs` does);
`regeneration_is_byte_identical`; `failed_filter_leaves_no_relation_and_no_meta` (a copy of
the fixture with a query filtering on a missing column); `extras_record_sources_and_versions`.

## J6: wiring

- kermit-bench: `GeneratorSpec::Job { template: u8 }` (doc: the JOB template, 1–33);
  `validate_generator`: a template outside `1..=33` is `Invalid` with reason `"job generator
  template must be in 1..=33"`. Tests: `job_spec_parses`, `job_template_out_of_range_is_rejected`
  (0 and 34), `job_spec_hash_changes_with_template`.
- `materialize::dispatch`: `GeneratorSpec::Job { template } => run_job(*template, bench_name,
  out_dir, spec_hash)`; `run_job` locates DuckDB, `raw::ensure(&duckdb,
  &raw::default_raw_dir()?)`, builds a `JobGenerator` with `Target { out_dir, bench_name, tag:
  bench_name, spec_hash: Some(spec_hash) }`, calls `kermit_generate::run`.
- `main.rs`: `GenSubcommand::Job { #[arg(long)] template: u8, #[command(flatten)] target:
  GenTarget }`; `run_gen_job` checks the range, names `job-{template}-{tag}`, calls
  `target.prepare("job", &bench_name)`, then generates with `spec_hash: None`, `tag` = the
  flag.
- `benchmarks/job-{N}.yml` for N = 1…33: `name: job-N`, a one-line description, and
  `generator: { kind: job, template: N }`; same shape as `benchmarks/lubm-reference.yml`.

## J7: downloads and the real 113

- `raw::ensure`: open, else stage in `<dir>.fetch-<pid>`: `download_verified` `imdb.tgz` from
  `IMDB_URLS` in order (next mirror on any error; all failing is an error listing each), every
  `QUERY_FILES` entry from `query_url`, `extract_tgz`, `build` with `IMDB_TABLE_ROWS` and the
  provenance, then remove the staging dir (the tgz and CSVs go; the cache keeps
  `imdb.duckdb` and `queries/`).
- `kermit/tests/job_translate.rs` (network-gated: skip with a stderr line if the first fetch
  fails): fetch every `QUERY_FILES` entry with `download_verified` into a tempdir; for each
  template, `parse_query` every variant and `plan_template`; assert every rule parses as a
  `JoinQuery` and, after `kermit_algos::rewrite_placeholders`, passes
  `kermit_algos::check_attribute_order(&JoinQuery)`, every relation name matches
  `[a-z][a-z0-9_]*`, every variable starts uppercase, 466 relation files in total, and 113
  rules.

## J8: docs

As the spec's § Docs lists, plus: CLAUDE.md's Benchmark Reference Docs entry, `bench gen
job` in Build Commands, and one gotcha (DuckDB via the flake or `KERMIT_DUCKDB_BIN`; the
shared raw cache at `~/.cache/kermit/imdb/`; the streamed handoff; `ORDER BY id`; carried ids;
text-order variables). `benchmarks/README.md`: the `job` kind and the `generator.scale` row
no longer "required for every kind" (unless #96 landed that edit first).

## J9 (design session)

After `FULL_EXIT` and the user's go-ahead: generate all 33 templates; `bench run job-N -i all
-a all --verify` for each; record each template × structure's peak memory or the failure;
write JOB.md's table; final gate (`cargo fmt --all --check`, `RUSTFLAGS=-Dwarnings cargo
clippy --all-targets`, `cargo test`, `RUSTDOCFLAGS=-Dwarnings cargo doc --workspace
--no-deps`, all in `nix develop` with `CARGO_BUILD_JOBS=2`).
