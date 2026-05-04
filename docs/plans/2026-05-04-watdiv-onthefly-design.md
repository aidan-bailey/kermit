# WatDiv On-The-Fly Generation — Design

**Date:** 2026-05-04
**Status:** Awaiting implementation plan
**Supersedes:** Extends `2026-04-17-watdiv-benchmark-design.md` by adding a generation path

## Context

The 2026-04-17 design established offline precompute of WatDiv artifacts: a
one-time pipeline (currently `scripts/watdiv-preprocess/`, Python) converts a
fixed `.nt` dataset plus `.sparql` queries into the dictionary-encoded Parquet
+ YAML artifacts that `kermit bench run` consumes. Twelve `watdiv-stress-*.yml`
files plus their ZivaHub-hosted Parquet artifacts are the result.

This design adds an **on-the-fly generation path**: a new
`kermit bench watdiv-gen` subcommand that drives the upstream WatDiv binary at
arbitrary scale factors and stress parameters, runs the equivalent of the
Python preprocessor in pure Rust, and writes a complete artifact set to the
existing benchmark cache directory. The artifacts are then runnable via the
unchanged `kermit bench run`.

### Why this is needed

The committed WatDiv stress YAMLs are pinned to the scale factors (100, 1000)
and stress parameters baked into one historical Python pipeline run. Research
flexibility — exploring how LFTJ scales across SF=2, SF=7, SF=50, etc., or
trying different `<max-query-size>` and `<constants-per-query>` values —
requires regeneration. Manually re-running Python and re-uploading to ZivaHub
each time is friction; the goal is a single Rust subcommand that produces a
ready-to-run benchmark.

### Key constraint discovered empirically

The upstream WatDiv binary is **non-deterministic by construction**. Two
back-to-back invocations of `./watdiv -d <model> 1` produce different output
sizes (15.0 MB vs 15.7 MB), different triple counts (109,961 vs 114,711), and
different content from the very first line — distinct random literal text,
distinct names, distinct user IDs, distinct optional-attribute presence. The
binary imports `srand`, `time`, `gettimeofday`, and
`std::random_device::_M_init/_M_getval`; no `--seed` flag exists. There is no
way to make watdiv produce reproducible output from the CLI.

This forces a particular shape on the design: **each generation produces a
fresh, locally-owned artifact set tagged by the user**, never an attempt to
byte-reproduce a prior set. The committed `watdiv-stress-*.yml` snapshots
remain the canonical reproducible reference for thesis citations; on-the-fly
generations are *additional* artifacts with their own provenance.

## Decisions (locked from brainstorm)

1. **Motivation = research flexibility.** Accept fingerprint drift as the cost
   of parametric freedom. Committed snapshots stay as-is for reproducible
   citations; generated snapshots are additions, not replacements.
2. **Scope = full pipeline.** A single `watdiv-gen` invocation drives `-d`,
   `-s`, and `-q` modes and produces a complete kermit benchmark artifact set
   (`.nt`, templates, queries, dict.parquet, per-predicate parquets, YAML,
   expected results).
3. **Coupling = generate-then-bench, tagged.** `kermit bench watdiv-gen
   --scale N --tag X` writes to `~/.cache/kermit/benchmarks/
   watdiv-stress-N-X/`; subsequent `kermit bench run watdiv-stress-N-X` reads
   the cached artifacts unchanged. Generation and benchmarking are decoupled
   commands.
4. **Implementation = pure Rust.** No Python at runtime. The existing
   `scripts/watdiv-preprocess/` is retired in Phase 2 of the migration.
5. **Topology = one new crate `kermit-rdf`.** Holds the watdiv binary driver,
   N-Triples parsing, dictionary building, predicate partitioning, Parquet
   I/O, SPARQL parsing, SPARQL→Datalog translation, and expected-results
   computation. `kermit-bench` keeps its definitions, cache, and
   single-root-discovery API; a small additive change extends discovery
   to also walk a cache root (see Discovery integration). The `kermit`
   binary's `bench` subcommand gains a `watdiv-gen` child.

## Architecture

### Crate topology

