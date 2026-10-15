# Names contract: push 1, the shared generator layer (#95)

Design session's contract for drafting `docs/superpowers/plans/2026-10-15-generate-layer.md`.
Spec: `docs/specs/2026-10-15-h2o-benchmark-design.md` at 41537a4 on `aidanb/h2o` (Part 1 and
D13–D17).

**Consumers.** #96 (SNAP) and #97 (JOB) take kermit-generate's type, function, module and
file names from this contract. Any change to it is committed and announced to the benchmark
co-ordinator session, which relays it. Read the spec's Part 1 first. This contract fixes every name, signature, file and
constant. A design decision it does not make goes back to the design session in your report;
do not invent one.

Worktree: `/tb/Source/Academia/kermit/.loom/worktrees/aidanb/h2o_18dea44a38dbca15`.
`BASE` = `4e0beac`. Branch `aidanb/h2o` (never switch, amend or push).

**Host rule for drafters:** do NOT run cargo (build, test, check, clippy, doc, fmt). A timing
run (#101 replicate chain) is live until `FULL_EXIT`. Read source only, including
`~/.cargo/registry/src/*/parquet-59.3.0/` and `arrow-*-59.3.0/` for exact APIs. No
filesystem-wide `find`.

## Stories

| Story | Title | Drafter |
|---|---|---|
| S1 | `ParquetWriter` and `write_parquet` in kermit-ds | drafter A |
| S2 | The `kermit-generate` crate: manifest, workspace wiring, `utc_iso8601_now`, `output_dir` | drafter B |
| S3 | The layer: `Generator`, `RelationSink`/`RelationWriter`, `GeneratedMeta`, `run`, toy-generator tests | drafter B |
| S4 | CLAUDE.md, then the final gate | design session |

S2 and S3 depend on S1 (`kermit_ds::ParquetWriter`). S3 depends on S2.

## Facts of the tree at BASE (verified 2026-10-15)

- `kermit-ds/src/relation.rs` (1047 lines). Its private `file_stem` becomes `pub(crate)` for
  `parquet_writer.rs` (S1). `RelationError { Csv, Io, Parquet, Arrow,
  InvalidData(String) }`, with `From` impls for each library error. `RelationHeader`
  derives `Clone, Debug, PartialEq, Eq` and has `new(name, attrs: Vec<String>)`, `name()`,
  `attrs() -> &[String]`, `arity()`, `is_nameless()`. A positional header has empty
  `attrs`. The private `fn file_stem(path: &Path) -> String` is at :428, `fn open_parquet` at
  :524, `pub fn read_parquet_header` at :543, `pub fn read_parquet` at :561 (returns
  `(RelationHeader, Tuples)`, Int64 columns only, rows ≤ `RowId::MAX`).
- **Name clash:** the `#[cfg(test)] mod tests` in relation.rs has a private helper
  `fn write_parquet(path: &Path, attrs: &[&str], rows: &[Vec<i64>])` (~:748). It writes raw
  i64 rows, including values `read_parquet` rejects. S1 renames it to
  `write_i64_parquet` and updates every call site in that module; it must not be replaced by
  the new writer.
- `kermit-ds/src/lib.rs` has `#![deny(missing_docs)]` and re-exports through one `pub use
  { … relation::{ read_csv, read_csv_header, read_parquet, read_parquet_header, … } … }`
  block, with `kermit_iters::{LeafCursor, LeafRows, RowId, Tuples}`.
- kermit-ds deps: `arrow = "59.3.0"`, `parquet = "59.3.0"`; dev `tempfile = "3"`.
- `kermit_iters::RowId` = `u32`. `Tuples`: `new(arity)`, `with_capacity(arity, rows)`,
  `from_flat(arity, len, data: Vec<usize>)` (panics above `RowId::MAX`), `arity()`,
  `len()`, `is_empty()`, `rows()`, `push(&[usize])`, `as_flat()`, `sort()`, `to_vecs()`,
  `From<Vec<Vec<usize>>>`.
- kermit-rdf: `src/lib.rs` declares `mod timestamp;` (private).
  `src/timestamp.rs:19` has `pub(crate) fn utc_iso8601_now() -> String`. `generator.rs`:
  `pub struct Target<'a> { out_dir: &'a Path, bench_name: &'a str, tag: &'a str,
  spec_hash: Option<&'a str> }` (pub fields, `Clone, Copy`), `pub trait GeneratorMeta:
  Serialize + DeserializeOwned { schema_version, kind, tag, generated_at_utc,
  relation_count, query_count, spec_hash }` (exact signatures at :141–157),
  `pub struct MetaHeader { schema_version, kind, spec_hash }` with `MetaHeader::read(out_dir)`,
  `pub fn meta_json_path(out_dir) -> PathBuf`, `pub fn write_meta_json<M: GeneratorMeta>(out_dir,
  &M) -> Result<(), RdfError>`, `pub const META_SCHEMA_VERSION: u32 = 4`. RDF pipelines
  write `file://{out_dir.canonicalize()}` base URLs (generator.rs:334) and
  `<out_dir>/benchmark.yml`.
- kermit-bench: `BenchmarkDefinition { name, description, relations: Vec<RelationSource>,
  queries: Vec<QueryDefinition>, generator: Option<GeneratorSpec> }` with
  `validate() -> Result<(), BenchError>`. `RelationSource { name, url: Option<String>, path:
  Option<String>, sha256: Option<String> }`. `QueryDefinition { name, description, query,
  expected: Option<u64> }`. `cache::sha256_hex(&Path) -> Result<String, BenchError>`,
  `cache::base_cache_dir() -> Result<PathBuf, BenchError>`. `discovery::load_all_benchmarks_with_cache(
  workspace_root, cache_root)`; a workspace root without `benchmarks/` yields no workspace
  entries.
- kermit-parser: `JoinQuery: FromStr<Err = ParseError>`, `JoinQuery { head: Predicate, body:
  Vec<Predicate> }`, `Predicate { name: String, terms: Vec<Term> }`. Identifiers:
  `[A-Za-z][A-Za-z0-9_]*`. The reserved prefixes `Const_`, `Select_` and `Index_` all start
  uppercase.
- Root `Cargo.toml`: `members = [...]` (8 crates) and a `[patch.crates-io]` table listing
  every crate by path. Crate manifests use `authors.workspace = true`, `edition`,
  `homepage`, `license`, `publish`, `readme` and `repository`, all `.workspace = true`
  (copy kermit-rdf's `[package]` block).
- The binary's `kermit/Cargo.toml` `[dependencies]` lists the internal crates with
  `{ version, path }`.

## S1: `ParquetWriter` and `write_parquet` (kermit-ds)

**New file** `kermit-ds/src/parquet_writer.rs` (`mod parquet_writer;` in lib.rs), as the spec
says since 41537a4: a new module keeps the 1047-line `relation.rs` from growing. Re-export `parquet_writer::{write_parquet,
ParquetWriter}` from the crate root, beside `read_parquet`.

```rust
/// Rows per chunk handed to `ArrowWriter`; the writer's only buffering.
pub(crate) const WRITE_CHUNK_ROWS: usize = 1 << 16;
/// Rows per row group; a multiple of `WRITE_CHUNK_ROWS`.
pub(crate) const ROW_GROUP_ROWS: usize = 1 << 20;

pub struct ParquetWriter {
    writer: Option<ArrowWriter<File>>,   // None once finished or dropped
    schema: SchemaRef,
    columns: Vec<Vec<i64>>,              // one chunk buffer per column, capacity WRITE_CHUNK_ROWS
    buffered: usize,                     // rows in `columns`
    rows: u64,                           // rows accepted so far (all batches)
    path: PathBuf,                       // final path
    part: PathBuf,                       // `<path>.part` (path with ".part" appended to the file name)
    failed: bool,                        // set by any error; later calls return InvalidData
}

impl ParquetWriter {
    pub fn create<P: AsRef<Path>>(path: P, header: &RelationHeader) -> Result<Self, RelationError>;
    pub fn write_batch(&mut self, batch: &Tuples) -> Result<(), RelationError>;
    pub fn finish(self) -> Result<u64, RelationError>;
}
impl Drop for ParquetWriter { /* if writer is Some: drop it, then remove `part`, ignoring errors */ }

pub fn write_parquet<P: AsRef<Path>>(
    path: P, header: &RelationHeader, tuples: &Tuples,
) -> Result<(), RelationError>;  // create, one write_batch, finish
```

Rules, all errors `RelationError::InvalidData(msg)` unless stated:
- `create`:
  - errors if `header.attrs().is_empty()` (positional or arity 0; Parquet needs one named
    column per attribute), and if `file_stem(path) != header.name()` (message names both);
  - schema: one `Field::new(attr, DataType::Int64, false)` per attribute;
  - properties: `WriterProperties::builder().set_max_row_group_row_count(Some(ROW_GROUP_ROWS))
    .build()`. `set_max_row_group_size` is deprecated since parquet 58.0.0
    (`parquet-59.3.0/src/file/properties.rs:741`) and would fail clippy under `-Dwarnings`;
  - creates the `.part` file (truncating any leftover one).
- `write_batch`:
  - an empty batch is a no-op at any arity (as `Relation::from_tuples` accepts an empty
    batch under any header);
  - errors if a non-empty `batch.arity() != header arity`, or if `self.failed`;
  - errors if `rows + batch.len() > RowId::MAX as u64`;
  - converts each value with `i64::try_from` (error message `"row {r}, column {c}: {v}
    exceeds i64::MAX"`, `r` counted across all batches);
  - pushes values row by row into `columns`, flushing a `RecordBatch` of exactly
    `WRITE_CHUNK_ROWS` rows to `ArrowWriter` each time `buffered` reaches it;
  - sets `failed = true` before returning any error.
- `finish`: flushes the remainder (if `buffered > 0`), `close()`s the writer, renames `part`
  → `path` (`std::fs::rename`), returns `rows`. Any error leaves no final file (Drop removes
  `.part`).
- The doc comment states the four guarantees from the spec: the exact inverse of
  `read_parquet`; bytes independent of batching (and why: page cuts are counted from each
  input batch's start, so the writer re-chunks); no partial files; bytes stable for a fixed
  build only (`created_by`).

**S1 tests** (inline `#[cfg(test)] mod tests` in parquet_writer.rs, `tempfile`):
- `round_trips_arities_1_2_3`, plus `round_trips_zero_rows`:
  `read_parquet(write_parquet(..)) == (header, tuples)`;
- `rejects_positional_header`, `rejects_stem_mismatch`, `rejects_batch_arity_mismatch`,
  `rejects_value_above_i64_max` (use `usize::MAX`);
- `batching_does_not_change_bytes`: the same rows (arity 2, `2 * WRITE_CHUNK_ROWS + 3` rows)
  written one-shot and in batch sizes 1, 7, 1000, `WRITE_CHUNK_ROWS - 1` and
  `WRITE_CHUNK_ROWS + 1`; every file's bytes are equal;
- `crosses_a_row_group_boundary`: arity 1, `ROW_GROUP_ROWS + 3` rows; round-trips, and the
  footer has 2 row groups (`ParquetRecordBatchReaderBuilder` metadata);
- `dropped_writer_leaves_no_file`: after `create` + one batch + drop, neither `path` nor
  `.part` exists;
- `failed_writer_refuses_more_batches`;
- plus relation.rs's renamed helper (`write_i64_parquet`) with every call site updated.

## S2: the `kermit-generate` crate

Files: `Cargo.toml` (root: add `"kermit-generate"` to `members` and
`kermit-generate = { path = "kermit-generate" }` to `[patch.crates-io]`),
`kermit-generate/Cargo.toml`, `kermit-generate/src/lib.rs`,
`kermit-generate/src/output_dir.rs`, `kermit-rdf/src/lib.rs` + `kermit-rdf/src/timestamp.rs`
(visibility), `kermit/Cargo.toml` (dependency).

`kermit-generate/Cargo.toml`:
```toml
[package]
name = "kermit-generate"
description = "Non-RDF benchmark generators for Kermit and the layer they share"
version = "0.1.0"
authors.workspace = true
edition.workspace = true
homepage.workspace = true
license.workspace = true
publish.workspace = true
readme.workspace = true
repository.workspace = true

[dependencies]
kermit-bench = { version = "0.1.2", path = "../kermit-bench" }
kermit-ds = { version = "0.2.0", path = "../kermit-ds" }
kermit-parser = { version = "0.0.3", path = "../kermit-parser" }
kermit-rdf = { version = "0.1.1", path = "../kermit-rdf" }
anyhow = "1.0"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
serde_yaml = "0.9"

[dev-dependencies]
tempfile = "3"
```
(Verify each internal crate's current `version` in its manifest.)

`kermit/Cargo.toml` `[dependencies]` gains `kermit-generate = { version = "0.1.0", path =
"../kermit-generate" }`, unused until push 2.

kermit-rdf: `mod timestamp;` → `pub mod timestamp;`, and `pub(crate) fn utc_iso8601_now` →
`pub fn utc_iso8601_now`. The module doc already exists; make sure `#![deny(missing_docs)]`
is still satisfied.

`kermit-generate/src/lib.rs`:
- crate docs, `#![deny(missing_docs)]`;
- `mod generator; mod meta; mod output_dir; mod run; mod sink;` (S3 adds four of them; S2
  adds only `output_dir` and the `pub use`s it can);
- `pub use { generator::{Generated, Generator}, meta::GeneratedMeta, output_dir::{output_dir,
  OutputDir}, run::run, sink::{RelationRecord, RelationSink, RelationWriter} };`
- the crate docs explain, without intra-doc links to binary items:
  - what the crate is: the layer #95/#96/#97 share, plus their generators as `pub mod
    h2o;` / `snap` / `job` (added by those issues);
  - how `Generator` relates to `kermit_rdf::generator::Generator`: that one drives external
    RDF tools and partitions N-Triples; this one writes relations directly. kermit-rdf
    cannot host it, because it must not depend on kermit-ds;
  - the cache contract: `run` deletes the old `meta.json`, writes relations as `.part` then
    renames them, writes `benchmark.yml`, and writes `meta.json` last as the commit marker;
  - that the binary's `materialize::dispatch` arms discard `run`'s `GeneratedMeta`, which
    `bench gen` may print a summary from (plain text, no link).

`kermit-generate/src/output_dir.rs`:
```rust
/// Where `bench gen <kind>` writes one benchmark.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputDir {
    /// `<parent>/<bench_name>`.
    pub path: PathBuf,
    /// Whether the parent is the cache root that `bench list/fetch/run` scan.
    pub discovered: bool,
}

/// `<parent or kermit_bench::cache::base_cache_dir()>/<bench_name>`.
pub fn output_dir(bench_name: &str, parent: Option<&Path>) -> anyhow::Result<OutputDir>;
```
It is pure: no filesystem writes, no output. With `parent: None` the parent is
`base_cache_dir()?` (the one error: no platform cache dir) and `discovered` is `true`. With
`Some(dir)` it never fails: `discovered` is `base_cache_dir().is_ok_and(|b| b == dir)`. This
is a total function at a public boundary (CLAUDE.md priority 5), unlike `run_gen_lubm`,
which needs a cache dir even under `--output-dir`.

**S2 tests** (inline): `explicit_parent_is_not_discovered`,
`default_parent_is_the_cache_root`, `explicit_cache_root_is_discovered`.

## S3: the layer

Files: `kermit-generate/src/{generator.rs, sink.rs, meta.rs, run.rs}`, `lib.rs` (`mod`s and
`pub use`s), `kermit-generate/tests/run.rs` (integration tests through the public API).

`generator.rs`:
```rust
pub trait Generator {
    const KIND: &'static str;
    type Extras: Serialize + DeserializeOwned;
    fn target(&self) -> kermit_rdf::generator::Target<'_>;
    fn generate(&self, sink: &mut RelationSink) -> anyhow::Result<Generated<Self::Extras>>;
}

pub struct Generated<E> {
    pub description: String,
    pub queries: Vec<kermit_bench::QueryDefinition>,
    pub extras: E,
}
```
(Check `QueryDefinition`'s export path from kermit-bench's lib.rs.)

`sink.rs`:
```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelationRecord { pub name: String, pub columns: Vec<String>, pub rows: u64, pub sha256: String }

pub struct RelationSink { out_dir: PathBuf, records: Vec<RelationRecord> }
impl RelationSink {
    pub(crate) fn new(out_dir: &Path) -> Self;
    pub fn open(&mut self, header: &RelationHeader) -> anyhow::Result<RelationWriter<'_>>;
    pub fn write(&mut self, header: &RelationHeader, tuples: &Tuples) -> anyhow::Result<()>;
    pub fn raw_dir(&self) -> anyhow::Result<PathBuf>;     // `<out_dir>/raw`, create_dir_all
    pub fn records(&self) -> &[RelationRecord];
    pub(crate) fn into_records(self) -> Vec<RelationRecord>;
}

pub struct RelationWriter<'a> { sink: &'a mut RelationSink, writer: ParquetWriter, header: RelationHeader, path: PathBuf }
impl RelationWriter<'_> {
    pub fn write_batch(&mut self, batch: &Tuples) -> anyhow::Result<()>;
    pub fn finish(self) -> anyhow::Result<()>;   // writer.finish → sha256_hex(path) → push record
}
```
- `open` errors, before creating any file:
  - if `header.name()` does not match `^[a-z][a-z0-9_]*$` (checked by hand, no regex
    crate). This lowercase rule is the spec's (since 41537a4). It is stricter than the
    parser's identifiers and therefore excludes every reserved prefix (`Const_`, `Select_`,
    `Index_`) too;
  - if a record with that name already exists.

  It then calls `ParquetWriter::create(out_dir.join(format!("{name}.parquet")), header)`.
- `write` = `let mut w = self.open(header)?; w.write_batch(tuples)?; w.finish()`.
- A `RelationWriter` dropped without `finish` records nothing (the `ParquetWriter`'s Drop
  removes `.part`).

`meta.rs`:
```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]   // no `Eq`: `E` may be `serde_json::Value`
pub struct GeneratedMeta<E> {
    pub schema_version: u32,
    pub kind: String,
    pub tag: String,
    pub generated_at_utc: String,
    pub relation_count: u32,
    pub query_count: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spec_hash: Option<String>,
    pub relations: Vec<RelationRecord>,
    #[serde(flatten)]
    pub extras: E,
}
impl<E: Serialize + DeserializeOwned> kermit_rdf::generator::GeneratorMeta for GeneratedMeta<E> { … }

