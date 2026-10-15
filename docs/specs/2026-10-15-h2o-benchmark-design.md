# H2O Join Benchmark and the Shared Non-RDF Generator Layer

**Date:** 2026-10-15
**Status:** Design approved in conversation (2026-10-15), section by section.
**Scope:** Issue #95: the H2O.ai db-benchmark join tasks ("J1") as a generator benchmark,
`generator: { kind: h2o, … }`. It also covers the shared `kermit/src/generate/` layer that
#95, #96 (SNAP) and #97 (JOB) all build on. That layer is designed here against all three
issues' requirements and lands on master first, as its own push.
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
   `Generator` trait in a new binary module `kermit/src/generate/`, with a `RelationSink`
   that writes and hashes each relation as it arrives, and `generate::run`, which writes
   `benchmark.yml` and then, last, `meta.json`. Every generated cache satisfies the existing
   contract, so discovery, spec drift, `--force`, `OUTDATED_BEFORE` and `bench list` work
   unchanged.
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
| D3 | Layer shape: a trait plus a streaming sink, with `generate::run` owning `benchmark.yml` and `meta.json` (approach A, chosen over free helpers). | Specified. |
| D4 | The writer takes `&RelationHeader, &Tuples` and is the exact inverse of `read_parquet`. | Replaces #95's column-major `columns`. Chosen by the user in #96's session and confirmed here. |
| D5 | The writer streams (`create` / `write_batch` / `finish`), with the one-shot forms as wrappers. A file's bytes don't depend on how its rows were batched. | New. Chosen by the user in #97's session and confirmed here. |
| D6 | `RelationSink::raw_dir()` for raw artifacts. | New, for #96. |
| D7 | The "known limit" is rewritten from measurements: since #111, loading costs 8 B per column per row. | Replaces #95's 56 B/tuple extrapolation. |
| D8 | The golden test hashes each relation's data, not its file bytes. | Refines #95's "golden SHA-256 of a small plan's output". |
| D9 | `benchmark.yml` carries every relation's `sha256`, so `bench fetch` verifies generated caches. | New. |
| D10 | Random numbers: SplitMix64, Lemire bounded draws, Fisher–Yates, one stream per (column, step). | Settles #95's "SplitMix64- or xoshiro-class". |
| D11 | The end-to-end test is a unit test in the binary, because `generate/` and `materialize` are binary modules and the 1e7 floor stops any small `bench gen h2o`. | Refines #95. |
| D12 | `benchmarks/h2o-sorted.yml` is committed beside `h2o.yml`. | New, follows from D2. |

## Part 1: the shared layer

### `ParquetWriter` and `write_parquet` (kermit-ds)

These go in `kermit-ds/src/relation.rs`, beside `read_parquet`, and are re-exported at the
crate root like it.

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

### `kermit/src/generate/mod.rs`: trait, sink, run

A module of the binary, declared in `main.rs` beside `materialize`.

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
- `open` rejects a name that is already written, is not a predicate name kermit-parser
  accepts, or starts with a reserved prefix (`Const_`, `Select_`, `Index_`). It creates
  `ParquetWriter::create(<out_dir>/<name>.parquet, header)`.
- `write` is `open` + one `write_batch` + `finish`, so there is one code path.
- `RelationWriter` borrows the sink mutably, so only one relation can be open at a time.
- `finish` hashes the finished file with `kermit_bench::cache::sha256_hex`, the encoding
  `bench fetch` compares against, and pushes a `RelationRecord`. A writer dropped without
  `finish` records nothing.

**`generate::run`**
1. Creates `out_dir` and deletes any existing `meta.json`, so that a failed run can never
   leave an old commit marker beside new files.
2. Calls `generate` with a fresh sink.
3. Checks the queries: names are unique, each `query` parses as a `kermit_parser::JoinQuery`,
   and each body predicate names a recorded relation. A malformed rule therefore fails at
   generation time, not at `bench run`.
4. Builds a `BenchmarkDefinition`:
   - `name`: the target's bench name;
   - `description`: from `Generated`;
   - one `RelationSource` per record, in write order: `url: file://<out_dir>/<name>.parquet`,
     `path: None`, `sha256: Some(..)` (D9);
   - `queries`;
   - `generator: None`, as the RDF pipelines write it.

   It calls `validate()` and writes `benchmark.yml`.
5. Writes `meta.json` last, through `kermit_rdf::generator::write_meta_json`, and returns
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
  The module docs state the rule, and the toy-generator test pins it.
- The timestamp comes from kermit-rdf's `utc_iso8601_now`, made public (today it is
  `pub(crate)` in the private `timestamp` module). That one-line change is this design's
  only edit to kermit-rdf.
- A future output change for one kind bumps the shared `META_SCHEMA_VERSION` and adds that
  kind's floor to `OUTDATED_BEFORE`, the existing rule.

### What the layer leaves out