```
kermit-iters    (unchanged — zero deps)
kermit-derive   (unchanged)
kermit-parser   (unchanged — Datalog parsing only; no SPARQL)
kermit-ds       (unchanged)
kermit-algos    (unchanged — already depends on kermit-parser)
kermit-rdf      NEW — see modules below
kermit-bench    (definitions + cache unchanged; `discovery` extended to walk
                 a second root — see Discovery integration)
kermit          (binary; gains `bench watdiv-gen` subcommand)
```

`kermit-rdf` depends on `kermit-parser` (for the Datalog AST type produced by
the SPARQL translator and consumed when emitting `benchmark.yml`) and on
`kermit-algos` (for executing the translated Datalog against in-memory
dict-encoded data to compute expected results). It does **not** depend on
`kermit-bench`; the `kermit` binary's `watdiv-gen` handler invokes
`kermit-rdf` and writes results into the cache directory whose layout
`kermit-bench` already understands.

### `kermit-rdf` module breakdown

| Module | Responsibility |
|---|---|
| `driver/mod.rs` | Public API: `generate(opts: GenerateOptions) -> Result<GenerateOutput>` |
| `driver/sandbox.rs` | bwrap detection, env construction, `/usr/share/dict/words` bind-mount, working-dir staging |
| `driver/invoke.rs` | `Command` construction for watdiv `-d`, `-s`, `-q`; stdout capture; error mapping |
| `value.rs` | Typed RDF value model (Iri, Literal, BlankNode); thin wrappers around `oxrdf` types as needed |
| `ntriples.rs` | Streaming N-Triples parser via `oxttl::NTriplesParser`; yields `(Iri, Iri, Value)` |
| `dict.rs` | `Dictionary` type: bidirectional `Value ↔ usize` map; `intern`, `lookup`; serialization to Parquet |
| `partition.rs` | Group dict-encoded triples by predicate IRI; produce `Vec<(usize, usize)>` per predicate |
| `parquet.rs` | Write dict + per-predicate tables to Parquet (schema: dict = `(id: u64, value: string)`; predicate = `(s: u64, o: u64)`) |
| `sparql/parser.rs` | Thin wrapper around `spargebra::Query::parse` |
| `sparql/translator.rs` | SPARQL algebra → Datalog rule translation (the meaty port of `sparql_translator.py`) |
| `sparql/bindings.rs` | Variable-name bookkeeping; fresh-variable generation for blank nodes and constant atoms |
| `expected.rs` | Execute Datalog (via `kermit-algos`) against the in-memory dict-encoded data; write CSV result rows |
| `error.rs` | `RdfError` enum (thiserror) |

### Workspace deps added

- `oxttl` — N-Triples streaming parser
- `oxrdf` — RDF value types (transitive via oxttl, may be a direct dep)
- `spargebra` — SPARQL parser and algebra types
- `arrow` — Arrow array building (transitive via parquet)
- `parquet` — Parquet writer