/// The keys `GeneratedMeta` writes itself; `Extras` may use none of them.
pub(crate) const COMMON_FIELDS: [&str; 8] = ["schema_version", "kind", "tag",
    "generated_at_utc", "relation_count", "query_count", "spec_hash", "relations"];
```
serde_derive infers `E: Serialize` / `E: Deserialize<'de>` for the flattened field, so no
`#[serde(bound)]` is needed.

`run.rs`:
```rust
pub fn run<G: Generator>(generator: &G) -> anyhow::Result<GeneratedMeta<G::Extras>>;
fn check_extras<E: Serialize>(extras: &E) -> anyhow::Result<()>;
fn check_queries(queries: &[QueryDefinition], records: &[RelationRecord]) -> anyhow::Result<()>;
fn check_target(out_dir: &Path, bench_name: &str) -> anyhow::Result<()>;   // push 1b
```
`run`'s order is fixed by the spec's steps 0–6:
0. (push 1b) `check_target`: `bench_name` is non-empty and `out_dir.file_name() ==
   Some(bench_name)`, checked before anything is touched.
1. `create_dir_all(out_dir)`, then remove `meta_json_path(out_dir)`, ignoring `NotFound`.
2. `generate(&mut RelationSink::new(out_dir))`.
3. `check_extras`: `serde_json::to_value` must be `Value::Object`, with no key in
   `COMMON_FIELDS` (the error names the key).
