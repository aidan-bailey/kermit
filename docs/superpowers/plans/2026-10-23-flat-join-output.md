# Flat Join Output Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Execution:** inline, in the design session, with superpowers:executing-plans (the user's choice,
2026-10-23).

**Goal:** Every API that collects a join's result returns it flat: `lftj_join` / `hash_join` return
the answer as the `(RelationHeader, Tuples)` pair a `Catalog` is built from, `compute_join` returns
`Tuples`, and `JoinAlgo::join_iter` is deleted, with no change to any timed path.

**Architecture:** `execute` stays the one terminal step of the lifecycle; collecting is `execute`
with a `Tuples` sink sized by `Prepared::head_len()`. `prepare` names the answer through a new
`Prepared::answer_header()`, derived on demand from the executed query's head, so `Prepared`'s
fields and `execute`'s code do not change. The binary never collects; only its CSV header row moves
to `answer_header`.

**Tech Stack:** Rust nightly from the flake (`nix develop`), `allocation-counter` for the
allocation cells, `objdump` + `c++filt` + `python3` for the codegen check.

**Spec:** `docs/specs/2026-10-23-flat-join-output-design.md` (609fb6a, with Amendment 1).

---

## Conventions for every task

- **Worktree:** `/tb/Source/Academia/kermit/.loom/worktrees/aidanb/flat-vec-out_18e11f06f4332998`.
  Every command runs from it.
- **Scratch:** `SCRATCH=/tmp/claude-1000/-tb-Source-Academia-kermit--loom-worktrees-aidanb-flat-vec-out-18e11f06f4332998/b700993e-1014-4b73-9dd8-b36ae7a542c0/scratchpad`.
  Commands that use it set it first.
- **Cargo:** always through the flake, in the foreground, two jobs:
  `nix develop --command env CARGO_BUILD_JOBS=2 cargo …`. The flake's nightly is CI's; the host's
  harness kills background cargo when free memory is low; the system `cargo` (1.99 stable) would
  build a second target tree. A command that would exceed 10 minutes runs detached (Task 0 shows
  how); one that hits the limit is rerun, since compiled crates are kept.
- **Format** before every commit: `nix develop --command cargo fmt --all`. Never run `cargo fmt`
  outside `nix develop`.
- **Stage by name** (`git add <paths>`), never `-A`, and check `git status --short` first: this
  environment marks new files intent-to-add.
- **Commit messages** end with:
  ```
  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_012YYYQo7fUggdHrTt6wWjtk
  ```
- **Mutants** are applied with an exact edit after the work is committed, checked to have applied,
  and reverted with the exact reverse edit; never with `git checkout`.

## File structure

| File | Change |
|---|---|
| `kermit/src/db/lifecycle.rs` | `Prepared::answer_header()`; two unit tests |
| `kermit/src/db.rs` | module doc; private `collect_join`; `lftj_join` / `hash_join` return `(RelationHeader, Tuples)`; tests adapted; four new tests |
| `kermit/src/lib.rs` | `compute_join` returns `Tuples` through `collect_join`; doc; tests adapted; one new test |
| `kermit/tests/common/utils.rs` | `JoinEntry::join` returns the pair through the public collectors; `join_under_planner` checks the header and returns `(Tuples, Vec<IndexSpec>)`; `test_join` compares `to_vecs()` |
| `kermit/tests/subject_position_constant.rs` | `join_with_specs` converts with `to_vecs()` |
| `kermit/tests/all_orders.rs` | `rows_in_order` collects with a `Tuples` sink |
| `kermit-algos/src/join_algo.rs` | `join_iter` deleted; a `# Collecting` doc section |
| `kermit-algos/src/hash/hash_triejoin.rs` | test helper `joined`; four tests use it |
| `kermit-algos/src/sorted/leapfrog_triejoin.rs` | one comment names `join_for_each` |
| `kermit/src/main.rs` | CSV header from `answer_header`; `head_column_names` and its test deleted; imports |
| `kermit/tests/fixtures/reordered_head_query.dl` | new fixture: `path(Z, X) :- edge(X, Y), edge(Y, Z).` |
| `kermit/tests/cli_join_tests.rs` | one test pins the CSV header row |
| `kermit/tests/result_allocation.rs` | three cells: collecting allocates per doubling, not per row |
| `CLAUDE.md`, `ARCHITECTURE.md`, `docs/specs/benchmarking-architecture.md`, `docs/algorithms/hash-triejoin.md`, `kermit-algos/README.md`, `kermit/README.md` | documentation |

---

### Task 0: Baseline builds and the fingerprint script

**Files:** none in the repository (scratch only).

- [ ] **Step 1: Confirm the starting point**

```bash
git log --oneline -1 && git status --short
```

Expected: `… docs(plans): flat join output plan; spec Amendment 1` on top of `609fb6a`, and a clean
tree (no output from `status`).

- [ ] **Step 2: Build the baseline release binary and warm the test build, detached**

A first build in this worktree (no `target/` yet) exceeds the 10-minute foreground limit.

```bash
SCRATCH=/tmp/claude-1000/-tb-Source-Academia-kermit--loom-worktrees-aidanb-flat-vec-out-18e11f06f4332998/b700993e-1014-4b73-9dd8-b36ae7a542c0/scratchpad
setsid nohup nix develop --command bash -c "CARGO_BUILD_JOBS=2 cargo build --release -p kermit && cp target/release/kermit $SCRATCH/kermit-before && CARGO_BUILD_JOBS=2 cargo test --workspace --no-run; echo EXIT=\$?" > "$SCRATCH/warm.log" 2>&1 < /dev/null & disown
```

- [ ] **Step 3: Wait for it**

Run as a background command and wait for its notification:

```bash
SCRATCH=/tmp/claude-1000/-tb-Source-Academia-kermit--loom-worktrees-aidanb-flat-vec-out-18e11f06f4332998/b700993e-1014-4b73-9dd8-b36ae7a542c0/scratchpad
until grep -q '^EXIT=' "$SCRATCH/warm.log"; do sleep 15; done; tail -3 "$SCRATCH/warm.log"
```

Expected: the last line is `EXIT=0`.

- [ ] **Step 4: Write the fingerprint script**

Create `$SCRATCH/fingerprint.sh` with exactly:

```bash
#!/usr/bin/env bash
# fingerprint.sh BINARY: one line per function in BINARY's .text,
# "<symbol>\t<md5 of its instructions>". Each instruction loses its address,
# objdump's trailing comment, absolute branch targets and rip-relative
# displacements, and padding instructions are dropped, so a function compiled
# from the same code fingerprints the same wherever the linker placed it.
set -euo pipefail
objdump -d --no-show-raw-insn "$1" | awk '
  function flush() { if (name != "") print name "\t" body }
  /^[0-9a-f]+ <.*>:$/ { flush(); name = $2; body = ""; next }
  /^ *[0-9a-f]+:\t/ {
    sub(/^ *[0-9a-f]+:\t/, "")
    sub(/[ \t]*#.*$/, "")
    if ($0 ~ /^(int3|nop|xchg +%ax,%ax|cs nop|data16)/) next
    gsub(/[0-9a-f]+ </, "<")
    gsub(/-?0x[0-9a-f]+\(%rip\)/, "RIP")
    body = body $0 ";"
  }
  END { flush() }
' | python3 -I -c '
import hashlib, sys
for line in sys.stdin:
    name, _, body = line.rstrip("\n").partition("\t")
    print(name + "\t" + hashlib.md5(body.encode()).hexdigest())
' | LC_ALL=C sort
```

Then `chmod +x "$SCRATCH/fingerprint.sh"`.

- [ ] **Step 5: Fingerprint the baseline**

```bash
SCRATCH=/tmp/claude-1000/-tb-Source-Academia-kermit--loom-worktrees-aidanb-flat-vec-out-18e11f06f4332998/b700993e-1014-4b73-9dd8-b36ae7a542c0/scratchpad
sha256sum "$SCRATCH/kermit-before" | tee "$SCRATCH/kermit-before.sha256"
systemd-run --user --scope -q -p MemoryMax=3G -p MemorySwapMax=0 "$SCRATCH/fingerprint.sh" "$SCRATCH/kermit-before" > "$SCRATCH/before.fp"
wc -l < "$SCRATCH/before.fp"
grep -c 'count_rows\|7execute17h' "$SCRATCH/before.fp"
```