All four are mature, MIT/Apache-licensed, widely used in the Rust RDF
ecosystem (Tpt's `oxigraph` is the upstream coordinator).

## Data flow

A single `kermit bench watdiv-gen --scale N --tag T` invocation runs six
sequential stages:

```
stage 1: drive watdiv -d  →  data.nt
stage 2: drive watdiv -s  →  templates/*.sparql
stage 3: drive watdiv -q  →  queries/*.sparql
stage 4: parse + dict + partition + parquet
                          →  dict.parquet
                          →  relations/<predicate>.parquet  (one per distinct predicate)
stage 5: parse + translate SPARQL
                          →  benchmark.yml
stage 6: read watdiv .desc cardinalities (one int per query)
                          →  expected/<query>.csv  (single-line `cardinality\n<N>\n`)
                          →  meta.json (written last)
```

Stages 1–3 are three distinct watdiv invocations sharing one sandbox setup
(bwrap, working-dir staging, env). Stages 4–6 are pure Rust within a single
process. `meta.json` is written last as the "this entry is complete" marker;
its absence indicates a partial / interrupted generation.

**Stage 6 scope note:** the original spec called for in-memory Datalog
evaluation that emits full result rows. The implementation instead reads
watdiv's per-query `.desc` cardinality file and writes a one-line CSV
(`cardinality\n<N>\n`). This is sufficient to validate join engines
("does the row count match watdiv's expectation?") without re-implementing
a Datalog interpreter inside `kermit-rdf`. Full result-row capture is
out of scope and would belong in a future engine-agnostic verifier.

### Cache directory layout

```
~/.cache/kermit/benchmarks/watdiv-stress-<scale>-<tag>/
  meta.json                         provenance: watdiv binary sha256, scale,
                                    stress params, timestamp, kermit git SHA,
                                    names-file sha256s, model-file sha256
  data.nt                           raw watdiv -d output
  templates/*.sparql                raw watdiv -s output
  queries/*.sparql                  concrete watdiv -q output
  dict.parquet                      (id: u64, value: string)
  relations/<predicate>.parquet     per-predicate (s: u64, o: u64) tables
  benchmark.yml                     kermit-bench-loadable Datalog YAML
  expected/<query>.csv              ground-truth result rows per query
```

This is identical to the layout `kermit bench fetch` produces today, so
`kermit bench run watdiv-stress-<scale>-<tag>` requires no runner changes.

## WatDiv binary location and portability

### Vendoring

The 360 KB binary, its data files, the model file, license, and version
identifier are vendored under the `kermit-rdf` crate:

```
kermit-rdf/
  vendor/
    watdiv/
      bin/Release/watdiv          (360 KB ELF, x86_64 Linux)
      files/firstnames.txt        (1780 lines, from upstream)
      files/lastnames.txt         (4760 lines, from upstream)
      files/words                 (minimal fallback wordlist; bound to
                                   /usr/share/dict/words via bwrap)
      MODEL.txt                   (wsdbm-data-model.txt, 357 lines)
      VERSION                     (upstream identifier + sha256 of binary)
      LICENSE                     (upstream license text)
```

Total vendored size ≈ 410 KB; below the threshold where Git LFS is useful.
`.gitattributes` marks `vendor/watdiv/bin/Release/watdiv` as `binary` to
keep diffs clean.

### Binary path is configurable

Resolution precedence:

1. `--watdiv-bin <PATH>` CLI flag (highest)
2. `KERMIT_WATDIV_BIN` env var
3. Vendored default at `<crate-root>/vendor/watdiv/bin/Release/watdiv`

Regardless of which path resolves, the driver creates a fresh per-generation
temp directory (`/tmp/kermit-watdiv-XXXXXX/`) staged as:

```
/tmp/kermit-watdiv-XXXXXX/
  bin/Release/watdiv     -> <resolved binary path>      (symlink)
  files/firstnames.txt   ← copied from vendor/
  files/lastnames.txt    ← copied from vendor/
  files/words            ← copied from vendor/
```

The driver `chdir`s to `bin/Release/` and execs `./watdiv ...`. The relative
`../../files/firstnames.txt` lookup the binary performs resolves to the
staged copies. Temp dir is owned by an RAII guard that removes it on Drop.

### `/usr/share/dict/words`

The binary has `/usr/share/dict/words` hardcoded as an absolute path. Three
options were considered:

- **A.** Always run under bwrap, bind-mounting the vendored wordlist to the
  hardcoded path. Picked.
- **B.** Detect host wordlist; bwrap-fallback. Rejected — doubles the test
  matrix and introduces host-variance into generation that's already
  non-deterministic enough.
- **C.** Refuse to run without host wordlist. Rejected — poor UX, NixOS
  unfriendly.

Picked **A**. `--no-bwrap` is exposed as an escape hatch for environments
where bwrap is unavailable (some CI containers, sandboxes within sandboxes).

### libstdc++ portability

The vendored binary is Linux x86_64 only. Behavior on supported platforms:

- **NixOS with `nix-ld` configured:** runs (verified).
- **Other Linux x86_64 with libstdc++:** expected to run (standard ldd
  resolution).
- **NixOS without `nix-ld`:** fails to load. Documented; the kermit Nix flake
  enables `nix-ld`.
- **macOS / aarch64 / Windows:** does not run. `kermit bench watdiv-gen`
  fails with a clear error referring users to the build-from-source path.
  Pre-generated artifacts remain consumable via `kermit bench run` on these
  platforms.

A `nix build .#watdiv-from-source` flake target is provided for users who
need to rebuild watdiv (out of scope for the initial implementation; flagged
in Future Work).

## CLI surface

```
kermit bench watdiv-gen
  --scale <N>                       required; integer ≥ 1; passed to watdiv -d
  --tag <STRING>                    required; appended to cache dir name;
                                    must contain at least one non-numeric
                                    character to avoid collision with
                                    committed benchmark names
  --max-query-size <N>              default: 5
  --query-count <N>                 default: 20
  --constants-per-query <N>         default: 2
  --allow-join-vertex <bool>        default: false
  --watdiv-bin <PATH>               override binary; default vendored
  --output-dir <PATH>               override cache dir parent; final artifacts
                                    land in <output-dir>/watdiv-stress-<N>-<T>/;
                                    default <output-dir> = ~/.cache/kermit/benchmarks/
  --no-bwrap                        skip sandbox; require host /usr/share/dict/words
```

Generation never produces timing data, so `kermit bench`'s shared
`--report-json` flag (which writes a `BenchReport` array describing one
benchmark *invocation*) is intentionally not exposed on `watdiv-gen`.
`meta.json` is always written into the cache directory and is the
machine-readable provenance record.

Defaults for `--max-query-size`, `--query-count`, `--constants-per-query`,
and `--allow-join-vertex` match the parameters used to produce the existing
committed `watdiv-stress-*.yml` snapshots, so a user running
`kermit bench watdiv-gen --scale 100 --tag default` produces a new SF=100
benchmark with the same workload shape (modulo non-determinism) as the
committed `watdiv-stress-100-*` set.

### Discovery integration

`kermit-bench`'s `discovery` is extended to walk **two** roots:

1. `workspace_root/benchmarks/*.yml` (existing, committed benchmarks)
2. `<cache_dir>/*/benchmark.yml` (existing fetched + new generated
   benchmarks); cache_dir defaults to `~/.cache/kermit/benchmarks/`

This is a small, additive change to `kermit-bench`: the existing
single-root signature stays available; a new `load_all_benchmarks_with_cache`
(or equivalent) accepts the cache root as a second argument. The
`kermit bench list` command in the binary calls the two-root variant.

`kermit bench list` shows both sets under their respective names. The
`--tag` validation rule (must contain a non-numeric character) prevents
namespace collisions with committed snapshots whose names follow the
pattern `watdiv-stress-{100,1000}-{warmup,test-N}`.

## Migration plan

### Phase 1 — verify parity, ship in parallel

1. Implement `kermit-rdf` and `kermit bench watdiv-gen` per this design.
2. Keep `scripts/watdiv-preprocess/` and the committed `watdiv-stress-*.yml`
   files in place.
3. **Acceptance test (translator parity):** port the SPARQL test cases from
   `scripts/watdiv-preprocess/tests/test_translator.py` into a Rust
   golden-file test suite (`kermit-rdf/tests/translator_golden.rs`). For
   each (input SPARQL, expected Datalog) pair, confirm the Rust translator
   produces output equivalent to the Python translator's output.
4. CI runs both pipelines for at least one week of green builds before
   proceeding to Phase 2.

### Phase 2 — retire Python

Once Phase 1 parity holds:

1. Delete `scripts/watdiv-preprocess/` entirely.
2. Update `CLAUDE.md`: remove the `scripts/watdiv-preprocess/` references in
   the WatDiv gotcha; add a section describing `kermit bench watdiv-gen`.
3. The committed `kermit/tests/fixtures/watdiv-mini/` and the
   `kermit/tests/watdiv_correctness.rs` test stay as-is. The fixture is
   deterministic, committed Parquet — its provenance (originally produced
   by the retired Python pipeline) does not affect its value as an LFTJ
   regression test. Replacing it with on-the-fly generation would trade a
   deterministic correctness check for a non-deterministic structural one,
   which is a regression.
4. The 12 committed `watdiv-stress-{100,1000}-*.yml` files **stay
   committed** as the canonical reproducible thesis-citation snapshots.
   Their ZivaHub-hosted Parquet artifacts continue to work via
   `kermit bench fetch`.

### What this migration does NOT do

- It does not attempt to byte-reproduce the committed YAMLs from the new
  tool. Watdiv non-determinism makes that impossible.
- It does not refresh the committed snapshots. If a future thesis version
  wants new canonical snapshots, that is a separate decision: run
  `kermit bench watdiv-gen --scale 100 --tag <new-tag>`, evaluate the
  outputs, commit them as the new canonical set, and decide whether to
  retire the old.

## Error handling

`kermit-rdf` defines a `RdfError` enum following the `thiserror` pattern
used by `kermit-bench`. Variants cover: binary not found, binary failed
(non-zero exit, segfault), sandbox setup failure, malformed N-Triples,
malformed SPARQL, unsupported SPARQL feature (constructs the Datalog target
cannot express), expected-results computation failure, and the
transparent-wrap variants for `std::io::Error`, `arrow::ArrowError`, and
`parquet::ParquetError`.

The CLI binary continues to use `anyhow::Result` and surfaces `RdfError`
via `anyhow::Context`.

### Failure cleanup

- The driver's per-generation temp dir is owned by an RAII guard
  (`TempStagingDir`) that removes itself on Drop, including on panic.
- Partial cache entries (those reached partway through a failed generation)
  are **not** auto-deleted, for debugging value. The
  `meta.json`-written-last protocol distinguishes complete entries from
  partial ones; `kermit bench run` refuses to load a cache entry without
  `meta.json` and reports the missing-marker condition clearly.

## Testing strategy

| Layer | Location | Scope | Speed |
|---|---|---|---|
| 1. Unit | inline `#[cfg(test)]` per module | dict roundtrip, NT line parser, partitioning | <1s |
| 2. Translator golden | `kermit-rdf/tests/translator_golden.rs` | port of Python `test_translator.py` cases | <1s |
| 3. Pipeline (no binary) | `kermit-rdf/tests/pipeline.rs` | hand-crafted `.nt` + `.sparql` through stages 4–6 | ~1s |
| 4. End-to-end | `kermit-rdf/tests/e2e_watdiv.rs` | actual binary invocation at SF=1 with fixed tag | ~5s |
| 5. CLI smoke | `kermit/tests/watdiv_correctness.rs` | full `kermit bench watdiv-gen` + `kermit bench run` | ~10s |

### Pinning e2e tests against non-determinism

Layers 4–5 invoke a non-deterministic binary; byte-equality assertions are
impossible. They assert three structural properties instead:

1. Pipeline completes without panic or returned error.
2. Output has expected shape: `dict.parquet` non-empty; `relations/`
   non-empty; `benchmark.yml` parses; `expected/*.csv` files exist for the
   queries reported in `meta.json`, parse as `cardinality\n<N>\n`, and the
   count of CSVs equals `meta.query_count`. (Comparing those counts to a
   live join engine is out of scope here — see "Stage 6 scope note".)
3. Round-trip consistency: parsing the generated `.nt` + `dict` +
   `relations/` back into memory produces the same content as a fresh
   `parse(generated.nt) → dict → partition` round.

### CI integration

- Layers 1–3 run on every CI job (all platforms).
- Layers 4–5 run only on Linux x86_64 with bwrap available; other platforms
  emit a clear "skipping watdiv e2e: bwrap not found" message and pass the
  job.
- Miri runs on layers 1–3 only. Layers 4–5 are excluded (`Command` spawning
  is outside miri's model). This is a strict improvement over today, where
  the Python preprocessor's logic gets no miri coverage at all.

## Out of scope / future work

- **Build-from-source flake target** (`nix build .#watdiv-from-source`) —
  documented but not delivered in the initial implementation. Useful for
  non-x86_64 / non-Linux platforms; deferred until someone needs it.
- **Patch watdiv to accept `--seed`** — would require maintaining a fork of
  the upstream C++ source. Significant maintenance burden for a feature
  (deterministic generation) that the chosen design philosophy explicitly
  doesn't require. Out of scope.
- **Other RDF benchmarks (LUBM, BSBM)** — the watdiv-specific pieces
  (sandbox setup, invocation, vendored binary) live under `driver/`. Adding
  a sibling benchmark would mean introducing a `driver/<name>/` subdirectory
  and a parallel `bench <name>-gen` subcommand. Not part of this work.
- **Refreshing the canonical committed snapshots** — a separate decision
  for a future thesis revision. The on-the-fly tool makes refresh tractable
  but does not itself trigger one.
