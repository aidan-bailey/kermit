# H2O Join Benchmark and the Shared Non-RDF Generator Layer

**Date:** 2026-10-15
**Status:** Design approved in conversation (2026-10-15), section by section. It was amended
the same day (D13–D17) after the benchmark co-ordinator session cross-checked it against
#96's and #97's specs. The user then moved the layer into its own crate (D13).
**Scope:** Issue #95: the H2O.ai db-benchmark join tasks ("J1") as a generator benchmark,
`generator: { kind: h2o, … }`. It also covers the shared generator layer, a new workspace
crate `kermit-generate`, that #95, #96 (SNAP) and #97 (JOB) all build on. That layer is
designed here against all three issues' requirements and lands on master first, as its own
push.
**Related:**
- #96 (SNAP) and #97 (JOB): the layer's other two consumers. Their sessions sent their
  requirements on 2026-10-15; the table under "Requirements from #96 and #97" traces each
  one.
- #111: flat tuple batches. It made `read_parquet` return one `Tuples` buffer, which made
  #95's "known limit" obsolete.
- #101: input-order effects on HashTrie, the reason for the `order` field.
- #74: a generator's output must be a pure function of its spec.
- #80: seek strategies, which q5 separates.

## Summary

Two pieces of work:

1. **The layer.** A new streaming Parquet writer in `kermit-ds` (`ParquetWriter`, with
   `write_parquet` as a wrapper), the exact inverse of `read_parquet`. On top of it, a
   `Generator` trait in a new workspace crate, `kermit-generate` (D13), with a
   `RelationSink` that writes and hashes each relation as it arrives, and `run`, which
   writes `benchmark.yml` and then, last, `meta.json`. Every generated cache
   satisfies the existing contract, so discovery, spec drift, `--force`, `OUTDATED_BEFORE`
   and `bench list` work unchanged.
2. **H2O.** `GeneratorSpec::H2o { scale, seed, order }`, a reimplementation of
   db-benchmark's `_data/join-datagen.R` that keeps its structure and uses in-repo random
   numbers. It produces six per-query projected relations and three queries (q1, q2, q5),
   each with an exact `expected`. Also `bench gen h2o` and the committed `benchmarks/h2o.yml`
   and `benchmarks/h2o-sorted.yml` at N = 1e7.

## Motivation

See #95 for the full case. In short, H2O gives the thesis:
- a relational baseline: acyclic foreign-key/primary-key joins, the classic hash-join
  workload;
- targeted workloads: q1 is almost pure leaf enumeration, and q5 intersects two dense key
  lists that overlap by 90%, which separates the seek strategies;
- a scaling knob, N;
- with `order`, a controlled input-order axis on synthetic data, for #101's locality work.

## Decisions

Made in this design, including where it departs from #95's body:

| # | Decision | Versus #95 |
|---|---|---|
| D1 | The H2O session builds the shared layer against #95/#96/#97's needs and lands it on master first; the snap and job sessions merge it. | Replaces "whichever of #95 and #96 lands first". |
| D2 | `order: shuffled \| sorted` spec field, default `shuffled`. `sorted` = each relation's rows in `Tuples::sort` order. The draws are the same in both modes. | New. |
| D3 | Layer shape: a trait plus a streaming sink, with `kermit_generate::run` owning `benchmark.yml` and `meta.json` (approach A, chosen over free helpers). | Specified. |
| D4 | The writer takes `&RelationHeader, &Tuples` and is the exact inverse of `read_parquet`. | Replaces #95's column-major `columns`. Chosen by the user in #96's session and confirmed here. |
| D5 | The writer streams (`create` / `write_batch` / `finish`), with the one-shot forms as wrappers. A file's bytes don't depend on how its rows were batched. | New. Chosen by the user in #97's session and confirmed here. |
| D6 | `RelationSink::raw_dir()` for raw artifacts. | New, for #96. |
| D7 | The "known limit" is rewritten from measurements: since #111, loading costs 8 B per column per row. | Replaces #95's 56 B/tuple extrapolation. |
| D8 | The golden test hashes each relation's data, not its file bytes. | Refines #95's "golden SHA-256 of a small plan's output". |
| D9 | `benchmark.yml` carries every relation's `sha256`, so `bench fetch` verifies generated caches. | New. |
| D10 | Random numbers: SplitMix64, Lemire bounded draws, Fisher–Yates, one stream per (column, step). | Settles #95's "SplitMix64- or xoshiro-class". |
| D11 | The end-to-end test is an integration test, `kermit/tests/h2o_generate.rs`. It counts through `kermit::db`'s entry points, as `lubm_cardinalities.rs`, `watdiv_correctness.rs` and #96's and #97's tests do, because the 1e7 floor stops any small `bench gen h2o`. The `ExecutionFamily::load` path that `bench run` takes is covered by the manual `--verify` acceptance at 1e7. The cache test is a unit test in `materialize.rs`, because `materialize` is a binary module. | Refines #95. |
| D12 | `benchmarks/h2o-sorted.yml` is committed beside `h2o.yml`. | New, follows from D2. |
| D13 | The layer is a **new workspace crate, `kermit-generate`** (the user's decision; it replaced an earlier "module of the `kermit` library" placement). The `kermit` library stays the join engine's interface. The binary depends on `kermit-generate`, and `materialize` calls `kermit_generate::run`. Push 1 lands the layer with no consumer: in a binary every unused item would be dead code, which CI's `-Dwarnings` rejects, but a library crate's `pub` items are not dead. Its docs are linted by CI's `cargo doc --workspace`. This mirrors `kermit-rdf`: generators in their own library, with the binary dispatching. | New. |
| D14 | `kermit_generate::run` enforces that `Extras` serializes to a JSON object whose keys don't overlap the common `GeneratedMeta` fields. `serde(flatten)` would otherwise let a duplicate key through silently. | New (co-ordinator). |
| D15 | Every non-RDF `bench gen <kind>` takes lubm's flags, `--tag` (required) and `--output-dir` (optional), through one shared clap `GenTarget` struct in the binary, whose code this spec gives verbatim. `lubm` and `watdiv` are not migrated. | New (co-ordinator). |
| D17 | Push 1 carries `GenTarget`'s pure resolver, `kermit_generate::output_dir`, tested and therefore not dead. The clap struct lands with whichever comes first, push 2 or #96's phase 2, copied verbatim from this spec; the later one merges and drops its own copy. #96 therefore waits only for push 1. | New (user, after the co-ordinator flagged the ordering hazard). |
| D16 | The trait keeps the name `Generator`, which #96's and #97's specs cite. The crate docs say how it relates to `kermit_rdf::generator::Generator`. | New (co-ordinator). |

## Part 1: the shared layer

### `ParquetWriter` and `write_parquet` (kermit-ds)

These go in a new `kermit-ds/src/parquet_writer.rs` (`relation.rs`, which holds
`read_parquet`, is already 1,047 lines), and are re-exported at the crate root beside
`read_parquet`.

```rust
pub struct ParquetWriter { /* ArrowWriter<File>, column chunk buffers, row count, paths */ }

impl ParquetWriter {
    /// Opens `<path>.part` for writing. Errors if `path`'s file stem differs from
    /// `header.name()`, or if `header` has arity 0.
    pub fn create<P: AsRef<Path>>(path: P, header: &RelationHeader) -> Result<Self, RelationError>;
    /// Appends `batch`'s rows. Errors if its arity differs from the header's, if a value
    /// exceeds `i64::MAX`, or if the running total would exceed `RowId::MAX`.
    pub fn write_batch(&mut self, batch: &Tuples) -> Result<(), RelationError>;
    /// Flushes, closes, renames `<path>.part` to `path`, and returns the row count.
    pub fn finish(self) -> Result<u64, RelationError>;
}
// Drop without finish: close and remove `<path>.part`, ignoring errors.

pub fn write_parquet<P: AsRef<Path>>(path: P, header: &RelationHeader, tuples: &Tuples)
    -> Result<(), RelationError>; // = create + one write_batch + finish
```

- **The exact inverse.** One non-nullable Int64 column per attribute, named as in the
  header. Parquet stores only the columns, and `read_parquet` names the relation after the
  file stem. That is why `create` requires the stem to equal `header.name()`: every
  successful write then satisfies
  `read_parquet(path) == (header, tuples)`.
- **Bytes independent of batching, by construction.** parquet-rs's `ArrowWriter` fixes
  row-group boundaries, but it decides where data pages are cut at checkpoints counted from
  the start of each input batch, so different batchings can produce different bytes.
  `ParquetWriter` therefore transposes caller rows into its own fixed-size column buffers
  of `WRITE_CHUNK_ROWS` (2¹⁶) rows. It hands `ArrowWriter` only full chunks, plus the
  remainder at `finish`. `max_row_group_size` is pinned to 2²⁰, a multiple of the chunk
  size, so a chunk never straddles a row group. `ArrowWriter`'s input then depends only on
  the row sequence. This also bounds the writer's extra memory at one chunk, whatever the
  caller's batch size.
- **No partial files.** Writing to `.part` and renaming on `finish` means a relation only
  appears under its real name once it is complete. A writer dropped mid-stream (#97's
  DuckDB child dying) leaves nothing behind.
- File bytes are stable for a fixed build, but not across parquet crate versions, because
  the footer records `created_by`. Provenance hashes in `meta.json` describe one cache;
  tests that pin generated *data* hash the `Tuples` buffers (D8).
- `kermit-rdf`'s own Parquet writer is left alone (scope discipline).

### The `kermit-generate` crate: trait, sink, run

A new workspace crate (D13), added to the root `Cargo.toml`'s `members`.

**Dependencies.**
- On its own manifest: `kermit-ds` (`ParquetWriter`, `RelationHeader`, `Tuples`);
  `kermit-bench` (`BenchmarkDefinition`, `QueryDefinition`, `RelationSource`,
  `cache::sha256_hex`, `cache::base_cache_dir`, and later `H2oOrder`); `kermit-rdf`
  (`generator::{Target, GeneratorMeta, write_meta_json, META_SCHEMA_VERSION}`,
  `utc_iso8601_now`); `kermit-parser` (`JoinQuery`, for `run`'s query check); `serde`,
  `serde_json`, `serde_yaml` and `anyhow`. Dev: `tempfile`.
- `kermit/Cargo.toml` gains `kermit-generate` in push 1, so neither push 2 nor #96 has to
  add it. An unused dependency is not a warning.
- Each generator's own dependencies go in `kermit-generate/Cargo.toml`, never in
  `kermit/Cargo.toml`: #96's `flate2`, #97's `sqlparser`. H2O needs none.
- The flow becomes `kermit-ds`, `kermit-bench`, `kermit-rdf`, `kermit-parser` →
  `kermit-generate` → `kermit`. `kermit-rdf` still depends only on `kermit-bench`,
  `kermit-bench` stays a leaf, and `kermit-generate` cannot depend on `kermit`, which would
  be a cycle.

**Layout**, the paths #96 and #97 copy:

```text
kermit-generate/
  Cargo.toml
  src/
    lib.rs          crate docs, #![deny(missing_docs)], `pub mod`s and re-exports
    generator.rs    Generator, Generated
    sink.rs         RelationSink, RelationWriter, RelationRecord
    meta.rs         GeneratedMeta
    run.rs          run (with the extras and query checks)
    output_dir.rs   output_dir, OutputDir (D17)
    h2o/            mod.rs, keys.rs, rng.rs                  (push 2, #95)
    snap.rs, snap/counts.rs                                  (#96)
    job/            mod.rs, translate.rs, duckdb.rs, raw.rs  (#97; its own spec's list governs)
```

`lib.rs` re-exports the layer at the crate root: `kermit_generate::{Generator, Generated,
RelationSink, RelationWriter, RelationRecord, GeneratedMeta, run, output_dir, OutputDir}`.
Each generator adds one `pub mod <kind>;` line, so the three branches meet only one-line
add/add conflicts.

**Crate docs** (`#![deny(missing_docs)]`, like the seven existing library crates). They
have no intra-doc links to binary items such as `materialize` or `ExecutionFamily`, which
`cargo doc` cannot reach. They:
- explain how `Generator` relates to `kermit_rdf::generator::Generator` (D16). That trait
  drives external RDF tools and partitions their N-Triples; this one writes relations
  directly. kermit-rdf cannot host it, because it must not depend on kermit-ds.
- note that the binary's `dispatch` arms discard `run`'s returned `GeneratedMeta`, which
  `bench gen` may print a summary from.

```rust
pub trait Generator {
    /// `meta.json`'s `kind` ("h2o", "snap", "job").
    const KIND: &'static str;
    /// This generator's own `meta.json` fields, flattened beside the common ones.
    type Extras: Serialize + DeserializeOwned;
    /// Output directory, benchmark name, tag and spec hash.
    fn target(&self) -> kermit_rdf::generator::Target<'_>;
    /// Writes every relation through `sink` and returns the queries.
    fn generate(&self, sink: &mut RelationSink) -> anyhow::Result<Generated<Self::Extras>>;
}

pub struct Generated<E> {
    pub description: String,             // benchmark.yml's description
    pub queries: Vec<QueryDefinition>,   // each with `expected: Option<u64>`
    pub extras: E,
}

pub struct RelationSink { /* out_dir, Vec<RelationRecord> */ }
impl RelationSink {
    pub fn open(&mut self, header: &RelationHeader) -> anyhow::Result<RelationWriter<'_>>;
    pub fn write(&mut self, header: &RelationHeader, tuples: &Tuples) -> anyhow::Result<()>;
    /// `<out_dir>/raw/`, created on first call.
    pub fn raw_dir(&self) -> anyhow::Result<PathBuf>;
}

pub struct RelationWriter<'a> { /* &'a mut RelationSink, ParquetWriter, header */ }
impl RelationWriter<'_> {
    pub fn write_batch(&mut self, batch: &Tuples) -> anyhow::Result<()>;
    /// Finishes the file, hashes it and records it in the sink.
    pub fn finish(self) -> anyhow::Result<()>;
}

#[derive(Serialize, Deserialize)]
pub struct RelationRecord { pub name: String, pub columns: Vec<String>, pub rows: u64, pub sha256: String }

pub fn run<G: Generator>(generator: &G) -> anyhow::Result<GeneratedMeta<G::Extras>>;
```

**`RelationSink`**
- `open` rejects a name that is already written, or that does not match
  `[a-z][a-z0-9_]*`. That rule is stricter than kermit-parser's identifiers
  (`[A-Za-z][A-Za-z0-9_]*`) and excludes every reserved prefix (`Const_`, `Select_`,
  `Index_`), all of which start uppercase. It creates
  `ParquetWriter::create(<out_dir>/<name>.parquet, header)`.
- `write` is `open` + one `write_batch` + `finish`, so there is one code path.
- `RelationWriter` borrows the sink mutably, so only one relation can be open at a time.
- `finish` hashes the finished file with `kermit_bench::cache::sha256_hex`, the encoding
  `bench fetch` compares against, and pushes a `RelationRecord`. A writer dropped without
  `finish` records nothing.

**`kermit_generate::run`**
0. Checks the target, before anything is touched (Amendment 1): `out_dir` must end in a
   component equal to `bench_name`, which is where discovery, `materialize` and
   `kermit_bench::cache::relation_cache_path` look for the cache.
1. Creates `out_dir` and deletes any existing `meta.json`, so that a failed run can never
   leave an old commit marker beside new files.
2. Calls `generate` with a fresh sink.
3. Checks the extras (D14): `serde_json::to_value(&extras)` must be an object, and none of
   its keys may be a common `GeneratedMeta` field (`schema_version`, `kind`, `tag`,
   `generated_at_utc`, `relation_count`, `query_count`, `spec_hash`, `relations`). An
   overlap is an error naming the key.
4. Checks the queries: names are unique, each `query` parses as a `kermit_parser::JoinQuery`,
   and each body predicate names a recorded relation, with one term per column (Amendment
   1). A malformed rule therefore fails at generation time, not at `bench run`, and no
   cache is committed for it.
5. Builds a `BenchmarkDefinition`:
   - `name`: the target's bench name;
   - `description`: from `Generated`;
   - one `RelationSource` per record, in write order: `url: file://<out_dir>/<name>.parquet`,
     `path: None`, `sha256: Some(..)` (D9);
   - `queries`;
   - `generator: None`, as the RDF pipelines write it.

   It calls `validate()` and writes `benchmark.yml`.
6. Writes `meta.json` last, through `kermit_rdf::generator::write_meta_json`, and returns
   the meta.

### `GeneratedMeta<E>`

```rust
#[derive(Serialize, Deserialize)]
pub struct GeneratedMeta<E> {
    pub schema_version: u32,            // META_SCHEMA_VERSION
    pub kind: String,                   // G::KIND
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
impl<E: Serialize + DeserializeOwned> GeneratorMeta for GeneratedMeta<E> { … }
```

- `MetaHeader::read` already ignores unknown fields, so drift detection, `outdated_reason`
  and `bench list` read every kind's file unchanged.
- An `Extras` type must not reuse a common field's name, because flattening would collide.
  `kermit_generate::run` enforces this (step 3, D14), and a toy-generator test pins it.
- The timestamp comes from kermit-rdf's `utc_iso8601_now`, made public (today it is
  `pub(crate)` in the private `timestamp` module). That one-line change is this design's
  only edit to kermit-rdf.
- A future output change for one kind bumps the shared `META_SCHEMA_VERSION` and adds that
  kind's floor to `OUTDATED_BEFORE`, the existing rule. While #95, #96 and #97 are in
  flight, bumps and floors go through the benchmark co-ordinator session, so each lands
  once.

### `output_dir`: `GenTarget`'s resolver (D17)

```rust
/// Where `bench gen <kind>` writes the benchmark `bench_name`.
pub struct OutputDir {
    /// `<parent>/<bench_name>`.
    pub path: PathBuf,
    /// Whether `parent` is the cache root that `bench list/fetch/run` scan.
    pub discovered: bool,
}

/// `<output_dir or kermit_bench::cache::base_cache_dir()>/<bench_name>`.
pub fn output_dir(bench_name: &str, output_dir: Option<&Path>) -> anyhow::Result<OutputDir>;
```

This is the rule `run_gen_lubm` applies inline today, as a pure function with no output, no
filesystem writes and no workspace lookup. Tests cover `Some(dir)` (`discovered: false`),
`None` (`base_cache_dir()/<name>`, `discovered: true`) and
`Some(base_cache_dir())` (`discovered: true`).

### What the layer leaves out

- Downloads. SNAP and JOB both need a digest-checked download of a non-Parquet file, and
  one of #96 and #97 will split kermit-bench's Parquet-only `download_file`.
- Random numbers, which only H2O needs (they live in `kermit-generate/src/h2o/`).
- Any notion of `scale`. Neither the trait, `Target` nor any shared `bench gen` flag
  requires one, because SNAP and JOB have none.
- Directory wiping. `materialize` (drift and `--force`) and `bench gen` keep owning it, as
  today.

### Requirements from #96 and #97

| Requirement | Where |
|---|---|
| #96: a raw-artifact directory, `<out_dir>/raw/` | `RelationSink::raw_dir()` |
| #96: `RelationSink::write(&RelationHeader, &Tuples)` over a `write_parquet` that is `read_parquet`'s inverse; rows and SHA-256 recorded | `RelationSink::write`, `write_parquet`, `RelationRecord` |
| #96: queries with `expected: Option<u64>` | `Generated::queries: Vec<QueryDefinition>` |
| #96: typed per-generator `meta.json` fields | `GeneratedMeta<E>` with `#[serde(flatten)] extras: E` |
| #96: `benchmark.yml` from `BenchmarkDefinition`, with SHA-256s | `kermit_generate::run` step 4 |
| #96: discovery, drift, `--force` and `bench list` unchanged; one `dispatch` arm per kind | `GeneratedMeta` implements `GeneratorMeta`; `meta.json` written last |
| #96: nothing shared assumes `scale` | "What the layer leaves out" |
| #97: streaming writer beside the one-shot form, one code path | `RelationSink::open` → `RelationWriter`; `ParquetWriter` |
| #97: bytes independent of batch boundaries, pinned by a test | internal re-chunking; the batching test |
| #97: a dropped writer leaves no `<name>.parquet` | `.part` + rename; `Drop` removes `.part` |
| Co-ordinator: enforce the extras rule | `kermit_generate::run` step 3 (D14) |
| Co-ordinator: one `bench gen` flag set for the three kinds | `GenTarget` (D15), with its resolver in push 1 (D17) |

## Part 2: H2O

### Spec (kermit-bench)

```rust
GeneratorSpec::H2o {
    /// N, the rows of `x`. At least `H2O_MIN_SCALE`.
    scale: u32,
    /// Default `DEFAULT_H2O_SEED` = 108 (upstream's `set.seed(108)`).
    #[serde(default = "default_h2o_seed")]
    seed: u32,
    /// Default `Shuffled`.
    #[serde(default)]
    order: H2oOrder,
}

#[derive(Default, …)]
#[serde(rename_all = "kebab-case")]
pub enum H2oOrder { #[default] Shuffled, Sorted }

pub const H2O_MIN_SCALE: u32 = 10_000_000;
pub const DEFAULT_H2O_SEED: u32 = 108;
```

- All three fields are always serialized, so all of them feed `spec_hash`, and an omitted
  `seed` hashes the same as `seed: 108`. No H2O cache exists yet, so nothing needs
  `skip_serializing_if`.
- `BenchmarkDefinition::validate` rejects `scale < H2O_MIN_SCALE` with a reason naming
  upstream's floor, below which `small` has no left-only or right-only keys.
- `scale` is a `u32`, like the other kinds. Its maximum, about 4.29e9, is also `RowId::MAX`.

### Relations and queries

| Relation | Columns | Rows |
|---|---|---|
| `small` | `id1` | N/10⁶ |
| `medium` | `id2` | N/10³ |
| `big` | `id3` | N |
| `x_id1` | `id1`, `id3` | N |
| `x_id2` | `id2`, `id3` | N |
| `x_id3` | `id3` | N |

| Query | Datalog | Isolates |
|---|---|---|
| `q1` | `Q(K, I) :- small(K), x_id1(K, I).` | leaf enumeration: about 9 matching keys with about 1M children each at 1e7 |
| `q2` | `Q(K, I) :- medium(K), x_id2(K, I).` | about 9,000 matching keys with about 1,000 children each |
| `q5` | `Q(K) :- big(K), x_id3(K).` | a dense 90%-overlap intersection, which separates the seek strategies (#80) |

- Per-query projections, not one `x(id1, id2, id3)`: a `_` is still a trie level, so a
  shared `x` would descend unused columns on every query, under either column-order policy.
- `x` carries its unique `id3`, so every result row is distinct and no relation holds a
  duplicate. `expected` is then the same for all three cells, HashTrie's multiset count
  included.
- Every query is valid under `--column-orders stored`.
- Dropped from upstream: q3 (left outer join), q4 (string keys, which become q2 under
  dictionary encoding), the `v1`/`v2` payloads, the right-hand tables' extra columns, and
  NAs.

### Random numbers (`kermit-generate/src/h2o/rng.rs`)

- **SplitMix64** (Steele, Lea & Flood 2014; Vigna's reference `splitmix64.c`). It passes
  BigCrush, and its period of 2⁶⁴ is far beyond the roughly 10¹⁰ draws needed at the
  largest N. It is in-repo because `rand`'s `StdRng` and its sampling algorithms are not
  value-stable across versions (#74).
- **Bounded draws** use Lemire's nearly divisionless method (2019): unbiased values in
  `[0, n)` via a 128-bit multiply.
- **Shuffles** use Fisher–Yates (Durstenfeld): for `i` from `len − 1` down to `1`, swap `i`
  with `bounded(i + 1)`.
- **Streams.** Every (column, step) pair has its own generator. Columns are
  `id1, id2, id3` (0, 1, 2); steps are `Pool, Pad, LeftShuffle, RightShuffle` (0–3);
  `stream_id = column × 4 + step`. The numbering is fixed, and renumbering is an output
  change. A stream's initial state is SplitMix64's output function applied to
  `(seed as u64) << 32 | stream_id`. That input is injective, and the function is a
  bijection, so every stream starts at its own effectively random point on the 2⁶⁴ cycle.
  Each relation's data then depends only on `(plan, seed, order)`, not on the order
  relations are generated or written in.

### Key generation (`kermit-generate/src/h2o/keys.rs`)

```rust
#[derive(Serialize, Deserialize, Clone, Copy)]
pub struct KeyPlan { rows: usize, small: usize, medium: usize }   // big = rows
impl KeyPlan {
    /// Errors unless 1 ≤ small ≤ rows, 1 ≤ medium ≤ rows and rows ≤ RowId::MAX.
    pub fn new(rows: usize, small: usize, medium: usize) -> anyhow::Result<Self>;
    /// (N, N / 1_000_000, N / 1_000).
    pub fn for_scale(scale: u32) -> anyhow::Result<Self>;
}
```

For a key column with cardinality n, where n is `small`, `medium` or `rows`:

1. **Pool.** `keys = 1..=n + n/10`, shuffled (stream `Pool`).
   `shared = keys[..n − n/10]`, `left = keys[n − n/10..n]`, `right = keys[n..]`.
   This integer split replaces R's `n*0.9`, so every N is well-defined.
2. **x's column** (upstream's `sample_all`). Start from `keys[..n]` (shared ++ left), append
   `rows − n` uniform draws from `keys[..n]` (stream `Pad`), then shuffle (stream
   `LeftShuffle`). Every shared and left key appears at least once. For `id3`, n = rows, so
   there is no padding and the column is a permutation.
3. **The right table.** `shared ++ right`, shuffled (stream `RightShuffle`): exactly n
   unique keys.
4. **Matches.** The count of x's rows whose key is in `shared`, tested with a bitmap over
   `0..=n + n/10`.

### Relation order, sorting and memory (`kermit-generate/src/h2o/mod.rs`)

`H2oGenerator { target, plan, seed, order }` implements `Generator` with `KIND = "h2o"`:

1. **`id3`.** Build the pool; build `x3` (x's `id3` column, kept); count q5; build `big`;
   drop the pool. Write `x_id3` (`Tuples::from_flat(1, N, x3.clone())`) and `big`.
2. **`id1`.** Build `x1`; count q1; interleave `x1` with `x3` into one `Vec` and take it as
   `Tuples::from_flat(2, N, …)`, then drop `x1`. Write `x_id1`, then `small`.
3. **`id2`.** The same steps produce `x_id2` and `medium`.

- Under `order: sorted`, each relation's `Tuples` is sorted (`Tuples::sort`, the sort both
  sorted tries use) just before it is written. Counts are taken from the unsorted columns,
  and the draws are identical, so a sorted relation equals its shuffled counterpart sorted,
  and `expected` is identical. `x_id1` sorted is grouped by `id1`, with ascending `id3`
  inside each group.
- Peak memory is about 4N words (`x3`, `x1` and the 2N interleaved buffer): about 3.2 GB
  at N = 1e8.
- **Extras:** `seed`, `order`, `plan` (`rows`, `small`, `medium`).
- **Expected at 1e7:** q5 = 9,000,000 exactly, as upstream. q1 and q2 land near 9.0M but
  won't equal upstream's 8,997,136 and 9,001,606, because the random-number generator
  differs. H2O.md records kermit's values and the reason.

### Surface

- **`materialize::dispatch`:** an `H2o { scale, seed, order }` arm builds `KeyPlan::for_scale`
  and an `H2oGenerator` with the spec hash, and calls `kermit_generate::run`.
- **The error chain:** `materialize` reports a failed generator with `{e:#}`, not `{e}`,
  so the `anyhow` context chain of a `kermit-generate` arm survives (lubm and watdiv errors
  have a single level, so they are unaffected). The first `kermit-generate` arm to land
  makes this change, in its own commit, with a test that a two-level `.context()` error
  shows both messages. That arm is #96's phase 2 (Amendment 2), so push 2 inherits the
  change.
- **`bench gen h2o --scale N [--seed S] [--order shuffled|sorted] --tag T [--output-dir PATH]`**
  writes `h2o-{N}-{T}` under the cache root (or `--output-dir`) with `spec_hash: None`, like
  `bench gen lubm` (`lubm-{scale}-{tag}`).
  - **`GenTarget`** (D15, D17) goes in `kermit/src/main.rs` beside `GenSubcommand`. It
    lands with whichever comes first, push 2 or #96's phase 2, exactly as below; the later
    one merges and drops its own copy. Each non-RDF subcommand includes it with
    `#[command(flatten)] target: GenTarget`, builds its benchmark name, and calls
    `target.prepare("<kind>", &bench_name)?` for the output directory:

    ```rust
    /// Output flags shared by every non-RDF `bench gen <kind>` (D15).
    #[derive(clap::Args)]
    struct GenTarget {
        /// Tag appended to the benchmark name; pick a value that won't
        /// collide with committed snapshot names
        #[arg(long, value_name = "STRING", required = true)]
        tag: String,

        /// Override the cache dir parent (default: ~/.cache/kermit/benchmarks).
        /// NOTE: benchmarks generated outside the default cache are NOT
        /// auto-discovered by `bench list/fetch/run`; mainly useful for tests.
        #[arg(long, value_name = "PATH")]
        output_dir: Option<PathBuf>,
    }

    impl GenTarget {
        /// Resolves and creates `bench_name`'s output directory, as `bench gen lubm`
        /// does: refuses a name a workspace YAML already uses, and notes an
        /// undiscovered `--output-dir` and a non-empty directory on stderr.
        fn prepare(&self, kind: &str, bench_name: &str) -> anyhow::Result<PathBuf> {
            let workspace_names = kermit_bench::discovery::list_benchmarks(&workspace_root())
                .map_err(|e| anyhow::anyhow!("failed to enumerate workspace benchmarks: {e}"))?;
            if workspace_names.iter().any(|n| n == bench_name) {
                anyhow::bail!(
                    "--tag {:?} produces bench name {bench_name:?} which already exists in the \
                     workspace; pick a different tag",
                    self.tag
                );
            }
            let out = kermit_generate::output_dir(bench_name, self.output_dir.as_deref())?;
            if !out.discovered {
                eprintln!(
                    "[gen {kind}] note: --output-dir is set; {} will NOT be auto-discovered by \
                     `bench list/fetch/run`",
                    out.path.display()
                );
            }
            if std::fs::read_dir(&out.path).map(|mut d| d.next().is_some()).unwrap_or(false) {
                eprintln!(
                    "[gen {kind}] note: {} is non-empty; existing files will be overwritten",
                    out.path.display()
                );
            }
            std::fs::create_dir_all(&out.path)?;
            Ok(out.path)
        }
    }
    ```
  - `--scale` is range-checked against `H2O_MIN_SCALE`.
  - Seed and order are recorded in `meta.json`, not in the name.
  - kermit-bench has no clap dependency, so the CLI maps its own `ValueEnum` onto `H2oOrder`.
- **Committed YAMLs:**
  - `benchmarks/h2o.yml`: `kind: h2o`, `scale: 10000000`; the reference, like
    `lubm-reference.yml`.
  - `benchmarks/h2o-sorted.yml`: the same with `order: sorted`.

## Testing

### Layer

- **Round trip.** `write_parquet` → `read_parquet` returns the same
  `(RelationHeader, Tuples)` at arities 1, 2 and 3, including a relation with zero rows.
- **Rejections:**
  - arity 0;
  - a stem that differs from the header name;
  - a batch whose arity differs from the header's;
  - a value above `i64::MAX`.
- **Batching.** The same rows written one-shot and in batches of 1, 7, 1000,
  `WRITE_CHUNK_ROWS − 1` and `WRITE_CHUNK_ROWS + 1` give byte-identical files. One unary
  case spans a row-group boundary (2²⁰ + 3 rows).
- **Dropped writer.** A `ParquetWriter` dropped before `finish` leaves neither `.part` nor
  the final file. A `RelationWriter` dropped the same way leaves no record either.
- **`output_dir`:** the three cases under "`output_dir`: `GenTarget`'s resolver".
- **`RelationSink`:**
  - duplicate names, and names outside `[a-z][a-z0-9_]*` (uppercase, reserved, digit-first, hyphenated, empty), are rejected;
  - a record's `sha256` equals the file's hash and its `rows` equal the row count;
  - `raw_dir()` creates `raw/`, and the directory is absent when nobody calls it.
- **`kermit_generate::run` with a toy generator** (two relations, one streamed; one query):
  - `benchmark.yml` loads through kermit-bench and validates, with `file://` URLs, `sha256`
    and `expected`;
  - `meta.json` reads as a `MetaHeader` (`kind`, `spec_hash`, `schema_version`) and
    round-trips as `GeneratedMeta<ToyExtras>` with flattened extras;
  - there is no `meta.json` when `generate` fails, when a query fails to parse, when it
    names a relation that was never written, or when the extras contain a common field's
    key (a toy `relations` extra), including when an old `meta.json` was present
    beforehand;
  - `load_all_benchmarks_with_cache` discovers the output directory.

### H2O

- **Spec:**
  - `kind: h2o` parses with `seed` 108 and `order: shuffled` by default, and `order: sorted`
    parses;
  - `spec_hash` changes with `scale`, `seed` and `order`, and an omitted seed hashes the same
    as an explicit 108;
  - `scale < 10000000` is rejected with a clear error.
- **RNG:**
  - SplitMix64 matches a published reference vector, taken from the reference
    implementation during implementation, not recalled;
  - bounded draws stay in range at n = 1, 2ᵏ and 2ᵏ ± 1;
  - one deterministic chi-square check on a small n;
  - Fisher–Yates yields a permutation.
- **Keys**, on small plans: rows 2,000, small 20, medium 200, plus a plan with n < 10, where
  n/10 = 0.
  - The pool splits have the right sizes.
  - The right-hand table is exactly `shared ∪ right`, with no duplicate.
  - x's column has length `rows`, contains every shared and left key and no right key, and
    `x.id3` is a permutation.
  - `KeyPlan::new` rejects a zero cardinality, a cardinality above `rows`, and
    `rows > RowId::MAX`.
- **Counts.** Each `expected` equals a brute-force join over the generated relations, which
  shares no code with the bitmap count.
- **Determinism:**
  - the same `(plan, seed, order)` gives identical `Tuples`, and a different seed gives
    different ones;
  - `sorted` equals `sort(shuffled)` relation by relation, with the same `expected`;
  - a golden SHA-256 over each relation's flat buffer pins one small plan in both orders
    (D8). If it must change, bump `META_SCHEMA_VERSION` and add an `h2o` floor to
    `OUTDATED_BEFORE`.
- **Generator through the layer.** `kermit_generate::run` on a small plan writes the six
  Parquet files, `benchmark.yml` with three queries and their `expected`, and `meta.json`
  with the extras.
- **Cache behaviour** (a unit test in `kermit/src/materialize.rs`, D11). `materialize`
  with a temporary cache root and a directly constructed `H2o { scale: 1_000_000 }` (below
  the floor, so `small` = 1):
  - the first call is a miss and generates;
  - the second is a hit and doesn't regenerate;
  - a changed seed gives `SpecDrift`;
  - `force` regenerates.
- **End to end** (`kermit/tests/h2o_generate.rs`, D11). A small plan is generated in both
  orders through `kermit_generate::run` into a temporary directory. Each relation is loaded
  with `read_parquet`, and every query is counted through `kermit::db`'s entry points
  (`Database` with `lftj_join_for_each` / `hash_join_for_each`), as `lubm_cardinalities.rs`
  does. This covers every structure × `Optimiser::value_variants()` × both column-order
  policies, and every count must equal `expected`.
- **CLI** (`kermit/tests/cli_gen_scale_range.rs`). `bench gen h2o --scale 1000 --tag t`
  exits non-zero and names the floor.

### Manual acceptance

Release build, inside `nix develop`, with a scratch `XDG_CACHE_HOME` so the shared
`~/.cache/kermit` is not written without asking:
- `bench run h2o -i all -a all --verify` and `bench run h2o-sorted -i all -a all --verify`
  pass at 1e7;
- generating twice gives identical per-relation SHA-256s in `meta.json`;
- `bench gen h2o --scale 10000000 --tag dev` writes `h2o-10000000-dev`;
- `bench run h2o -i all -a all -m space` gives the space per structure that H2O.md records.

## Documentation

- **`docs/benchmarks/H2O.md`**, in the style of LUBM.md and WATDIV.md:
  - origin and attribution (db-benchmark, MPL-2.0; the generator is a reimplementation, not a
    copy);
  - the deviations: integer split, per-query projections, the carried `id3`, the dropped
    queries, payloads and NAs, and the in-repo RNG, which is why q1 and q2 differ from
    upstream's counts;
  - the `order` axis and its definition;
  - the queries and what each isolates;
  - the spec fields and the floor;
  - space per structure measured at 1e7;
  - memory: 8 B per column per row to load, about 4N words at peak to generate, the
    `RowId::MAX` ceiling, and per-structure ceilings extrapolated from the measured 1e7
    space (D7).
- **`benchmarks/README.md`:** `h2o` in the generator kinds, and a fields table.
- **CLAUDE.md:**
  - "8 crates" becomes 9, with a Workspace Architecture row for `kermit-generate`;
  - the dependency-flow paragraph: `kermit-ds`, `kermit-bench`, `kermit-rdf`,
    `kermit-parser` → `kermit-generate` → `kermit`. The sentence "`kermit-parser` is
    consumed by `kermit-algos` only" gains `kermit-generate`. The leaf list is unchanged.
  - `GenTarget` (D15, D17) in the `kermit` binary's description;
  - `bench gen [watdiv|lubm|h2o]`;
  - an H2O.md entry in Benchmark Reference Docs;
  - `h2o` in "Adding a new benchmark";
  - gotchas:
    - non-RDF generators live in `kermit-generate`, because kermit-rdf must not depend on
      kermit-ds; each generator's dependencies go in its manifest, not `kermit`'s;
    - `kermit_generate::run` writes `meta.json` last, as the commit marker, and `ParquetWriter`
      renames `.part` files on `finish`;
    - H2O's in-repo RNG has one stream per step, and its golden data hash follows the
      bump-and-floor rule.
- **Crate docs on `kermit-generate`**, written so the snap and job sessions can implement
  the trait from them. CI's `cargo doc --workspace` (`-Dwarnings`) lints them.
  `GenTarget`, in the binary, is checked with
  `cargo rustdoc -p kermit --bin kermit -- --document-private-items -D warnings`, which has
  two known pre-existing errors.

## Landing

1. **Push 1, the layer:** `ParquetWriter` / `write_parquet` in kermit-ds; the new
   `kermit-generate` crate (workspace member, manifest, crate docs, trait, sink, `run` with
   its checks, `GeneratedMeta`, `output_dir`) with the toy-generator tests; the
   `utc_iso8601_now` visibility change; `kermit-generate` added to `kermit/Cargo.toml`; and
   the CLAUDE.md lines. No binary code: `GenTarget` lands with push 2 or #96's phase 2
   (D17), and the first `dispatch` arm with push 2. It
   follows the usual loom landing: merge `origin/master` in, run the full gate, then
   `push HEAD:master`, after the user's go-ahead. The commit SHA then goes to the snap and
   job sessions.
2. **Push 2, H2O:** spec, RNG, keys, generator, `dispatch` arm, `GenTarget` and
   `bench gen h2o`, YAMLs, tests, docs and the 1e7 measurements.

**Host scheduling.** No cargo build or test runs while the #101 replicate chain is live
(`FULL_EXIT` in its `logs/full.log`). The benchmark co-ordinator session schedules push 1's
gate first, then push 2's acceptance runs, around #96's and #97's runs.

## Out of scope

- Splitting kermit-bench's download helper (#96 or #97).
- q3, q4, payload columns and NAs.
- Moving kermit-rdf's Parquet writer onto `write_parquet`.
- Fixing any structure's memory ceiling at large N (measure and document only).
- Whether H2O joins the next authoritative sweep.

## Acceptance

- [ ] Push 1 is on master: `write_parquet` round-trips; batched and one-shot writes are
      byte-identical; a dropped writer leaves no file; the toy generator's cache is
      discovered and drift-checked like LUBM's.
- [ ] `benchmarks/h2o.yml` (`kind: h2o`, `scale: 10000000`) and `benchmarks/h2o-sorted.yml`
      generate on `bench run`; cache hit, drift, `--force` and `bench list` behave as for
      LUBM.
- [ ] `bench gen h2o --scale N [--seed S] [--order …] --tag T` writes `h2o-{N}-{T}`.
- [ ] `bench run h2o -i all -a all --verify` and the same for `h2o-sorted` pass on all three
      cells at 1e7.
- [ ] The same `(scale, seed, order)` produces byte-identical relations, and `meta.json`
      and `benchmark.yml` record each relation's SHA-256.
- [ ] The tests above pass.
- [ ] `docs/benchmarks/H2O.md` records the measured space per structure at 1e7.

## Amendment 1 (2026-10-15): push 1b, the final review's minors

Push 1 landed at 615066b. Its final review (0 Critical, 0 Important, 7 Minor) left seven
minor findings, which the user chose to fix in a small follow-up before the generators build
on the layer:

1. **Arity.** `run`'s query check also requires each body atom to have one term per column
   of the relation it names (step 4). Without it, a wrong-arity query commits a cache that
   fails at `bench run`, and since the spec hash matches, the broken cache would then be a
   hit.
2. **Target.** A new step 0 checks, before `meta.json` is touched, that `out_dir` ends in
   `bench_name`. The full name rules are still checked by `validate()` at step 5.
3. **I/O errors name the file.** `RelationSink::open`, `RelationWriter::write_batch` and
   `finish`, and `run`'s `meta.json` write add context naming the relation and its path.
   kermit-ds's errors stay typed, so the path is added by the layer.
4. **Two failure paths get tests:** the `RowId::MAX` total, and a failing `finish` (a rename
   onto a directory), which must leave no `.part` file.
5. **`ROW_GROUP_ROWS % WRITE_CHUNK_ROWS == 0` is a compile-time assertion**, not a comment.
6. **Docs written ahead of the code** are reworded: the crate docs' description of the
   binary's dispatch, and ARCHITECTURE.md's tree line.
7. **`ParquetWriter` and `RelationWriter` implement `Debug`** (path, rows, failed;
   relation name), by hand, without the Arrow writer or the buffers.

No name changes. #96 and #97 are told before it lands, because items 1 and 2 make `run`
stricter. Outside #95, the review also found that `write_meta_json` is not atomic: #115.


## Amendment 2 (2026-10-15): push 2, after #96's phase 2

#96's phase 2 lands before push 2. It carries `GenTarget`, copied verbatim from § Surface,
and the materialize `{e:#}` change with its test (the first-lander rules of D17 and
§ Surface). Push 2 merges both from `origin/master` and adds neither. Push 2's plan settles
these points within the design above, and its names contract
(`docs/superpowers/plans/2026-10-15-h2o-generator-contract.md`) records them:

1. **Layout.** `kermit-generate/src/h2o/{mod,rng,keys}.rs`. `KeyPlan`, `H2oGenerator` and
   `H2oExtras` are public. The random numbers and `KeyPool` (steps 1–4 of § Key generation)
   are crate-private. The generator's tests are an integration test,
   `kermit-generate/tests/h2o.rs`, and use only the public API.
2. **Pinned values are computed independently.** The SplitMix64 vectors come from Vigna's
   `splitmix64.c`, compiled and run. The golden digests and counts of the reference plan
   (2,000 / 20 / 200, seed 108: q1 = 1812, q2 = 1814, q5 = 1800) come from a Python model
   of this spec written in the design session, which shares no code with the Rust. A
   pinned bounded-draw vector exercises Lemire's rejection loop.
3. **Query descriptions** use upstream's question names: q1 "small inner on int", q2
   "medium inner on int", q5 "big inner on int".
4. **H2O.md's 1e7 measurements** (q1/q2 at 1e7, space per structure, generation time and
   peak RSS, ceilings) are written by push 2's last story, after the acceptance runs in a
   heavy-run slot. Until then, the story before it leaves a stub section.