Expected: tens of thousands of lines; the `grep -c` prints a number (0 is possible when
`count_rows` and `execute` are fully inlined into their callers, which is why Task 9 compares every
function).

---

### Task 1: `Prepared::answer_header`

**Files:**
- Modify: `kermit/src/db/lifecycle.rs` (`impl Prepared`, after `head_len` at line 66; tests after
  `all_constant_atoms_become_checks`, which ends at line 592)

- [ ] **Step 1: Write the failing tests**

Insert after the closing `}` of `all_constant_atoms_become_checks` (line 592):

```rust

    /// The answer is named after the head predicate, with the head's
    /// variables as its attributes in head order, also when the head reorders
    /// the body's variables; its arity is the head's length.
    #[test]
    fn the_answer_header_names_the_head() {
        let c = catalog(vec![("p", 2, vec![vec![1, 10]])]);
        let prepared = prepared_for(&c, "Answer(Y, X) :- p(X, Y).");
        let header = prepared.answer_header();
        assert_eq!(header.name(), "Answer");
        assert_eq!(header.attrs().to_vec(), vec!["Y", "X"]);
        assert_eq!(header.arity(), prepared.head_len());
    }

    /// An empty head names an answer with no attributes. The parser cannot
    /// write an empty head, so the query is built by clearing one.
    #[test]
    fn an_empty_head_names_an_answer_of_arity_zero() {
        let c = catalog(vec![("s", 2, vec![vec![7, 8]])]);
        let mut query = parse("Q(X) :- s(c7, c8).");
        query.head.terms.clear();
        let header = prepare(&c, &query, &lexicographic())
            .expect("a query of checks")
            .answer_header();
        assert_eq!((header.name(), header.arity()), ("Q", 0));
        assert!(header.attrs().is_empty());
    }
```

- [ ] **Step 2: Run them to see them fail**

```bash
nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit --lib -- db::lifecycle::tests::the_answer_header_names_the_head db::lifecycle::tests::an_empty_head_names_an_answer_of_arity_zero
```

Expected: compile error `no method named `answer_header` found for struct `Prepared``.

- [ ] **Step 3: Implement `answer_header`**

Insert after `pub fn head_len(&self) -> usize { self.head_len }` (line 66):

```rust

    /// The answer's header: named after the head predicate, with one attribute
    /// per head term, in head order. [`prepare`] validated the head, so the
    /// terms are distinct variables and the attributes are their names; an
    /// empty head gives no attributes. Paired with the answer's rows, it is
    /// the form a [`Catalog`] is built from.
    ///
    /// Built from the executed query's head on each call rather than stored,
    /// so [`execute`], which reads a `Prepared` in the timed region, sees no
    /// new field.
    pub fn answer_header(&self) -> RelationHeader {
        let head = &self.query.head;
        RelationHeader::new(&head.name, head.terms.iter().map(ToString::to_string).collect())
    }
```

- [ ] **Step 4: Run them to see them pass**

```bash
nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit --lib -- db::lifecycle::tests::the_answer_header_names_the_head db::lifecycle::tests::an_empty_head_names_an_answer_of_arity_zero
```

Expected: `test result: ok. 2 passed`.

- [ ] **Step 5: Run the module**

```bash
nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit --lib db::
```

Expected: `test result: ok.` with 0 failed.

- [ ] **Step 6: Format and commit**

```bash
nix develop --command cargo fmt --all
git status --short
git add kermit/src/db/lifecycle.rs
git commit -q -F - <<'EOF'
feat(db): Prepared::answer_header names a query's answer

The header is named after the head predicate, one attribute per head
variable, derived from the executed query's head on each call so
Prepared gains no field.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_012YYYQo7fUggdHrTt6wWjtk
EOF
```

---

### Task 2: `lftj_join` and `hash_join` return the answer as a relation

**Files:**
- Modify: `kermit/src/db.rs` (module doc lines 1–14, imports 32–36, `run_join` 38–60, `lftj_join`
  85–101, `hash_join` 120–135, tests 137–596)

- [ ] **Step 1: Write the failing tests**

In `kermit/src/db.rs`'s test module, insert after the closing `}` of
`the_streaming_entry_points_visit_every_row` (line 448):

```rust

    /// A collected answer is a relation: named after the head predicate, one
    /// attribute per head variable in head order, and its rows at the head's
    /// arity, from both families.
    #[test]
    fn the_answer_is_a_relation_named_after_the_head() {
        let catalog = counts(vec![("p", 2, vec![vec![1, 10], vec![2, 20]])]);
        let query: JoinQuery = "Q(Y, X) :- p(X, Y).".parse().unwrap();
        let planner = planner_of(LexicographicOptimiser);
        for (header, rows) in [
            lftj_join::<TreeTrie, LeapfrogTriejoin>(&catalog, query.clone(), &planner).unwrap(),
            hash_join::<HashTrie>(&catalog, query.clone(), &planner).unwrap(),
        ] {
            assert_eq!(header.name(), "Q");
            assert_eq!(header.attrs().to_vec(), vec!["Y", "X"]);
            assert_eq!(rows.arity(), 2);
            let mut got = rows.to_vecs();
            got.sort();
            assert_eq!(got, vec![vec![10, 1], vec![20, 2]]);
        }
    }

    /// An empty answer keeps the head's arity: the batch is sized by the head,
    /// not by a first row, which a list of rows could not show.
    #[test]
    fn an_empty_answer_keeps_the_heads_arity() {
        let catalog = counts(vec![
            ("p", 2, vec![vec![1, 10], vec![2, 20]]),
            ("q", 1, vec![vec![30]]),
        ]);
        let query: JoinQuery = "Q(X, Y) :- p(X, Y), q(Y).".parse().unwrap();
        let planner = planner_of(LexicographicOptimiser);
        for (header, rows) in [
            lftj_join::<TreeTrie, LeapfrogTriejoin>(&catalog, query.clone(), &planner).unwrap(),
            hash_join::<HashTrie>(&catalog, query.clone(), &planner).unwrap(),
        ] {
            assert!(rows.is_empty());
            assert_eq!(rows.arity(), 2);
            assert_eq!(header.arity(), 2);
        }
    }

    /// A query whose atoms are all constant checks answers with the empty
    /// row: arity 0, once on a sorted trie and once per matching row on a
    /// hash trie. The parser cannot write an empty head, so it is cleared.
    #[test]
    fn an_answer_of_checks_alone_has_arity_zero() {
        let catalog = counts(vec![("s", 2, vec![vec![7, 8], vec![7, 8], vec![9, 9]])]);
        let mut query: JoinQuery = "Q(X) :- s(c7, c8).".parse().unwrap();
        query.head.terms.clear();
        let planner = planner_of(LexicographicOptimiser);

        let (header, sorted) =
            lftj_join::<TreeTrie, LeapfrogTriejoin>(&catalog, query.clone(), &planner).unwrap();
        assert_eq!((header.name(), header.arity()), ("Q", 0));
        assert_eq!((sorted.arity(), sorted.len()), (0, 1));

        let (_, hashed) = hash_join::<HashTrie>(&catalog, query, &planner).unwrap();
        assert_eq!((hashed.arity(), hashed.len()), (0, 2));
    }

    /// An answer is a relation in the form a catalog takes, so a second query
    /// reads it with no conversion: two-step paths, then the vertices on a
    /// two-cycle, under both families.
    #[test]
    fn an_answer_reenters_a_catalog_as_a_relation() {
        let catalog = counts(vec![("edge", 2, vec![
            vec![1, 2],
            vec![2, 3],
            vec![3, 1],
            vec![2, 1],
        ])]);
        let paths: JoinQuery = "Q(X, Z) :- edge(X, Y), edge(Y, Z).".parse().unwrap();
        let cycles: JoinQuery = "P(X) :- Q(X, X).".parse().unwrap();
        let planner = planner_of(LexicographicOptimiser);

        let answer =
            lftj_join::<TreeTrie, LeapfrogTriejoin>(&catalog, paths.clone(), &planner).unwrap();
        let again = Catalog::new(vec![answer], StatisticsLevel::TupleCounts);
        let (_, rows) =
            lftj_join::<TreeTrie, LeapfrogTriejoin>(&again, cycles.clone(), &planner).unwrap();
        let mut sorted = rows.to_vecs();
        sorted.sort();
        assert_eq!(sorted, vec![vec![1], vec![2]]);

        let answer = hash_join::<HashTrie>(&catalog, paths, &planner).unwrap();
        let again = Catalog::new(vec![answer], StatisticsLevel::TupleCounts);
        let (_, rows) = hash_join::<HashTrie>(&again, cycles, &planner).unwrap();
        let mut hashed = rows.to_vecs();
        hashed.sort();
        assert_eq!(hashed, vec![vec![1], vec![2]]);
    }
```