4. `check_queries`: query names are unique, and each `query` parses as `JoinQuery`, whose
   every `body` predicate `name` is a record name with `terms.len() == record.columns.len()`
   (the arity part is push 1b). Errors name the query.
5. Build a `BenchmarkDefinition`:
   - `name = target.bench_name`;
   - `description = generated.description`;
   - one `RelationSource` per record, in record order: `url =
     Some(format!("file://{}/{}.parquet", out_dir.canonicalize()?.display(), name))`,
     `path: None`, `sha256: Some(..)`;
   - `queries`;
   - `generator: None`.

   Then `validate()` it and write it with `serde_yaml::to_string` to
   `<out_dir>/benchmark.yml`.
6. Build the `GeneratedMeta`:
   - `schema_version = META_SCHEMA_VERSION`, `kind = G::KIND`;
   - `tag`, and `spec_hash` from the target;
   - `generated_at_utc = kermit_rdf::timestamp::utc_iso8601_now()`;
   - counts via `u32::try_from`;
   - `relations = sink.into_records()`.

   Then `write_meta_json`, and return the meta.

**S3 tests.**
- Inline in sink.rs:
  - `rejects_uppercase_and_reserved_names` (`Edge`, `Const_x`, `1x`, `x-y`, empty);
  - `rejects_duplicate_names`;
  - `record_hash_matches_file`;
  - `raw_dir_is_created_on_demand` (absent before the call);
  - `dropped_relation_writer_records_nothing`.
