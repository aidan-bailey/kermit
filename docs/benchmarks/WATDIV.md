# WatDiv (Waterloo SPARQL Diversity Test Suite)

The Waterloo SPARQL Diversity Test Suite is a synthetic RDF benchmark
introduced by Aluç et al. (2014). It targets stress-testing of SPARQL engines
across a wide variety of structurally diverse queries, in contrast to LUBM's
14 hand-designed queries. Kermit uses WatDiv for distributional load: each
stress workload contains ~12 400 mechanically-generated queries spanning
star, snowflake, and chain shapes anchored on a randomly-chosen entity
constant.

WatDiv complements LUBM. LUBM probes the engine on named, individually
meaningful queries (including two clean triangles for LFTJ); WatDiv probes
the engine on a query distribution to amortise noise and stress unfamiliar
patterns. Both flow through the same `kermit-rdf` parquet/dict artefacts
and run via the same `kermit bench run`.

For a side-by-side comparison with other benchmarks in the suite, see
[`README.md`](README.md#cross-benchmark-comparison).

## Default workloads

Two generator-driven benchmarks ship as runnable suites (regenerated on
`bench run`, non-deterministic — see the non-determinism notes below):

- **`watdiv-basic`** — the canonical Basic Testing query set: Linear
  (`L1`–`L5`), Star (`S1`–`S7`), Snowflake (`F1`–`F5`), Complex (`C1`–`C3`),
  20 templates. `generator.kind: watdiv-basic`. Runs `watdiv -d` then `-q` on
  the vendored templates (skips `-s`), one concrete query per template.
- **`watdiv-stress-default`** — the diversified stress workload at default
  parameters (`generator.kind: watdiv`, default `WatdivStressSpec`),
  complementing the frozen `watdiv-stress-{100,1000}` snapshots.

Run: `cargo run -- bench run watdiv-basic -i tree-trie -a leapfrog-triejoin`.

### Vendored Basic Testing templates

`kermit-rdf/vendor/watdiv/testsuite/{L,S,F,C}*.txt` (20 files) are the upstream
WatDiv Basic Testing query templates (same provenance as the vendored binary;
see VERSION). Each is a standard `-q` template: a `#mapping` line per
placeholder plus a BGP `SELECT`. `S1`'s `%v2%` is a subject-position
placeholder, so the basic workload exercises the subject-position-constant
join path.

### Absent predicates in the basic workload

The fixed basic templates reference predicates by the WatDiv data model, but
WatDiv generates data probabilistically, so a referenced predicate may have
zero instances at a given scale. The basic pipeline therefore seeds an **empty
relation** for any query-referenced predicate absent from the generated data
(absent predicate = empty relation = empty result), rather than failing
translation. This keeps the workload robust and deterministic regardless of
scale. At low scale, expect some basic queries to return 0 results because a
relation is empty — raise the scale for more meaningful cardinalities.

## How to run

### Committed snapshots (default)

The 12 `benchmarks/watdiv-stress-{100,1000}-{warmup,test-1..5}.yml` snapshots
reference Parquet artefacts hosted on ZivaHub. (The `watdiv-stress-*.yml` glob
also matches `watdiv-stress-default.yml`, but that file is a declarative
generator spec — see [Default workloads](#default-workloads) — not a ZivaHub
snapshot.) Run with:

```bash
kermit bench fetch watdiv-stress-100-test-1   # one-time download to cache
kermit bench run watdiv-stress-100-test-1 \
    -i tree-trie -a leapfrog-triejoin
```

| File | Scale factor | Queries | Purpose |
|------|--------------|---------|---------|
| `watdiv-stress-100-warmup.yml` | 100 | ~12 400 | Warm-up batch, stress params 5/20/2/false |
| `watdiv-stress-100-test-{1..5}.yml` | 100 | ~12 400 each | Five test batches at SF=100 |
| `watdiv-stress-1000-warmup.yml` | 1000 | ~12 400 | Warm-up at SF=1000 |
| `watdiv-stress-1000-test-{1..5}.yml` | 1000 | ~12 400 each | Five test batches at SF=1000 |

These are the canonical reproducible reference for thesis citations. They
were produced by a single historical run of `scripts/watdiv-preprocess/`
(Python) and uploaded to ZivaHub. **Do not edit them by hand** — the YAMLs,
`dict.parquet`, and per-predicate `*.parquet` files are dictionary-coupled
and must be regenerated together if regeneration is needed. See
`scripts/watdiv-preprocess/README.md` for the regeneration workflow.

### On-the-fly generation

`kermit bench gen watdiv --scale N --tag STR` drives the vendored WatDiv
binary at arbitrary scale factors and stress parameters, runs the
preprocessing pipeline in pure Rust, and writes a complete artefact set to
the local cache. Subsequent `kermit bench run watdiv-stress-{N}-{tag}` reads
those artefacts without re-downloading.

```bash
kermit bench gen watdiv --scale N --tag STR \
    [--max-query-size N=5] \
    [--query-count N=20] \
    [--constants-per-query N=2] \
    [--allow-join-vertex] \
    [--watdiv-bin PATH | env KERMIT_WATDIV_BIN] \
    [--output-dir PATH] \
    [--no-bwrap]
```

| Flag | Default | Notes |
|------|---------|-------|
| `--scale` | required | WatDiv `-d` scale factor; ≥ 1 |
| `--tag` | required | Suffix on the bench name; **must contain a non-numeric character** to avoid colliding with committed snapshot names like `test-1` |
| `--max-query-size` | 5 | Stress template size parameter |
| `--query-count` | 20 | Concrete queries per template |
| `--constants-per-query` | 2 | Constants per generated query |
| `--allow-join-vertex` | false | Permit join-vertex shapes in stress templates |
| `--watdiv-bin` | vendored | Override path to the watdiv binary; also reads `KERMIT_WATDIV_BIN` |
| `--output-dir` | `~/.cache/kermit/benchmarks` | Custom output dirs are NOT auto-discovered by `bench list/fetch/run` |
| `--no-bwrap` | false | Skip the bubblewrap sandbox; requires host `/usr/share/dict/words` |

Of the four stress flags, only `--max-query-size` and `--query-count` are
currently forwarded to the vendored binary and shape the workload;
`--constants-per-query` and `--allow-join-vertex` are recorded in `meta.json`
for provenance but have no effect on the generated data or queries (the
vendored binary has no matching flag yet). Their semantics are described in
[Workload reference](#workload-reference).

End-to-end runtime is dominated by query generation; SF=100 with default
stress params completes in roughly 30 s on a developer laptop.

### Declarative YAML spec (commit-and-run)

A `benchmarks/<name>.yml` may declare a generator block instead of
relations + queries. On `kermit bench run <name>`, the data is materialised
on first invocation and cached; subsequent runs short-circuit on the cached
`meta.json` if the spec hash matches.

```yaml
# benchmarks/watdiv-100-stress1.yml
name: watdiv-100-stress1
description: "WatDiv stress, scale 100"
generator:
  kind: watdiv
  scale: 100
  stress:
    max_query_size: 5
    query_count: 20
    constants_per_query: 2
    allow_join_vertex: false
```

Run with `kermit bench run watdiv-100-stress1 -i tree-trie -a leapfrog-triejoin`.
Editing the YAML's params and re-running errors with `SpecDrift` — pass
`--force` to opt into regenerating (a multi-minute pipeline; the gate exists
so a typo doesn't trigger silent rebuilds). See `benchmarks/README.md` for
the full schema.

The declarative path uses the same pipeline as on-the-fly generation; the
choice between the two is between imperative ad-hoc generation (your shell
history holds the params) and declarative reproducibility (the params live
in git).

## Pipeline

```
watdiv binary (vendored)        kermit-rdf::driver
    │                                  │
    │ -d <model> <scale>               │ stage temp dir, bind-mount
    │ -s <model> <data> <q-size> <q-n> │ /usr/share/dict/words via bwrap
    │ -q <model> <template> <count>    │ <recurrence> hard-coded to 1
    ▼                                  ▼
 stdout: triples / templates / queries (split on `#end` markers)
                                       │
                                       │ kermit-rdf::partition
                                       ▼
                            <pred>.parquet × N + dict.parquet
                                       │
                                       │ kermit-rdf::sparql::translator
                                       │ (BGP-only SPARQL → Datalog)
                                       ▼
                              benchmark.yml + meta.json
```

Unlike the LUBM pipeline, there is **no entailment step** — WatDiv data is
fully materialised by the upstream binary, so the partition stage consumes
the raw N-Triples directly. The SPARQL translator is the same one used by
LUBM (BGP-only, rejects FILTER/OPTIONAL/UNION).

## Output layout

```
~/.cache/kermit/benchmarks/watdiv-stress-{scale}-{tag}/
  meta.json                    PipelineMeta — kind = "watdiv-onthefly"
  benchmark.yml                kermit BenchmarkDefinition
  dict.parquet                 Shared URI/literal → usize dictionary
  <predicate>.parquet × ~86    One per predicate (consistent across runs)
  raw/data.nt                  Raw watdiv stdout from -d
  raw/templates/*.txt          Stress templates from -s
  raw/queries/*.sparql         Concrete queries from -q
  expected/                    Empty — the vendored binary emits no .desc sidecars
```

`meta.json` records the SHA-256 of the vendored watdiv binary, the names
files, and the model file. The 12 committed snapshots have a similar
provenance record from their original Python pipeline run.

## Workload reference

WatDiv's `-s` mode emits stress templates of the form
`#mapping <var> <type> <constraint> #end`. The `-q` mode then instantiates
each template with concrete random constants. The four stress parameters
control template diversity:

| Parameter | Effect |
|-----------|--------|
| `max-query-size` | Maximum number of triple patterns per query (forwarded to `-s`) |
| `query-count` | Concrete queries instantiated per template (forwarded to `-s`) |
| `constants-per-query` | Intended number of `c<id>` constants pinning each query — **recorded in `meta.json` for provenance only; NOT currently forwarded to the vendored binary** (no effect on generated data/queries) |
| `allow-join-vertex` | Intended toggle for whether stress templates can include join vertices — **recorded in `meta.json` for provenance only; NOT currently forwarded to the vendored binary** (no effect on generated data/queries) |

All four parameters are settable on the command line via the matching
`--max-query-size`, `--query-count`, `--constants-per-query`, and
`--allow-join-vertex` flags, or via the `stress:` block in the declarative
YAML spec — see [How to run](#how-to-run). Only the first two currently affect
the binary; the other two are persisted for provenance and will be wired up if
the vendored binary gains a matching flag.

Body atom count distribution from a representative SF=100 stress file (200
queries sampled): 4 atoms (75), 6 atoms (61), 5 atoms (31), 2 atoms (21),
3 atoms (12). Most queries are stars or snowflakes around a central
`c<dict-id>` anchor; explicit triangles are rare and incidental.

The translator emits queries of the form
`Q_test_1_q0042(V0, V2, V3) :- pred1(V0, c149844), pred2(V0, V2), pred3(V0, V3).`
The `c<dict-id>` atoms are resolved via kermit's const-rewrite path
(`kermit_algos::rewrite_atoms`) into singleton-trie unary predicates before
LFTJ runs.

## Determinism

**WatDiv is non-deterministic by construction.** The upstream binary calls
`srand`, `time`, `gettimeofday`, and `std::random_device::_M_init`; there is
no `--seed` flag. Two back-to-back invocations of `./watdiv -d <model> 1`
produce different output sizes (15.0 MB vs 15.7 MB), different triple counts
(109 961 vs 114 711), and different content from the very first line.

This forces a particular discipline:

- **Each `bench gen watdiv` invocation produces a fresh, locally-owned snapshot
  tagged by the user.** Re-running with the same `--tag` overwrites silently;
  re-running with a different `--tag` produces a fresh artefact set with
  unrelated data.
- **The 12 committed `watdiv-stress-*.yml` snapshots are the canonical
  reproducible reference** — they pin specific Parquet blobs on ZivaHub.
  `bench gen watdiv` outputs are *additional* artefacts, not replacements.
- **`meta.json` records the binary's SHA-256 and ISO timestamp** to
  distinguish snapshots post-hoc, even though the underlying random data
  cannot be reproduced.

For thesis claims that must reproduce later, prefer the committed snapshots.
For research flexibility — exploring SF=2, SF=7, SF=50, or different stress
parameters — use `bench gen watdiv` and accept fingerprint drift between runs.

## Vendored generator

`kermit-rdf/vendor/watdiv/bin/Release/watdiv` (~360 KB, committed/vendored —
ships with the repo, like the LUBM jar).

| Field | Value |
|-------|-------|
| Source | <https://github.com/dgasmith/watdiv> (upstream archived) |
| Version tag | `watdiv-upstream-2014` (from `kermit-rdf/vendor/watdiv/VERSION`) |
| Build | `make` in the upstream tree (see Source URL above) |

The binary is committed alongside its surrounding files (`MODEL.txt`,
`files/firstnames.txt`, `files/lastnames.txt`, `files/words`, `LICENSE`,
`VERSION`), so the on-the-fly pipeline runs against a fresh clone with no local
build step. Tests still gate on `bin.exists()` and skip when the binary is
absent (e.g. on unsupported platforms).

## Sandboxing

The vendored watdiv binary expects `/usr/share/dict/words` at a hard-coded
path and `firstnames.txt`/`lastnames.txt`/`words` relative to its working
directory. The driver wraps each invocation in `bwrap` (Bubblewrap):

- A tmpfs at `/usr` with selective re-binds of `/usr/bin`, `/usr/lib`,
  `/usr/lib64`, `/usr/local` (via `--ro-bind-try`)
- A staged `words` file bind-mounted at `/usr/share/dict/words`
- A `--chdir` into the staging `bin/Release/` so relative paths resolve

This recipe works on minimal hosts (NixOS) where `/usr/share/dict` may not
exist. The simpler `--bind <words> /usr/share/dict/words` recipe fails
there because bwrap can't `mkdir` under a host-RO `/usr/`.

`--no-bwrap` skips the sandbox entirely and requires the host to provide
`/usr/share/dict/words` directly. Useful in tightly-sandboxed CI runners
where bwrap itself isn't available; failures manifest as the binary aborting
on missing files.

The kermit Nix flake provides `pkgs.bubblewrap` so `nix develop` is the
recommended dev environment.

## Tests

| Test | What it validates | Gate |
|------|------------------|------|
| `kermit-rdf/tests/e2e_watdiv.rs` | Full pipeline against vendored binary at SF=1 | binary exists + bwrap works |
| `kermit/tests/cli_watdiv_gen.rs` | CLI smoke for `bench gen watdiv` | binary exists + bwrap works |
| `kermit/tests/watdiv_correctness.rs` | Loads the committed mini fixture and validates round-trip | always |

The first two auto-skip on non-Linux/non-x86_64 hosts and on hosts where
bwrap can't construct the `/usr/share/dict/words` bind.

## References

- G. Aluç, O. Hartig, M. T. Özsu, K. Daudjee. *Diversified Stress Testing of
  RDF Data Management Systems.* ISWC 2014.
- T. L. Veldhuizen. *Leapfrog Triejoin: a worst-case optimal join algorithm.*
  ICDT 2014.
- Upstream WatDiv: <https://dsg.uwaterloo.ca/watdiv/>
- Existing design docs: `docs/plans/2026-04-17-watdiv-benchmark-design.md`
  (offline pipeline) and `docs/plans/2026-05-04-watdiv-onthefly-design.md`
  (on-the-fly pipeline)
- Sibling benchmark reference: [`LUBM.md`](LUBM.md)

## Future work

- **Result oracle** — the vendored binary doesn't emit `.desc` sidecars, so
  `expected/*.csv` is empty. A reference engine (e.g. blazegraph) could
  produce ground-truth cardinalities for cross-engine validation.
- **Vendor-binary SHA-256 verification** in `kermit-rdf::driver::drive` —
  refuse to invoke a binary whose hash doesn't match an embedded constant
  unless `--watdiv-bin` is explicit.
- **Deterministic mode upstream** — patching the watdiv binary to accept a
  `--seed` flag would unify reproducibility with the LUBM pipeline.
- **Migration off the Python preprocessor** — `scripts/watdiv-preprocess/`
  is retained for historical regeneration of the 12 committed snapshots
  but is functionally superseded by the Rust on-the-fly path.
