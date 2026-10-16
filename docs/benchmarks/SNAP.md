# SNAP (Stanford Network Analysis Project community graphs)

SNAP is Stanford's collection of real network datasets. Its "com-" graphs, introduced
by Yang and Leskovec (2012), are large undirected social and product networks
published with ground-truth communities: Amazon co-purchases, DBLP co-authorship and
YouTube friendships. Kermit uses only their edges. Each graph becomes one symmetric
`edge(src, dst)` relation, and four graph patterns run over it: `triangle`, `path3`,
`cycle4` and `clique4`. Where LUBM and WatDiv are mostly acyclic, three of these are
cyclic, over real skewed degree distributions, which is the workload a worst-case
optimal join should win on. Every query carries an exact `expected` count, computed
by an oracle that shares no code with the joins, so `--verify` checks real data.

Unlike LUBM and WatDiv, SNAP has no RDF and no external generator: the generator
downloads a pinned `.gz`, parses it and writes Parquet itself (`kermit-generate`,
`kermit-generate/src/snap.rs`). For a side-by-side comparison with the other
benchmarks in the suite, see [`README.md`](README.md#cross-benchmark-comparison).

## How to run

### On-the-fly generation

```bash
kermit bench gen snap (--graph com-amazon|com-dblp|com-youtube | --url URL --sha256 HEX) \
    --tag STR [--queries triangle,path3,cycle4,clique4] [--output-dir PATH]
```

Downloads the graph (checking its SHA-256 first), writes
`~/.cache/kermit/benchmarks/snap-<name>-<tag>/`, and prints
`snap benchmark written to <dir>`. Run it with the unmodified `kermit bench run`:

```bash
kermit bench gen snap --graph com-amazon --tag dev
kermit bench run snap-com-amazon-dev -i all -a all --verify
```

`<name>` is the graph's name, or for `--url` the last path segment of the URL up to its
first `.` (`…/com-orkut.ungraph.txt.gz` gives `com-orkut`). The bench name is
collision-checked against committed YAMLs at runtime.

| Flag | Default | Notes |
|------|---------|-------|
| `--graph` | one of `--graph` / `--url` required | `com-amazon`, `com-dblp` or `com-youtube`; each has a pinned URL and digest. Conflicts with `--url` and `--sha256` |
| `--url` | — | The URL of a SNAP edge-list `.gz` (`http://` or `https://`). Requires `--sha256` |
| `--sha256` | — | SHA-256 of the `.gz` at `--url`, 64 lowercase hex. Compute it with `sha256sum`. Requires `--url` |
| `--queries` | all four | Comma-separated subset of `triangle`, `path3`, `cycle4`, `clique4` |
| `--tag` | required | Suffix on the bench name |
| `--output-dir` | `~/.cache/kermit/benchmarks` | Custom output dirs are NOT auto-discovered by `bench list/fetch/run` |

A digest mismatch fails before anything is parsed and leaves no cache: the directory stays, but without `meta.json` it is not discovered, and `raw/` holds no `.gz` or `.part`.
`--url` is for SNAP-format edge lists (see
[Inference / preprocessing](#inference--preprocessing)); the digest is required
because a spec's text, and so its `spec_hash`, says nothing about the file's bytes.

### Declarative YAML spec (commit-and-run)

`benchmarks/snap-amazon.yml` and `benchmarks/snap-dblp.yml` are committed. Both have
this shape (`snap-dblp.yml` says `com-dblp` in the description and `graph`), and `bench run snap-amazon` downloads and generates on first invocation:

```yaml
# benchmarks/snap-amazon.yml
name: snap-amazon
description: "SNAP com-amazon (Yang & Leskovec, ICDM 2012) as one symmetric edge relation, with the triangle, path3, cycle4 and clique4 queries (walk semantics)."
generator:
  kind: snap
  graph: com-amazon              # a named graph, or instead:
  # url: https://…/x.ungraph.txt.gz
  # sha256: <64 lowercase hex of the .gz>
  # queries: [triangle, clique4] # optional; all four, in this order, by default
```

Required: exactly one of `graph` and `url`; with `url`, a `sha256` (and with `graph`,
none). Optional: `queries`, which must be non-empty and free of duplicates. There is no
`scale`. An unknown graph or query name is a parse error that lists the valid names, and
`BenchmarkDefinition::validate` rejects the other mistakes (both forms, neither, a
`url` that is not `http(s)://`, a `sha256` that is not 64 lowercase hex).

Editing the YAML and re-running errors with `SpecDrift` — pass `--force` to opt into
regenerating. See `benchmarks/README.md` for the full schema.

## Pipeline

```
com-<x>.ungraph.txt.gz           kermit-bench::cache::download_verified
(snap.stanford.edu, pinned)              │  streamed into .part, SHA-256 checked,
                                         │  renamed only on a match
                                         ▼
                                  raw/com-<x>.ungraph.txt.gz
                                         │
                                         │ kermit-generate::snap::parse_gz
                                         │ (gunzip, clean, symmetrise)
                                         ▼
                                  edge(src, dst) rows ──────────────┐
                                         │                          │ snap::expected_counts
                                         │ RelationSink::write      │ (count oracle)
                                         ▼                          ▼
                                    edge.parquet          expected for each query
                                         └────────────┬─────────────┘
                                                      ▼
                                       benchmark.yml + meta.json
```

The oracle reads the parsed rows, never the Parquet file or any `kermit-ds` or
`kermit-algos` structure.

## Output layout

```
~/.cache/kermit/benchmarks/snap-com-amazon-dev/
  meta.json                          kind = "snap"; written last, so its presence marks a finished cache
  benchmark.yml                      relation `edge` (file:// URL + SHA-256) and the selected queries, each with `expected`
  edge.parquet                       edge(src, dst): two rows per kept input edge
  raw/com-amazon.ungraph.txt.gz      the verified download, kept for provenance
```

A declarative spec writes the same layout into a directory named after the YAML
(`snap-amazon`). `meta.json` has the fields every generated benchmark has (`kind`,
`tag`, `generated_at_utc`, `spec_hash` for a declarative run, and each relation's name,
columns, rows and SHA-256) plus SNAP's own:

| Field | Meaning |
|-------|---------|
| `graph` | The named graph (`com-amazon`…), or `null` for a `url` |
| `source_url` | The URL downloaded, or `null` for a local source (tests) |
| `gz_sha256` | SHA-256 of the `.gz`, computed from the bytes on disk |
| `input_edges` | Data lines read (comment lines excluded) |
| `dropped_self_loops` | Lines with `s = d`, dropped |
| `dropped_duplicates` | Lines whose unordered pair had already appeared, dropped |

## Inference / preprocessing

SNAP files are edge lists: `#` comment lines, then one `s d` pair per line, separated
by a tab or spaces. The parser keeps the raw ids (keys are `usize` already, so there is
no dictionary and no remapping; amazon has 334,863 nodes but ids up to 548,551).

| Rule | Effect | Recorded |
|------|--------|----------|
| Line starts with `#` | Skipped | — |
| Self-loop (`s = d`) | Dropped | `dropped_self_loops` |
| Unordered pair seen before (a reversed repeat included) | Dropped; the first occurrence is kept | `dropped_duplicates` |
| Each kept line `(s, d)` | Becomes rows `(s, d)` then `(d, s)`, in file order | `relations[0].rows` |
| Not exactly two unsigned integers (a blank line included) | Error: `<source>:<line>: malformed edge line …`, with the 1-based line number, comments counted | — |
| No line survives | Error: `<source> has no edges after dropping <n> self-loops and <m> duplicates` | — |

Dropping self-loops and duplicates makes set and bag counts agree, so HashTrie's
multiset count matches the sorted tries'. A malformed line is an error, never skipped.
An edgeless input is an error because a benchmark with no data measures nothing. This
only matters for a `url`: all three named graphs have exactly four header lines, no
malformed line, no self-loop and no duplicate, and every data line has `s < d`.

## Workload reference

### The graphs

Measured 2026-10-15 from `https://snap.stanford.edu/data/bigdata/communities/`. The
downloads are pinned by the SHA-256 of the `.gz`.

| Graph | `.gz` bytes | `.gz` SHA-256 | Edges | `edge` rows | Max id | Max degree | Σ d(v)² |
|---|---|---|---|---|---|---|---|
| `com-amazon` | 4,501,435 | `bb4b9fcf549c9174049e885e9a0a4057c9724846e7297c4a1ea299b8e92eec34` | 925,872 | 1,851,744 | 548,551 | 549 | 2.14 × 10⁷ |
| `com-dblp` | 4,138,463 | `9eb0bd30312ddd04e2624f7c36c0983a2e99b116f0385be5a7fce6d6170f4cb3` | 1,049,866 | 2,099,732 | 425,956 | 343 | 4.57 × 10⁷ |
| `com-youtube` | 10,590,328 | `dff1b97ba7d2fa9c59884b67dcd2275e717ff9501f86ed82ce6582ed4971f3e0` | 2,987,624 | 5,975,248 | 1,157,827 | 28,754 | 2.96 × 10⁹ |

Each URL is `https://snap.stanford.edu/data/bigdata/communities/<graph>.ungraph.txt.gz`.
`com-youtube`'s maximum degree is 28,754 against a few hundred for the other two, which
is why its `path3` and `cycle4` run into the billions (see
[Practical scale ceiling](#practical-scale-ceiling)).

### The queries

The relation is `edge(src, dst)`, symmetric: every undirected edge is stored in both
directions. No query forces its variables to be distinct, so the queries count
**walks**, not simple paths or simple cycles.

| Name | Datalog | Counts |
|------|---------|--------|
| `triangle` | `T(A, B, C) :- edge(A, B), edge(B, C), edge(A, C).` | 6 × triangles |
| `path3` | `P(A, B, C, D) :- edge(A, B), edge(B, C), edge(C, D).` | 3-step walks, 1ᵀA³1; vertices may repeat (A = C is allowed) |
| `cycle4` | `C(A, B, C, D) :- edge(A, B), edge(B, C), edge(C, D), edge(A, D).` | closed 4-step walks, tr(A⁴); **not** 8 × 4-cycles |
| `clique4` | `K(A, B, C, D) :- edge(A, B), edge(A, C), edge(A, D), edge(B, C), edge(B, D), edge(C, D).` | 24 × 4-cliques |

- **`triangle` and `clique4`** relate every pair of their variables by an edge, and
  `edge` has no self-loops, so their variables are always distinct. They count true
  triangles and 4-cliques, once per ordering of the vertices (3! and 4!).
- **`path3` and `cycle4`** relate only adjacent variables, so non-adjacent ones may
  coincide. A walk that doubles back (`A = C` or `B = D`) is a row. Such walks are 62%
  of amazon's `cycle4` count and 16.8% of dblp's, so `cycle4`'s count is far from
  eight times the number of 4-cycles.
- **Why `cycle4` is written `edge(A, D)`.** The natural last atom is `edge(D, A)`, but
  under `--column-orders stored` (the default) every atom's variables must follow its
  stored column order, and `edge(A, B)`, `edge(B, C)`, `edge(C, D)` already force
  A before B before C before D, so `edge(D, A)` is `CyclicAttributeOrder`. The relation
  is symmetric, so `edge(A, D)` holds the same tuples and the same answers, and the
  query plans without a reordered copy. Write your own cycle queries the same way.

### Verified counts

Scipy closed forms and a direct 4-clique count (2026-10-05), which the generator's
oracle reproduces:

| | `triangle` | `path3` | `cycle4` | `clique4` |
|---|---|---|---|---|
| `com-amazon` | 4,002,774 | 206,831,062 | 65,863,072 | 6,623,064 |
| `com-dblp` | 13,346,310 | 1,453,845,122 | 530,084,528 | 401,116,608 |

That is 667,129 triangles and 275,961 4-cliques in amazon, and 2,224,385 triangles and
16,713,192 4-cliques in dblp. `com-youtube`'s counts are not pinned.

### How `expected` is computed

Every query's `expected` comes from `kermit-generate/src/snap/counts.rs`, which reads
only the parsed rows. It shares no code with the joins, so a bug in a trie, an
intersection routine or an optimiser cannot make the count wrong in the same way.

- **`path3`**: the sum over rows (b, c) of d(b) · d(c).
- **`triangle`, `clique4`**: edges are oriented from the lower to the higher
  (degree, id), so each triangle or 4-clique is counted once; the oriented neighbour
  lists are intersected, then the count is scaled by 6 or 24.
- **`cycle4`**: tr(A⁴), the sum over v and w of (A²)[v][w] squared, tallying the 2-step walks
  from each v.
- All arithmetic is checked `u64`: an overflow is an error, never a wrong `expected`.

`bench run snap-amazon -i all -a all --verify` then runs each query once and aborts on a
mismatch. `clique4`, with six atoms and four levels, checks deep leapfrog descents on real data,
which the small fixtures of the join test suites cannot.

## Determinism

The `.gz` is pinned by its digest, and parsing is a pure function of it: the same file
always gives the same rows in the same order and the same counts. The Parquet bytes are
identical when regenerating with the same build; they can differ across builds, since
the file's footer records the Parquet writer's version. `meta.json` records the `.gz`
SHA-256, each relation's SHA-256 and the rows dropped, so a cache can be identified
after the fact.

## Practical scale ceiling

Result sizes are the cost, not the data, which is at most 6 million rows. The joins
stream every answer to a counter and never collect, but timing them still enumerates
every row:

| Graph | `triangle` | `path3` | `cycle4` | `clique4` |
|---|---|---|---|---|
| `com-amazon` | 4.0 M | 207 M | 66 M | 6.6 M |
| `com-dblp` | 13.3 M | **1.45 billion** | 530 M | 401 M |
| `com-youtube` | not pinned | billions | billions | not pinned |

- **`com-youtube` is generated on request and not committed.** `bench gen snap --graph
  com-youtube` works (its Σ d(v)² is 2.96 × 10⁹), but a `path3` or `cycle4` run on it
  is far beyond the other two; use `--queries triangle,clique4` for a lighter run.
- **Never write a result to disk.** `kermit join` writes every row as CSV; on dblp's
  `path3` that is 1.45 billion rows. `bench run` only counts.
- **`bench run --all` includes both committed YAMLs**, as it does every workspace
  benchmark: it downloads amazon and dblp and times every query on both, dblp's `path3`
  included. Name the benchmarks you want when that is too much.
- **The oracle** runs in seconds for amazon and dblp and about ten seconds for youtube
  (`cycle4`, the dearest), over all rows in memory (youtube: about 96 MB). **The
  parser** also keeps a set of the unordered pairs it has seen (about 100 MB at
  youtube) to drop duplicates.
- **Downloads** are 4.5 MB, 4.1 MB and 10.6 MB. A download that stalls for 30 s fails;
  there is no total time limit.

## Vendored generator

Nothing is vendored: SNAP publishes plain gzipped edge lists, and kermit parses them
itself (`kermit-generate/src/snap.rs`).

| Field | Value |
|---|---|
| Source URL | `https://snap.stanford.edu/data/bigdata/communities/com-<name>.ungraph.txt.gz` |
| Version pin | the `.gz` SHA-256 per graph (§ Workload reference); SNAP's files carry no version or tag |
| Build toolchain | none (no external tool runs) |
| Vendor path | none |
| Committed vs fetched | nothing committed; the `.gz` is downloaded on generation and kept in `raw/` of the benchmark's cache directory |

To regenerate, run `bench run --force <name>`, which re-downloads and re-parses the
graph, or delete the cache directory. A changed upstream file fails its pinned digest
instead of silently changing the data.

## Tests

| Test | What it validates | Gate |
|------|-------------------|------|
| `kermit-bench/src/definition.rs` (`snap_*` unit tests) | Names round-trip through serde; each pinned source is an `https://snap.stanford.edu/` URL ending `<name>.ungraph.txt.gz` with a 64-hex digest; both spec forms parse; each bad combination is rejected; `spec_hash` changes with every field | always |
| `kermit-generate/src/snap.rs` (parser tests) | Comments skipped; a malformed line names its number and text; self-loops and duplicates (exact and reversed) dropped and counted; rows symmetrised in file order; an edgeless file rejected with its counts; a gzipped list; the URL stem | always |
| `kermit-generate/src/snap/counts.rs` | K₄ gives 24, 108, 84, 24 and C₄ gives 0, 32, 32, 0 (`triangle`, `path3`, `cycle4`, `clique4`); 50 seeded random graphs equal a fresh nested-loop count; an empty query list counts nothing | always |
| `kermit-generate/src/snap.rs` (`rules_are_the_spec_queries`, `a_local_fixture_generates_a_cache`) | Each rule parses and names only `edge`; a local fixture generates a cache whose `expected` values, extras and Parquet round-trip are right | always |
| `kermit-generate/src/snap.rs` (`reference_counts_match_the_verified_values`) | Downloads amazon and dblp (8.6 MB) through `download_verified` and checks the eight values above through the parser and the oracle | network; skips with a note on stderr when the connection, DNS or a timeout fails (a `Download` error with no HTTP status); a 4xx/5xx status or a digest mismatch fails |
| `kermit/tests/snap_generate.rs` | The fixture, generated through `kermit_generate::run`, joins to its `expected` count for all four queries on the three cells × every optimiser × both `--column-orders` policies; no query is rejected under `stored` (this pins the `edge(A, D)` rewrite); two generations give byte-identical `edge.parquet`; `meta.json` round-trips | always |
| `kermit/tests/cli_snap_gen.rs` | `bench gen snap --url` against a loopback server, then `bench run … -i all -a all -m iteration --verify`; a wrong `--sha256` exits non-zero and leaves no cache: the directory stays, but without `meta.json` it is not discovered, and `raw/` holds no `.gz` or `.part`; a workspace `kind: snap` YAML materialises, is a hit the second time, raises `SpecDrift` after an edit and regenerates under `--force` | Linux; loopback only, no internet |
| `kermit/src/bench/gen.rs` (`snap_clap_names_match_the_spec_names`, `snap_flags_conflict_and_require_as_documented`, `snap_bench_name_follows_the_graph_or_the_url_stem`) | The CLI's value names equal the spec's serde names; `--graph` and `--url`/`--sha256` conflict and require as documented; the benchmark name follows the graph or the URL's stem | always |
| `kermit/src/materialize.rs` (`run_snap_without_a_source_is_an_error`, `generator_failure_keeps_the_context_chain`) | A `snap` spec with neither source fails cleanly; a generator's `anyhow` context chain reaches the error message | always |

The fixture is `kermit/tests/fixtures/snap/mini.ungraph.txt`: a header comment, a
4-clique, a 4-cycle, a hub of degree 5 or more, one self-loop and one reversed
duplicate. The tests gzip it on the fly, so a digest is always computed from the bytes
served.

## References

- J. Yang, J. Leskovec. *Defining and Evaluating Network Communities based on
  Ground-truth.* ICDM 2012.
- T. L. Veldhuizen. *Leapfrog Triejoin: a worst-case optimal join algorithm.*
  ICDT 2014.
- SNAP datasets: <https://snap.stanford.edu/data/>
- Downloads: <https://snap.stanford.edu/data/bigdata/communities/>
- Design: `docs/specs/2026-10-15-snap-benchmark-design.md`
- The generator: `kermit-generate/src/snap.rs` and `kermit-generate/src/snap/counts.rs`
- Sibling benchmark references: [`LUBM.md`](LUBM.md), [`WATDIV.md`](WATDIV.md)

## Future work

- **A `com-youtube` reference-count test** and a committed YAML for it, once its counts
  are pinned independently and a `path3`/`cycle4` run on it is judged worth the time.
- **Distinct-variable variants** of `path3` and `cycle4` (simple paths and simple
  cycles), if a thesis chapter needs counts that do not include doubled-back walks.
- **SNAP in the thesis sweep.** The sweep's scripts name their benchmarks, and adding
  `snap-amazon` and `snap-dblp` to them is a separate decision (`bench run --all`
  already includes them; see [Practical scale ceiling](#practical-scale-ceiling)).
- **A parallel oracle**, if a much larger graph makes the single-threaded count slow.
