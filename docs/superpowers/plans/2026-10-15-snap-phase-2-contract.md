# SNAP Phase 2 (#96): Names Contract

The binding interface for phase 2's plan (`2026-10-15-snap-phase-2.md`). Each drafted task
takes every name, signature, path and behaviour from here. A decision this contract
doesn't make goes back to the manager; the drafter doesn't make it.

- **Spec:** `docs/specs/2026-10-15-snap-benchmark-design.md` (approved; phase-2 decisions
  in ab124ec).
- **Base:** `aidanb/snap` at ab124ec. It contains phase 1 (`download_verified`, df01e37)
  and H2O's push 1 (`kermit-generate`, 615066b).
- **Generator layer (landed):** `kermit-generate/src/{generator,sink,meta,run,output_dir}.rs`.
  The root re-exports are `kermit_generate::{Generator, Generated, GeneratedMeta,
  RelationSink, RelationWriter, RelationRecord, run, output_dir, OutputDir}`. `Target` is
  `kermit_rdf::generator::Target { out_dir, bench_name, tag, spec_hash }`.

## Task order (every commit builds and passes)

| Task | Crate / files | What |
|---|---|---|
| T1 | `kermit-bench/src/definition.rs`, `lib.rs` | `SnapGraph`, `SnapQuery` (types only, no `GeneratorSpec` change) |
| T2 | `kermit-generate/Cargo.toml`, `src/lib.rs`, `src/snap.rs` | the parser: `edge_header`, `ParsedEdges`, `parse_edge_list`, `parse_gz`, `url_stem` |
| T3 | `kermit-generate/src/snap/counts.rs` | the oracle: `expected_counts` |
| T4 | `kermit-generate/src/snap.rs` | the generator: `rule`, `SnapSource`, `SnapExtras`, `SnapGenerator`, and the network reference-count test |
| T5 | `kermit-bench/src/definition.rs`, `kermit/src/materialize.rs` | `GeneratorSpec::Snap` and its `validate_generator` arm, plus the `dispatch` arm, in one commit (the match is exhaustive) |
| T6 | `kermit/src/materialize.rs` | `{e}` → `{e:#}`, its own commit with a test |
| T7 | `kermit/src/main.rs` | `GenTarget` (verbatim), `GenSubcommand::Snap`, the clap mirrors and `run_gen_snap` |
| T8 | `benchmarks/snap-{amazon,dblp}.yml`, `kermit/tests/fixtures/snap/mini.ungraph.txt`, `kermit/tests/{snap_generate,cli_snap_gen}.rs`, `kermit/Cargo.toml` | committed YAMLs, the fixture, end-to-end and CLI tests |
| T9 | `docs/benchmarks/SNAP.md`, `benchmarks/README.md`, `CLAUDE.md` | docs |
| T10 | — | the gate |

## T1: kermit-bench spec types

```rust
// kermit-bench/src/definition.rs, beside GeneratorSpec
/// A SNAP community graph with a pinned download (Yang & Leskovec, ICDM 2012).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SnapGraph { ComAmazon, ComDblp, ComYoutube }

impl SnapGraph {
    /// Every named graph, in this order.
    pub const ALL: [SnapGraph; 3] = [Self::ComAmazon, Self::ComDblp, Self::ComYoutube];
    /// The YAML and CLI spelling: "com-amazon", "com-dblp", "com-youtube".
    pub fn name(self) -> &'static str;
    /// The pinned download: (URL, SHA-256 of the `.gz`).
    pub fn source(self) -> (&'static str, &'static str);
}

/// One of the four SNAP queries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SnapQuery { Triangle, Path3, Cycle4, Clique4 }

impl SnapQuery {
    /// Every query, in the default order a spec without `queries` uses.
    pub const ALL: [SnapQuery; 4] = [Self::Triangle, Self::Path3, Self::Cycle4, Self::Clique4];
    /// The YAML, CLI and query name: "triangle", "path3", "cycle4", "clique4".
    pub fn name(self) -> &'static str;
}
```

