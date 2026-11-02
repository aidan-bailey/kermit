# `DuckDB`

> **Status:** experimental · **CLI:** `--engine duckdb` · **Feature:** `engine-duckdb` (dev shell), `engine-duckdb-bundled` (CI) · **Implementation:** [`kermit::engine::duckdb`](../../kermit/src/engine/duckdb.rs)

DuckDB, run in process through the `duckdb` crate, is the production binary-join engine kermit's
structures are compared against. It is already kermit's JOB oracle (a JOB query's `expected` is its
`COUNT(*)`), and the engine that Free Join and the Linear-Chained work measure against. It runs
exactly the query kermit's cells run: it reads `Prepared` after `prepare` has validated, rewritten,
extracted accesses and planned, and never parses Datalog. The thesis question it serves is RQ1.4: a
bounded sanity baseline against an established peer, not a head-to-head claim.

## Phase mapping

| Phase | What DuckDB does | Timed | Notes |
|---|---|---|---|
| `load` | An in-memory database, `PRAGMA threads = N`, and one table `rel_<name>` (columns `c0…`, `UBIGINT`) per relation the query reads, appended from the catalog's flat tuples | no | |
| `build` | One `CREATE TEMP TABLE` per `IndexSpec`: the spec's filtered, projected rows from the relation table (one copy; no index exists). Then the join statement is prepared | yes | The preparation (planning, well under a millisecond on these queries) is the last step of `build`, so `iteration` excludes it and each `end_to_end` sample includes it once (decision 11) |
| `iteration` | The prepared join over the spec tables, streamed as Arrow batches whose rows are counted | yes | DuckDB builds its hash tables here, inside the query |
| `materialize` | The same statement, each batch's columns copied into kermit `Tuples` (row-major), then the sort and distinct count that kermit's own cells use (`kermit::db::materialize_count`) | yes, opt-in | Every sample builds nothing new: `MATERIALIZE_MUTATES` is false |
| `space` | Not reported | | DuckDB keeps no index; its hash tables exist only inside a query. The note `{label} reports no space` is printed |

`Engine` constants: `NAME` is `"duckdb"`, `STREAMS` true, `KEEPS_DUPLICATES` true,
`MATERIALIZE_MUTATES` false, `HAS_SPACE` false.

## Translation

The shared SQL translator (`kermit/src/engine/sql.rs`) reads the executed query and its specs, whose
shape `extract_accesses` and `plan_indexes` guarantee: every atom names an index id, has one variable
per column in index-column order, repeats no variable and holds no constant. Each spec `#k` becomes
a table `idx_k` followed by a per-build suffix; each atom becomes an alias `a{i}` over its spec's
table, with column `c{j}` bound to the atom's `j`-th variable; every variable is selected once, at
its first binding, in canonical order; every later binding is an equality with the first; an atom
that shares no variable with the atoms before it joins by `CROSS JOIN`; identifiers are
double-quoted. For the triangle under the plan `X, Y, Z`, where all three atoms read the merged
spec `#0`:

```sql
SELECT a0."c0" AS "X", a0."c1" AS "Y", a1."c1" AS "Z"
FROM "idx_0" AS a0
JOIN "idx_0" AS a1 ON a1."c0" = a0."c1"
JOIN "idx_0" AS a2 ON a2."c0" = a0."c0" AND a2."c1" = a1."c1";
```

A spec's build statement selects the spec's columns from the base relation under its `Filter`
(constants as `c{c} = v`, equalities as `c{a} = c{b}`). For a ternary `r` read as `r(Y, X, c42)`
under the plan `X, Y`:
`CREATE TEMP TABLE "idx_0_<n>" AS SELECT "c1", "c0" FROM "rel_r" WHERE "c2" = 42`.

## Fairness caveats

- **Compare DuckDB with kermit on `end_to_end` only.** DuckDB's index work (its hash tables) sits
  in its `iteration`, while kermit's sits in `build`, so only the sum is the same work. `kl.engines()`
  draws exactly that pair.
- **DuckDB chooses its own join order.** The statement fixes the query, not the order: kermit's
  variable order is the plan of its own cells, and DuckDB's optimiser plans the joins itself.
- **A zero all-constant count skips the engine's build.** On a query with an all-constant atom whose
  count is 0 the result is known to be empty, so the engine cell skips its `build` entirely, while
  kermit's `build` still builds every index. The engine's `build` there is near zero and is not
  like-for-like.
- **The `plan` axis is kermit's order, DuckDB ignores it.** Engine rows carry the `plan` axis, which is
  kermit's variable order; DuckDB plans its own join order and does not follow it, so `plan` only
  says which query ran.
- **Bag semantics.** DuckDB counts duplicate rows, as `HashTrie` does (`KEEPS_DUPLICATES`), so on
  duplicate-free inputs its count equals kermit's and the benchmark's `expected`.
- **One thread by default** (`PRAGMA threads = 1`), the same parallelism as kermit's joins and serial
  builds. `--engine-threads N` raises it; reports carry `engine_threads`.
- **Memory.** Every connection sets `memory_limit = '8GB'`, under the sweep's 14 GiB cap (DuckDB's
  default is 80% of RAM), and a private spill directory.
- **Probes are single cold runs.** A probe's `build`, `iteration` and `materialize` are one run each:
  compare them with other single runs, never with a Criterion estimate.

## Version pin

- The crate is `duckdb = "~1.4"` in `kermit/Cargo.toml`: DuckDB 1.4.x, matching the flake's DuckDB
  1.4.4, which computes JOB's `expected`. The newest crate line (1.20000.x) is DuckDB 2.0.
