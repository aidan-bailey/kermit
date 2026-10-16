# JOB Benchmark Generator (#97) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The Join Order Benchmark (Leis et al., VLDB 2015) becomes a generator benchmark: `generator: { kind: job, template: <1–33> }` produces `job-<N>`, every variant of one JOB template over IMDb, with each alias's filters applied during generation by DuckDB, so kermit runs JOB's real joins and checks each count against DuckDB's `COUNT(*)`.

**Architecture:**
- **Translator (J1, J2), pure Rust.** `sqlparser` parses each query; the WHERE clause's `AND` tree is flattened (so `BETWEEN … AND …` stays one node) and each conjunct classified as a join, a filter or an error. `plan_template` merges joins into variables ordered by first appearance, gives each alias a relation (join columns in variable order, then `id` unless joined), shares identical relations within the template and writes one rule per query.
- **DuckDB (J3), an external tool.** `Duckdb` locates the CLI (`KERMIT_DUCKDB_BIN` or PATH; the flake adds `pkgs.duckdb`) and runs every session under a 4 GB memory limit with spilling. `stream_rows` turns `COPY (…) TO '/dev/stdout'` into `Tuples` batches.
- **Raw cache (J4, J7).** `<cache>/kermit/imdb/` holds `imdb.duckdb`, the pinned query files and `provenance.json`, built once in a staging directory and renamed into place; J7 adds the downloads through #96's `download_verified`.
- **Generator (J5).** `JobGenerator` implements #95's `kermit_generate::Generator`: per relation it streams DuckDB's filtered rows into a `RelationWriter`, per query it runs the `COUNT(*)` oracle.
- **Wiring (J6).** `GeneratorSpec::Job`, the `materialize::dispatch` arm, `bench gen job` with #95's `GenTarget`, and the 33 committed YAMLs.
- **Docs (J8) and acceptance (J9).**

**Tech Stack:** Rust nightly (workspace `rust-toolchain.toml`) in the nix dev shell; `sqlparser` 0.63.0 (feature `visitor`); the DuckDB 1.4.4 CLI from the flake's nixpkgs; #95's `kermit-generate` crate and `kermit_ds::ParquetWriter`; #96's `kermit_bench::cache::download_verified`.

**Spec:** `docs/specs/2026-10-15-job-benchmark-design.md` (approved at da09595). **Names contract:** `docs/superpowers/plans/2026-10-15-job-benchmark-contract.md` (5dd1691; the Merge notes record where this plan departs from it). Read both first. Every decision they record is settled; where the drafted parts of this plan disagreed with each other or with the contract, the choices are in the Merge notes at the end.

---

## Ground rules for the executor

- **Paths.** `WT=/tb/Source/Academia/kermit/.loom/worktrees/aidanb/job_18dea468f74c13c3`. Every command runs from `$WT`. `SCRATCH` is the executing session's scratchpad.
- **Branch:** the loom branch `aidanb/job`. Never switch branches, never amend, never push. JOB lands on master once, after J7 and J9's count check, through the user.
- **Upstream gates.** Each phase starts only when its upstream code is on `origin/master` and merged into `aidanb/job` with `git merge origin/master` (never rebase):

  | Phase | Tasks | Needs on master (and in this branch) |
  | --- | --- | --- |
  | 1 | J1–J5, J4 Part A | #95 push 1 (`kermit-generate`, `ParquetWriter`) |
  | 1b | J4 Part B (the IMDb measurements) | `FULL_EXIT` and the user's go-ahead for the 1.26 GB download |
  | 2 | J6 | #95's `GenTarget` (#95 push 2 or #96 phase 2) |
  | 3 | J7 | #96 phase 1 (`download_verified`) and J4 Part B |
  | 4 | J8–J9 | J1–J7 |

  The benchmark co-ordinator session relays each upstream SHA. **Premise re-check:** after each merge, compare every quote of upstream code in the phase's tasks (names, signatures, file paths) with the tree, and fix the plan text before implementing. The kermit-generate names come from #95's contract (`docs/superpowers/plans/2026-10-15-generate-layer-contract.md` on `aidanb/h2o`, ffc516c, updated 0de175a with no renames).
- **Host:** no cargo command and no DuckDB run on real data while a timing run is live. Before any compile, test or DuckDB load:
  ```bash
  find /proc/[0-9]*/exe -maxdepth 0 -lname '*kermit-bench-runs*' 2>/dev/null | wc -l
  ```
  must print `0`, and the co-ordinator must have announced `FULL_EXIT` for the #101 chain.
- **Builds:** every cargo command runs inside the dev shell with two jobs, in the foreground: `nix develop --command env CARGO_BUILD_JOBS=2 cargo …`; clippy `nix develop --command env CARGO_BUILD_JOBS=2 RUSTFLAGS=-Dwarnings cargo clippy --all-targets`; doc `nix develop --command env CARGO_BUILD_JOBS=2 RUSTDOCFLAGS=-Dwarnings cargo doc --workspace --no-deps`. Long runs go through `setsid nohup … & disown` with a `kill -0` watcher.
- **Reading the code blocks.** A fence nested under a step carries the step's two-space list indentation, which an Edit's `old_string` and `new_string` do not: strip it before applying. A fence at column 0 is file text as is (a whole new file, or text appended to a file).
- **Formatting:** `nix develop --command cargo fmt --all` only (stable rustfmt rewrites ~30 files).
- **CI's nightly for fmt and clippy.** The flake's `rust-overlay` lock lags CI's floating nightly, so the *check* runs of every task's lint step use a non-writing override: `nix develop --override-input rust-overlay github:oxalica/rust-overlay --no-write-lock-file --command cargo fmt --all -- --check` and `nix develop --override-input rust-overlay github:oxalica/rust-overlay --no-write-lock-file --command env CARGO_BUILD_JOBS=2 RUSTFLAGS=-Dwarnings cargo clippy --all-targets`. Never commit a `flake.lock` change from this branch.
- **Temp files on disk.** Every `cargo test` and probe runs with `TMPDIR=/tb/Source/Academia/kermit-bench-runs/job-97/tmp` (create it first), so `tempfile` dirs and probe scratch land on disk, not in the RAM-backed `/tmp`.
- **Doc comments:** `kermit-generate` is `#![deny(missing_docs)]`; document every `pub` item, backtick every identifier (clippy `doc_markdown`), and link no binary item (`materialize`, `ExecutionFamily`).
- **No `unsafe`.**
- **Never write under `~/.cache/kermit`** (no `bench gen`, `bench run`, `--force` or `bench clean` there) without asking the user.
- **`/tmp` is RAM-backed** (tmpfs, ~8 GB free of 30 GB RAM), and `$SCRATCH` lives there. Small test temp dirs (`tempfile`) are fine; anything large is not: the IMDb dump, its CSVs, `imdb.duckdb`, DuckDB spill dirs and any generated template go under `RUNS=/tb/Source/Academia/kermit-bench-runs/job-97` on disk (outside the repo), with `XDG_CACHE_HOME="$RUNS/xdg"` when a step needs a cache.
- **The IMDb dump** (1.26 GB) is downloaded only with the user's go-ahead and a heavy-run slot from the co-ordinator, under `$RUNS`, never the shared cache.
- **Every shell block stands alone**: each plan step may run in a fresh shell, so a block sets every variable it uses (`RUNS=…`, `M=…`, `KERMIT=…`), every `rm -rf` is written `"${VAR:?}"`, and loops that split words run under `bash` (the session shell is zsh).
- **DuckDB** is on PATH only inside `nix develop`, from J3's flake edit on; before that, `nix shell --inputs-from . nixpkgs#duckdb`. Every DuckDB session the code opens sets `enable_progress_bar=false`.
- **Licences.** The JOB query files have no licence: never commit their text. Tests use hand-written JOB-shaped SQL or fetch the pinned files at test time.
- **Commits.** Conventional commits referencing `(#97)`, staging new files by name, committed by pathspec with the trailers as `--trailer` flags (above). The two trailers:
  ```
  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01WeRnvdstSxUY8CJGSx1NFB
  ```
- **Who implements: supervised** (the user's choice, 2026-10-15). A `plan-execution-supervisor` dispatches the roster's developer agents per task. Their hook, `scripts/hooks/developer_git_guard.py`, exists only in the roster's source tree, so it is copied into `$WT/scripts/hooks/` (done 2026-10-15; the shared `.git/info/exclude` ignores it and `.superpowers/`). Never commit it. If it is missing, copy it again from `/nix/store/02b0qd7bq81fgcn1axw3bfs15kmpvfvd-source/scripts/hooks/developer_git_guard.py`. The guard blocks `git reset/rebase/stash/clean/checkout/switch/restore/push`, `commit --amend`/`-a`, and `git add` of `.` or a directory; every commit step names its paths and carries the two trailers as flags, never inside a multi-line `-m` string (pasted from an indented fence, its second line would keep the indent): `git commit -m "<subject>" [-m "<body>"] --trailer "Co-Authored-By: …" --trailer "Claude-Session: …" -- <paths>`.
- **Green tree:** every task leaves the workspace compiling with all tests passing and ends in one commit.
- **Upstream state merged so far:** #96 phase 1 (`kermit_bench::cache::download_verified`; `BenchError::{Download { url, source }, UnusableDownload, Integrity { subject, .. }, Io}` as Part 4 assumes) and the fix for `lubm_mini_oracle`'s dictionary pin, both from `origin/master` at df01e37, merged into `aidanb/job` at f2d128b. The gate has no known pre-existing failure.
- **Arm and variant order is cosmetic.** Where J6 adds a `GeneratorSpec` variant, a `validate_generator` arm, a `dispatch` arm or a `gen` subcommand, #95's and #96's may already be there in any order. Insert JOB's as the last one, adjusting an Edit's anchor to the current last item if the plan's `old_string` no longer matches; resolve order textually at merge time.
- **Scope (Priority 6):** no task touches an index structure, an algorithm, an optimiser, `kermit-rdf`, or #95's and #96's files beyond the one `pub mod job;` line in `kermit-generate/src/lib.rs` and the `kermit-generate/Cargo.toml` dependency.

## File map

| File | Responsibility | Tasks |
| --- | --- | --- |
| `kermit-generate/Cargo.toml` | `sqlparser = { version = "0.63.0", features = ["visitor"] }` | J1 |
| `kermit-generate/src/lib.rs` | `pub mod job;` and one crate-docs line | J1 |
| `kermit-generate/src/job/mod.rs` (new) | module docs and `pub mod`s (J1, J3, J4); `JobGenerator`, `JobExtras`, `JobSource`, `TEMPLATES`, `filter_select` (J5) | J1, J3, J4, J5 |
| `kermit-generate/src/job/translate.rs` (new) | `parse_query`, `JobQuery`, `TranslateError` (J1); `plan_template`, `TemplatePlan` (J2) | J1, J2 |
| `kermit-generate/src/job/duckdb.rs` (new) | `Duckdb`: `locate`, `execute`, `query_u64`, `stream_rows` | J3 |
| `kermit-generate/src/job/pins.rs` (new) | mirrors, query commit, 114 file digests, 21 row counts | J4 |
| `kermit-generate/src/job/raw.rs` (new) | `RawCache`, `build`, `extract_tgz`, `default_raw_dir` (J4); `ensure` stub (J6), downloads (J7) | J4, J6, J7 |
| `kermit-generate/tests/fixtures/job/` (new) | hand-written IMDb-shaped fixture | J4 |
| `.gitignore` | `!kermit-generate/tests/fixtures/job/csv/*.csv` (the repo ignores `**/*.csv`) | J4 |
| `flake.nix` | `pkgs.duckdb` | J3 |
| `kermit/tests/common/job.rs` (new), `kermit/tests/common/mod.rs` | shared JOB test helpers (fixture cache, reversed CSVs, pinned provenance) | J5 |
| `kermit/tests/job_generate.rs` (new) | fixture pipeline, DuckDB-gated | J5 |
| `kermit/tests/cli_job_gen.rs` (new) | `bench gen job` and `bench run job-1` through the real binary | J6 |
| `kermit-bench/src/definition.rs` | `GeneratorSpec::Job`, validation, tests | J6 |
| `kermit/src/materialize.rs` | `dispatch` arm, `run_job` | J6 |
| `kermit/src/main.rs` | `GenSubcommand::Job`, `run_gen_job` | J6 |
| `benchmarks/job-1.yml` … `job-33.yml` (new) | the committed generator specs | J6 |
| `kermit/tests/job_translate.rs` (new) | the real 113 queries, network-gated | J7 |
| `docs/benchmarks/JOB.md` (new), `docs/benchmarks/README.md`, `benchmarks/README.md`, `CLAUDE.md` | docs | J8, J9 |
| `docs/superpowers/plans/2026-10-15-job-benchmark.md` | this plan | before J1 |

## Execution chunks

Each task is one chunk, implemented by one subagent and reviewed before the next. J9 is the design session's, after `FULL_EXIT` and with the user's go-ahead.

| Chunk | Plan text | Phase |
| --- | --- | --- |
| C01 | Task J1 | 1 |
| C02 | Task J2 | 1 |
| C03 | Task J3 | 1 |
| C04 | Task J4 Part A (Steps 1–14) | 1 |
| C04b | Task J4 Part B (Steps 15–27): the IMDb measurements, design session, user-gated | 1b |
| C05 | Task J5 | 1 |
| C06 | Task J6 | 2 |
| C07 | Task J7 | 3 |
| C08 | Task J8 | 4 |
| C09 | Task J9 and the final gate | 4 |

---

# Part 1: the translator (J1, J2)

### Task J1: Crate wiring and `parse_query`

The translator's first half. `kermit-generate/src/job/` appears with `translate.rs`; `parse_query` reads one JOB query with `sqlparser` (PostgreSQL dialect) and sorts the conjuncts of its `WHERE` clause into joins (`a.x = b.y` across two aliases) and per-alias filters, refusing every other shape by name (#78). Nothing here reads data or starts a process.

**Needs:** #95's push 1 (`kermit-generate`, so `lib.rs` and `Cargo.toml` exist) merged into `aidanb/job`. Re-check the two premises below after that merge, before Step 2.

**Premises about files this branch does not have yet** (from #95's contract, `git show 0de175a:docs/superpowers/plans/2026-10-15-generate-layer-contract.md`, S2/S3):
- `kermit-generate/Cargo.toml` has a `[dependencies]` table that ends `… serde_yaml = "0.9"`, with `kermit-parser` among the internal dependencies (the tests use `kermit_parser::JoinQuery` in J2).
- `kermit-generate/src/lib.rs` is `#![deny(missing_docs)]`, declares its modules with `mod …;` lines, and its crate docs say the generators `h2o` / `snap` / `job` are added by their issues.

**API notes** (sqlparser 0.63.0, `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/sqlparser-0.63.0/src/`; every name below was read from source):
- `Parser::parse_sql(&dyn Dialect, &str) -> Result<Vec<Statement>, ParserError>`: `parser/mod.rs:567`. An empty string parses to an empty `Vec` (`parse_statements`, `parser/mod.rs:517-548`). `ParserError`'s `Display` is `sql parser error: …`: `parser/mod.rs:178`. `PostgreSqlDialect {}`: `dialect/postgresql.rs:55`.
- `Statement::Query(Box<Query>)`: `ast/mod.rs:3597`. `Query { with, body: Box<SetExpr>, order_by, limit_clause, .. }`: `ast/query.rs:40-69`. `SetExpr::Select(Box<Select>)`: `ast/query.rs:152`.
- `Select { distinct, projection, from: Vec<TableWithJoins>, selection: Option<Expr>, group_by: GroupByExpr, having, .. }`: `ast/query.rs:445-508`. `GroupByExpr::Expressions(Vec<Expr>, Vec<GroupByWithModifier>)`: `ast/query.rs:3822`.
- `TableWithJoins { relation, joins }`: `ast/query.rs:1208`. `TableFactor::Table { name: ObjectName, alias: Option<TableAlias>, args, .. }`: `ast/query.rs:1462-1497`. `TableAlias { explicit, name: Ident, columns, .. }`: `ast/query.rs:2552`. `ObjectName(pub Vec<ObjectNamePart>)`, `ObjectNamePart::Identifier(Ident)`: `ast/mod.rs:415`, `:439`.
- `Expr::{Identifier, CompoundIdentifier(Vec<Ident>), BinaryOp { left, op, right }, Between, Nested(Box<Expr>)}`: `ast/mod.rs:891-1017`, `:1244`. `BinaryOperator::{And, Eq}`: `ast/operator.rs:123-127`.
- `visit_expressions(&V, FnMut(&Expr) -> ControlFlow<E>)`: `ast/visitor.rs:632`. `visit_expressions_mut(&mut V, FnMut(&mut Expr) -> ControlFlow<E>)`: `ast/visitor.rs:706`; its closure runs in `post_visit_expr`, after the node's children (`ast/visitor.rs:595-600`). Both are re-exported from `sqlparser::ast` under feature `visitor` (`ast/mod.rs:132-133`).
- `Display` is single-line unless the alternate flag `{:#}` is used (`SpaceOrNewline`, `display_utils.rs:74`), so `to_string()` gives `SELECT … FROM a AS x, b AS y WHERE …` with single spaces. `Between` prints `expr BETWEEN low AND high` (`ast/mod.rs:1819`), `Nested` prints `(…)` (`:2057`), `!=` is `BinaryOperator::NotEq` and prints `<>` (`ast/operator.rs:352`), a table alias prints `AS name` iff it was written with `AS` (`ast/query.rs:2569`).
- The projection `COUNT(*)` is built from `Function { name, uses_odbc_syntax, parameters, args, filter, null_treatment, over, within_group }` (`ast/mod.rs:8187-8232`), the same literal the visitor's own doctest uses (`ast/visitor.rs:665-682`); `Ident::new` (`ast/mod.rs:320`), `ObjectName: From<Vec<Ident>>` (`:417`), `FunctionArgExpr::Wildcard` (`:7974`).

**Files:**
- Modify: `kermit-generate/Cargo.toml` (one dependency line)
- Modify: `kermit-generate/src/lib.rs` (`pub mod job;`, one crate-docs line)
- Create: `kermit-generate/src/job/mod.rs`
- Create: `kermit-generate/src/job/translate.rs`

- [ ] **Step 1: Check the host.**
  ```bash
  find /proc/[0-9]*/exe -maxdepth 0 -lname '*kermit-bench-runs*' 2>/dev/null | wc -l
  ```
  Expected: `0`, and the co-ordinator has announced `FULL_EXIT`. Do not compile under a timing run.

- [ ] **Step 2: Wire the crate, then write the types, the tests and a stub `parse_query`.**

  **2a.** In `kermit-generate/Cargo.toml`, add the dependency after `serde_yaml` (alphabetical; if the premise above no longer holds, put it in its alphabetical place in `[dependencies]`):

  Edit, old: `serde_yaml = "0.9"`; new:
  ```toml
  serde_yaml = "0.9"
  sqlparser = { version = "0.63.0", features = ["visitor"] }
  ```
  The `visitor` feature is what exposes `visit_expressions_mut`; default features stay on (the contract pins exactly this line). `Cargo.lock` is gitignored in this repository, so the lock change is not committed.

  **2b.** In `kermit-generate/src/lib.rs`, declare the module and name JOB in the crate docs. Read the file first, then:
  - add `pub mod job;` in the module-declaration block (rustfmt's `reorder_modules` sorts the block; Step 6 runs `cargo fmt`, which puts it between `mod generator;` and `mod meta;`);
  - in the crate docs' sentence or list about the generators that sit on the layer, add the line below; if the docs have no such place, append it as the last paragraph of the crate docs:
  ```rust
  //! - [`job`]: the Join Order Benchmark over IMDb (#97).
  ```

  **2c.** Create `kermit-generate/src/job/mod.rs`:

```rust
//! The Join Order Benchmark (JOB) generator (#97).
//!
//! JOB (Leis et al., "How Good Are Query Optimizers, Really?", VLDB 2015) is
//! 113 SQL queries, in 33 templates, over the IMDb dataset. This module turns
//! one template into a kermit benchmark: each alias's filters are applied
//! while the relations are generated, so the Datalog rules join JOB's real
//! tables with JOB's real selectivities.
//!
//! - [`translate`]: SQL text to relation files and Datalog rules. Pure Rust
//!   over `sqlparser`; it reads no data.
//!
//! The design is in `docs/specs/2026-10-15-job-benchmark-design.md`.

pub mod translate;
```

  **2d.** Create `kermit-generate/src/job/translate.rs`. `parse_query` is a stub (`todo!`) so the tests compile and fail by panicking; the types, the error enum and its `Display` are final.

```rust
//! Translating JOB queries into relation files and Datalog rules.
//!
//! A JOB query is `SELECT MIN(…), … FROM table AS alias, … WHERE c1 AND c2 …`:
//! a conjunction of conditions over aliased tables. `parse_query` reads one
//! with `sqlparser` and sorts each condition into a *join* (`a.x = b.y`
//! across two aliases) or a *filter* (a condition on one alias); anything
//! else is an error that names the query and the condition. `plan_template`
//! then merges the joins of one template's variants into variables and
//! writes the relation files and the Datalog rule of each query.
//!
//! Filtering one table commutes with an inner join, so no filter reaches a
//! rule: the generator applies a relation's filter while it writes the
//! relation, and the rules join the filtered relations. This module is pure:
//! it reads no data and starts no process.

use std::fmt;

/// One `alias.column` reference.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ColumnRef {
    /// The alias, as written in the `FROM` clause.
    pub alias: String,
    /// The column of the alias's table.
    pub column: String,
}

/// One `FROM` entry, `table AS alias`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Alias {
    /// The alias.
    pub name: String,
    /// The table it names.
    pub table: String,
}

/// A JOB query, parsed and its `WHERE` conjuncts classified.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobQuery {
    /// The query's name, `6a`.
    pub name: String,
    /// The `FROM` entries, in `FROM` order.
    pub aliases: Vec<Alias>,
    /// The join conditions in text order, each as written: `(left, right)`.
    pub joins: Vec<(ColumnRef, ColumnRef)>,
    /// The filters in text order: the alias, and the conjunct as `sqlparser`
    /// prints it with every `alias.` stripped from its columns.
    pub filters: Vec<(String, String)>,
    /// The statement with its projection replaced by `COUNT(*)`, printed
    /// without a trailing `;`. DuckDB's answer to it is the query's expected
    /// cardinality.
    pub count_sql: String,
}

/// Why a JOB query could not be translated. Every variant but `NameClash`
/// names the query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TranslateError {
    /// `sqlparser` rejected the text.
    Parse {
        /// The query's name.
        query: String,
        /// `sqlparser`'s error text.
        message: String,
    },
    /// The text is not exactly one plain `SELECT … FROM … WHERE …`.
    NotOneSelect {
        /// The query's name.
        query: String,
    },
    /// A `FROM` item that is not `table AS alias`.
    UnsupportedFrom {
        /// The query's name.
        query: String,
        /// The item as `sqlparser` prints it.
        item: String,
    },
    /// Two `FROM` items share an alias.
    DuplicateAlias {
        /// The query's name.
        query: String,
        /// The repeated alias.
        alias: String,
    },
    /// A condition names `x.column` with `x` not in `FROM`.
    UnknownAlias {
        /// The query's name.
        query: String,
        /// The unknown alias.
        alias: String,
    },
    /// A condition names two or more aliases and is not `a.x = b.y`.
    CrossAliasCondition {
        /// The query's name.
        query: String,
        /// The condition as `sqlparser` prints it.
        condition: String,
    },
    /// A condition names no alias.
    NoAliasCondition {
        /// The query's name.
        query: String,
        /// The condition as `sqlparser` prints it.
        condition: String,
    },
    /// An alias in no join condition (raised by `plan_template`).
    UnjoinedAlias {
        /// The query's name.
        query: String,
        /// The alias.
        alias: String,
    },
    /// Two relations of a template, or two variables of a query, share a name
    /// (raised by `plan_template`).
    NameClash {
        /// The shared name.
        name: String,
    },
}

impl fmt::Display for TranslateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            | TranslateError::Parse { query, message } => write!(f, "query {query}: {message}"),
            | TranslateError::NotOneSelect { query } => write!(
                f,
                "query {query}: expected one SELECT ... FROM ... WHERE ... statement without \
                 DISTINCT, GROUP BY, HAVING, ORDER BY, LIMIT or WITH"
            ),
            | TranslateError::UnsupportedFrom { query, item } => {
                write!(f, "query {query}: FROM item `{item}` is not `table AS alias`")
            },
            | TranslateError::DuplicateAlias { query, alias } => {
                write!(f, "query {query}: alias `{alias}` appears twice in FROM")
            },
            | TranslateError::UnknownAlias { query, alias } => write!(
                f,
                "query {query}: a condition names alias `{alias}`, which is not in FROM"
            ),
            | TranslateError::CrossAliasCondition { query, condition } => write!(
                f,
                "query {query}: condition `{condition}` names several aliases but is not \
                 `a.x = b.y`"
            ),
            | TranslateError::NoAliasCondition { query, condition } => {
                write!(f, "query {query}: condition `{condition}` names no alias")
            },
            | TranslateError::UnjoinedAlias { query, alias } => {
                write!(f, "query {query}: alias `{alias}` is in no join condition")
            },
            | TranslateError::NameClash { name } => {
                write!(f, "two relations or variables are both named `{name}`")
            },
        }
    }
}

impl std::error::Error for TranslateError {}

/// Parses one JOB query and sorts the conjuncts of its `WHERE` clause.
///
/// `name` is the query's name (`6a`); it is carried into the result and into
/// every error. The text must be exactly one plain `SELECT … FROM … WHERE …`
/// (no `DISTINCT`, `GROUP BY`, `HAVING`, `ORDER BY`, `LIMIT` or `WITH`) whose
/// `FROM` items are all `table AS alias`.
///
/// The `WHERE` clause is flattened through its top-level `AND`s, left before
/// right. A parenthesised conjunct stays whole (`(a OR b)` is one conjunct)
/// and so does `x BETWEEN lo AND hi`, which `sqlparser` parses as one node. A
/// conjunct is classified by the aliases of its `alias.column` references:
/// two different aliases compared by `=` is a join; exactly one alias is a
/// filter on it; anything else is an error.
///
/// # Errors
///
/// A [`TranslateError`] for text `sqlparser` rejects, for any shape above that
/// is not supported, for an alias used twice or not declared, and for a
/// conjunct that is neither a join nor a filter.
pub fn parse_query(name: &str, sql: &str) -> Result<JobQuery, TranslateError> {
    todo!("parse and classify {name}: {sql}")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Parses `sql`, failing the test with the translator's own message.
    fn parse(name: &str, sql: &str) -> JobQuery {
        parse_query(name, sql).unwrap_or_else(|error| panic!("{error}"))
    }

    fn col(alias: &str, name: &str) -> ColumnRef {
        ColumnRef {
            alias: alias.to_owned(),
            column: name.to_owned(),
        }
    }

    fn alias(name: &str, table: &str) -> Alias {
        Alias {
            name: name.to_owned(),
            table: table.to_owned(),
        }
    }

    fn filter(alias: &str, text: &str) -> (String, String) { (alias.to_owned(), text.to_owned()) }

    #[test]
    fn between_is_one_filter() {
        let query = parse(
            "1a",
            "SELECT MIN(t.title) FROM title AS t, movie_keyword AS mk \
             WHERE t.production_year BETWEEN 1990 AND 2000 AND t.id = mk.movie_id",
        );
        assert_eq!(query.filters, vec![filter("t", "production_year BETWEEN 1990 AND 2000")]);
        assert_eq!(query.joins, vec![(col("t", "id"), col("mk", "movie_id"))]);
    }

    #[test]
    fn or_in_parentheses_is_one_filter() {
        let query = parse(
            "1a",
            "SELECT MIN(k.keyword) FROM keyword AS k, movie_keyword AS mk \
             WHERE (k.keyword = 'a' OR k.keyword = 'b') AND k.id = mk.keyword_id",
        );
        assert_eq!(query.filters, vec![filter("k", "(keyword = 'a' OR keyword = 'b')")]);
        assert_eq!(query.joins, vec![(col("k", "id"), col("mk", "keyword_id"))]);
    }

    #[test]
    fn quoted_and_and_dots_do_not_split() {
        // The second literal looks like a join and like a qualified column;
        // inside quotes it is neither, and it keeps its text.
        let query = parse(
            "1a",
            "SELECT MIN(mi.info) FROM movie_info AS mi, title AS t \
             WHERE mi.info LIKE '%a.b AND c%' AND mi.note LIKE '%t.id = mi.movie_id%' \
             AND mi.movie_id = t.id",
        );
        assert_eq!(
            query.filters,
            vec![
                filter("mi", "info LIKE '%a.b AND c%'"),
                filter("mi", "note LIKE '%t.id = mi.movie_id%'"),
            ]
        );
        assert_eq!(query.joins, vec![(col("mi", "movie_id"), col("t", "id"))]);
    }

    #[test]
    fn like_not_like_in_is_null() {
        let query = parse(
            "1a",
            "SELECT MIN(n.name) FROM name AS n, cast_info AS ci \
             WHERE n.name LIKE '%Ab%' AND n.name NOT LIKE '%Cd%' \
             AND ci.note IN ('(voice)', '(uncredited)') AND ci.note IS NOT NULL \
             AND n.gender IS NULL AND n.id = ci.person_id",
        );
        assert_eq!(
            query.filters,
            vec![
                filter("n", "name LIKE '%Ab%'"),
                filter("n", "name NOT LIKE '%Cd%'"),
                filter("ci", "note IN ('(voice)', '(uncredited)')"),
                filter("ci", "note IS NOT NULL"),
                filter("n", "gender IS NULL"),
            ]
        );
        assert_eq!(query.joins, vec![(col("n", "id"), col("ci", "person_id"))]);
    }

    #[test]
    fn self_join_aliases_are_distinct() {
        let query = parse(
            "1a",
            "SELECT MIN(mi1.info) FROM info_type AS it1, info_type AS it2, \
             movie_info AS mi1, movie_info AS mi2 \
             WHERE it1.info = 'rating' AND it2.info = 'votes' \
             AND mi1.info_type_id = it1.id AND mi2.info_type_id = it2.id \
             AND mi1.movie_id = mi2.movie_id",
        );
        assert_eq!(
            query.aliases,
            vec![
                alias("it1", "info_type"),
                alias("it2", "info_type"),
                alias("mi1", "movie_info"),
                alias("mi2", "movie_info"),
            ]
        );
        assert_eq!(
            query.filters,
            vec![filter("it1", "info = 'rating'"), filter("it2", "info = 'votes'")]
        );
        assert_eq!(
            query.joins,
            vec![
                (col("mi1", "info_type_id"), col("it1", "id")),
                (col("mi2", "info_type_id"), col("it2", "id")),
                (col("mi1", "movie_id"), col("mi2", "movie_id")),
            ]
        );
    }

    #[test]
    fn join_chain_is_three_joins() {
        // Conditions stay as written, whichever side the alias is on; merging
        // them into one variable is `plan_template`'s job.
        let query = parse(
            "1a",
            "SELECT MIN(t.title) FROM title AS t, movie_keyword AS mk, cast_info AS ci, \
             movie_info AS mi \
             WHERE t.id = mk.movie_id AND mk.movie_id = ci.movie_id \
             AND mi.movie_id = ci.movie_id",
        );
        assert_eq!(
            query.joins,
            vec![
                (col("t", "id"), col("mk", "movie_id")),
                (col("mk", "movie_id"), col("ci", "movie_id")),
                (col("mi", "movie_id"), col("ci", "movie_id")),
            ]
        );
        assert!(query.filters.is_empty());
    }

    #[test]
    fn filter_text_strips_the_alias() {
        // `sqlparser` prints the conjunct, so whitespace is normalised and
        // `!=` becomes `<>`; the alias goes from every column, and only from
        // columns, never from a string literal.
        let query = parse(
            "1a",
            "SELECT MIN(t.title) FROM title AS t, movie_keyword AS mk \
             WHERE t.title   !=\n    ''  AND t.season_nr < t.episode_nr \
             AND t.title LIKE 't.title%' AND t.id = mk.movie_id",
        );
        assert_eq!(
            query.filters,
            vec![
                filter("t", "title <> ''"),
                filter("t", "season_nr < episode_nr"),
                filter("t", "title LIKE 't.title%'"),
            ]
        );
    }

    #[test]
    fn count_sql_replaces_min() {
        let query = parse(
            "1a",
            "SELECT MIN(t.title) AS movie_title, MIN(n.name) AS actor \
             FROM title AS t, name AS n, cast_info AS ci \
             WHERE t.production_year > 2000 AND t.id = ci.movie_id AND n.id = ci.person_id;",
        );
        assert_eq!(
            query.count_sql,
            "SELECT COUNT(*) FROM title AS t, name AS n, cast_info AS ci \
             WHERE t.production_year > 2000 AND t.id = ci.movie_id AND n.id = ci.person_id"
        );
    }

    #[test]
    fn syntax_error_is_a_parse_error() {
        let error =
            parse_query("7b", "SELECT MIN(t.title) FROM title AS t WHERE (t.id = ").unwrap_err();
        match error {
            | TranslateError::Parse { query, message } => {
                assert_eq!(query, "7b");
                assert!(!message.is_empty());
            },
            | other => panic!("expected a parse error, got {other:?}"),
        }
    }

    #[test]
    fn not_a_single_select_is_rejected() {
        let shapes = [
            "",
            "SELECT MIN(a.x) FROM t AS a WHERE a.x = 1; SELECT MIN(a.x) FROM t AS a WHERE a.x = 1",
            "SELECT MIN(a.x) FROM t AS a WHERE a.x = 1 UNION SELECT MIN(b.x) FROM t AS b \
             WHERE b.x = 1",
            "SELECT MIN(a.x) FROM t AS a",
            "SELECT 1 WHERE 1 = 1",
            "SELECT DISTINCT a.x FROM t AS a WHERE a.x = 1",
            "SELECT a.x FROM t AS a WHERE a.x = 1 GROUP BY a.x",
            "SELECT a.x FROM t AS a WHERE a.x = 1 ORDER BY a.x",
            "SELECT a.x FROM t AS a WHERE a.x = 1 LIMIT 5",
            "WITH c AS (SELECT 1) SELECT MIN(a.x) FROM t AS a WHERE a.x = 1",
            "SELECT MIN(a.x) FROM t AS a WHERE a.x = 1 HAVING MIN(a.x) > 0",
            "DROP TABLE t",
        ];
        for sql in shapes {
            assert_eq!(
                parse_query("2b", sql),
                Err(TranslateError::NotOneSelect {
                    query: "2b".to_owned(),
                }),
                "{sql}"
            );
        }
    }

    #[test]
    fn unsupported_from_items_are_rejected() {
        let cases = [
            ("SELECT MIN(a.x) FROM t, u AS a WHERE a.x = 1", "t"),
            ("SELECT MIN(a.x) FROM public.t AS a WHERE a.x = 1", "public.t AS a"),
            ("SELECT MIN(a.x) FROM t AS a JOIN u AS b ON a.x = b.x WHERE a.x = 1", "t AS a"),
            ("SELECT MIN(a.x) FROM (SELECT b.x FROM u AS b) AS a WHERE a.x = 1", "(SELECT"),
            ("SELECT MIN(a.x) FROM t AS a (x, y) WHERE a.x = 1", "t AS a (x, y)"),
            ("SELECT MIN(a.x) FROM generate_series(1, 3) AS a WHERE a.x = 1", "generate_series"),
        ];
        for (sql, prefix) in cases {
            match parse_query("3c", sql) {
                | Err(TranslateError::UnsupportedFrom { query, item }) => {
                    assert_eq!(query, "3c");
                    assert!(item.starts_with(prefix), "{sql}: item was {item:?}");
                },
                | other => panic!("{sql}: expected UnsupportedFrom, got {other:?}"),
            }
        }
    }

    #[test]
    fn duplicate_alias_is_rejected() {
        assert_eq!(
            parse_query("4a", "SELECT MIN(t.x) FROM title AS t, name AS t WHERE t.x = 1"),
            Err(TranslateError::DuplicateAlias {
                query: "4a".to_owned(),
                alias: "t".to_owned(),
            })
        );
    }

    #[test]
    fn unknown_alias_is_rejected() {
        let cases = [
            (
                "SELECT MIN(t.title) FROM title AS t, movie_keyword AS mk \
                 WHERE t.id = mk.movie_id AND zz.keyword = 'x'",
                "zz",
            ),
            (
                "SELECT MIN(t.title) FROM title AS t, movie_keyword AS mk \
                 WHERE t.id = ghost.movie_id",
                "ghost",
            ),
        ];
        for (sql, unknown) in cases {
            assert_eq!(
                parse_query("5a", sql),
                Err(TranslateError::UnknownAlias {
                    query: "5a".to_owned(),
                    alias: unknown.to_owned(),
                }),
                "{sql}"
            );
        }
    }

    #[test]
    fn cross_alias_condition_is_rejected() {
        // Each is printed back exactly as written. The last is a plain join in
        // parentheses: a `Nested` is never flattened, so it is not a join.
        let conditions = [
            "t.production_year > mk.movie_id",
            "(t.id = mk.movie_id OR t.id = ci.movie_id)",
            "t.id = mk.movie_id + 1",
            "(t.id = mk.movie_id)",
        ];
        for condition in conditions {
            let sql = format!(
                "SELECT MIN(t.title) FROM title AS t, movie_keyword AS mk, cast_info AS ci \
                 WHERE {condition}"
            );
            assert_eq!(
                parse_query("5c", &sql),
                Err(TranslateError::CrossAliasCondition {
                    query: "5c".to_owned(),
                    condition: condition.to_owned(),
                }),
                "{sql}"
            );
        }
    }

    #[test]
    fn condition_without_alias_is_rejected() {
        for condition in ["production_year > 2000", "1 = 1"] {
            let sql = format!(
                "SELECT MIN(t.title) FROM title AS t, movie_keyword AS mk \
                 WHERE t.id = mk.movie_id AND {condition}"
            );
            assert_eq!(
                parse_query("5d", &sql),
                Err(TranslateError::NoAliasCondition {
                    query: "5d".to_owned(),
                    condition: condition.to_owned(),
                }),
                "{sql}"
            );
        }
    }

    #[test]
    fn error_messages_name_the_query_and_the_item() {
        let cross = parse_query(
            "6a",
            "SELECT MIN(t.title) FROM title AS t, movie_keyword AS mk \
             WHERE t.production_year > mk.movie_id",
        )
        .unwrap_err();
        let errors = vec![
            (
                TranslateError::Parse {
                    query: "6a".to_owned(),
                    message: "boom".to_owned(),
                },
                vec!["6a", "boom"],
            ),
            (
                TranslateError::NotOneSelect {
                    query: "6a".to_owned(),
                },
                vec!["6a"],
            ),
            (
                TranslateError::UnsupportedFrom {
                    query: "6a".to_owned(),
                    item: "title".to_owned(),
                },
                vec!["6a", "`title`"],
            ),
            (
                TranslateError::DuplicateAlias {
                    query: "6a".to_owned(),
                    alias: "t".to_owned(),
                },
                vec!["6a", "`t`"],
            ),
            (
                TranslateError::UnknownAlias {
                    query: "6a".to_owned(),
                    alias: "zz".to_owned(),
                },
                vec!["6a", "`zz`"],
            ),
            (cross, vec!["6a", "`t.production_year > mk.movie_id`"]),
            (
                TranslateError::NoAliasCondition {
                    query: "6a".to_owned(),
                    condition: "1 = 1".to_owned(),
                },
                vec!["6a", "`1 = 1`"],
            ),
            (
                TranslateError::UnjoinedAlias {
                    query: "6a".to_owned(),
                    alias: "k".to_owned(),
                },
                vec!["6a", "`k`"],
            ),
            (
                TranslateError::NameClash {
                    name: "movie_keyword".to_owned(),
                },
                vec!["`movie_keyword`"],
            ),
        ];
        for (error, needles) in errors {
            let text = error.to_string();
            for needle in needles {
                assert!(text.contains(needle), "{text:?} lacks {needle:?}");
            }
            let _: &dyn std::error::Error = &error;
        }
    }
}
```

- [ ] **Step 3: Run the tests. All 16 fail.**
  ```bash
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-generate job::translate::tests
  ```
  Expected: the first build compiles `sqlparser`, `sqlparser_derive`, `recursive` and `stacker` (a few minutes), then `test result: FAILED. 0 passed; 16 failed`. Every failure reads `not yet implemented: parse and classify <name>: <sql>`, including `error_messages_name_the_query_and_the_item` (its `cross` case calls `parse_query`). A compile error here is a mistake in Step 2; fix it before going on.

- [ ] **Step 4: Implement `parse_query`.** Two edits to `kermit-generate/src/job/translate.rs`. No `cargo fmt` has run since Step 2, so both old strings are exact.

  **4a.** Replace the import line. Edit, old: `use std::fmt;`; new:

```rust
use {
    sqlparser::{
        ast::{
            visit_expressions, visit_expressions_mut, BinaryOperator, Expr, Function, FunctionArg,
            FunctionArgExpr, FunctionArgumentList, FunctionArguments, GroupByExpr, Ident,
            ObjectName, ObjectNamePart, Select, SelectItem, SetExpr, Statement, TableFactor,
        },
        dialect::PostgreSqlDialect,
        parser::Parser,
    },
    std::{fmt, ops::ControlFlow},
};
```

  **4b.** Replace the stub. Edit, old:

```rust
pub fn parse_query(name: &str, sql: &str) -> Result<JobQuery, TranslateError> {
    todo!("parse and classify {name}: {sql}")
}
```

  new:

```rust
pub fn parse_query(name: &str, sql: &str) -> Result<JobQuery, TranslateError> {
    let statements = Parser::parse_sql(&PostgreSqlDialect {}, sql).map_err(|error| {
        TranslateError::Parse {
            query: name.to_owned(),
            message: error.to_string(),
        }
    })?;
    let Ok([mut statement]) = <[Statement; 1]>::try_from(statements) else {
        return Err(not_one_select(name));
    };
    let select = plain_select(name, &mut statement)?;
    let aliases = read_aliases(name, select)?;
    let Some(selection) = select.selection.as_ref() else {
        return Err(not_one_select(name));
    };
    let Conditions { joins, filters } = classify(name, &aliases, selection)?;
    select.projection = vec![count_star()];
    Ok(JobQuery {
        name: name.to_owned(),
        aliases,
        joins,
        filters,
        count_sql: statement.to_string(),
    })
}

fn not_one_select(name: &str) -> TranslateError {
    TranslateError::NotOneSelect {
        query: name.to_owned(),
    }
}

/// The statement's one `SELECT`, if it is a plain one: no `WITH`, `ORDER BY`,
/// `LIMIT`, `DISTINCT`, `GROUP BY` or `HAVING`, and a `FROM` clause. Each
/// would change what `COUNT(*)` counts, and JOB uses none of them.
fn plain_select<'a>(
    name: &str, statement: &'a mut Statement,
) -> Result<&'a mut Select, TranslateError> {
    let Statement::Query(query) = statement else {
        return Err(not_one_select(name));
    };
    if query.with.is_some() || query.order_by.is_some() || query.limit_clause.is_some() {
        return Err(not_one_select(name));
    }
    let SetExpr::Select(select) = &mut *query.body else {
        return Err(not_one_select(name));
    };
    let ungrouped = matches!(
        &select.group_by,
        GroupByExpr::Expressions(expressions, modifiers)
            if expressions.is_empty() && modifiers.is_empty()
    );
    if select.from.is_empty() || select.distinct.is_some() || select.having.is_some() || !ungrouped
    {
        return Err(not_one_select(name));
    }
    Ok(&mut **select)
}

/// The `FROM` clause as `table AS alias` entries. A join, a subquery, a
/// table function, a schema-qualified name, an alias with column names or a
/// missing alias is `UnsupportedFrom`.
fn read_aliases(name: &str, select: &Select) -> Result<Vec<Alias>, TranslateError> {
    let mut aliases: Vec<Alias> = Vec::new();
    for item in &select.from {
        let unsupported = || TranslateError::UnsupportedFrom {
            query: name.to_owned(),
            item: item.to_string(),
        };
        if !item.joins.is_empty() {
            return Err(unsupported());
        }
        let TableFactor::Table {
            name: table_name,
            alias: Some(alias),
            args: None,
            ..
        } = &item.relation
        else {
            return Err(unsupported());
        };
        let [ObjectNamePart::Identifier(table)] = table_name.0.as_slice() else {
            return Err(unsupported());
        };
        if !alias.columns.is_empty() {
            return Err(unsupported());
        }
        if aliases.iter().any(|seen| seen.name == alias.name.value) {
            return Err(TranslateError::DuplicateAlias {
                query: name.to_owned(),
                alias: alias.name.value.clone(),
            });
        }
        aliases.push(Alias {
            name: alias.name.value.clone(),
            table: table.value.clone(),
        });
    }
    Ok(aliases)
}

/// The `WHERE` clause, sorted.
struct Conditions {
    joins: Vec<(ColumnRef, ColumnRef)>,
    filters: Vec<(String, String)>,
}

/// Sorts each conjunct of `selection` into a join or a filter on one alias.
fn classify(
    name: &str, aliases: &[Alias], selection: &Expr,
) -> Result<Conditions, TranslateError> {
    let mut conditions = Conditions {
        joins: Vec::new(),
        filters: Vec::new(),
    };
    for conjunct in conjuncts(selection) {
        let columns = column_refs(conjunct);
        if let Some(unknown) = columns
            .iter()
            .find(|column| !aliases.iter().any(|alias| alias.name == column.alias))
        {
            return Err(TranslateError::UnknownAlias {
                query: name.to_owned(),
                alias: unknown.alias.clone(),
            });
        }
        // The distinct aliases the conjunct names, in order of mention.
        let mut named: Vec<&str> = Vec::new();
        for column in &columns {
            if !named.contains(&column.alias.as_str()) {
                named.push(&column.alias);
            }
        }
        match named.as_slice() {
            | [] => {
                return Err(TranslateError::NoAliasCondition {
                    query: name.to_owned(),
                    condition: conjunct.to_string(),
                });
            },
            | [alias] => conditions.filters.push(((*alias).to_owned(), without_alias(conjunct))),
            | _ => match as_join(conjunct) {
                | Some(join) => conditions.joins.push(join),
                | None => {
                    return Err(TranslateError::CrossAliasCondition {
                        query: name.to_owned(),
                        condition: conjunct.to_string(),
                    });
                },
            },
        }
    }
    Ok(conditions)
}

/// The top-level `AND` tree of `expr` as a list, left before right. Only
/// `AND` is flattened: `Nested` (`(a OR b)`) and `Between` stay whole.
fn conjuncts(expr: &Expr) -> Vec<&Expr> {
    fn walk<'a>(expr: &'a Expr, out: &mut Vec<&'a Expr>) {
        match expr {
            | Expr::BinaryOp {
                left,
                op: BinaryOperator::And,
                right,
            } => {
                walk(left, out);
                walk(right, out);
            },
            | other => out.push(other),
        }
    }
    let mut out = Vec::new();
    walk(expr, &mut out);
    out
}

/// Every `alias.column` in `conjunct`, in text order. A reference with any
/// other number of parts (`column`, `schema.table.column`) is not one.
fn column_refs(conjunct: &Expr) -> Vec<ColumnRef> {
    let mut columns = Vec::new();
    let _ = visit_expressions(conjunct, |expr| {
        if let Expr::CompoundIdentifier(parts) = expr {
            if let [alias, column] = parts.as_slice() {
                columns.push(ColumnRef {
                    alias: alias.value.clone(),
                    column: column.value.clone(),
                });
            }
        }
        ControlFlow::<()>::Continue(())
    });
    columns
}

/// `a.x = b.y` as `(a.x, b.y)`, when `conjunct` is exactly that.
fn as_join(conjunct: &Expr) -> Option<(ColumnRef, ColumnRef)> {
    let Expr::BinaryOp {
        left,
        op: BinaryOperator::Eq,
        right,
    } = conjunct
    else {
        return None;
    };
    let (Expr::CompoundIdentifier(left_parts), Expr::CompoundIdentifier(right_parts)) =
        (&**left, &**right)
    else {
        return None;
    };
    let ([left_alias, left_column], [right_alias, right_column]) =
        (left_parts.as_slice(), right_parts.as_slice())
    else {
        return None;
    };
    Some((
        ColumnRef {
            alias: left_alias.value.clone(),
            column: left_column.value.clone(),
        },
        ColumnRef {
            alias: right_alias.value.clone(),
            column: right_column.value.clone(),
        },
    ))
}

/// `conjunct` as `sqlparser` prints it, with every `alias.column` rewritten
/// to `column` in the syntax tree (so a string literal is never touched).
fn without_alias(conjunct: &Expr) -> String {
    let mut stripped = conjunct.clone();
    let _ = visit_expressions_mut(&mut stripped, |expr| {
        let column = match expr {
            | Expr::CompoundIdentifier(parts) if parts.len() == 2 => parts.pop(),
            | _ => None,
        };
        if let Some(column) = column {
            *expr = Expr::Identifier(column);
        }
        ControlFlow::<()>::Continue(())
    });
    stripped.to_string()
}

/// The projection `COUNT(*)`.
fn count_star() -> SelectItem {
    SelectItem::UnnamedExpr(Expr::Function(Function {
        name: ObjectName::from(vec![Ident::new("COUNT")]),
        uses_odbc_syntax: false,
        parameters: FunctionArguments::None,
        args: FunctionArguments::List(FunctionArgumentList {
            duplicate_treatment: None,
            args: vec![FunctionArg::Unnamed(FunctionArgExpr::Wildcard)],
            clauses: vec![],
        }),
        filter: None,
        null_treatment: None,
        over: None,
        within_group: vec![],
    }))
}
```

- [ ] **Step 5: Run the tests. All 16 pass.**
  ```bash
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-generate job::translate::tests
  ```
  Expected: `test result: ok. 16 passed; 0 failed`. A failure whose message shows a different `sqlparser` printing than the test expects (spacing, parentheses, `<>`) means the test's expected string is wrong, not the translator: the AST is what the translator reads, and the printed form is only what the tests pin. Check it against the API notes, then correct the test's string.

- [ ] **Step 6: Format, lint, document, run the crate, commit.**
  ```bash
  nix develop --command cargo fmt --all
  git status --short
  ```
  Expected: only `kermit-generate/Cargo.toml`, `kermit-generate/src/lib.rs`, `kermit-generate/src/job/mod.rs` and `kermit-generate/src/job/translate.rs` are changed or new (`Cargo.lock` is ignored). If `fmt` touched any other file, the tree was not fmt-clean before this task: revert that file by hand and report it.
  ```bash
  nix develop --command env CARGO_BUILD_JOBS=2 RUSTFLAGS=-Dwarnings cargo clippy -p kermit-generate --all-targets
  nix develop --command env CARGO_BUILD_JOBS=2 RUSTDOCFLAGS=-Dwarnings cargo doc -p kermit-generate --no-deps
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-generate
  ```
  Expected: no warnings from clippy or doc; every `kermit-generate` test passes (the 16 above plus #95's). Then commit, staging the new files by name and committing by pathspec:
  ```bash
  git add kermit-generate/src/job/mod.rs kermit-generate/src/job/translate.rs
  git commit -m "feat(job): parse JOB queries into joins and filters (#97)" \
    -m "kermit-generate gains a job module and its sqlparser dependency. parse_query reads one JOB query (PostgreSQL dialect), flattens the WHERE clause through its top-level ANDs so BETWEEN and parenthesised ORs stay whole, and sorts each conjunct into a join, a filter on one alias, or a named error." \
    --trailer "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>" \
    --trailer "Claude-Session: https://claude.ai/code/session_01WeRnvdstSxUY8CJGSx1NFB" \
    -- kermit-generate/Cargo.toml kermit-generate/src/lib.rs kermit-generate/src/job/mod.rs kermit-generate/src/job/translate.rs
  ```

- [ ] **Step 7: Mutation checks.** Each must make the named tests fail. For each: confirm the commit is in, apply the exact edit, check `git diff --stat` shows it applied, run the named tests and see them fail, reverse the exact edit (never `git checkout`), re-run to green, and end with `git status --short` empty.

  | # | Mutant | Must fail |
  | --- | --- | --- |
  | 1 | in `conjuncts`' `walk`, add the arm `\| Expr::Nested(inner) => walk(inner, out),` before `\| other => out.push(other),` | `or_in_parentheses_is_one_filter`, `cross_alias_condition_is_rejected` |
  | 2 | in `without_alias`, return `conjunct.to_string()` in place of `stripped.to_string()` | `between_is_one_filter`, `filter_text_strips_the_alias`, `or_in_parentheses_is_one_filter` |

  Run: `nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-generate job::translate::tests`.

---

### Task J2: `plan_template`

The translator's second half. One template's variants (`6a`…`6f`, already parsed) become the relation files the generator writes and one Datalog rule per query. Each query's joins merge into variables (union-find over `alias.column`), ordered by the position of the first join that mentions them; each alias becomes a relation of its join columns in variable order plus a carried `id`; identical relations are shared by (table, columns, filter) across the template's variants.

The spec's template 6, from six hand-written statements of the same shape, comes out as nine files and these rules (the first is the spec's example):

```
6a: Q(K, T, N) :- cast_info(T, N, _), keyword_k_ace(K), movie_keyword(K, T, _), name_n_abcde(N), title_t_a(T).
```

**Needs:** J1 committed.

**Decisions this task makes where the contract is silent** (also in the report to the design session):
- Two join columns of one alias in one variable (`ml.movie_id = t.id AND ml.linked_movie_id = t.id`) keep their order of first mention (a stable sort by variable position). JOB has none (spec § Workload); `two_columns_of_one_alias_in_one_variable_keep_text_order` pins the choice.
- A relation's sharing key is (table, columns, filter) as the contract says; `join_columns` is not in it. Two aliases that differ only in whether `id` is joined or carried would share one file, which is harmless because `id` is a primary key and never NULL.

**Files:**
- Modify: `kermit-generate/src/job/translate.rs`

- [ ] **Step 1: Check the host.** As J1 Step 1 (`0`, `FULL_EXIT` announced).

- [ ] **Step 2: Add the plan types, a stub `plan_template` and the tests.** All edits to `kermit-generate/src/job/translate.rs`, which `cargo fmt` has already formatted in J1 Step 6.

  **2a. Types and stub.** Insert the following immediately above the line `#[cfg(test)]` (it appears once). The types are final; `plan_template` is a stub.

```rust
/// One relation file of a template.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelationPlan {
    /// The file and relation name: the table's name when unfiltered, else
    /// `{table}_{alias}_{letters}` (`keyword_k_ace`). The generator's sink
    /// requires `[a-z][a-z0-9_]*`, which holds for JOB's lowercase table and
    /// alias names; the sink rejects anything else.
    pub name: String,
    /// The IMDb table the rows come from.
    pub table: String,
    /// The columns in file order: the join columns in variable order, then
    /// `id` unless it is already a join column.
    pub columns: Vec<String>,
    /// How many leading columns are join columns; rows where one is NULL
    /// cannot join and are left out.
    pub join_columns: usize,
    /// The alias's filters joined by ` AND ` in text order, with the alias
    /// stripped from every column; empty when the relation is unfiltered.
    pub filter: String,
    /// The queries that use this relation, in input order.
    pub queries: Vec<String>,
}

/// One query's rule and its oracle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RulePlan {
    /// The query's name, `6a`.
    pub query: String,
    /// The Datalog rule, `Q(K, T, N) :- …`.
    pub datalog: String,
    /// The query with `COUNT(*)` in place of its projection: the oracle for
    /// the rule's cardinality.
    pub count_sql: String,
}

/// A template's relation files and its rules.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplatePlan {
    /// The distinct relation files, in order of first use (query order, then
    /// `FROM` order).
    pub relations: Vec<RelationPlan>,
    /// One rule per query, in input order.
    pub rules: Vec<RulePlan>,
}

/// Plans one template: `queries` are its variants in letter order.
///
/// **Variables.** Within a query, the joins' columns merge into variables
/// (union-find over `alias.column`). A variable's position is that of the
/// first join, in text order, that mentions one of its columns. It is named
/// after the first alias, in text order, whose `id` column it contains
/// (`k.id` gives `K`, `mi_idx1.id` gives `MI_IDX1`), or `ALIAS_COLUMN`
/// uppercased after its first column when it contains no `id`.
///
/// **Relations.** An alias's columns are its join columns in variable order,
/// then `id` unless `id` is already one of them. A relation is shared by
/// every alias, in every query of the template, with the same table, columns
/// and filter, whatever the aliases are called.
///
/// **Rules.** `Q(<every variable, in position order>) :- <one atom per alias,
/// in FROM order>.`, each atom the alias's relation with its variables as
/// terms and `_` for a carried `id`. An empty slice plans nothing.
///
/// # Errors
///
/// `UnjoinedAlias` for an alias in no join condition, and `NameClash` for two
/// variables of one query, or two relations of the template, with one name.
pub fn plan_template(queries: &[JobQuery]) -> Result<TemplatePlan, TranslateError> {
    todo!("plan {} queries", queries.len())
}
```

  **2b. Tests.** Inside `mod tests`, after the last J1 test (`error_messages_name_the_query_and_the_item`) and before the closing `}` of the module, add the helpers and tests below. Read the file's tail first and edit from the last test's closing brace.

```rust
    /// Plans the `(name, sql)` variants of one template.
    fn plan<S: AsRef<str>>(variants: &[(&str, S)]) -> TemplatePlan {
        let queries: Vec<JobQuery> =
            variants.iter().map(|(name, sql)| parse(name, sql.as_ref())).collect();
        plan_template(&queries).unwrap_or_else(|error| panic!("{error}"))
    }

    /// One line per relation: `name: table(columns) join=n filter=[…] queries=…`.
    fn described(planned: &TemplatePlan) -> Vec<String> {
        planned
            .relations
            .iter()
            .map(|relation| {
                format!(
                    "{}: {}({}) join={} filter=[{}] queries={}",
                    relation.name,
                    relation.table,
                    relation.columns.join(", "),
                    relation.join_columns,
                    relation.filter,
                    relation.queries.join(",")
                )
            })
            .collect()
    }

    fn datalog(planned: &TemplatePlan) -> Vec<String> {
        planned.rules.iter().map(|rule| rule.datalog.clone()).collect()
    }

    /// The six variants of a template-6-shaped query: five aliases, three
    /// variables, and a filter on `k`, `n` and `t` that changes from variant
    /// to variant (`6f` has none on `n`).
    ///
    /// The plan, derived by hand. The joins, in text order, mention these
    /// columns first-to-last: `k.id`, `mk.keyword_id`, `t.id`, `mk.movie_id`,
    /// `ci.movie_id`, `n.id`, `ci.person_id`. The variables, in position
    /// order, are `K` = {`k.id`, `mk.keyword_id`}, `T` = {`t.id`,
    /// `mk.movie_id`, `ci.movie_id`} and `N` = {`n.id`, `ci.person_id`}, at
    /// positions 0, 1 and 2, each named after the `id` it contains. The
    /// drafts, in `FROM` order (`ci`, `k`, `mk`, `n`, `t`), are
    /// `cast_info(movie_id, person_id, id)` read as `(T, N, _)`, `keyword(id)`
    /// as `(K)`, `movie_keyword(keyword_id, movie_id, id)` as `(K, T, _)`,
    /// `name(id)` as `(N)` and `title(id)` as `(T)`.
    fn template_6() -> Vec<JobQuery> {
        let one = "k.keyword = 'kw-one'";
        let two = "k.keyword IN ('kw-two', 'kw-three')";
        let person = "n.name LIKE '%Ab%Cd%'";
        [
            ("6a", one, person, "t.production_year > 2010"),
            ("6b", two, person, "t.production_year > 2014"),
            ("6c", one, person, "t.production_year > 2014"),
            ("6d", two, person, "t.production_year > 2000"),
            ("6e", one, person, "t.production_year > 2000"),
            ("6f", two, "", "t.production_year > 2000"),
        ]
        .into_iter()
        .map(|(name, keyword_filter, person_filter, year_filter)| {
            let filters: String = [keyword_filter, person_filter, year_filter]
                .iter()
                .filter(|condition| !condition.is_empty())
                .map(|condition| format!("{condition} AND "))
                .collect();
            parse(
                name,
                &format!(
                    "SELECT MIN(k.keyword), MIN(n.name), MIN(t.title) \
                     FROM cast_info AS ci, keyword AS k, movie_keyword AS mk, name AS n, \
                     title AS t \
                     WHERE {filters}k.id = mk.keyword_id AND t.id = mk.movie_id \
                     AND t.id = ci.movie_id AND ci.movie_id = mk.movie_id \
                     AND n.id = ci.person_id"
                ),
            )
        })
        .collect()
    }

    /// Template 6's rule for a variant that uses these relations.
    fn rule_6(keyword: &str, person: &str, title: &str) -> String {
        format!(
            "Q(K, T, N) :- cast_info(T, N, _), {keyword}(K), movie_keyword(K, T, _), \
             {person}(N), {title}(T)."
        )
    }

    #[test]
    fn template_6_shapes() {
        let queries = template_6();
        let planned = plan_template(&queries).unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(
            described(&planned),
            [
                "cast_info: cast_info(movie_id, person_id, id) join=2 filter=[] \
                 queries=6a,6b,6c,6d,6e,6f",
                "keyword_k_ace: keyword(id) join=1 filter=[keyword = 'kw-one'] queries=6a,6c,6e",
                "movie_keyword: movie_keyword(keyword_id, movie_id, id) join=2 filter=[] \
                 queries=6a,6b,6c,6d,6e,6f",
                "name_n_abcde: name(id) join=1 filter=[name LIKE '%Ab%Cd%'] queries=6a,6b,6c,6d,6e",
                "title_t_a: title(id) join=1 filter=[production_year > 2010] queries=6a",
                "keyword_k_bdf: keyword(id) join=1 \
                 filter=[keyword IN ('kw-two', 'kw-three')] queries=6b,6d,6f",
                "title_t_bc: title(id) join=1 filter=[production_year > 2014] queries=6b,6c",
                "title_t_def: title(id) join=1 filter=[production_year > 2000] queries=6d,6e,6f",
                "name: name(id) join=1 filter=[] queries=6f",
            ]
        );
        // The spec's example, verbatim.
        assert_eq!(
            planned.rules[0].datalog,
            "Q(K, T, N) :- cast_info(T, N, _), keyword_k_ace(K), movie_keyword(K, T, _), \
             name_n_abcde(N), title_t_a(T)."
        );
        assert_eq!(
            datalog(&planned),
            [
                rule_6("keyword_k_ace", "name_n_abcde", "title_t_a"),
                rule_6("keyword_k_bdf", "name_n_abcde", "title_t_bc"),
                rule_6("keyword_k_ace", "name_n_abcde", "title_t_bc"),
                rule_6("keyword_k_bdf", "name_n_abcde", "title_t_def"),
                rule_6("keyword_k_ace", "name_n_abcde", "title_t_def"),
                rule_6("keyword_k_bdf", "name", "title_t_def"),
            ]
        );
        assert_eq!(planned.rules.len(), queries.len());
        for (rule, query) in planned.rules.iter().zip(&queries) {
            assert_eq!(rule.query, query.name);
            assert_eq!(rule.count_sql, query.count_sql);
        }
    }

    #[test]
    fn every_rule_parses_as_datalog() {
        use kermit_parser::{JoinQuery, Term};
        let planned = plan_template(&template_6()).unwrap_or_else(|error| panic!("{error}"));
        let names: Vec<&str> =
            planned.relations.iter().map(|relation| relation.name.as_str()).collect();
        for rule in &planned.rules {
            let parsed: JoinQuery = rule
                .datalog
                .parse()
                .unwrap_or_else(|error| panic!("{}: {error}", rule.datalog));
            assert_eq!(parsed.head.name, "Q");
            assert_eq!(
                parsed.head.terms,
                vec![Term::Var("K".into()), Term::Var("T".into()), Term::Var("N".into())]
            );
            assert_eq!(parsed.body.len(), 5);
            for atom in &parsed.body {
                assert!(names.contains(&atom.name.as_str()), "{} is not planned", atom.name);
            }
            assert_eq!(
                parsed.body[0].terms,
                vec![Term::Var("T".into()), Term::Var("N".into()), Term::Placeholder]
            );
        }
    }

    #[test]
    fn variables_follow_text_order() {
        let forward = "SELECT MIN(t.title) FROM movie_keyword AS mk, title AS t, keyword AS k \
                       WHERE t.id = mk.movie_id AND k.id = mk.keyword_id";
        let reversed = "SELECT MIN(t.title) FROM movie_keyword AS mk, title AS t, keyword AS k \
                        WHERE k.id = mk.keyword_id AND t.id = mk.movie_id";
        let first = plan(&[("1a", forward)]);
        let second = plan(&[("1a", reversed)]);
        assert_eq!(datalog(&first), ["Q(T, K) :- movie_keyword(T, K, _), title(T), keyword(K)."]);
        assert_eq!(datalog(&second), ["Q(K, T) :- movie_keyword(K, T, _), title(T), keyword(K)."]);
        assert_eq!(first.relations[0].columns, ["movie_id", "keyword_id", "id"]);
        assert_eq!(second.relations[0].columns, ["keyword_id", "movie_id", "id"]);
    }

    #[test]
    fn id_is_appended_only_when_not_joined() {
        // `kt` joins on its `id` alone, `t` on `kind_id` and `id`, `ci` on
        // `movie_id` only: its `id` is carried.
        let planned = plan(&[(
            "1a",
            "SELECT MIN(t.title) FROM title AS t, kind_type AS kt, cast_info AS ci \
             WHERE t.kind_id = kt.id AND t.id = ci.movie_id",
        )]);
        assert_eq!(
            described(&planned),
            [
                "title: title(kind_id, id) join=2 filter=[] queries=1a",
                "kind_type: kind_type(id) join=1 filter=[] queries=1a",
                "cast_info: cast_info(movie_id, id) join=1 filter=[] queries=1a",
            ]
        );
        assert_eq!(
            datalog(&planned),
            ["Q(KT, T) :- title(KT, T), kind_type(KT), cast_info(T, _)."]
        );
    }

    #[test]
    fn shared_relation_keeps_column_order_in_its_key() {
        // The same filter on `mk` in all three variants, but `1b` reads the
        // joins the other way round, so its `mk` file has its columns the
        // other way round: a second file, not a shared one. `1a` and `1c`
        // agree and share.
        let sql = |joins: &str| {
            format!(
                "SELECT MIN(t.title) FROM movie_keyword AS mk, title AS t, keyword AS k \
                 WHERE mk.keyword_id > 5 AND {joins}"
            )
        };
        let forward = sql("t.id = mk.movie_id AND k.id = mk.keyword_id");
        let reversed = sql("k.id = mk.keyword_id AND t.id = mk.movie_id");
        let planned = plan(&[("1a", &forward), ("1b", &reversed), ("1c", &forward)]);
        assert_eq!(
            described(&planned),
            [
                "movie_keyword_mk_ac: movie_keyword(movie_id, keyword_id, id) join=2 \
                 filter=[keyword_id > 5] queries=1a,1c",
                "title: title(id) join=1 filter=[] queries=1a,1b,1c",
                "keyword: keyword(id) join=1 filter=[] queries=1a,1b,1c",
                "movie_keyword_mk_b: movie_keyword(keyword_id, movie_id, id) join=2 \
                 filter=[keyword_id > 5] queries=1b",
            ]
        );
        assert_eq!(
            datalog(&planned),
            [
                "Q(T, K) :- movie_keyword_mk_ac(T, K, _), title(T), keyword(K).",
                "Q(K, T) :- movie_keyword_mk_b(K, T, _), title(T), keyword(K).",
                "Q(T, K) :- movie_keyword_mk_ac(T, K, _), title(T), keyword(K).",
            ]
        );
    }

    #[test]
    fn filtered_name_lists_variant_letters() {
        // Multi-digit templates: `16a` contributes `a`, not `16a`.
        let sql = |year: &str| {
            format!(
                "SELECT MIN(t.title) FROM title AS t, movie_keyword AS mk \
                 WHERE t.production_year > {year} AND t.id = mk.movie_id"
            )
        };
        let planned = plan(&[("16a", sql("2000")), ("16b", sql("2010")), ("16c", sql("2000"))]);
        assert_eq!(
            described(&planned),
            [
                "title_t_ac: title(id) join=1 filter=[production_year > 2000] queries=16a,16c",
                "movie_keyword: movie_keyword(movie_id, id) join=1 filter=[] \
                 queries=16a,16b,16c",
                "title_t_b: title(id) join=1 filter=[production_year > 2010] queries=16b",
            ]
        );
        assert_eq!(
            datalog(&planned),
            [
                "Q(T) :- title_t_ac(T), movie_keyword(T, _).",
                "Q(T) :- title_t_b(T), movie_keyword(T, _).",
                "Q(T) :- title_t_ac(T), movie_keyword(T, _).",
            ]
        );
    }

    #[test]
    fn renamed_aliases_still_share() {
        // Template-8 style: the variants rename their aliases (`an1`/`an`,
        // `n1`/`n`), so the variables differ, but the files do not.
        let planned = plan(&[
            (
                "8a",
                "SELECT MIN(an1.name) FROM aka_name AS an1, name AS n1, cast_info AS ci \
                 WHERE an1.name LIKE '%Y%' AND an1.person_id = n1.id AND n1.id = ci.person_id",
            ),
            (
                "8b",
                "SELECT MIN(an.name) FROM aka_name AS an, name AS n, cast_info AS ci \
                 WHERE an.name LIKE '%Y%' AND an.person_id = n.id AND n.id = ci.person_id",
            ),
        ]);
        assert_eq!(
            described(&planned),
            [
                "aka_name_an1_ab: aka_name(person_id, id) join=1 filter=[name LIKE '%Y%'] \
                 queries=8a,8b",
                "name: name(id) join=1 filter=[] queries=8a,8b",
                "cast_info: cast_info(person_id, id) join=1 filter=[] queries=8a,8b",
            ]
        );
        assert_eq!(
            datalog(&planned),
            [
                "Q(N1) :- aka_name_an1_ab(N1, _), name(N1), cast_info(N1, _).",
                "Q(N) :- aka_name_an1_ab(N, _), name(N), cast_info(N, _).",
            ]
        );
    }

    #[test]
    fn variable_named_after_id_alias() {
        // The `id` member is on the right of both joins, and its alias has a
        // digit. Both `cct` aliases read one shared file, once each in `queries`.
        let planned = plan(&[(
            "1a",
            "SELECT MIN(cc.id) FROM complete_cast AS cc, comp_cast_type AS cct1, \
             comp_cast_type AS cct2 \
             WHERE cc.subject_id = cct1.id AND cc.status_id = cct2.id",
        )]);
        assert_eq!(
            described(&planned),
            [
                "complete_cast: complete_cast(subject_id, status_id, id) join=2 filter=[] \
                 queries=1a",
                "comp_cast_type: comp_cast_type(id) join=1 filter=[] queries=1a",
            ]
        );
        assert_eq!(
            datalog(&planned),
            [
                "Q(CCT1, CCT2) :- complete_cast(CCT1, CCT2, _), comp_cast_type(CCT1), \
                 comp_cast_type(CCT2)."
            ]
        );
    }

    #[test]
    fn fallback_variable_name() {
        // No member is an `id`: the variable is named after its first member.
        let planned = plan(&[(
            "1a",
            "SELECT MIN(ci.note) FROM cast_info AS ci, movie_keyword AS mk \
             WHERE ci.movie_id = mk.movie_id",
        )]);
        assert_eq!(
            datalog(&planned),
            ["Q(CI_MOVIE_ID) :- cast_info(CI_MOVIE_ID, _), movie_keyword(CI_MOVIE_ID, _)."]
        );
    }

    #[test]
    fn two_columns_of_one_alias_in_one_variable_keep_text_order() {
        // JOB has no such query; the translator keeps the columns in the order
        // the joins first mention them, and the variable repeats in the atom.
        let planned = plan(&[(
            "1a",
            "SELECT MIN(t.title) FROM movie_link AS ml, title AS t \
             WHERE ml.movie_id = t.id AND ml.linked_movie_id = t.id",
        )]);
        assert_eq!(
            described(&planned),
            [
                "movie_link: movie_link(movie_id, linked_movie_id, id) join=2 filter=[] \
                 queries=1a",
                "title: title(id) join=1 filter=[] queries=1a",
            ]
        );
        assert_eq!(datalog(&planned), ["Q(T) :- movie_link(T, T, _), title(T)."]);
    }

    #[test]
    fn unjoined_alias_is_an_error() {
        // `k` appears in a filter, which does not join it.
        let query = parse(
            "9z",
            "SELECT MIN(t.title) FROM title AS t, keyword AS k, movie_keyword AS mk \
             WHERE k.keyword = 'kw' AND mk.movie_id = t.id",
        );
        assert_eq!(
            plan_template(&[query]),
            Err(TranslateError::UnjoinedAlias {
                query: "9z".to_owned(),
                alias: "k".to_owned(),
            })
        );
    }

    #[test]
    fn relation_name_clash_is_an_error() {
        // Unfiltered, so both `mk` files would be called `movie_keyword`, but
        // their columns differ (see `variables_follow_text_order`).
        let forward = parse(
            "1a",
            "SELECT MIN(t.title) FROM movie_keyword AS mk, title AS t, keyword AS k \
             WHERE t.id = mk.movie_id AND k.id = mk.keyword_id",
        );
        let reversed = parse(
            "1b",
            "SELECT MIN(t.title) FROM movie_keyword AS mk, title AS t, keyword AS k \
             WHERE k.id = mk.keyword_id AND t.id = mk.movie_id",
        );
        assert_eq!(
            plan_template(&[forward, reversed]),
            Err(TranslateError::NameClash {
                name: "movie_keyword".to_owned(),
            })
        );
    }

    #[test]
    fn variable_name_clash_is_an_error() {
        // `mi.x` has no `id` in its variable: its name falls back to `MI_X`,
        // which is also the name `mi_x.id` gives the other variable.
        let query = parse(
            "1a",
            "SELECT MIN(mi.x) FROM movie_info AS mi, aka_name AS a, title AS mi_x, \
             keyword AS b \
             WHERE mi.x = a.y AND mi_x.id = b.z",
        );
        assert_eq!(
            plan_template(&[query]),
            Err(TranslateError::NameClash {
                name: "MI_X".to_owned(),
            })
        );
    }

    #[test]
    fn names_survive_digits_and_underscores() {
        // A two-variant template 16 whose aliases carry digits and underscores
        // (`it1`, `mi_idx1`) and whose number has two digits: the names keep
        // the digits and underscores, and the letters drop the `16`. (Template
        // 6's names are pinned in `template_6_shapes`.)
        let rank = |year: &str| {
            format!(
                "SELECT MIN(mi_idx1.info) FROM info_type AS it1, movie_info_idx AS mi_idx1, \
                 title AS t \
                 WHERE it1.info = 'rank-info' AND t.production_year > {year} \
                 AND mi_idx1.info_type_id = it1.id AND mi_idx1.movie_id = t.id"
            )
        };
        let sixteen = plan(&[("16a", rank("2000")), ("16b", rank("2005"))]);
        assert_eq!(
            sixteen.relations.iter().map(|relation| relation.name.as_str()).collect::<Vec<_>>(),
            ["info_type_it1_ab", "movie_info_idx", "title_t_a", "title_t_b"]
        );
    }
```

- [ ] **Step 3: Run the tests. The 14 new ones fail, the 16 from J1 pass.**
  ```bash
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-generate job::translate::tests
  ```
  Expected: `test result: FAILED. 16 passed; 14 failed`. Each failure reads `not yet implemented: plan N queries`. (`unjoined_alias_is_an_error`, `relation_name_clash_is_an_error` and `variable_name_clash_is_an_error` fail the same way; `every_rule_parses_as_datalog` too.)

- [ ] **Step 4: Implement `plan_template`.** Edit `kermit-generate/src/job/translate.rs`: no `cargo fmt` has run since Step 2, so the old string is exact. Old:

```rust
pub fn plan_template(queries: &[JobQuery]) -> Result<TemplatePlan, TranslateError> {
    todo!("plan {} queries", queries.len())
}
```

  new:

```rust
pub fn plan_template(queries: &[JobQuery]) -> Result<TemplatePlan, TranslateError> {
    let mut drafts: Vec<Draft> = Vec::new();
    let mut sketches: Vec<Sketch> = Vec::new();
    for query in queries {
        sketches.push(plan_query(query, &mut drafts)?);
    }
    // Names need every user of a relation, so they come last.
    let names: Vec<String> = drafts.iter().map(relation_name).collect();
    if let Some(name) = first_duplicate(&names) {
        return Err(TranslateError::NameClash { name: name.clone() });
    }
    let rules: Vec<RulePlan> = sketches
        .into_iter()
        .map(|sketch| {
            let body: Vec<String> = sketch
                .atoms
                .iter()
                .map(|(relation, terms)| format!("{}({})", names[*relation], terms.join(", ")))
                .collect();
            RulePlan {
                query: sketch.query,
                datalog: format!("Q({}) :- {}.", sketch.head.join(", "), body.join(", ")),
                count_sql: sketch.count_sql,
            }
        })
        .collect();
    let relations = drafts
        .into_iter()
        .zip(names)
        .map(|(draft, name)| RelationPlan {
            name,
            table: draft.table,
            columns: draft.columns,
            join_columns: draft.join_columns,
            filter: draft.filter,
            queries: draft.queries,
        })
        .collect();
    Ok(TemplatePlan { relations, rules })
}

/// The column every table keys its rows on, carried when it is not joined.
const ID: &str = "id";

/// The term of a carried `id`.
const PLACEHOLDER: &str = "_";

/// A relation file under construction: its sharing key (`table`, `columns`,
/// `filter`) and who uses it.
struct Draft {
    table: String,
    columns: Vec<String>,
    join_columns: usize,
    filter: String,
    /// The alias of the first query's first use of the key; it names a
    /// filtered relation.
    first_alias: String,
    queries: Vec<String>,
}

/// One query's rule before the relations have their names.
struct Sketch {
    query: String,
    count_sql: String,
    /// The variable names in position order.
    head: Vec<String>,
    /// One atom per alias in `FROM` order: the relation's index in the
    /// drafts, and its terms.
    atoms: Vec<(usize, Vec<String>)>,
}

/// The join variables of one query.
struct Variables {
    /// Every joined column once, in the order the joins first mention it,
    /// with the position of its variable.
    columns: Vec<(ColumnRef, usize)>,
    /// The variables' names, by position.
    names: Vec<String>,
}

impl Variables {
    fn of(query: &JobQuery) -> Variables {
        // Number the joined columns by first mention.
        let mut columns: Vec<&ColumnRef> = Vec::new();
        let mut pairs: Vec<(usize, usize)> = Vec::new();
        for (left, right) in &query.joins {
            let left = intern(&mut columns, left);
            let right = intern(&mut columns, right);
            pairs.push((left, right));
        }
        // Union-find in its plainest form: a join merges the labels of its two
        // columns, so equal labels are one variable.
        let mut label: Vec<usize> = (0..columns.len()).collect();
        for &(left, right) in &pairs {
            let (kept, merged) = (label[left], label[right]);
            for other in &mut label {
                if *other == merged {
                    *other = kept;
                }
            }
        }
        // A variable's position is the rank of the first join that mentions it.
        let mut order: Vec<usize> = Vec::new();
        let mut position = vec![0; columns.len()];
        for &(left, right) in &pairs {
            let rank = intern(&mut order, label[left]);
            position[left] = rank;
            position[right] = rank;
        }
        // A variable is named after the first `id` among its columns, else
        // after its first column.
        let mut names = vec![String::new(); order.len()];
        let mut named_by_id = vec![false; order.len()];
        for (column, &rank) in columns.iter().zip(&position) {
            if column.column == ID && !named_by_id[rank] {
                names[rank] = column.alias.to_uppercase();
                named_by_id[rank] = true;
            } else if names[rank].is_empty() {
                names[rank] = format!("{}_{}", column.alias, column.column).to_uppercase();
            }
        }
        Variables {
            columns: columns.into_iter().cloned().zip(position).collect(),
            names,
        }
    }

    /// The join columns of `alias` with their variables' positions, ordered by
    /// position; two columns in one variable stay in the order of first
    /// mention.
    fn of_alias(&self, alias: &str) -> Vec<(&str, usize)> {
        let mut joined: Vec<(&str, usize)> = self
            .columns
            .iter()
            .filter(|(column, _)| column.alias == alias)
            .map(|(column, rank)| (column.column.as_str(), *rank))
            .collect();
        joined.sort_by_key(|&(_, rank)| rank);
        joined
    }

    fn check_names(&self) -> Result<(), TranslateError> {
        match first_duplicate(&self.names) {
            | Some(name) => Err(TranslateError::NameClash { name: name.clone() }),
            | None => Ok(()),
        }
    }
}

/// The index of `item` in `seen`, appending it first when it is new.
fn intern<T: PartialEq>(seen: &mut Vec<T>, item: T) -> usize {
    let found = seen.iter().position(|other| *other == item);
    match found {
        | Some(index) => index,
        | None => {
            seen.push(item);
            seen.len() - 1
        },
    }
}

/// The first name that an earlier entry already has.
fn first_duplicate(names: &[String]) -> Option<&String> {
    names
        .iter()
        .enumerate()
        .find(|&(index, name)| names[..index].contains(name))
        .map(|(_, name)| name)
}

/// Plans one query: its variables, and for each alias the relation it reads
/// (found among `drafts` or added to them).
fn plan_query(query: &JobQuery, drafts: &mut Vec<Draft>) -> Result<Sketch, TranslateError> {
    let variables = Variables::of(query);
    if let Some(alias) = query
        .aliases
        .iter()
        .find(|alias| variables.of_alias(&alias.name).is_empty())
    {
        return Err(TranslateError::UnjoinedAlias {
            query: query.name.clone(),
            alias: alias.name.clone(),
        });
    }
    variables.check_names()?;
    let mut atoms = Vec::new();
    for alias in &query.aliases {
        let joined = variables.of_alias(&alias.name);
        let mut columns: Vec<String> =
            joined.iter().map(|&(column, _)| column.to_owned()).collect();
        let mut terms: Vec<String> =
            joined.iter().map(|&(_, rank)| variables.names[rank].clone()).collect();
        if !columns.iter().any(|column| column == ID) {
            columns.push(ID.to_owned());
            terms.push(PLACEHOLDER.to_owned());
        }
        let filter = query
            .filters
            .iter()
            .filter(|(owner, _)| *owner == alias.name)
            .map(|(_, text)| text.as_str())
            .collect::<Vec<_>>()
            .join(" AND ");
        let found = drafts.iter().position(|draft| {
            draft.table == alias.table && draft.columns == columns && draft.filter == filter
        });
        let index = match found {
            | Some(index) => index,
            | None => {
                drafts.push(Draft {
                    table: alias.table.clone(),
                    columns,
                    join_columns: joined.len(),
                    filter,
                    first_alias: alias.name.clone(),
                    queries: Vec::new(),
                });
                drafts.len() - 1
            },
        };
        if drafts[index].queries.last() != Some(&query.name) {
            drafts[index].queries.push(query.name.clone());
        }
        atoms.push((index, terms));
    }
    Ok(Sketch {
        query: query.name.clone(),
        count_sql: query.count_sql.clone(),
        head: variables.names,
        atoms,
    })
}

/// A relation's file name: the table for an unfiltered one, else
/// `{table}_{alias}_{letters}` with the variant letters (`6a` gives `a`) of
/// every query that uses it.
fn relation_name(draft: &Draft) -> String {
    if draft.filter.is_empty() {
        return draft.table.clone();
    }
    let letters: String = draft
        .queries
        .iter()
        .map(|query| query.trim_start_matches(|c: char| c.is_ascii_digit()))
        .collect();
    format!("{}_{}_{}", draft.table, draft.first_alias, letters)
}
```

- [ ] **Step 5: Run the tests. All 30 pass.**
  ```bash
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-generate job::translate::tests
  ```
  Expected: `test result: ok. 30 passed; 0 failed`. A mismatch in a `described` line or a rule is either a real planner defect or a wrong expectation: re-derive the expected string by hand from the rules in `plan_template`'s doc comment (the derivation for template 6 is in `template_6`'s doc comment) before changing either side.

- [ ] **Step 6: Format, lint, document, run the crate, commit.**
  ```bash
  nix develop --command cargo fmt --all
  git status --short
  ```
  Expected: only `kermit-generate/src/job/translate.rs` is modified.
  ```bash
  nix develop --command env CARGO_BUILD_JOBS=2 RUSTFLAGS=-Dwarnings cargo clippy -p kermit-generate --all-targets
  nix develop --command env CARGO_BUILD_JOBS=2 RUSTDOCFLAGS=-Dwarnings cargo doc -p kermit-generate --no-deps
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-generate
  ```
  Expected: no warnings; every `kermit-generate` test passes. Then commit by pathspec:
  ```bash
  git commit -m "feat(job): plan a template's relations and rules (#97)" \
    -m "plan_template merges each query's joins into variables ordered by first mention, gives each alias a relation of its join columns plus a carried id, shares identical relations across the template's variants by (table, columns, filter), and writes one Datalog rule per query. Template 6 comes out as nine files." \
    --trailer "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>" \
    --trailer "Claude-Session: https://claude.ai/code/session_01WeRnvdstSxUY8CJGSx1NFB" \
    -- kermit-generate/src/job/translate.rs
  ```

- [ ] **Step 7: Mutation checks.** Same procedure as J1 Step 7 (commit is in; apply the exact edit; `git diff --stat` shows it; run `nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-generate job::translate::tests`, see the named tests fail; reverse the exact edit; green; `git status --short` empty).

  | # | Mutant | Must fail |
  | --- | --- | --- |
  | 1 | in `plan_query`'s `position` closure, delete the `&& draft.columns == columns` conjunct | `shared_relation_keeps_column_order_in_its_key`, `relation_name_clash_is_an_error` |
  | 2 | in `relation_name`, add `.take(1)` after the `.iter()` line (after `cargo fmt` the chain `draft.queries.iter().map(…)` is split across lines) | `template_6_shapes`, `filtered_name_lists_variant_letters`, `renamed_aliases_still_share` |
  | 3 | in `plan_query`, change `if !columns.iter().any(\|column\| column == ID) {` to `if false {` | `id_is_appended_only_when_not_joined`, `template_6_shapes`, `fallback_variable_name` |
  | 4 | in `Variables::of`, replace `if column.column == ID && !named_by_id[rank] {` with `if false {` | `template_6_shapes`, `variable_named_after_id_alias`, `renamed_aliases_still_share` |

---

# Part 2: DuckDB, the raw cache and the fixture (J3, J4)

### Task J3: `pkgs.duckdb` in the flake and `duckdb.rs`

`Duckdb` is the one place kermit starts a `duckdb` process. Every call prefixes the same
session settings (`memory_limit = '4GB'`, a `temp_directory` inside the raw cache,
`preserve_insertion_order = true`, `enable_progress_bar = false`), so DuckDB's joins, sorts and aggregates spill to disk
past the limit instead of claiming most of the host's RAM beside a timing run.
`stream_rows` turns `COPY (…) TO '/dev/stdout'` into `Tuples` batches of `BATCH_ROWS` rows:
the filter stage (J5) hands each batch to a `RelationWriter`, so memory is one batch in
kermit and at most `MEMORY_LIMIT` in DuckDB. Nothing here knows about JOB's tables; J4's
`raw.rs` and J5's generator are the callers.

- **Decision 1: the env override is testable without touching the environment.**
  `locate()` is `locate_from(std::env::var_os(DUCKDB_BIN_ENV))`; the private `locate_from`
  takes the value, so `locate_honours_the_env_override` passes it directly instead of
  mutating process-global state while other tests run.
- **Decision 2: one method beyond the contract, `query_strings`.** J4's `build` must load
  "exactly the tables `schema.sql` creates, read back from `information_schema.tables`", and
  must name a table that lacks its CSV or its pinned count. Table names are text, and
  `query_u64` / `stream_rows` carry integers only, so `query_strings` returns the lines of a
  one-column read-only query. It is the only addition to the contract's `Duckdb` API.
- **Decision 3: the spill directory is created by kermit.** `command()` runs
  `create_dir_all(temp_dir)` before every call, so no call depends on DuckDB creating a
  missing (or nested) `temp_directory`.
- **Decision 4: two helpers are `pub(crate)`.** `sql_literal` / `path_literal` quote a string
  or path as a single-quoted SQL literal (`'` doubled); J4's `COPY` statements reuse them.
- **The quoting of `ESCAPE`.** DuckDB string literals have no backslash escapes, so the
  statement the CLI must receive is `ESCAPE '\'`: one backslash. In a Rust string literal
  that is `'\\'`; in a bash double-quoted string it is also `'\\'`; the contract's
  `ESCAPE '\\'` is that bash or Rust spelling, not the bytes DuckDB sees.
- **Premises this task checks before relying on them** (Step 3): that the CLI prints
  nothing after `SET` statements, that `COPY … TO '/dev/stdout'` is followed by no extra
  `Count` row on stdout (`-csv -noheader`), that `SET temp_directory` and
  `SET enable_progress_bar` are accepted under `-readonly` (the progress bar, which the CLI may
  draw after about two seconds, would otherwise land in the row stream on stdout or fail
  `stream_rows`'s empty-stderr check), and that a NULL prints as an empty line. If the second does not hold, the
  step says to stop: `stream_rows`'s line parser cannot tell a trailing count from a
  one-column row, and the fix is a design decision.

**Needs:** phase 1's upstream gate (#95 push 1 merged into `aidanb/job`); J1 (`job/mod.rs` exists with `pub mod translate;`).

**Files:**
- Modify: `flake.nix` (the `buildInputs` list, after `pkgs.jdk8`)
- Create: `kermit-generate/src/job/duckdb.rs`
- Modify: `kermit-generate/src/job/mod.rs` (one `pub mod duckdb;` line)
- Test: `kermit-generate/src/job/duckdb.rs` (`mod tests`)

- [ ] **Step 1: Check the host and the upstream gate.**
  ```bash
  find /proc/[0-9]*/exe -maxdepth 0 -lname '*kermit-bench-runs*' 2>/dev/null | wc -l
  ls kermit-generate/src/job/mod.rs kermit-generate/Cargo.toml
  grep -n "pub mod" kermit-generate/src/job/mod.rs kermit-generate/src/lib.rs
  ```
  Expected: `0` (and the co-ordinator has announced `FULL_EXIT`), both files listed,
  `pub mod translate;` in `job/mod.rs` and `pub mod job;` in `lib.rs`. If `kermit-generate`
  is missing, #95 push 1 is not merged: stop.

- [ ] **Step 2: Add `pkgs.duckdb` to the dev shell.**

  Edit `flake.nix` (the list items are indented 12 spaces in the file; the blocks below are
  shown indented 14 by this list). Before:
  ```nix
              pkgs.jdk8
              pkgs.python313
  ```
  After:
  ```nix
              pkgs.jdk8
              # DuckDB runs the JOB generator's filter and count stages; required by
              # `kermit bench gen job` and the DuckDB-gated tests (kermit-generate's
              # job module, kermit/tests/job_generate.rs).
              pkgs.duckdb
              pkgs.python313
  ```
  `flake.lock` does not change: `pkgs.duckdb` comes from the already-locked nixpkgs. Check the
  shell sees it (the first `nix develop` after the edit may fetch DuckDB from the binary cache):
  ```bash
  nix develop --command duckdb --version
  ```
  Expected: `v1.4.4 (Andium) 6ddac802ff`. A different version means the lock moved since the
  contract was written (2026-10-15): continue, but say so in the commit message and in the
  hand-off to the design session, because the error-text assertions below were written for
  1.4.4.

- [ ] **Step 3: Probe the invocations `duckdb.rs` is about to build.**

  This runs DuckDB on a three-row scratch table (no IMDb data), so the host rule allows it
  once Step 1 printed `0`. Create the script in the scratchpad (not in the repo):
  `$SCRATCH/scripts/ddb_j3_probe.sh`:
  ```bash
  #!/usr/bin/env bash
  # Probes the exact command lines duckdb.rs builds. Usage: ddb_j3_probe.sh <new empty dir>
  set -u
  cd "$1" || exit 2
  P="SET memory_limit='4GB'; SET temp_directory='$1/tmp'; SET preserve_insertion_order=true; SET enable_progress_bar=false;"
  duckdb scratch.duckdb -csv -noheader -c "$P CREATE TABLE t AS SELECT range + 1 AS a FROM range(3);" > /dev/null
  echo "create exit $?"
  echo "--- COUNT"; duckdb -readonly scratch.duckdb -csv -noheader -c "$P SELECT COUNT(*) FROM t;" | cat -A
  echo "--- COPY"; duckdb -readonly scratch.duckdb -csv -noheader -c "$P COPY (SELECT a FROM t ORDER BY a) TO '/dev/stdout' (FORMAT csv, HEADER false)" | cat -A
  echo "--- NULL"; duckdb -readonly scratch.duckdb -csv -noheader -c "$P COPY (SELECT CAST(NULL AS BIGINT) AS a) TO '/dev/stdout' (FORMAT csv, HEADER false)" | cat -A
  echo "--- NAMES"; duckdb -readonly scratch.duckdb -csv -noheader -c "$P SELECT table_name FROM information_schema.tables WHERE table_schema = 'main' AND table_type = 'BASE TABLE' ORDER BY table_name" | cat -A
  ```
  Run it in a fresh directory:
  ```bash
  mkdir -p "$SCRATCH/j3-probe" && nix develop --command bash "$SCRATCH/scripts/ddb_j3_probe.sh" "$SCRATCH/j3-probe"
  ```
  Expected, exactly (`cat -A` prints `$` at each line end):
  ```text
  create exit 0
  --- COUNT
  3$
  --- COPY
  1$
  2$
  3$
  --- NULL
  $
  --- NAMES
  t$
  ```
  Any extra line (for example a `3$` after the COPY rows: DuckDB's own `Count` result),
  any error text, or a different exit code means a premise above is false: **stop and
  report the raw output to the design session**. Do not invent a workaround.

- [ ] **Step 4: Write the failing tests.**

  In `kermit-generate/src/job/mod.rs`, add the module (rustfmt keeps `pub mod`s sorted, so it
  lands before `translate`). Before:
  ```rust
  pub mod translate;
  ```
  After:
  ```rust
  pub mod duckdb;
  pub mod translate;
  ```
  Create `kermit-generate/src/job/duckdb.rs` with the module doc, the `use` block and the
  tests only; Step 6 adds the implementation between the `use` block and `test_support`:

```rust
//! The DuckDB command-line client that the JOB generator drives.
//!
//! Every call starts one `duckdb` process with the same session settings: a
//! [`MEMORY_LIMIT`] and a spill directory, so DuckDB's joins, sorts and
//! aggregates go to disk past the limit instead of claiming most of the
//! host's RAM beside a timing run, `preserve_insertion_order` so that an
//! `ORDER BY` result is not reshuffled on its way out, and no progress bar
//! (which the CLI may draw after about two seconds, into the row stream or
//! into stderr). [`Duckdb::stream_rows`]
//! turns a `SELECT` into [`Tuples`] batches and never holds more than one.

use {
    anyhow::{anyhow, ensure, Context},
    kermit_ds::Tuples,
    std::{
        ffi::OsString,
        fs,
        io::{BufRead, BufReader, Read},
        path::{Path, PathBuf},
        process::{Command, Stdio},
        thread,
    },
};

#[cfg(test)]
pub(crate) mod test_support {
    use super::Duckdb;

    /// The located DuckDB, or `None` after a stderr line saying why: the
    /// DuckDB-gated tests skip, as the LUBM tests skip without `java`.
    pub(crate) fn locate_or_skip() -> Option<Duckdb> {
        match Duckdb::locate() {
            Ok(duckdb) => Some(duckdb),
            Err(error) => {
                eprintln!("skipping: no DuckDB ({error:#})");
                None
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use {
        super::{test_support::locate_or_skip, *},
        std::io::Cursor,
    };

    /// A scratch database holding one empty table `t`, plus its spill directory.
    struct Scratch {
        _dir: tempfile::TempDir,
        database: PathBuf,
        temp: PathBuf,
    }

    impl Scratch {
        fn new(duckdb: &Duckdb) -> Scratch {
            let dir = tempfile::tempdir().unwrap();
            let database = dir.path().join("scratch.duckdb");
            let temp = dir.path().join("tmp");
            duckdb
                .execute(&database, &temp, "CREATE TABLE t (a INTEGER)")
                .unwrap();
            Scratch {
                _dir: dir,
                database,
                temp,
            }
        }
    }

    // ---- no DuckDB needed -------------------------------------------------

    #[test]
    fn sql_literal_doubles_quotes() {
        assert_eq!(sql_literal("plain"), "'plain'");
        assert_eq!(sql_literal("it's"), "'it''s'");
        assert_eq!(sql_literal(""), "''");
    }

    /// The env override names a binary that does not exist: the error says
    /// which variable and which path, so a mistyped override is not mistaken
    /// for a missing install.
    #[test]
    fn locate_honours_the_env_override() {
        let missing = "/nonexistent/kermit-test/duckdb";
        let error = Duckdb::locate_from(Some(OsString::from(missing))).unwrap_err();
        let message = format!("{error:#}");
        assert!(message.contains(missing), "{message}");
        assert!(message.contains(DUCKDB_BIN_ENV), "{message}");
    }

    #[test]
    fn read_batches_cuts_at_batch_rows() {
        let text: String = (0..70_000).map(|i| format!("{i},{}\n", i * 2)).collect();
        let mut sizes = Vec::new();
        let mut next = 0usize;
        let rows = read_batches(Cursor::new(text), 2, &mut |batch: &Tuples| {
            sizes.push(batch.len());
            for row in batch.rows() {
                assert_eq!(row, &[next, next * 2][..]);
                next += 1;
            }
            Ok(())
        })
        .unwrap();
        assert_eq!(sizes, [BATCH_ROWS, 70_000 - BATCH_ROWS]);
        assert_eq!((rows, next), (70_000, 70_000));
    }

    #[test]
    fn read_batches_empty_input_calls_nothing() {
        let mut calls = 0;
        let rows = read_batches(Cursor::new(""), 3, &mut |_: &Tuples| {
            calls += 1;
            Ok(())
        })
        .unwrap();
        assert_eq!((rows, calls), (0, 0));
    }

    /// A line that is not `arity` integers is an input failure naming its line
    /// number, and rows before it in an unfinished batch are never handed off.
    #[test]
    fn read_batches_names_the_bad_line() {
        for (text, arity) in [("1,2\n3,x\n", 2), ("1,2\n3\n", 2), ("1\n\n", 1), ("1,2,3\n", 2)] {
            let mut calls = 0;
            let failure = read_batches(Cursor::new(text), arity, &mut |_: &Tuples| {
                calls += 1;
                Ok(())
            })
            .unwrap_err();
            assert!(
                matches!(&failure, StreamFailure::Input(error)
                    if error.to_string().contains("line ")),
                "{text:?}: {failure:?}"
            );
            assert_eq!(calls, 0, "{text:?}");
        }
        let failure = read_batches(Cursor::new("1,2\n3,x\n"), 2, &mut |_: &Tuples| Ok(()))
            .unwrap_err();
        assert!(
            matches!(&failure, StreamFailure::Input(error) if error.to_string().contains("line 2")),
            "{failure:?}"
        );
    }

    /// The sink's own error comes back as the sink's, unwrapped, so the caller
    /// can tell a refused batch from malformed input.
    #[test]
    fn read_batches_reports_a_sink_error_as_the_sinks() {
        let failure = read_batches(Cursor::new("1\n2\n"), 1, &mut |_: &Tuples| {
            Err(anyhow!("sink refused"))
        })
        .unwrap_err();
        assert!(
            matches!(&failure, StreamFailure::Sink(error) if error.to_string() == "sink refused"),
            "{failure:?}"
        );
    }

    // ---- DuckDB-gated ----------------------------------------------------

    /// 70,000 rows arrive as one full batch and one remainder, in `ORDER BY`
    /// order, and the call returns the row count.
    #[test]
    fn stream_rows_batches_and_orders() {
        let Some(duckdb) = locate_or_skip() else { return };
        let scratch = Scratch::new(&duckdb);
        let mut sizes = Vec::new();
        let mut next = 0usize;
        let rows = duckdb
            .stream_rows(
                &scratch.database,
                &scratch.temp,
                "SELECT range AS a, range * 2 AS b FROM range(70000) ORDER BY a",
                2,
                &mut |batch: &Tuples| {
                    sizes.push(batch.len());
                    for row in batch.rows() {
                        assert_eq!(row, &[next, next * 2][..]);
                        next += 1;
                    }
                    Ok(())
                },
            )
            .unwrap();
        assert_eq!(sizes, [65_536, 4_464]);
        assert_eq!((rows, next), (70_000, 70_000));
    }

    /// An empty result never calls the sink: the row count would otherwise
    /// include a stray line (DuckDB's own `Count` row would show up here).
    #[test]
    fn stream_rows_empty_result_calls_nothing() {
        let Some(duckdb) = locate_or_skip() else { return };
        let scratch = Scratch::new(&duckdb);
        let mut calls = 0;
        let rows = duckdb
            .stream_rows(&scratch.database, &scratch.temp, "SELECT a FROM t", 1, &mut |_| {
                calls += 1;
                Ok(())
            })
            .unwrap();
        assert_eq!((rows, calls), (0, 0));
    }

    /// One column is the shape a unary relation has, and the one where a
    /// trailing count line could not be told from a row.
    #[test]
    fn stream_rows_one_column_reads_exactly_the_rows() {
        let Some(duckdb) = locate_or_skip() else { return };
        let scratch = Scratch::new(&duckdb);
        let mut seen = Vec::new();
        let rows = duckdb
            .stream_rows(
                &scratch.database,
                &scratch.temp,
                "SELECT range AS a FROM range(5) ORDER BY a",
                1,
                &mut |batch: &Tuples| {
                    seen.extend(batch.rows().map(|row| row[0]));
                    Ok(())
                },
            )
            .unwrap();
        assert_eq!(seen, [0, 1, 2, 3, 4]);
        assert_eq!(rows, 5);
    }

    /// A sink that fails stops the stream at its first batch: the error is the
    /// sink's, unwrapped, and the sink is called once. (`stream_rows` kills
    /// DuckDB instead of waiting for the 5,000,000 rows; the test does not
    /// time that.)
    #[test]
    fn stream_rows_stops_when_the_sink_fails() {
        let Some(duckdb) = locate_or_skip() else { return };
        let scratch = Scratch::new(&duckdb);
        let mut calls = 0;
        let error = duckdb
            .stream_rows(
                &scratch.database,
                &scratch.temp,
                "SELECT range AS a FROM range(5000000) ORDER BY a",
                1,
                &mut |_| {
                    calls += 1;
                    Err(anyhow!("sink refused"))
                },
            )
            .unwrap_err();
        assert_eq!(error.to_string(), "sink refused");
        assert_eq!(calls, 1);
    }

    /// A failing statement is an error that quotes DuckDB's stderr, from every
    /// entry point, and a stream that failed never reached its sink.
    #[test]
    fn sql_error_quotes_stderr() {
        let Some(duckdb) = locate_or_skip() else { return };
        let scratch = Scratch::new(&duckdb);
        let error = duckdb
            .query_u64(&scratch.database, &scratch.temp, "SELECT COUNT(*) FROM no_such_table")
            .unwrap_err();
        let message = format!("{error:#}");
        assert!(message.contains("Catalog Error"), "{message}");
        assert!(message.contains("no_such_table"), "{message}");

        let mut calls = 0;
        let error = duckdb
            .stream_rows(&scratch.database, &scratch.temp, "SELECT a FROM no_such_table", 1, &mut |_| {
                calls += 1;
                Ok(())
            })
            .unwrap_err();
        assert!(format!("{error:#}").contains("Catalog Error"), "{error:#}");
        assert_eq!(calls, 0);
    }

    /// `query_u64` and `stream_rows` open the database read-only: a write is
    /// refused and the table is unchanged.
    #[test]
    fn readonly_refuses_writes() {
        let Some(duckdb) = locate_or_skip() else { return };
        let scratch = Scratch::new(&duckdb);
        let refused = duckdb.query_u64(&scratch.database, &scratch.temp, "INSERT INTO t VALUES (1)");
        assert!(refused.is_err());
        let rows = duckdb
            .query_u64(&scratch.database, &scratch.temp, "SELECT COUNT(*) FROM t")
            .unwrap();
        assert_eq!(rows, 0);
    }

    #[test]
    fn query_u64_reads_a_count() {
        let Some(duckdb) = locate_or_skip() else { return };
        let scratch = Scratch::new(&duckdb);
        duckdb
            .execute(&scratch.database, &scratch.temp, "INSERT INTO t SELECT range FROM range(7)")
            .unwrap();
        let rows = duckdb
            .query_u64(&scratch.database, &scratch.temp, "SELECT COUNT(*) FROM t")
            .unwrap();
        assert_eq!(rows, 7);
        // Text, or a negative number, is an error and not a zero.
        let text = duckdb.query_u64(&scratch.database, &scratch.temp, "SELECT 'seven'");
        assert!(format!("{:#}", text.unwrap_err()).contains("seven"));
        assert!(duckdb.query_u64(&scratch.database, &scratch.temp, "SELECT -1").is_err());
    }

    #[test]
    fn query_strings_reads_a_text_column() {
        let Some(duckdb) = locate_or_skip() else { return };
        let scratch = Scratch::new(&duckdb);
        duckdb
            .execute(&scratch.database, &scratch.temp, "CREATE TABLE u (b INTEGER)")
            .unwrap();
        let names = duckdb
            .query_strings(
                &scratch.database,
                &scratch.temp,
                "SELECT table_name FROM information_schema.tables \
                 WHERE table_schema = 'main' AND table_type = 'BASE TABLE' ORDER BY table_name",
            )
            .unwrap();
        assert_eq!(names, ["t", "u"]);
    }
}
```

- [ ] **Step 5: Run the tests; expect a compile failure.**
  ```bash
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-generate job::duckdb
  ```
  Expected: FAIL to compile: `cannot find type `Duckdb` in this scope`, `cannot find function
  `read_batches``, `cannot find value `BATCH_ROWS``, `cannot find function `sql_literal``
  and `cannot find type `StreamFailure``. The exact wording and the number of errors may
  differ: for example, `use super::Duckdb;` in `test_support` is reported as E0432
  (`unresolved import`), not as a missing type. Any compile failure that names these items is
  the expected one.

- [ ] **Step 6: Implement `duckdb.rs`.**

  Insert this block between the `use` block and `#[cfg(test)] pub(crate) mod test_support`:

```rust
/// The environment variable that names the DuckDB binary. When it is unset or
/// empty, `duckdb` is looked up on `PATH`.
pub const DUCKDB_BIN_ENV: &str = "KERMIT_DUCKDB_BIN";

/// DuckDB's `memory_limit` in every session. Past it, DuckDB's join, sort and
/// aggregate operators spill to the session's `temp_directory`.
pub const MEMORY_LIMIT: &str = "4GB";

/// Rows per [`Tuples`] batch that [`Duckdb::stream_rows`] hands to its sink
/// (the last batch of a stream is shorter).
pub const BATCH_ROWS: usize = 1 << 16;

/// `text` as a single-quoted SQL string literal (`'` doubled).
pub(crate) fn sql_literal(text: &str) -> String { format!("'{}'", text.replace('\'', "''")) }

/// `path` as a SQL string literal. DuckDB receives the statement as text, so
/// the path must be valid UTF-8.
pub(crate) fn path_literal(path: &Path) -> anyhow::Result<String> {
    let text = path
        .to_str()
        .with_context(|| format!("path {} is not valid UTF-8", path.display()))?;
    Ok(sql_literal(text))
}

/// A located DuckDB CLI.
#[derive(Debug, Clone)]
pub struct Duckdb {
    bin: PathBuf,
    version: String,
}

impl Duckdb {
    /// Finds the DuckDB CLI: the binary `$KERMIT_DUCKDB_BIN` names, else
    /// `duckdb` on `PATH`, and runs it with `--version` to be sure it works.
    ///
    /// # Errors
    ///
    /// Fails when the binary is missing or does not run; the message says to
    /// enter `nix develop` or to set `KERMIT_DUCKDB_BIN`.
    pub fn locate() -> anyhow::Result<Duckdb> {
        Self::locate_from(std::env::var_os(DUCKDB_BIN_ENV))
    }

    /// [`locate`](Self::locate) with the environment variable's value passed in, so a
    /// test can exercise the override without touching the process environment.
    fn locate_from(env: Option<OsString>) -> anyhow::Result<Duckdb> {
        match env.filter(|value| !value.is_empty()) {
            Some(bin) => {
                let bin = PathBuf::from(bin);
                Self::at(bin.clone()).with_context(|| {
                    format!("`{DUCKDB_BIN_ENV}` is set to {}, which cannot be run", bin.display())
                })
            },
            None => Self::at(PathBuf::from("duckdb")).with_context(|| {
                format!(
                    "no usable `duckdb` on PATH: enter `nix develop` (the flake provides it) or \
                     set `{DUCKDB_BIN_ENV}` to a DuckDB CLI"
                )
            }),
        }
    }

    /// Uses the DuckDB CLI at `bin`, after running `bin --version`.
    ///
    /// # Errors
    ///
    /// Fails when `bin` cannot be run or `--version` exits non-zero or prints
    /// nothing.
    pub fn at(bin: PathBuf) -> anyhow::Result<Duckdb> {
        let output = Command::new(&bin)
            .arg("--version")
            .stdin(Stdio::null())
            .output()
            .with_context(|| format!("running `{} --version`", bin.display()))?;
        ensure!(
            output.status.success(),
            "`{} --version` exited with {}",
            bin.display(),
            output.status
        );
        let stdout = String::from_utf8_lossy(&output.stdout);
        let version = stdout.lines().next().unwrap_or("").trim().to_owned();
        ensure!(!version.is_empty(), "`{} --version` printed nothing", bin.display());
        Ok(Duckdb { bin, version })
    }

    /// The first line of `duckdb --version`, trimmed (`v1.4.4 (Andium) 6ddac802ff`).
    pub fn version(&self) -> &str { &self.version }

    /// Runs `sql` against `database` read-write, creating the file if it is
    /// missing. This is the raw-cache build's call; everything else is
    /// read-only.
    ///
    /// # Errors
    ///
    /// Fails if DuckDB cannot start or exits non-zero; the message quotes its
    /// stderr.
    pub fn execute(&self, database: &Path, temp_dir: &Path, sql: &str) -> anyhow::Result<()> {
        self.capture(self.command(database, temp_dir, false, sql)?, database)?;
        Ok(())
    }

    /// Runs a read-only query that returns one non-negative integer, such as a
    /// `COUNT(*)`.
    ///
    /// # Errors
    ///
    /// Fails if DuckDB fails, or if its output is not exactly one non-negative
    /// integer.
    pub fn query_u64(&self, database: &Path, temp_dir: &Path, sql: &str) -> anyhow::Result<u64> {
        let stdout = self.capture(self.command(database, temp_dir, true, sql)?, database)?;
        let text = stdout.trim();
        text.parse::<u64>().with_context(|| {
            format!(
                "expected one non-negative integer from DuckDB on {}, got `{text}`",
                database.display()
            )
        })
    }

    /// Runs a read-only query with one column and returns one string per output
    /// line, exactly as DuckDB's CSV output prints it (no unquoting). For names
    /// and other plain text, such as `information_schema.tables`.
    ///
    /// # Errors
    ///
    /// Fails if DuckDB fails.
    pub fn query_strings(
        &self, database: &Path, temp_dir: &Path, sql: &str,
    ) -> anyhow::Result<Vec<String>> {
        let stdout = self.capture(self.command(database, temp_dir, true, sql)?, database)?;
        Ok(stdout.lines().map(str::to_owned).collect())
    }

    /// Streams `COPY ({select}) TO '/dev/stdout' (FORMAT csv, HEADER false)`
    /// read-only, parses each output line into `arity` integers and calls
    /// `on_batch` with batches of [`BATCH_ROWS`] rows (the last one shorter;
    /// never for an empty result). Returns the row count.
    ///
    /// Memory is one batch here and at most [`MEMORY_LIMIT`] in DuckDB. The
    /// rows must be non-negative integers (cast to `BIGINT` in `select`); a
    /// NULL prints as an empty field and is an error naming its line.
    ///
    /// # Errors
    ///
    /// Succeeds only if DuckDB exits 0 with an empty stderr; otherwise the
    /// error quotes stderr. A line that does not parse is an error naming its
    /// line number. If `on_batch` fails, DuckDB is killed and that error is
    /// returned unchanged. Batches handed off before an error stay handed
    /// off, so a caller must not treat a partial stream as complete.
    pub fn stream_rows(
        &self, database: &Path, temp_dir: &Path, select: &str, arity: usize,
        on_batch: &mut dyn FnMut(&Tuples) -> anyhow::Result<()>,
    ) -> anyhow::Result<u64> {
        ensure!(arity > 0, "stream_rows needs at least one column");
        let copy = format!("COPY ({select}) TO '/dev/stdout' (FORMAT csv, HEADER false)");
        let mut command = self.command(database, temp_dir, true, &copy)?;
        let mut child = command
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| format!("starting {}", self.bin.display()))?;
        let mut stderr = child.stderr.take().context("DuckDB's stderr was not piped")?;
        let stderr_reader = thread::spawn(move || {
            let mut bytes = Vec::new();
            // A read error leaves whatever arrived; the exit status still decides.
            // Bytes, not `read_to_string`: stderr that is not UTF-8 must not
            // lose the rest of the message.
            let _ = stderr.read_to_end(&mut bytes);
            String::from_utf8_lossy(&bytes).into_owned()
        });
        let stdout = child.stdout.take().context("DuckDB's stdout was not piped")?;
        match read_batches(BufReader::with_capacity(1 << 20, stdout), arity, on_batch) {
            Ok(rows) => {
                let status = child.wait().context("waiting for DuckDB")?;
                let stderr = stderr_reader.join().unwrap_or_default();
                ensure!(
                    status.success() && stderr.trim().is_empty(),
                    "DuckDB failed ({status}) streaming from {}:\n{}",
                    database.display(),
                    stderr.trim()
                );
                Ok(rows)
            },
            Err(failure) => {
                // Stop DuckDB before it blocks on a pipe nobody reads.
                let _ = child.kill();
                let _ = child.wait();
                let stderr = stderr_reader.join().unwrap_or_default();
                match failure {
                    StreamFailure::Sink(error) => Err(error),
                    StreamFailure::Input(error) if stderr.trim().is_empty() => Err(error),
                    StreamFailure::Input(error) => {
                        Err(error.context(format!("DuckDB reported:\n{}", stderr.trim())))
                    },
                }
            },
        }
    }

    /// The command that runs `sql` against `database` under the session
    /// settings: `duckdb [-readonly] <database> -csv -noheader -c "<settings> <sql>"`.
    fn command(
        &self, database: &Path, temp_dir: &Path, read_only: bool, sql: &str,
    ) -> anyhow::Result<Command> {
        fs::create_dir_all(temp_dir)
            .with_context(|| format!("creating DuckDB's spill directory {}", temp_dir.display()))?;
        let script = format!(
            "SET memory_limit='{MEMORY_LIMIT}'; SET temp_directory={}; \
             SET preserve_insertion_order=true; SET enable_progress_bar=false; {sql}",
            path_literal(temp_dir)?
        );
        let mut command = Command::new(&self.bin);
        if read_only {
            command.arg("-readonly");
        }
        command
            .arg(database)
            .args(["-csv", "-noheader", "-c"])
            .arg(script)
            .stdin(Stdio::null());
        Ok(command)
    }

    /// Runs `command` to completion and returns its stdout, or an error that
    /// quotes its stderr when it exits non-zero.
    fn capture(&self, mut command: Command, database: &Path) -> anyhow::Result<String> {
        let output = command
            .output()
            .with_context(|| format!("running {}", self.bin.display()))?;
        ensure!(
            output.status.success(),
            "DuckDB failed ({}) on {}:\n{}",
            output.status,
            database.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }
}

/// Why `read_batches` stopped: the sink refused a batch, or the input was
/// not what `stream_rows` expects.
#[derive(Debug)]
enum StreamFailure {
    /// `on_batch` returned this error.
    Sink(anyhow::Error),
    /// DuckDB's output could not be read or parsed.
    Input(anyhow::Error),
}

/// Parses `lines` into `arity`-integer rows and hands them to `on_batch` in
/// batches of [`BATCH_ROWS`]; returns the row count. A fresh batch is
/// allocated after each hand-off, so the sink may keep nothing and the next
/// batch never aliases the last.
fn read_batches(
    mut lines: impl BufRead, arity: usize, on_batch: &mut dyn FnMut(&Tuples) -> anyhow::Result<()>,
) -> Result<u64, StreamFailure> {
    let mut batch = Tuples::with_capacity(arity, BATCH_ROWS);
    let mut row = Vec::with_capacity(arity);
    let mut line = String::new();
    let mut total = 0u64;
    loop {
        line.clear();
        let read = lines
            .read_line(&mut line)
            .map_err(|error| StreamFailure::Input(anyhow!(error).context("reading DuckDB's output")))?;
        if read == 0 {
            break;
        }
        total += 1;
        parse_row(line.trim_end_matches(['\n', '\r']), arity, total, &mut row)
            .map_err(StreamFailure::Input)?;
        batch.push(&row);
        if batch.len() == BATCH_ROWS {
            on_batch(&batch).map_err(StreamFailure::Sink)?;
            batch = Tuples::with_capacity(arity, BATCH_ROWS);
        }
    }
    if !batch.is_empty() {
        on_batch(&batch).map_err(StreamFailure::Sink)?;
    }
    Ok(total)
}

/// Parses one output line (`text`, number `line_number`) into `row`.
fn parse_row(
    text: &str, arity: usize, line_number: u64, row: &mut Vec<usize>,
) -> anyhow::Result<()> {
    row.clear();
    for field in text.split(',') {
        let value = field.parse::<usize>().map_err(|_| {
            anyhow!(
                "DuckDB output line {line_number}: `{text}` is not {arity} comma-separated \
                 non-negative integers (a NULL prints as an empty field)"
            )
        })?;
        row.push(value);
    }
    ensure!(
        row.len() == arity,
        "DuckDB output line {line_number}: `{text}` has {} fields, expected {arity}",
        row.len()
    );
    Ok(())
}
```

- [ ] **Step 7: Run the tests; expect PASS.**
  ```bash
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-generate job::duckdb
  ```
  Expected: `test result: ok. 14 passed; 0 failed` (6 need no DuckDB; 8 run for real inside
  `nix develop` and print `skipping: no DuckDB (…)` and pass vacuously without it). Confirm the
  gated ones ran: no `skipping` line on stderr (`-- --nocapture`).
  If `sql_error_quotes_stderr` fails only on the exact text `Catalog Error`, DuckDB's wording
  differs from 1.4.4's: change that one needle to what stderr shows, and say so in the commit
  message. Any other failure is a real bug.

- [ ] **Step 8: Format, lint, document.**
  ```bash
  nix develop --command cargo fmt --all
  nix develop --command env CARGO_BUILD_JOBS=2 RUSTFLAGS=-Dwarnings cargo clippy -p kermit-generate --all-targets
  nix develop --command env CARGO_BUILD_JOBS=2 RUSTDOCFLAGS=-Dwarnings cargo doc -p kermit-generate --no-deps
  ```
  Expected: all clean (`cargo fmt` may rewrap the code above; accept its output). A
  `private_intra_doc_links` or `doc_markdown` warning means a doc comment needs backticks or
  a plain-text name.

- [ ] **Step 9: Commit.** Stage the three files by name, then commit by pathspec (never a
  bare `git commit`, `-a` or `--amend`):

```bash
git add flake.nix kermit-generate/src/job/mod.rs kermit-generate/src/job/duckdb.rs
git commit -m "feat(job): drive the DuckDB CLI from the JOB generator (#97)" \
  -m "pkgs.duckdb joins the dev shell. Duckdb locates the CLI (KERMIT_DUCKDB_BIN, else PATH), runs every session under a 4 GB memory limit with a spill directory, and streams a SELECT as Tuples batches of 65,536 rows." \
  --trailer "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>" \
  --trailer "Claude-Session: https://claude.ai/code/session_01WeRnvdstSxUY8CJGSx1NFB" \
  -- flake.nix kermit-generate/src/job/mod.rs kermit-generate/src/job/duckdb.rs
```

  Expected: one commit touching exactly those three files.

### Task J4: `pins.rs`, `raw.rs`, the committed fixture and the IMDb measurements

The raw cache is `<cache>/kermit/imdb/`: `imdb.duckdb`, `queries/` (the pinned JOB files) and
`provenance.json`, shared by every template. `raw::build` loads it from a directory of CSVs
and the pinned `schema.sql`; `pins.rs` holds everything the build is checked against; the
fixture is a hand-written, IMDb-shaped miniature that J4's tests build a raw cache from and
that J5's pipeline test generates templates 1 and 2 from. The downloads are J7's.

Two parts, so every commit leaves the tree green:

- **Part A (Steps 1-14)** is code, fixture and tests. It runs in phase 1, with DuckDB on
  the tiny fixture only. `IMDB_SHA256` and the 21 counts of `IMDB_TABLE_ROWS` are placeholders
  (`"UNMEASURED"`, `0`) that nothing before J7 reads.
- **Part B (Steps 15-27)** measures the real dump and replaces the placeholders. It needs
  `FULL_EXIT`, the user's go-ahead for the 1.26 GB download, and about 5 GB of disk under
  `/tb/Source/Academia/kermit-bench-runs/job-97/` (disk, not `/tmp`, which is RAM-backed).
  J5 and J6 do not wait for it; **J7 does**, because `raw::ensure` verifies the download
  against `IMDB_SHA256`.

- **Decision 1: `build` accepts exactly the query files it can serve.** It copies every file
  in `queries_dir`; each must be a regular file named `<digits><lowercase letters>.sql` (for
  example `16b.sql`), and anything else is an error naming it. `template_queries(n)` reads the
  copy back, keeps the files whose digits equal `n` (so `1` never matches `10a.sql`) and sorts
  them by letters; a template with no files returns an empty vector, and J5 turns that into
  its error. J7 therefore stages the 113 query files alone in `queries_dir` and passes
  `schema.sql` separately.
- **Decision 2: a leftover staging directory is never reused.** `Staging::create` removes
  whatever a killed run left at `<dir>.building-<pid>` (a reused pid included) before the build
  starts, and `Staging`'s `Drop` removes the directory on every `?` return and on a panic.
  `SIGINT` and `SIGKILL` skip `Drop`, so a staging directory under a dead pid can remain; there
  is no automatic cleanup, and `JOB.md` (J8) tells the user to delete any `imdb.building-*` or
  `imdb.fetch-*` leftovers by hand. A build that loses the final `rename` to a concurrent one
  adopts the winner if `dir` now holds a finished cache (one with `provenance.json`), and
  fails otherwise.
- **Decision 3: the fixture's CSVs need a `.gitignore` exception.** The repository ignores
  `**/*.csv` and re-includes only `kermit/tests/fixtures/*.csv` and `benchmarks/data/**/*.csv`.
  Step 3 adds `!kermit-generate/tests/fixtures/job/csv/*.csv`. (This is a tracked file outside
  the contract's list; #95 or #96 may add a sibling line next to it, which is a trivial
  add/add merge.)
- **Decision 4: tests that need no DuckDB stay ungated.** The contract lists J4's tests as
  DuckDB-gated; those that only read a hand-made cache directory (`template_queries…`,
  `provenance_round_trips`, `open_needs_provenance_last`) and the `tar` and `pins` tests run in
  CI too.
- **Decision 5: two queries, `2a` and `2b`, form template 2.** `2b` was added at the
  co-ordinator's request: one of its filters selects no row, so one of its relations is empty
  and its `COUNT(*)` is 0, which J5 uses to check that an empty relation round-trips.
- **Decision 6: `Staging` is the one staging guard.** J7's download stage reuses it
  (`Staging::create(path)`, `.path()`, `Drop`) for `<dir>.fetch-<pid>`. `create` also creates
  the staging directory's parent, so a cache directory whose parent does not exist yet (a fresh
  `$XDG_CACHE_HOME/kermit/imdb`) works for both stages.

**Needs:** J3 committed (`Duckdb`, `test_support::locate_or_skip`, `path_literal`); J1 (`job/mod.rs`).
Part B also needs `FULL_EXIT` and the user's go-ahead.

**Files:**
- Modify: `.gitignore`, `kermit-generate/src/job/mod.rs`
- Create: `kermit-generate/src/job/pins.rs`, `kermit-generate/src/job/raw.rs`
- Create (fixture): `kermit-generate/tests/fixtures/job/schema.sql`,
  `csv/{title,kind_type,keyword,movie_keyword,name,cast_info,info_type,movie_info}.csv`,
  `queries/{1a,1b,2a,2b}.sql`, `rows.txt`
- Test: `pins.rs` and `raw.rs` (`mod tests`)

### Part A: code, fixture and tests

- [ ] **Step 1: Check the host and the base.**
  ```bash
  find /proc/[0-9]*/exe -maxdepth 0 -lname '*kermit-bench-runs*' 2>/dev/null | wc -l
  git log --oneline -3
  ls kermit-generate/src/job/
  ```
  Expected: `0`; the J3 commit on top; `duckdb.rs  mod.rs  translate.rs` (J1/J2 files may
  differ in number but `duckdb.rs` must be there).

- [ ] **Step 2: Create the fixture.** Every file below is created exactly as shown. The
  fixture is hand-written, not JOB's text (the JOB repository carries no licence): the column
  names are JOB's for the columns used, `cast_info.person_id` and `movie_info.info_type_id`
  are deliberately nullable (JOB's are `NOT NULL`), and the data is built so each filter in
  the queries removes at least one row. SQL comments avoid apostrophes on purpose.

`kermit-generate/tests/fixtures/job/schema.sql`:
```sql
-- Hand-written IMDb-shaped schema for the JOB generator tests. Column names follow
-- the Join Order Benchmark schema for the columns used here; nothing is copied.
CREATE TABLE title (
    id integer NOT NULL PRIMARY KEY,
    title text NOT NULL,
    kind_id integer NOT NULL,
    production_year integer
);

CREATE TABLE kind_type (
    id integer NOT NULL PRIMARY KEY,
    kind text NOT NULL
);

CREATE TABLE keyword (
    id integer NOT NULL PRIMARY KEY,
    keyword text NOT NULL
);

CREATE TABLE movie_keyword (
    id integer NOT NULL PRIMARY KEY,
    movie_id integer NOT NULL,
    keyword_id integer NOT NULL
);

CREATE TABLE name (
    id integer NOT NULL PRIMARY KEY,
    name text NOT NULL
);

-- person_id is nullable here (NOT NULL in JOB) so the fixture has a NULL join key.
CREATE TABLE cast_info (
    id integer NOT NULL PRIMARY KEY,
    person_id integer,
    movie_id integer NOT NULL
);

CREATE TABLE info_type (
    id integer NOT NULL PRIMARY KEY,
    info text NOT NULL
);

-- info_type_id is nullable here (NOT NULL in JOB), for the same reason.
CREATE TABLE movie_info (
    id integer NOT NULL PRIMARY KEY,
    movie_id integer NOT NULL,
    info_type_id integer,
    info text NOT NULL
);
```

`kermit-generate/tests/fixtures/job/csv/title.csv` (no header; `\"` is a backslash-escaped
quote inside a quoted field; row 5 has a comma in its title and a NULL year):
```csv
1,Hero Begins,1,1985
2,"The \"Hero\" Returns",1,1995
3,Quiet Street,1,1992
4,Villain Rising,2,2001
5,"Hero, Interrupted",1,
6,Old Western,2,1950
7,Sidekick Story,1,1990
8,Last Hero,3,2010
9,"\"Hero\" Rides Again",2,2003
```

`kermit-generate/tests/fixtures/job/csv/kind_type.csv`:
```csv
1,movie
2,tv series
3,video
```

`kermit-generate/tests/fixtures/job/csv/keyword.csv`:
```csv
1,hero
2,superhero
3,villain
4,sidekick
```

`kermit-generate/tests/fixtures/job/csv/movie_keyword.csv` (`id,movie_id,keyword_id`):
```csv
1,1,1
2,2,1
3,2,2
4,3,4
5,4,3
6,8,1
7,7,4
8,3,2
9,1,2
10,7,3
```

`kermit-generate/tests/fixtures/job/csv/name.csv`:
```csv
1,Ann Lee
2,Bob Ray
3,Cy Moss
4,Ray Dunn
5,Di Park
```

`kermit-generate/tests/fixtures/job/csv/cast_info.csv` (`id,person_id,movie_id`; row 3 has a NULL `person_id`):
```csv
1,2,2
2,4,2
3,,2
4,1,2
5,2,3
6,4,9
7,2,9
8,4,1
9,3,8
```

`kermit-generate/tests/fixtures/job/csv/info_type.csv`:
```csv
1,genres
2,rating
3,runtime
```

`kermit-generate/tests/fixtures/job/csv/movie_info.csv` (`id,movie_id,info_type_id,info`; row 14 has a NULL `info_type_id`):
```csv
1,1,1,Action
2,1,2,7.5
3,2,1,Drama
4,2,2,8.1
5,2,1,Comedy
6,3,1,Documentary
7,3,2,6.0
8,3,1,Drama
9,4,1,Action
10,4,2,5.5
11,7,1,Drama
12,7,2,7.0
13,8,1,Horror
14,8,,Action
15,2,2,7.9
16,1,3,95 min
```

`kermit-generate/tests/fixtures/job/rows.txt` (`<table> <rows>`, one per line; the tests read
it into `table_rows`):
```text
title 9
kind_type 3
keyword 4
movie_keyword 10
name 5
cast_info 9
info_type 3
movie_info 16
```

`kermit-generate/tests/fixtures/job/queries/1a.sql` (a self-join on `info_type`, five filters
of four kinds, a redundant join condition as JOB has):
```sql
SELECT MIN(t.title) AS movie_title,
       MIN(mi1.info) AS genre
FROM info_type AS it1,
     info_type AS it2,
     keyword AS k,
     movie_info AS mi1,
     movie_info AS mi2,
     movie_keyword AS mk,
     title AS t
WHERE it1.info = 'genres'
  AND it2.info = 'rating'
  AND k.keyword LIKE '%hero%'
  AND mi1.info IN ('Action', 'Drama')
  AND t.production_year > 1990
  AND it1.id = mi1.info_type_id
  AND it2.id = mi2.info_type_id
  AND t.id = mi1.movie_id
  AND t.id = mi2.movie_id
  AND t.id = mk.movie_id
  AND k.id = mk.keyword_id
  AND mi1.movie_id = mk.movie_id;
```

`kermit-generate/tests/fixtures/job/queries/1b.sql` (shares `it1`, `k` and `mk` with `1a`):
```sql
SELECT MIN(t.title) AS movie_title,
       MIN(mi1.info) AS genre
FROM info_type AS it1,
     keyword AS k,
     movie_info AS mi1,
     movie_keyword AS mk,
     title AS t
WHERE it1.info = 'genres'
  AND k.keyword LIKE '%hero%'
  AND mi1.info NOT LIKE 'Doc%'
  AND t.production_year BETWEEN 1980 AND 1995
  AND it1.id = mi1.info_type_id
  AND t.id = mi1.movie_id
  AND t.id = mk.movie_id
  AND k.id = mk.keyword_id;
```

`kermit-generate/tests/fixtures/job/queries/2a.sql` (the filter touches the rows whose titles
contain backslash-escaped quotes in the CSV; `title` joins on `id` and on `kind_id`):
```sql
SELECT MIN(t.title) AS movie_title,
       MIN(n.name) AS actor
FROM cast_info AS ci,
     kind_type AS kt,
     name AS n,
     title AS t
WHERE kt.kind = 'movie'
  AND n.name LIKE '%Ray%'
  AND t.title LIKE '%"Hero"%'
  AND ci.movie_id = t.id
  AND ci.person_id = n.id
  AND kt.id = t.kind_id;
```

`kermit-generate/tests/fixtures/job/queries/2b.sql` (`n.name LIKE '%Zed%'` matches no name,
so `name_n_b` is empty and the count is 0):
```sql
SELECT MIN(t.title) AS movie_title,
       MIN(n.name) AS actor
FROM cast_info AS ci,
     name AS n,
     title AS t
WHERE n.name LIKE '%Zed%'
  AND t.production_year > 1990
  AND ci.movie_id = t.id
  AND ci.person_id = n.id;
```

- [ ] **Step 3: Let git see the CSVs.**

  Edit `.gitignore`. Before:
  ```gitignore
  **/*.csv
  !kermit/tests/fixtures/*.csv
  ```
  After:
  ```gitignore
  **/*.csv
  !kermit/tests/fixtures/*.csv
  # The JOB generator's hand-written IMDb-shaped fixture (kermit-generate).
  !kermit-generate/tests/fixtures/job/csv/*.csv
  ```
  Check:
  ```bash
  git check-ignore -q kermit-generate/tests/fixtures/job/csv/title.csv; echo "exit $?"
  git status --short kermit-generate/tests/fixtures .gitignore
  ```
  Expected: `exit 1` (not ignored), then `.gitignore` modified and the fixture directory
  untracked. (`-q`, not `-v`: for a path that a `!` pattern re-includes, `git check-ignore -v`
  prints that pattern and still exits 0, which reads like "ignored".)

- [ ] **Step 4: Fixture facts (reference for J5; checked by the test of Step 9, which Step 12 runs).**

  The tables below are what `plan_template` (J2) must produce for the four fixture queries
  under the contract's rules, and what each relation holds. They were derived by hand from the
  CSVs above; `fixture_queries_count_as_the_facts_say` (Step 9) recomputes the four counts
  with DuckDB. If it disagrees, the table is wrong, not DuckDB: recompute by hand and fix both
  the table and that test.

  **Variables** (union-find over the join conditions; position = index of the first join
  mentioning a member, 0-based, in text order; the name comes from the first member whose
  column is `id`):

  | Query | Variables in position order |
  | --- | --- |
  | 1a | `IT1` (pos 0: `it1.id`, `mi1.info_type_id`), `IT2` (1: `it2.id`, `mi2.info_type_id`), `T` (2: `t.id`, `mi1.movie_id`, `mi2.movie_id`, `mk.movie_id`), `K` (5: `k.id`, `mk.keyword_id`) |
  | 1b | `IT1` (0), `T` (1: `t.id`, `mi1.movie_id`, `mk.movie_id`), `K` (3: `k.id`, `mk.keyword_id`) |
  | 2a | `T` (0: `ci.movie_id`, `t.id`), `N` (1: `ci.person_id`, `n.id`), `KT` (2: `kt.id`, `t.kind_id`) |
  | 2b | `T` (0), `N` (1) |

  **Rules** (`RulePlan.datalog`; `Q(` head = every variable in position order; body = one
  atom per alias in FROM order; `_` is a carried `id`):

  | Query | `datalog` |
  | --- | --- |
  | 1a | `Q(IT1, IT2, T, K) :- info_type_it1_ab(IT1), info_type_it2_a(IT2), keyword_k_ab(K), movie_info_mi1_a(IT1, T, _), movie_info(IT2, T, _), movie_keyword(T, K, _), title_t_a(T).` |
  | 1b | `Q(IT1, T, K) :- info_type_it1_ab(IT1), keyword_k_ab(K), movie_info_mi1_b(IT1, T, _), movie_keyword(T, K, _), title_t_b(T).` |
  | 2a | `Q(T, N, KT) :- cast_info(T, N, _), kind_type_kt_a(KT), name_n_a(N), title_t_a(T, KT).` |
  | 2b | `Q(T, N) :- cast_info(T, N, _), name_n_b(N), title_t_b(T).` |

  **Relations** (`TemplatePlan.relations`, in first-use order: query order, then FROM order;
  template 1 has 9, template 2 has 6; `jc` = `join_columns`). Every relation's file holds the
  source rows that pass `filter` and have no NULL in its first `jc` columns, as the listed
  rows, cast to integers, in `ORDER BY id` order (the last column or, when `id` leads, the
  first, is the source `id`):

  | Template | `name` | `table` | `columns` | `jc` | `filter` | `queries` |
  | --- | --- | --- | --- | --- | --- | --- |
  | 1 | `info_type_it1_ab` | `info_type` | `id` | 1 | `info = 'genres'` | 1a, 1b |
  | 1 | `info_type_it2_a` | `info_type` | `id` | 1 | `info = 'rating'` | 1a |
  | 1 | `keyword_k_ab` | `keyword` | `id` | 1 | `keyword LIKE '%hero%'` | 1a, 1b |
  | 1 | `movie_info_mi1_a` | `movie_info` | `info_type_id, movie_id, id` | 2 | `info IN ('Action', 'Drama')` | 1a |
  | 1 | `movie_info` | `movie_info` | `info_type_id, movie_id, id` | 2 | (none) | 1a |
  | 1 | `movie_keyword` | `movie_keyword` | `movie_id, keyword_id, id` | 2 | (none) | 1a, 1b |
  | 1 | `title_t_a` | `title` | `id` | 1 | `production_year > 1990` | 1a |
  | 1 | `movie_info_mi1_b` | `movie_info` | `info_type_id, movie_id, id` | 2 | `info NOT LIKE 'Doc%'` | 1b |
  | 1 | `title_t_b` | `title` | `id` | 1 | `production_year BETWEEN 1980 AND 1995` | 1b |
  | 2 | `cast_info` | `cast_info` | `movie_id, person_id, id` | 2 | (none) | 2a, 2b |
  | 2 | `kind_type_kt_a` | `kind_type` | `id` | 1 | `kind = 'movie'` | 2a |
  | 2 | `name_n_a` | `name` | `id` | 1 | `name LIKE '%Ray%'` | 2a |
  | 2 | `title_t_a` | `title` | `id, kind_id` | 2 | `title LIKE '%"Hero"%'` | 2a |
  | 2 | `name_n_b` | `name` | `id` | 1 | `name LIKE '%Zed%'` | 2b |
  | 2 | `title_t_b` | `title` | `id` | 1 | `production_year > 1990` | 2b |

  Shared across variants: `info_type_it1_ab` and `keyword_k_ab` (filtered; the name lists both
  variant letters), `movie_keyword` (unfiltered, same column order in both queries) and
  `cast_info`. Not shared although the table is: `movie_info_mi1_a` and `movie_info_mi1_b`
  (different filters), and `movie_info` (1a only). A carried `id` (the `_` in a rule) appears
  only in `movie_info_mi1_a`, `movie_info`, `movie_keyword`, `movie_info_mi1_b` and
  `cast_info`; every other relation joins on its own `id`, so none appends a second one
  (`title_t_a` of template 2 is `(id, kind_id)`).

  **Relation contents** (a tuple lists the relation's `columns` in order; the file's rows are
  in `ORDER BY id` order, and each relation holds the source rows that pass its `filter` and
  have no NULL in its first `jc` columns):

  | `name` | Rows | Source rows the filter or the NULL check removes |
  | --- | --- | --- |
  | `info_type_it1_ab` | (1) | 2 `rating`, 3 `runtime` (by `=`) |
  | `info_type_it2_a` | (2) | 1, 3 |
  | `keyword_k_ab` | (1), (2) | 3 `villain`, 4 `sidekick` (by `LIKE`) |
  | `movie_info_mi1_a` | (1,1,1), (1,2,3), (1,3,8), (1,4,9), (1,7,11) | by `IN`: 2, 4-7, 10, 12, 13, 15, 16; by NULL: 14 (`Action`, NULL type) |
  | `movie_info` | (1,1,1), (2,1,2), (1,2,3), (2,2,4), (1,2,5), (1,3,6), (2,3,7), (1,3,8), (1,4,9), (2,4,10), (1,7,11), (2,7,12), (1,8,13), (2,2,15), (3,1,16) | NULL: 14 |
  | `movie_keyword` | (1,1,1), (2,1,2), (2,2,3), (3,4,4), (4,3,5), (8,1,6), (7,4,7), (3,2,8), (1,2,9), (7,3,10) | none |
  | `title_t_a` (1) | (2), (3), (4), (8), (9) | by `>`: 1 (1985), 6 (1950), 7 (1990, the boundary); NULL year: 5 |
  | `movie_info_mi1_b` | (1,1,1), (2,1,2), (1,2,3), (2,2,4), (1,2,5), (2,3,7), (1,3,8), (1,4,9), (2,4,10), (1,7,11), (2,7,12), (1,8,13), (2,2,15), (3,1,16) | by `NOT LIKE`: 6 `Documentary`; NULL: 14 |
  | `title_t_b` | (1), (2), (3), (7) | by `BETWEEN`: 4, 6, 8, 9; NULL year: 5; kept at the bounds: 2 (1995) |
  | `cast_info` | (2,2,1), (2,4,2), (2,1,4), (3,2,5), (9,4,6), (9,2,7), (1,4,8), (8,3,9) | NULL `person_id`: 3 |
  | `kind_type_kt_a` | (1) | 2, 3 |
  | `name_n_a` | (2), (4) | 1, 3, 5 (by `LIKE`) |
  | `title_t_a` (2) | (2,1), (9,2) | 2 and 9 are the two titles holding `"Hero"` (the CSV's `\"`); every other title fails the `LIKE` |
  | `name_n_b` | (empty) | every name |
  | `title_t_b` (2) | (2), (3), (4), (8), (9) | 1, 5, 6, 7 |

  **Counts** (`COUNT(*)` of the original statement; bag semantics; NULL keys never join):

  | Query | `COUNT(*)` | Working |
  | --- | --- | --- |
  | 1a | **5** | `it1`, `it2` are single rows (ids 1, 2). Per movie `t` in `title_t_a` = {2,3,4,8,9}: `mi1` rows (type 1, `IN`) × `mi2` rows (type 2) × `mk` rows with keyword in {1,2}. t=2: 1 (row 3) × 2 (rows 4, 15) × 2 (kw 1, 2) = 4. t=3: 1 (row 8) × 1 (row 7) × 1 (kw 2; kw 4 fails the `LIKE`) = 1. t=4: 1 × 1 × 0 (kw 3 only) = 0. t=8: 0 `mi1` rows (13 is `Horror`, 14 has a NULL type). t=9: no rows. Total 5. |
  | 1b | **7** | `it1` = id 1. Per movie in `title_t_b` = {1,2,3,7}: `mi1` rows (type 1, not `Doc%`) × `mk` rows with keyword in {1,2}. t=1: 1 (row 1) × 2 = 2. t=2: 2 (rows 3, 5) × 2 = 4. t=3: 1 (row 8; row 6 is `Documentary`) × 1 (kw 2) = 1. t=7: 1 × 0 (kw 4, 3) = 0. Total 7. |
  | 2a | **2** | `kt` = id 1; titles with `"Hero"` are 2 (kind 1) and 9 (kind 2): only 2 joins `kt`. `cast_info` rows for movie 2: persons 2, 4, NULL, 1; names matching `%Ray%` are 2 and 4. Total 2. (Ignoring the `kind_type` join would give 4: title 9 brings `cast_info` rows 6 and 7.) |
  | 2b | **0** | `name LIKE '%Zed%'` matches none of the five names. |

  Mutant checks the numbers support (each filter changes at least one count): dropping the
  `IN` filter makes 1a 10 (t=2 gains `Comedy`: +4; t=3 gains `Documentary`: +1); dropping `NOT LIKE` makes 1b 8 (t=3 gains `Documentary`); dropping
  `>` makes 1a 7 (t=1 joins in); dropping the keyword `LIKE` makes 1a 7 (t=3 and t=4 gain
  one keyword each).

  J5 reads `rows.txt` with `line.split_once(' ')` (table, then the count), as Step 9's
  `fixture_rows` helper does.

- [ ] **Step 5: Write the failing `pins.rs` tests.**

  In `kermit-generate/src/job/mod.rs` add the modules (rustfmt keeps them sorted). Before:
  ```rust
  pub mod duckdb;
  pub mod translate;
  ```
  After:
  ```rust
  pub mod duckdb;
  pub mod pins;
  pub mod raw;
  pub mod translate;
  ```
  Create `kermit-generate/src/job/pins.rs` with the module doc and the tests only (Step 7 adds
  the constants above `mod tests`):

```rust
//! What the raw IMDb cache is checked against: where `imdb.tgz` is served, the
//! digest of its bytes, the commit and the digests of the 114 files read from
//! `gregrahn/join-order-benchmark`, and the row count of each of the 21 tables
//! after loading. Nothing here is fetched or computed at run time.

#[cfg(test)]
mod tests {
    use {super::*, std::collections::HashSet};

    /// The 21 tables `schema.sql` creates, in its order.
    const SCHEMA_TABLES: [&str; 21] = [
        "aka_name",
        "aka_title",
        "cast_info",
        "char_name",
        "comp_cast_type",
        "company_name",
        "company_type",
        "complete_cast",
        "info_type",
        "keyword",
        "kind_type",
        "link_type",
        "movie_companies",
        "movie_info",
        "movie_info_idx",
        "movie_keyword",
        "movie_link",
        "name",
        "person_info",
        "role_type",
        "title",
    ];

    #[test]
    fn query_files_are_unique_and_hex() {
        let mut seen = HashSet::new();
        for (file, digest) in QUERY_FILES {
            assert!(seen.insert(file), "{file} is pinned twice");
            assert_eq!(digest.len(), 64, "{file}");
            assert!(
                digest.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
                "{file}: {digest} is not lowercase hex"
            );
        }
    }

    /// `schema.sql` first, then the 113 queries of the 33 templates in version
    /// order (`9d` before `10a`), which is the order the files are fetched in.
    #[test]
    fn query_files_are_schema_then_113_queries_in_version_order() {
        assert_eq!(QUERY_FILES[0].0, "schema.sql");
        assert_eq!(QUERY_FILES.len() - 1, 113);
        let mut previous = (0u32, String::new());
        let mut templates = HashSet::new();
        for (file, _) in &QUERY_FILES[1..] {
            let stem = file.strip_suffix(".sql").unwrap();
            let split = stem.find(|c: char| !c.is_ascii_digit()).unwrap();
            let (number, letters) = stem.split_at(split);
            let key = (number.parse::<u32>().unwrap(), letters.to_owned());
            assert!((1..=33).contains(&key.0), "{file}");
            assert!(key > previous, "{file} is out of order");
            templates.insert(key.0);
            previous = key;
        }
        assert_eq!(templates.len(), 33);
    }

    #[test]
    fn query_url_has_the_commit() {
        assert_eq!(
            query_url("6a.sql"),
            "https://raw.githubusercontent.com/gregrahn/join-order-benchmark/\
             a39603662e023e449cb2121997a5034df9e02ebf/6a.sql"
        );
        assert!(query_url("schema.sql").contains(QUERY_COMMIT));
    }

    #[test]
    fn imdb_table_rows_name_the_21_tables() {
        let names: Vec<&str> = IMDB_TABLE_ROWS.iter().map(|(table, _)| *table).collect();
        assert_eq!(names, SCHEMA_TABLES);
    }
}
```

- [ ] **Step 6: Give `raw.rs` its module doc, then run the tests; expect a compile failure.**

  Step 5 declared `pub mod raw;`, so the file must exist, and `kermit-generate` is
  `#![deny(missing_docs)]`, so an empty file would not compile (the module has no docs). Create
  `kermit-generate/src/job/raw.rs` holding the `//!` block below and nothing else; Step 9 adds
  the `use` block and the tests under it, Step 11 the code:

```rust
//! The raw IMDb cache: `imdb.duckdb` (the 21 IMDb tables), `queries/` (the
//! pinned JOB query files) and `provenance.json`, shared by every template.
//!
//! [`build`] loads a cache from a directory of CSVs in a staging directory
//! beside the target and renames it into place, with `provenance.json`
//! written last: a directory that has it is a finished cache, and one that
//! lacks it is never reused. [`RawCache::open`] is the only reader.
```

  Then:
  ```bash
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-generate job::pins
  ```
  Expected: FAIL to compile (`cannot find value `QUERY_FILES``, `IMDB_TABLE_ROWS`,
  `cannot find function `query_url``, `QUERY_COMMIT`); the exact wording may differ. No error
  may be about `raw.rs`.

- [ ] **Step 7: Implement `pins.rs`.**

  Insert this block above `#[cfg(test)] mod tests` (the 114 digests are the `sha256sum` of the
  files at commit `a396036`, computed 2026-10-15; the two placeholders are replaced in Part B):

```rust
/// Where `imdb.tgz` is served, in the order they are tried. Both serve the
/// same 1,263,193,115 bytes.
pub const IMDB_URLS: [&str; 2] = [
    "https://bonsai.cedardb.com/job/imdb.tgz",
    "http://event.cwi.nl/da/job/imdb.tgz",
];

/// SHA-256 (lowercase hex) of `imdb.tgz`.
///
/// `"UNMEASURED"` until Part B of the J4 task measures the dump; the download in the raw
/// cache (J7) is checked against it.
pub const IMDB_SHA256: &str = "UNMEASURED";

/// The commit of `gregrahn/join-order-benchmark` the query files are read at.
pub const QUERY_COMMIT: &str = "a39603662e023e449cb2121997a5034df9e02ebf";

/// Where the query files are served: `{QUERY_BASE_URL}/{QUERY_COMMIT}/{file}`.
pub const QUERY_BASE_URL: &str = "https://raw.githubusercontent.com/gregrahn/join-order-benchmark";

/// `schema.sql` and the 113 query files at [`QUERY_COMMIT`], each with the
/// SHA-256 (lowercase hex) of its bytes: `schema.sql` first, then `1a.sql` …
/// `33c.sql` in version order. The repository carries no licence, so kermit
/// pins the files instead of committing them.
pub const QUERY_FILES: [(&str, &str); 114] = [
    ("schema.sql", "1d10ba0c9a881e890aeb951c0960cc4e3472c9c9695ce1edb6493e9e4466d3d2"),
    ("1a.sql", "53a90dac948641a2f9dd1ee12b68c66354d4c44016494d9e460b2b1879c5d7b2"),
    ("1b.sql", "3f6bc4f99b49862e63aa9f0881abad66c9a4c63a96f3c6dca722d6f007854d29"),
    ("1c.sql", "2111c8a7ecbdba7f4b9ce943c102e6da789df5337f1f0c90be310afe578f84bb"),
    ("1d.sql", "cb8a207ee5459e05a77e9428efcdca946178c893f9c7a45dcba495427a185757"),
    ("2a.sql", "71f44856563aa35d2e2612c24c80c4d67003412dbdd87fa2612a899fee6d6d9e"),
    ("2b.sql", "0da8ea6095c1550ce82aff78ecb3f6d0a46a3915aede853338a77eefef529e83"),
    ("2c.sql", "bf101673a7e20c7eb85795e4b0c3c55a4ae94523c2f245e9e5645476d989fbbb"),
    ("2d.sql", "2d0c0b02fe1f59a4f6414461fde58befc0c51fe344ed72fef505747572354459"),
    ("3a.sql", "09c04f1846b8b7d839d050b5719230e844e7612357dd749c59b37a7eb3cf9db2"),
    ("3b.sql", "a36ee625b0dbf5f7a82ebd6d558d374b1753054f8361e5cb4364badc6eb05ab9"),
    ("3c.sql", "ccfe237f533897b5666e7ee74ad6c8f36de19d6c0e0c537e8626b0c53791db6b"),
    ("4a.sql", "c80e515f37a633adca3ab45bbd5cba20e760c80a71d7841b51e64154b1008065"),
    ("4b.sql", "5e0a78655c8bf0714f365697ac2e9c4e12cc6d2096f625ff81baf33f499e6654"),
    ("4c.sql", "db79ff846ccde743ab3ae806a3047eab9c9417e45ca86272fafc43bd7422ecc4"),
    ("5a.sql", "b78eed60ed4341b93d663a99ef4268d2f0c589c159a3a56d32027aee85d8f17c"),
    ("5b.sql", "ed0bcd17c29e612d1594693215f1a192ac954011ec2c0880a0ba7c0f159491cc"),
    ("5c.sql", "7f542875e1b5247a43f7c3aad51f8fe73431866245509a98f4b321247d1794fd"),
    ("6a.sql", "666385ab792d62acef9442fc2884282d3f9194a172a6c0741544ab92eaa029b2"),
    ("6b.sql", "1510acb47c7b220b1328956345c9791c0d3857e06ed87a43d8fb2380c5b7d2e4"),
    ("6c.sql", "4ca2c51e003a181a4099984c725328151248eed7ee708009e4d4cce2fb0cac2b"),
    ("6d.sql", "8138175c4f011b8582a62921492dcf7fe4555b17fc27d4f02a9c693b519b4786"),
    ("6e.sql", "03d81143246c44c4e59206d4c3960f4cfd85da1ee63d78f065c9cde1b9bc311a"),
    ("6f.sql", "95279032832884404c90dcc043c61fb38f70f74d470d7598f4f00bd1532b96a0"),
    ("7a.sql", "2753771bb818061fdd15aea4fc147d56a89b491a2826c0b58466e72e819ff883"),
    ("7b.sql", "79b9358e5186d27007b2a3f35c18bb009b054040f85417e1858f19413993914b"),
    ("7c.sql", "ba0091ee5a01f36f30910035b54bbb08582949dda268ddd4e700854e409c97bf"),
    ("8a.sql", "a8d5f36074bd12f3d42934913b7847774ca70b43338beb2cde2952851f50e3f6"),
    ("8b.sql", "e3cda2801dcb05a99f339b5c3ef7ed8646e1ecf382664168061029eb3ae29db6"),
    ("8c.sql", "d583eba4db3a15277447be27652879cd4439ec34f1fc471982dea76729a5d099"),
    ("8d.sql", "5ad3875f0bd158450b47e4a24b24e53045a6546fa0f7530a951504b61bad4e62"),
    ("9a.sql", "1e2fcf63ad6fa5286704eb78c9b1c998a12605412fb34ed6f0b92923adc4f543"),
    ("9b.sql", "652106b0bc0741517a54838af9e71249765856d883088bdca4ec7caf4858fd65"),
    ("9c.sql", "b3a9cb8f2e5b9d07c8906793ec054e4dcbcd48ed2e8f078db87066372528664c"),
    ("9d.sql", "be3b61d598df58d2b43cbabaa07997ace2e13b0c7165ae33834a2c79e295ce22"),
    ("10a.sql", "dcffd7d004228d834af7bf332e365837cc5a3fb60fb3859a55235da6d1455a89"),
    ("10b.sql", "66be16fbc0d5bb3a99e50581448613645e6b53286e7b15cebbb5043a98bcbb3d"),
    ("10c.sql", "ccbce6a9f74d3ce0da968314672b0b5c7a479eb72621d4e845586b1150287292"),
    ("11a.sql", "b9c2b7f747fd10fff3e9078534d5578ea8b53fc9dcb4bd9330952d74ec1ece0e"),
    ("11b.sql", "38695698b69dbcc55ab609cdb5b13e1182127adea2b761c3ea9e2330e7a84927"),
    ("11c.sql", "2d0d7040e322b74ffb48d648463a0292beb0c4d363ebddb531a8d2a0e75ea127"),
    ("11d.sql", "9f3d08a09c46eb835fb3e946d1e6c22730079cfcee5dab7bf3e9a056b95a4d4e"),
    ("12a.sql", "871ec385dd6e28440d15b03cf0fdd9c9376b3453bb8ec552c4e5eb1e2afdbeb8"),
    ("12b.sql", "cdfa0891b60af9ef3bde65e85556a87d027557a89aa7ad6eec91366cacd6ca5f"),
    ("12c.sql", "19d286824b7efb84aaa9371dbf1e842d5488e70f264a23c2c944c47f819e26c2"),
    ("13a.sql", "c7d5c4fa1a5eb37bc327f531c4fe49fa4b8e2fe2ad251938880cafb3c191553d"),
    ("13b.sql", "cfc30b5bc8a2d513a060f8f58c12aeb61e17526bf4ba1921de40122197341b98"),
    ("13c.sql", "b61cbf84a4288c89c5494d812efed29b8a39c60dc637e37c9dedbb5fdfa2a2e0"),
    ("13d.sql", "871c9d8cfa90f6299c4fb950df8c7550ee4c1fe5581c166304816c706915a8ba"),
    ("14a.sql", "0c6770d9c162dca23d0d32a5d9ed2d308fc2a1b3a673463d05c81ced8445f032"),
    ("14b.sql", "6878ded3d9334f2ab68c33d10ed72e912db9407babe6547469c66d2c1bb246fe"),
    ("14c.sql", "cfeef4c875e3de99c575766bd17d613af1ee59e1588a2b5460e710d13bf87d7b"),
    ("15a.sql", "c332055a7b8e19fcbdf3eac68f6797432ffd67d87a10267a5241d0769f0a8a0d"),
    ("15b.sql", "ad3c98ba9ba0798ac5a061038d5a2a49158a196575377a21858c8b8189e551ba"),
    ("15c.sql", "875e39117606a49cdb45e2ebb57261efe38ed9f786f8cae345cf620b33dd8ca1"),
    ("15d.sql", "d052c7260a0fe10db83e108f25bc7a7488f82fa64d1963b9210af5799e2c36b4"),
    ("16a.sql", "43cd687d2b4005c6b665762dbf7e5277135ebe738d82e672371571ca255902b9"),
    ("16b.sql", "d65807d6a5dc9b37b5e484f7c57f2b469f9bd2b9e446704badb66ab29bb3a6ed"),
    ("16c.sql", "85d9b65cd520f9edc7e14c0c4e39e05cc364b3f8c4ab30de07f7c343227c8edd"),
    ("16d.sql", "83e7744d08279ea388a184256ab2deb8bf3c05fb93679c320fb508d10be32293"),
    ("17a.sql", "a52d479958ec18149f197df005f183f8cee35712398524f344b9751c9dce9fe2"),
    ("17b.sql", "da2f53d30ace326f7110861e21884165707975234072f5bdd2fff9032833f889"),
    ("17c.sql", "56b241e6789c1f20b49b9cf8957154550af396b9bf6913aff974939e2f7fb7e9"),
    ("17d.sql", "0dfa90692bd83e530058a329a5ee035d7bcf7d7c0d7ddba43eb6f8ba87a04601"),
    ("17e.sql", "6723e52c994de2ecec2f9f0aa0cbfd9e471a15b3055905dfdabc0520ca7fe0de"),
    ("17f.sql", "a03f4e3c87d842f9ac27602b01fd0c40db7f1756afb9679d40cbc3c99d553da4"),
    ("18a.sql", "04816db9a582e138e9d1b2af9491520f42fa84307cf9abf3395f9c697f02d21e"),
    ("18b.sql", "920582b900c1206a1eba63c935492109c108927b75bd548b7fe6229d8d879f0a"),
    ("18c.sql", "3247d00269cadb278d9a417b805b928b609db6e49011f981482612434dc623a9"),
    ("19a.sql", "de0e1cae5dd400d5fc0b2954eaa41616861e76e1f4a45bbf923a8b94660fee53"),
    ("19b.sql", "2148cf91cbf25063f906086d1944a00f73d5b07e5076a9f1ed8bb4712fb12807"),
    ("19c.sql", "fe5b602cc75a95ba14f4694bfbe5d8082c405aa0881a5e3e2c259ed5e0f9e8df"),
    ("19d.sql", "f450bd39710978c8bfbf961587cb3649190effe724855e6345bcf86d42ad39dc"),
    ("20a.sql", "eea79cc4c815789c64bf7f4306e8e30db85289a8938bbcd012cb22388264067f"),
    ("20b.sql", "36c7ddba787747bca16a6dcb63ad50fba60f766afe771e5d3555b71607b98298"),
    ("20c.sql", "86f02702929dee0430d2ba6e447d3454f518a09a0b9f5504dacf80a5753c5e89"),
    ("21a.sql", "c899a4175c318b17df6c253c53253a130efeed6e6d2230180b02e08e9dcba722"),
    ("21b.sql", "33f2c3edf99b23a99ea819fb41a0ff63af19f3ffbcd6e98ed94d1decb1accb67"),
    ("21c.sql", "e62c4d11928d31a55a09fc2aef9bc3812e510f016ecc2463596a099a1c4a9366"),
    ("22a.sql", "eec5b2d12dc15e51fb6bf5e83d13c0bc185ca150b4d75c07bda8eb57f861a29c"),
    ("22b.sql", "819cce32cbcac10161aaeab26817e0136e0370bb4b9cbca5cc415172bfd0ff2c"),
    ("22c.sql", "9401e4b3000f15cb9fd1a39abfda016c8f94f079c9cd8acc93d314a23e8890c5"),
    ("22d.sql", "96691d9b9a24284e9fa6992ab4c83b285579a1f1a5cf7cf4bd943b6c1fc91a71"),
    ("23a.sql", "0115941960633919edac315eb3e40d1df1953a66b93cbab316244018d2adcb09"),
    ("23b.sql", "893cb0185455fb67072b934bf12158b9dc217bc9e3ffe680deabf84c53fbc713"),
    ("23c.sql", "93566f0155c4f6816b51eb71d52aecaec868a1b160c38b4643e5cb9780bc9c3d"),
    ("24a.sql", "2364e4924f1ad3a81b07420e6d5310d3220df7474c5b11b50ed062e884daf480"),
    ("24b.sql", "199711fb4c5f118e67fbd926218b66cb96a8c2530668bc43fbe812af2a658314"),
    ("25a.sql", "643c0557c8d898467d56c492ff7b0ebe4ebacaa6131af16e0b22f26ba9f21ace"),
    ("25b.sql", "aad58fb332fd9c7c8a67067086dbc32b7884e252435bf17d3c0fbcd69d5cbae1"),
    ("25c.sql", "4a33e40c4c643d39a8d5933a4d2960a8a3f2a100e5a063ac2c20f8b007d8e5f8"),
    ("26a.sql", "e68db6c6477fcd41adde48510c3eaec1765ad13fa9e6b899348dc996f6a589cc"),
    ("26b.sql", "b0393215e99a169caa77ba893beed022e1090c9044cc412c304a31b24370f242"),
    ("26c.sql", "afd4656f06cdad04640e6ccf4db24cc251954689da270e9de259c89825d50404"),
    ("27a.sql", "b470129ed757160c21317ecd59d0fd71a3aac72052c2472a43f3007170cd6d0d"),
    ("27b.sql", "5e807f3f664f140b3bbe7637036c8bd85183ad9343b9e18d4dde49a4b30751d6"),
    ("27c.sql", "1d00d9231b2b3d9e379c5e0788fdb38e49c42bc2a73398d9588385022105cdac"),
    ("28a.sql", "f4d032f672de59f119453a2f948717e1c88dbd2358e1fb5186ca295030eac9d1"),
    ("28b.sql", "a0f8a1130ecd9954c3acb5058e91d436edeab93bff589823d8cb58dae83ae14c"),
    ("28c.sql", "8a5d2826e0d8edada1aa3afb1266e4988fc17ffa5cd0ccfa5b3410258958d7b5"),
    ("29a.sql", "a693c52de057655e421c2ab5569503ac54da257653dcb0cde8020eef027b5c96"),
    ("29b.sql", "a43ad7b45d9e01c7983b565c229356722e3f7028faecee48d66b5d7546181772"),
    ("29c.sql", "b338eb16c2ff5a5022ef609b9a77e6c4ef76a9771eca8de5785aef382ea33078"),
    ("30a.sql", "9ec9bb8b2521099af76e45201b970cc4373478ad8c0eb0323a4a02068751c802"),
    ("30b.sql", "6294013729f3373603d5dd24e49df10a3f87f51c883054d19e7112c493a91e3f"),
    ("30c.sql", "1fc2c1125858123d908e2808f46471a76c0e1e787161e88e32670c15b1da6d8e"),
    ("31a.sql", "dd779ae7e005300c9af6fd006ab07ff93d75cdd766df13467b73955271a6e02b"),
    ("31b.sql", "c97f5c77bede5ce118782f92c87a3dd05fd93d2453749860124ba19c77451d51"),
    ("31c.sql", "8064d891a412b1d953c11b8b8c60d5666c3e017aa3048dc29e1648e393d984b3"),
    ("32a.sql", "920bf48d8f22cb80a07f4d6d1567614fc6ca269487e18e4a897c72443529427b"),
    ("32b.sql", "b8bb4489b603250702299fe3a8eb763a514f2fa42568524a2d486f692b7d0eb7"),
    ("33a.sql", "16ec76ddb7fed9155cf98d85c3a6741cc75c1f08e45da9ffda3206a0d8215c2b"),
    ("33b.sql", "89639c0c29423a3fe0d8bc4ddf3314ca83c4be72a09ec201c6cb14df0604ca5c"),
    ("33c.sql", "ad00e304413df75daefbde27f18872bcfc33f1bcc86a95c6dd133d4a5fc27f1d"),
];

/// The row count of each of the 21 IMDb tables after loading `imdb.tgz`,
/// in `schema.sql`'s order.
///
/// All `0` until Part B of the J4 task measures them; the raw-cache build
/// checks every loaded table against its entry.
pub const IMDB_TABLE_ROWS: [(&str, u64); 21] = [
    ("aka_name", 0),
    ("aka_title", 0),
    ("cast_info", 0),
    ("char_name", 0),
    ("comp_cast_type", 0),
    ("company_name", 0),
    ("company_type", 0),
    ("complete_cast", 0),
    ("info_type", 0),
    ("keyword", 0),
    ("kind_type", 0),
    ("link_type", 0),
    ("movie_companies", 0),
    ("movie_info", 0),
    ("movie_info_idx", 0),
    ("movie_keyword", 0),
    ("movie_link", 0),
    ("name", 0),
    ("person_info", 0),
    ("role_type", 0),
    ("title", 0),
];

/// The URL of one query file (or `schema.sql`) at [`QUERY_COMMIT`].
pub fn query_url(file: &str) -> String { format!("{QUERY_BASE_URL}/{QUERY_COMMIT}/{file}") }
```

- [ ] **Step 8: Run the `pins` tests; expect PASS.**
  ```bash
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-generate job::pins
  ```
  Expected: `test result: ok. 4 passed; 0 failed`.

- [ ] **Step 9: Write the failing `raw.rs` tests.**

  `kermit-generate/src/job/raw.rs` holds only the `//!` block that Step 6 wrote. Append the
  `use` block and the tests below it (keep the `//!` block as the file's first lines); Step 11
  adds the implementation between the `use` block and `mod tests`:

```rust
use {
    crate::job::duckdb::{path_literal, Duckdb},
    anyhow::{anyhow, ensure, Context},
    serde::{Deserialize, Serialize},
    std::{
        collections::BTreeMap,
        fs,
        path::{Path, PathBuf},
        process::Command,
    },
};

#[cfg(test)]
mod tests {
    use {super::*, crate::job::duckdb::test_support::locate_or_skip};

    /// The committed fixture: `schema.sql`, `csv/`, `queries/` and `rows.txt`.
    fn fixture_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/job")
    }

    /// `rows.txt`: one `<table> <rows>` pair per line.
    fn fixture_rows() -> Vec<(String, u64)> {
        let text = fs::read_to_string(fixture_dir().join("rows.txt")).unwrap();
        text.lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| {
                let (table, rows) = line.split_once(' ').unwrap();
                (table.to_owned(), rows.parse().unwrap())
            })
            .collect()
    }

    fn provenance(duckdb_version: &str) -> RawProvenance {
        RawProvenance {
            imdb_sha256: "ab".repeat(32),
            query_commit: "a39603662e023e449cb2121997a5034df9e02ebf".to_owned(),
            query_sha256: BTreeMap::from([("1a.sql".to_owned(), "cd".repeat(32))]),
            duckdb_version: duckdb_version.to_owned(),
        }
    }

    /// Builds the fixture's queries and `rows` from `csv_dir` into `dir`.
    fn build_from(
        duckdb: &Duckdb, dir: &Path, csv_dir: &Path, rows: &[(String, u64)], version: &str,
    ) -> anyhow::Result<RawCache> {
        let fixture = fixture_dir();
        let schema_sql = fixture.join("schema.sql");
        let queries_dir = fixture.join("queries");
        let table_rows: Vec<(&str, u64)> =
            rows.iter().map(|(table, n)| (table.as_str(), *n)).collect();
        build(
            duckdb,
            dir,
            &BuildInputs {
                csv_dir,
                schema_sql: &schema_sql,
                queries_dir: &queries_dir,
                table_rows: &table_rows,
                provenance: provenance(version),
            },
        )
    }

    fn build_fixture(duckdb: &Duckdb, dir: &Path, version: &str) -> anyhow::Result<RawCache> {
        build_from(duckdb, dir, &fixture_dir().join("csv"), &fixture_rows(), version)
    }

    /// The statement that counts a fixture query: its projection replaced by
    /// `COUNT(*)`. (The generator derives this from the parsed AST; the test
    /// needs no translator.)
    fn count_sql(sql: &str) -> String {
        let from = sql.find("\nFROM").expect("every fixture query has a FROM line");
        format!("SELECT COUNT(*){}", sql[from..].trim_end().trim_end_matches(';'))
    }

    /// A hand-made cache directory (no DuckDB): `queries/` holding `files`
    /// and a `provenance.json`.
    fn hand_made_cache(root: &Path, files: &[&str]) -> RawCache {
        let dir = root.join("imdb");
        fs::create_dir_all(dir.join(QUERIES_DIR)).unwrap();
        for file in files {
            fs::write(dir.join(QUERIES_DIR).join(file), format!("-- {file}\n")).unwrap();
        }
        let json = serde_json::to_string_pretty(&provenance("v")).unwrap();
        fs::write(dir.join(PROVENANCE_FILE), json).unwrap();
        RawCache::open(&dir).unwrap()
    }

    // ---- no DuckDB needed ---------------------------------------------------

    #[test]
    fn split_query_stem_takes_template_and_letters() {
        assert_eq!(split_query_stem("1a"), Some((1, "a")));
        assert_eq!(split_query_stem("10a"), Some((10, "a")));
        assert_eq!(split_query_stem("33c"), Some((33, "c")));
        for bad in ["", "1", "a1", "schema", "1A", "1a2", "x1a"] {
            assert_eq!(split_query_stem(bad), None, "{bad:?}");
        }
    }

    /// Variants come back in letter order, a template never picks up another
    /// whose number starts with its own (`1` and `10a`), and files that are
    /// not `<template><letters>.sql` are ignored.
    #[test]
    fn template_queries_are_in_letter_order() {
        let scratch = tempfile::tempdir().unwrap();
        let cache = hand_made_cache(
            scratch.path(),
            &["1b.sql", "10a.sql", "1a.sql", "1c.sql", "2a.sql", "notes.txt", "1.sql", "schema.sql"],
        );
        let names = |template: u8| -> Vec<String> {
            cache
                .template_queries(template)
                .unwrap()
                .into_iter()
                .map(|(name, _)| name)
                .collect()
        };
        assert_eq!(names(1), ["1a", "1b", "1c"]);
        assert_eq!(names(10), ["10a"]);
        assert_eq!(names(2), ["2a"]);
        assert!(names(3).is_empty());
        let first = cache.template_queries(1).unwrap().remove(0);
        assert_eq!(first, ("1a".to_owned(), "-- 1a.sql\n".to_owned()));
    }

    #[test]
    fn provenance_round_trips() {
        let scratch = tempfile::tempdir().unwrap();
        let cache = hand_made_cache(scratch.path(), &["1a.sql"]);
        assert_eq!(cache.provenance().unwrap(), provenance("v"));
        // A corrupt file is an error naming it, not a default.
        fs::write(cache.dir().join(PROVENANCE_FILE), "not json").unwrap();
        let error = cache.provenance().unwrap_err();
        assert!(format!("{error:#}").contains(PROVENANCE_FILE), "{error:#}");
    }

    /// `provenance.json` is the commit marker: a directory with a database and
    /// queries but no provenance is an unfinished cache.
    #[test]
    fn open_needs_provenance_last() {
        let scratch = tempfile::tempdir().unwrap();
        let dir = scratch.path().join("imdb");
        fs::create_dir_all(dir.join(QUERIES_DIR)).unwrap();
        fs::write(dir.join(DATABASE_FILE), b"").unwrap();
        assert!(RawCache::open(&dir).is_none());
        fs::write(dir.join(PROVENANCE_FILE), "{}").unwrap();
        assert!(RawCache::open(&dir).is_some());
        assert!(RawCache::open(&scratch.path().join("absent")).is_none());
    }

    #[test]
    fn default_raw_dir_sits_beside_the_benchmark_cache() {
        let (Ok(raw), Ok(benchmarks)) = (default_raw_dir(), kermit_bench::cache::base_cache_dir())
        else {
            eprintln!("skipping: no platform cache directory");
            return;
        };
        assert_eq!(raw.file_name().unwrap(), RAW_DIR_NAME);
        assert_eq!(raw.parent(), benchmarks.parent());
    }

    #[test]
    fn extract_tgz_unpacks_at_the_top_level() {
        let scratch = tempfile::tempdir().unwrap();
        let source = scratch.path().join("source");
        fs::create_dir(&source).unwrap();
        fs::write(source.join("title.csv"), "1,a\n").unwrap();
        let tgz = scratch.path().join("x.tgz");
        let made = Command::new("tar").arg("-czf").arg(&tgz).arg("-C").arg(&source).arg("title.csv").status();
        let Ok(status) = made else {
            eprintln!("skipping: no tar");
            return;
        };
        assert!(status.success());
        let into = scratch.path().join("out");
        extract_tgz(&tgz, &into).unwrap();
        assert_eq!(fs::read_to_string(into.join("title.csv")).unwrap(), "1,a\n");
    }

    #[test]
    fn extract_tgz_rejects_a_file_that_is_not_a_tgz() {
        // Without `tar`, `extract_tgz` fails before it names the file.
        if Command::new("tar").arg("--version").output().is_err() {
            eprintln!("skipping: no tar");
            return;
        }
        let scratch = tempfile::tempdir().unwrap();
        let tgz = scratch.path().join("garbage.tgz");
        fs::write(&tgz, "this is not gzip").unwrap();
        let error = extract_tgz(&tgz, &scratch.path().join("out")).unwrap_err();
        assert!(format!("{error:#}").contains("garbage.tgz"), "{error:#}");
    }

    // ---- DuckDB-gated -------------------------------------------------------

    /// The fixture loads: every table has its pinned row count, the
    /// backslash-escaped quotes arrived as quotes with the other fields in
    /// place, the NULLs stayed NULL, 2b's filter matches nothing, the queries
    /// were copied, and nothing is left beside the cache.
    #[test]
    fn builds_the_fixture() {
        let Some(duckdb) = locate_or_skip() else { return };
        let scratch = tempfile::tempdir().unwrap();
        let dir = scratch.path().join("imdb");
        let cache = build_fixture(&duckdb, &dir, "v-test").unwrap();
        assert_eq!(cache.dir(), dir.as_path());
        assert!(cache.database().is_file());
        assert!(RawCache::open(&dir).is_some());
        assert_eq!(cache.provenance().unwrap(), provenance("v-test"));
        let count = |sql: &str| {
            duckdb.query_u64(&cache.database(), &cache.temp_dir(), sql).unwrap()
        };
        for (table, rows) in fixture_rows() {
            assert_eq!(count(&format!("SELECT COUNT(*) FROM \"{table}\"")), rows, "{table}");
        }
        assert_eq!(count(r#"SELECT COUNT(*) FROM title WHERE title LIKE '%"Hero"%'"#), 2);
        assert_eq!(count("SELECT production_year FROM title WHERE id = 2"), 1995);
        assert_eq!(count("SELECT COUNT(*) FROM title WHERE title = 'Hero, Interrupted'"), 1);
        assert_eq!(count("SELECT COUNT(*) FROM title WHERE production_year IS NULL"), 1);
        assert_eq!(count("SELECT COUNT(*) FROM cast_info WHERE person_id IS NULL"), 1);
        assert_eq!(count("SELECT COUNT(*) FROM movie_info WHERE info_type_id IS NULL"), 1);
        assert_eq!(count("SELECT COUNT(*) FROM name WHERE name LIKE '%Zed%'"), 0);
        for query in ["1a.sql", "1b.sql", "2a.sql", "2b.sql"] {
            assert!(dir.join(QUERIES_DIR).join(query).is_file(), "{query}");
        }
        assert_eq!(fs::read_dir(scratch.path()).unwrap().count(), 1, "no staging directory");
    }

    /// The four fixture queries, counted by DuckDB, give the numbers the
    /// fixture-facts table in the plan derives by hand (and that J5 relies on).
    #[test]
    fn fixture_queries_count_as_the_facts_say() {
        let Some(duckdb) = locate_or_skip() else { return };
        let scratch = tempfile::tempdir().unwrap();
        let cache = build_fixture(&duckdb, &scratch.path().join("imdb"), "v-test").unwrap();
        let mut counts = Vec::new();
        for template in [1, 2] {
            for (name, sql) in cache.template_queries(template).unwrap() {
                let rows = duckdb
                    .query_u64(&cache.database(), &cache.temp_dir(), &count_sql(&sql))
                    .unwrap();
                counts.push((name, rows));
            }
        }
        let expected = [("1a", 5), ("1b", 7), ("2a", 2), ("2b", 0)];
        assert_eq!(
            counts,
            expected.map(|(name, rows)| (name.to_owned(), rows)),
        );
    }

    /// A pinned count that disagrees with the loaded table is an error naming
    /// the table and both counts, and leaves neither a cache nor a staging
    /// directory.
    #[test]
    fn row_count_mismatch_leaves_no_cache() {
        let Some(duckdb) = locate_or_skip() else { return };
        let scratch = tempfile::tempdir().unwrap();
        let dir = scratch.path().join("imdb");
        let mut rows = fixture_rows();
        for entry in &mut rows {
            if entry.0 == "title" {
                entry.1 = 10;
            }
        }
        let error = build_from(&duckdb, &dir, &fixture_dir().join("csv"), &rows, "v").unwrap_err();
        let message = format!("{error:#}");
        assert!(message.contains("table `title` loaded 9 rows, expected 10"), "{message}");
        assert!(!dir.exists());
        assert!(RawCache::open(&dir).is_none());
        assert_eq!(fs::read_dir(scratch.path()).unwrap().count(), 0, "staging directory removed");
    }

    /// A cache directory whose parent does not exist yet (a fresh
    /// `$XDG_CACHE_HOME/kermit/imdb`) builds: `Staging::create` makes the
    /// parent, and nothing but the cache is left in it.
    #[test]
    fn builds_under_a_parent_that_does_not_exist_yet() {
        let Some(duckdb) = locate_or_skip() else { return };
        let scratch = tempfile::tempdir().unwrap();
        let parent = scratch.path().join("kermit");
        let dir = parent.join("imdb");
        assert!(!parent.exists());
        let cache = build_fixture(&duckdb, &dir, "v-test").unwrap();
        assert_eq!(cache.dir(), dir.as_path());
        assert!(RawCache::open(&dir).is_some());
        assert_eq!(fs::read_dir(&parent).unwrap().count(), 1, "no staging directory");
    }

    #[test]
    fn missing_csv_names_the_table() {
        let Some(duckdb) = locate_or_skip() else { return };
        let scratch = tempfile::tempdir().unwrap();
        let csv = scratch.path().join("csv");
        fs::create_dir(&csv).unwrap();
        for entry in fs::read_dir(fixture_dir().join("csv")).unwrap() {
            let path = entry.unwrap().path();
            let file = path.file_name().unwrap().to_owned();
            if file != "keyword.csv" {
                fs::copy(&path, csv.join(file)).unwrap();
            }
        }
        let dir = scratch.path().join("imdb");
        let error = build_from(&duckdb, &dir, &csv, &fixture_rows(), "v").unwrap_err();
        assert!(format!("{error:#}").contains("table `keyword` has no CSV"), "{error:#}");
        assert!(!dir.exists());
    }

    #[test]
    fn missing_row_count_names_the_table() {
        let Some(duckdb) = locate_or_skip() else { return };
        let scratch = tempfile::tempdir().unwrap();
        let dir = scratch.path().join("imdb");
        let mut rows = fixture_rows();
        rows.retain(|(table, _)| table != "keyword");
        let error = build_from(&duckdb, &dir, &fixture_dir().join("csv"), &rows, "v").unwrap_err();
        assert!(
            format!("{error:#}").contains("table `keyword` has no pinned row count"),
            "{error:#}"
        );
        assert!(!dir.exists());
    }

    /// A `<dir>.building-<pid>` left by a killed run is removed, never reused
    /// (not even one that looks finished).
    #[test]
    fn leftover_staging_dir_is_not_reused() {
        let Some(duckdb) = locate_or_skip() else { return };
        let scratch = tempfile::tempdir().unwrap();
        let dir = scratch.path().join("imdb");
        let stale = scratch.path().join(format!("imdb.building-{}", std::process::id()));
        fs::create_dir_all(stale.join(QUERIES_DIR)).unwrap();
        fs::write(stale.join("stale.txt"), "left by a killed build").unwrap();
        fs::write(stale.join(PROVENANCE_FILE), "{}").unwrap();
        let cache = build_fixture(&duckdb, &dir, "v-test").unwrap();
        assert!(!cache.dir().join("stale.txt").exists());
        assert!(!stale.exists());
        assert_eq!(cache.provenance().unwrap(), provenance("v-test"));
    }

    /// `provenance.json` is the last thing written: nothing in the cache is
    /// newer than it.
    #[test]
    fn build_writes_provenance_last() {
        let Some(duckdb) = locate_or_skip() else { return };
        let scratch = tempfile::tempdir().unwrap();
        let dir = scratch.path().join("imdb");
        let cache = build_fixture(&duckdb, &dir, "v-test").unwrap();
        let modified = |path: PathBuf| fs::metadata(path).unwrap().modified().unwrap();
        let provenance = modified(dir.join(PROVENANCE_FILE));
        assert!(provenance >= modified(cache.database()));
        assert!(provenance >= modified(dir.join(QUERIES_DIR).join("1a.sql")));
    }

    /// A build that loses the final rename to a concurrent one discards its
    /// own staging directory and adopts the winner's cache.
    #[test]
    fn losing_the_rename_adopts_the_winner() {
        let Some(duckdb) = locate_or_skip() else { return };
        let scratch = tempfile::tempdir().unwrap();
        let dir = scratch.path().join("imdb");
        let first = build_fixture(&duckdb, &dir, "winner").unwrap();
        let second = build_fixture(&duckdb, &dir, "loser").unwrap();
        assert_eq!(second.dir(), first.dir());
        assert_eq!(second.provenance().unwrap().duckdb_version, "winner");
        assert_eq!(fs::read_dir(scratch.path()).unwrap().count(), 1, "the loser's staging is gone");
    }
}
```

- [ ] **Step 10: Run the tests; expect a compile failure.**
  ```bash
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-generate job::raw
  ```
  Expected: FAIL to compile (`cannot find type `RawProvenance``, `BuildInputs`, `RawCache`,
  `function `build``, `extract_tgz`, `default_raw_dir`, `split_query_stem`, `QUERIES_DIR`,
  `DATABASE_FILE`, `PROVENANCE_FILE`, `RAW_DIR_NAME`). The exact wording and the number of
  errors may differ (E0425 for a value or function, E0412 for a type, E0422 for a struct
  literal); any compile failure that names these items is the expected one.

- [ ] **Step 11: Implement `raw.rs`.**

  Insert this block between the `use` block and `#[cfg(test)] mod tests`:

```rust
/// The directory name of the raw cache under `<platform cache>/kermit/`.
pub const RAW_DIR_NAME: &str = "imdb";

const DATABASE_FILE: &str = "imdb.duckdb";
const QUERIES_DIR: &str = "queries";
const TEMP_DIR: &str = "duckdb.tmp";
const PROVENANCE_FILE: &str = "provenance.json";

/// `<platform cache>/kermit/imdb`, beside the `benchmarks` directory that
/// `kermit_bench::cache::base_cache_dir` names.
///
/// # Errors
///
/// Fails when the platform has no cache directory.
pub fn default_raw_dir() -> anyhow::Result<PathBuf> {
    let benchmarks = kermit_bench::cache::base_cache_dir()?;
    let kermit_cache = benchmarks
        .parent()
        .with_context(|| format!("{} has no parent directory", benchmarks.display()))?;
    Ok(kermit_cache.join(RAW_DIR_NAME))
}

/// What a raw cache was built from; `provenance.json` holds it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RawProvenance {
    /// SHA-256 of the `imdb.tgz` the tables were loaded from.
    pub imdb_sha256: String,
    /// The commit of `gregrahn/join-order-benchmark` the queries were read at.
    pub query_commit: String,
    /// SHA-256 of each query file (and `schema.sql`), by file name.
    pub query_sha256: BTreeMap<String, String>,
    /// `duckdb --version` of the build.
    pub duckdb_version: String,
}

/// Everything [`build`] loads a raw cache from.
pub struct BuildInputs<'a> {
    /// A directory holding `<table>.csv` for every table `schema_sql` creates.
    pub csv_dir: &'a Path,
    /// The `schema.sql` that creates the tables.
    pub schema_sql: &'a Path,
    /// A directory holding only the query files, `<template><letters>.sql`
    /// (`16b.sql`); they are copied into the cache.
    pub queries_dir: &'a Path,
    /// The row count each table must have after loading.
    pub table_rows: &'a [(&'a str, u64)],
    /// Written to `provenance.json` once everything else is in place.
    pub provenance: RawProvenance,
}

/// A complete raw cache: `imdb.duckdb`, `queries/` and `provenance.json`
/// (written last).
#[derive(Debug, Clone)]
pub struct RawCache {
    dir: PathBuf,
}

impl RawCache {
    /// The cache at `dir`, if it is finished: `Some` exactly when
    /// `<dir>/provenance.json` exists, since [`build`] writes it last.
    pub fn open(dir: &Path) -> Option<RawCache> {
        dir.join(PROVENANCE_FILE).is_file().then(|| RawCache {
            dir: dir.to_path_buf(),
        })
    }

    /// The cache directory.
    pub fn dir(&self) -> &Path { &self.dir }

    /// `<dir>/imdb.duckdb`.
    pub fn database(&self) -> PathBuf { self.dir.join(DATABASE_FILE) }

    /// `<dir>/duckdb.tmp`, the spill directory of every session on this cache.
    pub fn temp_dir(&self) -> PathBuf { self.dir.join(TEMP_DIR) }

    /// What the cache was built from.
    ///
    /// # Errors
    ///
    /// Fails if `provenance.json` cannot be read or parsed.
    pub fn provenance(&self) -> anyhow::Result<RawProvenance> {
        let path = self.dir.join(PROVENANCE_FILE);
        let text =
            fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
        serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))
    }

    /// `(name, sql)` of `<dir>/queries/<template><letters>.sql` for every
    /// variant of `template`, in letter order (`("1a", …)`, `("1b", …)`).
    /// Empty when the cache holds none; other files in `queries/` are ignored,
    /// and `1` never matches `10a`.
    ///
    /// # Errors
    ///
    /// Fails if `queries/` or a query file cannot be read.
    pub fn template_queries(&self, template: u8) -> anyhow::Result<Vec<(String, String)>> {
        let dir = self.dir.join(QUERIES_DIR);
        let mut found = Vec::new();
        for entry in fs::read_dir(&dir).with_context(|| format!("reading {}", dir.display()))? {
            let path = entry
                .with_context(|| format!("reading {}", dir.display()))?
                .path();
            if path.extension().and_then(|extension| extension.to_str()) != Some("sql") {
                continue;
            }
            let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
                continue;
            };
            let Some((number, letters)) = split_query_stem(stem) else {
                continue;
            };
            if number == u32::from(template) {
                found.push((letters.to_owned(), path.clone()));
            }
        }
        found.sort_by_key(|(letters, _)| (letters.len(), letters.clone()));
        found
            .into_iter()
            .map(|(letters, path)| -> anyhow::Result<(String, String)> {
                let sql = fs::read_to_string(&path)
                    .with_context(|| format!("reading {}", path.display()))?;
                Ok((format!("{template}{letters}"), sql))
            })
            .collect()
    }
}

/// Splits a query file stem such as `16b` into its template number and its
/// variant letters; `None` for anything else (`schema`, `1`, `1A`, `1a2`).
fn split_query_stem(stem: &str) -> Option<(u32, &str)> {
    let digits = stem.find(|c: char| !c.is_ascii_digit())?;
    let (number, letters) = stem.split_at(digits);
    if number.is_empty() || !letters.chars().all(|c| c.is_ascii_lowercase()) {
        return None;
    }
    Some((number.parse().ok()?, letters))
}

/// A staging directory that removes itself on drop unless it was renamed
/// into place. It is the one staging guard of the raw cache: [`build`] stages
/// in `<dir>.building-<pid>`, and the download stage of `ensure` stages in
/// `<dir>.fetch-<pid>` with the same type.
///
/// `Drop` runs on every `?` return and on a panic. A process killed by a
/// signal (`SIGINT`, `SIGKILL`) skips it, so a directory under a dead pid can
/// remain; nothing cleans those up automatically.
struct Staging {
    path: Option<PathBuf>,
}

impl Staging {
    /// Creates `path` empty, and its parent directory if that is missing (a
    /// fresh `$XDG_CACHE_HOME/kermit` has no `imdb` yet), first removing
    /// whatever a killed run left at `path`.
    fn create(path: PathBuf) -> anyhow::Result<Staging> {
        if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
            fs::create_dir_all(parent)
                .with_context(|| format!("creating {}", parent.display()))?;
        }
        if path.exists() {
            fs::remove_dir_all(&path)
                .with_context(|| format!("removing the stale staging directory {}", path.display()))?;
        }
        fs::create_dir(&path)
            .with_context(|| format!("creating the staging directory {}", path.display()))?;
        Ok(Staging { path: Some(path) })
    }

    fn path(&self) -> &Path { self.path.as_deref().expect("a live staging directory") }

    /// The directory has been renamed into place: nothing to clean up.
    fn renamed(mut self) { self.path = None; }
}

impl Drop for Staging {
    fn drop(&mut self) {
        if let Some(path) = &self.path {
            let _ = fs::remove_dir_all(path);
        }
    }
}

/// Builds a raw cache in `<dir>.building-<pid>` and renames it to `dir`.
///
/// Loads `schema.sql`, then each table the schema created (read back from
/// `information_schema.tables`) with `COPY <table> FROM '<csv>' (FORMAT csv,
/// HEADER false, ESCAPE '\')`, checking every row count against
/// `inputs.table_rows`, copies the queries and writes `provenance.json` last.
/// A table without its CSV or without a pinned count, a count that differs,
/// or a query file that is not `<template><letters>.sql` is an error naming
/// it, and leaves no `dir` and no staging directory (a process killed by a
/// signal leaves its staging directory behind). `dir`'s parent directories are
/// created if missing. If `dir` appeared while the build ran and holds a
/// finished cache, the staging directory is removed and that cache is
/// returned.
///
/// # Errors
///
/// As described above, and on any DuckDB or file-system failure.
pub fn build(duckdb: &Duckdb, dir: &Path, inputs: &BuildInputs<'_>) -> anyhow::Result<RawCache> {
    let parent = match dir.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    };
    let name = dir
        .file_name()
        .with_context(|| format!("{} names no directory", dir.display()))?
        .to_string_lossy()
        .into_owned();
    let staging = Staging::create(parent.join(format!("{name}.building-{}", std::process::id())))?;
    load_into(duckdb, staging.path(), inputs)?;
    match fs::rename(staging.path(), dir) {
        Ok(()) => {
            staging.renamed();
            Ok(RawCache {
                dir: dir.to_path_buf(),
            })
        },
        // Another build finished first: keep its cache, drop ours.
        Err(error) => match RawCache::open(dir) {
            Some(winner) => Ok(winner),
            None => Err(anyhow!(error).context(format!(
                "moving {} to {}",
                staging.path().display(),
                dir.display()
            ))),
        },
    }
}

/// Fills `staging`: database, queries, then `provenance.json`.
fn load_into(duckdb: &Duckdb, staging: &Path, inputs: &BuildInputs<'_>) -> anyhow::Result<()> {
    let database = staging.join(DATABASE_FILE);
    let temp = staging.join(TEMP_DIR);

    let schema = fs::read_to_string(inputs.schema_sql)
        .with_context(|| format!("reading {}", inputs.schema_sql.display()))?;
    duckdb
        .execute(&database, &temp, &schema)
        .with_context(|| format!("loading {}", inputs.schema_sql.display()))?;
    let tables = duckdb
        .query_strings(
            &database,
            &temp,
            "SELECT table_name FROM information_schema.tables \
             WHERE table_schema = 'main' AND table_type = 'BASE TABLE' ORDER BY table_name",
        )
        .context("listing the tables the schema created")?;
    ensure!(!tables.is_empty(), "{} created no tables", inputs.schema_sql.display());

    // Check every table has its CSV and its pinned count before loading any.
    let mut plan = Vec::with_capacity(tables.len());
    for table in &tables {
        let csv = inputs.csv_dir.join(format!("{table}.csv"));
        ensure!(csv.is_file(), "table `{table}` has no CSV at {}", csv.display());
        let expected = inputs
            .table_rows
            .iter()
            .find(|(name, _)| *name == table.as_str())
            .map(|(_, rows)| *rows)
            .with_context(|| format!("table `{table}` has no pinned row count"))?;
        plan.push((table, csv, expected));
    }
    for (table, csv, expected) in plan {
        let copy = format!(
            "COPY \"{table}\" FROM {} (FORMAT csv, HEADER false, ESCAPE '\\')",
            path_literal(&csv)?
        );
        duckdb
            .execute(&database, &temp, &copy)
            .with_context(|| format!("loading table `{table}` from {}", csv.display()))?;
        let loaded = duckdb
            .query_u64(&database, &temp, &format!("SELECT COUNT(*) FROM \"{table}\""))
            .with_context(|| format!("counting table `{table}`"))?;
        ensure!(loaded == expected, "table `{table}` loaded {loaded} rows, expected {expected}");
    }

    copy_queries(inputs.queries_dir, &staging.join(QUERIES_DIR))?;

    let provenance = serde_json::to_string_pretty(&inputs.provenance)
        .context("serialising the provenance")?;
    let path = staging.join(PROVENANCE_FILE);
    fs::write(&path, format!("{provenance}\n"))
        .with_context(|| format!("writing {}", path.display()))
}

/// Copies every query file of `from` into the new directory `to`; each must
/// be a regular file named `<template><letters>.sql`.
fn copy_queries(from: &Path, to: &Path) -> anyhow::Result<()> {
    fs::create_dir(to).with_context(|| format!("creating {}", to.display()))?;
    let mut files = Vec::new();
    for entry in fs::read_dir(from).with_context(|| format!("reading {}", from.display()))? {
        files.push(entry.with_context(|| format!("reading {}", from.display()))?.path());
    }
    files.sort();
    ensure!(!files.is_empty(), "{} holds no query files", from.display());
    for file in files {
        let name = file.file_name().and_then(|name| name.to_str()).unwrap_or("");
        let is_query =
            file.is_file() && name.strip_suffix(".sql").and_then(split_query_stem).is_some();
        ensure!(is_query, "{} is not a query file named <template><letters>.sql", file.display());
        fs::copy(&file, to.join(name))
            .with_context(|| format!("copying {}", file.display()))?;
    }
    Ok(())
}

/// Unpacks `tgz` into the directory `into` (created if missing) with
/// `tar -xzf <tgz> -C <into>`.
///
/// # Errors
///
/// Fails if `tar` cannot run or exits non-zero; the message quotes its stderr.
pub fn extract_tgz(tgz: &Path, into: &Path) -> anyhow::Result<()> {
    fs::create_dir_all(into).with_context(|| format!("creating {}", into.display()))?;
    let output = Command::new("tar")
        .arg("-xzf")
        .arg(tgz)
        .arg("-C")
        .arg(into)
        .output()
        .context("running `tar`")?;
    ensure!(
        output.status.success(),
        "`tar -xzf {}` failed ({}): {}",
        tgz.display(),
        output.status,
        String::from_utf8_lossy(&output.stderr).trim()
    );
    Ok(())
}
```

- [ ] **Step 12: Run the `raw` and `pins` tests; expect PASS.**
  ```bash
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-generate job::raw
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-generate job::pins
  ```
  Expected: `job::raw`: `test result: ok. 16 passed; 0 failed` (7 need no DuckDB; 9 run for
  real inside `nix develop`); `job::pins`: `4 passed`. Add `-- --nocapture` once to confirm
  there is no `skipping: no DuckDB` line.
  If `fixture_queries_count_as_the_facts_say` reports a count other than `5, 7, 2, 0`, the
  hand-computed table in Step 4 is wrong: recompute from the CSVs, fix Step 4's table and the
  `expected` array together, and say so in the commit message. If `builds_the_fixture` fails on
  a loader error, the CSV dialect (escape, NULL) differs from the contract's probe: stop and
  report the DuckDB message.

- [ ] **Step 13: Format, lint, document.**
  ```bash
  nix develop --command cargo fmt --all
  nix develop --command env CARGO_BUILD_JOBS=2 RUSTFLAGS=-Dwarnings cargo clippy -p kermit-generate --all-targets
  nix develop --command env CARGO_BUILD_JOBS=2 RUSTDOCFLAGS=-Dwarnings cargo doc -p kermit-generate --no-deps
  ```
  Expected: clean. `cargo fmt` rewraps the long lines above; keep its output.

- [ ] **Step 14: Commit Part A.** Stage each file by name (the 14 fixture files individually,
  never the directory), then commit by pathspec (never a bare `git commit`, `-a` or
  `--amend`):

```bash
git add .gitignore \
  kermit-generate/src/job/mod.rs \
  kermit-generate/src/job/pins.rs \
  kermit-generate/src/job/raw.rs \
  kermit-generate/tests/fixtures/job/schema.sql \
  kermit-generate/tests/fixtures/job/rows.txt \
  kermit-generate/tests/fixtures/job/csv/title.csv \
  kermit-generate/tests/fixtures/job/csv/kind_type.csv \
  kermit-generate/tests/fixtures/job/csv/keyword.csv \
  kermit-generate/tests/fixtures/job/csv/movie_keyword.csv \
  kermit-generate/tests/fixtures/job/csv/name.csv \
  kermit-generate/tests/fixtures/job/csv/cast_info.csv \
  kermit-generate/tests/fixtures/job/csv/info_type.csv \
  kermit-generate/tests/fixtures/job/csv/movie_info.csv \
  kermit-generate/tests/fixtures/job/queries/1a.sql \
  kermit-generate/tests/fixtures/job/queries/1b.sql \
  kermit-generate/tests/fixtures/job/queries/2a.sql \
  kermit-generate/tests/fixtures/job/queries/2b.sql
git status --short
git commit -m "feat(job): raw IMDb cache, pins and the committed fixture (#97)" \
  -m "RawCache/build load imdb.duckdb from a directory of CSVs in a staging directory renamed into place, check every table against a pinned row count and write provenance.json last. pins.rs holds the mirrors, the query commit and the 114 file digests; the IMDb digest and row counts are placeholders until they are measured. The hand-written IMDb-shaped fixture (8 tables, queries 1a, 1b, 2a, 2b) has NULL join keys, backslash-escaped quotes and rows removed by LIKE, IN, BETWEEN and a range." \
  --trailer "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>" \
  --trailer "Claude-Session: https://claude.ai/code/session_01WeRnvdstSxUY8CJGSx1NFB" \
  -- .gitignore \
  kermit-generate/src/job/mod.rs \
  kermit-generate/src/job/pins.rs \
  kermit-generate/src/job/raw.rs \
  kermit-generate/tests/fixtures/job/schema.sql \
  kermit-generate/tests/fixtures/job/rows.txt \
  kermit-generate/tests/fixtures/job/csv/title.csv \
  kermit-generate/tests/fixtures/job/csv/kind_type.csv \
  kermit-generate/tests/fixtures/job/csv/keyword.csv \
  kermit-generate/tests/fixtures/job/csv/movie_keyword.csv \
  kermit-generate/tests/fixtures/job/csv/name.csv \
  kermit-generate/tests/fixtures/job/csv/cast_info.csv \
  kermit-generate/tests/fixtures/job/csv/info_type.csv \
  kermit-generate/tests/fixtures/job/csv/movie_info.csv \
  kermit-generate/tests/fixtures/job/queries/1a.sql \
  kermit-generate/tests/fixtures/job/queries/1b.sql \
  kermit-generate/tests/fixtures/job/queries/2a.sql \
  kermit-generate/tests/fixtures/job/queries/2b.sql
```

  Expected: `git status --short` lists exactly those 18 paths as added or modified (no
  `target/`, no scratch files); one commit.

### Part B: measure the real dump (**needs `FULL_EXIT` and the user's go-ahead**)

Everything below touches the 1.26 GB `imdb.tgz` and about 5 GB of disk, and runs DuckDB on
real data for minutes. **Do not start it until (a) the co-ordinator has announced
`FULL_EXIT` for the #101 replicate chain, (b) the process check in Step 15 prints `0`, and
(c) the user has said yes to this exact list:** a 1,263,193,115-byte download from
`bonsai.cedardb.com` (or the CWI mirror if that fails), a second streamed 1.26 GB read of the
other mirror to compare digests (nothing stored), about 5 GB written under
`M=/tb/Source/Academia/kermit-bench-runs/job-97/imdb-measure`, and a DuckDB load that the spec
expects to take a few minutes. Everything goes under `$M`, which is on disk: `/tmp` (so
`$SCRATCH`) is RAM-backed with about 8 GB free, so the dump, the extracted CSVs, the database
and DuckDB's spill directory (`$M/db/tmp`) never go there. **Nothing is written under
`~/.cache/kermit`.** Each step runs in a fresh shell, so every block below sets `M` itself, and
every `rm -rf` writes `"${M:?}"` so that an unset `M` deletes nothing. DuckDB is on `PATH` only
inside the dev shell (J3 added it to the flake), so every DuckDB call below goes through
`nix develop --command`. If any step fails or disagrees with its expected output, stop and
report the raw output to the design session; do not raise the 4 GB limit or edit the schema
to make a load pass.

Premises this part is the first to test on real data: that `schema.sql` loads with
`ESCAPE '\'` and no header and every CSV loads (its `NOT NULL` text columns and primary keys
included) under `memory_limit = '4GB'`; that both mirrors serve identical bytes; that the dump
has one CSV per schema table. The first run of `raw::build` on the real dump (J7 / J9) is the
same load, so a failure here is a finding about the spec, not about this task's code.

- [ ] **Step 15: Check the host, the disk and the go-ahead.**
  ```bash
  find /proc/[0-9]*/exe -maxdepth 0 -lname '*kermit-bench-runs*' 2>/dev/null | wc -l
  df -h /tb/Source/Academia/kermit-bench-runs | tail -1
  ```
  Expected: `0`, and at least 8 GB available (on `/tb`, not `/tmp`). Without the user's
  explicit go-ahead for the list above, stop here and report that Part B is pending.

- [ ] **Step 16: Download `imdb.tgz` into the measurement directory.**
  ```bash
  M=/tb/Source/Academia/kermit-bench-runs/job-97/imdb-measure
  mkdir -p "$M/dl" "$M/csv" "$M/db" "$M/scripts"
  for url in https://bonsai.cedardb.com/job/imdb.tgz http://event.cwi.nl/da/job/imdb.tgz; do
    if curl --fail --silent --show-error --location --output "$M/dl/imdb.tgz.part" "$url"; then
      mv "$M/dl/imdb.tgz.part" "$M/dl/imdb.tgz"
      echo "downloaded from $url"
      break
    fi
  done
  stat -c %s "$M/dl/imdb.tgz"
  ```
  Expected: `downloaded from …` and `1263193115`. Any other size: stop (the spec's premise is
  that both mirrors serve 1,263,193,115 bytes). Run it in the foreground only if the link is
  fast; otherwise start it with `setsid nohup bash -c '…' & disown` (the `bash -c` string
  starts with the same `M=…` line) and poll the `.part` file's size.

- [ ] **Step 17: Digest it.**
  ```bash
  M=/tb/Source/Academia/kermit-bench-runs/job-97/imdb-measure
  sha256sum "$M/dl/imdb.tgz" | tee "$M/imdb.sha256"
  ```
  Expected: one line, 64 lowercase hex digits and the path. This is `IMDB_SHA256`.

- [ ] **Step 18: Check the other mirror serves the same bytes (streamed, nothing stored).**
  ```bash
  curl --fail --silent --show-error --location http://event.cwi.nl/da/job/imdb.tgz | sha256sum
  ```
  (If Step 16 downloaded from the CWI mirror, stream `https://bonsai.cedardb.com/job/imdb.tgz`
  instead.) Expected: the same digest as Step 17. A different digest means `raw::ensure`'s
  mirror fallback (J7) would fail its integrity check on one mirror: report it; do not pin
  either digest silently.

- [ ] **Step 19: Fetch the pinned `schema.sql`, extract, and match tables to CSVs.**
  ```bash
  M=/tb/Source/Academia/kermit-bench-runs/job-97/imdb-measure
  curl --fail --silent --show-error --location --output "$M/schema.sql" \
    https://raw.githubusercontent.com/gregrahn/join-order-benchmark/a39603662e023e449cb2121997a5034df9e02ebf/schema.sql
  echo "1d10ba0c9a881e890aeb951c0960cc4e3472c9c9695ce1edb6493e9e4466d3d2  $M/schema.sql" | sha256sum --check
  tar -xzf "$M/dl/imdb.tgz" -C "$M/csv"
  ls "$M/csv" | wc -l
  diff <(ls "$M/csv" | sed 's/\.csv$//' | sort) \
       <(sed -n 's/^CREATE TABLE \([a-z_]*\) .*/\1/p' "$M/schema.sql" | sort) \
    && echo "21 tables, one CSV each"
  ```
  Run under bash (process substitution). Expected: `…/schema.sql: OK`, `21`, and `21 tables,
  one CSV each` with no `diff` output. The archive is untrusted data: it is extracted into its
  own new empty directory, and nothing is run from inside it.

- [ ] **Step 20: Write the load script.** Create `$M/scripts/load_imdb.sh` (outside the
  CSV directory). It loads the way `raw::build` does: the same session settings, the same
  `COPY` options, then a `COUNT(*)` per table. Inside the double quotes bash turns `'\\'` into
  the one-backslash literal `'\'` that DuckDB must receive. The script calls plain `duckdb`:
  Step 21 runs the whole script under `nix develop --command`, which is what puts DuckDB on
  `PATH`. Its spill directory (`<temp-dir>`) is `$M/db/tmp`, on disk:
  ```bash
  #!/usr/bin/env bash
  # Usage: load_imdb.sh <schema.sql> <csv-dir> <database> <temp-dir>
  # Loads the IMDb CSVs and prints "<table> <rows>" per table, in schema order.
  # Run it inside the dev shell: it needs `duckdb` on PATH.
  set -euo pipefail
  schema=$1 csv_dir=$2 db=$3 tmp=$4
  mkdir -p "$tmp"
  rm -f "$db" "$db.wal"
  prelude="SET memory_limit='4GB'; SET temp_directory='$tmp'; SET preserve_insertion_order=true; SET enable_progress_bar=false;"
  duckdb "$db" -c "$prelude $(cat "$schema")" > /dev/null
  tables=$(sed -n 's/^CREATE TABLE \([a-z_]*\) .*/\1/p' "$schema")
  for table in $tables; do
    echo "loading $table" >&2
    duckdb "$db" -c "$prelude COPY \"$table\" FROM '$csv_dir/$table.csv' (FORMAT csv, HEADER false, ESCAPE '\\');" > /dev/null
  done
  for table in $tables; do
    rows=$(duckdb -readonly "$db" -csv -noheader -c "$prelude SELECT COUNT(*) FROM \"$table\";")
    printf '%s %s\n' "$table" "$rows"
  done
  ```

- [ ] **Step 21: Load and count (long: minutes).** Run it detached with a pid file so a
  watcher can poll (`kill -0`), not `/proc` greps. Run it from the repository root (the flake
  lives there):
  ```bash
  M=/tb/Source/Academia/kermit-bench-runs/job-97/imdb-measure
  setsid nohup bash -c "nix develop --command bash '$M/scripts/load_imdb.sh' '$M/schema.sql' '$M/csv' '$M/db/imdb.duckdb' '$M/db/tmp' > '$M/table-rows.txt' 2> '$M/load.log'; echo \$? > '$M/load.exit'" > /dev/null 2>&1 &
  echo $! > "$M/load.pid"; disown
  ```
  Poll between other work (set `M` as above in that shell too):
  `kill -0 "$(cat "$M/load.pid")" 2>/dev/null && tail -1 "$M/load.log"`.
  When the process is gone:
  ```bash
  M=/tb/Source/Academia/kermit-bench-runs/job-97/imdb-measure
  cat "$M/load.exit"; wc -l < "$M/table-rows.txt"; cat "$M/table-rows.txt"
  nix develop --command duckdb --version | tee "$M/duckdb.version"
  ```
  Expected: `0`; `21`; 21 lines `<table> <rows>` in schema order starting `aka_name`, ending
  `title`, every count positive (the spec expects `cast_info` near 36 million); then the
  DuckDB version line (`v1.4.4 (Andium) 6ddac802ff` on the locked nixpkgs), which Steps 22
  and 26 record with the pins. A non-zero exit or a short file: stop and report
  `$M/load.log`.

- [ ] **Step 22: Write the measured values into `pins.rs`.**

  Generate the rows block:
  ```bash
  M=/tb/Source/Academia/kermit-bench-runs/job-97/imdb-measure
  awk '{ printf "    (\"%s\", %s),\n", $1, $2 }' "$M/table-rows.txt" > "$M/table-rows.rs.txt"
  cat "$M/table-rows.rs.txt"
  cut -d' ' -f1 "$M/imdb.sha256"
  cat "$M/duckdb.version"
  ```
  Then edit `kermit-generate/src/job/pins.rs` (Edit tool; paste the printed values, not the
  `<…>` markers). Replace the digest. Before:
  ```rust
  /// SHA-256 (lowercase hex) of `imdb.tgz`.
  ///
  /// `"UNMEASURED"` until Part B of the J4 task measures the dump; the download in the raw
  /// cache (J7) is checked against it.
  pub const IMDB_SHA256: &str = "UNMEASURED";
  ```
  After:
  ```rust
  /// SHA-256 (lowercase hex) of `imdb.tgz`, measured on 2026-10-NN from both mirrors in the
  /// run that measured `IMDB_TABLE_ROWS`, under DuckDB `<the version line printed above>`; the
  /// download in the raw cache is checked against it.
  pub const IMDB_SHA256: &str = "<the 64 digits printed above>";
  ```
  Replace the 21 placeholder lines of `IMDB_TABLE_ROWS`. Before:
  ```rust
  /// The row count of each of the 21 IMDb tables after loading `imdb.tgz`,
  /// in `schema.sql`'s order.
  ///
  /// All `0` until Part B of the J4 task measures them; the raw-cache build
  /// checks every loaded table against its entry.
  pub const IMDB_TABLE_ROWS: [(&str, u64); 21] = [
      ("aka_name", 0),
      …
      ("title", 0),
  ];
  ```
  After (the 21 lines are `$M/table-rows.rs.txt`, in the same order):
  ```rust
  /// The row count of each of the 21 IMDb tables after loading `imdb.tgz` with `schema.sql`
  /// under DuckDB (`<the version line printed above>`), in `schema.sql`'s order; the
  /// raw-cache build checks every loaded table against its entry.
  pub const IMDB_TABLE_ROWS: [(&str, u64); 21] = [
      ("aka_name", <rows>),
      …
      ("title", <rows>),
  ];
  ```
  Check that no placeholder is left:
  ```bash
  grep -c 'UNMEASURED\|", 0),\|<the \|<rows>' kermit-generate/src/job/pins.rs
  ```
  Expected: `0` (no placeholder and no unfilled `<…>` marker is left).

- [ ] **Step 23: Make the placeholders impossible to ship.** Add this test to `pins.rs`'s
  `mod tests` (after `imdb_table_rows_name_the_21_tables`):

```rust
    /// Part B of the J4 task replaced the placeholders: a 64-digit lowercase hex
    /// digest, and a non-empty table for each of the 21 names.
    #[test]
    fn imdb_pins_are_measured() {
        assert_eq!(IMDB_SHA256.len(), 64, "{IMDB_SHA256}");
        assert!(
            IMDB_SHA256.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
            "{IMDB_SHA256} is not lowercase hex"
        );
        for (table, rows) in IMDB_TABLE_ROWS {
            assert!(rows > 0, "{table} has no pinned row count");
        }
    }
```

- [ ] **Step 24: Run the `pins` tests; expect PASS.**
  ```bash
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-generate job::pins
  ```
  Expected: `test result: ok. 5 passed; 0 failed`.

- [ ] **Step 25: Format and lint.**
  ```bash
  nix develop --command cargo fmt --all
  nix develop --command env CARGO_BUILD_JOBS=2 RUSTFLAGS=-Dwarnings cargo clippy -p kermit-generate --all-targets
  ```
  Expected: clean.

- [ ] **Step 26: Commit Part B.** Stage the file by name and commit by pathspec (never a bare
  `git commit`, `-a` or `--amend`); the body records the DuckDB version line (read from
  `$M/duckdb.version`, so the history shows which build the row counts came from):

```bash
M=/tb/Source/Academia/kermit-bench-runs/job-97/imdb-measure
git add kermit-generate/src/job/pins.rs
git commit -m "feat(job): pin the IMDb dump's digest and table row counts (#97)" \
  -m "imdb.tgz is 1,263,193,115 bytes; both mirrors serve the same bytes. The row counts are what schema.sql plus the 21 CSVs load to under DuckDB $(cat "$M/duckdb.version") with a 4 GB memory limit and ESCAPE '\\'." \
  --trailer "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>" \
  --trailer "Claude-Session: https://claude.ai/code/session_01WeRnvdstSxUY8CJGSx1NFB" \
  -- kermit-generate/src/job/pins.rs
```

  Expected: one commit touching only `pins.rs`; `git log -1 --format=%b` shows the version line
  and `ESCAPE '\'` (one backslash). J7 can now start.

- [ ] **Step 27: Tidy the measurement directory.** The 5 GB are not needed again (J7 and J9
  rebuild the cache through `raw::ensure`):
  ```bash
  M=/tb/Source/Academia/kermit-bench-runs/job-97/imdb-measure
  rm -rf "${M:?}/csv" "${M:?}/db"
  ls "$M"
  ```
  Keep `$M/dl/imdb.tgz`, `imdb.sha256`, `table-rows.txt`, `duckdb.version` and `load.log` until J9, in case the
  user wants to point a local run at the archive. Report the digest, the 21 counts, the load
  time and the DuckDB version to the design session.

### Drafter B notes for the merge (J3, J4)

---

# Part 3: the generator and the wiring (J5, J6)

### Task J5: `JobGenerator`, the filter and count stages

`JobGenerator` implements #95's `Generator` for one JOB template. The **filter stage** writes one
relation file per `RelationPlan` of `plan_template`: it streams `filter_select(relation)` through
`Duckdb::stream_rows` into a `RelationWriter`, so memory is one 65,536-row batch here and at most
`MEMORY_LIMIT` in DuckDB. The **count stage** runs each rule's `count_sql` through
`Duckdb::query_u64`: DuckDB's `COUNT(*)` of the original query is the rule's `expected`, an oracle
independent of kermit's executors. `meta.json` gets the template, the pins the raw cache was built
from, the generation's DuckDB version and one `{relation, table, filter}` per file.

The integration test `kermit/tests/job_generate.rs` builds J4's fixture into a raw cache, generates
templates 1 and 2 through `kermit_generate::run`, reads the files back with `kermit_ds::read_parquet`
and joins them on every (structure, algorithm) cell.

- **Decision 1: `ORDER BY id` is tested with a second, reversed load.** The fixture's CSVs list
  every table in id order, and DuckDB returns a tiny table's rows in load order, so a `SELECT`
  that lost its `ORDER BY "id"` would still pass every test on the fixture as written. The tests
  that read row order (`relations_hold_filtered_rows_in_id_order`, `regeneration_is_byte_identical`)
  therefore also build a raw cache from a copy of `csv/` with the lines of every file reversed
  (`common::job::csv_dir(…, true)`). The expected rows and the generated bytes must not change.
  J4's fixture is not touched.
- **Decision 2: extras are strict about the provenance.** `query_sha256` holds `schema.sql` and the
  template's own `<name>.sql` files, copied from the raw cache's provenance. A file the provenance
  has no digest for is an error naming it, not a skipped entry. J7 records all 114 pinned files, so
  the error cannot fire on a real cache. The digests are checked **before** the filter stage, so a
  cache with a short provenance fails before any DuckDB process starts and writes no relation file
  (`a_provenance_without_a_query_digest_fails_before_anything_is_written`). `duckdb_version` is the
  **generation's** (`Duckdb::version`); the cache keeps its build's in `provenance.json`.
- **Decision 3: a failed run keeps the relations it finished.** `kermit_generate::run` does not
  delete files a failed `generate` finished (`meta.json` is the commit marker, and the sink has no
  delete). `failed_filter_leaves_no_relation_and_no_meta` therefore makes the **first** relation
  of its template fail, where "no relation `.parquet`" is literally true, and for a failure on the
  **last** relation asserts only what the layer promises: no `meta.json`, no `benchmark.yml`, no
  `.part`, no file for the failing relation.
- **Decision 4: the tests share one helper module.** `kermit/tests/common/job.rs` (new; one line in
  `common/mod.rs`) holds the fixture paths, `rows.txt` reading, the DuckDB skip, the reversed-CSV
  copy and the raw-cache build. J6's `cli_job_gen.rs` uses it too.
- **Decision 5: no shared static fixture.** Each test builds its own raw cache (about 40 DuckDB
  starts on 60 rows). A `static` would own a `TempDir` that is never dropped, leaking it.
- **Decision 6: J5 needs only #95's push 1.** Nothing here uses `GenTarget`; the contract's phase
  table puts J5 in phase 2 with J6, but J5 can run as soon as J1–J4 are committed.

**Needs:** J1–J4 Part A committed (translator, `Duckdb`, `pins`, `raw`, the fixture). Part B of J4
(the IMDb measurements) is not needed.

**Files:**
- Modify: `kermit-generate/src/job/mod.rs` (module docs, imports, `TEMPLATES`, `JobSource`,
  `JobExtras`, `JobGenerator`, `filter_select`; one unit test)
- Modify: `kermit/tests/common/mod.rs` (one module line)
- Create: `kermit/tests/common/job.rs`
- Create: `kermit/tests/job_generate.rs`
- Test: `kermit-generate/src/job/mod.rs` (`mod tests`), `kermit/tests/job_generate.rs`

- [ ] **Step 1: Check the host and the premises.**
  ```bash
  find /proc/[0-9]*/exe -maxdepth 0 -lname '*kermit-bench-runs*' 2>/dev/null | wc -l
  git log --oneline -4
  grep -n "pub use" -A8 kermit-generate/src/lib.rs
  grep -n "^pub mod" kermit-generate/src/job/mod.rs
  grep -n "pub struct RelationPlan\|pub join_columns\|pub struct RulePlan\|pub count_sql\|pub fn parse_query\|pub fn plan_template" kermit-generate/src/job/translate.rs
  grep -n "pub fn template_queries\|pub fn provenance\|pub fn database\|pub fn temp_dir\|pub fn dir\|pub fn build\|pub struct BuildInputs\|pub struct RawProvenance" kermit-generate/src/job/raw.rs
  grep -n "pub fn stream_rows\|pub fn query_u64\|pub fn version\|pub fn locate" kermit-generate/src/job/duckdb.rs
  grep -n "kermit-generate" kermit/Cargo.toml
  ls kermit-generate/tests/fixtures/job kermit-generate/tests/fixtures/job/queries
  ```
  Expected, in order:
  - `0`, and the co-ordinator has announced `FULL_EXIT`;
  - J4's Part A commit on top;
  - a `pub use { … }` block naming `Generated`, `Generator`, `GeneratedMeta`, `run`, `RelationSink`,
    `RelationWriter`, `RelationRecord` (and `output_dir`, `OutputDir`), plus `pub mod job;`;
  - `pub mod duckdb;`, `pub mod pins;`, `pub mod raw;`, `pub mod translate;`;
  - `RelationPlan` with a `pub join_columns` field, `RulePlan` with a `pub count_sql` field,
    `pub fn parse_query` and `pub fn plan_template`;
  - `template_queries`, `provenance`, `database`, `temp_dir`, `dir`, `pub fn build`, `BuildInputs`,
    `RawProvenance`;
  - `stream_rows`, `query_u64`, `version`, `locate`;
  - a `kermit-generate = { version = …, path = "../kermit-generate" }` line in `kermit/Cargo.toml`;
  - `csv  queries  rows.txt  schema.sql`, then `1a.sql  1b.sql  2a.sql  2b.sql`.

  Any difference means the contract moved since this plan was written: fix the plan text (and tell
  the design session) before Step 2. A missing `kermit-generate` dependency in `kermit/Cargo.toml`
  means #95's push 1 is not merged: stop.

- [ ] **Step 2: Write the failing tests.**

  **2a. The unit test.** In `kermit-generate/src/job/mod.rs`, append the test module after the last
  `pub mod` line (Step 4 inserts the code between the two). Edit. Before:
  ```rust
  pub mod translate;
  ```
  After:
  ```rust
  pub mod translate;

  #[cfg(test)]
  mod tests {
      use super::*;

      /// A relation plan of the given shape; `queries` plays no part in the `SELECT`.
      fn relation(
          name: &str, table: &str, columns: &[&str], join_columns: usize, filter: &str,
      ) -> RelationPlan {
          RelationPlan {
              name: name.to_owned(),
              table: table.to_owned(),
              columns: columns.iter().map(|column| (*column).to_owned()).collect(),
              join_columns,
              filter: filter.to_owned(),
              queries: vec!["1a".to_owned()],
          }
      }

      #[test]
      fn filter_select_shapes() {
          // Filtered, two join columns, then a carried id.
          let filtered = relation(
              "movie_info_mi1_a",
              "movie_info",
              &["info_type_id", "movie_id", "id"],
              2,
              "info IN ('Action', 'Drama')",
          );
          assert_eq!(
              filter_select(&filtered),
              concat!(
                  r#"SELECT CAST("info_type_id" AS BIGINT) AS "info_type_id", "#,
                  r#"CAST("movie_id" AS BIGINT) AS "movie_id", CAST("id" AS BIGINT) AS "id" "#,
                  r#"FROM "movie_info" WHERE info IN ('Action', 'Drama') "#,
                  r#"AND "info_type_id" IS NOT NULL AND "movie_id" IS NOT NULL ORDER BY "id""#,
              )
          );

          // Unfiltered: no filter conjunct, only the NULL checks.
          let unfiltered = relation(
              "movie_keyword",
              "movie_keyword",
              &["movie_id", "keyword_id", "id"],
              2,
              "",
          );
          assert_eq!(
              filter_select(&unfiltered),
              concat!(
                  r#"SELECT CAST("movie_id" AS BIGINT) AS "movie_id", "#,
                  r#"CAST("keyword_id" AS BIGINT) AS "keyword_id", CAST("id" AS BIGINT) AS "id" "#,
                  r#"FROM "movie_keyword" WHERE "movie_id" IS NOT NULL "#,
                  r#"AND "keyword_id" IS NOT NULL ORDER BY "id""#,
              )
          );

          // Id only: the id is the one join column and is checked like any other.
          let id_only = relation("title_t_a", "title", &["id"], 1, "production_year > 1990");
          assert_eq!(
              filter_select(&id_only),
              concat!(
                  r#"SELECT CAST("id" AS BIGINT) AS "id" FROM "title" "#,
                  r#"WHERE production_year > 1990 AND "id" IS NOT NULL ORDER BY "id""#,
              )
          );

          // The id first and a second join column after it: both are checked, in column
          // order, and a filter text holding double quotes passes through untouched.
          let id_first = relation(
              "title_t_a",
              "title",
              &["id", "kind_id"],
              2,
              r#"title LIKE '%"Hero"%'"#,
          );
          assert_eq!(
              filter_select(&id_first),
              concat!(
                  r#"SELECT CAST("id" AS BIGINT) AS "id", CAST("kind_id" AS BIGINT) AS "kind_id" "#,
                  r#"FROM "title" WHERE title LIKE '%"Hero"%' "#,
                  r#"AND "id" IS NOT NULL AND "kind_id" IS NOT NULL ORDER BY "id""#,
              )
          );

          // No join column and no filter (the planner never produces one): no WHERE at all,
          // never a dangling `WHERE`.
          let bare = relation("title", "title", &["id"], 0, "");
          assert_eq!(
              filter_select(&bare),
              r#"SELECT CAST("id" AS BIGINT) AS "id" FROM "title" ORDER BY "id""#
          );
      }
  }
  ```

  **2b. The shared helper module.** In `kermit/tests/common/mod.rs`, add the module. Before:
  ```rust
  #[allow(dead_code)]
  pub mod cli;
  pub mod macros;
  ```
  After:
  ```rust
  #[allow(dead_code)]
  pub mod cli;
  #[allow(dead_code)]
  pub mod job;
  pub mod macros;
  ```
  Create `kermit/tests/common/job.rs`:

```rust
//! The IMDb-shaped fixture of `kermit-generate`'s JOB generator
//! (`kermit-generate/tests/fixtures/job/`), built into a raw cache for the
//! DuckDB-gated tests (`job_generate.rs`, `cli_job_gen.rs`).
//!
//! Every cache is built in a directory the caller owns (a temp dir): nothing
//! here touches `~/.cache/kermit`.

use {
    kermit_generate::job::{
        duckdb::Duckdb,
        pins,
        raw::{self, BuildInputs, RawCache, RawProvenance},
    },
    std::{
        fs,
        path::{Path, PathBuf},
    },
};

/// `kermit-generate/tests/fixtures/job/`: `schema.sql`, `csv/`, `queries/`
/// and `rows.txt`.
pub fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../kermit-generate/tests/fixtures/job")
}

/// `rows.txt`: the row count each fixture table must load to, one
/// `<table> <rows>` pair per line.
pub fn fixture_rows() -> Vec<(String, u64)> {
    let path = fixture_dir().join("rows.txt");
    let text = fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let (table, rows) = line
                .split_once(' ')
                .expect("rows.txt lines are `<table> <rows>`");
            (table.to_owned(), rows.parse().expect("a row count"))
        })
        .collect()
}

/// The located DuckDB, or `None` after a stderr line saying why `test`
/// skipped: CI installs no DuckDB, as it installs no Java for the LUBM tests.
pub fn locate_or_skip(test: &str) -> Option<Duckdb> {
    match Duckdb::locate() {
        | Ok(duckdb) => Some(duckdb),
        | Err(error) => {
            eprintln!("skipping {test}: no DuckDB ({error:#})");
            None
        },
    }
}

/// The directory of fixture CSVs to load: the committed `csv/`, or (when
/// `reversed`) a copy in `scratch` with the lines of every file in reverse
/// order. DuckDB returns a table's rows in load order unless the query sorts
/// them, so only the reversed copy can tell a missing `ORDER BY`. Every
/// fixture record is one line, so reversing the lines reverses the rows.
pub fn csv_dir(scratch: &Path, reversed: bool) -> PathBuf {
    let source = fixture_dir().join("csv");
    if !reversed {
        return source;
    }
    let target = scratch.join("csv-reversed");
    fs::create_dir_all(&target).unwrap();
    for entry in fs::read_dir(&source).unwrap() {
        let path = entry.unwrap().path();
        let text = fs::read_to_string(&path).unwrap();
        let mut lines: Vec<&str> = text.lines().collect();
        lines.reverse();
        let mut reversed_text = lines.join("\n");
        reversed_text.push('\n');
        fs::write(target.join(path.file_name().unwrap()), reversed_text).unwrap();
    }
    target
}

/// Builds the raw cache at `dir` from the fixture's `schema.sql` and
/// `rows.txt`, the CSVs in `csv` and the query files in `queries_dir`.
///
/// # Panics
///
/// Panics if the build fails.
pub fn build_raw_cache(
    duckdb: &Duckdb, dir: &Path, csv: &Path, queries_dir: &Path, provenance: RawProvenance,
) -> RawCache {
    let rows = fixture_rows();
    let table_rows: Vec<(&str, u64)> = rows.iter().map(|(table, n)| (table.as_str(), *n)).collect();
    let schema_sql = fixture_dir().join("schema.sql");
    raw::build(duckdb, dir, &BuildInputs {
        csv_dir: csv,
        schema_sql: &schema_sql,
        queries_dir,
        table_rows: &table_rows,
        provenance,
    })
    .unwrap_or_else(|error| panic!("building the fixture raw cache: {error:#}"))
}

/// A provenance equal to the pins: the dump's digest, the query commit and
/// all 114 file digests. The fixture queries are not JOB's files, so the
/// digests describe nothing about them; the point is that a cache with this
/// provenance is one `raw::ensure` accepts as current, whether `ensure` only
/// opens a finished cache or also compares its provenance with the pins.
pub fn pinned_provenance(duckdb: &Duckdb) -> RawProvenance {
    RawProvenance {
        imdb_sha256: pins::IMDB_SHA256.to_owned(),
        query_commit: pins::QUERY_COMMIT.to_owned(),
        query_sha256: pins::QUERY_FILES
            .iter()
            .map(|(file, digest)| ((*file).to_owned(), (*digest).to_owned()))
            .collect(),
        duckdb_version: duckdb.version().to_owned(),
    }
}
```

  **2c. The integration tests.** Create `kermit/tests/job_generate.rs`:

```rust
//! The JOB generator end to end on the committed IMDb-shaped fixture
//! (`kermit-generate/tests/fixtures/job/`): a raw cache built from it,
//! templates 1 and 2 generated through `kermit_generate::run`, the relation
//! files read back with `kermit_ds::read_parquet`, and every query joined on
//! every (structure, algorithm) cell, optimiser and column-order policy.
//!
//! Needs DuckDB, so each test skips with a stderr line where there is none
//! (CI installs no DuckDB; the LUBM tests skip without Java the same way)
//! and runs under `nix develop`. Raw caches and outputs live in temp dirs.
//!
//! The expected relations and counts were derived by hand from the fixture's
//! CSVs. `kermit-generate`'s `fixture_queries_count_as_the_facts_say` has
//! DuckDB recompute the four counts. Query `2b` selects no name, so its
//! relation `name_n_b` is empty and its count is 0.

mod common;

use {
    clap::ValueEnum,
    common::{
        job::{build_raw_cache, csv_dir, fixture_dir, locate_or_skip},
        utils::{join_under_planner, load_parquet_relations, JoinEntry},
    },
    kermit_algos::{
        ColumnOrderPolicy, HashTriejoin, JoinQuery, LeapfrogTriejoin, Optimiser, Planner,
    },
    kermit_bench::BenchmarkDefinition,
    kermit_ds::{
        BinarySeek, Cardinality, ColumnTrie, GallopingSeek, HashTrie, LazyExpansion, LinearSeek,
        NoPruning, Relation, SingletonPruning, TreeTrie,
    },
    kermit_generate::{
        job::{
            duckdb::Duckdb,
            raw::{RawCache, RawProvenance},
            JobExtras, JobGenerator,
        },
        run, GeneratedMeta,
    },
    kermit_iters::{FxHashStrategy, SipHashStrategy},
    kermit_rdf::generator::Target,
    std::{
        collections::{BTreeMap, BTreeSet},
        fs,
        path::Path,
    },
};

type HashTrieSip = HashTrie<SipHashStrategy>;
type HashTrieFx = HashTrie<FxHashStrategy>;
type HashTrieSipPruned = HashTrie<SipHashStrategy, SingletonPruning>;
type HashTrieFxPruned = HashTrie<FxHashStrategy, SingletonPruning>;
type HashTrieSipLazy = HashTrie<SipHashStrategy, NoPruning, LazyExpansion>;
type HashTrieFxLazy = HashTrie<FxHashStrategy, NoPruning, LazyExpansion>;
type HashTrieSipPrunedLazy = HashTrie<SipHashStrategy, SingletonPruning, LazyExpansion>;
type HashTrieFxPrunedLazy = HashTrie<FxHashStrategy, SingletonPruning, LazyExpansion>;
type TreeTrieLinear = TreeTrie<LinearSeek>;
type TreeTrieBinary = TreeTrie<BinarySeek>;
type TreeTrieGalloping = TreeTrie<GallopingSeek>;
type ColumnTrieLinear = ColumnTrie<LinearSeek>;
type ColumnTrieBinary = ColumnTrie<BinarySeek>;
type ColumnTrieGalloping = ColumnTrie<GallopingSeek>;

/// The tag and spec hash every generation here passes in its `Target`.
const TAG: &str = "fixture";
const SPEC_HASH: &str = "job-generate-test-spec-hash";

/// The `COUNT(*)` of each fixture query, by hand.
const COUNTS: [(&str, u64); 4] = [("1a", 5), ("1b", 7), ("2a", 2), ("2b", 0)];

fn expected_count(query: &str) -> u64 {
    COUNTS
        .iter()
        .find(|(name, _)| *name == query)
        .unwrap_or_else(|| panic!("no hand count for query {query}"))
        .1
}

/// The rule of each fixture query, as the translator writes it:
/// `(template, query, Datalog)`.
const RULES: [(u8, &str, &str); 4] = [
    (
        1,
        "1a",
        "Q(IT1, IT2, T, K) :- info_type_it1_ab(IT1), info_type_it2_a(IT2), keyword_k_ab(K), \
         movie_info_mi1_a(IT1, T, _), movie_info(IT2, T, _), movie_keyword(T, K, _), \
         title_t_a(T).",
    ),
    (
        1,
        "1b",
        "Q(IT1, T, K) :- info_type_it1_ab(IT1), keyword_k_ab(K), movie_info_mi1_b(IT1, T, _), \
         movie_keyword(T, K, _), title_t_b(T).",
    ),
    (
        2,
        "2a",
        "Q(T, N, KT) :- cast_info(T, N, _), kind_type_kt_a(KT), name_n_a(N), title_t_a(T, KT).",
    ),
    (
        2,
        "2b",
        "Q(T, N) :- cast_info(T, N, _), name_n_b(N), title_t_b(T).",
    ),
];

/// One relation file the fixture predicts: its name, its columns and its
/// rows in file order (`ORDER BY id`).
struct Expect {
    name: &'static str,
    columns: &'static [&'static str],
    rows: &'static [&'static [usize]],
}

/// Template 1 (queries `1a`, `1b`), in the order the planner first uses
/// each relation.
const TEMPLATE_1: &[Expect] = &[
    Expect {
        name: "info_type_it1_ab",
        columns: &["id"],
        rows: &[&[1]],
    },
    Expect {
        name: "info_type_it2_a",
        columns: &["id"],
        rows: &[&[2]],
    },
    Expect {
        name: "keyword_k_ab",
        columns: &["id"],
        rows: &[&[1], &[2]],
    },
    // `info IN ('Action', 'Drama')`; row 14 (`Action`, NULL type) drops out.
    Expect {
        name: "movie_info_mi1_a",
        columns: &["info_type_id", "movie_id", "id"],
        rows: &[&[1, 1, 1], &[1, 2, 3], &[1, 3, 8], &[1, 4, 9], &[1, 7, 11]],
    },
    // Unfiltered; only row 14 drops out, for its NULL `info_type_id`.
    Expect {
        name: "movie_info",
        columns: &["info_type_id", "movie_id", "id"],
        rows: &[
            &[1, 1, 1],
            &[2, 1, 2],
            &[1, 2, 3],
            &[2, 2, 4],
            &[1, 2, 5],
            &[1, 3, 6],
            &[2, 3, 7],
            &[1, 3, 8],
            &[1, 4, 9],
            &[2, 4, 10],
            &[1, 7, 11],
            &[2, 7, 12],
            &[1, 8, 13],
            &[2, 2, 15],
            &[3, 1, 16],
        ],
    },
    Expect {
        name: "movie_keyword",
        columns: &["movie_id", "keyword_id", "id"],
        rows: &[
            &[1, 1, 1],
            &[2, 1, 2],
            &[2, 2, 3],
            &[3, 4, 4],
            &[4, 3, 5],
            &[8, 1, 6],
            &[7, 4, 7],
            &[3, 2, 8],
            &[1, 2, 9],
            &[7, 3, 10],
        ],
    },
    // `production_year > 1990`: 1990 itself, 1985, 1950 and the NULL year drop out.
    Expect {
        name: "title_t_a",
        columns: &["id"],
        rows: &[&[2], &[3], &[4], &[8], &[9]],
    },
    // `info NOT LIKE 'Doc%'`: row 6 (`Documentary`) and row 14 drop out.
    Expect {
        name: "movie_info_mi1_b",
        columns: &["info_type_id", "movie_id", "id"],
        rows: &[
            &[1, 1, 1],
            &[2, 1, 2],
            &[1, 2, 3],
            &[2, 2, 4],
            &[1, 2, 5],
            &[2, 3, 7],
            &[1, 3, 8],
            &[1, 4, 9],
            &[2, 4, 10],
            &[1, 7, 11],
            &[2, 7, 12],
            &[1, 8, 13],
            &[2, 2, 15],
            &[3, 1, 16],
        ],
    },
    // `production_year BETWEEN 1980 AND 1995`: both bounds are kept (1995 is title 2).
    Expect {
        name: "title_t_b",
        columns: &["id"],
        rows: &[&[1], &[2], &[3], &[7]],
    },
];

/// Template 2 (queries `2a`, `2b`).
const TEMPLATE_2: &[Expect] = &[
    // Unfiltered; row 3 drops out for its NULL `person_id`.
    Expect {
        name: "cast_info",
        columns: &["movie_id", "person_id", "id"],
        rows: &[
            &[2, 2, 1],
            &[2, 4, 2],
            &[2, 1, 4],
            &[3, 2, 5],
            &[9, 4, 6],
            &[9, 2, 7],
            &[1, 4, 8],
            &[8, 3, 9],
        ],
    },
    Expect {
        name: "kind_type_kt_a",
        columns: &["id"],
        rows: &[&[1]],
    },
    Expect {
        name: "name_n_a",
        columns: &["id"],
        rows: &[&[2], &[4]],
    },
    // The two titles holding `"Hero"` (the CSV's backslash-escaped quotes), with their kinds.
    Expect {
        name: "title_t_a",
        columns: &["id", "kind_id"],
        rows: &[&[2, 1], &[9, 2]],
    },
    // `name LIKE '%Zed%'` matches no name: the file keeps its header and has no rows.
    Expect {
        name: "name_n_b",
        columns: &["id"],
        rows: &[],
    },
    Expect {
        name: "title_t_b",
        columns: &["id"],
        rows: &[&[2], &[3], &[4], &[8], &[9]],
    },
];

/// Provenance values no real cache has, so every extras assertion can tell a
/// field from its neighbour.
fn provenance() -> RawProvenance {
    RawProvenance {
        imdb_sha256: "a".repeat(64),
        query_commit: "fixture-commit".to_owned(),
        query_sha256: [
            ("schema.sql", "b"),
            ("1a.sql", "c"),
            ("1b.sql", "d"),
            ("2a.sql", "e"),
            ("2b.sql", "f"),
        ]
        .into_iter()
        .map(|(file, digit)| (file.to_owned(), digit.repeat(64)))
        .collect(),
        duckdb_version: "built-with-another-duckdb".to_owned(),
    }
}

/// A raw cache built from the fixture, and the DuckDB that built it.
struct Fixture {
    /// Owns the raw cache's directory.
    _scratch: tempfile::TempDir,
    duckdb: Duckdb,
    raw: RawCache,
}

impl Fixture {
    /// The fixture with its CSVs loaded as written, or (when `reversed`) in
    /// reverse order; `None`, after a stderr line, without DuckDB.
    fn build(test: &str, reversed: bool) -> Option<Fixture> {
        Fixture::build_with(test, reversed, &fixture_dir().join("queries"))
    }

    /// As [`Fixture::build`], with the query files taken from `queries_dir`.
    fn build_with(test: &str, reversed: bool, queries_dir: &Path) -> Option<Fixture> {
        Fixture::build_recording(test, reversed, queries_dir, provenance())
    }

    /// As [`Fixture::build_with`], with `recorded` written to the cache's
    /// `provenance.json` in place of `provenance()`.
    fn build_recording(
        test: &str, reversed: bool, queries_dir: &Path, recorded: RawProvenance,
    ) -> Option<Fixture> {
        let duckdb = locate_or_skip(test)?;
        let scratch = tempfile::tempdir().unwrap();
        let csv = csv_dir(scratch.path(), reversed);
        let raw = build_raw_cache(
            &duckdb,
            &scratch.path().join("imdb"),
            &csv,
            queries_dir,
            recorded,
        );
        Some(Fixture {
            _scratch: scratch,
            duckdb,
            raw,
        })
    }

    /// Generates `template` into `out` through `kermit_generate::run`.
    fn try_generate(
        &self, template: u8, out: &Path,
    ) -> anyhow::Result<GeneratedMeta<JobExtras>> {
        let bench_name = format!("job-{template}");
        let generator = JobGenerator {
            template,
            raw: &self.raw,
            duckdb: &self.duckdb,
            target: Target {
                out_dir: out,
                bench_name: &bench_name,
                tag: TAG,
                spec_hash: Some(SPEC_HASH),
            },
        };
        run(&generator)
    }

    /// As [`Fixture::try_generate`], for a generation that must succeed.
    fn generate(&self, template: u8, out: &Path) -> GeneratedMeta<JobExtras> {
        self.try_generate(template, out)
            .unwrap_or_else(|error| panic!("generating template {template}: {error:#}"))
    }
}

/// Every `.parquet` file in `dir`, by stem, with its bytes.
fn parquet_files(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    fs::read_dir(dir)
        .unwrap()
        .filter_map(|entry| {
            let path = entry.unwrap().path();
            (path.extension()? == "parquet").then(|| {
                let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
                (stem, fs::read(&path).unwrap())
            })
        })
        .collect()
}

/// The stems of the `.parquet` files in `dir`.
fn parquet_stems(dir: &Path) -> BTreeSet<String> { parquet_files(dir).into_keys().collect() }

/// The names of everything in `dir`, sorted.
fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// The `benchmark.yml` a generation wrote into `dir`, validated.
fn emitted_benchmark(dir: &Path) -> BenchmarkDefinition {
    let yaml = fs::read_to_string(dir.join("benchmark.yml")).expect("benchmark.yml is missing");
    let bench: BenchmarkDefinition = serde_yaml::from_str(&yaml).expect("benchmark.yml is malformed");
    bench.validate().expect("benchmark.yml must validate");
    bench
}

#[test]
fn relations_hold_filtered_rows_in_id_order() {
    // Once with the CSVs as written and once loaded in reverse id order: DuckDB
    // returns rows in load order unless the query sorts, so only the second pass
    // notices an `ORDER BY "id"` going missing.
    for reversed in [false, true] {
        let Some(fixture) = Fixture::build("relations_hold_filtered_rows_in_id_order", reversed)
        else {
            return;
        };
        for (template, expected) in [(1u8, TEMPLATE_1), (2, TEMPLATE_2)] {
            let context = format!("template {template}, reversed load = {reversed}");
            let out = tempfile::tempdir().unwrap();
            let meta = fixture.generate(template, out.path());

            // The files are exactly the planned relations, recorded in plan order.
            let planned: BTreeSet<String> = expected.iter().map(|e| e.name.to_owned()).collect();
            assert_eq!(parquet_stems(out.path()), planned, "{context}");
            let recorded: Vec<(&str, u64)> = meta
                .relations
                .iter()
                .map(|record| (record.name.as_str(), record.rows))
                .collect();
            let want: Vec<(&str, u64)> = expected
                .iter()
                .map(|e| (e.name, e.rows.len() as u64))
                .collect();
            assert_eq!(recorded, want, "{context}");

            for e in expected {
                let path = out.path().join(format!("{}.parquet", e.name));
                let (header, tuples) = kermit_ds::read_parquet(&path)
                    .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
                assert_eq!(header.name(), e.name, "{context}");
                assert_eq!(header.attrs(), e.columns, "{} ({context})", e.name);
                let rows: Vec<Vec<usize>> = e.rows.iter().map(|row| row.to_vec()).collect();
                assert_eq!(tuples.to_vecs(), rows, "{} ({context})", e.name);
            }
        }
    }
}

/// kermit's count of `bench`'s queries over the relations in `dir`, joined as
/// `R` by `JA` under every optimiser and column-order policy, against each
/// query's `expected` (DuckDB's `COUNT(*)`). One line per disagreement.
fn mismatches<R: Relation + Cardinality, JA: JoinEntry<R>>(
    bench: &BenchmarkDefinition, dir: &Path,
) -> Vec<String> {
    let mut found = Vec::new();
    for &policy in ColumnOrderPolicy::value_variants() {
        for &optimiser in Optimiser::value_variants() {
            let planner = Planner::new(optimiser.instantiate(), policy);
            let label = format!(
                "{} / {} / {}",
                std::any::type_name::<R>(),
                optimiser.axis_value(),
                policy.axis_value()
            );
            let (relations, inputs) = load_parquet_relations::<R>(
                dir,
                bench.relations.iter().map(|relation| relation.name.as_str()),
            );
            let mut database = JA::database(relations, planner.required_statistics());
            for query in &bench.queries {
                let parsed: JoinQuery = query.query.parse().expect("generated Datalog parses");
                let want = query.expected.expect("every JOB query carries its count");
                match join_under_planner::<R, JA>(&mut database, &inputs, parsed, &planner) {
                    | Ok((rows, _)) if rows.len() as u64 == want => {},
                    | Ok((rows, _)) => found.push(format!(
                        "[{label}] {}: got {}, expected {want}",
                        query.name,
                        rows.len()
                    )),
                    | Err(error) => found.push(format!("[{label}] {}: {error}", query.name)),
                }
            }
        }
    }
    found
}

/// One sweep per (structure, algorithm) cell: the six sorted tries under
/// LFTJ and the eight HashTrie Layouts under Hash Triejoin.
fn cells() -> [fn(&BenchmarkDefinition, &Path) -> Vec<String>; 14] {
    [
        mismatches::<TreeTrieLinear, LeapfrogTriejoin>,
        mismatches::<TreeTrieBinary, LeapfrogTriejoin>,
        mismatches::<TreeTrieGalloping, LeapfrogTriejoin>,
        mismatches::<ColumnTrieLinear, LeapfrogTriejoin>,
        mismatches::<ColumnTrieBinary, LeapfrogTriejoin>,
        mismatches::<ColumnTrieGalloping, LeapfrogTriejoin>,
        mismatches::<HashTrieSip, HashTriejoin>,
        mismatches::<HashTrieFx, HashTriejoin>,
        mismatches::<HashTrieSipPruned, HashTriejoin>,
        mismatches::<HashTrieFxPruned, HashTriejoin>,
        mismatches::<HashTrieSipLazy, HashTriejoin>,
        mismatches::<HashTrieFxLazy, HashTriejoin>,
        mismatches::<HashTrieSipPrunedLazy, HashTriejoin>,
        mismatches::<HashTrieFxPrunedLazy, HashTriejoin>,
    ]
}

#[test]
fn counts_match_duckdb_on_every_cell() {
    let Some(fixture) = Fixture::build("counts_match_duckdb_on_every_cell", false) else {
        return;
    };
    let mut found = Vec::new();
    for template in [1u8, 2] {
        let out = tempfile::tempdir().unwrap();
        fixture.generate(template, out.path());
        let bench = emitted_benchmark(out.path());
        // DuckDB's count is the oracle; it must itself equal the hand count.
        for query in &bench.queries {
            assert_eq!(
                query.expected,
                Some(expected_count(&query.name)),
                "query {}: DuckDB's COUNT(*) disagrees with the hand count",
                query.name
            );
        }
        for sweep in cells() {
            found.extend(sweep(&bench, out.path()));
        }
    }
    assert!(
        found.is_empty(),
        "{} count mismatches across 14 cells x {} optimisers x {} column-order policies x 2 \
         templates:\n{}",
        found.len(),
        Optimiser::value_variants().len(),
        ColumnOrderPolicy::value_variants().len(),
        found.join("\n")
    );
}

#[test]
fn regeneration_is_byte_identical() {
    let Some(fixture) = Fixture::build("regeneration_is_byte_identical", false) else {
        return;
    };
    // The same data loaded in the opposite order: the files must not depend on the
    // order DuckDB happens to hold the rows in.
    let Some(reversed) = Fixture::build("regeneration_is_byte_identical", true) else {
        return;
    };
    for template in [1u8, 2] {
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        let third = tempfile::tempdir().unwrap();
        let a = fixture.generate(template, first.path());
        let b = fixture.generate(template, second.path());
        let c = reversed.generate(template, third.path());

        let files = parquet_files(first.path());
        assert!(!files.is_empty(), "template {template} wrote no relation");
        for (label, dir) in [("second run", second.path()), ("reversed load", third.path())] {
            let other = parquet_files(dir);
            assert_eq!(
                files.keys().collect::<Vec<_>>(),
                other.keys().collect::<Vec<_>>(),
                "template {template}, {label}: different files"
            );
            for (name, bytes) in &files {
                assert!(
                    *bytes == other[name],
                    "template {template}, {label}: {name}.parquet differs"
                );
            }
        }
        // The hashes the layer records agree with the bytes on disk, in all three runs...
        for (meta, dir) in [(&a, first.path()), (&b, second.path()), (&c, third.path())] {
            for record in &meta.relations {
                let path = dir.join(format!("{}.parquet", record.name));
                let on_disk = kermit_bench::cache::sha256_hex(&path)
                    .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
                assert_eq!(
                    record.sha256, on_disk,
                    "template {template}: the recorded hash of {} is not its file's",
                    record.name
                );
            }
        }
        // ...and so they agree between the runs.
        let hashes = |meta: &GeneratedMeta<JobExtras>| -> Vec<(String, String)> {
            meta.relations
                .iter()
                .map(|record| (record.name.clone(), record.sha256.clone()))
                .collect()
        };
        assert_eq!(hashes(&a), hashes(&b), "template {template}");
        assert_eq!(hashes(&a), hashes(&c), "template {template}");
    }
}

/// Two queries filtering on a column the tables lack: `1a`'s first relation
/// (`cast_info_ci_a`) and `2a`'s last (`title_t_a`) fail in DuckDB.
fn write_broken_queries(dir: &Path) {
    fs::write(
        dir.join("1a.sql"),
        "SELECT MIN(t.title) AS movie_title\n\
         FROM cast_info AS ci,\n     title AS t\n\
         WHERE ci.no_such_column = 1\n  AND ci.movie_id = t.id;\n",
    )
    .unwrap();
    fs::write(
        dir.join("2a.sql"),
        "SELECT MIN(t.title) AS movie_title\n\
         FROM cast_info AS ci,\n     title AS t\n\
         WHERE t.no_such_column = 1\n  AND ci.movie_id = t.id;\n",
    )
    .unwrap();
}

#[test]
fn failed_filter_leaves_no_relation_and_no_meta() {
    let queries = tempfile::tempdir().unwrap();
    write_broken_queries(queries.path());
    let Some(fixture) = Fixture::build_with(
        "failed_filter_leaves_no_relation_and_no_meta",
        false,
        queries.path(),
    ) else {
        return;
    };

    // Template 1: the first relation fails, so nothing at all is written. A stale
    // `meta.json` planted first must be gone: `run` removes the old commit marker
    // before it generates.
    let out = tempfile::tempdir().unwrap();
    fs::write(out.path().join("meta.json"), "{}").unwrap();
    let message = format!("{:#}", fixture.try_generate(1, out.path()).unwrap_err());
    assert!(message.contains("cast_info_ci_a"), "{message}");
    assert!(message.contains("no_such_column"), "{message}");
    assert_eq!(
        names(out.path()),
        Vec::<String>::new(),
        "no relation file, finished or partial, and no stale meta.json"
    );

    // Template 2: the last relation fails. Relations finished before the failure may
    // stay (`meta.json`'s absence is what marks the output incomplete), but the
    // failing one leaves neither a file nor a `.part`, and nothing commits.
    let out = tempfile::tempdir().unwrap();
    fs::write(out.path().join("meta.json"), "{}").unwrap();
    let message = format!("{:#}", fixture.try_generate(2, out.path()).unwrap_err());
    assert!(message.contains("title_t_a"), "{message}");
    assert!(message.contains("no_such_column"), "{message}");
    let left = names(out.path());
    assert!(
        left.iter().all(|name| name.ends_with(".parquet")),
        "only finished relations may remain, not meta.json, benchmark.yml or a .part: {left:?}"
    );
    assert!(!left.contains(&"title_t_a.parquet".to_owned()), "{left:?}");
}

#[test]
fn unknown_templates_are_errors() {
    let Some(fixture) = Fixture::build("unknown_templates_are_errors", false) else {
        return;
    };
    // Outside 1..=33 is refused before any query is read.
    for template in [0u8, 34] {
        let out = tempfile::tempdir().unwrap();
        let message = format!("{:#}", fixture.try_generate(template, out.path()).unwrap_err());
        assert!(message.contains("1..=33"), "template {template}: {message}");
        assert_eq!(names(out.path()), Vec::<String>::new());
    }
    // Inside the range but absent from the cache (the fixture holds 1 and 2).
    let out = tempfile::tempdir().unwrap();
    let message = format!("{:#}", fixture.try_generate(3, out.path()).unwrap_err());
    assert!(message.contains("no query files"), "{message}");
    assert!(message.contains("template 3"), "{message}");
    assert_eq!(names(out.path()), Vec::<String>::new());
}

#[test]
fn extras_record_sources_and_versions() {
    let Some(fixture) = Fixture::build("extras_record_sources_and_versions", false) else {
        return;
    };
    let out = tempfile::tempdir().unwrap();
    let meta = fixture.generate(1, out.path());

    assert_eq!(meta.kind, "job");
    assert_eq!(meta.tag, TAG);
    assert_eq!(meta.spec_hash.as_deref(), Some(SPEC_HASH));
    assert_eq!((meta.relation_count, meta.query_count), (9, 2));

    let extras = &meta.extras;
    assert_eq!(extras.template, 1);
    assert_eq!(extras.imdb_sha256, "a".repeat(64));
    assert_eq!(extras.query_commit, "fixture-commit");
    // The generation's DuckDB, not the one the cache was built with.
    assert_eq!(extras.duckdb_version, fixture.duckdb.version());
    assert_ne!(extras.duckdb_version, provenance().duckdb_version);
    // `schema.sql` and this template's queries, not template 2's.
    let files: Vec<&str> = extras.query_sha256.keys().map(String::as_str).collect();
    assert_eq!(files, ["1a.sql", "1b.sql", "schema.sql"]);
    assert_eq!(extras.query_sha256["schema.sql"], "b".repeat(64));
    assert_eq!(extras.query_sha256["1a.sql"], "c".repeat(64));
    assert_eq!(extras.query_sha256["1b.sql"], "d".repeat(64));

    let sources: Vec<(&str, &str, &str)> = extras
        .sources
        .iter()
        .map(|s| (s.relation.as_str(), s.table.as_str(), s.filter.as_str()))
        .collect();
    assert_eq!(sources, [
        ("info_type_it1_ab", "info_type", "info = 'genres'"),
        ("info_type_it2_a", "info_type", "info = 'rating'"),
        ("keyword_k_ab", "keyword", "keyword LIKE '%hero%'"),
        ("movie_info_mi1_a", "movie_info", "info IN ('Action', 'Drama')"),
        ("movie_info", "movie_info", ""),
        ("movie_keyword", "movie_keyword", ""),
        ("title_t_a", "title", "production_year > 1990"),
        ("movie_info_mi1_b", "movie_info", "info NOT LIKE 'Doc%'"),
        ("title_t_b", "title", "production_year BETWEEN 1980 AND 1995"),
    ]);
    // `sources` joins the common `relations` list by name, in the same order.
    let recorded: Vec<&str> = meta.relations.iter().map(|r| r.name.as_str()).collect();
    let named: Vec<&str> = extras.sources.iter().map(|s| s.relation.as_str()).collect();
    assert_eq!(recorded, named);

    // meta.json holds the same, with the extras flattened beside the common fields.
    let text = fs::read_to_string(out.path().join("meta.json")).unwrap();
    let on_disk: GeneratedMeta<JobExtras> = serde_json::from_str(&text).unwrap();
    assert_eq!(on_disk, meta);
    let value: serde_json::Value = serde_json::from_str(&text).unwrap();
    for key in [
        "template",
        "imdb_sha256",
        "query_commit",
        "query_sha256",
        "duckdb_version",
        "sources",
        "relations",
    ] {
        assert!(value.get(key).is_some(), "meta.json has no `{key}`: {text}");
    }
}

/// The provenance must hold a digest for `schema.sql` and for every query file
/// of the template: a file without one is an error naming it, raised before the
/// filter stage, so no relation is written and nothing commits.
#[test]
fn a_provenance_without_a_query_digest_fails_before_anything_is_written() {
    let mut recorded = provenance();
    recorded.query_sha256.remove("1b.sql");
    let Some(fixture) = Fixture::build_recording(
        "a_provenance_without_a_query_digest_fails_before_anything_is_written",
        false,
        &fixture_dir().join("queries"),
        recorded,
    ) else {
        return;
    };

    // Template 1 reads 1b.sql. A stale meta.json planted first is gone (`run` removes
    // the old commit marker), and no relation file was written.
    let out = tempfile::tempdir().unwrap();
    fs::write(out.path().join("meta.json"), "{}").unwrap();
    let message = format!("{:#}", fixture.try_generate(1, out.path()).unwrap_err());
    assert!(message.contains("records no SHA-256 for 1b.sql"), "{message}");
    assert_eq!(
        names(out.path()),
        Vec::<String>::new(),
        "no relation file, no meta.json and no benchmark.yml"
    );

    // Template 2 reads 2a.sql and 2b.sql, whose digests are present: the gap does not
    // touch it.
    let out = tempfile::tempdir().unwrap();
    fixture.generate(2, out.path());
}

#[test]
fn benchmark_yml_carries_the_rules_and_counts() {
    let Some(fixture) = Fixture::build("benchmark_yml_carries_the_rules_and_counts", false) else {
        return;
    };
    for (template, expected) in [(1u8, TEMPLATE_1), (2, TEMPLATE_2)] {
        let out = tempfile::tempdir().unwrap();
        let meta = fixture.generate(template, out.path());
        let bench = emitted_benchmark(out.path());

        assert_eq!(bench.name, format!("job-{template}"));
        assert_eq!(
            bench.description,
            format!(
                "Join Order Benchmark template {template} (Leis et al., VLDB 2015) over IMDb, \
                 filters applied at generation"
            )
        );
        assert!(
            bench.generator.is_none(),
            "the cache-side benchmark.yml carries no generator block"
        );

        let named: Vec<&str> = bench.relations.iter().map(|r| r.name.as_str()).collect();
        let planned: Vec<&str> = expected.iter().map(|e| e.name).collect();
        assert_eq!(named, planned);
        for (source, record) in bench.relations.iter().zip(&meta.relations) {
            assert_eq!(
                source.sha256.as_deref(),
                Some(record.sha256.as_str()),
                "{}",
                source.name
            );
            let url = source.url.as_deref().expect("a relation URL");
            assert!(
                url.starts_with("file://") && url.ends_with(&format!("/{}.parquet", source.name)),
                "{url}"
            );
        }

        let rules: Vec<(&str, &str)> = RULES
            .iter()
            .filter(|(owner, _, _)| *owner == template)
            .map(|(_, name, datalog)| (*name, *datalog))
            .collect();
        let queries: Vec<(&str, &str)> = bench
            .queries
            .iter()
            .map(|q| (q.name.as_str(), q.query.as_str()))
            .collect();
        assert_eq!(queries, rules);
        for query in &bench.queries {
            assert_eq!(
                query.description,
                format!("JOB {}, filters applied at generation", query.name)
            );
            assert_eq!(query.expected, Some(expected_count(&query.name)));
        }
    }
}
```

- [ ] **Step 3: Run the tests; expect compile failures.**
  ```bash
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-generate --lib job::tests 2>&1 | tail -20
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit --test job_generate --no-run 2>&1 | tail -30
  ```
  Expected: the first FAILS to compile with `cannot find function `filter_select`` and `cannot find
  type `RelationPlan``; the second with `unresolved imports `kermit_generate::job::JobExtras``,
  `` `kermit_generate::job::JobGenerator` `` (E0432). No test runs.

- [ ] **Step 4: Implement the generator.**

  **4a. The module docs.** Add the new bullets to `job/mod.rs`'s module documentation. Edit.
  Before (J1's wording; J3 and J4 did not change the docs):
  ```rust
  //!   over `sqlparser`; it reads no data.
  //!
  //! The design is in
  ```
  After:
  ```rust
  //!   over `sqlparser`; it reads no data.
  //! - [`duckdb`]: the DuckDB command-line client every filter, count and load
  //!   goes through.
  //! - [`pins`]: the mirrors, the query commit and the digests the raw cache is
  //!   checked against.
  //! - [`raw`]: the shared raw IMDb cache (`imdb.duckdb`, the query files and
  //!   `provenance.json`).
  //! - [`JobGenerator`]: one template, generated from a raw cache through the
  //!   shared `Generator` layer. The filter stage streams DuckDB's filtered rows
  //!   into each relation file; the count stage records DuckDB's `COUNT(*)` of
  //!   each query as the rule's expected cardinality.
  //!
  //! The design is in
  ```
  If the Edit fails because J1's wording differs, add the same four bullets after the `translate`
  bullet by hand.

  **4b. The code.** Insert this block between the last `pub mod` line and `#[cfg(test)]` (Step
  2a put the test module right after `pub mod translate;`):

```rust
use {
    crate::{
        job::{
            duckdb::Duckdb,
            raw::RawCache,
            translate::{parse_query, plan_template, RelationPlan},
        },
        Generated, Generator, RelationSink,
    },
    anyhow::{ensure, Context},
    kermit_bench::QueryDefinition,
    kermit_ds::{RelationHeader, Tuples},
    kermit_rdf::generator::Target,
    serde::{Deserialize, Serialize},
    std::{collections::BTreeMap, iter, ops::RangeInclusive},
};

/// The JOB templates, `1` to `33`. A template holds the two to six variants
/// (`6a` … `6f`) of one query shape (Leis et al., "How Good Are Query
/// Optimizers, Really?", VLDB 2015); `bench gen job` and `generator: { kind:
/// job }` name one.
pub const TEMPLATES: RangeInclusive<u8> = 1..=33;

/// The name `schema.sql` has among a raw cache's pinned files; every
/// template's `query_sha256` records its digest beside its own queries'.
const SCHEMA_FILE: &str = "schema.sql";

/// Where one relation file's rows come from.
///
/// `meta.json` records one per relation file, in the order the files were
/// written; `relation` joins it by name to the common `relations` list, which
/// holds each file's columns, row count and SHA-256.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobSource {
    /// The relation's name, also its file stem.
    pub relation: String,
    /// The IMDb table its rows are read from.
    pub table: String,
    /// The filter applied to the table, with the alias stripped from its
    /// columns; empty when the relation is unfiltered.
    pub filter: String,
}

/// The JOB-specific fields of `meta.json`, flattened beside the common ones:
/// what the template's relations were generated from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobExtras {
    /// The template, `1` to `33`.
    pub template: u8,
    /// SHA-256 of the `imdb.tgz` the raw cache was built from.
    pub imdb_sha256: String,
    /// The commit of `gregrahn/join-order-benchmark` the queries were read at.
    pub query_commit: String,
    /// SHA-256 of `schema.sql` and of each of this template's query files, by
    /// file name.
    pub query_sha256: BTreeMap<String, String>,
    /// `duckdb --version` of this generation; the raw cache records its
    /// build's separately.
    pub duckdb_version: String,
    /// Where each relation file's rows come from, in the order the files
    /// were written.
    pub sources: Vec<JobSource>,
}

/// Generates one JOB template from a raw IMDb cache.
///
/// The filter stage writes one relation file per distinct (table, columns,
/// filter) among the template's variants: DuckDB selects the rows that pass
/// the filter and have no NULL join key, in `ORDER BY id` order, and the rows
/// stream to the relation's writer in batches, so no relation is held whole.
/// The count stage runs each variant's `COUNT(*)` as the rule's `expected`
/// cardinality, an oracle independent of kermit's executors. The design is in
/// `docs/specs/2026-10-15-job-benchmark-design.md`.
pub struct JobGenerator<'a> {
    /// The template to generate, one of [`TEMPLATES`].
    pub template: u8,
    /// The raw cache the filters and counts run against.
    pub raw: &'a RawCache,
    /// The DuckDB client.
    pub duckdb: &'a Duckdb,
    /// Output directory, benchmark name, tag and spec hash.
    pub target: Target<'a>,
}

/// The `SELECT` that the filter stage streams for one relation.
///
/// Every column is cast to `BIGINT` (`read_parquet` reads only `Int64`); the
/// rows are those that pass the relation's filter and have no NULL in a join
/// column (an SQL equi-join excludes them); and `ORDER BY "id"` fixes their
/// order, which a parallel scan does not promise and which the tries' build
/// cost and byte-identical regeneration depend on. The filter is the text of
/// conjuncts joined by `AND`, each an operand of the query's top-level `AND`,
/// so an `OR` inside one is parenthesised in the source and appending `AND …`
/// cannot change its meaning. Identifiers are quoted; they are IMDb column
/// names, which never contain `"`.
pub fn filter_select(relation: &RelationPlan) -> String {
    let projection = relation
        .columns
        .iter()
        .map(|column| format!("CAST(\"{column}\" AS BIGINT) AS \"{column}\""))
        .collect::<Vec<_>>()
        .join(", ");
    let filter = (!relation.filter.is_empty()).then(|| relation.filter.clone());
    let not_null = relation
        .columns
        .iter()
        .take(relation.join_columns)
        .map(|column| format!("\"{column}\" IS NOT NULL"));
    let conditions: Vec<String> = filter.into_iter().chain(not_null).collect();
    let predicate = if conditions.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", conditions.join(" AND "))
    };
    format!("SELECT {projection} FROM \"{}\"{predicate} ORDER BY \"id\"", relation.table)
}

impl Generator for JobGenerator<'_> {
    const KIND: &'static str = "job";
    type Extras = JobExtras;

    fn target(&self) -> Target<'_> { self.target }

    fn generate(&self, sink: &mut RelationSink) -> anyhow::Result<Generated<JobExtras>> {
        ensure!(
            TEMPLATES.contains(&self.template),
            "JOB has templates {}..={}, not {}",
            TEMPLATES.start(),
            TEMPLATES.end(),
            self.template
        );
        let files = self.raw.template_queries(self.template)?;
        ensure!(
            !files.is_empty(),
            "the raw cache at {} holds no query files for JOB template {}",
            self.raw.dir().display(),
            self.template
        );
        let queries = files
            .iter()
            .map(|(name, sql)| parse_query(name, sql))
            .collect::<Result<Vec<_>, _>>()?;
        let plan = plan_template(&queries)?;

        // The pins the cache was built from, narrowed to this template's files. Checked
        // before any DuckDB process starts: a cache whose provenance lacks one of the
        // digests fails here, with no relation written.
        let provenance = self.raw.provenance()?;
        let mut query_sha256 = BTreeMap::new();
        let pinned_files = iter::once(SCHEMA_FILE.to_owned())
            .chain(files.iter().map(|(name, _)| format!("{name}.sql")));
        for file in pinned_files {
            let digest = provenance.query_sha256.get(&file).with_context(|| {
                format!(
                    "the raw cache at {} records no SHA-256 for {file}",
                    self.raw.dir().display()
                )
            })?;
            query_sha256.insert(file, digest.clone());
        }

        let database = self.raw.database();
        let temp_dir = self.raw.temp_dir();

        // Filter stage: one DuckDB process per relation, streamed into its writer. The
        // writer is finished only after DuckDB exited 0 with an empty stderr
        // (`stream_rows` fails otherwise), so a failed filter leaves no file for it.
        for relation in &plan.relations {
            let header = RelationHeader::new(relation.name.clone(), relation.columns.clone());
            let mut writer = sink.open(&header)?;
            self.duckdb
                .stream_rows(
                    &database,
                    &temp_dir,
                    &filter_select(relation),
                    relation.columns.len(),
                    &mut |batch: &Tuples| writer.write_batch(batch),
                )
                .with_context(|| {
                    format!(
                        "filtering relation `{}` from table `{}`",
                        relation.name, relation.table
                    )
                })?;
            writer.finish()?;
        }

        // Count stage: DuckDB's COUNT(*) of the original query is the rule's expected
        // cardinality.
        let mut definitions = Vec::with_capacity(plan.rules.len());
        for rule in &plan.rules {
            let expected = self
                .duckdb
                .query_u64(&database, &temp_dir, &rule.count_sql)
                .with_context(|| format!("counting JOB query {}", rule.query))?;
            definitions.push(QueryDefinition {
                name: rule.query.clone(),
                description: format!("JOB {}, filters applied at generation", rule.query),
                query: rule.datalog.clone(),
                expected: Some(expected),
            });
        }

        let sources = plan
            .relations
            .iter()
            .map(|relation| JobSource {
                relation: relation.name.clone(),
                table: relation.table.clone(),
                filter: relation.filter.clone(),
            })
            .collect();

        Ok(Generated {
            description: format!(
                "Join Order Benchmark template {} (Leis et al., VLDB 2015) over IMDb, filters \
                 applied at generation",
                self.template
            ),
            queries: definitions,
            extras: JobExtras {
                template: self.template,
                imdb_sha256: provenance.imdb_sha256,
                query_commit: provenance.query_commit,
                query_sha256,
                duckdb_version: self.duckdb.version().to_owned(),
                sources,
            },
        })
    }
}
```

- [ ] **Step 5: Run the unit test; expect PASS.**
  ```bash
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-generate --lib job::tests
  ```
  Expected: `test result: ok. 1 passed; 0 failed`.

- [ ] **Step 6: Run the integration tests; expect PASS.**
  ```bash
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit --test job_generate -- --nocapture 2>&1 | tail -30
  ```
  Expected: `test result: ok. 8 passed; 0 failed` and no `skipping` line (DuckDB is in the dev
  shell). Without DuckDB the eight tests print `skipping … no DuckDB` and pass vacuously: that is
  not a pass for this step.

  If something fails:
  - A cell **panics or errors on `name_n_b`** (the empty relation) or on a lazy HashTrie: that is a
    finding about the structure, not a J5 defect. Stop and report it to the design session; do not
    touch a sibling structure (Priority 6).
  - `relations_hold_filtered_rows_in_id_order` shows other rows: compare the filter and the failing
    relation with the fixture-facts table in J4 Step 4 before changing either side.
  - `extras_record_sources_and_versions` fails on `query_sha256`: the raw cache's provenance (J4's
    `build` takes it as given) and the digests in `provenance()` above must agree.
  - `a_provenance_without_a_query_digest_fails_before_anything_is_written` finds relation files:
    the digest check in `generate` has slipped below the filter stage.

- [ ] **Step 7: Format, lint, document.**
  ```bash
  nix develop --command cargo fmt --all
  nix develop --command env CARGO_BUILD_JOBS=2 RUSTFLAGS=-Dwarnings cargo clippy -p kermit-generate -p kermit --all-targets
  nix develop --command env CARGO_BUILD_JOBS=2 RUSTDOCFLAGS=-Dwarnings cargo doc -p kermit-generate --no-deps
  git status --short
  ```
  Expected: clean, and `git status --short` lists exactly the four files of this task (`cargo fmt`
  rewraps the code above; keep its output). A `doc_markdown` warning means a doc comment needs
  backticks around a code-like word.

- [ ] **Step 8: Commit.** Stage the four files by name, then commit by pathspec (never a bare
  `git commit`, `-a` or `--amend`):

```bash
git add kermit-generate/src/job/mod.rs kermit/tests/common/mod.rs kermit/tests/common/job.rs kermit/tests/job_generate.rs
git commit -m "feat(job): generate a JOB template from the raw cache (#97)" \
  -m "JobGenerator implements the shared Generator. The filter stage streams DuckDB's rows (no NULL join key, ORDER BY id) into each relation file in 65,536-row batches; the count stage records DuckDB's COUNT(*) of each query as the rule's expected cardinality. meta.json records the template, the pins, the generation's DuckDB version and where each relation comes from. The fixture pipeline test runs templates 1 and 2 on every structure, algorithm, optimiser and column-order policy, and checks the files are byte-identical across runs and load orders." \
  --trailer "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>" \
  --trailer "Claude-Session: https://claude.ai/code/session_01WeRnvdstSxUY8CJGSx1NFB" \
  -- kermit-generate/src/job/mod.rs kermit/tests/common/mod.rs kermit/tests/common/job.rs kermit/tests/job_generate.rs
```

  Expected: one commit touching exactly those four files.

- [ ] **Step 9: Mutation checks.** The commit is in. For each row: apply the exact edit with the
  Edit tool, confirm `git diff --stat` shows it, run the named command and see exactly the named
  tests fail, **reverse the exact edit** (never `git checkout`), re-run to green, and confirm
  `git status --short` is empty.

  | # | File and exact edit | Command | Must fail |
  | --- | --- | --- | --- |
  | 1 | `job/mod.rs`, in `filter_select`: `format!("SELECT {projection} FROM \"{}\"{predicate} ORDER BY \"id\"", relation.table)` becomes `format!("SELECT {projection} FROM \"{}\"{predicate}", relation.table)` | `cargo test -p kermit-generate --lib job::tests` and `cargo test -p kermit --test job_generate` | `filter_select_shapes`; `relations_hold_filtered_rows_in_id_order` (the reversed pass); `regeneration_is_byte_identical` (the reversed load) |
  | 2 | `job/mod.rs`, in `filter_select`: `.take(relation.join_columns)` becomes `.take(0)` | both | `filter_select_shapes`; `relations_hold_filtered_rows_in_id_order`; `counts_match_duckdb_on_every_cell`; `regeneration_is_byte_identical`; `extras_record_sources_and_versions`; `benchmark_yml_carries_the_rules_and_counts` (DuckDB prints a NULL as an empty field, and `stream_rows` rejects the line naming its number) |
  | 3 | `job/mod.rs`, in `filter_select`: `let filter = (!relation.filter.is_empty()).then(\|\| relation.filter.clone());` becomes `let filter: Option<String> = None;` | both | `filter_select_shapes`; `relations_hold_filtered_rows_in_id_order`; `counts_match_duckdb_on_every_cell` |
  | 4 | `job/mod.rs`, in `generate`: `duckdb_version: self.duckdb.version().to_owned(),` becomes `duckdb_version: provenance.duckdb_version.clone(),` | `cargo test -p kermit --test job_generate` | `extras_record_sources_and_versions` |
  | 5 | `job/mod.rs`, in `generate`: `table: relation.table.clone(),` becomes `table: relation.name.clone(),` | same | `extras_record_sources_and_versions` |
  | 6 | `job/mod.rs`, in `generate`: `expected: Some(expected),` becomes `expected: None,` | same | `counts_match_duckdb_on_every_cell`; `benchmark_yml_carries_the_rules_and_counts` |
  | 7 | `job/mod.rs`, in `generate`: move the digest block (from `let provenance = self.raw.provenance()?;` through the closing brace of `for file in pinned_files { … }`, with its comment) to just above `let sources = plan`, i.e. below the count stage (two Edits: delete, then insert) | same | `a_provenance_without_a_query_digest_fails_before_anything_is_written` (the relation files the filter stage wrote before the error) |

  If mutant 1 does **not** fail `relations_hold_filtered_rows_in_id_order` (DuckDB re-sorted the
  reversed load by itself), say so in the hand-off: the ordering test is then vacuous and the
  design session needs to know before relying on `ORDER BY id`.

---

### Task J6: `GeneratorSpec::Job`, `bench gen job` and the 33 YAMLs

J6 makes the generator reachable. kermit-bench learns the `kind: job` spec; the binary's
`materialize::dispatch` calls `run_job` for it (DuckDB located, the raw cache from `raw::ensure`, a
`JobGenerator`, `kermit_generate::run`); `bench gen job --template N --tag T` does the same without
a spec hash; `benchmarks/job-1.yml` … `job-33.yml` declare the 33 benchmarks. Three commits, each
leaving the tree green: **Part A** (spec, `ensure` stub, `dispatch` arm), **Part B**
(`bench gen job`), **Part C** (the YAMLs and the end-to-end `bench run` test).

- **Decision 1: the `ensure` stub is J6's, and it is deliberately untested.** It opens a finished
  cache or fails, naming the directory and saying the download arrives later (J7). J7 deletes the
  doc comment and the function (its Step 4b) and replaces them with the download. A test of the
  missing-cache branch would, after J7, try to download 1.26 GB, so none is written. The stub is
  covered by what J7 keeps true: Parts B and C's CLI tests run `ensure` over a fixture cache, built
  with a provenance **equal to the pins** (`common::job::pinned_provenance`), which J7's pins check
  also accepts. The stub's parameter is `_duckdb` (unused until J7); the signature at call sites is
  the contract's.
- **Decision 2: the error chain is printed by the shared site, not by the JOB arm.** `materialize`
  wraps a `dispatch` failure in `format!("generator pipeline failed: {e}")` (materialize.rs:228 at
  5dd1691), and an `anyhow::Error`'s `{}` shows only its outermost message, hiding DuckDB's stderr.
  The benchmark co-ordinator routed the fix: the first `kermit-generate` arm to land (#95 push 2 or
  #96 phase 2) changes that line to `{e:#}`. JOB lands after both, so its arm returns `run_job`'s
  error unchanged; Step 1's premise check confirms the shared line reads `{e:#}`, and if it does
  not, stop and report to the co-ordinator rather than re-wrapping here.
- **Decision 3: the template range lives twice, and a test ties the copies.** kermit-bench is a
  leaf and cannot name `kermit_generate::job::TEMPLATES`, so `validate_generator` has its own
  `JOB_TEMPLATES`. `the_spec_and_the_generator_agree_on_the_template_range` (in the binary, which
  sees both) checks every `u8`.
- **Decision 4: `run_gen_job` checks the range itself, and locates DuckDB before it creates the
  directory.** The contract has `run_gen_job` check the range (the #78 precedent for `--scale 0`
  is a clap `value_parser` range; the contract's form needs no literal `33` in an attribute). It
  runs first, so a bad template writes nothing. DuckDB is located before `GenTarget::prepare`
  creates `job-N-TAG`, so a host without it fails without leaving an empty directory in the cache.
  The raw cache is ensured after `prepare`, because `prepare` can refuse a colliding name in
  milliseconds and `ensure` can take hours.
- **Decision 5: the CLI tests redirect the cache.** Every test that runs `bench gen` or `bench run`
  sets `XDG_CACHE_HOME` (Linux only; elsewhere they skip, as the other CLI tests do) and passes
  `--output-dir` or `KERMIT_WORKSPACE`, so nothing is written under `~/.cache/kermit`. The
  DuckDB-gated tests put the fixture raw cache at `<XDG_CACHE_HOME>/kermit/imdb`, where
  `raw::default_raw_dir()` looks. The cache-hit check runs with `KERMIT_DUCKDB_BIN` pointing at a
  missing file: a hit never starts DuckDB, a regeneration would fail, and nothing can download.
- **Decision 6: the YAML descriptions are derived from `pins.rs`.** Each file reads
  `description: "Join Order Benchmark, template N (Na-Nx)"`, `x` being the last variant letter
  `QUERY_FILES` lists for N. J8's docs copy the line printed by Step 17 for `job-6.yml`.

**Needs:** J4 Part A (the `pins` table the YAML script reads, the fixture) and J5 (`JobGenerator`,
`common/job.rs`) committed. **Phase gate:** #95's `GenTarget` (push 2, or #96's phase 2) on
`origin/master` and merged into `aidanb/job`. Parts B and C stop without it; Part A does not need it.

**Files:**
- Modify: `kermit-bench/src/definition.rs` (`GeneratorSpec::Job`, `JOB_TEMPLATES`, the validation
  arm, tests)
- Modify: `kermit-generate/src/job/raw.rs` (the `ensure` stub)
- Modify: `kermit/src/materialize.rs` (imports, the `dispatch` arm, `run_job`, tests)
- Modify: `kermit/src/main.rs` (`GenSubcommand::Job`, `run_gen_job`, the arm in `main`)
- Create: `kermit/tests/cli_job_gen.rs`
- Create: `benchmarks/job-1.yml` … `benchmarks/job-33.yml`
- Test: all of the above

- [ ] **Step 1: Check the host and the premises.**
  ```bash
  find /proc/[0-9]*/exe -maxdepth 0 -lname '*kermit-bench-runs*' 2>/dev/null | wc -l
  git log --oneline -4
  grep -n "struct GenTarget\|fn prepare" kermit/src/main.rs
  grep -n "generator pipeline failed" kermit/src/materialize.rs
  grep -n "kermit-generate" kermit/Cargo.toml
  grep -n "pub fn ensure" kermit-generate/src/job/raw.rs
  grep -n "Job" kermit-bench/src/definition.rs
  grep -n "H2o\|Snap" kermit-bench/src/definition.rs kermit/src/materialize.rs kermit/src/main.rs
  grep -n "pub struct JobGenerator\|pub const TEMPLATES\|pub fn filter_select" kermit-generate/src/job/mod.rs
  grep -n "pub fn default_raw_dir\|pub fn open\|^const PROVENANCE_FILE" kermit-generate/src/job/raw.rs
  grep -n '("1a.sql"\|("33c.sql"' kermit-generate/src/job/pins.rs
  grep -n "pub fn kermit_bin" kermit/tests/common/cli.rs
  ```
  Expected, in order:
  - `0`, and `FULL_EXIT` announced;
  - J5's commit on top;
  - `materialize.rs`'s `generator pipeline failed` line formats with `{e:#}` (the co-ordinator's routed shared fix; if it still reads `{e}`, stop and report to the co-ordinator, never re-wrap in the JOB arm).
  - `struct GenTarget` and `fn prepare`, **exactly the code below**. If they are absent, #95 push 2
    and #96 phase 2 are both unmerged: **stop** (Part A does not need them; Parts B and C do). If
    present but different (field names, `prepare`'s signature or its stderr notes), fix this task's
    Part B before implementing it:

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
        fn prepare(&self, kind: &str, bench_name: &str) -> anyhow::Result<PathBuf> { … }
    }
    ```
    (Quoted from #95's spec, `docs/specs/2026-10-15-h2o-benchmark-design.md` at 41537a4, § Surface;
    the body is elided here, only `tag`, `output_dir` and `prepare`'s signature matter.)
  - a `kermit-generate = { version = …, path = "../kermit-generate" }` line;
  - **no output** for `pub fn ensure` (J6 writes the stub; if J7 has landed first, its `ensure`
    stays and Part A skips Step 5);
  - **no output** for `Job` in kermit-bench;
  - `H2o`/`Snap` lines in the three files once #95 push 2 or #96 phase 2 is merged (J6 runs after
    one of them; no lines means the tree is as quoted in 4c and 6b). JOB's variant and its arms go
    **after the last existing one** in every file: 4c's and 6b's anchors are the text after the
    last arm, so they match with or without H2O/SNAP arms before them;
  - `JobGenerator`, `TEMPLATES`, `filter_select` (J5);
  - `default_raw_dir`, `open`, `const PROVENANCE_FILE`;
  - the `1a.sql` line and the `33c.sql` line of `QUERY_FILES`;
  - `pub fn kermit_bin`.

### Part A: the spec, the `ensure` stub and the `dispatch` arm

- [ ] **Step 2: Write the failing tests.**

  **2a. kermit-bench.** In `kermit-bench/src/definition.rs`, add the tests to `mod tests`, after
  `spec_hash_distinguishes_lubm_query_subset` and before the NIST test. Edit. Before:
  ```rust
          assert_ne!(a.spec_hash(), b.spec_hash());
      }

      /// Pins the encoding against the NIST SHA-256 vector for `"abc"`. The
  ```
  After:
  ```rust
          assert_ne!(a.spec_hash(), b.spec_hash());
      }

      #[test]
      fn job_spec_parses() {
          let yaml = r#"
  name: job-6
  description: "Join Order Benchmark, template 6 (6a-6f)"
  generator:
    kind: job
    template: 6
  "#;
          let def: BenchmarkDefinition = serde_yaml::from_str(yaml).unwrap();
          def.validate().unwrap();
          assert_eq!(def.generator, Some(GeneratorSpec::Job { template: 6 }));
      }

      /// `template` has no default and must fit a byte: a missing, oversized,
      /// negative or non-numeric one is a parse error, not a silent template 0.
      #[test]
      fn job_spec_requires_a_template_that_fits_a_byte() {
          for generator in [
              "kind: job",
              "kind: job\n  template: 256",
              "kind: job\n  template: -1",
              "kind: job\n  template: six",
          ] {
              let yaml = format!("name: job-x\ndescription: \"x\"\ngenerator:\n  {generator}\n");
              assert!(
                  serde_yaml::from_str::<BenchmarkDefinition>(&yaml).is_err(),
                  "{generator}"
              );
          }
      }

      #[test]
      fn job_template_out_of_range_is_rejected() {
          for template in [0u8, 34, 255] {
              let spec = GeneratorSpec::Job { template };
              let message = validate_generator("job-x", &spec).unwrap_err().to_string();
              assert!(
                  message.contains("job generator template must be in 1..=33"),
                  "{message}"
              );
          }
          for template in [1u8, 17, 33] {
              let spec = GeneratorSpec::Job { template };
              assert!(validate_generator("job-x", &spec).is_ok(), "{template}");
          }
      }

      #[test]
      fn job_spec_hash_changes_with_template() {
          let one = GeneratorSpec::Job { template: 1 };
          let two = GeneratorSpec::Job { template: 2 };
          assert_ne!(one.spec_hash(), two.spec_hash());
          assert_eq!(one.spec_hash(), GeneratorSpec::Job { template: 1 }.spec_hash());
          // The kind is part of the hash: scale 1 of another kind is not template 1.
          assert_ne!(
              one.spec_hash(),
              GeneratorSpec::WatdivBasic { scale: 1 }.spec_hash()
          );
      }

      /// Pins the encoding against the NIST SHA-256 vector for `"abc"`. The
  ```

  **2b. The binary.** In `kermit/src/materialize.rs`, add two tests to `mod tests`, after
  `cache_only_current_lubm_cache_passes_through`. Edit. Before:
  ```rust
          let out = materialize(def, dir.path(), false).unwrap();
          assert_eq!(out.name, "lubm-1-new");
      }
  ```
  After:
  ```rust
          let out = materialize(def, dir.path(), false).unwrap();
          assert_eq!(out.name, "lubm-1-new");
      }

      fn job_def(name: &str, template: u8) -> BenchmarkDefinition {
          BenchmarkDefinition {
              name: name.to_string(),
              description: "x".to_string(),
              relations: vec![],
              queries: vec![],
              generator: Some(GeneratorSpec::Job { template }),
          }
      }

      /// A JOB cache is judged like any generator's: `meta.json` carries the
      /// spec hash `kermit_generate::run` writes, so the same template is a hit
      /// and another is drift (never a silent reuse, never a silent rebuild),
      /// and `kind: job` has no outdated-encoding floor.
      #[test]
      fn a_job_cache_is_a_hit_for_its_template_and_drift_for_another() {
          let dir = tempfile::tempdir().unwrap();
          let subdir = dir.path().join("job-6");
          write_meta_and_yaml(&subdir, None);
          let meta = serde_json::json!({
              "schema_version": kermit_rdf::generator::META_SCHEMA_VERSION,
              "kind": "job",
              "spec_hash": GeneratorSpec::Job { template: 6 }.spec_hash(),
          });
          fs::write(subdir.join("meta.json"), meta.to_string()).unwrap();

          let hit = materialize(job_def("job-6", 6), dir.path(), false).unwrap();
          assert!(hit.generator.is_none(), "the cached YAML is post-materialization");
          assert_eq!(hit.relations.len(), 1);

          match materialize(job_def("job-6", 7), dir.path(), false).unwrap_err() {
              | BenchError::SpecDrift {
                  name, ..
              } => assert_eq!(name, "job-6"),
              | other => panic!("expected SpecDrift, got {other:?}"),
          }
          assert!(subdir.exists(), "drift must not delete the cache");
      }

      /// kermit-bench validates a template against its own copy of the range
      /// (it cannot name `kermit_generate::job::TEMPLATES`); the two must agree
      /// on every byte.
      #[test]
      fn the_spec_and_the_generator_agree_on_the_template_range() {
          for template in 0..=u8::MAX {
              let valid = job_def(&format!("job-{template}"), template).validate().is_ok();
              assert_eq!(
                  valid,
                  kermit_generate::job::TEMPLATES.contains(&template),
                  "template {template}"
              );
          }
      }
  ```

- [ ] **Step 3: Run the tests; expect compile failures.**
  ```bash
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-bench job 2>&1 | tail -20
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit --bin kermit materialize::tests::a_job 2>&1 | tail -20
  ```
  Expected: both FAIL to compile: `no variant or associated item named `Job` found for enum
  `GeneratorSpec``.

- [ ] **Step 4: Add the spec.**

  In `kermit-bench/src/definition.rs`. **4a. The range constant.** Edit. Before:
  ```rust
  const LUBM_QUERY_COUNT: u32 = 14;
  ```
  After:
  ```rust
  const LUBM_QUERY_COUNT: u32 = 14;

  /// The JOB templates a `kind: job` spec may name: the benchmark has 33 (Leis
  /// et al., VLDB 2015). Mirrors `kermit_generate::job::TEMPLATES`, which this
  /// crate cannot name (it is a leaf); a test in the `kermit` binary pins the
  /// two together.
  const JOB_TEMPLATES: std::ops::RangeInclusive<u8> = 1..=33;
  ```
  **4b. The variant.** Edit (append it after the last variant; if H2O or SNAP variants are
  already there, put `Job` after them). Before:
  ```rust
          queries: Option<Vec<String>>,
      },
  }

  /// WatDiv stress-template parameters.
  ```
  After:
  ```rust
          queries: Option<Vec<String>>,
      },
      /// Drives the Join Order Benchmark generator
      /// (`kermit_generate::job::JobGenerator`): every variant of one JOB
      /// template over IMDb, with each alias's filters applied during
      /// generation. Unlike the other kinds it has no `scale`; the template
      /// picks the queries and so the data.
      Job {
          /// The JOB template, `1` to `33`.
          template: u8,
      },
  }

  /// WatDiv stress-template parameters.
  ```
  **4b (docs). Two doc comments `Job` makes stale.** Both name only the `kermit-rdf` kinds. Check
  first with
  `grep -n "kind: watdiv. or\|corresponding .kermit-rdf" kermit-bench/src/definition.rs`.
  If #95 or #96 already widened a comment, keep that wording and only make sure it covers
  `kind: job` and the `kermit-generate` generators. The `GeneratorSpec` doc. Before:
  ```rust
  /// A declarative generator spec. Tagged on the `kind` field
  /// (`kind: watdiv` or `kind: lubm`).
  ```
  After:
  ```rust
  /// A declarative generator spec. Tagged on the `kind` field; each variant
  /// names one generator and carries its parameters (for example `kind: watdiv`,
  /// `kind: lubm` or `kind: job`).
  ```
  The `generator` field doc on `BenchmarkDefinition`. Before:
  ```rust
      /// Optional declarative generator spec. When present, `bench run` runs
      /// the corresponding `kermit-rdf` pipeline on first invocation and
      /// caches the artefacts; subsequent runs short-circuit on the cached
      /// `meta.json` if the spec hash matches.
  ```
  After:
  ```rust
      /// Optional declarative generator spec. When present, `bench run` runs
      /// the corresponding generator (a `kermit-rdf` pipeline or a
      /// `kermit-generate` one) on first invocation and caches the artefacts;
      /// subsequent runs short-circuit on the cached `meta.json` if the spec
      /// hash matches.
  ```
  **4c. The validation arm.** Edit, anchored on the text after the match's last arm
  (`validate_generator`'s closing: the match's `}`, `Ok(())`, the function's `}`, and the next
  function's name), so it matches whether `Lubm` or a later H2O/SNAP arm is last. The new arm goes
  in as the **last** arm: if H2O/SNAP arms are already there, it follows them. Verify the anchor in
  `definition.rs` first (`grep -n "fn is_valid_lubm_query_name" -B4 kermit-bench/src/definition.rs`).
  Before:
  ```rust
      }
      Ok(())
  }

  fn is_valid_lubm_query_name
  ```
  After:
  ```rust
          | GeneratorSpec::Job {
              template,
          } => {
              if !JOB_TEMPLATES.contains(template) {
                  return Err(BenchError::Invalid {
                      name: bench_name.to_string(),
                      reason: format!(
                          "job generator template must be in {}..={}",
                          JOB_TEMPLATES.start(),
                          JOB_TEMPLATES.end()
                      ),
                  });
              }
          },
      }
      Ok(())
  }

  fn is_valid_lubm_query_name
  ```

- [ ] **Step 5: Add the `ensure` stub.**

  In `kermit-generate/src/job/raw.rs`, insert it between `extract_tgz` and J4's test module (skip
  this step if Step 1 found a `pub fn ensure`). Edit. Before:
  ```rust
  #[cfg(test)]
  mod tests {
  ```
  After:
  ```rust
  /// The IMDb raw cache at `dir`, if a finished one is there.
  ///
  /// This is the stand-in until the download lands (#97's last phase): it opens
  /// the cache or fails, and never builds one. The failure says the cache is
  /// missing; [`build`] makes one from a directory of CSVs. The signature is
  /// the final one, so callers do not change when the download replaces the
  /// body.
  ///
  /// # Errors
  ///
  /// Fails when `dir` holds no finished cache (no `provenance.json`).
  pub fn ensure(_duckdb: &Duckdb, dir: &Path) -> anyhow::Result<RawCache> {
      RawCache::open(dir).ok_or_else(|| {
          anyhow!(
              "the IMDb raw cache at {} is missing (no finished cache with {PROVENANCE_FILE}); \
               downloading it arrives with the next phase of #97 (J7)",
              dir.display()
          )
      })
  }

  #[cfg(test)]
  mod tests {
  ```
  (`anyhow!` and `Duckdb` are already imported by J4's `use` block. The doc comment must stay
  directly above `pub fn ensure`: J7 deletes the comment and the function together.)

- [ ] **Step 6: Add the `dispatch` arm and `run_job`.**

  In `kermit/src/materialize.rs`. **6a. Imports.** Edit. Before:
  ```rust
      kermit_bench::{BenchError, BenchmarkDefinition, GeneratorSpec, WatdivStressSpec},
      kermit_rdf::generator::MetaHeader,
  ```
  After:
  ```rust
      kermit_bench::{BenchError, BenchmarkDefinition, GeneratorSpec, WatdivStressSpec},
      kermit_generate::job::{duckdb::Duckdb, raw, JobGenerator},
      kermit_rdf::generator::{MetaHeader, Target},
  ```
  (If H2O's or SNAP's imports are already in the block, add JOB's names beside them in the same
  style.)

  **6b. The arm.** Edit, anchored on the match's closing and the next function's name, so it
  matches whether `Lubm` or a later H2O/SNAP arm is last. The new arm goes in as the **last** arm:
  if H2O/SNAP arms are already there, it follows them. Verify the anchor in `materialize.rs` first
  (`grep -n "^fn run_watdiv(" -B4 kermit/src/materialize.rs`). Before:
  ```rust
      }
  }

  fn run_watdiv(
  ```
  After:
  ```rust
          | GeneratorSpec::Job {
              template,
          } => run_job(*template, bench_name, out_dir, spec_hash),
      }
  }

  fn run_watdiv(
  ```

  **6c. `run_job`.** Edit (insert before the test module). Before:
  ```rust
  #[cfg(test)]
  mod tests {
      use {
          super::*,
  ```
  After:
  ```rust
  /// Generates one JOB template (`kind: job`) into `out_dir` through the shared
  /// layer. DuckDB and the IMDb raw cache are found here, on a cache miss, so a
  /// cache hit needs neither.
  fn run_job(
      template: u8, bench_name: &str, out_dir: &Path, spec_hash: &str,
  ) -> anyhow::Result<()> {
      let duckdb = Duckdb::locate()?;
      let cache = raw::ensure(&duckdb, &raw::default_raw_dir()?)?;
      let generator = JobGenerator {
          template,
          raw: &cache,
          duckdb: &duckdb,
          target: Target {
              out_dir,
              bench_name,
              tag: bench_name,
              spec_hash: Some(spec_hash),
          },
      };
      kermit_generate::run(&generator)?;
      Ok(())
  }

  #[cfg(test)]
  mod tests {
      use {
          super::*,
  ```

- [ ] **Step 7: Run the tests; expect PASS.**
  ```bash
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-bench job
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit --bin kermit materialize::tests
  ```
  Expected: kermit-bench `test result: ok. 4 passed; 0 failed` (`job_spec_parses`,
  `job_spec_requires_a_template_that_fits_a_byte`, `job_template_out_of_range_is_rejected`,
  `job_spec_hash_changes_with_template`); the binary's `materialize::tests` all pass, the two new
  ones named `a_job_cache_is_a_hit_for_its_template_and_drift_for_another` and
  `the_spec_and_the_generator_agree_on_the_template_range`.

- [ ] **Step 8: Format, lint, document, commit Part A.**
  ```bash
  nix develop --command cargo fmt --all
  nix develop --command env CARGO_BUILD_JOBS=2 RUSTFLAGS=-Dwarnings cargo clippy -p kermit-bench -p kermit-generate -p kermit --all-targets
  nix develop --command env CARGO_BUILD_JOBS=2 RUSTDOCFLAGS=-Dwarnings cargo doc -p kermit-bench -p kermit-generate --no-deps
  git status --short
  ```
  Expected: clean; `git status --short` lists exactly `kermit-bench/src/definition.rs`,
  `kermit-generate/src/job/raw.rs` and `kermit/src/materialize.rs`. Then:

```bash
git add kermit-bench/src/definition.rs kermit-generate/src/job/raw.rs kermit/src/materialize.rs
git commit -m "feat(job): GeneratorSpec::Job and the materialize arm (#97)" \
  -m "kind: job takes a template, 1 to 33; the spec hash covers it, so a changed template is drift and not a silent reuse. The dispatch arm locates DuckDB and the IMDb raw cache and runs JobGenerator through the shared layer, keeping the error chain that materialize would otherwise cut to its outermost message. raw::ensure is a stub that opens a finished cache until the download lands." \
  --trailer "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>" \
  --trailer "Claude-Session: https://claude.ai/code/session_01WeRnvdstSxUY8CJGSx1NFB" \
  -- kermit-bench/src/definition.rs kermit-generate/src/job/raw.rs kermit/src/materialize.rs
```

  Expected: one commit touching exactly those three files.

### Part B: `bench gen job`

- [ ] **Step 9: Write the failing CLI tests.** Create `kermit/tests/cli_job_gen.rs`. (Part C
  appends its end-to-end test to this file.)

```rust
//! `kermit bench gen job` and `bench run job-N`, through the real binary.
//!
//! The first two tests fail before any generation starts and need no DuckDB.
//! The others build the fixture raw cache (`kermit-generate/tests/fixtures/job/`)
//! at `<XDG_CACHE_HOME>/kermit/imdb`, where `raw::default_raw_dir()` looks, with
//! a provenance equal to the pins, and skip with a stderr line without DuckDB
//! (CI installs none; under `nix develop` they run).
//!
//! Every test redirects the platform cache with `XDG_CACHE_HOME`, which `dirs`
//! honours only on Linux, and passes `--output-dir` or `KERMIT_WORKSPACE`, so
//! nothing is written under `~/.cache/kermit`.

mod common;

use {
    common::{
        cli::kermit_bin,
        job::{build_raw_cache, csv_dir, fixture_dir, locate_or_skip, pinned_provenance},
    },
    kermit_bench::BenchmarkDefinition,
    kermit_generate::{
        job::{duckdb::Duckdb, JobExtras},
        GeneratedMeta,
    },
    std::{
        fs,
        path::Path,
        process::{Command, Output},
    },
};

/// The `--tag` every `bench gen` here passes.
const TAG: &str = "citest";

/// The tests that start generation redirect the cache with `XDG_CACHE_HOME`,
/// which only Linux honours.
fn skip_unsupported() -> bool {
    if cfg!(not(target_os = "linux")) {
        eprintln!("skipping cli_job_gen test: XDG_CACHE_HOME needs Linux");
        return true;
    }
    false
}

/// The fixture raw cache at `<cache>/kermit/imdb`, with a provenance equal to
/// the pins so `raw::ensure` takes it as current.
fn fixture_raw_cache(duckdb: &Duckdb, cache: &Path) {
    let csv = csv_dir(cache, false);
    build_raw_cache(
        duckdb,
        &cache.join("kermit").join("imdb"),
        &csv,
        &fixture_dir().join("queries"),
        pinned_provenance(duckdb),
    );
}

/// `kermit bench gen job --template <template> --tag citest --output-dir <out>`
/// with the platform cache at `cache`; `duckdb_bin` overrides `KERMIT_DUCKDB_BIN`.
fn gen_job(cache: &Path, out: &Path, template: &str, duckdb_bin: Option<&str>) -> Output {
    let mut command = Command::new(kermit_bin());
    command
        .env("XDG_CACHE_HOME", cache)
        .args(["bench", "gen", "job", "--template", template, "--tag", TAG])
        .arg("--output-dir")
        .arg(out);
    if let Some(bin) = duckdb_bin {
        command.env("KERMIT_DUCKDB_BIN", bin);
    }
    command.output().expect("failed to execute kermit binary")
}

#[test]
fn template_outside_1_to_33_is_rejected_before_anything_is_written() {
    for template in ["0", "34"] {
        let cache = tempfile::tempdir().unwrap();
        let out = tempfile::tempdir().unwrap();
        let output = gen_job(cache.path(), out.path(), template, None);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            !output.status.success(),
            "--template {template} must fail; stderr: {stderr}"
        );
        assert!(
            stderr.contains("--template") && stderr.contains("1..=33"),
            "the error must name the flag and the range: {stderr}"
        );
        assert_eq!(fs::read_dir(out.path()).unwrap().count(), 0, "nothing in --output-dir");
        assert_eq!(fs::read_dir(cache.path()).unwrap().count(), 0, "nothing in the cache");
    }
}

/// A host without DuckDB fails with an error naming the variable that
/// overrides it, and has created nothing: DuckDB is located before the output
/// directory is made.
#[test]
fn a_missing_duckdb_is_reported_before_anything_is_written() {
    let cache = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    let output = gen_job(cache.path(), out.path(), "1", Some("/nonexistent/kermit-test/duckdb"));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "stderr: {stderr}");
    assert!(stderr.contains("KERMIT_DUCKDB_BIN"), "{stderr}");
    assert_eq!(fs::read_dir(out.path()).unwrap().count(), 0, "nothing in --output-dir");
    assert_eq!(fs::read_dir(cache.path()).unwrap().count(), 0, "nothing in the cache");
}

#[test]
fn gen_job_writes_a_benchmark_from_the_raw_cache() {
    if skip_unsupported() {
        return;
    }
    let Some(duckdb) = locate_or_skip("gen_job_writes_a_benchmark_from_the_raw_cache") else {
        return;
    };
    let cache = tempfile::tempdir().unwrap();
    fixture_raw_cache(&duckdb, cache.path());
    let out = tempfile::tempdir().unwrap();

    let output = gen_job(cache.path(), out.path(), "1", None);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "bench gen job failed; stderr: {stderr}");
    assert!(stderr.contains("[gen job] wrote"), "{stderr}");

    let dir = out.path().join(format!("job-1-{TAG}"));
    let text = fs::read_to_string(dir.join("meta.json")).unwrap();
    let meta: GeneratedMeta<JobExtras> = serde_json::from_str(&text).unwrap();
    assert_eq!(meta.kind, "job");
    assert_eq!(meta.tag, TAG);
    assert_eq!(meta.spec_hash, None, "bench gen records no spec hash, like bench gen lubm");
    assert_eq!((meta.relation_count, meta.query_count, meta.extras.template), (9, 2, 1));

    let yaml = fs::read_to_string(dir.join("benchmark.yml")).unwrap();
    let bench: BenchmarkDefinition = serde_yaml::from_str(&yaml).unwrap();
    bench.validate().unwrap();
    assert_eq!(bench.name, format!("job-1-{TAG}"));
    let counts: Vec<(&str, Option<u64>)> = bench
        .queries
        .iter()
        .map(|query| (query.name.as_str(), query.expected))
        .collect();
    assert_eq!(counts, [("1a", Some(5)), ("1b", Some(7))]);

    // The output went to --output-dir; the cache holds only the raw cache the test built.
    assert!(cache.path().join("kermit/imdb/provenance.json").is_file());
    assert!(!cache.path().join("kermit/benchmarks").exists());
}
```

- [ ] **Step 10: Run the tests; expect failures.**
  ```bash
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit --test cli_job_gen 2>&1 | tail -30
  ```
  Expected: the three tests FAIL (clap rejects `job` as an unrecognised subcommand, exit code 2:
  the assertions on stderr and on `--template`/`1..=33`/`KERMIT_DUCKDB_BIN` do not hold; the
  third also finds no `meta.json`).

- [ ] **Step 11: Add the subcommand.**

  In `kermit/src/main.rs`. **11a. The variant.** Edit. Before:
  ```rust
  }

  #[derive(Subcommand)]
  enum Commands {
  ```
  After:
  ```rust

      /// Generate one Join Order Benchmark template on the fly (needs DuckDB)
      Job {
          /// The JOB template to generate, 1-33: every variant of it (`6` is
          /// 6a-6f) becomes one query of the benchmark
          #[arg(long, value_name = "N", required = true)]
          template: u8,

          #[command(flatten)]
          target: GenTarget,
      },
  }

  #[derive(Subcommand)]
  enum Commands {
  ```
  (The blank line goes after the previous variant's closing `},`; if H2O or SNAP variants follow
  `Lubm`, insert `Job` after them, before the enum's closing brace.)

  **11b. The handler.** Edit (insert before `main`). Before:
  ```rust
  fn main() -> anyhow::Result<()> {
      let cli = Cli::parse();
  ```
  After:
  ```rust
  /// Handler for `bench gen job`: generate one JOB template into
  /// `job-{template}-{tag}` through `kermit-generate`, with no spec hash (like
  /// `bench gen lubm`). The range is checked first and DuckDB is located before
  /// the output directory is made, so a bad template or a host without DuckDB
  /// leaves nothing behind.
  fn run_gen_job(template: u8, target: GenTarget) -> anyhow::Result<()> {
      use kermit_generate::job::{duckdb::Duckdb, raw, JobGenerator, TEMPLATES};

      anyhow::ensure!(
          TEMPLATES.contains(&template),
          "--template {template} is not a JOB template: the benchmark has templates {}..={}",
          TEMPLATES.start(),
          TEMPLATES.end()
      );
      let bench_name = format!("job-{template}-{}", target.tag);
      let duckdb = Duckdb::locate()?;
      let out_dir = target.prepare("job", &bench_name)?;
      let cache = raw::ensure(&duckdb, &raw::default_raw_dir()?)?;
      let generator = JobGenerator {
          template,
          raw: &cache,
          duckdb: &duckdb,
          target: kermit_rdf::generator::Target {
              out_dir: &out_dir,
              bench_name: &bench_name,
              tag: &target.tag,
              spec_hash: None,
          },
      };
      let meta = kermit_generate::run(&generator)?;
      eprintln!(
          "[gen job] wrote {} (relations={}, queries={})",
          out_dir.display(),
          meta.relation_count,
          meta.query_count
      );
      Ok(())
  }

  fn main() -> anyhow::Result<()> {
      let cli = Cli::parse();
  ```

  **11c. The arm in `main`.** Edit. Before:
  ```rust
                      lubm_jar,
                      ontology,
                      output_dir,
                  )?,
              },
  ```
  After:
  ```rust
                      lubm_jar,
                      ontology,
                      output_dir,
                  )?,

                  | GenSubcommand::Job {
                      template,
                      target,
                  } => run_gen_job(template, target)?,
              },
  ```
  (After the last existing `gen` arm if H2O or SNAP arms are there.)

- [ ] **Step 12: Run the tests; expect PASS.**
  ```bash
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit --test cli_job_gen -- --nocapture 2>&1 | tail -20
  ```
  Expected: `test result: ok. 3 passed; 0 failed`, no `skipping` line.

- [ ] **Step 13: Format, lint, commit Part B.**
  ```bash
  nix develop --command cargo fmt --all
  nix develop --command env CARGO_BUILD_JOBS=2 RUSTFLAGS=-Dwarnings cargo clippy -p kermit --all-targets
  git status --short
  ```
  Expected: clean; exactly `kermit/src/main.rs` and `kermit/tests/cli_job_gen.rs`. Then:

```bash
git add kermit/src/main.rs kermit/tests/cli_job_gen.rs
git commit -m "feat(job): bench gen job --template N --tag T (#97)" \
  -m "Writes job-N-TAG through the shared GenTarget, with no spec hash, as bench gen lubm does. A template outside 1 to 33 and a host without DuckDB both fail before any directory is created. The CLI tests run the real binary over the fixture raw cache with the platform cache redirected, so nothing is written under the user's cache." \
  --trailer "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>" \
  --trailer "Claude-Session: https://claude.ai/code/session_01WeRnvdstSxUY8CJGSx1NFB" \
  -- kermit/src/main.rs kermit/tests/cli_job_gen.rs
```

  Expected: one commit touching exactly those two files.

### Part C: the 33 YAMLs and the end-to-end run

- [ ] **Step 14: Write the failing tests.**

  **14a. The YAMLs are the 33 templates.** In `kermit-bench/src/definition.rs`'s `mod tests`,
  before the NIST test (anchor on its doc comment, which `cargo fmt` leaves alone). Edit. Before:
  ```rust
      /// Pins the encoding against the NIST SHA-256 vector for `"abc"`. The
  ```
  After:
  ```rust
      /// `benchmarks/job-1.yml` … `job-33.yml` are the 33 JOB templates: each
      /// loads through discovery (so it parses, validates and is named for its
      /// file), declares its own template, and no other `job-*.yml` exists.
      #[test]
      fn committed_job_yamls_are_the_33_templates() {
          let workspace = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
          for template in 1..=33u8 {
              let name = format!("job-{template}");
              let def = crate::discovery::load_benchmark(&workspace, &name)
                  .unwrap_or_else(|error| panic!("{name}: {error}"));
              assert_eq!(
                  def.generator,
                  Some(GeneratorSpec::Job { template }),
                  "{name} must declare its own template"
              );
          }
          let committed = std::fs::read_dir(workspace.join("benchmarks"))
              .unwrap()
              .filter_map(|entry| entry.unwrap().file_name().into_string().ok())
              .filter(|file| file.starts_with("job-") && file.ends_with(".yml"))
              .count();
          assert_eq!(committed, 33);
      }

      /// Pins the encoding against the NIST SHA-256 vector for `"abc"`. The
  ```

  **14b. `bench run` end to end.** Append to `kermit/tests/cli_job_gen.rs`:

```rust

/// The committed `benchmarks/job-1.yml` as the only YAML of a temp workspace.
fn workspace_with_job_1() -> tempfile::TempDir {
    let workspace = tempfile::tempdir().unwrap();
    let benchmarks = workspace.path().join("benchmarks");
    fs::create_dir(&benchmarks).unwrap();
    let committed = Path::new(env!("CARGO_MANIFEST_DIR")).join("../benchmarks/job-1.yml");
    fs::copy(&committed, benchmarks.join("job-1.yml")).unwrap();
    workspace
}

/// `kermit bench <timing flags> run job-1 -m iteration --verify <extra>` from
/// `workspace` against the platform cache `cache`, writing the report to
/// `report`; `duckdb_bin` overrides `KERMIT_DUCKDB_BIN`. `bench`'s own flags
/// (`--sample-size`, `--measurement-time`, `--warm-up-time`, `--report-json`)
/// belong to `bench`, not `run`, so they come before `run`, as in
/// `cli_bench_run_verify.rs`. `--verify` checks every count before the timing.
fn bench_run(
    cache: &Path, workspace: &Path, report: &Path, duckdb_bin: Option<&str>, extra: &[&str],
) -> Output {
    let mut command = Command::new(kermit_bin());
    command
        .env("XDG_CACHE_HOME", cache)
        .env("KERMIT_WORKSPACE", workspace)
        .args(["bench", "--sample-size", "10", "--measurement-time", "1"])
        .args(["--warm-up-time", "1", "--report-json"])
        .arg(report)
        .args(["run", "job-1", "-m", "iteration", "--verify"])
        .args(extra);
    if let Some(bin) = duckdb_bin {
        command.env("KERMIT_DUCKDB_BIN", bin);
    }
    command.output().expect("failed to execute kermit binary")
}

/// `(query, verified)` of each report in `report`, in file order.
fn verified(report: &Path) -> Vec<(String, bool)> {
    let text = fs::read_to_string(report).expect("the report file");
    let reports: Vec<serde_json::Value> = serde_json::from_str(&text).expect("a JSON array");
    reports
        .iter()
        .map(|r| {
            (
                r["axes"]["query"].as_str().expect("query axis").to_owned(),
                r["axes"]["verified"] == true,
            )
        })
        .collect()
}

/// The whole path a user takes: a committed YAML, a cache miss that generates
/// from the raw cache, `--verify` against DuckDB's counts, then a hit, drift
/// and `--force`, as for LUBM.
#[test]
fn bench_run_materialises_a_committed_yaml_and_verifies_every_count() {
    if skip_unsupported() {
        return;
    }
    let Some(duckdb) = locate_or_skip("bench_run_materialises_a_committed_yaml") else {
        return;
    };
    let cache = tempfile::tempdir().unwrap();
    fixture_raw_cache(&duckdb, cache.path());
    let workspace = workspace_with_job_1();
    let reports = tempfile::tempdir().unwrap();
    let subdir = cache.path().join("kermit/benchmarks/job-1");

    // 1. A miss: job-1 (template 1) is generated, then both queries verify (5 and 7).
    let report = reports.path().join("miss.json");
    let output = bench_run(
        cache.path(),
        workspace.path(),
        &report,
        None,
        &["-i", "tree-trie", "-a", "leapfrog-triejoin"],
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "the first run failed; stderr: {stderr}");
    assert!(subdir.join("meta.json").is_file() && subdir.join("benchmark.yml").is_file());
    assert_eq!(
        verified(&report),
        [("1a".to_owned(), true), ("1b".to_owned(), true)]
    );

    // 2. A hit: with no usable DuckDB the run succeeds only if nothing is regenerated.
    let report = reports.path().join("hit.json");
    let output = bench_run(
        cache.path(),
        workspace.path(),
        &report,
        Some("/nonexistent/kermit-test/duckdb"),
        &["-i", "tree-trie", "-a", "leapfrog-triejoin", "-q", "1a"],
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "a cache hit must not need DuckDB; stderr: {stderr}");
    assert_eq!(verified(&report), [("1a".to_owned(), true)]);

    // 3. Drift: the YAML now names template 2, so the cached spec hash no longer matches.
    let yaml_path = workspace.path().join("benchmarks/job-1.yml");
    let yaml = fs::read_to_string(&yaml_path).unwrap();
    let drifted = yaml.replace("template: 1", "template: 2");
    assert_ne!(yaml, drifted, "job-1.yml must declare `template: 1`");
    fs::write(&yaml_path, drifted).unwrap();
    let report = reports.path().join("drift.json");
    let output = bench_run(
        cache.path(),
        workspace.path(),
        &report,
        None,
        &["-i", "tree-trie", "-a", "leapfrog-triejoin"],
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "drift must fail without --force; stderr: {stderr}");
    assert!(stderr.contains("spec drift"), "{stderr}");
    assert!(subdir.join("meta.json").is_file(), "drift must not delete the cache");

    // 4. --force regenerates from the new spec: template 2's queries, including 2b, whose
    //    relation `name_n_b` is empty and whose count is 0, on Hash Triejoin.
    let report = reports.path().join("force.json");
    let output = bench_run(
        cache.path(),
        workspace.path(),
        &report,
        None,
        &["-i", "hash-trie", "-a", "hash-triejoin", "--force"],
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "--force failed; stderr: {stderr}");
    assert_eq!(
        verified(&report),
        [("2a".to_owned(), true), ("2b".to_owned(), true)]
    );
}
```

- [ ] **Step 15: Run the tests; expect failures.**
  ```bash
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-bench committed_job_yamls 2>&1 | tail -15
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit --test cli_job_gen bench_run 2>&1 | tail -15
  ```
  Expected: the first FAILS with `job-1: …` (`NotFound`: no `job-1.yml`); the second FAILS at its
  `fs::copy` (`No such file or directory`).

- [ ] **Step 16: Write the 33 YAMLs.** Write this script to
  `/tb/Source/Academia/kermit-bench-runs/job-97/scripts/write_job_yamls.sh` (`$RUNS/scripts/…`)
  with the Write tool (outside the repo, so no scratch file reaches a commit; on disk, not in the
  RAM-backed `/tmp`). It reads each template's variant range from the `QUERY_FILES` table J4
  pinned, so the descriptions cannot disagree with the pins:

```bash
#!/usr/bin/env bash
# Writes benchmarks/job-1.yml ... job-33.yml, each template's variant range read
# from the QUERY_FILES table in pins.rs. Usage: write_job_yamls.sh <worktree>
set -euo pipefail
cd "$1"
for n in $(seq 1 33); do
  # rustfmt splits each QUERY_FILES tuple, so the file name sits alone on its line.
  letters=$(sed -n "s/^        \"${n}\([a-z]\)\.sql\",\$/\1/p" kermit-generate/src/job/pins.rs)
  first=$(printf '%s\n' "$letters" | head -n 1)
  last=$(printf '%s\n' "$letters" | tail -n 1)
  if [ -z "$first" ]; then
    echo "template $n: no variants found in pins.rs" >&2
    exit 1
  fi
  cat > "benchmarks/job-$n.yml" <<EOF
name: job-$n
description: "Join Order Benchmark, template $n (${n}${first}-${n}${last})"
generator:
  kind: job
  template: $n
EOF
done
ls benchmarks/job-*.yml | wc -l
```

  Run it (bash, not zsh):
  ```bash
  WT=/tb/Source/Academia/kermit/.loom/worktrees/aidanb/job_18dea468f74c13c3
  RUNS=/tb/Source/Academia/kermit-bench-runs/job-97
  mkdir -p "$RUNS/scripts"
  bash "$RUNS/scripts/write_job_yamls.sh" "$WT"
  ```
  Expected: `33`. `benchmarks/job-6.yml` reads:
  ```yaml
  name: job-6
  description: "Join Order Benchmark, template 6 (6a-6f)"
  generator:
    kind: job
    template: 6
  ```

- [ ] **Step 17: Check the YAMLs against `pins.rs`.**
  ```bash
  RUNS=/tb/Source/Academia/kermit-bench-runs/job-97
  ls benchmarks/job-*.yml | wc -l
  cat benchmarks/job-6.yml benchmarks/job-24.yml
  for f in benchmarks/job-*.yml; do sed -n 's/^description: .*(\([0-9]*\)a-[0-9]*\([a-z]\))"$/\2/p' "$f"; done | awk '{ n += index("abcdef", $1) } END { print n }'
  grep -L 'kind: job' benchmarks/job-*.yml | wc -l
  mkdir -p "$RUNS/xdg"
  nix develop --command env CARGO_BUILD_JOBS=2 XDG_CACHE_HOME="$RUNS/xdg" cargo run -q -p kermit -- bench list 2>/dev/null | grep -c '^job-[0-9]\+ ('
  ```
  Expected, in order: `33`; the two files (`job-6.yml` as above, `job-24.yml` with `(24a-24b)`
  and `template: 24`); `113` (the variants the descriptions claim sum to the 113 queries);
  `0` (every file declares `kind: job`); `33` (`bench list` finds every one as a workspace
  benchmark, reading the redirected cache).

- [ ] **Step 18: Run the tests; expect PASS.**
  ```bash
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-bench committed_job_yamls
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit --test cli_job_gen -- --nocapture 2>&1 | tail -20
  ```
  Expected: `test result: ok. 1 passed` and `test result: ok. 4 passed; 0 failed`, no `skipping`
  line. The end-to-end test takes about 20 to 30 seconds: three of its four `bench run`s time
  queries (two, one and two query cells, each about two seconds: one of warm-up, one of
  measurement); the drift run stops before any timing; the rest is the fixture raw cache and two
  generations.

- [ ] **Step 19: Format, lint, document, and run what the task touched.**
  ```bash
  nix develop --command cargo fmt --all
  nix develop --command env CARGO_BUILD_JOBS=2 RUSTFLAGS=-Dwarnings cargo clippy -p kermit-bench -p kermit-generate -p kermit --all-targets
  nix develop --command env CARGO_BUILD_JOBS=2 RUSTDOCFLAGS=-Dwarnings cargo doc -p kermit-bench -p kermit-generate --no-deps
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-bench -p kermit-generate
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit --bin kermit --test cli_job_gen --test job_generate
  git status --short | head -40
  ```
  Expected: all clean and green; `git status --short` lists `kermit-bench/src/definition.rs`,
  `kermit/tests/cli_job_gen.rs` and the 33 YAMLs (untracked) and nothing else.

- [ ] **Step 20: Commit Part C.** Stage by name (`benchmarks/job-{1..33}.yml` expands to the 33
  file names), then commit by pathspec:

```bash
git add kermit-bench/src/definition.rs kermit/tests/cli_job_gen.rs benchmarks/job-{1..33}.yml
git commit -m "feat(job): the 33 committed job-N benchmark specs (#97)" \
  -m "benchmarks/job-1.yml to job-33.yml declare one generator benchmark per JOB template, named for it, with the variant range in its description. A kermit-bench test pins that the files are exactly those 33 and each names its own template. The end-to-end CLI test runs bench run job-1 over the fixture raw cache: a miss generates and verifies both counts, a hit needs no DuckDB, a changed template is drift, and --force regenerates template 2 (including the empty relation) on Hash Triejoin." \
  --trailer "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>" \
  --trailer "Claude-Session: https://claude.ai/code/session_01WeRnvdstSxUY8CJGSx1NFB" \
  -- kermit-bench/src/definition.rs kermit/tests/cli_job_gen.rs benchmarks/job-{1..33}.yml
```

  Expected: one commit touching 35 files.

- [ ] **Step 21: Mutation checks.** The three commits are in. For each row: apply the exact edit
  with the Edit tool, confirm `git diff --stat` shows it, run the command and see exactly the named
  tests fail, **reverse the exact edit** (never `git checkout`), re-run to green, and confirm
  `git status --short` is empty.

  | # | File and exact edit | Command | Must fail |
  | --- | --- | --- | --- |
  | 1 | `definition.rs`: `const JOB_TEMPLATES: std::ops::RangeInclusive<u8> = 1..=33;` becomes `… = 1..=34;` | `cargo test -p kermit-bench job` and `cargo test -p kermit --bin kermit materialize::tests` | `job_template_out_of_range_is_rejected`; `the_spec_and_the_generator_agree_on_the_template_range` |
  | 2 | `materialize.rs`, in `run_job`: `spec_hash: Some(spec_hash),` becomes `spec_hash: None,` | `cargo test -p kermit --test cli_job_gen bench_run` | `bench_run_materialises_a_committed_yaml_and_verifies_every_count` (its second run, the hit, sees a cache with no spec hash and reports drift) |
  | 3 | `main.rs`, in `run_gen_job`: delete the whole `anyhow::ensure!(TEMPLATES.contains(&template), …);` statement | `cargo test -p kermit --test cli_job_gen` | `template_outside_1_to_33_is_rejected_before_anything_is_written` (the directory `prepare` creates, or the missing flag name in the error) |
  | 4 | `benchmarks/job-17.yml`: `template: 17` becomes `template: 18` | `cargo test -p kermit-bench committed_job_yamls` | `committed_job_yamls_are_the_33_templates` |
  | 5 | `main.rs`, in `run_gen_job`: swap the two lines `let duckdb = Duckdb::locate()?;` and `let out_dir = target.prepare("job", &bench_name)?;` | `cargo test -p kermit --test cli_job_gen` | `a_missing_duckdb_is_reported_before_anything_is_written` (the empty `job-1-citest` directory `prepare` left behind) |

---

# Part 4: downloads and docs (J7, J8)

### Task J7: `raw::ensure` downloads, and the translator on the real 113

**Done 2026-10-16 at 169c651 (af3ba49..169c651).** Where this task's text and what landed
differ (the staging names, the test lists, the golden count of 465, `validate_query`), the
Merge notes' "J7 results" section is authoritative.

Phase 3: needs #96's `download_verified` on `origin/master` and merged into `aidanb/job`
(`git merge origin/master`, never rebase). It replaces J6's `ensure` stub, whose signature
stays the contract's, `ensure(duckdb: &Duckdb, dir: &Path) -> anyhow::Result<RawCache>`, so
J6's `run_job` call site does not change. It then adds the one test that runs the translator
on JOB's real queries.

- **`ensure` stays thin.** It passes `download_verified` (its `BenchError` mapped into
  `anyhow`) and the pinned `Sources` to a private `ensure_from`, which tests drive with a fake
  fetch over J4's fixture. The private helpers are `Sources` (`Sources::pinned()` reads
  `pins`), `QueryFile`, `ensure_from`, `fetch_and_build`, `staging_dir`, `fetch_from_mirrors`,
  and `check_provenance` with `provenance_mismatch`. This task defines **no** staging guard and
  **no** `SCHEMA_FILE`: it uses J4's `Staging` (the guard `build` already uses, in the same
  `raw.rs`) and J5's `SCHEMA_FILE` (`job/mod.rs`, imported here as `super::SCHEMA_FILE`).
- **Pins check (coordinator's decision).** `ensure_from` opens a complete cache and compares its
  `provenance()` with the `Sources`: `imdb_sha256`, `query_commit` and the full `query_sha256`
  map (a changed, an extra or a missing file all count). `duckdb_version` is recorded but not
  compared: it is the build's, not a pin. On any mismatch it returns an error naming the field,
  the cache's value, the pinned value and the directory to delete. It **never** deletes or
  rebuilds the cache itself, the same rule as generator spec drift (`SpecDrift` never
  regenerates).
- **Order.** `ensure_from` returns a complete cache whose provenance matches. Otherwise it
  stages in
  `<dir>.fetch-<pid>` (a sibling of `dir`, so `build`'s final rename stays on one filesystem),
  fetches `schema.sql` and the 113 queries **first**, then the 1.26 GB dump from `IMDB_URLS` in
  order (any error moves to the next mirror; all failing is one error listing each), extracts it
  and calls `build` with `IMDB_TABLE_ROWS` and the provenance.
- **Cleanup.** J4's `Staging` removes its directory on drop, so a failed or panicking call
  leaves no `dir` and no staging directory, and the tarball and CSVs go on success too. Its
  `create` replaces a leftover of the same name (an earlier process with this pid, killed before
  its guard ran) and also creates the parent directory, so a fresh
  `$XDG_CACHE_HOME/kermit/imdb` works with neither `kermit` nor `imdb` present. A process
  stopped by Ctrl-C or `SIGKILL` leaves its staging directory behind, under a pid that is now
  dead; JOB.md says such `imdb.building-*` / `imdb.fetch-*` directories are safe to delete.
- **The network test.** `kermit/tests/job_translate.rs` fetches the 114 pinned files with
  `download_verified` into a tempdir, once per test binary. It skips, with a stderr line, only
  when the **first** fetch fails with `BenchError::Download { .. }`. `Integrity`,
  `UnusableDownload` and `Io` panic, as does any failure after the first fetch, so a stale pin
  cannot hide behind a skip. `Download` covers an HTTP 4xx/5xx as well as no network (#96:
  reqwest's `error_for_status` raises it), so the skip also requires `source.status().is_none()`
  (a connection, DNS or timeout failure, as #96's own network test does): a 404 for a moved
  repository fails. It matches on the error variant and the status, never on
  `download_verified`'s text.
- **No `Cargo.toml` edit.** `kermit-generate` has been a normal dependency of `kermit` since
  #95's push 1, and `kermit-generate` already depends on `kermit-bench` and has `tempfile` as a
  dev-dependency. Step 1 confirms both.

**Files:**
- Modify: `kermit-generate/src/job/raw.rs` (the `ensure` stub is replaced; the new private items
  follow it; the imports gain the names Step 4 lists; J4's test helpers `fixture_dir` and
  `fixture_rows` become `pub(super)`; a second test module, `ensure_tests`, is appended)
- Create: `kermit/tests/job_translate.rs`
- Test: both files above

- [ ] **Step 1: Check the host and the premises.**
  ```bash
  WT=/tb/Source/Academia/kermit/.loom/worktrees/aidanb/job_18dea468f74c13c3
  cd "$WT"
  find /proc/[0-9]*/exe -maxdepth 0 -lname '*kermit-bench-runs*' 2>/dev/null | wc -l
  ```
  Expected: `0`. If not, wait: a compile under a timing run contaminates it.

  Then check what this task builds on. Every line must print what is shown, or the plan text
  is stale and must be fixed before Step 2:
  ```bash
  WT=/tb/Source/Academia/kermit/.loom/worktrees/aidanb/job_18dea468f74c13c3
  cd "$WT"
  grep -n 'pub fn download_verified' kermit-bench/src/cache.rs
  grep -n 'kermit-generate' kermit/Cargo.toml
  grep -n 'kermit-bench\|tempfile' kermit-generate/Cargo.toml
  grep -n 'pub mod' kermit-generate/src/job/mod.rs
  grep -n 'pub fn ensure\|pub fn build\|pub fn extract_tgz\|pub struct BuildInputs\|pub struct RawProvenance\|pub struct RawCache' kermit-generate/src/job/raw.rs
  grep -c 'pub const IMDB_SHA256: &str = "[0-9a-f]\{64\}";' kermit-generate/src/job/pins.rs
  grep -c 'pub const QUERY_FILES: \[(&str, &str); 114\]' kermit-generate/src/job/pins.rs
  ls kermit-generate/tests/fixtures/job
  grep -n 'struct Staging\|fn create\|fn fixture_dir\|fn fixture_rows' kermit-generate/src/job/raw.rs
  grep -n 'fn locate_or_skip' kermit-generate/src/job/duckdb.rs
  grep -n 'const SCHEMA_FILE' kermit-generate/src/job/mod.rs
  ```
  Expected, in order:
  - one line, `pub fn download_verified(url: &str, dest: &Path, sha256: &str) -> Result<(), BenchError> {`;
  - a `kermit-generate = { version = …, path = "../kermit-generate" }` line;
  - a `kermit-bench = …` line and a `tempfile = …` line;
  - `pub mod` lines naming at least `duckdb`, `pins`, `raw` and `translate`;
  - one line for each of the six names (`ensure` is J6's stub, `build` and `extract_tgz` are J4's);
  - `1` (the IMDb digest is measured, not J4's `…` placeholder);
  - `1`;
  - `csv  queries  rows.txt  schema.sql`;
  - exactly one `struct Staging` line (J4's: this task adds no second one, which would be
    E0428), its `fn create`, and J4's private `fn fixture_dir()` and `fn fixture_rows()` in
    `mod tests`;
  - `pub(crate) fn locate_or_skip() -> Option<Duckdb> {` in `duckdb.rs`'s `test_support`;
  - `const SCHEMA_FILE: &str = "schema.sql";` in `job/mod.rs` (J5's; private, which a child
    module such as `raw` may still name).

  J6's stub body is not quoted here, because J6 is not in the tree when this plan is written.
  Read it now: `sed -n "$(grep -n 'pub fn ensure' kermit-generate/src/job/raw.rs | cut -d: -f1),+12p" kermit-generate/src/job/raw.rs`.
  Step 4 replaces it whole, with its doc comment.

- [ ] **Step 2: Write the failing tests.**

  **2a. Share J4's fixture helpers.** `ensure_tests` is a second test module (below), so it
  cannot see the private helpers in J4's `mod tests`; it reuses them instead of redefining the
  fixture path and `rows.txt` parser. In J4's `mod tests`, make exactly two edits, each one
  word: `fn fixture_dir() -> PathBuf {` becomes `pub(super) fn fixture_dir() -> PathBuf {`, and
  `fn fixture_rows() -> Vec<(String, u64)> {` becomes
  `pub(super) fn fixture_rows() -> Vec<(String, u64)> {`. (`locate_or_skip` is already
  `pub(crate)` in `duckdb.rs`'s `test_support`.)

  **2b. The module.** Append this module at the very end of `kermit-generate/src/job/raw.rs`,
  after J4's `mod tests`. It is a second test module so J4's names cannot collide with these.
  The tests that touch DuckDB skip with a stderr line when `Duckdb::locate()` fails (CI has no
  DuckDB); the mirror loop, the staging guard, the pins and the provenance-comparison tests
  need none and always run.

```rust
#[cfg(test)]
mod ensure_tests {
    use {
        super::{
            tests::{fixture_dir, fixture_rows},
            *,
        },
        crate::job::duckdb::test_support::locate_or_skip,
        kermit_bench::cache::sha256_hex,
        std::{
            collections::{BTreeMap, BTreeSet},
            process::Command,
        },
        tempfile::TempDir,
    };

    const MIRROR_A: &str = "fixture://mirror-a/imdb.tgz";
    const MIRROR_B: &str = "fixture://mirror-b/imdb.tgz";

    /// The names in `dir` that end in `suffix`, sorted.
    fn files_ending(dir: &Path, suffix: &str) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .filter(|name| name.ends_with(suffix))
            .collect();
        names.sort();
        names
    }

    /// The fixture dressed as the network: `Sources` that name fake URLs, and the
    /// file each URL serves. The tarball is built with `tar -czf` from the
    /// fixture's CSVs, which sit at its top level as in the real dump.
    struct Fixture {
        _scratch: TempDir,
        sources: Sources,
        served: BTreeMap<String, PathBuf>,
    }

    impl Fixture {
        fn new() -> Fixture {
            let scratch = tempfile::tempdir().unwrap();
            let root = fixture_dir();

            let tgz = scratch.path().join("imdb.tgz");
            let status = Command::new("tar")
                .arg("-czf")
                .arg(&tgz)
                .arg("-C")
                .arg(root.join("csv"))
                .args(files_ending(&root.join("csv"), ".csv"))
                .status()
                .expect("tar runs");
            assert!(status.success(), "tar -czf failed");

            let mut served = BTreeMap::new();
            served.insert(MIRROR_A.to_owned(), tgz.clone());
            served.insert(MIRROR_B.to_owned(), tgz.clone());

            // schema.sql first, then the queries, as `pins::QUERY_FILES` orders them.
            let mut query_files = Vec::new();
            let queries = files_ending(&root.join("queries"), ".sql");
            for file in std::iter::once(SCHEMA_FILE.to_owned()).chain(queries) {
                let source = if file == SCHEMA_FILE {
                    root.join(&file)
                } else {
                    root.join("queries").join(&file)
                };
                let url = format!("fixture://job/{file}");
                query_files.push(QueryFile {
                    sha256: sha256_hex(&source).unwrap(),
                    url: url.clone(),
                    file,
                });
                served.insert(url, source);
            }

            let sources = Sources {
                imdb_urls: vec![MIRROR_A.to_owned(), MIRROR_B.to_owned()],
                imdb_sha256: sha256_hex(&tgz).unwrap(),
                query_commit: "fixture-commit".to_owned(),
                query_files,
                table_rows: fixture_rows(),
            };
            Fixture {
                _scratch: scratch,
                sources,
                served,
            }
        }
    }

    /// The fake network. It serves `served`, refuses the URLs in `failing`,
    /// records every request in `calls`, and checks the digest the way
    /// `download_verified` does: a mismatch leaves no file and is an error.
    struct FakeNet<'a> {
        served: &'a BTreeMap<String, PathBuf>,
        failing: BTreeSet<String>,
        calls: Vec<String>,
    }

    impl<'a> FakeNet<'a> {
        fn new(served: &'a BTreeMap<String, PathBuf>) -> Self {
            FakeNet {
                served,
                failing: BTreeSet::new(),
                calls: Vec::new(),
            }
        }

        fn fetch(&mut self, url: &str, dest: &Path, sha256: &str) -> anyhow::Result<()> {
            self.calls.push(url.to_owned());
            if self.failing.contains(url) {
                anyhow::bail!("{url} is unreachable");
            }
            let source = self
                .served
                .get(url)
                .with_context(|| format!("no fixture file serves {url}"))?;
            fs::copy(source, dest)?;
            let actual = sha256_hex(dest)?;
            if actual != sha256 {
                fs::remove_file(dest)?;
                anyhow::bail!("digest mismatch for {url}: expected {sha256}, got {actual}");
            }
            Ok(())
        }
    }

    /// A fresh cache root and the `imdb` directory inside it, which does not exist yet.
    fn cache_dir() -> (TempDir, PathBuf) {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("imdb");
        (root, dir)
    }

    fn run(
        duckdb: &Duckdb, dir: &Path, fixture: &Fixture, net: &mut FakeNet<'_>,
    ) -> anyhow::Result<RawCache> {
        ensure_from(duckdb, dir, &fixture.sources, &mut |url, dest, sha256| {
            net.fetch(url, dest, sha256)
        })
    }

    #[test]
    fn staging_dir_is_a_sibling_named_for_the_process() {
        let staged = staging_dir(Path::new("/cache/kermit/imdb"));
        assert_eq!(staged.parent(), Some(Path::new("/cache/kermit")));
        assert_eq!(
            staged.file_name().unwrap().to_str().unwrap(),
            format!("imdb.fetch-{}", std::process::id())
        );
    }

    #[test]
    fn staging_replaces_a_leftover_and_removes_itself() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("imdb.fetch-1");
        fs::create_dir_all(path.join("queries")).unwrap();
        fs::write(path.join("leftover.sql"), "stale").unwrap();

        let staging = Staging::create(path.clone()).unwrap();
        assert!(path.is_dir());
        assert_eq!(fs::read_dir(&path).unwrap().count(), 0, "the leftover is gone");
        assert_eq!(staging.path(), path);
        drop(staging);
        assert!(!path.exists(), "the guard removes the directory");
    }

    #[test]
    fn pinned_sources_mirror_the_pins() {
        let sources = Sources::pinned();
        assert_eq!(sources.imdb_urls, pins::IMDB_URLS);
        assert_eq!(sources.imdb_sha256, pins::IMDB_SHA256);
        assert_eq!(sources.query_commit, pins::QUERY_COMMIT);
        assert_eq!(sources.query_files.len(), pins::QUERY_FILES.len());
        assert_eq!(sources.query_files[0].file, SCHEMA_FILE, "schema.sql is fetched first");
        for query in &sources.query_files {
            assert_eq!(query.url, pins::query_url(&query.file));
        }
        assert_eq!(sources.table_rows.len(), pins::IMDB_TABLE_ROWS.len());
    }

    /// The provenance a build from exactly `sources` records.
    fn provenance_of(sources: &Sources) -> RawProvenance {
        RawProvenance {
            imdb_sha256: sources.imdb_sha256.clone(),
            query_commit: sources.query_commit.clone(),
            query_sha256: sources
                .query_files
                .iter()
                .map(|query| (query.file.clone(), query.sha256.clone()))
                .collect(),
            duckdb_version: "v0.0.0 (a different build)".to_owned(),
        }
    }

    #[test]
    fn a_matching_provenance_has_no_mismatch() {
        let sources = Sources::pinned();
        // The DuckDB version is the build's, not a pin: it never counts.
        assert!(provenance_mismatch(&provenance_of(&sources), &sources).is_none());
    }

    #[test]
    fn each_differing_pin_is_named_with_both_values() {
        let sources = Sources::pinned();
        let file = sources.query_files[1].file.clone();
        let pinned_digest = sources.query_files[1].sha256.clone();
        let zeros = "0".repeat(64);

        let mut built = provenance_of(&sources);
        built.imdb_sha256 = zeros.clone();
        let mismatch = provenance_mismatch(&built, &sources).unwrap();
        assert_eq!(mismatch.field, "imdb_sha256");
        assert_eq!(mismatch.cache, zeros);
        assert_eq!(mismatch.pinned, sources.imdb_sha256);

        let mut built = provenance_of(&sources);
        built.query_commit = "another-commit".to_owned();
        let mismatch = provenance_mismatch(&built, &sources).unwrap();
        assert_eq!(mismatch.field, "query_commit");
        assert_eq!(mismatch.cache, "another-commit");
        assert_eq!(mismatch.pinned, sources.query_commit);

        // A changed query digest.
        let mut built = provenance_of(&sources);
        built.query_sha256.insert(file.clone(), zeros.clone());
        let mismatch = provenance_mismatch(&built, &sources).unwrap();
        assert_eq!(mismatch.field, format!("query_sha256[{file}]"));
        assert_eq!(mismatch.cache, zeros);
        assert_eq!(mismatch.pinned, pinned_digest);

        // A pinned file the cache lacks.
        let mut built = provenance_of(&sources);
        built.query_sha256.remove(&file);
        let mismatch = provenance_mismatch(&built, &sources).unwrap();
        assert_eq!(mismatch.field, format!("query_sha256[{file}]"));
        assert_eq!(mismatch.cache, ABSENT);
        assert_eq!(mismatch.pinned, pinned_digest);

        // A file the cache has and the pins do not.
        let mut built = provenance_of(&sources);
        built.query_sha256.insert("99z.sql".to_owned(), zeros.clone());
        let mismatch = provenance_mismatch(&built, &sources).unwrap();
        assert_eq!(mismatch.field, "query_sha256[99z.sql]");
        assert_eq!(mismatch.cache, zeros);
        assert_eq!(mismatch.pinned, ABSENT);
    }

    #[test]
    fn a_failing_mirror_falls_through_to_the_next() {
        let urls = vec!["http://a/imdb.tgz".to_owned(), "http://b/imdb.tgz".to_owned()];
        let mut calls = Vec::new();
        let mut fetch = |url: &str, _dest: &Path, _sha256: &str| -> anyhow::Result<()> {
            calls.push(url.to_owned());
            if url.starts_with("http://a/") {
                anyhow::bail!("a is down");
            }
            Ok(())
        };
        fetch_from_mirrors(&mut fetch, &urls, Path::new("imdb.tgz"), "digest").unwrap();
        assert_eq!(calls, urls, "a was tried first, then b");
    }

    #[test]
    fn every_mirror_failing_lists_each_failure() {
        let urls = vec!["http://a/imdb.tgz".to_owned(), "http://b/imdb.tgz".to_owned()];
        let mut fetch = |url: &str, _dest: &Path, _sha256: &str| -> anyhow::Result<()> {
            anyhow::bail!("{url} refused")
        };
        let error =
            fetch_from_mirrors(&mut fetch, &urls, Path::new("imdb.tgz"), "digest").unwrap_err();
        let message = format!("{error:#}");
        assert!(message.contains("http://a/imdb.tgz refused"), "{message}");
        assert!(message.contains("http://b/imdb.tgz refused"), "{message}");
        assert!(message.contains("2 mirror(s)"), "{message}");
    }

    #[test]
    fn ensure_fetches_the_queries_before_the_dump_then_builds() {
        let Some(duckdb) = locate_or_skip() else {
            return;
        };
        let fixture = Fixture::new();
        let (_root, dir) = cache_dir();
        let mut net = FakeNet::new(&fixture.served);

        let cache = run(&duckdb, &dir, &fixture, &mut net).unwrap();

        // Every query file, schema.sql among them, is requested before the dump.
        let dump = net.calls.iter().position(|url| url == MIRROR_A).expect("the dump was fetched");
        assert_eq!(dump, fixture.sources.query_files.len(), "{:?}", net.calls);
        assert_eq!(net.calls.len(), dump + 1, "the first mirror served the dump");

        // The cache holds the fixture's queries, in letter order, and its provenance.
        assert_eq!(cache.dir(), dir.as_path());
        let names: Vec<String> =
            cache.template_queries(1).unwrap().into_iter().map(|(name, _)| name).collect();
        assert_eq!(names, ["1a", "1b"]);
        let provenance = cache.provenance().unwrap();
        assert_eq!(provenance.imdb_sha256, fixture.sources.imdb_sha256);
        assert_eq!(provenance.query_commit, "fixture-commit");
        assert_eq!(provenance.query_sha256.len(), fixture.sources.query_files.len());
        assert_eq!(provenance.duckdb_version, duckdb.version());

        // The tarball and the CSVs went with the staging directory.
        assert!(!staging_dir(&dir).exists(), "the staging directory is removed");
    }

    /// A fresh `$XDG_CACHE_HOME` has no `kermit` directory, so neither the cache
    /// nor its staging sibling has a parent yet: the first run creates both.
    #[test]
    fn a_first_run_creates_the_missing_cache_parent() {
        let Some(duckdb) = locate_or_skip() else {
            return;
        };
        let fixture = Fixture::new();
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("kermit").join("imdb");
        assert!(!root.path().join("kermit").exists(), "the premise: no parent");
        let mut net = FakeNet::new(&fixture.served);

        let cache = run(&duckdb, &dir, &fixture, &mut net).unwrap();

        assert_eq!(cache.dir(), dir.as_path());
        assert!(RawCache::open(&dir).is_some());
        assert!(!staging_dir(&dir).exists(), "the staging directory is removed");
    }

    #[test]
    fn ensure_falls_back_to_the_second_mirror() {
        let Some(duckdb) = locate_or_skip() else {
            return;
        };
        let fixture = Fixture::new();
        let (_root, dir) = cache_dir();
        let mut net = FakeNet::new(&fixture.served);
        net.failing.insert(MIRROR_A.to_owned());

        run(&duckdb, &dir, &fixture, &mut net).unwrap();

        let start = net.calls.len() - 2;
        assert_eq!(&net.calls[start..], [MIRROR_A, MIRROR_B]);
        assert!(RawCache::open(&dir).is_some());
    }

    #[test]
    fn a_complete_cache_is_reused_without_a_fetch() {
        let Some(duckdb) = locate_or_skip() else {
            return;
        };
        let fixture = Fixture::new();
        let (_root, dir) = cache_dir();
        let mut net = FakeNet::new(&fixture.served);
        run(&duckdb, &dir, &fixture, &mut net).unwrap();

        let again = ensure_from(&duckdb, &dir, &fixture.sources, &mut |url, _dest, _sha256| {
            panic!("a complete cache must not fetch {url}")
        })
        .unwrap();
        assert_eq!(again.dir(), dir.as_path());
    }

    #[test]
    fn a_cache_built_from_other_pins_is_refused_and_kept() {
        let Some(duckdb) = locate_or_skip() else {
            return;
        };
        let mut fixture = Fixture::new();
        let (_root, dir) = cache_dir();
        let mut net = FakeNet::new(&fixture.served);
        run(&duckdb, &dir, &fixture, &mut net).unwrap();
        let built_from = fixture.sources.imdb_sha256.clone();

        // The pins move on: the dump's digest now differs from the cache's.
        let other = "0".repeat(64);
        fixture.sources.imdb_sha256 = other.clone();
        let error =
            ensure_from(&duckdb, &dir, &fixture.sources, &mut |url, _dest, _sha256| {
                panic!("a refused cache must not fetch {url}")
            })
            .unwrap_err();

        // The error names the field, both values and the directory to delete...
        let message = format!("{error:#}");
        assert!(message.contains("imdb_sha256"), "{message}");
        assert!(message.contains(&built_from), "{message}");
        assert!(message.contains(&other), "{message}");
        assert!(message.contains(&dir.display().to_string()), "{message}");
        // ...and leaves everything where it was: nothing is deleted or rebuilt.
        assert!(dir.is_dir(), "the cache is kept");
        assert!(RawCache::open(&dir).is_some(), "and still complete");
        assert!(!staging_dir(&dir).exists());
    }

    #[test]
    fn a_stale_query_pin_fails_before_the_dump() {
        let Some(duckdb) = locate_or_skip() else {
            return;
        };
        let mut fixture = Fixture::new();
        fixture.sources.query_files[1].sha256 = "0".repeat(64);
        let stale = fixture.sources.query_files[1].url.clone();
        let (_root, dir) = cache_dir();
        let mut net = FakeNet::new(&fixture.served);

        let error = run(&duckdb, &dir, &fixture, &mut net).unwrap_err();

        assert!(format!("{error:#}").contains(&stale), "the error names {stale}: {error:#}");
        assert!(
            !net.calls.iter().any(|url| url == MIRROR_A || url == MIRROR_B),
            "the dump is never requested: {:?}",
            net.calls
        );
        assert!(!dir.exists());
        assert!(!staging_dir(&dir).exists());
    }

    #[test]
    fn a_failed_build_leaves_no_cache_and_no_staging() {
        let Some(duckdb) = locate_or_skip() else {
            return;
        };
        let mut fixture = Fixture::new();
        fixture.sources.table_rows[0].1 += 1;
        let table = fixture.sources.table_rows[0].0.clone();
        let (_root, dir) = cache_dir();
        let mut net = FakeNet::new(&fixture.served);

        let error = run(&duckdb, &dir, &fixture, &mut net).unwrap_err();

        assert!(format!("{error:#}").contains(&table), "the error names {table}: {error:#}");
        assert!(!dir.exists(), "a failed build leaves no cache");
        assert!(!staging_dir(&dir).exists(), "the failed call's download is discarded");
    }
}
```

  `Sources`, `QueryFile`, `staging_dir`, `fetch_from_mirrors`, `provenance_mismatch`,
  `ensure_from` and `ABSENT` do not exist yet, and `SCHEMA_FILE` (J5's, in `job/mod.rs`) is not
  imported into `raw.rs` yet. `Staging`, `fixture_dir`, `fixture_rows` and `locate_or_skip`
  exist already (J4's and J3's).

- [ ] **Step 3: Run the tests; they must not compile.**
  ```bash
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-generate --lib job::raw::ensure_tests 2>&1 | tail -40
  ```
  Expected: `error[E0425]: cannot find function \`staging_dir\` in this scope` and
  `cannot find function \`fetch_from_mirrors\``, `ensure_from`, `provenance_mismatch`,
  `error[E0422]: cannot find struct, variant or union type \`QueryFile\``,
  `error[E0412]`/`E0433` for `Sources`, and `cannot find value` errors for `SCHEMA_FILE` and
  `ABSENT`; no test runs. `Staging` is **not** among the errors: it is J4's.

- [ ] **Step 4: Replace the stub with the downloads.**

  **4a. Imports.** The module's `use {…}` block (one block, keep it) must name everything below.
  J4's block already has `crate::job::duckdb::{path_literal, Duckdb}`, `anyhow::{anyhow,
  ensure, Context}`, `serde`, `BTreeMap`, `fs`, `Path`, `PathBuf` and `Command`; J6 adds none.
  Merge in only what is missing (`pins`, `SCHEMA_FILE`, `bail`, `download_verified`,
  `BTreeSet`, `OsStr`) and leave the rest, since a duplicate is E0252 and a gap is E0425, both
  of which Step 5 reports. The block reads, after the merge:
  ```rust
  use {
      crate::job::{
          duckdb::{path_literal, Duckdb},
          pins, SCHEMA_FILE,
      },
      anyhow::{anyhow, bail, ensure, Context},
      kermit_bench::cache::download_verified,
      serde::{Deserialize, Serialize},
      std::{
          collections::{BTreeMap, BTreeSet},
          ffi::OsStr,
          fs,
          path::{Path, PathBuf},
          process::Command,
      },
  };
  ```
  There is no `std::io`: it was only for a staging guard this task no longer writes.
  `SCHEMA_FILE` is J5's private constant in `job/mod.rs`; a child module may import it.

  **4b. The code.** Delete J6's `ensure` (the doc comment above `pub fn ensure` and the
  function) and put this in its place. Everything after `ensure` is private.

```rust
/// The IMDb raw cache at `dir`: opened if it is complete, otherwise downloaded
/// and built.
///
/// A first call fetches `schema.sql` and the 113 pinned queries, then the
/// 1.26 GB dump, each through `download_verified`, so a stale query pin fails
/// before the long download. It stages everything in `<dir>.fetch-<pid>`,
/// builds `imdb.duckdb` with [`build`], and removes the staging directory
/// whether or not the build succeeded, so a failed call leaves no `dir`. The
/// dump's mirrors are tried in order.
///
/// A complete cache is returned only if its provenance matches the pins
/// (the dump's SHA-256, the query commit and every query file's SHA-256).
/// Otherwise `ensure` stops and names the directory to delete: like a
/// generator's spec drift, it never deletes or rebuilds a cache itself.
///
/// # Errors
///
/// A complete cache built from other pins (the error names the field, both
/// values and `dir`), a download that fails on every mirror (the error lists
/// each), an extraction that fails, or a build that fails (see [`build`]).
pub fn ensure(duckdb: &Duckdb, dir: &Path) -> anyhow::Result<RawCache> {
    ensure_from(duckdb, dir, &Sources::pinned(), &mut |url, dest, sha256| {
        download_verified(url, dest, sha256).map_err(anyhow::Error::from)
    })
}

/// What `ensure` fetches and checks. [`Sources::pinned`] is the production
/// value; tests substitute a fixture, which is why `ensure_from` takes one
/// instead of reading `pins` itself.
struct Sources {
    /// Mirrors of `imdb.tgz`, tried in order.
    imdb_urls: Vec<String>,
    /// SHA-256 of `imdb.tgz`.
    imdb_sha256: String,
    /// The query repository's commit, recorded in the provenance.
    query_commit: String,
    /// `schema.sql` first, then the queries.
    query_files: Vec<QueryFile>,
    /// The row count the build must find in each table.
    table_rows: Vec<(String, u64)>,
}

/// One pinned file of the query repository.
struct QueryFile {
    /// Its name in the repository: `schema.sql` or `6a.sql`.
    file: String,
    /// Where to fetch it.
    url: String,
    /// Its SHA-256.
    sha256: String,
}

impl Sources {
    /// The values `pins` records.
    fn pinned() -> Sources {
        Sources {
            imdb_urls: pins::IMDB_URLS.iter().map(|url| (*url).to_owned()).collect(),
            imdb_sha256: pins::IMDB_SHA256.to_owned(),
            query_commit: pins::QUERY_COMMIT.to_owned(),
            query_files: pins::QUERY_FILES
                .iter()
                .map(|(file, sha256)| QueryFile {
                    file: (*file).to_owned(),
                    url: pins::query_url(file),
                    sha256: (*sha256).to_owned(),
                })
                .collect(),
            table_rows: pins::IMDB_TABLE_ROWS
                .iter()
                .map(|(table, rows)| ((*table).to_owned(), *rows))
                .collect(),
        }
    }
}

/// [`ensure`] over any `sources` and any way of fetching: `fetch(url, dest,
/// sha256)` must leave `dest` holding the file at `url` or fail, as
/// `download_verified` does.
fn ensure_from(
    duckdb: &Duckdb, dir: &Path, sources: &Sources,
    fetch: &mut dyn FnMut(&str, &Path, &str) -> anyhow::Result<()>,
) -> anyhow::Result<RawCache> {
    match RawCache::open(dir) {
        | Some(cache) => {
            check_provenance(&cache, dir, sources)?;
            Ok(cache)
        },
        | None => fetch_and_build(duckdb, dir, sources, fetch),
    }
}

/// How a cache's provenance differs from the pins: the first field that does.
struct Mismatch {
    /// `imdb_sha256`, `query_commit` or `query_sha256[<file>]`.
    field: String,
    /// The cache's value, or `(absent)`.
    cache: String,
    /// The pinned value, or `(absent)`.
    pinned: String,
}

/// What a missing query file shows in a [`Mismatch`].
const ABSENT: &str = "(absent)";

/// `Some` if `cache` and `pinned` differ.
fn differing(field: String, cache: &str, pinned: &str) -> Option<Mismatch> {
    (cache != pinned).then(|| Mismatch {
        field,
        cache: cache.to_owned(),
        pinned: pinned.to_owned(),
    })
}

/// The first way `built` differs from `sources`, if any: the dump's digest,
/// then the query commit, then the query files in name order (a file in only
/// one of the two counts). `duckdb_version` is the build's, not a pin, and is
/// not compared.
fn provenance_mismatch(built: &RawProvenance, sources: &Sources) -> Option<Mismatch> {
    let pinned: BTreeMap<&str, &str> = sources
        .query_files
        .iter()
        .map(|query| (query.file.as_str(), query.sha256.as_str()))
        .collect();
    let files: BTreeSet<&str> =
        pinned.keys().copied().chain(built.query_sha256.keys().map(String::as_str)).collect();
    differing("imdb_sha256".to_owned(), &built.imdb_sha256, &sources.imdb_sha256)
        .or_else(|| {
            differing("query_commit".to_owned(), &built.query_commit, &sources.query_commit)
        })
        .or_else(|| {
            files.into_iter().find_map(|file| {
                differing(
                    format!("query_sha256[{file}]"),
                    built.query_sha256.get(file).map_or(ABSENT, String::as_str),
                    pinned.get(file).copied().unwrap_or(ABSENT),
                )
            })
        })
}

/// Refuses a complete cache that was built from other pins, naming the field,
/// both values and the directory to delete. Nothing is deleted or rebuilt: the
/// user decides, as with a generator's spec drift.
fn check_provenance(cache: &RawCache, dir: &Path, sources: &Sources) -> anyhow::Result<()> {
    let built = cache
        .provenance()
        .with_context(|| format!("reading the provenance of {}", dir.display()))?;
    match provenance_mismatch(&built, sources) {
        | None => Ok(()),
        | Some(Mismatch { field, cache, pinned }) => bail!(
            "the IMDb raw cache at {dir} was built from other pins: {field} is {cache} in the \
             cache but {pinned} in the pins; delete {dir} and run again to rebuild it (kermit \
             never deletes or rebuilds a cache itself)",
            dir = dir.display()
        ),
    }
}

/// Stages the files in [`staging_dir`], builds the cache from them and drops
/// the staging directory.
fn fetch_and_build(
    duckdb: &Duckdb, dir: &Path, sources: &Sources,
    fetch: &mut dyn FnMut(&str, &Path, &str) -> anyhow::Result<()>,
) -> anyhow::Result<RawCache> {
    let staging = Staging::create(staging_dir(dir))?;
    let queries_dir = staging.path().join("queries");
    let csv_dir = staging.path().join("csv");
    let schema_sql = staging.path().join(SCHEMA_FILE);
    let tgz = staging.path().join("imdb.tgz");
    fs::create_dir_all(&queries_dir)
        .with_context(|| format!("creating {}", queries_dir.display()))?;

    // The 114 small files first: a stale pin fails here, before the 1.26 GB dump.
    let mut query_sha256 = BTreeMap::new();
    for QueryFile { file, url, sha256 } in &sources.query_files {
        let dest = if file == SCHEMA_FILE {
            schema_sql.clone()
        } else {
            queries_dir.join(file)
        };
        fetch(url.as_str(), &dest, sha256.as_str())
            .with_context(|| format!("fetching {file} from {url}"))?;
        query_sha256.insert(file.clone(), sha256.clone());
    }

    fetch_from_mirrors(fetch, &sources.imdb_urls, &tgz, &sources.imdb_sha256)?;
    fs::create_dir_all(&csv_dir).with_context(|| format!("creating {}", csv_dir.display()))?;
    extract_tgz(&tgz, &csv_dir)?;

    let table_rows: Vec<(&str, u64)> =
        sources.table_rows.iter().map(|(table, rows)| (table.as_str(), *rows)).collect();
    let inputs = BuildInputs {
        csv_dir: &csv_dir,
        schema_sql: &schema_sql,
        queries_dir: &queries_dir,
        table_rows: &table_rows,
        provenance: RawProvenance {
            imdb_sha256: sources.imdb_sha256.clone(),
            query_commit: sources.query_commit.clone(),
            query_sha256,
            duckdb_version: duckdb.version().to_owned(),
        },
    };
    build(duckdb, dir, &inputs)
}

/// Fetches `dest` from the first of `urls` that serves it with a matching
/// digest. Any error moves on to the next mirror; when every mirror fails,
/// the error lists each failure.
fn fetch_from_mirrors(
    fetch: &mut dyn FnMut(&str, &Path, &str) -> anyhow::Result<()>, urls: &[String],
    dest: &Path, sha256: &str,
) -> anyhow::Result<()> {
    let mut failures = Vec::new();
    for url in urls {
        match fetch(url.as_str(), dest, sha256) {
            | Ok(()) => return Ok(()),
            | Err(error) => failures.push(format!("  {url}: {error:#}")),
        }
    }
    bail!(
        "could not download the IMDb dump from any of {} mirror(s):\n{}",
        urls.len(),
        failures.join("\n")
    )
}

/// `<dir>.fetch-<pid>`: where `ensure` stages a download, beside `dir` so
/// `build`'s rename into place stays on one filesystem. It carries the pid so
/// two processes never share one.
fn staging_dir(dir: &Path) -> PathBuf {
    let mut name = dir.file_name().unwrap_or_else(|| OsStr::new(RAW_DIR_NAME)).to_os_string();
    name.push(format!(".fetch-{}", std::process::id()));
    dir.with_file_name(name)
}
```

  Why the pieces are shaped this way:
  - `ensure_from` takes `&mut dyn FnMut`, not a generic, so `ensure`'s closure and a test's
    closure share one compiled body, and a test's closure may borrow its `FakeNet`.
  - `schema.sql` (J5's `SCHEMA_FILE`) is staged at the top level and the queries under
    `queries/`, because `BuildInputs::queries_dir` is "`<t><letter>.sql` files, copied into the
    cache" and must not hold `schema.sql`.
  - `RawProvenance::query_sha256` holds every pinned file, `schema.sql` included, so the
    provenance records exactly what was fetched.
  - J4's `Staging` is the only staging guard in `raw.rs`: `build` uses it for
    `<dir>.building-<pid>` and `fetch_and_build` for `<dir>.fetch-<pid>`. Its `Drop` runs on
    every exit, including `?` and a panic in a test, and its `create` makes the parent
    directory, which a first run under a fresh `$XDG_CACHE_HOME` needs.
  - `check_provenance` compares only what the pins name (`imdb_sha256`, `query_commit`, the
    `query_sha256` map). It reports the first difference, in that order and then by file name,
    so the message is deterministic. It is an error, never a rebuild: the same rule as a
    generator's `SpecDrift`, which waits for `--force`.

- [ ] **Step 5: Run the unit tests.**
  ```bash
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-generate --lib job::raw
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-generate
  ```
  Expected: `test result: ok` each time. The first run lists J4's tests and the fourteen
  `ensure_tests`: `staging_dir_is_a_sibling_named_for_the_process`,
  `staging_replaces_a_leftover_and_removes_itself`, `pinned_sources_mirror_the_pins`,
  `a_matching_provenance_has_no_mismatch`, `each_differing_pin_is_named_with_both_values`,
  `a_failing_mirror_falls_through_to_the_next`, `every_mirror_failing_lists_each_failure`,
  `ensure_fetches_the_queries_before_the_dump_then_builds`,
  `a_first_run_creates_the_missing_cache_parent`, `ensure_falls_back_to_the_second_mirror`,
  `a_complete_cache_is_reused_without_a_fetch`,
  `a_cache_built_from_other_pins_is_refused_and_kept`, `a_stale_query_pin_fails_before_the_dump`
  and `a_failed_build_leaves_no_cache_and_no_staging`. Inside `nix develop` DuckDB is on `PATH`,
  so none of them prints `skipping:`; outside it the last seven do, and pass.

  If a duplicate import is reported (E0252), delete the second. `Staging` and `SCHEMA_FILE` must
  each be defined exactly once in the crate (J4's and J5's): an E0428 or E0119 naming either
  means a second copy was left in `raw.rs`, so delete it and use the original.

- [ ] **Step 6: Write the real-113 test.**

  Create `kermit/tests/job_translate.rs`:

```rust
//! The translator on JOB's real 113 queries.
//!
//! `kermit-generate`'s unit tests run the translator on hand-written JOB-shaped
//! SQL, because the query repository carries no licence and its files cannot be
//! committed. This test fetches the pinned originals instead
//! (`gregrahn/join-order-benchmark` at `pins::QUERY_COMMIT`), each checked
//! against its SHA-256 by `download_verified`, parses every variant of every
//! template, plans the template, and checks every emitted rule and relation
//! against what the executors and the generator layer need:
//!
//! - every rule parses as a `JoinQuery`;
//! - after `rewrite_placeholders`, every rule passes `check_attribute_order`
//!   (JOB needs no `--column-orders any`);
//! - every relation name matches `[a-z][a-z0-9_]*`, which `RelationSink::open`
//!   enforces, and every variable starts with an ASCII uppercase letter;
//! - 113 rules and 465 relation files across the 33 templates.
//!
//! It lives in `kermit` because `check_attribute_order` and
//! `rewrite_placeholders` are in `kermit-algos`, which `kermit-generate` does
//! not depend on.
//!
//! Needs network access to `raw.githubusercontent.com`, in CI too. It skips,
//! with a stderr line (run with `--nocapture` to see it), only when the first
//! fetch cannot connect. A digest mismatch, a refused or empty answer, or a
//! failure after the first fetch panics, so a stale pin cannot hide behind a
//! skip.

use {
    kermit_algos::{check_attribute_order, rewrite_placeholders},
    kermit_bench::{cache::download_verified, BenchError},
    kermit_generate::job::{
        pins::{query_url, QUERY_FILES},
        translate::{parse_query, plan_template, JobQuery, TemplatePlan},
    },
    kermit_parser::{JoinQuery, Term},
    std::{collections::BTreeMap, fs, sync::OnceLock},
};

/// JOB's 33 templates.
const TEMPLATES: usize = 33;
/// JOB's 113 queries, one rule each.
const RULES: usize = 113;
/// The relation files across all 33 templates (the spec's golden count).
const RELATION_FILES: usize = 465;

/// The 113 pinned queries as `(name, sql)`, `name` being `6a`, in
/// `QUERY_FILES` order (template, then letter). `None` when the first fetch
/// could not connect. Fetched once per test binary.
fn pinned_queries() -> Option<&'static [(String, String)]> {
    static QUERIES: OnceLock<Option<Vec<(String, String)>>> = OnceLock::new();
    QUERIES.get_or_init(fetch_pinned_queries).as_deref()
}

fn fetch_pinned_queries() -> Option<Vec<(String, String)>> {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let mut queries = Vec::new();
    for (index, (file, sha256)) in QUERY_FILES.iter().enumerate() {
        let dest = dir.path().join(file);
        match download_verified(&query_url(file), &dest, sha256) {
            | Ok(()) => {},
            // Only a connection, DNS or timeout failure is "offline". A `Download` carrying
            // an HTTP status (a 404 for a vanished pin) fails the test, as #96's does.
            | Err(BenchError::Download { url, source }) if index == 0 && source.status().is_none() => {
                eprintln!(
                    "skipping: could not reach {url} ({source}); the pinned JOB queries are \
                     fetched at test time"
                );
                return None;
            },
            | Err(error) => panic!("fetching {file} failed: {error}"),
        }
        if let Some(name) = file.strip_suffix(".sql").filter(|name| *name != "schema") {
            let sql = fs::read_to_string(&dest).unwrap_or_else(|e| panic!("reading {file}: {e}"));
            queries.push((name.to_owned(), sql));
        }
    }
    Some(queries)
}

/// The template number of a query name: `6a` is 6, `33c` is 33.
fn template_number(name: &str) -> u8 {
    name.trim_end_matches(|c: char| c.is_ascii_lowercase())
        .parse()
        .unwrap_or_else(|_| panic!("{name} does not start with a template number"))
}

/// Every template's plan, and everything that went wrong on the way.
struct Translated {
    plans: BTreeMap<u8, TemplatePlan>,
    problems: Vec<String>,
}

/// Parses every query, groups the variants by template (they arrive in letter
/// order) and plans each template. A failure is recorded, not raised, so one
/// run reports them all.
fn translate(queries: &[(String, String)]) -> Translated {
    let mut problems = Vec::new();
    let mut variants: BTreeMap<u8, Vec<JobQuery>> = BTreeMap::new();
    for (name, sql) in queries {
        match parse_query(name, sql) {
            | Ok(query) => variants.entry(template_number(name)).or_default().push(query),
            | Err(error) => problems.push(format!("parse {name}: {error}")),
        }
    }
    let mut plans = BTreeMap::new();
    for (template, queries) in &variants {
        match plan_template(queries) {
            | Ok(plan) => {
                plans.insert(*template, plan);
            },
            | Err(error) => problems.push(format!("plan template {template}: {error}")),
        }
    }
    Translated { plans, problems }
}

/// Whether `name` matches `[a-z][a-z0-9_]*`, the rule `RelationSink::open`
/// enforces (checked by hand: no regex crate).
fn is_relation_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|first| first.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// What is wrong with one rule, if anything: it must parse, name its variables
/// with an uppercase initial, and, once its placeholders are variables, admit a
/// global attribute order under `--column-orders stored`.
fn rule_problems(query: &str, datalog: &str) -> Vec<String> {
    let parsed: JoinQuery = match datalog.parse() {
        | Ok(parsed) => parsed,
        | Err(error) => return vec![format!("{query}: `{datalog}` does not parse: {error}")],
    };
    let mut problems = Vec::new();
    let terms = parsed.head.terms.iter().chain(parsed.body.iter().flat_map(|atom| &atom.terms));
    for term in terms {
        match term {
            | Term::Var(name) if !name.starts_with(|c: char| c.is_ascii_uppercase()) => {
                problems.push(format!("{query}: variable {name:?} does not start uppercase"));
            },
            | _ => {},
        }
    }
    if let Err(cycle) = check_attribute_order(&rewrite_placeholders(parsed)) {
        problems.push(format!(
            "{query}: `{datalog}` has no attribute order; atoms {:?} form a cycle",
            cycle.atoms
        ));
    }
    problems
}

#[test]
#[cfg_attr(miri, ignore = "miri does not support network access")]
fn pinned_queries_translate_into_valid_rules() {
    let Some(queries) = pinned_queries() else {
        return;
    };
    let Translated { plans, mut problems } = translate(queries);
    for (template, plan) in &plans {
        for relation in &plan.relations {
            if !is_relation_name(&relation.name) {
                problems.push(format!(
                    "template {template}: relation name {:?} does not match [a-z][a-z0-9_]*",
                    relation.name
                ));
            }
        }
        for rule in &plan.rules {
            problems.extend(rule_problems(&rule.query, &rule.datalog));
        }
    }
    assert!(problems.is_empty(), "{} problem(s):\n{}", problems.len(), problems.join("\n"));
}

#[test]
#[cfg_attr(miri, ignore = "miri does not support network access")]
fn pinned_templates_give_the_golden_counts() {
    let Some(queries) = pinned_queries() else {
        return;
    };
    let Translated { plans, problems } = translate(queries);
    assert!(problems.is_empty(), "{} problem(s):\n{}", problems.len(), problems.join("\n"));

    assert_eq!(plans.len(), TEMPLATES, "templates");
    let rules: usize = plans.values().map(|plan| plan.rules.len()).sum();
    assert_eq!(rules, RULES, "rules, one per query");
    let relation_files: usize = plans.values().map(|plan| plan.relations.len()).sum();
    assert_eq!(relation_files, RELATION_FILES, "relation files across all templates");
}
```

  The two tests are separate so a count drift and a bad rule fail separately. Both share one
  fetch through the `OnceLock`.

- [ ] **Step 7: Run the real-113 test.**
  ```bash
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit --test job_translate -- --nocapture
  ```
  Expected, with network:
  ```text
  running 2 tests
  test pinned_queries_translate_into_valid_rules ... ok
  test pinned_templates_give_the_golden_counts ... ok

  test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
  ```
  The fetch is 114 small files; it writes only to a tempdir, never under `~/.cache/kermit`.
  Without network both tests print the same `skipping: could not reach …` line (once) and pass.

  **If an assertion fails, do not touch the constants or the checks.** The message lists every
  problem. A rule that does not parse or has no attribute order, a name outside
  `[a-z][a-z0-9_]*`, or a count other than 113 and 465 is a bug in J1's or J2's `translate.rs`
  (or the contract's golden number): stop and report it with the printed list.

- [ ] **Step 8: Lint, format, document.**
  ```bash
  nix develop --command env CARGO_BUILD_JOBS=2 RUSTFLAGS=-Dwarnings cargo clippy --all-targets
  nix develop --command cargo fmt --all
  nix develop --command cargo fmt --all -- --check
  nix develop --command env CARGO_BUILD_JOBS=2 RUSTDOCFLAGS=-Dwarnings cargo doc --workspace --no-deps
  ```
  Expected: all clean. `[`build`]` in `ensure`'s docs resolves to J4's `build`; `Sources::pinned`
  is a private item and its link is not rendered. `cargo fmt` may reflow the tests' closures and
  the `let … else` blocks; accept its output.

- [ ] **Step 9: Commit.**
  ```bash
  WT=/tb/Source/Academia/kermit/.loom/worktrees/aidanb/job_18dea468f74c13c3
  cd "$WT"
  git add kermit-generate/src/job/raw.rs kermit/tests/job_translate.rs
  git commit -m "feat(job): raw::ensure downloads the IMDb dump and the queries (#97)" \
    -m "ensure now fetches the 114 pinned query-repository files and then the 1.26 GB IMDb dump through download_verified, trying the dump's mirrors in order, extracts it and builds the raw cache, in a staging directory that is removed on every exit. The small files come first so a stale query pin fails before the long download. A complete cache is returned only if its provenance matches the pins; otherwise the call stops with an error naming the field, both values and the directory to delete (kermit never deletes or rebuilds it itself)." \
    -m "kermit/tests/job_translate.rs fetches the same pinned files at test time and translates all 113 queries: every rule parses, passes check_attribute_order after the placeholder rewrite, and uses parser-valid names; the 33 templates give 113 rules and 465 relation files. It skips only when the first fetch cannot connect, so a stale pin fails the test." \
    --trailer "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>" \
    --trailer "Claude-Session: https://claude.ai/code/session_01WeRnvdstSxUY8CJGSx1NFB" \
    -- kermit-generate/src/job/raw.rs kermit/tests/job_translate.rs
  git status --short
  ```
  `kermit/tests/job_translate.rs` is a new file, so it is staged by name first; the commit is
  by pathspec, never bare, `-a` or `--amend`. Expected: `git status --short` prints nothing.

- [ ] **Step 10: Mutation checks and the skip control.** Run after the commit, so each mutant
  is a diff against it. Per mutant:
  1. apply the exact edit;
  2. `git diff --stat` must show the file changed, or the mutant did not apply and the result
     means nothing;
  3. run the command, which must fail in the tests named;
  4. reverse the exact edit (swap the Edit's two strings), never `git checkout`;
  5. re-run to green; `git status --short` prints nothing.

  | # | Mutant | Must fail |
  | --- | --- | --- |
  | 1 | `fetch_from_mirrors` stops after the first failure | `a_failing_mirror_falls_through_to_the_next`, `every_mirror_failing_lists_each_failure`, `ensure_falls_back_to_the_second_mirror` |
  | 2 | `fetch_and_build` requests the dump before the query files | `ensure_fetches_the_queries_before_the_dump_then_builds`, `a_stale_query_pin_fails_before_the_dump` |
  | 3 | J4's `Staging` `Drop` (in `raw.rs`) removes nothing | `staging_replaces_a_leftover_and_removes_itself`, `ensure_fetches_the_queries_before_the_dump_then_builds`, `a_first_run_creates_the_missing_cache_parent`, `a_stale_query_pin_fails_before_the_dump`, `a_cache_built_from_other_pins_is_refused_and_kept`, `a_failed_build_leaves_no_cache_and_no_staging` |
  | 4 | `ensure_from` hands back a complete cache without checking its provenance | `a_cache_built_from_other_pins_is_refused_and_kept` |

  **Mutant 1.** In `fetch_from_mirrors`, replace
  ```rust
              | Err(error) => failures.push(format!("  {url}: {error:#}")),
  ```
  with
  ```rust
              | Err(error) => {
                  failures.push(format!("  {url}: {error:#}"));
                  break;
              },
  ```
  ```bash
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-generate --lib job::raw::ensure_tests::a_failing_mirror_falls_through_to_the_next
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-generate --lib job::raw::ensure_tests::ensure_falls_back_to_the_second_mirror
  ```
  Expected: the first panics at the `unwrap()` on `Err(could not download the IMDb dump from any of 2 mirror(s): …)`; the second the same inside `run(…).unwrap()`.

  **Mutant 2.** In `fetch_and_build`, replace the comment line
  ```rust
      // The 114 small files first: a stale pin fails here, before the 1.26 GB dump.
  ```
  with
  ```rust
      fetch_from_mirrors(fetch, &sources.imdb_urls, &tgz, &sources.imdb_sha256)?;
      // The 114 small files first: a stale pin fails here, before the 1.26 GB dump.
  ```
  and delete the original `fetch_from_mirrors(fetch, …)?;` line that follows the `for` loop, so
  the call appears once, before the loop.
  ```bash
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-generate --lib job::raw::ensure_tests::ensure_fetches_the_queries_before_the_dump_then_builds
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-generate --lib job::raw::ensure_tests::a_stale_query_pin_fails_before_the_dump
  ```
  Expected: the first fails `assertion \`left == right\` failed` with `left: 0` (the dump is
  the first request) and `right: 5` (`schema.sql` plus the fixture's 1a, 1b, 2a and 2b); the
  second fails `the dump is never requested`.

  **Mutant 3.** The guard is J4's, not this task's: in `impl Drop for Staging` in `raw.rs`
  (`grep -n 'remove_dir_all' kermit-generate/src/job/raw.rs` shows two lines, one in `create`
  and one in `drop`), replace
  ```rust
              let _ = fs::remove_dir_all(path);
  ```
  with
  ```rust
              let _ = path;
  ```
  ```bash
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-generate --lib job::raw::ensure_tests
  ```
  Expected: six tests fail, on `the guard removes the directory`
  (`staging_replaces_a_leftover_and_removes_itself`) and on the staging-directory assertions
  (`the staging directory is removed` in `ensure_fetches_the_queries_before_the_dump_then_builds`
  and `a_first_run_creates_the_missing_cache_parent`, `the failed call's download is discarded`
  in `a_failed_build_leaves_no_cache_and_no_staging`, and the bare
  `assert!(!staging_dir(&dir).exists())` that ends `a_stale_query_pin_fails_before_the_dump` and
  `a_cache_built_from_other_pins_is_refused_and_kept`). Running all of `job::raw` also fails
  J4's `losing_the_rename_adopts_the_winner` (the loser's staging is no longer gone): the
  mutant is J4's code, so J4's tests catch it too.

  **Mutant 4.** In `ensure_from`, replace
  ```rust
              check_provenance(&cache, dir, sources)?;
  ```
  with
  ```rust
              let _ = check_provenance;
  ```
  ```bash
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-generate --lib job::raw::ensure_tests::a_cache_built_from_other_pins_is_refused_and_kept
  ```
  Expected: the test fails at `.unwrap_err()` with `called \`Result::unwrap_err()\` on an \`Ok\`
  value`: the cache with other pins was handed back. (Run it against the unmutated code first:
  it passes, so the failure is the mutant's.)

  **Skip control A: a stale pin fails the real-113 test.** In `kermit-generate/src/job/pins.rs`,
  flip the first hex digit of the `schema.sql` digest, the first `QUERY_FILES` entry
  (`1d10ba0c…` becomes `0d10ba0c…`; confirm with `grep -c 1d10ba0c9a881 kermit-generate/src/job/pins.rs`
  printing `1` first):
  ```bash
  sed -i 's/1d10ba0c9a881e890aeb951c0960cc4e3472c9c9695ce1edb6493e9e4466d3d2/0d10ba0c9a881e890aeb951c0960cc4e3472c9c9695ce1edb6493e9e4466d3d2/' kermit-generate/src/job/pins.rs
  git diff --stat
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit --test job_translate -- --nocapture
  ```
  Expected: both tests **fail**, not skip, with a panic that names `schema.sql` and an
  integrity failure; it reads like `fetching schema.sql failed: integrity check failed for file
  'schema.sql' from …`. This is the first fetch, so it proves an `Integrity` error on the first
  fetch is not a skip. (Read the wording, never assert on it.)
  Reverse it with the same `sed` swapped, and re-run to green.

  **Skip control B: no network skips.** Point the proxy at a closed port:
  ```bash
  HTTPS_PROXY=http://127.0.0.1:9 nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit --test job_translate -- --nocapture
  ```
  Expected: one `skipping: could not reach https://raw.githubusercontent.com/… ` line and
  `test result: ok. 2 passed`. If the HTTP client does not honour `HTTPS_PROXY`, this control
  runs against the real network and passes without a skip line: that does not fail the task;
  note it in the report.

  Then `git status --short` prints nothing and the tests are green again.

---

### Task J8: Docs

Phase 4: needs J1–J7 on the branch. Docs only: no Rust changes, so no cargo run is needed
(CLAUDE.md: a doc-only change cannot break compilation or tests). It carries the spec's
Docs list and the contract's J8 list:

- `docs/benchmarks/JOB.md` (new), the whole file, following `docs/benchmarks/README.md`'s section
  schema in order, plus its two applicable-when-relevant sections `Inference / preprocessing`
  and `Practical scale ceiling`. `Sandboxing` is skipped: DuckDB runs unsandboxed. Its memory
  table keeps **one placeholder row under a single `<!-- J9-FILL: … -->` comment**; J9 replaces
  the row and deletes the comment, and the final gate greps `J9-FILL` and expects nothing.
- `docs/benchmarks/README.md`: JOB's Index entry and its column in the cross-benchmark matrix,
  per that file's own "Adding a new benchmark" checklist (Index entry; a matrix cell for every
  row).
- `benchmarks/README.md`: the `job` kind, its `template` field, the YAML example, and the
  `generator.scale` row no longer required for every kind.
- `ARCHITECTURE.md` (line 392 at 6e94dcb): the generator-driven bullet names `kind: watdiv|lubm|snap` and
  the generators behind them; add `job` (no `scale`, a `kermit-generate` generator).
- `CLAUDE.md`: a Benchmark Reference Docs entry, `bench gen job` in Build Commands, one gotcha,
  the "Adding a new benchmark" line (which names the generator kinds and `kermit-rdf`), and the
  `job` entries in lists #95 and #96 maintain.

Sources of the facts in JOB.md, so a reviewer can check them: the paper (§2.1, §2.2 and
footnote 4, at `/tb/Source/Academia/kermit-msc-thesis/background/markdown/Leis et al. - 2015 - How good are query optimizers, really.md`) for the IMDb snapshot, the 33 × (2–6) = 113 shape,
the `MIN` footnote, `cast_info`'s 36 M rows and the page range 204–215; the pinned repository's
`README.md` for the title line, the CWI mirror, the IMDb licence link and the absence of a
licence file; the spec for the scan facts (24 queries with `BETWEEN`, 589 alias occurrences
joining on `id`, 4–17 aliases, 2–10 join variables, 0 column-order conflicts); the contract for
every constant. The alias counts in the Workload reference table were counted from the pinned
`FROM` clauses.

**Files:**
- Create: `docs/benchmarks/JOB.md`
- Modify: `docs/benchmarks/README.md` (the Index; the comparison table)
- Modify: `benchmarks/README.md` (File layout; the generator tables; the examples; Caching)
- Modify: `CLAUDE.md` (Build Commands; Benchmark Reference Docs; Gotchas; "Adding a new
  benchmark"; the workspace lists)

- [ ] **Step 1: Check the premises.**
  ```bash
  WT=/tb/Source/Academia/kermit/.loom/worktrees/aidanb/job_18dea468f74c13c3
  cd "$WT"
  git log --oneline -8
  test ! -e docs/benchmarks/JOB.md && echo "JOB.md does not exist yet"
  ls benchmarks/job-1.yml benchmarks/job-33.yml
  sed -n 1,6p benchmarks/job-6.yml
  grep -n 'KERMIT_DUCKDB_BIN\|MEMORY_LIMIT\|BATCH_ROWS' kermit-generate/src/job/duckdb.rs | head
  grep -n 'RAW_DIR_NAME\|IMDB_URLS\|QUERY_COMMIT' kermit-generate/src/job/raw.rs kermit-generate/src/job/pins.rs | head
  grep -n 'pub fn clean_all' -A6 kermit-bench/src/cache.rs
  grep -n 'docs/benchmarks/' CLAUDE.md
  grep -n 'gen \[' CLAUDE.md
  grep -n 'For declarative generators' CLAUDE.md
  grep -n 'kermit-generate' CLAUDE.md
  grep -n 'generator.scale' benchmarks/README.md
  grep -n '^| Dimension' docs/benchmarks/README.md
  ```
  Expected:
  - the log shows J1–J7's commits;
  - `JOB.md does not exist yet`;
  - `job-1.yml` and `job-33.yml` exist, and `job-6.yml` reads `name: job-6`, a `description:`
    line, `generator:`, `kind: job`, `template: 6`. **Keep that `description:` line**: Step 2's
    YAML block must show the committed text, so copy it from this output if it differs from the
    one in the block;
  - `KERMIT_DUCKDB_BIN`, a 4GB `MEMORY_LIMIT` and `BATCH_ROWS = 1 << 16` in `duckdb.rs`; the
    imdb directory name and both mirrors in `raw.rs` / `pins.rs`; `QUERY_COMMIT` equal to
    `a39603662e023e449cb2121997a5034df9e02ebf`. If any differs, fix the plan text below before
    writing (the doc states these values);
  - `clean_all` removing `base_cache_dir()`, which is `<cache>/kermit/benchmarks`, so the raw
    cache `<cache>/kermit/imdb/` survives it;
  - the Benchmark Reference Docs entries, the `kermit` row's `gen […]` list (at 5dd1691 it is
    `gen [watdiv|lubm]`), the one "Adding a new benchmark" line that begins `Add a YAML file
    in` and contains `For declarative generators:` (at 5dd1691 its `kind:` list is
    `watdiv|lubm` and it ends "on demand via `kermit-rdf`"), any `kermit-generate` mentions
    (#95's push 1 adds them), and the `generator.scale` row;
  - one `| Dimension` header line in `docs/benchmarks/README.md`.

  Write down which of #95's and #96's doc edits are already on the branch (a `H2O.md` or
  `SNAP.md` entry in the Index, columns in the matrix, `h2o`/`snap` in the kind list). Steps 3,
  4 and 5 say what to do in each case.

- [ ] **Step 2: Create `docs/benchmarks/JOB.md`.**

  Write the file with exactly this content. It keeps `<!-- J9-FILL -->` once; nothing else in it
  is a placeholder.

````markdown
# JOB (Join Order Benchmark)

The Join Order Benchmark is a join-ordering workload introduced by Leis, Gubichev, Mirchev,
Boncz, Kemper and Neumann (2015). Its 113 multi-join queries, 33 query structures with 2–6
variants that differ only in their selections, run over a real May 2013 snapshot of the
Internet Movie Database (IMDb, 21 tables), whose correlations and skew defeat the uniformity
and independence assumptions that synthetic benchmarks share with query optimisers. Kermit
generates it as a benchmark: each query's per-alias filters are applied once, by DuckDB,
during generation, and the remaining equi-joins run as a Datalog rule on kermit's engines.
That keeps JOB's real data, real selectivities and hard join orders, and gives the cost-based
optimiser (#81) and `--column-orders any` (#93) a real workload to be judged on.

JOB is the relational counterpart of the RDF benchmarks: LUBM and WatDiv derive binary
predicates from triples, while JOB's relations come from a relational schema, with several
join columns each and one benchmark per query template. For a side-by-side comparison with
other benchmarks in the suite, see [`README.md`](README.md#cross-benchmark-comparison).

## How to run

JOB needs two things outside the workspace. **DuckDB**: `KERMIT_DUCKDB_BIN` if set, else
`duckdb` on `PATH`, which `nix develop` provides (`pkgs.duckdb`); without it generation fails
with an error naming both. **The IMDb raw cache**, built once on first use and shared by every
template (see [Output layout](#output-layout)): a 1.26 GB download through the pinned mirrors,
then a build into `imdb.duckdb`, with progress lines on stderr.

### On-the-fly generation

```bash
kermit bench gen job --template N --tag STR [--output-dir PATH]
```

Produces `~/.cache/kermit/benchmarks/job-{template}-{tag}/` runnable via the unmodified
`kermit bench run`. The bench name is collision-checked against committed YAMLs at runtime.

| Flag | Default | Notes |
|------|---------|-------|
| `--template` | required | JOB template, 1–33; `job-6-dev` holds queries 6a–6f |
| `--tag` | required | Suffix on the bench name; pick a value that won't collide with a committed spec |
| `--output-dir` | `~/.cache/kermit/benchmarks` | Custom output dirs are NOT auto-discovered by `bench list/fetch/run` |

DuckDB is chosen by the environment, not a flag: `KERMIT_DUCKDB_BIN=/path/to/duckdb`.

### Declarative YAML spec (commit-and-run)

The 33 templates are committed as `benchmarks/job-1.yml` … `benchmarks/job-33.yml`.
`bench run <name>` materialises the data on first invocation:

```yaml
# benchmarks/job-6.yml
name: job-6
description: "Join Order Benchmark, template 6 (6a-6f)"
generator:
  kind: job
  template: 6
```

- `kind: job` and `template` (1–33) are both required; `template` outside 1–33 is rejected by
  `BenchmarkDefinition::validate` with `job generator template must be in 1..=33`.
- There is no `scale`: the data is the fixed IMDb dump, and the template picks the queries.
- Editing `template` and re-running errors with `SpecDrift` — pass `--force` to opt into
  regenerating. `--force` rebuilds that benchmark only; the raw cache is untouched. See
  `benchmarks/README.md` for the full schema.

**Why one benchmark per template.** `bench run` loads every relation of a definition before
it runs any query, so one benchmark holding all 113 queries' relations would load all of them
at once, `cast_info` (about 36 M rows) among them. `job-6` holds the relations of 6a–6f only;
`bench run job-6 -q 6c` selects one query but still loads them all.

```bash
kermit bench run job-6 -i all -a all --verify
```

`--verify` runs each query once and aborts on a count that differs from its `expected`, which
is DuckDB's `COUNT(*)` (see [Determinism](#determinism)). `bench run --all` materialises all
33 templates.

## Pipeline

```
imdb.tgz (mirror 1, then mirror 2)            schema.sql + 113 query files
        │ download_verified (SHA-256 pinned)         │ download_verified (pinned commit)
        │ tar -xzf → 21 CSVs                         │
        ▼                                            │
 duckdb: schema.sql, then one COPY per table         │
 (FORMAT csv, HEADER false, backslash escape);       │
 row counts checked against the pins                 │
        │                                            │
        ▼                                            ▼
   imdb.duckdb                         queries/<template><letter>.sql
        └──────── raw cache, built once, shared by all 33 templates ────────┐
                                                                            │
                          translate::parse_query (sqlparser) ◄──────────────┤
                          translate::plan_template                          │
                                   │                                        │
                                   ▼                                        │
              per template: relations (table, columns, filter)              │
                            + one Datalog rule per query                    │
                                   │                                        │
        ┌──────────────────────────┴───────────────┐                        │
        ▼                                          ▼                        │
 per relation: duckdb                       per query: duckdb ◄─────────────┘
   COPY (SELECT CAST(c AS BIGINT), …          SELECT COUNT(*) of the
         FROM table                           original query
         WHERE <filter> AND k IS NOT NULL            │
         ORDER BY id) TO '/dev/stdout'               │ becomes the query's `expected`
        │ CSV on stdout → Tuples batches             │
        │ of 65,536 rows                             │
        ▼                                            │
 RelationWriter → <relation>.parquet × K             │
        │                                            │
        └──────────────────┬─────────────────────────┘
                           ▼
              benchmark.yml (+ expected per query) + meta.json
```

Generation is out of core end to end: DuckDB streams, kermit writes Parquet batch by batch,
and no relation is ever held whole. Only the pure-Rust translator reads the SQL; DuckDB is
the only thing that evaluates it.

### DuckDB sessions

Every DuckDB call sets `memory_limit = '4GB'`, `temp_directory` inside the raw cache,
`preserve_insertion_order = true` and `enable_progress_bar = false`, and opens the database
read-only, except the raw-cache build. DuckDB's join, sort and aggregate operators spill to
disk past the limit, instead of claiming their default 80% of RAM beside a timing run. The
progress bar is off because DuckDB's CLI may draw one after about two seconds: on stderr it
would fail the empty-stderr check below, and on stdout it would corrupt the row stream. A
`COPY … TO '/dev/stdout'` is trusted only if DuckDB exits 0 with an empty stderr; otherwise the
error names the relation and quotes stderr, and no `<name>.parquet` is left behind.

## Output layout

```
~/.cache/kermit/benchmarks/job-{N}/            (job-{N}-{tag} for bench gen)
  meta.json                  GeneratedMeta — kind = "job"; written last, the cache's commit marker
  benchmark.yml              kermit BenchmarkDefinition: every variant of template N as a query,
                             each with `expected`
  <relation>.parquet × K     one per distinct (table, columns, filter); Int64 columns; rows in
                             ascending id order

~/.cache/kermit/imdb/                          the shared raw cache; not a benchmark
  imdb.duckdb                the 21 IMDb tables
  queries/1a.sql … 33c.sql   the 113 pinned queries
  provenance.json            imdb_sha256, query_commit, a SHA-256 per query file, and the DuckDB
                             version that built the cache; written last
  duckdb.tmp/                DuckDB's spill directory (SET temp_directory)
```

`meta.json` carries #95's common fields (`schema_version`, `kind`, `tag`, `generated_at_utc`,
`spec_hash`, `relation_count`, `query_count`, and `relations`: each relation's columns, rows and
SHA-256), plus JOB's:
`template`, `imdb_sha256`, `query_commit`, `query_sha256` (this template's files and
`schema.sql`), `duckdb_version` (the generation's), `raw_duckdb_version` (the one that built the
raw cache, from its `provenance.json`), and `sources` (per relation `{relation, table, filter}`,
joined by name to `relations`).

The raw cache has no `benchmark.yml` and no `meta.json`, so benchmark discovery ignores it. It
sits beside the `benchmarks/` cache, not in it: `bench clean job-6` and `bench clean --all`
remove only `~/.cache/kermit/benchmarks`, so the raw cache survives both. Delete
`~/.cache/kermit/imdb/` by hand to force a re-download and rebuild.

**When the pins change.** Every generation first compares a complete raw cache's
`provenance.json` with the pins in the binary: the dump's SHA-256, the query commit and every
query file's SHA-256. If any differs, generation stops with an error naming the field, the
cache's value, the pinned value and the directory to delete. Kermit never deletes or rebuilds
the cache itself, the same rule as a generator's `SpecDrift`, which waits for `--force`. The
DuckDB version that built the cache is recorded but not compared: it is not a pin.

It is built in `imdb.fetch-<pid>-<seq>` and `imdb.building-<pid>-<seq>` beside it and renamed into place, so
an interrupted build is never reused. A failed run removes both directories. A run stopped
outright (Ctrl-C, `SIGKILL`) leaves them behind under the pid of a process that is gone: any
`imdb.fetch-*` or `imdb.building-*` directory whose process no longer runs is safe to delete
(a later run with the same pid replaces its own leftover anyway). Two first runs at once each
download; the later to finish is discarded.

## Inference / preprocessing

Pure Rust in `kermit-generate/src/job/translate.rs`: `sqlparser` (PostgreSQL dialect) parses
each query, and `plan_template` turns one template's variants into relations and rules. The
translator works on the parsed syntax tree, never on the query text.

| Rule kind | Examples | Required by |
|-----------|----------|-------------|
| Flatten the `AND` tree | `a AND b AND c` becomes three conjuncts; `x BETWEEN 1 AND 2` stays one node | 24 of the 113 queries use `BETWEEN`, which a text splitter would cut in half |
| Classify each conjunct | `mk.movie_id = t.id`: a **join** (two aliases, `=`). `t.production_year > …`: a **filter** on `t`. A non-equality between aliases, an OR across aliases, or a condition naming no alias: an **error** naming the query and the condition | All 113: none raises one, and every condition names an alias |
| Merge joins into variables | `mk.movie_id = t.id` and `mk.movie_id = ci.movie_id` make one variable over `mk.movie_id`, `t.id`, `ci.movie_id` | 4–17 aliases and 2–10 variables per query |
| Order the variables | By the position of the first join condition that mentions them, in text order | All 113; 0 column-order conflicts |
| Name the variables | After the first alias, in text order, whose `id` the variable contains (`K`, `T`, `MI_IDX1`); `<ALIAS>_<COLUMN>` when it contains no `id` | All 113 |
| Build each alias's relation | Its join columns in variable order, then the table's `id`, unless `id` is already a join column | 589 alias occurrences join on their own `id` |
| Exclude NULL join keys | `… AND movie_id IS NOT NULL` on every join column | Any nullable join column, such as `cast_info.person_role_id` (an SQL equi-join never matches NULL) |
| Share relations | Equal (table, ordered columns, filter text) within a template is one file | 465 files for 113 queries across 33 templates |
| Cast | Every column is `CAST(… AS BIGINT)` | The Parquet reader reads Int64 columns only |

The rest of the translation, in the order it matters when reading a rule:

**Variables are ordered by text.** The order uses no data: it is the order in which each
query's join conditions first mention a variable. Under `--column-orders stored` every atom is
pinned to its stored column order, which the relation files give in variable order, so the
order a query runs in is fixed by how it is written: the optimisers choose only among
variables that no atom orders against each other. JOB therefore measures one neutral order
under `stored`, and comparisons between optimisers belong under `--column-orders any`.

**The `id` is carried.** An alias's `id` rides along as a `_` in the rule. It keeps filtered
rows distinct, so the sorted tries (which are sets) and HashTrie (a multiset) agree on one
`expected` count. When `id` is itself a join column (`t.id = mk.movie_id`), it is stored once,
as that column. A `_` means an unused named variable, so a carried `id` costs a column, not a
row.

**Relation names.** Unfiltered: `<table>`. Filtered: `<table>_<alias>_<letters>`, where
`<alias>` is the alias of the first query (in variant order) that uses the relation and
`<letters>` are the variant letters of every query that does. Names match `[a-z][a-z0-9_]*`,
which the writer enforces. The sharing key includes the column order, and the filter text is
sqlparser's printout of the alias's conjuncts with `alias.col` rewritten to `col`, joined by
`AND` in text order, so whitespace differences vanish. Template 8's variants rename aliases
(`n1`/`an1`, `n`/`an`, `a1`) and still share, because the key never contains an alias.
Relations are shared only within a template: each `job-N` directory is self-contained.

**The rule.** `Q(<every join variable, in order>) :- <one atom per FROM alias, in FROM order>.`
The full head makes the lexicographic optimiser follow the text order. No query needs the
selection rewrite: no alias has two columns in one join variable.

## Workload reference

The 113 queries are not reproduced here: the query repository carries no licence (see
[Vendored generator](#vendored-generator)). They are fetched and checked by SHA-256 at
generation time. Each query becomes one rule in its template's `benchmark.yml`, named as JOB
names it (`6a`).

### Template 6, as emitted

Nine relation files for six queries (from the design; the tests pin the real output):

```
6a: Q(K, T, N) :- cast_info(T, N, _), keyword_k_ace(K), movie_keyword(K, T, _),
                  name_n_abcde(N), title_t_a(T).

movie_keyword(keyword_id, movie_id, id)   unfiltered            6a–6f
cast_info(movie_id, person_id, id)        unfiltered            6a–6f
keyword_k_ace(id)                         keyword = '…'         6a, 6c, 6e
keyword_k_bdf(id)                         keyword IN (…)        6b, 6d, 6f
name_n_abcde(id)                          name LIKE '%…%'       6a–6e
name(id)                                  unfiltered            6f
title_t_a / title_t_bc / title_t_def(id)  production_year > …
```

Across all 33 templates this gives 465 relation files.

### The 33 templates

Counted from the pinned `FROM` clauses; the alias count is the number of atoms in each rule,
and it is the same for every variant of a template. Cardinalities are not listed: they are
DuckDB's `COUNT(*)` for the dump actually in use, written to each query's `expected` in
`benchmark.yml`.

| Template | Variants | Aliases | Template | Variants | Aliases | Template | Variants | Aliases |
|----------|----------|---------|----------|----------|---------|----------|----------|---------|
| 1 | a–d | 5 | 12 | a–c | 8 | 23 | a–c | 11 |
| 2 | a–d | 5 | 13 | a–d | 9 | 24 | a–b | 12 |
| 3 | a–c | 4 | 14 | a–c | 8 | 25 | a–c | 9 |
| 4 | a–c | 5 | 15 | a–d | 9 | 26 | a–c | 12 |
| 5 | a–c | 5 | 16 | a–d | 8 | 27 | a–c | 12 |
| 6 | a–f | 5 | 17 | a–f | 7 | 28 | a–c | 14 |
| 7 | a–c | 8 | 18 | a–c | 7 | 29 | a–c | 17 |
| 8 | a–d | 7 | 19 | a–d | 10 | 30 | a–c | 12 |
| 9 | a–d | 8 | 20 | a–c | 10 | 31 | a–c | 11 |
| 10 | a–c | 7 | 21 | a–c | 9 | 32 | a–b | 6 |
| 11 | a–d | 8 | 22 | a–d | 11 | 33 | a–c | 14 |

Self-joins occur: a table may appear under several aliases (`info_type AS it1`, `info_type AS
it2`), each becoming its own relation, or one shared file when their filters are equal.

### What is kept, what changes

Kept: the schema, the May 2013 data, JOB's 33 × (2–6) query structure, every join and every
filter exactly as written, and JOB's query names.

Changed:

- **`MIN(…)` becomes a count.** The published queries wrap every projected attribute in `MIN`
  (the paper's footnote 4: to keep communication from dominating). Kermit counts the joined
  rows instead, `COUNT(*)`, the bag cardinality that a `_` gives.
- **NULL join keys are excluded**, as an SQL equi-join excludes them.
- **Filters are evaluated at generation, not at join time.** A filter on one table commutes
  with an inner join and every optimiser applies it before joining, so JOB's join problem is
  intact; only what evaluates the filter expressions moves, to DuckDB.
- **The head lists every join variable**, not the SELECT list.
- **Keys are IMDb's own integer ids**, not dictionary ids.
- **A filters-stripped "JOB-shape" variant is out of scope.**

## Determinism

A JOB benchmark is reproducible from its spec and its inputs: generating the same template
from the same dump and the same query commit twice yields byte-identical relations. There is no
random number generator, and no clock reaches the data (`generated_at_utc` is in `meta.json`
only).

- **Row order is fixed by `ORDER BY id`.** DuckDB's parallel scan promises no order, and build
  cost (#66) and byte-identical regeneration both depend on one. Build cost depends on row
  order; answers never do.
- **Kermit writes every byte.** The Parquet writer re-chunks rows into fixed 2^16-row buffers,
  so a file's bytes do not depend on how its rows were batched, and regeneration is
  byte-identical across DuckDB versions as long as DuckDB returns the same rows. The bytes are
  stable for a fixed kermit build only (the Parquet writer's `created_by`).
- **`expected` is an oracle independent of kermit's executors:** DuckDB's `COUNT(*)` of the
  original query.
- **Identification after the fact:** `meta.json` records the IMDb dump's SHA-256, the query
  commit and each query file's SHA-256, `duckdb --version`, and `sources`.

## Practical scale ceiling

Generation is not the limit. The **raw cache** needs about 5 GB of disk and a few minutes to
build, and each template then takes seconds to a couple of minutes (design estimates; the
measured figures replace them at acceptance). The dump is 1,263,193,115 bytes; the first build
stages it beside the cache with its extracted CSVs (3.6 GB, per the paper) and the growing
`imdb.duckdb`, then deletes the staging directory, so peak disk during that first build is
higher than the finished cache.

The **run phase** sets the memory ceiling. `cast_info` holds about 36 M rows and `movie_info`
15 M (Leis et al., §2.1), and a trie over `cast_info` may not fit for the `cast_info` templates
on a 30 GB host, HashTrie least of all. The table below records, per template, the output
size and the peak memory of `bench run job-N -i all -a all --verify` for each structure, or the
failure.

<!-- J9-FILL: replace the placeholder row below with one row per template (job-1 to job-33: the total bytes of its relation files, the peak memory of the verify run for each structure, or the failure), correct the disk and time figures in this section to the measured ones, then delete this comment. -->

| Template | Output size | Peak memory (tree-trie) | Peak memory (column-trie) | Peak memory (hash-trie) | Notes |
|----------|-------------|-------------------------|---------------------------|-------------------------|-------|
| *measured at acceptance* | — | — | — | — | — |

Cells that do not fit are listed here with their measured sizes, not hidden: see
[Future work](#future-work).

## Vendored generator

Nothing from JOB is committed. The query repository carries no licence and the IMDb dump is
1.26 GB, so kermit pins both by SHA-256 in `kermit-generate/src/job/pins.rs` and fetches them
on first use.

| Field | Value |
|-------|-------|
| Query source | <https://github.com/gregrahn/join-order-benchmark> (a mirror, not maintained by the original authors) |
| Commit | `a39603662e023e449cb2121997a5034df9e02ebf` |
| Files | `schema.sql` and the 113 queries `1a.sql` … `33c.sql`: 114 files, each pinned by SHA-256. `fkindexes.sql` is not used |
| Query licence | None: the repository has no licence file |
| IMDb dump | `imdb.tgz`, 1,263,193,115 bytes, the CSVs of May 2013 (21 tables); SHA-256 `IMDB_SHA256` in `pins.rs` |
| Mirrors, tried in order | <https://bonsai.cedardb.com/job/imdb.tgz>, then <http://event.cwi.nl/da/job/imdb.tgz> |
| IMDb licence | Non-commercial use (Leis et al. §2.1); the terms are at <http://www.imdb.com/interfaces> |
| Row counts | One per table, pinned in `IMDB_TABLE_ROWS`; the build refuses a table that differs |
| DuckDB | External tool: `pkgs.duckdb` in the flake, or `KERMIT_DUCKDB_BIN` |
| Local path | `~/.cache/kermit/imdb/` (the platform cache dir) |
| Committed | Digests and URLs only |

Kermit downloads the dump from the mirrors and never commits it or any file derived from it.
Both mirrors serve a file of the same size; whichever is used, the pinned digest is checked.

Changing a pin is a deliberate step: fetch the new file, `sha256sum` it into `pins.rs`, measure
the new table row counts with the DuckDB CLI, delete `~/.cache/kermit/imdb/`, and regenerate
every template with `--force`. Until the old cache is deleted, every `job` generation stops
with the error described under [Output layout](#output-layout). Timings from either side of a
pin change are not comparable.

## Tests

| Test | What it validates | Gate |
|------|-------------------|------|
| `kermit-generate/src/job/translate.rs` unit tests | Parsing and classifying JOB-shaped SQL: `BETWEEN`, OR in parentheses, quoted `AND` and dots, `LIKE`/`IN`/`IS NULL`, self-joins; variable order and names; `id` carried only when not a join column; relation sharing and naming; one error per unsupported shape | None |
| `kermit-generate/src/job/pins.rs` unit tests | Pinned digests are unique 64-character hex; query URLs carry the commit | None |
| `kermit-generate/src/job/duckdb.rs` unit tests | Session settings, batching and order of streamed rows, quoted stderr, read-only | `duckdb` on `PATH` or `KERMIT_DUCKDB_BIN` |
| `kermit-generate/src/job/raw.rs` unit tests | The raw-cache build on a fixture (row-count mismatch leaves no cache; a leftover staging directory is not reused; provenance is written last); `ensure`'s mirror fallback, queries-before-dump order, staging cleanup and a first run under a missing cache parent directory; a complete cache built from other pins is refused, naming the field and both values, and is left in place | The build and `ensure` tests need `duckdb`; the mirror loop, staging guard, pins and provenance-comparison tests need nothing |
| `kermit-bench` `definition.rs` tests | `kind: job` parses; templates 0 and 34 are rejected; `spec_hash` changes with the template | None |
| `kermit/tests/job_generate.rs` | The fixture pipeline: filtered rows in id order; kermit's count equals DuckDB's `COUNT(*)` on every structure × optimiser × `stored`/`any`; byte-identical regeneration; a failed filter leaves no relation and no `meta.json`; `meta.json` records sources and versions | `duckdb` available |
| `kermit/tests/cli_job_gen.rs` | `bench gen job` and `bench run job-1` through the real binary over a fixture raw cache, with the platform cache redirected so nothing is written under `~/.cache/kermit`: a template outside 1–33, a missing DuckDB and a tag that makes an invalid benchmark name fail before any directory is created; `bench gen job` writes `job-N-<tag>`; `bench run job-1` materialises the committed YAML, verifies every count, then hits the cache, reports drift and rebuilds under `--force` | Linux (`XDG_CACHE_HOME`); the first three need no DuckDB, the rest need `duckdb` |
| `kermit/tests/job_translate.rs` | The real 113 queries (fetched and digest-checked at test time): every rule parses and passes `kermit::db::validate_query` under `--column-orders stored` against its template's relations, the checks and rewrites `bench run` applies; every relation name is parser-valid; 33 templates, 113 rules and 465 relation files; template 12's three variants share one `company_name` relation | Network to `raw.githubusercontent.com` (CI too); skips only when the first fetch cannot connect; a digest mismatch fails; not miri |

The translator test on real queries lives in the `kermit` crate because `kermit-generate`
does not depend on `kermit-algos`; the fixture pipeline does for the same reason (it counts
through `kermit::db`). The fixture pipeline is the load-bearing correctness check for the
filter stage and the translation; finding it green is the precondition for relying on a
`job-N` benchmark. CI installs no DuckDB, so the DuckDB-gated tests run only under `nix
develop`.

## References

- V. Leis, A. Gubichev, A. Mirchev, P. Boncz, A. Kemper, T. Neumann. *How Good Are Query
  Optimizers, Really?* PVLDB 9(3), 204–215, 2015.
  <http://www.vldb.org/pvldb/vol9/p204-leis.pdf>
- The authors' own copy of the queries, as the paper links it:
  <http://www-db.in.tum.de/~leis/qo/job.tgz>
- Pinned query repository: <https://github.com/gregrahn/join-order-benchmark>
- IMDb data and terms: <http://www.imdb.com/interfaces>
- DuckDB: <https://duckdb.org>
- `sqlparser` crate: <https://crates.io/crates/sqlparser>
- Design: `docs/specs/2026-10-15-job-benchmark-design.md`
- Module: `kermit-generate/src/job/` (`translate.rs`, `duckdb.rs`, `raw.rs`, `pins.rs`)
- Sibling benchmark references: [`LUBM.md`](LUBM.md), [`WATDIV.md`](WATDIV.md)

## Future work

- **Cells that do not fit.** The memory table lists the template × structure cells that run out
  of memory on this host. A lower-memory configuration (the lazy HashTrie layout,
  `--ds-layout-expansion lazy`, builds children on demand) might move the ceiling; it is not
  measured.
- **A filters-stripped "JOB-shape" variant.** Dropping the filters changes the join problem
  (much larger inputs), so it would be a second workload with its own `expected`, not a flag.
- **An outdated-cache floor for `job`.** The spec hash covers only `template`. If the
  translator's output ever changes for a fixed template, add a per-kind floor
  (`MetaHeader::outdated_reason`) rather than letting old caches pass as hits.
- **Aggregate-faithful counting.** Kermit counts joined rows; reproducing JOB's `MIN(…)`
  projections would need aggregation in the engine, which is out of scope.
````

  Two facts the text states from memory rather than a source in the tree are checked in Step 6.

- [ ] **Step 3: Edit `docs/benchmarks/README.md`.**

  **3a. The Index.** After the WatDiv entry (the last bullet of `## Index`):
  ```
  - [WatDiv](WATDIV.md) — ~12 400 mechanically-generated stress queries per
    workload spanning star, snowflake, and chain shapes. The distributional
    counterpart to LUBM.
  ```
  add, as the next bullet (after any entries #95 or #96 added, in landing order):
  ```
  - [JOB](JOB.md) — the Join Order Benchmark: 113 multi-join queries (33
    templates) over a real IMDb snapshot, with each alias's filters applied
    at generation so kermit runs JOB's real joins. The join-ordering
    workload, and the natural evaluation of the cost-based optimiser and
    `--column-orders any`.
  ```

  **3b. The matrix.** The checklist (`Adding a new benchmark`, step 1) says a new benchmark adds
  a column's worth of cells, one per row. Replace the table at `## Cross-benchmark comparison`.

  Before, as it reads at 5dd1691:
  ```
  | Dimension | LUBM | WatDiv |
  |---|---|---|
  | Query count per dataset | 14 | ~12 400 per stress file (12 committed files) |
  | Authoring | Hand-designed for specific OWL features | Mechanically generated from templates |
  | Predicate arity | All binary + unary type lookups | All binary |
  | Inference required | OWL-Lite (subClassOf, subPropertyOf, transitivity, inverseOf, realisation) | None — data is pre-materialised |
  | Triangle queries | **Q2, Q9 — explicit hand-designed** | Incidental (e.g. q0010 sharing a country variable) |
  | Self-joins | None | Yes (e.g. `friendof(V_v2, V_v2)`) |
  | Result oracle | Paper Table 3, manually transcribed | None — vendored binary emits no `.desc` |
  | Reproducibility | Deterministic per `(seed, scale)` | Non-deterministic; tag-based snapshots |
  | Sandbox | — | bwrap required (or `--no-bwrap`) |
  | Vendored asset | Committed (~2.9 MB jar) | Committed (~360 KB binary) |
  | Pipeline command | `kermit bench gen lubm` | `kermit bench gen watdiv` |
  ```
  After:
  ```
  | Dimension | LUBM | WatDiv | JOB |
  |---|---|---|---|
  | Query count per dataset | 14 | ~12 400 per stress file (12 committed files) | 113 in 33 templates; one benchmark per template |
  | Authoring | Hand-designed for specific OWL features | Mechanically generated from templates | Hand-written SQL (Leis et al. 2015), translated mechanically to Datalog |
  | Predicate arity | All binary + unary type lookups | All binary | Mixed: an alias's join columns, plus a carried `id` unless `id` is itself a join column |
  | Inference required | OWL-Lite (subClassOf, subPropertyOf, transitivity, inverseOf, realisation) | None — data is pre-materialised | None — relational data; filters applied at generation |
  | Triangle queries | **Q2, Q9 — explicit hand-designed** | Incidental (e.g. q0010 sharing a country variable) | None designed — the workload targets join ordering over key/foreign-key joins |
  | Self-joins | None | Yes (e.g. `friendof(V_v2, V_v2)`) | Yes, through aliases (`info_type` under two aliases); no repeated variable within an atom |
  | Result oracle | Paper Table 3, manually transcribed | None — vendored binary emits no `.desc` | **DuckDB `COUNT(*)` of each original query, independent of kermit** |
  | Reproducibility | Deterministic per `(seed, scale)` | Non-deterministic; tag-based snapshots | Deterministic per `(template, dump, query commit)`; byte-identical regeneration |
  | Sandbox | — | bwrap required (or `--no-bwrap`) | — (DuckDB runs unsandboxed, under a 4 GB memory limit) |
  | Vendored asset | Committed (~2.9 MB jar) | Committed (~360 KB binary) | None — pinned by SHA-256, fetched on first use (1.26 GB dump) |
  | Pipeline command | `kermit bench gen lubm` | `kermit bench gen watdiv` | `kermit bench gen job` |
  ```
  If #95's and #96's columns are already in the table on the branch, keep them, append JOB's
  column after theirs, and use the same JOB cell for each row. The rows are the same eleven.

- [ ] **Step 4: Edit `benchmarks/README.md`.**

  Check each "before" is present first with `grep -cF '<before>' benchmarks/README.md`
  (expect `1`). A "before" that has already been changed by #95 or #96 is read, not
  overwritten: add `job` to what is there.

  **4a. File layout.** After the bullet that ends `(distinct from the frozen \`watdiv-stress-*\`
  snapshots covered later).` add:
  ```
  - `job-1.yml` … `job-33.yml` are declarative generator specs for the Join Order Benchmark, one per template (`job-6` holds queries 6a–6f); see `docs/benchmarks/JOB.md`.
  ```

  **4b. The kind row.** Before:
  ```
  | `generator.kind`            | `watdiv` \| `watdiv-basic` \| `lubm` | yes      | Selects the underlying `kermit-rdf` pipeline. |
  ```
  After:
  ```
  | `generator.kind`            | `watdiv` \| `watdiv-basic` \| `lubm` \| `job` | yes      | Selects the generator: a `kermit-rdf` pipeline for the RDF kinds, `kermit-generate` for `job`. |
  ```

  **4c. The scale row.** Before:
  ```
  | `generator.scale`           | u32 (≥ 1)          | yes      | Scale factor (`watdiv -d <N>` or `lubm-uba -u <N>`). |
  ```
  After:
  ```
  | `generator.scale`           | u32 (≥ 1)          | `watdiv`, `watdiv-basic`, `lubm` | Scale factor (`watdiv -d <N>` or `lubm-uba -u <N>`). `job` has no scale: its data is a fixed IMDb dump and `template` picks the queries. |
  ```
  If this row no longer reads `yes` (#96 landed the same edit first), read it and only add the
  `job` clause if it is missing.

  **4d. The JOB fields.** After the LUBM-specific table (its last row is
  `| \`generator.queries\` …`) and before the line `Structural invariants are checked at load time by`, add:
  ```
  JOB-specific fields (under `generator:` when `kind: job`):

  | Field                       | Type        | Required | Description |
  |-----------------------------|-------------|----------|-------------|
  | `generator.template`        | u8 (1–33)   | yes      | The JOB template: `job-<N>` holds every variant of template N (`job-6` = 6a–6f). `BenchmarkDefinition::validate` rejects a template outside 1–33. |

  A `job` benchmark needs DuckDB (on `PATH` inside `nix develop`, or at `KERMIT_DUCKDB_BIN`) and, on first use, downloads the 1.26 GB IMDb dump into a raw cache shared by every template, `~/.cache/kermit/imdb/`, which sits outside the `benchmarks/` cache. See `benchmarks/job-6.yml` and `docs/benchmarks/JOB.md`.
  ```
  Keep exactly one blank line between this block and the `Structural invariants` line.

  **4e. The example.** After the last example block of `## Declarative generator example
  (WatDiv)`, the one that ends
  ```
  generator:
    kind: lubm
    scale: 1
    queries: [q1, q3, q5]
  ```
  (and its closing fence), add:
  ````
  ```yaml
  name: job-6
  description: "Join Order Benchmark, template 6 (6a-6f)"
  generator:
    kind: job
    template: 6
  ```

  `job-6` generates every variant of JOB template 6 over IMDb, with each alias's filters applied during generation. There is no `scale`.
  ````
  Use the `description:` line printed by Step 1 for `benchmarks/job-6.yml`.

  **4f. Caching.** Before:
  ```
  Exact layout comes from `dirs::cache_dir()` (see `kermit-bench/src/cache.rs`). Use `kermit bench clean <name>` to drop a single benchmark's cache, or `kermit bench clean --all` to purge everything.
  ```
  After:
  ```
  Exact layout comes from `dirs::cache_dir()` (see `kermit-bench/src/cache.rs`). Use `kermit bench clean <name>` to drop a single benchmark's cache, or `kermit bench clean --all` to purge every benchmark's cache. The JOB generator's raw IMDb cache (`<cache dir>/kermit/imdb/`) is shared by all its templates and sits beside `benchmarks/`, so neither command removes it.
  ```

- [ ] **Step 5: Edit `CLAUDE.md`.**

  Check each anchor with `grep -cF` (expect `1`) before editing.

  **5a. Build Commands.** After the line
  ```
  cargo run -- bench gen lubm --scale 1 --tag dev                    # Generate LUBM benchmark on the fly (imperative)
  ```
  add (and after any `bench gen h2o` / `bench gen snap` lines #95 or #96 added, so each
  generator adds only its own line):
  ```
  cargo run -- bench gen job --template 6 --tag dev                  # Generate JOB template 6 on the fly (imperative; needs DuckDB, downloads the 1.26 GB IMDb dump on first use)
  ```

  **5b. Benchmark Reference Docs.** After
  ```
  - `docs/benchmarks/WATDIV.md` — WatDiv usage, stress-template parameters, vendoring rules, non-determinism discipline.
  ```
  (and after any entries #95 or #96 added) add:
  ```
  - `docs/benchmarks/JOB.md` — JOB usage, the SQL-to-Datalog translation rules (variables, relations, carried ids), the shared IMDb raw cache, DuckDB's settings, disk and time, the measured memory table.
  ```

  **5c. The gotcha.** Insert this bullet after the `**LUBM jar regeneration**` bullet (so the
  generators' gotchas stay together):
  ```
  - **JOB generation (`kind: job`, #97)**: `bench gen job --template N --tag STR`, or `bench run job-N` on a committed `benchmarks/job-N.yml`, shells out to DuckDB (`KERMIT_DUCKDB_BIN`, else `duckdb` on `PATH`, which `nix develop` provides as `pkgs.duckdb`; without it generation fails with an error naming both). The IMDb dump and the 114 pinned query-repository files (`gregrahn/join-order-benchmark@a396036`, SHA-256s in `kermit-generate/src/job/pins.rs`; the repo has no licence, so never commit its text) are downloaded once by `raw::ensure` through `download_verified` (1.26 GB) into a staging directory beside the cache and built into one **shared raw cache**, `~/.cache/kermit/imdb/`, which every template reuses. The cache keeps only `imdb.duckdb`, `queries/` and `provenance.json`: the tarball, the extracted CSVs and `schema.sql` are deleted with the staging directory after the build, and a killed run's `imdb.fetch-*` / `imdb.building-*` leftovers are safe to delete. `bench clean --all` removes only `~/.cache/kermit/benchmarks`, so the raw cache survives it, and `ensure` compares a complete cache's `provenance.json` with the pins (dump SHA-256, query commit, every query file's SHA-256) and refuses a mismatch with an error naming the field, both values and the directory to delete, never deleting or rebuilding it itself (like spec drift): delete it by hand after a pin change. Each relation is a DuckDB `COPY (SELECT … ORDER BY id) TO '/dev/stdout'` handed to `RelationSink` in 65,536-row batches, so no relation is ever held whole; `ORDER BY id` is load-bearing (DuckDB's parallel scan promises no order, and build cost and byte-identical regeneration depend on one). An alias's relation is its join columns in variable order, then its `id` carried as `_` unless `id` is itself a join column (the id keeps filtered rows distinct, so sets and multisets agree on `expected`), and a query's variables are ordered by first appearance among its join conditions (text order, data-blind), so `--column-orders stored` measures that fixed order and optimiser comparisons belong under `any`. `expected` is DuckDB's `COUNT(*)` of the original query. `kermit/tests/job_translate.rs` fetches the pinned files at test time (CI too) and skips only when its first fetch cannot connect; a digest mismatch fails it. See `docs/benchmarks/JOB.md`.
  ```

  **5d. Lists #95 and #96 maintain.** These lines are not in this tree at 5dd1691, so the text
  is not quoted: read each, and add only `job`, as the text reads at landing (every generator
  adds only its own kind). Skip any that is absent:
  - the `kermit` crate row's `gen [watdiv|lubm …]` list (`grep -n 'gen \[' CLAUDE.md`): add `job`;
  - the `kermit-generate` row of the Workspace Architecture (`grep -n 'kermit-generate' CLAUDE.md`),
    where it lists its generators and modules: add `job` (the JOB generator: `translate`,
    `duckdb`, `raw`, `pins`), and, if the row or its gotcha names the crate's third-party
    dependencies, `sqlparser` (the translator's parser);
  - `Declarative generator YAMLs` gotcha's `kind: watdiv|lubm` list: add `job`, without a
    `scale` (`kind: job, template: N`).

  **5e. "Adding a new benchmark" (companion to 5d).** Unlike 5d's lists, this sentence is in
  the tree at 5dd1691, so it is quoted. Check it first:
  `grep -c 'For declarative generators' CLAUDE.md` prints `1`. Before:
  ```
  For declarative generators: declare a `generator: { kind: watdiv|lubm, scale: N, ... }` block — `bench run <name>` will materialise the data on demand via `kermit-rdf` (see `benchmarks/README.md` and `kermit/src/materialize.rs::materialize`).
  ```
  After:
  ```
  For declarative generators: declare a `generator: { kind: watdiv|lubm, scale: N, ... }` block (or `{ kind: job, template: N }`, which has no scale) — `bench run <name>` will materialise the data on demand via `kermit-rdf` (via `kermit-generate` for `job`; see `benchmarks/README.md` and `kermit/src/materialize.rs::materialize`).
  ```
  If #95 or #96 landed first and already changed this sentence (their kinds in the `kind:`
  list, or `kermit-generate` in the "via …" clause), read it as it stands and add only what is
  missing: `job` with its `template: N` and no-scale note, and `job` in the "via …" clause.
  Never remove what they added; each generator adds only its own kind.

- [ ] **Step 6: Check the docs.**

  **Tables.** Every row of every table in the new and edited Markdown has as many cells as its
  header (cells separated by unescaped `|`):
  ```bash
  python3 -I - docs/benchmarks/JOB.md docs/benchmarks/README.md benchmarks/README.md <<'EOF'
  import re, sys
  bad = 0
  for path in sys.argv[1:]:
      lines = open(path, encoding="utf-8").read().split("\n")
      i = 0
      while i < len(lines):
          if lines[i].startswith("|"):
              j = i
              while j < len(lines) and lines[j].startswith("|"):
                  j += 1
              widths = {len(re.split(r"(?<!\\)\|", row)) for row in lines[i:j]}
              if len(widths) != 1:
                  print(f"{path}:{i + 1}: rows have different cell counts {sorted(widths)}")
                  bad += 1
              i = j
          else:
              i += 1
  sys.exit(1 if bad else 0)
  EOF
  echo "exit $?"
  ```
  Expected: `exit 0` and no other output. (The 33-template table's header and rows each have nine
  cells.)

  **The placeholder and the links.**
  ```bash
  grep -c 'J9-FILL' docs/benchmarks/JOB.md
  grep -rn 'J9-FILL' --include='*.md' . | grep -v 'docs/benchmarks/JOB.md\|docs/superpowers/plans'
  for f in README.md LUBM.md WATDIV.md; do ls docs/benchmarks/$f; done
  grep -o '(\(README\|LUBM\|WATDIV\)\.md[^)]*)' docs/benchmarks/JOB.md | sort -u
  ```
  Expected: `1`; no other file mentions `J9-FILL`; the three files exist; the links are
  `(README.md#cross-benchmark-comparison)`, `(LUBM.md)`, `(WATDIV.md)` and
  `(#output-layout)`-style anchors only. The anchors `#output-layout`, `#determinism`,
  `#vendored-generator` and `#future-work` each match a `##` heading in JOB.md.

  **The constants the doc states, against the code.** Each must match; fix the doc (not the
  code) if one does not:
  ```bash
  grep -n 'MEMORY_LIMIT: &str' kermit-generate/src/job/duckdb.rs        # "4GB"
  grep -n 'BATCH_ROWS: usize' kermit-generate/src/job/duckdb.rs         # 1 << 16
  grep -n 'DUCKDB_BIN_ENV: &str' kermit-generate/src/job/duckdb.rs      # "KERMIT_DUCKDB_BIN"
  grep -n 'RAW_DIR_NAME: &str' kermit-generate/src/job/raw.rs           # "imdb"
  grep -n 'IMDB_URLS' -A3 kermit-generate/src/job/pins.rs | head -6     # bonsai.cedardb.com first, event.cwi.nl second
  grep -n 'QUERY_COMMIT: &str' kermit-generate/src/job/pins.rs          # a39603662e023e449cb2121997a5034df9e02ebf
  grep -n 'job generator template must be' kermit-bench/src/definition.rs
  grep -n 'temp_directory\|preserve_insertion_order\|enable_progress_bar' kermit-generate/src/job/duckdb.rs | head -6   # all three settings in the prelude
  grep -n 'fkindexes' kermit-generate/src/job/pins.rs | wc -l           # 0
  ```
  And the three statements JOB.md makes from the design that no file in the tree yet proves:
  `grep -n 'duckdb.tmp' kermit-generate/src/job/raw.rs` (the spill directory's name),
  `grep -n 'imdb.building' kermit-generate/src/job/raw.rs` (the build's staging name), and
  `grep -n 'ESCAPE' kermit-generate/src/job/raw.rs` (the CSV dialect). Correct the doc to the
  code if one differs.

  **Facts from the paper.** The drafter checked these against the paper's text; re-check them
  in `/tb/Source/Academia/kermit-msc-thesis/background/markdown/Leis et al. - 2015 - How good are query optimizers, really.md`:
  `grep -n 'Vol. 9, No. 3'` (PVLDB 9(3)), `grep -nx '204\|215'` (the first and last page
  numbers, standing alone on their lines), `grep -n '36 M'` (`cast_info`),
  `grep -n 'noncommercial'` (the licence wording) and `grep -n 'job.tgz'` (the authors' link).
  All must match JOB.md's text.

- [ ] **Step 7: Commit.**
  ```bash
  WT=/tb/Source/Academia/kermit/.loom/worktrees/aidanb/job_18dea468f74c13c3
  cd "$WT"
  git add docs/benchmarks/JOB.md docs/benchmarks/README.md benchmarks/README.md CLAUDE.md
  git commit -m "docs: JOB benchmark reference, README rows and CLAUDE.md (#97)" \
    -m "docs/benchmarks/JOB.md follows the benchmark-doc schema: sources and citations, how to run, the pipeline and DuckDB's session settings, the translation rules (variables in text order, carried ids, relation naming and sharing), what is kept and what changes, determinism, the raw cache, disk and time, and a memory table whose row the acceptance run fills. The comparison matrix and Index gain JOB; benchmarks/README.md gains the job kind and its template field and stops requiring a scale for every kind; CLAUDE.md gains the reference entry, the bench gen job command, the generator-kind mentions and one gotcha." \
    --trailer "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>" \
    --trailer "Claude-Session: https://claude.ai/code/session_01WeRnvdstSxUY8CJGSx1NFB" \
    -- docs/benchmarks/JOB.md docs/benchmarks/README.md benchmarks/README.md CLAUDE.md
  git status --short
  ```
  `docs/benchmarks/JOB.md` is new, so it was staged by name first; the commit is by pathspec,
  never bare, `-a` or `--amend`. Expected: `git status --short` prints nothing. `J9-FILL` is
  still in `JOB.md` once; J9 removes it.

---

# Part 5: acceptance (J9)

### Task J9: Acceptance, the memory table and the final gate (design session)

**Needs:** J1–J8 committed; `FULL_EXIT`; a heavy-run slot from the benchmark co-ordinator (these runs take hours and must not overlap a timing run); the user's go-ahead for the 1.26 GB download and for every run below.

**Files:**
- Modify: `docs/benchmarks/JOB.md` (the `J9-FILL` table and the "Practical scale ceiling" figures J8 left for this task)

**Where things live.** `/tmp` is RAM-backed, so nothing here goes in `$SCRATCH`. Everything goes under `RUNS=/tb/Source/Academia/kermit-bench-runs/job-97` (on disk, outside the repo), including a private cache `XDG_CACHE_HOME="$RUNS/xdg"`, so the shared `~/.cache/kermit` is never touched. Each block below is a separate shell: it sets its own variables, and the scripts run under `bash` (the session shell is zsh, which does not word-split `$var`).

- [ ] **Step 1: Build the release binary and write the run scripts**

```bash
WT=/tb/Source/Academia/kermit/.loom/worktrees/aidanb/job_18dea468f74c13c3
RUNS=/tb/Source/Academia/kermit-bench-runs/job-97
mkdir -p "$RUNS/xdg" "$RUNS/gen" "$RUNS/accept"
cd "$WT" && nix develop --command env CARGO_BUILD_JOBS=2 cargo build --release -p kermit
nix develop --command sh -c 'command -v duckdb && duckdb --version' | tee "$RUNS/duckdb.txt"
```

Expected: the build succeeds; `duckdb.txt` holds the DuckDB path (`/nix/store/…-duckdb-1.4.4/bin/duckdb`) and `v1.4.4 …`. The scripts below run the release binary directly, with `KERMIT_DUCKDB_BIN` set from that path, so no `nix develop` process sits in any measured process tree.

Write `$RUNS/env.sh`, sourced by every script:

```bash
# Sourced by the J9 scripts: one place for the paths, guarded.
set -euo pipefail
WT=/tb/Source/Academia/kermit/.loom/worktrees/aidanb/job_18dea468f74c13c3
RUNS=/tb/Source/Academia/kermit-bench-runs/job-97
export XDG_CACHE_HOME="$RUNS/xdg"
export KERMIT_DUCKDB_BIN="$(head -1 "$RUNS/duckdb.txt")"
KERMIT="$WT/target/release/kermit"
TIME=/run/current-system/sw/bin/time   # GNU time; `time` is a shell keyword
[ -x "$KERMIT" ] && [ -x "$KERMIT_DUCKDB_BIN" ] && [ -x "$TIME" ] || { echo "env.sh: missing binary" >&2; exit 1; }
case "$XDG_CACHE_HOME" in "$RUNS"/*) ;; *) echo "env.sh: cache outside \$RUNS" >&2; exit 1 ;; esac
cd "$WT"
```

- [ ] **Step 2: `bench gen job` every template, timing generation**

`bench fetch` does not materialise generator benchmarks (`run_fetch` only calls `ensure_cached`), so generation is timed through `bench gen job`, which the acceptance criteria require to work anyway. Write `$RUNS/gen.sh`:

```bash
# Times `bench gen job` for every template; the first one also downloads and builds the raw cache.
source /tb/Source/Academia/kermit-bench-runs/job-97/env.sh
for n in $(seq 1 33); do
  set +e
  "$TIME" -f "%e s %M KiB" -o "$RUNS/gen/job-$n.time" \
    "$KERMIT" bench gen job --template "$n" --tag accept --output-dir "$RUNS/gen" \
    > "$RUNS/gen/job-$n.log" 2>&1
  rc=$?
  set -e
  echo "job-$n gen rc=$rc $(tail -1 "$RUNS/gen/job-$n.time")"
done | tee "$RUNS/gen.log"
du -sb "$XDG_CACHE_HOME/kermit/imdb" "$RUNS/gen" | tee "$RUNS/disk.log"
```

Run: `bash /tb/Source/Academia/kermit-bench-runs/job-97/gen.sh`

Expected: `rc=0` on every line. `job-1.log` shows the download progress and the raw-cache build; no later log has a download line. `$RUNS/gen/job-$n-accept/meta.json` exists for every `n`. `disk.log` gives the finished raw cache's size and all 33 templates' output. Note job-1's time (dump download + build + template) apart from the rest.

- [ ] **Step 3: Materialise `job-1` … `job-33` through `bench run`, untimed**

This exercises the `dispatch` arm from the committed YAMLs and leaves every template cached, so Step 4's processes are cache hits with no DuckDB child. It also verifies TreeTrie once. `bench`'s own flags (`--sample-size`, `--name`, `--report-json`, …) come before `run`: they are `BenchArgs`, flattened on `bench`, not `run` (`kermit/src/main.rs:188`; `kermit/tests/cli_bench_run_verify.rs` is the precedent). Write `$RUNS/materialise.sh`:

```bash
# Materialises every job-N from benchmarks/job-N.yml (a dispatch-arm run), verifying TreeTrie.
source /tb/Source/Academia/kermit-bench-runs/job-97/env.sh
for n in $(seq 1 33); do
  set +e
  timeout 3600 "$KERMIT" bench --sample-size 10 --measurement-time 1 --warm-up-time 1 \
    --name job-materialise --report-json "$RUNS/accept/job-$n-materialise.json" \
    run "job-$n" -i tree-trie -a leapfrog-triejoin --verify -m space \
    > "$RUNS/accept/job-$n-materialise.log" 2>&1
  rc=$?
  set -e
  echo "job-$n materialise rc=$rc"
done | tee "$RUNS/materialise.log"
```

Run: `bash /tb/Source/Academia/kermit-bench-runs/job-97/materialise.sh`

Expected: `rc=0` everywhere, or `137`/`124` for a template whose TreeTrie does not fit (Step 4 records it). Every `$XDG_CACHE_HOME/kermit/benchmarks/job-$n/meta.json` exists. A count mismatch (the log names the query and both counts) is a bug: stop and debug it (superpowers:systematic-debugging), never record it as a memory failure.

**Byte-identity on real data:** for each template, the relations' SHA-256s in `$RUNS/gen/job-$n-accept/meta.json` and `$XDG_CACHE_HOME/kermit/benchmarks/job-$n/meta.json` must be equal (same template, same raw cache, two generations):

```bash
RUNS=/tb/Source/Academia/kermit-bench-runs/job-97
for n in $(seq 1 33); do
  a=$(jq -r '[.relations[] | .name + " " + .sha256] | sort | .[]' "$RUNS/gen/job-$n-accept/meta.json")
  b=$(jq -r '[.relations[] | .name + " " + .sha256] | sort | .[]' "$RUNS/xdg/kermit/benchmarks/job-$n/meta.json")
  [ "$a" = "$b" ] && echo "job-$n identical" || echo "job-$n DIFFERS"
done
```

Expected: 33 lines `identical`. (If `jq` is not on the host: `nix shell nixpkgs#jq --command bash -c '…'`.)

- [ ] **Step 4: Verify every query on every cell, recording peak memory**

One process per (template, cell), each a cache hit, so each peak RSS belongs to one structure. Write `$RUNS/accept.sh`:

```bash
# Verifies every job-N query on each (structure, algorithm) cell; records rc, time and peak RSS.
source /tb/Source/Academia/kermit-bench-runs/job-97/env.sh
cells=("tree-trie leapfrog-triejoin" "column-trie leapfrog-triejoin" "hash-trie hash-triejoin")
for n in $(seq 1 33); do
  for cell in "${cells[@]}"; do
    read -r ds algo <<< "$cell"
    out="$RUNS/accept/job-$n-$ds"
    set +e
    "$TIME" -f "%e s %M KiB" -o "$out.time" \
      timeout 1800 "$KERMIT" bench --sample-size 10 --measurement-time 1 --warm-up-time 1 \
        --name job-accept --report-json "$out.json" \
        run "job-$n" -i "$ds" -a "$algo" --verify -m space > "$out.log" 2>&1
    rc=$?
    set -e
    echo "job-$n $ds rc=$rc $(tail -1 "$out.time")"
  done
done | tee "$RUNS/accept.log"
```

Run: `bash /tb/Source/Academia/kermit-bench-runs/job-97/accept.sh`

`rc` is the shell's exit status, not GNU time's `%x` (which prints 0 for a signal-killed child). Expected: `rc=0` for every cell that fits; `rc=137` (SIGKILL, the OOM killer) or `rc=124` (the 30-minute `timeout`) otherwise. A count mismatch (non-zero `rc` with a mismatch line in `$out.log`) is a bug: stop and debug it. No log may contain a download or DuckDB line (every run is a cache hit).

- [ ] **Step 5: Fill JOB.md**

In `docs/benchmarks/JOB.md`'s "Practical scale ceiling":
- Replace the placeholder row under the `<!-- J9-FILL: … -->` comment with one row per template, in J8's columns: **Template** (`job-N`), **Output size** (the template's relation files: `du -cb "$RUNS/xdg/kermit/benchmarks/job-N"/*.parquet | tail -1`, in MB with one decimal), **Peak memory** per structure (Step 4's `%M` in GiB with one decimal, or `OOM` for `rc=137`, `>30 min` for `rc=124`), **Notes** (e.g. "`cast_info` templates"). Then delete the comment.
- Replace the "about 5 GB of disk and a few minutes … (design estimates; the measured figures replace them at acceptance)" sentence with the measured figures: the raw cache's size (`disk.log`), job-1's generation time (download + build + template), and the range of the other templates' generation times (`gen.log`).
- Replace the sentence naming `bench run job-N -i all -a all --verify` with the command Step 4 actually ran (one cell per process, `--verify -m space`), and add one sentence naming the host (30 GB RAM) and the date.

Check:

```bash
cd /tb/Source/Academia/kermit/.loom/worktrees/aidanb/job_18dea468f74c13c3
grep -n "J9-FILL\|design estimates" docs/benchmarks/JOB.md
```

Expected: no output.

- [ ] **Step 6: Final gate**

```bash
cd /tb/Source/Academia/kermit/.loom/worktrees/aidanb/job_18dea468f74c13c3
nix develop --command cargo fmt --all --check
nix develop --command env CARGO_BUILD_JOBS=2 RUSTFLAGS=-Dwarnings cargo clippy --all-targets
nix develop --command env CARGO_BUILD_JOBS=2 RUSTDOCFLAGS=-Dwarnings cargo doc --workspace --no-deps
nix develop --command env CARGO_BUILD_JOBS=2 cargo test --test job_generate --test job_translate --test cli_job_gen -- --nocapture 2>&1 | grep -i "skipping"; echo "skip-grep rc: $?"
```

Expected: the first three pass; the last prints `skip-grep rc: 1` (no line matched: no test skipped, so DuckDB and the network were present). Then the full workspace test, through `setsid nohup` because it exceeds ten minutes:

```bash
cd /tb/Source/Academia/kermit/.loom/worktrees/aidanb/job_18dea468f74c13c3
RUNS=/tb/Source/Academia/kermit-bench-runs/job-97
setsid nohup nix develop --command env CARGO_BUILD_JOBS=2 cargo test > "$RUNS/gate-test.log" 2>&1 &
echo $! > "$RUNS/gate-test.pid"; disown
```

Watch it with `kill -0 "$(cat /tb/Source/Academia/kermit-bench-runs/job-97/gate-test.pid)"` until it exits. Expected: every `test result:` line in `gate-test.log` says `ok`, and `grep -c "FAILED\|panicked" gate-test.log` prints `0`.

- [ ] **Step 7: Commit**

```bash
cd /tb/Source/Academia/kermit/.loom/worktrees/aidanb/job_18dea468f74c13c3
git commit -m "docs: JOB's measured memory, disk and time per template (#97)" \
  --trailer "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>" \
  --trailer "Claude-Session: https://claude.ai/code/session_01WeRnvdstSxUY8CJGSx1NFB" \
  -- docs/benchmarks/JOB.md
```

- [ ] **Step 8: Land, through the user**

Report the acceptance results (Step 3's identity check, Step 4's table, any OOM or timeout cells) to the user and the co-ordinator. On the user's go-ahead: `git fetch origin && git merge origin/master`, re-run Step 6, then `git push origin HEAD:master`. Tick #97's acceptance boxes and post the memory table as a comment. `$RUNS` stays on disk until the user says otherwise (its `xdg/` holds the multi-GB raw cache).

---

## Merge notes

The plan was drafted in four parts (J1–J2, J3–J4, J5–J6, J7–J8) by separate drafters against the names contract, plus J9 by the design session. These are the design session's rulings on what the drafters escalated, the facts it verified after drafting, and the places where this plan departs from the contract.

### Verified by the design session (2026-10-15, after drafting)

- **DuckDB 1.4.4 CLI** (the flake's, `nix shell --inputs-from . nixpkgs#duckdb`, on scratch tables): `COPY (…) TO '/dev/stdout' (FORMAT csv, HEADER false)` under `-readonly -csv -noheader -c` prints only the rows (no count line); a NULL prints as an empty line; an empty result prints nothing; `SET memory_limit`, `SET temp_directory`, `SET preserve_insertion_order` all work under `-readonly`; `information_schema.tables` is readable; a missing table prints `Catalog Error: Table with name … does not exist!` on stderr with exit 1; `COPY t FROM 'x.csv' (FORMAT csv, HEADER false, ESCAPE '\\')` reads backslash-escaped quotes.
- **The J4 fixture**, extracted from this plan's text and loaded into DuckDB: every `rows.txt` count matches; the four counts are 1a = 5, 1b = 7, 2a = 2, 2b = 0, as the fixture facts say; and all 15 relations' rows (filter + NULL check + `ORDER BY id`) match the fixture facts' "Relation contents" table row for row, including 2b's empty `name_n_b`. A prototype of J2's rules, written independently in Python, gives the same 15 relations, names, columns, filters and sharing.
- **The real 113 queries** (pinned clone, prototype rules): 466 relation files (wrong: 465, see "J7 results"), 0 column-order conflicts, no unfiltered name clash in any template, every table and alias name matches `[a-z][a-z0-9_]*`.

### Rulings on the drafters' escalations

**J1–J2 (translator).** All accepted as drafted:
1. `NotOneSelect` also rejects `DISTINCT`, `GROUP BY`, `HAVING`, `ORDER BY`, `LIMIT`, `WITH` and an empty `FROM` (each would change what the `COUNT(*)` oracle counts); `UnsupportedFrom` also rejects table functions and aliases with column lists.
2. Two columns of one alias in one variable keep first-mention order (JOB has none).
3. The sharing key is (table, columns, filter) without `join_columns`: harmless, since `id` is never NULL.
4. `NameClash` names the relation or variable, not the query (the contract's shape).
5. Only two-part `alias.column` references count; an unqualified column in a condition makes it a filter that DuckDB later rejects. JOB always qualifies.
6. Filter text runs in DuckDB with bare column names over `FROM "<table>"`. JOB's filter columns (`info`, `kind`, `keyword`, `name`, `title`, `note`, `role`, `link`, `production_year`, …) are not DuckDB reserved words; the fixture's bare filters ran in DuckDB (above); J9's full run is the final check.
7. `TranslateError` is complete from J1; J2 raises `UnjoinedAlias` and `NameClash`.

**J3–J4 (DuckDB, raw cache, fixture).** All accepted:
1. `Duckdb::query_strings` is added (not in the contract), for `build`'s read of `information_schema.tables`.
2. `locate()` delegates to a private `locate_from(Option<OsString>)`, so the env-override test does not mutate the process environment.
3. **J4 is split.** Part A (Steps 1–14) is green and committed with `IMDB_SHA256 = "UNMEASURED"` and every `IMDB_TABLE_ROWS` count `0`. Part B (Steps 15–27) measures and pins them, after `FULL_EXIT` and with the user's go-ahead, and adds `imdb_pins_are_measured`. **J7 needs Part B.**
4. `build` copies every file in `queries_dir` and requires each to be named `<digits><lowercase letters>.sql`; `template_queries` returns an empty `Vec` for a template with no files, and J5 raises the error.
5. `.gitignore` gains `!kermit-generate/tests/fixtures/job/csv/*.csv`, because the repo ignores `**/*.csv`. A tracked file outside the contract's list.
6. Three tests the contract listed as DuckDB-gated need no DuckDB and run ungated; the extra tests are kept.

**J5–J6 (generator, wiring).** All accepted:
1. `generate` fails if the raw cache's provenance lacks a digest for `schema.sql` or one of the template's query files.
2. `JobExtras::duckdb_version` is the generation's DuckDB (the contract's).
3. A failed `generate` leaves the relations it had already finished; only `meta.json`'s absence marks the output incomplete (`kermit_generate::run` deletes the old `meta.json` first and writes it last). `failed_filter_leaves_no_relation_and_no_meta` makes the failing relation the template's first, so "no relation file" holds literally; a later failure is asserted as no `meta.json`, no `benchmark.yml`, no `.part`, no file for the failing relation. Stronger cleanup would belong in #95's `run`, not here.
4. J5 adds a reversed-load pass (`common::job::csv_dir(…, true)`), because the fixture's CSVs are in id order and so cannot reveal a missing `ORDER BY`; mutation check 1 confirms the pass discriminates.
5. **J5 moves to phase 1**: it needs only #95's push 1 (see the phase table).
6. New files outside the contract's list: `kermit/tests/common/job.rs` (and its line in `kermit/tests/common/mod.rs`), `kermit/tests/cli_job_gen.rs`; the extra tests are kept.
7. **Changed by the co-ordinator's routing:** the JOB `dispatch` arm does not re-wrap its error. The first `kermit-generate` arm to land (#95 push 2 or #96 phase 2) changes `materialize.rs`'s `format!("generator pipeline failed: {e}")` to `{e:#}`, so DuckDB's stderr survives for every kind; J6 Step 1 checks it.
8. The template range lives in `kermit-bench` (`JOB_TEMPLATES`) and `kermit_generate::job::TEMPLATES`, tied by a test in the binary.
9. `run_gen_job` checks the range itself (the contract's), and locates DuckDB before `GenTarget::prepare` creates the directory.
10. J6's `ensure` stub is untested; `cli_job_gen.rs` runs `bench gen` and `bench run` over a fixture cache whose provenance equals the pins (`common::job::pinned_provenance`), which J7's provenance check accepts.

**J7–J8 (downloads, docs).**
1. **Changed by the design session:** `ensure_from` compares a complete cache's provenance (`imdb_sha256`, `query_commit`, the full `query_sha256` map) with the pins and, on a mismatch, errors naming the field, both values and the directory to delete; it never deletes or rebuilds (spec drift's rule). The contract's J7 line "open, else stage" reads "open and check, else stage".
2. The staging directory is removed on every exit, so a failed build discards the download (keeping 5 GB of half-state was rejected).
3. `RawProvenance::query_sha256` holds all 114 files, `schema.sql` included.
4. No `outdated_reason` floor for `kind: job` (a new kind); JOB.md lists it as future work.
5. `docs/benchmarks/README.md`'s intro still says "RDF benchmarks"; left to whichever change widens it first.
6. JOB.md keeps the schema's `## Vendored generator` heading, and its memory table has a column per structure.
7. The final gate greps `J9-FILL` in `docs/benchmarks/JOB.md` only (this plan mentions the marker).
8. J8's CLAUDE.md edits to lines #95 and #96 add (the `gen […]` list, the `kermit-generate` row, the declarative-YAML kind list) are conditional text: apply them to whatever those pushes left.
9. **Changed after #96's review:** a 4xx/5xx is `BenchError::Download` too, so `job_translate.rs` skips only when the first fetch's `Download` has `source.status().is_none()` (a connection, DNS or timeout failure), as #96's own network test does; a 404 for a vanished pin fails. The mirror fallback tries the next mirror on any error.

### Departures from the contract

- Phase table: J5 is phase 1 (it needs only #95's push 1); J7 also needs J4 Part B.
- Added items: `Duckdb::query_strings`, `Duckdb::at` stays; private `locate_from`; `raw`'s private `Staging` (J4, reused by J7) and `Sources`, `ensure_from`, `check_provenance` (J7); `SET enable_progress_bar=false` in the DuckDB session prelude; the `.gitignore` exception; `kermit/tests/common/job.rs`, `kermit/tests/cli_job_gen.rs`; `docs/benchmarks/README.md` (Index entry and the JOB matrix column, per that file's own checklist).
- `kermit/tests/job_translate.rs`'s skip rule (J7–J8, ruling 9).

### Upstream facts that arrived during drafting

- #95's names contract moved ffc516c → 0de175a → f7e21c1 with no renames. `output_dir(_, Some(_))` never fails; `GeneratedMeta<E>` derives `PartialEq` (not `Eq`) without a serde bound; an empty batch is a no-op in `write_batch` at any arity, and open + finish with no batches writes an empty relation with its header (2b relies on it).
- #96's `download_verified`: signature unchanged; `Download` = request failed, 4xx/5xx, or a 30 s stall; `UnusableDownload` = a non-200 success status or an empty body; `Integrity` = digest mismatch (`subject` = `file '<name>'`); `Io` = local file errors. Progress on stderr past 64 MiB.

### Drafters' premise lists

Each task's Step 1 re-checks its premises against the tree after the upstream merges; the drafters' lists of what they could not verify (all J1–J6 names, #95's API, `GenTarget`'s shape, the J6 stub's text, `rows.txt`'s format) are settled there. Items not covered by a Step 1 grep: whether every one of the structure × layout cells copes with 2b's empty relation and its `Index_*` copies under `any` (J5 Step 6 says to report, not fix, a panic), whether clippy's `doc_markdown` flags bare `DuckDB` / `IMDb` (Step 7's clippy settles it), and whether the HTTP client honours `HTTPS_PROXY` (J7's skip control B).

### Pre-commit review (2026-10-15)

Four `code-quality-reviewer`s read the merged plan, one per part, against the real APIs (sqlparser 0.63.0's source, the kermit tree, #95's and #96's contracts). Their findings were applied before the plan was committed:

- **Part 1 (J1–J2):** no Critical or Important findings; every sqlparser name and expected `Display` string checked out. Applied: tests for the `WITH` / `HAVING` / table-function rejections; an unreachable duplicate assertion dropped; a mutation location reworded for `cargo fmt`'s line splits; the template-6 derivation added to its test doc; `RelationPlan.name`'s doc no longer overclaims.
- **Part 2 (J3–J4):** `raw.rs` is created with its `//!` block (an empty file fails `missing_docs`); `git check-ignore -q` for the `!`-pattern check; J4's `Staging` is the one staging guard (J7 reuses it) and its `create` makes the parent directory, so a fresh `$XDG_CACHE_HOME/kermit/imdb` works; Part B's paths live on disk under `$RUNS/imdb-measure` with guarded `rm -rf`; `enable_progress_bar=false` in the session prelude; stderr read lossily; tighter test needles.
- **Part 3 (J5–J6):** J6's `definition.rs` and `materialize.rs` Edits re-anchored after the last generator arm, so they still apply once #95's and #96's arms are merged; the provenance check runs before the filter stage (with a test); a hash comment made true by comparing `sha256_hex`; the CLI end-to-end test uses `-m iteration` and puts `bench`'s flags before `run`; two stale `definition.rs` docs updated.
- **Part 4–5 (J7–J9):** J7's duplicate `Staging` removed (it reuses J4's), mutation expectations corrected, JOB.md/CLAUDE.md doc fixes, J7 reuses J4's test helpers and J5's `SCHEMA_FILE`. J9 rewritten by the design session: scripts under `bash`, `bench`'s flags before `run`, GNU time at `/run/current-system/sw/bin/time`, the shell's `$?` (not `%x`) for an OOM kill, an untimed materialise pass so measured runs are cache hits, every block self-contained with a guarded private cache on disk, the memory table aligned with J8's columns, and a real-data byte-identity check between `bench gen` and `bench run` outputs.
- **Plan-wide:** commit trailers as `--trailer` flags (a multi-line `-m` pasted from an indented fence merges them); nothing large in `$SCRATCH` (tmpfs).

### Phase 1 carry-forward (2026-10-15, after C01–C05)

Phase 1 is done at 836a562; each story passed spec and quality review, and a cross-story review over 593ed09..836a562 approved it. Its Minors were deferred to the phases that edit the same files, to keep the host free for the #101 chain. Each phase's executor applies its items after Step 1's premise re-check, in that task's commit or as one fix commit before it. Line numbers are as at 836a562.

Departures from this plan's code text, already landed (each task's Step 1 should expect them):

- **The IMDb CSV dialect is stated in full** (643f5f8; spec d8b3d54): `COPY … (FORMAT csv, HEADER false, DELIMITER ',', QUOTE '"', ESCAPE '\', ALLOW_QUOTED_NULLS false, AUTO_DETECT false)`. PostgreSQL's CSV format, which wrote the dump, reads a quoted `""` as `''`; DuckDB's default reads it as NULL and sniffs any unset option.
- **Staging and spill names carry a sequence:** `<dir>.building-<pid>-<seq>` (`raw::staging_path`, 643f5f8) and per-call spill directories `duckdb.tmp/<pid>-<seq>` (b47a4cd). `duckdb.tmp` is now their parent, not the spill directory itself.
- **Every `Target`'s `out_dir` ends in the benchmark's name** (#95 push 1b): tests generate into `<tmp>/job-<N>` (`job_generate.rs`'s `job_dir`).

**J6** (it edits `kermit-generate/src/job/mod.rs` and adds beside `kermit/tests/job_generate.rs`):

1. `mod.rs:115`: the `BIGINT` cast's doc gives a stale reason. The cast gives `stream_rows` integer text; `ParquetWriter` writes `Int64` regardless.
2. `mod.rs:123,128,144`: add `sql_identifier` beside `duckdb::sql_literal`, doubling `"`, use it for every quoted identifier in `filter_select`, drop the "IMDb names never contain `"`" justification, and add one unit case with an embedded `"`.
3. `mod.rs:82`: `JobExtras::query_sha256` is copied from the raw cache's `provenance.json`, not re-hashed; say so. `mod.rs:199-200`: "no new file" (an older file of the same name survives a failed regeneration, harmlessly).
4. `mod.rs:175`: `plan_template`'s error gets `.with_context(|| format!("planning JOB template {}", self.template))`.
5. `job_generate.rs:393`: on the reversed fixture, assert DuckDB scans in load order (`SELECT id FROM movie_info LIMIT 1` gives 16), with a comment, so the missing-`ORDER BY` check fails loudly instead of going vacuous if the scan order changes.
6. `mod.rs:67-69`: `JobSource.filter`'s doc states the fixed rule that every relation also drops rows with a NULL in a join column (spec, `meta.json` extras). Design decision: document it, don't record it per relation.
7. Add `raw_duckdb_version` to `JobExtras`, copied from `RawProvenance.duckdb_version` (`raw.rs:53`), so `meta.json` names the DuckDB that loaded the CSVs as well as the one that filtered them.

**J7** (it edits `kermit-generate/src/job/raw.rs`):

1. `ensure`'s download staging (the plan's `<dir>.fetch-<pid>`, line 8032; code doc `raw.rs:164`) uses `staging_path`'s `<pid>-<seq>` scheme.
2. `ensure` runs `check_provenance` on `build`'s result too: when `build` loses the rename and adopts the winner, the adopted cache is never compared with `inputs.provenance`.
3. `raw.rs:144, 152-158`: `split_query_stem` rejects a leading `0` in the template number. `01a.sql` is accepted today, `template_queries` names it `1a`, and J5 then looks up `1a.sql`'s digest; `1a.sql` plus `01a.sql` collide only after every relation is written.
4. `raw.rs:938, 955`: give the scratch dialect test a column of all-`'x'` values so that dropping `QUOTE` and `AUTO_DETECT` fails (prove it with that mutant); if DuckDB's sniffer keeps the apostrophes even then, cut the "whatever the sample looks like" claim instead.
5. `raw.rs:545`: delete the doc clause "an empty folder is refused too" (the separate test at `:583` covers it).
6. `raw.rs:316, 327`: the `COPY` and the row-count query quote table names by hand; use `duckdb::sql_identifier` (41e956f).

**J8** (docs): JOB.md's DuckDB-sessions and output-layout text (lines 8630-8632, 8654) calls `duckdb.tmp` "DuckDB's spill directory"; it is the parent of the per-call `<pid>-<seq>` spill directories. Line 8676's `imdb.fetch-<pid>` / `imdb.building-<pid>` become `-<pid>-<seq>`.

**Outside JOB:** `kermit/tests/job_generate.rs` holds a third copy of the 14 structure × layout aliases (after `join_tests.rs` and `column_orders_any.rs`) and a `mismatches` / `cells` count-oracle loop that generalises the LUBM and WatDiv loops. Moving both into `kermit/tests/common/` is a cross-cutting test change; it is flagged to the benchmark co-ordinator for SNAP and H2O and is not JOB's.

**Optional** (skipped by earlier rulings; take one only when its file is open anyway): `translate.rs`'s `has_subquery` misses `ARRAY(SELECT …)` (`Expr::Function` with `FunctionArguments::Subquery`), so its doc at `:208-211` overclaims; a test comment near `:1063-1070` calls a top-level `NOT EXISTS` nested; `translate.rs:530`'s doc line breaks early; `duckdb.rs:86-87` says the next cache clean removes stray spill directories (`bench clean --all` leaves `~/.cache/kermit/imdb`), and `:243-244`'s "every path below reaps it" does not hold for a failed `take()` or `wait()`. From C06.0's review (41e956f): `mod.rs:69-72`'s parenthetical reads as if the columns are what the join never matches (say: "drops rows with a NULL in a join column, since an SQL equi-join never matches them; its join columns are the leading columns of its `relations` entry"); `mod.rs:272`'s struct literal has `raw_duckdb_version` before `duckdb_version`; `mod.rs:122-131`'s doc lines are ragged; and `job_generate.rs:555-585`'s `regeneration_is_byte_identical` relies on the reversed scan order without the `:398-415` guard (share the guard as a `Fixture` helper).

### Phase 2 carry-forward (2026-10-15, after C06.0–C06C)

Phase 2 (J6 and phase 1's J6 items) is done at 6e94dcb: four stories, each spec- and quality-reviewed, one batched fix commit re-reviewed, the five Step 21 mutants killed, and the gate re-run independently. Landed beyond the plan's text: `run_gen_job` validates the `BenchmarkDefinition` right after the range check and before `Duckdb::locate`, so a bad `--tag` writes nothing (new CLI test); the end-to-end test sets `CRITERION_HOME` to a tempdir and points the proxies at a dead port; a `job_generate.rs` test checks every YAML's "(Na-Nx)" description against `QUERY_FILES` (a durable Decision 6 guard). J8's file table (cli_job_gen's row) and edit list (`ARCHITECTURE.md`) are updated in place.

**J7:** 7. `cli_job_gen.rs`'s `gen_job` gets the same dead-proxy environment as `bench_run` once `ensure` really downloads, so no CLI test can reach the network.

**J8:** JOB.md's Determinism section states the cache rule for pins: `spec_hash` covers only the template, so a change to `pins.rs` (the IMDb digest, the query commit or a file digest) alters the output under a fixed spec; per CLAUDE.md's generator-cache rule it bumps `META_SCHEMA_VERSION` and adds a `job` floor to `OUTDATED_BEFORE`, never relying on users to `--force`. (J4 Part B replaces the placeholder digest and row counts before any real `job-N` cache exists, so that first change needs no floor.)

**Optional:** `job_generate.rs:7`'s module doc says every test needs DuckDB (the description test does not); `main.rs:1437` spells `kermit_bench::GeneratorSpec` where SNAP imports it locally; `job_generate.rs:891` hard-codes `1..=33` where it could iterate `TEMPLATES`; the range could have one source (`kermit_bench::JOB_TEMPLATES` made public, `TEMPLATES` re-exporting it) instead of two tied by a test.

**Outside JOB (older than it):** `materialize`'s `--force` wipes the cached benchmark before the generator checks its prerequisites (for JOB: DuckDB and the raw cache), so `bench run --force job-N` on a host without DuckDB loses the cache and then fails; this holds for every generator kind and is for the user to file. About 11 CLI test files spawn `bench run|join|ds` without `CRITERION_HOME` (the co-ordinator has routed that to the user as one cross-cutting item).

### J4 Part B results (2026-10-16, df2402f)

Run by the design session on the user's go-ahead, after #95 push 2's merge (4ba0d9a); the #101 chain had been dropped, so `FULL_EXIT` no longer applied, and the co-ordinator sequenced it after SNAP's acceptance. `imdb.tgz` is 1,263,193,115 bytes from `bonsai.cedardb.com`, SHA-256 `25f9d893c54f903366e0c263f88db0d429dbc2b159d4987ebc1e203242a7e988`; the CWI mirror streams the same bytes. `schema.sql` matched its pin. The 21 CSVs (3.88 GB) loaded under DuckDB `v1.4.4 (Andium) 6ddac802ff` in 29 s into a 2.5 GB database, every count positive and equal to its CSV's line count, with no `NOT NULL` or key violation; `cast_info` is 36,244,344 rows. The measurement directory keeps only the small records (`imdb.sha256`, `table-rows.txt`, `duckdb.version`, `load.log`, the load script).

Departures from Steps 15–27: Step 20's script used `raw::build`'s full COPY dialect (`DELIMITER ',', QUOTE '"', ESCAPE '\', ALLOW_QUOTED_NULLS false, AUTO_DETECT false`, the C04 fix), not the step's older `ESCAPE '\'` alone, since the step's rule is to load as `raw::build` does; Step 19's count was 22, not 21 (below), its diff restricted to `*.csv`; Step 27 deleted the tarball too (disk was at 12 GB free; the co-ordinator asked, and the digest is pinned, so a re-download is five minutes); `IMDB_SHA256`'s doc does not name a DuckDB version, which says nothing about the bytes.

**J7:** 8. The archive holds a 22nd entry, `schematext.sql`, a different schema from the pinned `schema.sql` (no `NOT NULL` constraints, other `varchar` lengths). `extract_tgz` unpacks it beside the CSVs and `build` reads only `<table>.csv`, so it is harmless, but `ensure` must load the pinned `schema.sql` it fetched, never anything from the archive; a fixture tarball with an extra file would pin that. 9. At the first `origin/master` merge after SNAP's `bench gen` split lands (`kermit/src/bench/gen.rs`, df767f2 on aidanb/snap; the co-ordinator relays the SHA), move `GenSubcommand::Job`, its `gen::run` arm, `run_gen_job` and its tests from `main.rs` into `gen.rs`, as a merge.

**J8:** the real numbers replace the spec's estimates: the load takes about 30 s, not minutes; a build peaks near 7.7 GB on disk (tarball 1.26, CSVs 3.88, database 2.5) and leaves the 2.5 GB database. The real dump has no empty string in any of its 49 `VARCHAR` columns (and 57.7 M NULLs, all in nullable ones), so the pinned dialect loads today's dump exactly as DuckDB's defaults would; JOB.md says the pin is defensive (no sniffing, PostgreSQL's `""` rule), not a correction to these counts. `run_gen_job` is cited at `kermit/src/bench/gen.rs` once item 9 lands.

### J7 results (2026-10-16, af3ba49..169c651)

Run supervised on the user's go-ahead after merging `origin/master` df767f2 (af3ba49, which did J7 item 9: `GenSubcommand::Job`, its arm and `run_gen_job` now live in `kermit/src/bench/gen.rs`). Every chunk passed spec and quality review, the whole implementation was reviewed again, and an independent gate at 169c651 passed: fmt, clippy `-Dwarnings`, doc `-Dwarnings`, `cargo test --workspace` (6,334 passed, 0 failed). The binary's private rustdoc keeps master's two known errors. Nothing was downloaded beyond `job_translate.rs`'s 114 small files into a tempdir.

Commits: 1fea8e9 and b922a9a (Phase 1 items 3–6); 8015d24, 980dadd, a0d18d9 and 874ead8 (`ensure`, items 1, 2, 7 and 8); 0c13fae and 169c651 (`job_translate.rs`); ddd924c (the final review's fixes).

**The golden count is 465, not 466** (the user approved the change to `RELATION_FILES`). Template 12's `company_name` filter is spaced one way in 12a and 12c and another in 12b. The sharing key is the filter as sqlparser prints it, which normalises the spacing, so the three variants share one relation, `company_name_cn_abc`, and template 12 has 17 files. The design scan compared filters with whitespace only collapsed, so it counted 18. An independent recount over the 113 pinned files agrees with the translator in every other template. `template_12_shares_one_company_name_relation` pins the case.

Departures from this task's text:

- **Staging names.** `staging_path(dir, kind) -> anyhow::Result<PathBuf>` names both staging directories `<dir>.<kind>-<pid>-<seq>` (`building`, `fetch`) on one counter. A `dir` with no file name is an error, not a fallback name. `staging_dir` is gone. The tests scan the parent for leftover `imdb.fetch-*` / `imdb.building-*` directories (`assert_no_staging_beside`), because re-deriving a sequenced name would be vacuous.
- **More refusals before any fetch.** `ensure_from` compares provenance once, after both paths, so a cache adopted after a lost rename is checked too (`a_cache_adopted_after_a_lost_rename_is_checked_against_the_pins`). An existing `dir` without `provenance.json` is refused before the download, naming `dir` to delete (`an_unfinished_dir_fails_before_any_fetch`); so is an unreadable `provenance.json` (`an_unreadable_provenance_says_to_delete_the_cache`).
- **The tarball is deleted right after `extract_tgz`**, so a first build peaks near 6.4 GB on disk (CSVs 3.88 GB plus the 2.5 GB database), not 7.7. No test covers this; it would need a production hook.
- **Item 8.** The fake dump carries a `schematext.sql` that is not SQL, and the build still succeeds from the fetched `schema.sql`.
- **Item 4.** J4's dialect test, renamed `the_csv_dialect_is_postgresqls`, gains a CSV with no `"` (`quip`). The first attempt reused the fixture CSV, whose own `"` makes DuckDB's sniffer pick `"` anyway. Without `QUOTE` and `AUTO_DETECT` the new table loads 0 matching rows instead of 2.
- **The real-113 test** admits each rule through `kermit::db::validate_query` under `ColumnOrderPolicy::Stored`, against headers from its template's relations. That is `bench run`'s path; the old uppercase loop could never fire, since kermit-parser reads any lowercase term as a constant. The fetch result is cached as a `Result`, so a failure costs one fetch round. It has 3 tests (the template-12 pin is the third). It ran in 37.8 s cold and 5–6 s warm.
- **CLI.** `cli_job_gen.rs` has a `no_network` helper for every command that may reach `ensure`, and `gen_job_refuses_a_raw_cache_built_from_other_pins`. That test fails when `ensure` is reverted to J6's stub.

Step 10's mutants, as run:

- Mutant 1 fails the three tests listed.
- Mutant 2 also fails `ensure_falls_back_to_the_second_mirror`.
- Mutant 3, over `job::raw`, fails 10:
  - `tests::staging_create_removes_a_leftover`
  - `tests::an_unfinished_dir_at_the_rename_is_an_error`
  - `tests::row_count_mismatch_leaves_no_cache`
  - `tests::losing_the_rename_adopts_the_winner`
  - `ensure_tests::a_stale_query_pin_fails_before_the_dump`
  - `ensure_tests::a_first_run_creates_the_missing_cache_parent`
  - `ensure_tests::a_cache_built_from_other_pins_is_refused_and_kept`
  - `ensure_tests::a_failed_build_leaves_no_cache_and_no_staging`
  - `ensure_tests::a_cache_adopted_after_a_lost_rename_is_checked_against_the_pins`
  - `ensure_tests::ensure_fetches_the_queries_before_the_dump_then_builds`
- Mutant 4 also fails the adopted-cache test.

Control A: all 3 tests fail on the integrity error, none skip. Control B: one `skipping:` line and 3 passed; reqwest 0.13.5 honours `HTTPS_PROXY`.

**J8:**

- **Real numbers.** The peak disk figure is 6.4 GB (it supersedes J4 Part B's 7.7). `job_translate.rs` takes about 40 s cold (114 serial fetches, each a fresh TLS client).
- **JOB.md's raw-cache section:**
  - The 114 small files come before the dump.
  - A cache built from other pins, an existing `imdb` directory without `provenance.json`, and an unreadable `provenance.json` are each refused before any download, naming the directory to delete.
  - The tarball is deleted once extracted.
  - A refused or failed raw cache leaves the empty `job-N-<tag>` directory `bench gen job` made, which discovery ignores.
- **Tests table.** The `job_translate.rs` row is updated in place. Add `cli_job_gen.rs`'s refusal test to its row.

**Follow-up for the user (not JOB-blocking):** `raw.rs` has grown to about 1,900 lines. Its download half could move to `job/raw/fetch.rs`.