- Downloads. SNAP and JOB both need a digest-checked download of a non-Parquet file, and
  one of #96 and #97 will split kermit-bench's Parquet-only `download_file`.
- Random numbers, which only H2O needs (they live in `generate/h2o/`).
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
| #96: `benchmark.yml` from `BenchmarkDefinition`, with SHA-256s | `generate::run` step 4 |
| #96: discovery, drift, `--force` and `bench list` unchanged; one `dispatch` arm per kind | `GeneratedMeta` implements `GeneratorMeta`; `meta.json` written last |
| #96: nothing shared assumes `scale` | "What the layer leaves out" |
| #97: streaming writer beside the one-shot form, one code path | `RelationSink::open` → `RelationWriter`; `ParquetWriter` |
| #97: bytes independent of batch boundaries, pinned by a test | internal re-chunking; the batching test |
| #97: a dropped writer leaves no `<name>.parquet` | `.part` + rename; `Drop` removes `.part` |

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

### Random numbers (`generate/h2o/rng.rs`)

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

### Key generation (`generate/h2o/keys.rs`)

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

### Relation order, sorting and memory (`generate/h2o/mod.rs`)

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
  and an `H2oGenerator` with the spec hash, and calls `generate::run`.
- **`bench gen h2o --scale N [--seed S] [--order shuffled|sorted] --tag T`** writes
  `h2o-{N}-{T}` under the cache root with `spec_hash: None`, like `bench gen lubm`
  (`lubm-{scale}-{tag}`).
  - `--scale` is range-checked against `H2O_MIN_SCALE`.
  - The tag rule, and the handling of an existing output directory, match `bench gen lubm`.
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
- **`RelationSink`:**
  - duplicate, unparseable and reserved names are rejected;
  - a record's `sha256` equals the file's hash and its `rows` equal the row count;
  - `raw_dir()` creates `raw/`, and the directory is absent when nobody calls it.
- **`generate::run` with a toy generator** (two relations, one streamed; one query):
  - `benchmark.yml` loads through kermit-bench and validates, with `file://` URLs, `sha256`
    and `expected`;
  - `meta.json` reads as a `MetaHeader` (`kind`, `spec_hash`, `schema_version`) and
    round-trips as `GeneratedMeta<ToyExtras>` with flattened extras;
  - there is no `meta.json` when `generate` fails, when a query fails to parse, or when it
    names a relation that was never written, including when an old `meta.json` was present
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
- **Generator through the layer.** `generate::run` on a small plan writes the six Parquet
  files, `benchmark.yml` with three queries and their `expected`, and `meta.json` with the
  extras.
- **Cache behaviour.** `materialize` with a temporary cache root and a directly constructed
  `H2o { scale: 1_000_000 }` (below the floor, so `small` = 1):
  - the first call is a miss and generates;
  - the second is a hit and doesn't regenerate;
  - a changed seed gives `SpecDrift`;
  - `force` regenerates.
- **End to end** (a binary unit test, D11). A small plan is generated in both orders, and
  every query is loaded and counted through the `ExecutionFamily` `load` and `count` calls
  that `bench run --verify` makes. This runs on every cell × `Optimiser::value_variants()` ×
  both column-order policies, and every count must equal `expected`.
- **CLI.** `bench gen h2o --scale 1000 --tag t` exits non-zero and names the floor.

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
  - the `generate/` layer in the `kermit` binary's description;
  - `bench gen [watdiv|lubm|h2o]`;
  - an H2O.md entry in Benchmark Reference Docs;
  - `h2o` in "Adding a new benchmark";
  - gotchas:
    - non-RDF generators live in `kermit/src/generate/`, because kermit-rdf must not
      depend on kermit-ds;
    - `generate::run` writes `meta.json` last, as the commit marker, and `ParquetWriter`
      renames `.part` files on `finish`;
    - H2O's in-repo RNG has one stream per step, and its golden data hash follows the
      bump-and-floor rule.
- **Module docs on `generate/mod.rs`**, written so the snap and job sessions can implement
  the trait from them. These live in the binary, which CI never rustdoc-lints, so check them
  with `cargo rustdoc -p kermit --bin kermit -- --document-private-items -D warnings`, which
  has two known pre-existing errors.

## Landing

1. **Push 1, the layer:** `ParquetWriter` / `write_parquet`, `generate/` (trait, sink,
   `run`, `GeneratedMeta`) with the toy-generator tests, the `utc_iso8601_now` visibility
   change, and the layer's CLAUDE.md lines. It follows the usual loom landing: merge
   `origin/master` in, run the full gate, then `push HEAD:master`, after the user's go-ahead.
   The commit SHA then goes to the snap and job sessions.
2. **Push 2, H2O:** spec, RNG, keys, generator, `dispatch` arm, `bench gen h2o`, YAMLs,
   tests, docs and the 1e7 measurements.

A comment on #95 summarising D1–D12 is posted only with the user's approval.

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
