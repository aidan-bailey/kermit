# SNAP Community Graphs as a Generator Benchmark (`kind: snap`)

**Date:** 2026-10-15
**Status:** Design approved in conversation (2026-10-15), section by section. It confirms
issue #96 (itself the output of a 2026-10-05 brainstorm) and records what changed since:
the shared generator layer moved to #95, the general download moved here, and the open
com-youtube facts were measured.
**Scope:** Issue #96. One symmetric `edge` relation built from a SNAP community graph and
four cyclic and acyclic graph patterns, generated on demand like LUBM and WatDiv, with
every `expected` count computed by an oracle that shares no code with the joins.
**Related:**
- #95: H2O. Its session owns the generator layer, the new `kermit-generate` crate, and
  `kermit_ds::write_parquet`; this design implements that layer's trait (§ The generator).
  Layer spec: Part 1 of `docs/specs/2026-10-15-h2o-benchmark-design.md` (41537a4 on
  `aidanb/h2o`), § "The `kermit-generate` crate".
- #97: JOB. It reuses this design's `download_verified` (§ The download). Spec:
  `docs/specs/2026-10-15-job-benchmark-design.md` (8358e7e on `aidanb/job`).
- #93: column orders. `cycle4` is written so that `--column-orders any` builds no copy.
- #111: flat tuple batches. The parser's output is a `Tuples`.
- #70: atomic, checked downloads, whose discipline `download_verified` keeps.
- #78: a malformed input is an error naming where, never skipped.

## Summary

kermit has no real-world cyclic workload: LUBM and WatDiv are mostly acyclic, and
`triangle.yml` is a 5-vertex sanity check. Cyclic patterns are where worst-case-optimal
joins should pull ahead. This design adds the SNAP community graphs (Yang & Leskovec,
ICDM 2012) as a generator kind. A spec names a graph, or gives a URL and its SHA-256. The
generator downloads the `.gz`, symmetrises it into `edge(src, dst)`, writes it through the
shared generator layer and emits four queries (`triangle`, `path3`, `cycle4`, `clique4`)
with exact `expected` counts. `benchmarks/snap-amazon.yml` and `benchmarks/snap-dblp.yml`
are committed.

The work lands in two pushes. Phase 1 is kermit-bench's general download alone, which
depends on nothing unlanded and is all #97 needs. Phase 2 is everything else (the spec
variant, the parser, the oracle, the generator, the CLI and the committed YAMLs). It
implements #95's generator trait and lands once that layer is on master (§ Sequencing).

## Decisions

From the 2026-10-05 brainstorm (issue #96), unchanged:

1. **One symmetric `edge` relation.** Each input line (s, d) becomes rows (s, d) then
   (d, s). Kraken's SNAP suite does the same, and SNAP's files are fixed, so the data is
   reproducible byte for byte. Rejected: an `s < d` relation and a degree-oriented one.
2. **Walk semantics.** No query forces its variables to be distinct, so on symmetric data
   `path3` counts 3-step walks and `cycle4` closed 4-step walks. The docs say so
   explicitly.
3. **Two source forms.** A named table (`com-amazon`, `com-dblp`, `com-youtube`, each
   with a pinned URL and `.gz` SHA-256) and a generic `url` with a required `sha256`.
4. **The generator computes every `expected`** with a combinatorial oracle that shares no
   code with the joins. A network-gated test pins amazon and dblp to the eight
   independently verified values.
5. **Cleaning.** Self-loops and duplicate edges are dropped (first occurrence kept) and
   counted, so set and bag counts agree and HashTrie's multiset count matches the sorted
   tries'. A malformed line is an error (kraken skips them).
6. **`cycle4` is written with `edge(A, D)`.** The stored column order rejects `edge(D, A)`
   with `CyclicAttributeOrder`, and on a symmetric relation the two hold the same tuples.

From this session (2026-10-15):

7. **The shared layer belongs to #95.** The H2O session designs it (the trait,
   `RelationSink`, `run`, `meta.json` and `benchmark.yml`) and `kermit_ds::write_parquet`.
   This session sent it #96's requirements; its spec traces all six and lands the layer on
   master as its own push, before any H2O code. The layer is a new workspace crate,
   `kermit-generate` (the user's decision, H2O's D13), not a module of the binary: its
   `pub` items are not dead code, and `kermit/tests/` can reach them.