- `engine-duckdb` links the system `libduckdb`, found through `DUCKDB_LIB_DIR` and
  `DUCKDB_INCLUDE_DIR`, which the dev shell exports from Nix's `duckdb.lib` and `duckdb.dev`
  outputs (`flake.nix`). A dev-shell build compiles no C++, and `kermit/build.rs` adds an rpath to
  `DUCKDB_LIB_DIR` so a binary built in the shell also runs outside it. Without `DUCKDB_LIB_DIR` and
  without bundling, the build fails with `engine-duckdb needs DUCKDB_LIB_DIR (the nix dev shell sets
  it) or the engine-duckdb-bundled feature`.
- `engine-duckdb-bundled` adds `duckdb/bundled`, which compiles DuckDB's C++ amalgamation. It is for
  CI (no Nix). **Never build it on a host that cannot spare several GB for the compiler process.**
- Every report's `engine_version` records the library that actually ran (`pragma_version()`), so a
  report says which DuckDB it measured whatever the pin.

To move the pin:
1. Change the `duckdb` requirement in `kermit/Cargo.toml` (and `cargo update -p duckdb`, which
   rewrites `Cargo.lock`), and `[profile.dev.package.libduckdb-sys]` in the root `Cargo.toml` if the
   `-sys` crate is renamed.
2. Make the dev shell's `duckdb` the same minor line: `nix flake update nixpkgs` moves it, and
   `duckdb --version` in `nix develop` shows it. The JOB generator drives the same library
   (`kermit-generate/src/job/duckdb.rs`), so a move that changes any JOB `expected` count is a
   regeneration question for the JOB caches, not only for this page.
3. Run `cargo test -p kermit --features engine-duckdb --test engine_oracle` and the acceptance
   probes below, and record the new version here.

## Settings and limits

- `DuckDbSettings { threads }`: `--engine-threads N`, default 1, range `1..=available_parallelism()`;
  outside it the error reads `--engine-threads must be between 1 and {available_parallelism}`.
- `memory_limit = '8GB'` and the private spill directory `kermit-duckdb-<pid>-<n>` under the system
  temporary directory, removed when the connection drops. DuckDB deletes every spill file in its
  directory on exit, so two connections sharing one would corrupt each other's spills; the JOB
  generator guards against the same hazard.
- `--engine-threads` applies to `--engine duckdb` only: given with no DuckDB engine selected it is a
  usage error (Nemo is single-threaded).
- An engine cell's preflight (untimed, before the describe step) names the benchmark, the query and
  the engine in its error. `bench join --engine` runs the 64-byte Criterion group-name clash check
  over its kermit and engine cells, as `bench run` does.
- kermit sets no timeout of its own; the sweep harness's per-step timeout and memory cap apply. An
  engine that fails ends its cell with an error, and nothing is retried.

## Tests

- `kermit/tests/engine_oracle.rs` checks DuckDB's counts and contents against the nested-loop oracle
  over the shared query shapes (`kermit/tests/common/shapes.rs`), on bag semantics.
- Unit tests in `kermit/src/engine/sql.rs` pin the translator's statements.
- The `engines` job of `.github/workflows/pr.yml` runs both (bundled, since CI has no Nix). The
  profile's gates build no cargo features, so a story that touches the engine names
  `--features engine-duckdb` in its own Verify line.

## Acceptance probes (2026-11-02)

`bench probe <benchmark> [-q <query>] -i all -a all --engine duckdb,nemo --materialize` on one query of every benchmark family: three kermit cells and both engines; a probe verifies each cell's count against `expected` where the benchmark has one (watdiv and oxford have none), and the Cells agree column compares the cells' counts. Times are single cold runs in milliseconds: compare them with other single runs, never with Criterion estimates. `-` means the engine does not offer the phase.

| Benchmark | Query | Cells | `answer_rows` | Cells agree | `build` ms | `iteration` ms | `materialize` ms | `space` bytes | `engine_version` |
|---|---|---|---|---|---|---|---|---|---|
| triangle | triangle | 5 | 4 | yes | 0.360 | 0.381 | 0.272 | - | v1.4.4 |
| lubm-reference | q1 | 5 | 4 | yes | 0.632 | 0.185 | 0.139 | - | v1.4.4 |
| watdiv-stress-100-test-1 | q0001 | 5 | 0 | yes | 6.610 | 0.379 | 0.311 | - | v1.4.4 |
| oxford-uniform-s3 | triangle | 5 | 7430, 7433 | NO | 0.925 | 1.790 | 2.402 | - | v1.4.4 |
| sw-snap-amazon-pre | triangle | 5 | 4002774 | yes | 26.908 | 875.905 | 1107.739 | - | v1.4.4 |
| sw-h2o-pre | q1 | 5 | 8999658 | yes | 152.436 | 59.051 | 464.643 | - | v1.4.4 |
| sw-job-1-pre | 1a | 5 | 142 | yes | 52.320 | 3.889 | 2.934 | - | v1.4.4 |

The oxford row disagrees by set against bag semantics, not by a fault: `R` and `S` of `oxford-uniform-s3` each hold one duplicate row (2744 rows, 2743 distinct, counted with DuckDB over the cache), so the bag cells (`HashTrie` and DuckDB) count 7433 and the set cells (`TreeTrie`, `ColumnTrie`, Nemo) 7430. See Fairness caveats, bag semantics.

## See also

- [`nemo.md`](nemo.md) and [`TEMPLATE.md`](TEMPLATE.md).
- The design: [`docs/specs/2026-11-02-external-engines-design.md`](../specs/2026-11-02-external-engines-design.md).
- The report schema's v8 axes: [`docs/specs/bench-report-schema.md`](../specs/bench-report-schema.md).