(Paths over `edge`: `Q = {(1,3), (1,1), (2,1), (2,2), (3,2)}`, so the two-cycle vertices are 1 and
2.)

- [ ] **Step 2: Run them to see them fail**

```bash
nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit --lib db::tests
```

Expected: compile errors `mismatched types` (`expected Vec<Vec<usize>>`, found a tuple) and
`no method named `name` found`.

- [ ] **Step 3: Rewrite the module doc and imports**

Replace lines 1–14 (from `//! Join entry points bridging` through
`//! wrapper for a constant or a repeated variable.`) with:

```rust
//! Join entry points bridging a parsed Datalog query to a [`Catalog`].
//!
//! A query runs in three steps, each a free function here: [`prepare`]
//! validates and plans it against the catalog's headers and statistics,
//! [`build`] builds the indexes the plan reads from the catalog's flat
//! tuples, and [`execute`] joins over them, lending each answer row to a
//! sink. Each iterator family has one streaming function that runs all
//! three — [`lftj_join_for_each`] for sorted tries under a
//! [`TrieIterable`]-family algorithm, [`hash_join_for_each`] for
//! [`HashTrieIterable`] structures under [`HashTriejoin`] — and one
//! collecting function ([`lftj_join`], [`hash_join`]) whose sink is a flat
//! [`Tuples`] batch. A collected answer is a relation: the
//! [`RelationHeader`] that [`Prepared::answer_header`] names and its rows,
//! the pair a [`Catalog`] is built from. Each is planned by a [`Planner`].
//! The streaming functions share one private body and the collecting ones
//! another, and they differ only in the bounds they put on the relation type
//! and the algorithm they run: an index is a plain relation, so no algorithm
//! needs a wrapper for a constant or a repeated variable.
```

Replace the import block

```rust
use {
    kermit_algos::{HashTriejoin, JoinAlgo, JoinQuery, Planner},
    kermit_ds::Relation,
    kermit_iters::{HashTrieIterable, TrieIterable},
};
```

with

```rust
use {
    kermit_algos::{HashTriejoin, JoinAlgo, JoinQuery, Planner},
    kermit_ds::{Relation, RelationHeader, Tuples},
    kermit_iters::{HashTrieIterable, TrieIterable},
};
```

- [ ] **Step 4: Add `collect_join` after `run_join`**

Insert after the closing `}` of `run_join`:

```rust

/// The collecting twin of [`run_join`]: the same three steps, with a flat
/// [`Tuples`] sink sized by the head, so an empty answer keeps the head's
/// arity. Returns the answer as a relation, its header from
/// [`Prepared::answer_header`].
///
/// # Errors
///
/// As [`run_join`].
///
/// # Panics
///
/// If the answer has more than [`RowId::MAX`](kermit_ds::RowId) rows, the
/// most a [`Tuples`] batch holds.
pub(crate) fn collect_join<R, JA>(
    catalog: &Catalog, query: &JoinQuery, planner: &Planner,
) -> Result<(RelationHeader, Tuples), JoinError>
where
    R: Relation,
    JA: JoinAlgo<R>,
{
    let prepared = prepare(catalog, query, planner)?;
    let indexes = build::<R>(catalog, &prepared);
    let mut rows = Tuples::new(prepared.head_len());
    execute::<R, JA>(&prepared, &indexes, |row| rows.push(row));
    Ok((prepared.answer_header(), rows))
}
```

- [ ] **Step 5: Rewrite `lftj_join` and `hash_join`**

Replace `lftj_join` (its doc comment and body, `/// [`lftj_join_for_each`], collected: returns
every result tuple.` through its closing `}`) with:

```rust
/// [`lftj_join_for_each`], collected: returns the answer as a relation, the
/// header [`Prepared::answer_header`] names and every result row in one flat
/// [`Tuples`] batch of the head's arity, the pair a [`Catalog`] is built
/// from.
///
/// # Errors
///
/// As [`lftj_join_for_each`].
///
/// # Panics
///
/// If the answer has more than [`RowId::MAX`](kermit_ds::RowId) rows; stream
/// such an answer with [`lftj_join_for_each`] instead.
pub fn lftj_join<R, JA>(
    catalog: &Catalog, query: JoinQuery, planner: &Planner,
) -> Result<(RelationHeader, Tuples), JoinError>
where
    R: Relation + TrieIterable,
    JA: JoinAlgo<R>,
{
    collect_join::<R, JA>(catalog, &query, planner)
}
```

Replace `hash_join` (its doc comment and body) with:

```rust
/// [`hash_join_for_each`], collected: returns the answer as a relation, as
/// [`lftj_join`] does.
///
/// # Errors
///
/// As [`hash_join_for_each`].
///
/// # Panics
///
/// If the answer has more than [`RowId::MAX`](kermit_ds::RowId) rows; stream
/// such an answer with [`hash_join_for_each`] instead.
pub fn hash_join<R>(
    catalog: &Catalog, query: JoinQuery, planner: &Planner,
) -> Result<(RelationHeader, Tuples), JoinError>
where
    R: Relation + HashTrieIterable,
{
    collect_join::<R, HashTriejoin>(catalog, &query, planner)
}
```

- [ ] **Step 6: Adapt the existing tests to the pair**

In `sorted_family`, replace

```rust
        lftj_join::<TreeTrie, LeapfrogTriejoin>(catalog, query.clone(), &planner)
```

with

```rust
        lftj_join::<TreeTrie, LeapfrogTriejoin>(catalog, query.clone(), &planner)
            .map(|(_, rows)| rows.to_vecs())
```

In `hash_family`, replace

```rust
        hash_join::<HashTrie>(catalog, query.clone(), &planner)
```

with

```rust
        hash_join::<HashTrie>(catalog, query.clone(), &planner).map(|(_, rows)| rows.to_vecs())
```

In `the_planner_reads_the_catalogs_statistics_per_access`, replace

```rust
            &planner_of(spy.clone()),
        )
        .unwrap();
        check(&spy, rows);
```

with

```rust
            &planner_of(spy.clone()),
        )
        .unwrap()
        .1
        .to_vecs();
        check(&spy, rows);
```

and replace

```rust
        let rows = hash_join::<HashTrie>(&catalog, query, &planner_of(spy.clone())).unwrap();
```

with

```rust
        let rows = hash_join::<HashTrie>(&catalog, query, &planner_of(spy.clone()))
            .unwrap()
            .1
            .to_vecs();
```

In `an_optimiser_reading_ungathered_statistics_is_refused`, replace

```rust
            lftj_join::<TreeTrie, LeapfrogTriejoin>(&analysed, query, &planner_of(NeedsColumns))
                .unwrap();
```

with

```rust
            lftj_join::<TreeTrie, LeapfrogTriejoin>(&analysed, query, &planner_of(NeedsColumns))
                .unwrap()
                .1
                .to_vecs();
```

`validation_errors_win_over_missing_statistics` compares with an `Err` and needs no change:
`RelationHeader` and `Tuples` both derive `PartialEq` and `Debug`.

- [ ] **Step 7: Run the module to see it pass**

```bash
nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit --lib db::
```

Expected: `test result: ok.`, including the four new tests.

- [ ] **Step 8: Format and commit**

```bash
nix develop --command cargo fmt --all
git status --short
git add kermit/src/db.rs
git commit -q -F - <<'EOF'
feat(db)!: lftj_join and hash_join return the answer as a relation

They return (RelationHeader, Tuples), the pair a Catalog is built
from, through a private collect_join: prepare, build, then execute
into a Tuples sink sized by the head. An empty answer keeps the head's
arity, and an answer re-enters a catalog with no conversion.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_012YYYQo7fUggdHrTt6wWjtk
EOF
```

---

### Task 3: `compute_join` returns `Tuples`

**Files:**
- Modify: `kermit/src/lib.rs` (doc lines 28–52, body 101–105, tests 116–131 and a new test)

- [ ] **Step 1: Write the failing test**

In `compute_join_tests`, insert after the closing `}` of `the_variables_are_the_output_columns`:

```rust

    /// The rows come back as one flat batch at the head's arity, which an
    /// empty result keeps.
    #[test]
    fn the_result_is_one_batch_at_the_heads_arity() {
        let rows = compute_join::<TreeTrie, LeapfrogTriejoin>(chain_input(), vec![0, 2], vec![
            vec![0, 1],
            vec![1, 2],
        ]);
        assert_eq!((rows.arity(), rows.len()), (2, 3));

        let input: Vec<Rows> = vec![Vec::new(), vec![vec![2, 3]]];
        let empty = compute_join::<HashTrie, HashTriejoin>(input, vec![0, 1, 2], vec![
            vec![0, 1],
            vec![1, 2],
        ]);
        assert_eq!((empty.arity(), empty.len()), (3, 0));
    }
```

- [ ] **Step 2: Run it to see it fail**

```bash
nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit --lib compute_join_tests::the_result_is_one_batch_at_the_heads_arity
```

Expected: compile error `no method named `arity` found for struct `Vec<Vec<usize>>``.

- [ ] **Step 3: Implement**

Replace

```rust
/// Convenience function that builds relations from raw tuple batches and runs a
/// join, returning the result tuples.
```

with

```rust
/// Convenience function that builds relations from raw tuple batches and runs a
/// join, returning the result rows in one flat [`Tuples`] batch. Its inputs
/// carry no headers, so neither does its result.
```

Replace

```rust
/// [`db::JoinError`]'s. Also if `rel_variables` has more entries than
/// `input` has batches: the entry's relation `R{i}` was never loaded, so its
/// atom is an unknown relation.
```

with

```rust
/// [`db::JoinError`]'s. Also if `rel_variables` has more entries than
/// `input` has batches: the entry's relation `R{i}` was never loaded, so its
/// atom is an unknown relation. Also if the result has more than
/// [`RowId::MAX`](kermit_ds::RowId) rows, the most a [`Tuples`] batch holds.
```

Replace the return type `) -> Vec<Vec<usize>>` of `compute_join` with `) -> Tuples`, and replace

```rust
    let catalog = Catalog::new(relations, planner.required_statistics());
    let mut rows = Vec::new();
    db::run_join::<R, JA>(&catalog, &query, &planner, |row| rows.push(row.to_vec()))
        .unwrap_or_else(|error| panic!("compute_join: {error}"));
    rows
```

with

```rust
    let catalog = Catalog::new(relations, planner.required_statistics());
    let (_, rows) = db::collect_join::<R, JA>(&catalog, &query, &planner)
        .unwrap_or_else(|error| panic!("compute_join: {error}"));
    rows
```

- [ ] **Step 4: Adapt the helper**

In `joined`, replace

```rust
        let mut sorted = compute_join::<TreeTrie, LeapfrogTriejoin>(
            input.clone(),
            variables.clone(),
            rel_variables.clone(),
        );
        let mut hashed = compute_join::<HashTrie, HashTriejoin>(input, variables, rel_variables);
```

with

```rust
        let mut sorted = compute_join::<TreeTrie, LeapfrogTriejoin>(
            input.clone(),
            variables.clone(),
            rel_variables.clone(),
        )
        .to_vecs();
        let mut hashed =
            compute_join::<HashTrie, HashTriejoin>(input, variables, rel_variables).to_vecs();
```

- [ ] **Step 5: Run the tests to see them pass**

```bash
nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit --lib compute_join_tests
```

Expected: `test result: ok. 9 passed` (eight existing, one new).

- [ ] **Step 6: Format and commit**

```bash
nix develop --command cargo fmt --all
git status --short
git add kermit/src/lib.rs
git commit -q -F - <<'EOF'
feat(kermit)!: compute_join returns its rows as one Tuples batch

Collected through collect_join; its inputs carry no headers, so the
result does not either.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_012YYYQo7fUggdHrTt6wWjtk
EOF
```

---

### Task 4: The test harness collects relations and checks their headers

**Files:**
- Modify: `kermit/tests/common/utils.rs` (doc 1–9, imports 11–20, `JoinEntry` 26–90,
  `join_under_planner` 122–137, `test_join` 196–210)
- Modify: `kermit/tests/subject_position_constant.rs:44-53`
- Modify: `kermit/tests/all_orders.rs:242-269`

- [ ] **Step 1: Rewrite the harness**

In `kermit/tests/common/utils.rs`, replace

```rust
//! [`JoinEntry`] trait maps an algorithm type to the execution that hosts
//! it, mirroring how the CLI's `execution` module pairs them.
```

with

```rust
//! [`JoinEntry`] trait maps an algorithm type to the execution that hosts
//! it, mirroring how the CLI's `execution` module pairs them: `join`
//! collects through `kermit::db::lftj_join` / `hash_join`, and `count`
//! streams through `execute` into a counter.
```

Replace `kermit::db::{build, execute, prepare, Catalog, JoinError},` with
`kermit::db::{build, execute, hash_join, lftj_join, prepare, Catalog, JoinError},`.

Replace the trait's `join` declaration

```rust
    /// Every row of `query`'s head, collected.
    fn join(
        catalog: &Catalog, query: JoinQuery, planner: &Planner,
    ) -> Result<Vec<Vec<usize>>, JoinError>;
```

with

```rust
    /// `query`'s answer, collected by the entry point as a relation: the
    /// header `Prepared::answer_header` names and every head row in one flat
    /// `Tuples` batch.
    fn join(
        catalog: &Catalog, query: JoinQuery, planner: &Planner,
    ) -> Result<(RelationHeader, Tuples), JoinError>;
```

Delete the `collect` function:

```rust
fn collect<R: Relation, JA: JoinAlgo<R>>(
    catalog: &Catalog, query: &JoinQuery, planner: &Planner,
) -> Result<Vec<Vec<usize>>, JoinError> {
    let mut rows = Vec::new();
    run::<R, JA>(catalog, query, planner, |row| rows.push(row.to_vec()))?;
    Ok(rows)
}

```

In the `LeapfrogTriejoin` impl, replace

```rust
    fn join(
        catalog: &Catalog, query: JoinQuery, planner: &Planner,
    ) -> Result<Vec<Vec<usize>>, JoinError> {
        collect::<R, LeapfrogTriejoin>(catalog, &query, planner)
    }
```

with

```rust
    fn join(
        catalog: &Catalog, query: JoinQuery, planner: &Planner,
    ) -> Result<(RelationHeader, Tuples), JoinError> {
        lftj_join::<R, LeapfrogTriejoin>(catalog, query, planner)
    }
```

In the `HashTriejoin` impl, replace

```rust
    fn join(
        catalog: &Catalog, query: JoinQuery, planner: &Planner,
    ) -> Result<Vec<Vec<usize>>, JoinError> {
        collect::<R, HashTriejoin>(catalog, &query, planner)
    }
```

with

```rust
    fn join(
        catalog: &Catalog, query: JoinQuery, planner: &Planner,
    ) -> Result<(RelationHeader, Tuples), JoinError> {
        hash_join::<R>(catalog, query, planner)
    }
```

Replace `join_under_planner` (its doc comment and body) with:

```rust
/// Runs `query` over `catalog` under `planner` through `JA`: joined and
/// collected, and counted through the streamed path `bench run` times, which
/// must agree. Checks the answer's header against `query`'s head: named
/// after the head predicate, the head's variables as attributes in head
/// order, and the batch's arity. Returns the rows and the index specs the
/// query built (what `prepare` named; each is built again by the join and by
/// the count).
pub fn join_under_planner<R: Relation, JA: JoinEntry<R>>(
    catalog: &Catalog, query: JoinQuery, planner: &Planner,
) -> Result<(Tuples, Vec<IndexSpec>), JoinError> {
    let specs = prepare(catalog, &query, planner)?.indexes().to_vec();
    let (header, rows) = JA::join(catalog, query.clone(), planner)?;
    let head: Vec<String> = query.head.terms.iter().map(ToString::to_string).collect();
    assert_eq!(header.name(), query.head.name, "the answer is named after the head");
    assert_eq!(header.attrs(), head, "the answer's attributes are the head's variables");
    assert_eq!(rows.arity(), header.arity(), "the rows have the header's arity");
    let streamed = JA::count(catalog, query, planner)?;
    assert_eq!(
        streamed,
        rows.len() as u64,
        "streamed count disagrees with the collected rows"
    );
    Ok((rows, specs))
}
```

At the end of `test_join`, replace