8. **`write_parquet` is the inverse of `read_parquet`:**
   `write_parquet(path, &RelationHeader, &Tuples)`, chosen over #95's original
   column-major `columns: &[Vec<usize>]` because #111 made `Tuples` the one batch type.
   One round-trip test then pins both directions. Requested of #95, not built here.
9. **#96 builds the general digest-checked download** in kermit-bench, streaming so it
   also serves JOB's 1.26 GB IMDb dump. The JOB session was told.
10. **Graph and query names are types** (`SnapGraph`, `SnapQuery`), so an unknown name
    fails when the YAML is parsed, naming the valid values.
11. **Raw SNAP ids are kept.** Keys are already `usize`; no dictionary and no remapping.
12. **Phases.** Phase 1 (the download) lands independently of #95; phase 2 waits for its
    layer. Amended 2026-10-15 after the benchmark co-ordinator's conflict map: the spec
    variant, parser and oracle moved from phase 1 to phase 2, because phase 1 must build on
    its own (§ Sequencing).

## Measured facts (2026-10-15)

Downloaded from `https://snap.stanford.edu/data/bigdata/communities/` (HTTP 200, no bot
challenge):

| Graph | `.gz` bytes | `.gz` SHA-256 | Edges | `edge` rows | Max id | Max degree | Σ d(v)² |
|---|---|---|---|---|---|---|---|
| `com-amazon` | 4,501,435 | `bb4b9fcf549c9174049e885e9a0a4057c9724846e7297c4a1ea299b8e92eec34` | 925,872 | 1,851,744 | 548,551 | 549 | 2.14 × 10⁷ |
| `com-dblp` | 4,138,463 | `9eb0bd30312ddd04e2624f7c36c0983a2e99b116f0385be5a7fce6d6170f4cb3` | 1,049,866 | 2,099,732 | 425,956 | 343 | 4.57 × 10⁷ |
| `com-youtube` | 10,590,328 | `dff1b97ba7d2fa9c59884b67dcd2275e717ff9501f86ed82ce6582ed4971f3e0` | 2,987,624 | 5,975,248 | 1,157,827 | 28,754 | 2.96 × 10⁹ |

All three have exactly four `#` header lines, no malformed line, no self-loop and no
duplicate edge (unordered pairs compared), and every data line has `s < d`. Ids are sparse
(amazon has 334,863 nodes but ids up to 548,551). The amazon and dblp digests match the
ones pinned in the issue.

## Workload

**Relation.** `edge(src, dst)`, from the cleaned input lines in file order, each
contributing (s, d) then (d, s). It is every SNAP benchmark's only relation, named `edge`
whatever the source: a `url` names the benchmark (`snap-<stem>-<tag>`), never the
relation. `edge` satisfies `RelationSink::open`'s name rule, `[a-z][a-z0-9_]*` (H2O's
spec, 41537a4).

**Queries.**

| Name | Datalog | Counts |
|---|---|---|
| `triangle` | `T(A, B, C) :- edge(A, B), edge(B, C), edge(A, C).` | 6 × triangles |
| `path3` | `P(A, B, C, D) :- edge(A, B), edge(B, C), edge(C, D).` | 3-step walks, 1ᵀA³1 |
| `cycle4` | `C(A, B, C, D) :- edge(A, B), edge(B, C), edge(C, D), edge(A, D).` | closed 4-step walks, tr(A⁴); **not** 8 × 4-cycles |
| `clique4` | `K(A, B, C, D) :- edge(A, B), edge(A, C), edge(A, D), edge(B, C), edge(B, D), edge(C, D).` | 24 × 4-cliques |

Walks that double back (A = C or B = D) are 62% of amazon's `cycle4` and 16.8% of dblp's.

**Verified counts** (scipy closed forms and a direct 4-clique count, 2026-10-05):

| | triangle | path3 | cycle4 | clique4 |
|---|---|---|---|---|
| amazon | 4,002,774 | 206,831,062 | 65,863,072 | 6,623,064 |
| dblp | 13,346,310 | 1,453,845,122 | 530,084,528 | 401,116,608 |

com-youtube's counts are not pinned. Its `path3` and `cycle4` run into the billions, so it
is reachable through `bench gen snap --graph com-youtube` but not committed.

## Design

### The spec (kermit-bench)