- **The pinned sources:** `https://snap.stanford.edu/data/bigdata/communities/com-<x>.ungraph.txt.gz`, with the three SHA-256s from the spec's § Measured facts (amazon `bb4b9fcf…ec34`, dblp `9eb0bd30…4cb3`, youtube `dff1b97b…f3e0`; copy the full 64 characters from the spec).
- **Export:** both from `kermit-bench/src/lib.rs`'s `pub use definition::{…}`.
- **Tests (in `definition.rs`'s `mod tests`):**
  - `snap_names_match_their_serde_spelling`: for every `ALL` member, `serde_yaml` round-trips the name;
  - `snap_graph_sources_are_pinned`: each URL starts with `https://snap.stanford.edu/`, its last segment is `<name>.ungraph.txt.gz`, and the digest is 64 lowercase hex;
  - `an_unknown_snap_name_fails_to_parse`: an unknown graph or query in YAML is a parse error.

## T2: the parser

- **Manifest:** `kermit-generate/Cargo.toml` gains `flate2 = "1"` in `[dependencies]` (the version `kermit-rdf` uses).
- **Module:** `kermit-generate/src/lib.rs` gains `pub mod snap;` (alphabetical among the `mod` lines). `snap.rs` declares `mod counts;` (T3 adds the file; T2 creates an empty `snap/counts.rs` holding only its module doc).

```rust
// kermit-generate/src/snap.rs
/// The one relation every SNAP benchmark has.
pub const EDGE: &str = "edge";
/// `edge(src, dst)`.
pub fn edge_header() -> RelationHeader;

/// A parsed, cleaned and symmetrised SNAP edge list.
#[derive(Debug)]
pub struct ParsedEdges {
    /// Arity 2: each kept line's (s, d) then (d, s), in file order.
    pub tuples: Tuples,
    /// Data lines read (comments excluded).
    pub input_edges: u64,
    /// Lines with s == d, dropped.
    pub self_loops: u64,
    /// Lines whose unordered pair had appeared before, dropped (a reversed repeat included).
    pub duplicates: u64,
}

/// Parses a SNAP edge list. `source` names the input in errors (a path or URL).
pub fn parse_edge_list(reader: impl BufRead, source: &str) -> anyhow::Result<ParsedEdges>;
/// `parse_edge_list` over a gzipped file (`flate2::read::MultiGzDecoder`).
pub fn parse_gz(path: &Path) -> anyhow::Result<ParsedEdges>;
/// The benchmark-name stem of a URL: its last path segment up to the first `.`
/// (`…/com-orkut.ungraph.txt.gz` gives `com-orkut`). `None` if that is empty.
pub fn url_stem(url: &str) -> Option<&str>;
```

- **Parsing rules** are spec § The parser's:
  - a `#` line is skipped;
  - a data line is exactly two whitespace-separated `u64`s that fit `usize`;
  - anything else, a blank line included, is an error `"<source>:<line>: malformed edge line {text:?}: expected two unsigned integers"`, with line numbers 1-based and comments counted;
  - a self-loop is dropped and counted, and so is a repeat of an unordered pair (`HashSet<(usize, usize)>` of `(min, max)`);
  - each kept line pushes `[s, d]` then `[d, s]`;
  - zero kept lines is the error `"<source> has no edges after dropping {self_loops} self-loops and {duplicates} duplicates"`.
- **Tests:** `comments_are_skipped`, `a_malformed_line_names_its_number_and_text` (one field, three fields, non-numeric, negative, blank), `self_loops_and_duplicates_are_dropped_and_counted` (exact and reversed), `rows_are_symmetrised_in_file_order`, `an_edgeless_file_is_rejected_with_its_counts`, `parse_gz_reads_a_gzipped_list` (GzEncoder into a temp file), `url_stem_takes_the_last_segment_up_to_its_first_dot`.

## T3: the oracle

```rust
// kermit-generate/src/snap/counts.rs
/// The exact answer count of each query in `queries` (same order) over `edges`, a
/// symmetric, self-loop-free, duplicate-free arity-2 batch (what `parse_edge_list`
/// produces). It shares no code with the joins.
pub fn expected_counts(edges: &Tuples, queries: &[SnapQuery]) -> anyhow::Result<Vec<u64>>;
```

- **Re-export:** `snap.rs` re-exports it (`pub use counts::expected_counts;`).
- **Internals (private, unit-tested in the same file):**
  - `struct Graph { offsets: Vec<usize>, neighbours: Vec<usize> }`, a CSR with each list sorted, sized by max id + 1;
  - `fn path3(&Graph) -> Option<u64>` (Σ over rows (b, c) of d(b)·d(c));
  - `fn triangle(&Graph) -> Option<u64>` (6 × Σ over oriented (u, v) of |N⁺(u) ∩ N⁺(v)|);
  - `fn cycle4(&Graph) -> Option<u64>` (tr(A⁴) via a reusable 2-walk tally);
  - `fn clique4(&Graph) -> Option<u64>` (24 × Σ over oriented (u, v), w ∈ W = N⁺(u) ∩ N⁺(v), of |N⁺(w) ∩ W|).
- **Orientation:** u → v iff (d(u), u) < (d(v), v).
- **Overflow:** checked arithmetic, where `None` becomes `bail!("the {name} count overflows u64")`.
- **Tests:**
  - `k4_counts` (24, 108, 84, 24) and `c4_counts` (0, 32, 32, 0), in the order triangle, path3, cycle4, clique4;
  - `random_graphs_match_a_brute_force_join`: a seeded in-test xorshift for 50 graphs on ≤ 9 vertices, with a fresh nested-loop count of each rule written in the test;
  - `an_empty_query_list_counts_nothing`.

## T4: the generator

```rust
// kermit-generate/src/snap.rs
/// The Datalog rule for `query` (spec § Workload, verbatim).
pub fn rule(query: SnapQuery) -> &'static str;

/// Where a graph's `.gz` comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SnapSource {
    /// A local `.gz` (tests).
    Local(PathBuf),
    /// A URL and its pinned SHA-256, fetched with `kermit_bench::cache::download_verified`.
    Remote { url: String, sha256: String },
}
impl SnapSource {
    /// A named graph's pinned download.
    pub fn named(graph: SnapGraph) -> Self;
}

/// SNAP's own `meta.json` fields.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SnapExtras {
    pub graph: Option<SnapGraph>,
    pub source_url: Option<String>,
    pub gz_sha256: String,
    pub input_edges: u64,
    pub dropped_self_loops: u64,
    pub dropped_duplicates: u64,
}

pub struct SnapGenerator<'a> { /* target, graph, source, queries: Vec<SnapQuery> */ }
impl<'a> SnapGenerator<'a> {
    /// `queries: None` means `SnapQuery::ALL`.
    pub fn new(
        target: Target<'a>, graph: Option<SnapGraph>, source: SnapSource,
        queries: Option<&[SnapQuery]>,
    ) -> Self;
}
impl Generator for SnapGenerator<'_> { const KIND: &'static str = "snap"; type Extras = SnapExtras; … }
```

`generate`:
1. **Places the `.gz`** at `sink.raw_dir()?.join(<file name>)`. The file name is the URL's last path segment for `Remote` (fetched with `download_verified(url, &dest, sha256)`) and the path's file name for `Local` (`fs::copy`).
2. **Hashes it:** `gz_sha256 = kermit_bench::cache::sha256_hex(&dest)`.
3. **Parses** with `parse_gz(&dest)`.
4. **Writes** `sink.write(&edge_header(), &parsed.tuples)`.
5. **Counts:** `expected_counts(&parsed.tuples, &self.queries)`.
6. **Returns** `Generated { description, queries, extras }`:
   - `description`: `"SNAP <graph name or url stem>: one symmetric `edge` relation; queries count walks (spec § Workload)"`;
   - one `QueryDefinition { name: q.name(), description, query: rule(q), expected: Some(count) }` per query, in order, with these descriptions:
     - triangle: "6 × triangles";
     - path3: "3-step walks (vertices may repeat)";
     - cycle4: "closed 4-step walks, tr(A⁴): not 8 × 4-cycles";
     - clique4: "24 × 4-cliques".

**Tests (inline in `snap.rs`):**
- `rules_are_the_spec_queries` (each parses as `kermit_parser::JoinQuery` and names only `edge`);
- `a_local_fixture_generates_a_cache`. Gzip a small edge list into a temp file, `run(&SnapGenerator::new(Target{…}, None, SnapSource::Local(path), None))`, then assert:
  - the extras' counts;
  - `raw/<name>` exists;
  - every query has `expected`;
  - `edge.parquet` round-trips through `kermit_ds::read_parquet` to the parsed tuples;
- `reference_counts_match_the_verified_values` (network). It downloads com-amazon and com-dblp through `SnapSource::named` into a temp dir with `download_verified` directly, then `parse_gz` and `expected_counts(…, &SnapQuery::ALL)`, checked against spec § Workload's eight values. A `BenchError::Download { source, .. }` with `source.status().is_none()` skips with an `eprintln!`; any other error fails.

## T5: the spec variant and the dispatch arm (one commit)

```rust
// kermit-bench GeneratorSpec, new variant (kind: snap)
Snap {
    #[serde(default, skip_serializing_if = "Option::is_none")] graph: Option<SnapGraph>,
    #[serde(default, skip_serializing_if = "Option::is_none")] url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")] sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")] queries: Option<Vec<SnapQuery>>,
},
```

- **`validate_generator`'s `Snap` arm** follows spec § The spec. Each error is a `BenchError::Invalid` whose reason starts `snap generator`:
  - exactly one of `graph` / `url`;
  - `url` needs `sha256`, and `graph` forbids it;
  - `url` starts with `http://` or `https://`;
  - `sha256` is 64 lowercase hex;
  - `queries`, when present, is non-empty and free of duplicates.
- **Tests:** `snap_named_and_url_forms_parse`, `snap_spec_rejects_bad_combinations` (one assertion per rule above), `snap_spec_hash_changes_with_each_field`.
- **Dispatch arm.** `kermit/src/materialize.rs`'s `dispatch` gains
  `| GeneratorSpec::Snap { graph, url, sha256, queries } => run_snap(*graph, url.as_deref(), sha256.as_deref(), queries.as_deref(), bench_name, out_dir, spec_hash)`. Then:
  ```rust
  fn run_snap(
      graph: Option<SnapGraph>, url: Option<&str>, sha256: Option<&str>,
      queries: Option<&[SnapQuery]>, bench_name: &str, out_dir: &Path, spec_hash: &str,
  ) -> anyhow::Result<()>
  ```
  - **Source:** `graph` gives `SnapSource::named(g)`. Otherwise `url` + `sha256` give `Remote`, with an `anyhow::bail!` if either is missing; validation makes that unreachable.
  - **Target:** `Target { out_dir, bench_name, tag: bench_name, spec_hash: Some(spec_hash) }`.
  - **Run:** `kermit_generate::run(&SnapGenerator::new(…))`, discarding the meta.

## T6: `materialize`'s error context

- **The helper:** extract `fn generator_failed(name: &str, err: &anyhow::Error) -> BenchError`, which returns `BenchError::Invalid { name, reason: format!("generator pipeline failed: {err:#}") }`. `materialize` calls it.
- **The test:** `generator_failure_keeps_the_context_chain`, where `anyhow!("inner").context("outer")` gives a reason containing both "outer" and "inner".
- **Rule:** own commit, first lander only (spec § Wiring). If H2O's push 2 has landed it by the time T6 runs, drop T6 and merge.

## T7: `bench gen snap`

- **`GenTarget` and `prepare`:** verbatim from `docs/specs/2026-10-15-h2o-benchmark-design.md` § Surface, in `kermit/src/main.rs` beside `GenSubcommand` (first lander, D17). If H2O's push 2 has landed it, use the landed one.
- **The clap mirrors:**
  - `#[derive(Clone, Copy, clap::ValueEnum)] enum SnapGraphArg { ComAmazon, ComDblp, ComYoutube }` with `impl From<SnapGraphArg> for SnapGraph`;
  - `enum SnapQueryArg { Triangle, Path3, Cycle4, Clique4 }` with `impl From<SnapQueryArg> for SnapQuery`;
  - the test `snap_clap_names_match_the_spec_names`, which for every `value_variants()` checks `to_possible_value().unwrap().get_name() == SnapGraph::from(v).name()` (and the same for queries).
- **The variant:**
  ```rust
  /// Generate a SNAP community-graph benchmark (#96)
  Snap {
      /// A named graph with a pinned download
      #[arg(long, value_enum, required_unless_present = "url", conflicts_with_all = ["url", "sha256"])]
      graph: Option<SnapGraphArg>,
      /// A SNAP edge-list `.gz` URL; requires --sha256
      #[arg(long, value_name = "URL", requires = "sha256")]
      url: Option<String>,
      /// SHA-256 of the `.gz` at --url (64 lowercase hex)
      #[arg(long, value_name = "HEX", requires = "url")]
      sha256: Option<String>,
      /// Queries to emit, comma-separated (default: all four)
      #[arg(long, value_enum, value_delimiter = ',')]
      queries: Vec<SnapQueryArg>,
      #[command(flatten)]
      target: GenTarget,
  },
  ```
- **`fn run_gen_snap(graph, url, sha256, queries, target) -> anyhow::Result<()>`:**
  1. Build `GeneratorSpec::Snap { … }` (with `queries: None` when the list is empty).
  2. Name the benchmark `snap-<graph.name() | url_stem(url)>-<tag>`, an error if `url_stem` is `None`.
  3. Validate the name, the digest and the URL in one place, through `BenchmarkDefinition { name, description: String::new(), relations: vec![], queries: vec![], generator: Some(spec) }.validate()`.
  4. Get the directory from `let out = target.prepare("snap", &bench_name)?;`.
  5. Run `kermit_generate::run` with `spec_hash: None` and `tag: &target.tag`.
  6. Print `"snap benchmark written to {}"` (as the LUBM handler does).
- **Wiring:** an arm in the `BenchSubcommand::Gen` match.

## T8: YAMLs, fixture, end-to-end and CLI tests

- **`benchmarks/snap-amazon.yml` and `snap-dblp.yml`:** `name`, a one-line `description`, and `generator: { kind: snap, graph: com-amazon }` (respectively `com-dblp`).
- **`kermit/tests/fixtures/snap/mini.ungraph.txt`:** plain text, about 30 data lines. It has a `#` header, a 4-clique, a 4-cycle, a hub of degree ≥ 5, one self-loop and one reversed duplicate. Its expected counts come from the generator, not hand-written.
- **`kermit/Cargo.toml` `[dev-dependencies]`:** `flate2 = "1"` and `kermit-generate`, if the tests need it beyond the normal dependency.
- **`kermit/tests/snap_generate.rs` (end to end):**
  - gzip the fixture into a temp file, then `kermit_generate::run` with `SnapSource::Local`;
  - then for the three cells (`TreeTrie` and `ColumnTrie` with `LeapfrogTriejoin`, `HashTrie` with `HashTriejoin`), every `Optimiser::value_variants()` and every `ColumnOrderPolicy::value_variants()`, every query's count equals its `expected`;
  - mirror `kermit/tests/lubm_mini_oracle.rs`'s `cardinality_mismatches`, `load_parquet_relations` and `join_under_planner`, and its hash-family counterpart if it has one;
  - also: generating twice gives a byte-identical `edge.parquet`, and `meta.json` round-trips as `GeneratedMeta<SnapExtras>`.
- **`kermit/tests/cli_snap_gen.rs`:**
  - **Server:** a loopback HTTP server serving the gzipped fixture to any number of connections.
  - **Isolation:** `XDG_CACHE_HOME` and `KERMIT_WORKSPACE` in temp dirs, and Linux-only skip guards as in `cli_bench_fetch_integrity.rs`.
  - **The CLI path:** `bench gen snap --url http://127.0.0.1:<port>/mini.ungraph.txt.gz --sha256 <hex> --tag t` writes `snap-mini-t`, and `bench run snap-mini-t -i all -a all -m iteration --verify` passes, with the fastest settings `cli_bench_run_verify.rs` uses.
  - **A wrong digest:** a wrong `--sha256` exits non-zero and leaves no `snap-mini-t` cache subdir.
  - **The declarative path:** a workspace YAML `benchmarks/snap-mini.yml` with `kind: snap`, `url` and `sha256` materialises on `bench run`. A second `bench run` is a hit. Editing its `queries` gives `SpecDrift` without `--force` and regenerates with it.

## T9: docs

Spec § Documentation, plus `kermit-generate`'s crate docs if they list modules.

## T10: the gate

As phase 1's T5: fmt, clippy and doc on the pinned and CI nightlies (non-writing override), the full workspace test detached, the scope check, and mutation checks the drafts propose (one per task that has a decisive test).

## Updates

- **H2O push 1b (bbaef2e, landing soon; base phase 2 on it):** `kermit_generate::run` now checks, before touching anything, that `target.out_dir`'s last path component equals `target.bench_name`. Every test that calls `run` with a hand-made out_dir names it after the benchmark (`tempdir().path().join("snap-mini")` with `bench_name: "snap-mini"`). `run`'s query check also requires every body atom to have exactly one term per column of the relation it names. SNAP's binary `edge` atoms satisfy it. No names change.