```rust
    let (mut actual, _) = join_under_planner::<R, JA>(&catalog, query, &planner)
        .unwrap_or_else(|e| panic!("{query_str}: {e}"));

    // Multiset equality (relational algebra semantics) — sort both sides
    // before asserting so algorithms with non-sorted output (hash-trie
    // family) and plans with different enumeration orders pass the same
    // suite.
    actual.sort();
```

with

```rust
    let (actual, _) = join_under_planner::<R, JA>(&catalog, query, &planner)
        .unwrap_or_else(|e| panic!("{query_str}: {e}"));

    // Multiset equality (relational algebra semantics) — sort both sides
    // before asserting so algorithms with non-sorted output (hash-trie
    // family) and plans with different enumeration orders pass the same
    // suite.
    let mut actual = actual.to_vecs();
    actual.sort();
```

- [ ] **Step 2: Adapt `subject_position_constant.rs`**

In `join_with_specs`, replace

```rust
    join_under_planner::<R, JA>(&catalog, q, &planner).unwrap()
```

with

```rust
    let (rows, specs) = join_under_planner::<R, JA>(&catalog, q, &planner).unwrap();
    (rows.to_vecs(), specs)
```

- [ ] **Step 3: Adapt `all_orders.rs`**

In `rows_in_order`, replace

```rust
    let indexes = build::<R>(catalog, &prepared);
    let mut rows = Vec::new();
    execute::<R, JA>(&prepared, &indexes, |row| rows.push(row.to_vec()));
    rows.sort();
    rows
```

with

```rust
    let indexes = build::<R>(catalog, &prepared);
    let mut rows = Tuples::new(prepared.head_len());
    execute::<R, JA>(&prepared, &indexes, |row| rows.push(row));
    rows.sort();
    rows.to_vecs()
```

- [ ] **Step 4: Run every suite that reads collected rows**

```bash
nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit --test join_tests --test all_orders --test subject_position_constant --test lubm_mini_oracle --test watdiv_correctness --test h2o_generate --test snap_generate --test job_generate --test lubm_cardinalities
```

Expected: every binary reports `test result: ok.` with 0 failed. (`lubm_cardinalities` runs here
because `nix develop` puts `java` on `PATH`.)

- [ ] **Step 5: Format and commit**

```bash
nix develop --command cargo fmt --all
git status --short
git add kermit/tests/common/utils.rs kermit/tests/subject_position_constant.rs kermit/tests/all_orders.rs
git commit -q -F - <<'EOF'
test(kermit): the join suites collect answers as relations

JoinEntry::join goes through lftj_join / hash_join, and
join_under_planner checks every answer's header against the head:
its name, its variables in head order, the batch's arity.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_012YYYQo7fUggdHrTt6wWjtk
EOF
```

- [ ] **Step 6: Check the header assertion bites (mutant)**

In `kermit/src/db/lifecycle.rs`'s `answer_header`, replace
`head.terms.iter().map(ToString::to_string).collect()` with
`head.terms.iter().rev().map(ToString::to_string).collect()`. Confirm it applied:

```bash
grep -c 'terms.iter().rev()' kermit/src/db/lifecycle.rs
```

Expected: `1`. Then:

```bash
nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit --test join_tests triangle_
```

Expected: FAIL, with `the answer's attributes are the head's variables` in the panic messages.
Revert with the exact reverse replacement (`.rev()` removed), then:

```bash
git status --short
```

Expected: no output.

---

### Task 5: Delete `JoinAlgo::join_iter`

**Files:**
- Modify: `kermit-algos/src/join_algo.rs` (whole file)
- Modify: `kermit-algos/src/hash/hash_triejoin.rs` (test module imports 298–301; tests 467–532)
- Modify: `kermit-algos/src/sorted/leapfrog_triejoin.rs:404-409` (comment)

- [ ] **Step 1: Give the HashTriejoin tests their own collector**

In `kermit-algos/src/hash/hash_triejoin.rs`, replace the test module's imports

```rust
    use {
        super::*,
        crate::optimiser::{CatalogStats, LexicographicOptimiser, QueryOptimiser},
    };
```

with

```rust
    use {
        super::*,
        crate::optimiser::{CatalogStats, LexicographicOptimiser, QueryOptimiser},
        kermit_iters::Tuples,
    };

    /// `query` joined by [`HashTriejoin`] under `plan`, collected as a caller
    /// collects through `join_for_each`: into a flat [`Tuples`] batch with
    /// one column per query variable. Returns the rows sorted.
    fn joined<DS: HashTrieIterable>(
        plan: &QueryPlan, query: JoinQuery, ds: HashMap<String, &DS>,
    ) -> Vec<Vec<usize>> {
        let mut rows = Tuples::new(plan.variable_ordering.len());
        HashTriejoin::join_for_each(plan, query, ds, |row| rows.push(row));
        rows.sort();
        rows.to_vecs()
    }
```

In `join_algo_drops_inner_level_collision_false_positive`, replace

```rust
        let mut out: Vec<Vec<usize>> = HashTriejoin::join_iter(&plan, query, ds).collect();
        out.sort();
        assert_eq!(out, vec![vec![1, 2, 5]]);
```

with

```rust
        assert_eq!(joined(&plan, query, ds), vec![vec![1, 2, 5]]);
```

In `join_algo_drops_leaf_chain_collision_false_positive`, replace

```rust
        let mut out: Vec<Vec<usize>> = HashTriejoin::join_iter(&plan, query, ds).collect();
        out.sort();
        assert_eq!(out, vec![vec![11]]);
```

with

```rust
        assert_eq!(joined(&plan, query, ds), vec![vec![11]]);
```

In `join_algo_unary_intersection`, replace

```rust
        let mut out: Vec<Vec<usize>> = HashTriejoin::join_iter(&plan, query, ds).collect();
        out.sort();
        assert_eq!(out, vec![vec![2], vec![3]]);
```

with

```rust
        assert_eq!(joined(&plan, query, ds), vec![vec![2], vec![3]]);
```

In `join_algo_triangle`, replace

```rust
        let mut out: Vec<Vec<usize>> = HashTriejoin::join_iter(&plan, query, ds).collect();
        out.sort();
        assert_eq!(out, vec![vec![1, 2, 3], vec![2, 3, 1], vec![3, 1, 2]]);
```

with

```rust
        assert_eq!(joined(&plan, query, ds), vec![
            vec![1, 2, 3],
            vec![2, 3, 1],
            vec![3, 1, 2]
        ]);
```

- [ ] **Step 2: Run them against the old trait (they must still pass)**

```bash
nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-algos --lib hash::hash_triejoin
```

Expected: `test result: ok.` with 0 failed.

- [ ] **Step 3: Delete `join_iter`**

Replace the whole of `kermit-algos/src/join_algo.rs` with:

```rust
//! This module defines the `JoinAlgo` trait, used as a base for join
//! algorithms.

use {
    crate::optimiser::QueryPlan, kermit_iters::JoinIterable, kermit_parser::JoinQuery,
    std::collections::HashMap,
};

/// The `JoinAlgo` trait is used as a base for join algorithms.
pub trait JoinAlgo<DS>
where
    DS: JoinIterable,
{
    /// Joins the given iterables according to `plan`, which must have been
    /// produced by a [`QueryOptimiser`](crate::optimiser::QueryOptimiser)
    /// for this exact `query` (the executed query of `IndexPlan::query`,
    /// whose atoms name the indexes, with the plan translated to it), and
    /// passes each result tuple to `emit` in canonical (head-first) variable
    /// order.
    ///
    /// # Streaming
    ///
    /// This is the trait's one method, and it never materialises the result.
    /// The slice handed to `emit` is borrowed for that call only, so an
    /// implementation reuses one scratch row and never allocates a result
    /// tuple. The `iteration` and `end_to_end` benchmark metrics time
    /// exactly this path, counting rows through a `black_box` sink (issue
    /// #65). The sink is a generic parameter rather than `&mut dyn FnMut`, so
    /// the per-tuple call is monomorphised, not an indirect call inside the
    /// timed region.
    ///
    /// # Collecting
    ///
    /// To collect the result, pass a sink that pushes each row into a
    /// [`Tuples`](kermit_iters::Tuples) batch of arity
    /// `plan.variable_ordering.len()`: one column per variable of `query`, in
    /// canonical order.
    ///
    /// # Panics
    ///
    /// Implementations panic if `plan` fails
    /// [`QueryPlan::validate`] against `query` — an invalid plan is a
    /// caller programming error, never a recoverable runtime condition.
    fn join_for_each<S: FnMut(&[usize])>(
        plan: &QueryPlan, query: JoinQuery, datastructures: HashMap<String, &DS>, emit: S,
    );
}
```