```rust
// kermit-bench/src/definition.rs, a new GeneratorSpec variant (`kind: snap`)
Snap {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    graph: Option<SnapGraph>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    queries: Option<Vec<SnapQuery>>,
},

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SnapGraph { ComAmazon, ComDblp, ComYoutube }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SnapQuery { Triangle, Path3, Cycle4, Clique4 }

impl SnapGraph {
    /// The pinned download URL and `.gz` SHA-256 (§ Measured facts).
    pub fn source(self) -> (&'static str, &'static str);
}
```

```yaml
generator:
  kind: snap
  graph: com-amazon              # a named graph, or instead:
  # url: https://…/x.ungraph.txt.gz
  # sha256: <64 lowercase hex of the .gz>
  queries: [triangle, clique4]   # optional; all four by default
```

`validate_generator` gets a `Snap` arm. It requires:
- exactly one of `graph` or `url` (as `RelationSource`'s `url`/`path` pair);
- with `url`: a `sha256`, because `spec_hash` covers the spec's text, not the file's
  content; and with `graph`: no `sha256`;
- `url` to start with `http://` or `https://`;
- `sha256` to be 64 lowercase hex characters;
- `queries`, when present, to be non-empty and free of duplicates.

There is no `scale` field. Adding a named graph is one `SnapGraph` variant and one
`source` arm. `spec_hash` needs no change: the YAML serialisation of the variant covers
every field.

### The download (kermit-bench)

```rust
// kermit-bench/src/cache.rs
/// Downloads `url` to `dest`, streaming it into `<dest>.part` while hashing, and renames it
/// onto `dest` only once its SHA-256 equals `sha256`.
pub fn download_verified(url: &str, dest: &Path, sha256: &str) -> Result<(), BenchError>;
```

- **One streaming core** shared with the relation path: GET; reject a non-200 status and
  a WAF challenge (`x-amzn-waf-action`) before reading the body; stream into
  `<dest>.part` while hashing; reject an empty body (zero bytes streamed); return the
  digest. Every rejection removes `.part` and keeps today's `unusable_download` message as
  a `BenchError::UnusableDownload`.
- **`download_verified`** compares the digest. On a mismatch it removes `.part` and
  returns `BenchError::Integrity`; otherwise it renames `.part` onto `dest`. No partial
  file ever sits at `dest`.
- **The relation path** (`download_file`, called by `ensure_cached`) keeps its optional
  digest. Its `PAR1` check moves from the in-memory body to the `.part` file before the
  rename, using `cached_file_problem`, so there is one definition of "this is a Parquet
  file". A failed check is reported as `UnusableDownload` (not `CorruptCache`) and removes
  `.part`. The existing `download_file` tests then pass unchanged: they assert only that
  the message names the cause ("challenge", "202", "empty", "Parquet") and that neither
  `dest` nor `.part` remains.
- **`BenchError::UnusableDownload`'s message** becomes `download from {url} is unusable:
  {reason}`, since `download_verified` also raises it for a `.gz`. Today's "did not return
  a Parquet file" is wrong for a non-Parquet download. No test or doc pins the old wording,
  and the Parquet case still says "Parquet" through its reason.
- **Client and timeouts.** In reqwest 0.13's blocking client, `ClientBuilder::timeout`
  (default 30 s) bounds each operation: the wait for the response headers, and each
  `Read::read` of the body (`reqwest/src/blocking/response.rs`, `impl Read for
  Response`). `Response::bytes()` instead waits on the whole body under one timeout, so
  today's `download_file` caps a whole download at 30 s, which a 1.26 GB download (JOB)
  would hit below about 42 MB/s. Streaming through `Read` turns the same 30 s into a
  stall limit with no total. The helper's client sets `timeout(30 s)` and
  `connect_timeout(30 s)` explicitly. A loopback test pins the per-read semantics: a body
  trickled in gaps shorter than the timeout, but longer in total, succeeds; a stall longer
  than the timeout fails. A reqwest upgrade that changed the semantics would fail it.
  (Corrected 2026-10-15 while planning: an earlier draft read the async client's docs
  and specified `timeout(None)` plus a per-read timeout, which the blocking client
  doesn't offer.)
- **A failed body read** (a stalled or dropped connection) is a `BenchError::Download`,
  as a failure before the body is. `Response::read` wraps reqwest's error in an
  `io::Error`, which the core unwraps.
- **Progress.** For a body over 64 MiB, a line on stderr every 10 s with the bytes received,
  and the total when the response gives a `Content-Length`. SNAP's files stay silent; JOB's
  1.26 GB dump would otherwise be minutes without output (JOB asked for it, 2026-10-15).
- **`BenchError::Integrity`** generalises its `relation` field to a `subject` holding the
  noun phrase (`relation 'parentcountry'`, `file 'com-amazon.ungraph.txt.gz'`). The
  message stays `integrity check failed for {subject} from {location}: …`, so the
  existing relation messages, and `cli_bench_fetch_integrity.rs`'s assertion on them,
  are byte-identical. One existing test changes: `verify_integrity_reports_a_mismatch`
  (`cache.rs`) destructures `relation` and asserts `"edge"`; it destructures `subject`
  and asserts `"relation 'edge'"` instead.

### The parser (`kermit-generate/src/snap.rs`)

Input: a local `.gz` path. Output: `(RelationHeader, Tuples)` with header `edge(src, dst)`,
plus the counts `meta.json` records.

- Decode with flate2's `MultiGzDecoder` (a new dependency of `kermit-generate`, in its
  own manifest only; `kermit-rdf` already uses flate2 1) and read line by line.
- A line starting with `#` is skipped.
- A data line is exactly two whitespace-separated unsigned integers. SNAP uses tabs, and
  some SNAP graphs use spaces. Anything else, a blank line included, is an error naming its
  1-based line number (comments counted) and its text.
- A self-loop (s = d) is dropped and counted.
- A line whose unordered pair {s, d} has appeared before is dropped and counted once.
  That also covers a reversed repeat (d, s). The check keeps a `HashSet` of normalised
  pairs, about 100 MB at youtube.
- Each kept line pushes (s, d) then (d, s), with raw ids.

The `meta.json` extras are: the graph name or the URL, the `.gz` SHA-256, the input edge
count, and the self-loops and duplicates dropped.

### The count oracle (`kermit-generate/src/snap/counts.rs`)

It reads only the parser's `Tuples`: nothing in `kermit-algos` and none of `kermit-ds`'s
structures. It computes only the selected queries.

- **CSR.** Count degrees over the rows, prefix-sum, fill, then sort each adjacency list.
  Arrays are sized by the largest id plus one (ids are sparse).
- **`path3`** = Σ over rows (b, c) of d(b) · d(c).
- **Orientation**, for `triangle` and `clique4`: u → v when (d(u), u) < (d(v), v). Each
  triangle and 4-clique has exactly one ordering consistent with it, so each is counted
  once, and |N⁺(v)| is O(√m).
- **`triangle`** = 6 × Σ over oriented edges (u, v) of |N⁺(u) ∩ N⁺(v)|, by sorted
  intersection.
- **`clique4`** = 24 × Σ over oriented edges (u, v) of Σ over w ∈ W of |N⁺(w) ∩ W|, where
  W = N⁺(u) ∩ N⁺(v).
- **`cycle4`** = tr(A⁴) = Σ_v Σ_w (A²)_{vw}². For each v, add 1 to `scratch[w]` for every
  2-step walk v → u → w, sum the squares of the touched entries, then reset only those.
  The work is Σ d(u)², which is seconds for amazon and dblp and about ten seconds for
  youtube.
- **Arithmetic** is `u64` with checked adds and multiplies; an overflow is an error, never
  a wrong `expected`.

The oracle exists so that `--verify` means something. A shared CSR or intersection routine
could be wrong in exactly the way LFTJ is, and the 2026-09-08 failed-descent bug survived
because nothing independent checked deep descents on real data. `clique4` on amazon and
dblp, with six atoms and four levels, is that kind of check.

### The generator (phase 2, against #95's layer)

Written against the layer as specified in Part 1 of
`docs/specs/2026-10-15-h2o-benchmark-design.md` (4e0beac). If the landed layer differs,
this section is amended before the phase-2 plan is written.

The SNAP module is `kermit-generate/src/snap.rs` plus `snap/counts.rs`, registered by one
`pub mod snap;` line in the crate's `lib.rs`. It uses the layer through the root
re-exports (`kermit_generate::{Generator, Generated, RelationSink}`). The crate has
`#![deny(missing_docs)]`, so every `pub` item below is documented, and `cargo doc
--workspace` lints those docs; they link to no binary item (`materialize`,
`ExecutionFamily`), which rustdoc cannot reach.

```rust
// kermit-generate/src/snap.rs
pub enum SnapSource {
    /// A local `.gz` (tests).
    Local(PathBuf),
    /// A named graph's pinned pair, or a spec's `url` + `sha256`.
    Remote { url: String, sha256: String },
}

pub struct SnapGenerator<'a> {
    target: Target<'a>,
    /// `Some` for a named graph, recorded in `meta.json`.
    graph: Option<SnapGraph>,
    source: SnapSource,
    /// Already defaulted to all four, in the issue's order, when the spec omits the list.
    queries: Vec<SnapQuery>,
}

#[derive(Serialize, Deserialize)]
pub struct SnapExtras {
    pub graph: Option<SnapGraph>,
    pub source_url: Option<String>,   // `None` for a local source
    pub gz_sha256: String,            // computed from the bytes read, for either source
    pub input_edges: u64,             // data lines read
    pub dropped_self_loops: u64,
    pub dropped_duplicates: u64,
}

impl Generator for SnapGenerator<'_> {
    const KIND: &'static str = "snap";
    type Extras = SnapExtras;
    fn target(&self) -> Target<'_> { self.target }
    fn generate(&self, sink: &mut RelationSink) -> anyhow::Result<Generated<SnapExtras>>;
}
```

`generate`:
1. **Places the `.gz`.** A `Remote` source is fetched with `download_verified` into
   `sink.raw_dir()?`, where it stays for provenance. A `Local` source is copied there.
2. **Parses it** into `edge`'s `(RelationHeader, Tuples)` and writes `edge` once with
   `sink.write(&header, &tuples)`. The oracle needs every row in memory anyway, so there's
   no need for the streaming `open` / `write_batch` form.
3. **Runs the oracle** for the selected queries.
4. **Returns** `Generated { description, queries, extras }`. Each `QueryDefinition` carries
   its rule from § Workload, a one-line description saying what it counts, and `expected`.

`SnapExtras` serialises to a JSON object whose keys avoid every common `GeneratedMeta`
field (`kind`, `tag`, `relations`, …), which `run` enforces (H2O's D14). Everything else
(`benchmark.yml` with `file://` URLs and SHA-256s, `meta.json` written last, the check that
each query parses and names a written relation) is `kermit_generate::run`'s.

The six requirements this session sent the H2O session on 2026-10-15 are traced in that
spec's "Requirements from #96 and #97" table:

1. A raw-artifact directory: `RelationSink::raw_dir()`.
2. `RelationSink::write(&RelationHeader, &Tuples)` over a `write_parquet` that is
   `read_parquet`'s inverse; rows and SHA-256 are recorded in `RelationRecord`.
3. Queries with `expected`: `Generated::queries: Vec<QueryDefinition>`.
4. Typed extras: `GeneratedMeta<E>` with `#[serde(flatten)] extras: E`.
5. Discovery, drift, `--force` and `bench list` unchanged: `GeneratedMeta` implements
   `GeneratorMeta`.
6. No shared `scale`.

### Wiring (phase 2)

- **`materialize::dispatch`** (in the binary) gains a `GeneratorSpec::Snap` arm that
  resolves the source (`SnapGraph::source()` or the spec's pair), builds a
  `SnapGenerator` with `spec_hash`, and calls `kermit_generate::run`.
- **`bench gen snap`** takes `--graph <com-amazon|com-dblp|com-youtube>`, or `--url URL
  --sha256 HEX`, plus `--queries triangle,clique4` (optional), and the shared output flags
  through `#[command(flatten)] target: GenTarget` (`--tag`, required; `--output-dir`,
  optional; H2O's D15). It writes `snap-<name>-<tag>`, where `<name>` is the graph or the
  URL's last path segment up to its first `.` (`com-orkut.ungraph.txt.gz` gives
  `com-orkut`); a name that is not a portable filename is an error. It then calls
  `target.prepare("snap", &bench_name)?`, which refuses a name a workspace YAML uses and
  resolves the directory through `kermit_generate::output_dir`, and runs the generator
  with `spec_hash: None`.
  - **`GenTarget`** lands with whichever of H2O's push 2 and #96's phase 2 comes first
    (H2O's D17). If #96 is first, it copies the struct and `prepare` byte for byte from
    H2O's spec, § Surface, into `kermit/src/main.rs` beside `GenSubcommand`, and the
    later lander drops its copy at merge. If H2O is first, #96 uses the landed one.
  - kermit-bench has no clap dependency, so the CLI declares its own `ValueEnum`s for the
    graph and query names and maps them onto `SnapGraph` and `SnapQuery`, as H2O does for
    `H2oOrder`. A unit test pins that every clap value name equals its serde name.
- **Committed YAMLs.** `benchmarks/snap-amazon.yml` and `benchmarks/snap-dblp.yml`, each
  `generator: { kind: snap, graph: com-<name> }` with all four queries.

## Testing

### Phase 1

- **Download** (kermit-bench, against the existing `serve_once` loopback server):
  - A matching digest writes `dest`.
  - A mismatch leaves neither `dest` nor `.part` and reports both digests.
  - A WAF challenge, a non-200 status and an empty body are named as today.
  - The relation path's existing tests pass unchanged, except the one `Integrity`
    destructure (§ The download).

### Phase 2

- **Spec** (kermit-bench unit tests):
  - Both forms parse.
  - Rejected: both forms at once, neither, a URL without a digest, a `graph` with a digest,
    a malformed digest, a non-HTTP URL, an empty `queries`, a duplicated query.
  - An unknown graph or query name is a parse error naming the valid values.
  - `spec_hash` changes with the graph, URL, digest and queries.
  - Every `SnapGraph::source()` is an `https` URL with a 64-hex digest.
- **Parser** (inline unit tests in `kermit-generate/src/snap.rs`):
  - Comments are skipped.
  - A malformed line errors with its line number and text: one field, three fields,
    non-numeric, negative, blank.
  - Self-loops, exact duplicates and reversed duplicates are dropped and counted.
  - Rows come out (s, d), (d, s) in file order.
- **Oracle** (inline unit tests in `kermit-generate/src/snap/counts.rs`):
  - Hand-checked: K₄ gives triangle 24, clique4 24, path3 108, cycle4 84; C₄ gives
    triangle 0, clique4 0, path3 32, cycle4 32.
  - On seeded random small graphs, every count equals a brute-force nested-loop evaluation
    of the query's rule over vertex tuples, written fresh in the test.
  - Overflow is an error.
- **Fixture:** `kermit/tests/fixtures/snap/mini.ungraph.txt`, committed as plain text so it
  reads in a diff: a header comment, a 4-clique, a 4-cycle, a hub, one self-loop and one
  reversed duplicate. Tests gzip it on the fly with flate2, so a loopback test's digest is
  computed from the bytes actually served and never goes stale.
- **End to end** (`kermit/tests/snap_generate.rs`, beside H2O's `h2o_generate.rs`; it
  joins through `kermit::db`, the library entry points, so it needs nothing of the
  binary):
  - The fixture goes through `kermit_generate::run` with a `Local` source into a temp
    directory.
  - All four queries run on the three cells × every `Optimiser::value_variants()` × both
    `--column-orders` policies, through the real entry points; every count equals the
    emitted `expected`.
  - Under `stored`, all four queries validate for every optimiser (no
    `CyclicAttributeOrder`), which pins decision 6: a `cycle4` rewritten to `edge(D, A)`
    fails here. Under `any`, copies are the planner's business and are not asserted, since
    an optimiser may legitimately choose an order that needs one.
  - Generating twice in one build gives a byte-identical `edge.parquet`. Bytes are stable
    only for a fixed build, since the Parquet footer records the writer's `created_by`
    version.
  - `meta.json` round-trips as `GeneratedMeta<SnapExtras>` with the fixture's counts
    (input edges, one self-loop and one duplicate dropped).
- **CLI** (`kermit/tests/cli_snap_gen.rs`; `XDG_CACHE_HOME` and `KERMIT_WORKSPACE` in temp
  dirs, so the shared cache is never touched):
  - `bench gen snap --url http://127.0.0.1:… --sha256 …` generates, and
    `bench run … -i all -a all -m iteration --verify` passes.
  - A wrong `--sha256` exits non-zero and leaves no cache subdir.
  - A workspace `kind: snap` YAML materialises on `bench run`, is a hit the second time,
    raises `SpecDrift` after an edit and regenerates under `--force`. #95's tests cover
    the general mechanism; this pins SNAP's `dispatch` arm.
- **Reference counts** (an inline unit test in `kermit-generate/src/snap.rs`, beside the
  parser and oracle tests): downloads amazon and dblp (8.6 MB) into a temp directory through
  `download_verified` and checks the eight verified values through the parser and the
  oracle. A `BenchError::Download` (connection, DNS) skips with a note on stderr; any
  other error, a digest mismatch included, fails.

## Out of scope

- The shared generator layer and `kermit_ds::write_parquet` (#95).
- A com-youtube reference-count test, and committing its YAML.
- Distinct-variable (simple path and simple cycle) variants of `path3` and `cycle4`.
- Adding SNAP to a sweep, which is a separate decision.
- Parallelising the oracle.

## Acceptance

- [ ] `benchmarks/snap-amazon.yml` and `snap-dblp.yml` generate on `bench run`; a cache
      hit, drift, `--force` and `bench list` behave as for LUBM.
- [ ] `bench gen snap` works in both forms, including `--graph com-youtube`, and a wrong
      SHA-256 fails before parsing and leaves no cache subdir.
- [ ] Generated counts for amazon and dblp equal the eight verified values, and
      `bench run snap-amazon -i all -a all --verify` (and `snap-dblp`) passes on all three
      cells.
- [ ] Regenerating the same source with the same build gives byte-identical Parquet;
      `meta.json` records the `.gz` SHA-256, the relation SHA-256 and the dropped rows.
- [ ] `download_verified` is public in kermit-bench and streams; the relation path's
      behaviour is unchanged and its messages still name the cause.
- [ ] The tests above pass, and CI is green.

## Documentation

- `docs/benchmarks/SNAP.md`: source and citation; the graph table with all three digests
  and the measured facts; symmetrisation and cleaning; the four queries with exact
  semantics ("closed 4-step walks") and the verified counts; the `edge(A, D)` rewrite;
  costs (dblp `path3` is 1.45 billion rows; youtube not committed).
- `benchmarks/README.md`: the `snap` kind and its fields. The common-fields table stops
  marking `generator.scale` as required for every kind (today's row 59). JOB needs the
  same edit, so whichever of #96 and #97 lands first changes the row and the other keeps
  it.
- `CLAUDE.md`: a Benchmark Reference Docs entry for `SNAP.md`; in the workspace table,
  `snap` added to the `bench gen […]` list and to `kermit-generate`'s entry, each as it
  reads on master at landing (every generator adds only its own kind); a gotcha on
  `download_verified` (streaming, required digest, no 30 s total timeout), landing with
  phase 1; a `bench gen snap` line in Build Commands.

## Sequencing

- **Phase 1** is kermit-bench alone: `download_verified`, the shared streaming core, the
  `Integrity` and `UnusableDownload` message changes, their tests, and the CLAUDE.md
  gotcha. It touches neither `GeneratorSpec` nor the binary, so it builds and lands on
  master by itself, at any time.
- **Phase 2** is everything else, landed as one push: `SnapGraph`, `SnapQuery` and
  `GeneratorSpec::Snap` with its `validate_generator` arm; the parser and the oracle
  (`kermit-generate/src/snap.rs`, `snap/counts.rs`); the generator, the `dispatch` arm,
  `bench gen snap` (with `GenTarget`, if H2O's push 2 hasn't landed it), the YAMLs, the
  tests and the docs. It starts when the H2O session's push 1 (the layer alone, before
  any H2O code) is on master: merge `origin/master` into this branch (never rebase), check
  § The generator against the landed interface (amend it if needed), then write the
  phase-2 plan. The crate, its `lib.rs` and its entry in `kermit/Cargo.toml` come from
  push 1, so #96 adds one `pub mod snap;` line and `flate2` to `kermit-generate`'s
  manifest; H2O and JOB each add their own line, a one-line add/add conflict at most.
- **Why the split falls there.** A `GeneratorSpec::Snap` variant without its
  `materialize::dispatch` arm fails to build, because that match (`materialize.rs:261`) is
  exhaustive, with no `_` arm. A stub `dispatch` arm (`bail!`) was rejected: it would put
  a `kind: snap` on master that validates but cannot generate. The parser and the oracle
  live in `kermit-generate`, which push 1 creates. (While the layer was planned as a
  module of the binary, they also had no caller in `main`, which `-Dwarnings` makes a
  `dead_code` error; in a library crate their `pub` items are not dead.)
- **Ordering with #97.** JOB builds on `download_verified` once phase 1 lands.