- Inline in meta.rs: `common_fields_match_serialized_keys`. Serialize a
  `GeneratedMeta<Empty>` (an `#[derive(Serialize, Deserialize)] struct Empty {}`) with
  `spec_hash: Some`, and assert its key set equals `COMMON_FIELDS`.
- Integration, `kermit-generate/tests/run.rs`. A `ToyGenerator` (configurable: `fail:
  bool`, `bad_query: bool`, `unknown_relation: bool`, and the extras as a
  `serde_json::Value`) writes `edge(src, dst)` one-shot and `node(id)` streamed in 3
  batches, with one query `Q(X, Y) :- edge(X, Y), node(X).` and `expected: Some(n)`.
  - `run_writes_a_discoverable_cache`: `benchmark.yml` parses into a `BenchmarkDefinition`
    that validates, with `file://` URLs whose paths exist and `sha256` equal to
    `sha256_hex`; `MetaHeader::read` gives `kind == "toy"`, `spec_hash`, and
    `META_SCHEMA_VERSION`; `read_meta_json::<GeneratedMeta<ToyExtras>>` round-trips the
    extras; `load_all_benchmarks_with_cache(empty_workspace, cache_root)` finds the bench.
  - `failed_generate_writes_no_meta`, `bad_query_writes_no_meta`,
    `unknown_relation_writes_no_meta`, `colliding_extras_write_no_meta` (extras
    `{"relations": 1}`) and `non_object_extras_write_no_meta` (extras `42`). In each, a
    stale `meta.json` is planted first and must be gone afterwards.
  - `rows_and_relation_count_are_recorded`.

## S4: CLAUDE.md (design session)

- line 7: "8 crates" → "9 crates";
- Workspace Architecture: a `kermit-generate` row (after `kermit-rdf`);
- the `kermit` row: unchanged in push 1 (`GenTarget` comes later);
- Dependency flow: `kermit-ds`/`kermit-bench`/`kermit-rdf`/`kermit-parser` →
  `kermit-generate`. "`kermit-parser` is consumed by `kermit-algos` only" becomes "by
  `kermit-algos` and `kermit-generate`". The leaves are unchanged;
- a gotcha: non-RDF generators live in `kermit-generate` (the cache contract, the
  generator dependencies in its manifest), with the D16 relation to kermit-rdf's trait.

The final gate (after FULL_EXIT only):
- `cargo fmt --all --check`;
- `RUSTFLAGS=-Dwarnings cargo clippy --all-targets`;
- `cargo test`;
- `RUSTDOCFLAGS=-Dwarnings cargo doc --workspace --no-deps`.

All run in `nix develop` with `CARGO_BUILD_JOBS=2`.