- [ ] **Step 4: Update the LFTJ test-placement comment**

In `kermit-algos/src/sorted/leapfrog_triejoin.rs`, replace

```rust
// These tests stay inline deliberately, despite using `kermit-ds` and being
// roughly half this file. They drive `LeapfrogTriejoinIter` directly — a
// `pub(crate)` type — rather than the public `LeapfrogTriejoin::join_iter`,
// so moving them to `tests/` would mean exporting the iterator and its two
// traits purely to serve tests. Keeping the iterator internal is the
// deliberate choice; the test placement follows from it.
```

with

```rust
// These tests stay inline deliberately, despite using `kermit-ds` and being
// roughly half this file. They drive `LeapfrogTriejoinIter` directly — a
// `pub(crate)` type — rather than the public
// `LeapfrogTriejoin::join_for_each`, so moving them to `tests/` would mean
// exporting the iterator and its two traits purely to serve tests. Keeping
// the iterator internal is the deliberate choice; the test placement follows
// from it.
```

- [ ] **Step 5: Run the crate and check nothing names `join_iter`**

```bash
nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-algos
grep -rn '::join_iter\|fn join_iter' --include='*.rs' kermit-algos kermit kermit-ds kermit-iters
```

Expected: `test result: ok.` for every kermit-algos binary; the `grep` prints nothing.

- [ ] **Step 6: Build the dependents**

```bash
nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit --no-run
```

Expected: `Finished`, no errors.

- [ ] **Step 7: Format and commit**

```bash
nix develop --command cargo fmt --all
git status --short
git add kermit-algos/src/join_algo.rs kermit-algos/src/hash/hash_triejoin.rs kermit-algos/src/sorted/leapfrog_triejoin.rs
git commit -q -F - <<'EOF'
refactor(algos)!: delete JoinAlgo::join_iter

join_for_each is the trait's one method; a caller collects with a
sink that pushes into a flat Tuples batch, as the HashTriejoin tests
now do.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_012YYYQo7fUggdHrTt6wWjtk
EOF
```

---

### Task 6: The CSV header row comes from `answer_header`

**Files:**
- Create: `kermit/tests/fixtures/reordered_head_query.dl`
- Modify: `kermit/tests/cli_join_tests.rs` (new test after `cli_join_path_query`, which ends at line
  268)
- Modify: `kermit/src/main.rs` (imports 16 and 19; `head_column_names` 413–428;
  `write_prepared_join` 495–519; test 1093–1097)

- [ ] **Step 1: Add the fixture**

```bash
printf 'path(Z, X) :- edge(X, Y), edge(Y, Z).' > kermit/tests/fixtures/reordered_head_query.dl
```

(The other `.dl` fixtures have no trailing newline either.)

- [ ] **Step 2: Pin the header row (characterisation test)**

In `kermit/tests/cli_join_tests.rs`, insert after the closing `}` of `cli_join_path_query`:

```rust

/// The CSV header row is the answer's attributes: the head's variables in
/// head order, not the body's, on every cell. `path(Z, X)` reorders and
/// projects the path query's variables. Over edge = {(1,2), (2,3), (3,4),
/// (1,3)} the paths are (1,2,3), (1,3,4) and (2,3,4).
#[test]
fn cli_join_writes_the_heads_variables_as_the_header_row() {
    for (algorithm, indexstructure) in [
        ("leapfrog-triejoin", "tree-trie"),
        ("leapfrog-triejoin", "column-trie"),
        ("hash-triejoin", "hash-trie"),
    ] {
        let output = run_join(
            &["edge.csv"],
            "reordered_head_query.dl",
            algorithm,
            indexstructure,
        );
        assert!(
            output.status.success(),
            "{indexstructure}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert_eq!(
            stdout.lines().next(),
            Some("Z,X"),
            "{indexstructure}: the header row is the head's variables"
        );
        assert_eq!(parse_output(&output), vec![
            vec![3, 1],
            vec![4, 1],
            vec![4, 2]
        ]);
    }
}
```

- [ ] **Step 3: Run it against the current code (it must pass)**

```bash
nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit --test cli_join_tests cli_join_writes_the_heads_variables_as_the_header_row
```

Expected: `test result: ok. 1 passed`.

- [ ] **Step 4: Take the header from `answer_header`**

In `kermit/src/main.rs`, replace `    kermit_algos::{JoinAlgorithm, JoinQuery},` with
`    kermit_algos::JoinAlgorithm,`, and delete the line `    kermit_parser::Term,`.

Delete `head_column_names` with its doc comment (lines 413–428, from `/// Column names derived from a
query's head predicate` through its closing `}`, plus the blank line after it).

Replace

```rust
/// Builds the indexes of `workload`'s one query on `family` and streams the
/// join to `writer` as CSV: the header row, then one line per tuple as the
/// join produces it, so writing a result never materialises it. `workload`
/// comes from [`prepare_query`], so it holds exactly one query.
```

with

```rust
/// Builds the indexes of `workload`'s one query on `family` and streams the
/// join to `writer` as CSV: the header row (the answer's attributes,
/// `Prepared::answer_header`), then one line per tuple as the join produces
/// it, so writing a result never materialises it. `workload` comes from
/// [`prepare_query`], so it holds exactly one query.
```

and replace

```rust
    let query = &workload.workload.queries[0].query;
    let prepared = &workload.prepared[0];
    let indexes = family.build_indexes(&workload.catalog, prepared);
    let mut sink = CsvSink::new(writer, &head_column_names(query))?;
```

with

```rust
    let prepared = &workload.prepared[0];
    let indexes = family.build_indexes(&workload.catalog, prepared);
    let mut sink = CsvSink::new(writer, prepared.answer_header().attrs())?;
```

Delete the unit test (and the blank line after it):

```rust
    #[test]
    fn head_column_names_extracts_variables_atoms_and_placeholders() {
        let q: JoinQuery = "Q(X, Y, _) :- R(X, Y, Z).".parse().unwrap();
        assert_eq!(head_column_names(&q), vec!["X", "Y", "_"]);
    }
```

- [ ] **Step 5: Run the CLI tests and the binary's unit tests**

```bash
nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit --test cli_join_tests --test cli_query_errors
nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit --bin kermit
```

Expected: `test result: ok.` for each, 0 failed.

- [ ] **Step 6: Lint the binary's docs**

```bash
nix develop --command env CARGO_BUILD_JOBS=2 cargo rustdoc -p kermit --bin kermit -- --document-private-items -D warnings
```

Expected: `Finished` (a warning about the output filename colliding with the lib's docs is
harmless).

- [ ] **Step 7: Format and commit**

```bash
nix develop --command cargo fmt --all
git status --short
git add kermit/src/main.rs kermit/tests/cli_join_tests.rs kermit/tests/fixtures/reordered_head_query.dl
git commit -q -F - <<'EOF'
refactor(cli): take the CSV header row from Prepared::answer_header

head_column_names goes; a CLI test pins the header row on every cell
with a head that reorders and projects the body's variables.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_012YYYQo7fUggdHrTt6wWjtk
EOF
```

---

### Task 7: Collecting allocates per doubling, not per row

**Files:**
- Modify: `kermit/tests/result_allocation.rs` (imports line 57; new section appended after the last
  test, which ends at line 717)

- [ ] **Step 1: Import the collectors**

Replace `    kermit::db::{build, execute, prepare, Catalog, Indexes, Prepared},` with
`    kermit::db::{build, execute, hash_join, hash_join_for_each, lftj_join, lftj_join_for_each, prepare, Catalog, Indexes, Prepared},`
(rustfmt rewraps it).

- [ ] **Step 2: Append the cells**

Append to the end of the file:

```rust

// ------------------------------------------------------------------
// Collecting an answer.
//
// `kermit::db::lftj_join` / `hash_join` run the lifecycle of their
// `_for_each` twins and push each head row into one flat `Tuples` batch.
// Measured against the streamed twin over the same catalog, the extra
// allocations are the batch's growth and the answer's header: 1000x more
// rows may add a few doublings, but never one allocation per row, as the
// `Vec` per row the collectors built before did.
// ------------------------------------------------------------------

/// The most 1000x more collected rows may add to the collector's extra
/// allocations: the batch's doublings, ⌈log₂ 1000⌉ = 10, with 2× slack. One
/// allocation per row would add about 10⁵.
const COLLECTED_GROWTH_BOUND: u64 = 20;

/// `QUERY` over the relations of `fan_out`, in a catalog of tuple counts.
fn collected_workload(fan_out: usize) -> (Catalog, JoinQuery) {
    (
        Catalog::new(relations(fan_out), StatisticsLevel::TupleCounts),
        QUERY.parse().unwrap(),
    )
}

/// The allocations `lftj_join` makes beyond `lftj_join_for_each` over the
/// same catalog: both prepare, build and execute alike, so the difference is
/// the collected batch and its header. Checks that the answer holds every
/// row. `allocation-counter`'s allocator implements no `realloc`, so the
/// default one allocates anew and every doubling of the batch is counted.
fn lftj_collected_extra<Rel: TrieIterable + Relation>(fan_out: usize) -> u64 {
    let (catalog, query) = collected_workload(fan_out);
    let planner = planner();
    let (collected_query, streamed_query) = (query.clone(), query);
    let mut rows = 0;
    let collected = allocation_counter::measure(|| {
        let (_, answer) =
            lftj_join::<Rel, LeapfrogTriejoin>(&catalog, collected_query, &planner).unwrap();
        rows = std::hint::black_box(answer).len();
    });
    assert_eq!(rows, XS * fan_out, "the answer must hold every row");
    let streamed = allocation_counter::measure(|| {
        lftj_join_for_each::<Rel, LeapfrogTriejoin>(&catalog, streamed_query, &planner, |row| {
            std::hint::black_box(row);
        })
        .unwrap();
    });
    collected
        .count_total
        .checked_sub(streamed.count_total)
        .expect("collecting allocates at least what streaming does")
}

/// [`lftj_collected_extra`] for `HashTrie<H, P>` under `hash_join`.
fn htj_collected_extra<H: HashStrategy, P: PruningPolicy>(fan_out: usize) -> u64 {
    let (catalog, query) = collected_workload(fan_out);
    let planner = planner();
    let (collected_query, streamed_query) = (query.clone(), query);
    let mut rows = 0;
    let collected = allocation_counter::measure(|| {
        let (_, answer) =
            hash_join::<HashTrie<H, P>>(&catalog, collected_query, &planner).unwrap();
        rows = std::hint::black_box(answer).len();
    });
    assert_eq!(rows, XS * fan_out, "the answer must hold every row");
    let streamed = allocation_counter::measure(|| {
        hash_join_for_each::<HashTrie<H, P>>(&catalog, streamed_query, &planner, |row| {
            std::hint::black_box(row);
        })
        .unwrap();
    });
    collected
        .count_total
        .checked_sub(streamed.count_total)
        .expect("collecting allocates at least what streaming does")
}

fn assert_collected_growth(cell: &str, small: u64, large: u64) {
    assert!(
        large <= small + COLLECTED_GROWTH_BOUND,
        "{cell}: collecting 1000x more rows took {large} allocations beyond the streamed join, \
         against {small}; the collector is allocating per row"
    );
}

#[test]
fn tree_trie_lftj_collects_without_allocating_per_row() {
    lftj_collected_extra::<TreeTrie>(SMALL);
    assert_collected_growth(
        "TreeTrie/LFTJ",
        lftj_collected_extra::<TreeTrie>(SMALL),
        lftj_collected_extra::<TreeTrie>(LARGE),
    );
}

#[test]
fn column_trie_lftj_collects_without_allocating_per_row() {
    lftj_collected_extra::<ColumnTrie>(SMALL);
    assert_collected_growth(
        "ColumnTrie/LFTJ",
        lftj_collected_extra::<ColumnTrie>(SMALL),
        lftj_collected_extra::<ColumnTrie>(LARGE),
    );
}

#[test]
fn hash_trie_sip_collects_without_allocating_per_row() {
    htj_collected_extra::<SipHashStrategy, NoPruning>(SMALL);
    assert_collected_growth(
        "HashTrie<Sip>/HTJ",
        htj_collected_extra::<SipHashStrategy, NoPruning>(SMALL),
        htj_collected_extra::<SipHashStrategy, NoPruning>(LARGE),
    );
}
```

- [ ] **Step 3: Run the cells**

```bash
nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit --test result_allocation collects_without_allocating_per_row
```

Expected: `test result: ok. 3 passed`. (At 100 rows of arity 3 the batch grows to 512 values in 8
allocations; at 100,000 rows to 524,288 in 18; the header adds 5 at both.)

- [ ] **Step 4: Run the whole file**

```bash
nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit --test result_allocation
```

Expected: `test result: ok.`, 0 failed.

- [ ] **Step 5: Format and commit**

```bash
nix develop --command cargo fmt --all
git status --short
git add kermit/tests/result_allocation.rs
git commit -q -F - <<'EOF'
test(kermit): collecting an answer allocates per doubling, not per row

One cell per valid (structure, algorithm) pair: lftj_join / hash_join
less their _for_each twin over the same catalog, at two result sizes
1000x apart, may differ by at most 20 allocations.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_012YYYQo7fUggdHrTt6wWjtk
EOF
```

- [ ] **Step 6: Check the cells bite (mutant)**

In `kermit/src/db.rs`'s `collect_join`, replace
`    execute::<R, JA>(&prepared, &indexes, |row| rows.push(row));` with

```rust
    execute::<R, JA>(&prepared, &indexes, |row| {
        rows.push(row);
        std::hint::black_box(row.to_vec());
    });
```

Confirm it applied:

```bash
grep -c 'black_box(row.to_vec())' kermit/src/db.rs
```

Expected: `1`. Then:

```bash
nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit --test result_allocation collects_without_allocating_per_row
```

Expected: 3 FAILED, each `the collector is allocating per row`. Revert with the exact reverse
replacement, then:

```bash
git status --short
```

Expected: no output.

---

### Task 8: Documentation

**Files:**
- Modify: `CLAUDE.md` (lines 147, 186, 218, 342)
- Modify: `ARCHITECTURE.md` (lines 322, 328–343, 464, 468)
- Modify: `docs/specs/benchmarking-architecture.md:101`
- Modify: `docs/algorithms/hash-triejoin.md` (lines 45, 48)
- Modify: `kermit-algos/README.md:31`
- Modify: `kermit/README.md` (lines 13, 14, 16)

- [ ] **Step 1: `CLAUDE.md`**

Replace the line

```
                  db::lftj_join / db::hash_join collect through them. Each runs the
```

with

```
                  db::lftj_join / db::hash_join collect the answer as a relation (the
                  RelationHeader Prepared::answer_header names and one flat Tuples
                  batch, the pair a Catalog is built from). Each runs the
```

In the `define_multiway_join_test_suite!()` bullet, replace
`rows are compared exactly as the entry points return them (a fixture's` with
`rows are compared exactly as the entry points return them, and utils::join_under_planner also checks each answer's header against the head (a fixture's`.

In "Adding a new join algorithm" step 2, replace `(`join_iter` is provided)` with
`(to collect, pass a sink that pushes into a flat `Tuples`)`.

In the "CLI join CSV header" gotcha, replace
`prepend a CSV header row built from the head's variable names (via `head_column_names` in `kermit/src/main.rs`).`
with
`prepend a CSV header row of the answer's attributes, the head's variable names (`Prepared::answer_header`, read by `write_prepared_join` in `kermit/src/main.rs`).`

- [ ] **Step 2: `ARCHITECTURE.md`**

Replace
`` `join_iter` is a provided method that collects through `join_for_each`, for callers that need the rows; benchmarks never call it inside a timed region (issue #65).``
with
`` `join_for_each` is the trait's only method: a caller that needs the rows collects them with a sink that pushes into a flat `Tuples` batch, and the benchmarks time it with a counting sink (issue #65).``

In the `JoinAlgo` code block, delete these lines (and the blank line before them):

```rust
    // Provided: collects through `join_for_each`.
    fn join_iter(
        plan: &QueryPlan,
        query: JoinQuery,
        datastructures: HashMap<String, &DS>,
    ) -> impl Iterator<Item = Vec<usize>> { … }
```

In the `prepare` bullet of "Catalog and lifecycle", replace
`A `Prepared` holds the executed query, the plan, the `IndexSpec`s, the constant checks and the head's length.`
with
`A `Prepared` holds the executed query, the plan, the `IndexSpec`s, the constant checks and the head's length, and its `answer_header()` names the answer: a `RelationHeader` after the head predicate, with one attribute per head variable.`

In the paragraph after the bullets, replace
`` `lftj_join` / `hash_join` collect through them and return `Result<Vec<Vec<usize>>, JoinError>`;``
with
`` `lftj_join` / `hash_join` run the same steps with a flat `Tuples` sink sized by `Prepared::head_len()` and return the answer as a relation, `Result<(RelationHeader, Tuples), JoinError>`, the pair a `Catalog` is built from;``
and replace
`so it validates and applies every rewrite like every other entry point.`
with
`so it validates and applies every rewrite like every other entry point; it returns the rows alone, as one `Tuples` batch, since its inputs carry no headers.`

- [ ] **Step 3: The remaining docs**

`docs/specs/benchmarking-architecture.md`: replace `   names (`head_column_names(query)`).` with
`   names (`Prepared::answer_header`).`

`docs/algorithms/hash-triejoin.md` line 45: replace `end-to-end through `join_iter`.` with
`end-to-end through `join_for_each`.` Line 48: replace
`` `JoinAlgo::join_iter` is a provided method that collects through `join_for_each`, for callers that need the rows.``
with
`A caller that needs the rows collects them with a sink that pushes into a flat `Tuples` batch.`

`kermit-algos/README.md` line 31: replace
`` `join_iter` is a provided method that collects through it, for callers that need the rows.``
with
`A caller that needs the rows collects them with a sink that pushes into a flat `Tuples` batch.`

`kermit/README.md`: replace `[`db::lftj_join`](src/db.rs) collects it.` with
`[`db::lftj_join`](src/db.rs) collects it as a relation: the `RelationHeader` that `Prepared::answer_header` names and the rows in one flat `Tuples` batch, the pair a `Catalog` is built from.`;
replace `[`db::hash_join`](src/db.rs) collects it.` with
`[`db::hash_join`](src/db.rs) collects it the same way.`; and replace
`helper that builds a `Catalog` from raw tuple vectors and runs a join end-to-end.` with
`helper that builds a `Catalog` from raw tuple vectors and runs a join end-to-end, returning the rows as one flat `Tuples` batch.`

- [ ] **Step 4: Check no living doc names the old API**

```bash
grep -rn 'join_iter\|head_column_names\|Vec<Vec<usize>>, JoinError' --include='*.md' . | grep -v 'docs/plans/\|docs/superpowers/plans/\|docs/audits/\|CHANGELOG\|docs/specs/20\|test_leapfrog_join_iter'
```

Expected: no output.

- [ ] **Step 5: Doc and lint gates**

```bash
nix develop --command env CARGO_BUILD_JOBS=2 RUSTDOCFLAGS=-Dwarnings cargo doc --workspace --no-deps
nix develop --command env CARGO_BUILD_JOBS=2 cargo clippy --all-targets -- -D warnings
```

Expected: both `Finished`, no warnings.

- [ ] **Step 6: Commit**

```bash
git status --short
git add CLAUDE.md ARCHITECTURE.md docs/specs/benchmarking-architecture.md docs/algorithms/hash-triejoin.md kermit-algos/README.md kermit/README.md
git commit -q -F - <<'EOF'
docs: answers are relations; JoinAlgo has one method

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_012YYYQo7fUggdHrTt6wWjtk
EOF
```

---

### Task 9: Final verification

**Files:** none changed (the report goes to the user).

- [ ] **Step 1: Format check**

```bash
nix develop --command cargo fmt --all -- --check
```

Expected: no output, exit 0.

- [ ] **Step 2: The workspace tests**

```bash
nix develop --command env CARGO_BUILD_JOBS=2 cargo test --workspace
```

Expected: every binary `test result: ok.` If `e2e_watdiv` fails, rerun it alone
(`cargo test -p kermit-rdf --test e2e_watdiv`) before suspecting this change: it is a known flake
under the parallel run.

- [ ] **Step 3: Docs, binary docs and lint**

```bash
nix develop --command env CARGO_BUILD_JOBS=2 RUSTDOCFLAGS=-Dwarnings cargo doc --workspace --no-deps
nix develop --command env CARGO_BUILD_JOBS=2 cargo rustdoc -p kermit --bin kermit -- --document-private-items -D warnings
nix develop --command env CARGO_BUILD_JOBS=2 cargo clippy --all-targets -- -D warnings
```

Expected: each `Finished`, no warnings.

- [ ] **Step 4: The python job**

```bash
nix develop --command env CARGO_BUILD_JOBS=2 cargo build -p kermit
nix develop --command bash -c 'cd python/kermit-lab && uv sync --group test && KERMIT_BIN=$PWD/../../target/debug/kermit uv run pytest'
```

Expected: pytest passes, including the real-binary contract test.

- [ ] **Step 5: Build the release binary, detached**

```bash
SCRATCH=/tmp/claude-1000/-tb-Source-Academia-kermit--loom-worktrees-aidanb-flat-vec-out-18e11f06f4332998/b700993e-1014-4b73-9dd8-b36ae7a542c0/scratchpad
setsid nohup nix develop --command bash -c "CARGO_BUILD_JOBS=2 cargo build --release -p kermit && cp target/release/kermit $SCRATCH/kermit-after; echo EXIT=\$?" > "$SCRATCH/release-after.log" 2>&1 < /dev/null & disown
```

Then, as a background command, wait:

```bash
SCRATCH=/tmp/claude-1000/-tb-Source-Academia-kermit--loom-worktrees-aidanb-flat-vec-out-18e11f06f4332998/b700993e-1014-4b73-9dd8-b36ae7a542c0/scratchpad
until grep -q '^EXIT=' "$SCRATCH/release-after.log"; do sleep 15; done; tail -3 "$SCRATCH/release-after.log"
```

Expected: `EXIT=0`.

- [ ] **Step 6: Compare the binaries' code, function by function**

```bash
SCRATCH=/tmp/claude-1000/-tb-Source-Academia-kermit--loom-worktrees-aidanb-flat-vec-out-18e11f06f4332998/b700993e-1014-4b73-9dd8-b36ae7a542c0/scratchpad
systemd-run --user --scope -q -p MemoryMax=3G -p MemorySwapMax=0 "$SCRATCH/fingerprint.sh" "$SCRATCH/kermit-after" > "$SCRATCH/after.fp"
LC_ALL=C comm -3 "$SCRATCH/before.fp" "$SCRATCH/after.fp" | sed 's/^\t//' | cut -f1 | LC_ALL=C sort -u | sed -E 's/^<(.*)>:$/\1/' | c++filt | tee "$SCRATCH/changed-functions.txt"
wc -l < "$SCRATCH/changed-functions.txt"
```

Expected: a short list. It must contain `head_column_names` (removed) and `answer_header` (new), and
the `write_prepared_join` instantiations or the `main.rs` functions they inline into; their
presence shows the fingerprint detects a change. Pass: no entry is on the timed path, that is, none
names `execute`, `build_with`, `build_indexes`, `count_rows`, `join_for_each`, `enumerate`,
`emit_leaf`, a `kermit::bench::` function or a Criterion routine. A timed-path entry is recorded
with its symbol and judged as codegen placement under the ±3% bound accepted on 2026-10-04; it
does not trigger a re-measurement.

- [ ] **Step 7: Check the spec's acceptance criteria**

```bash
git diff 609fb6a --stat
git diff 609fb6a -- kermit/src/bench_report.rs kermit/src/execution.rs kermit/src/bench/ | head -5
grep -rn --include='*.rs' -- '-> .*Vec<Vec<usize>>' kermit-algos/src kermit/src
```

Expected: the stat lists only the files in "File structure" plus the plan and spec; the second
command prints nothing (no report-schema, family or runner change). Every hit of the `grep` lies
inside its file's `#[cfg(test)]` module except `kermit-algos/src/hash/hash_triejoin.rs:25`,
`build_variable_to_iter_map`, a private helper that maps variables to iterators and is not a
join's result; so no public function returns `Vec<Vec<usize>>`. (Before this change the grep also
hit `kermit/src/db.rs:93`, `:128` and `kermit/src/lib.rs:55`.)

- [ ] **Step 8: Report**

Tell the user: the commits (`git log --oneline 609fb6a..`), the gate results with any rerun, the
changed-function list from Step 6, and whether each acceptance criterion in the spec holds.
