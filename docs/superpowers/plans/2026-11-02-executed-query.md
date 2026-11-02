# The Executed Query as a Type: Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development
> (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use
> checkbox (`- [ ]`) syntax for tracking. (Executed inline, by user choice, 2026-11-02.)

**Goal:** `JoinAlgo::join_for_each` takes an `ExecutedQuery`, which only a checked constructor
makes, and its indexes as `&[DS]` in id order. The analysis and plan check leave the timed
`iteration`, and the report schema goes to v7.

**Architecture:** a new `kermit-algos/src/executed.rs` holds the type and its error. The trait
changes in one commit, together with both joins, `plan_indexes` and `kermit::db`, since the
signature spans the crates. The v7 bump then touches the Rust constant, kermit-lab and the schema
doc, and the docs follow last.

**Tech stack:** Rust nightly (inside `nix develop`), and kermit-lab (Python, `uv`).

**Spec:** `docs/specs/2026-11-02-executed-query-design.md`.

**Conventions for every step:**
- Run cargo inside `nix develop --command …`, with `CARGO_BUILD_JOBS=2`.
- Run `cargo fmt --all` before each commit.
- Commit by pathspec, never `git add -A`.
- Every commit ends with the two attribution lines from the session's system reminder.

---

## File map

| File | Change |
|---|---|
| `kermit-algos/src/executed.rs` | **new**: `ExecutedQuery`, `ExecutedQueryError`, unit tests |
| `kermit-algos/src/lib.rs` | `mod executed;` and its re-export |
| `kermit-algos/src/join_algo.rs` | new signature and contract; `assert_index_count` |
| `kermit-algos/src/sorted/leapfrog_triejoin.rs` | `join_for_each` on the type; tests migrated and added |
| `kermit-algos/src/hash/hash_triejoin.rs` | the same; the early return dropped |
| `kermit-algos/src/index_plan.rs` | `IndexPlan { query: ExecutedQuery, indexes }`; tests migrated |
| `kermit-algos/src/optimiser/plan.rs` | the dead `earlier != later` guard removed |
| `kermit-algos/src/analysis.rs` | a stale "timed" comment corrected |
| `kermit/src/db/lifecycle.rs` | `Prepared` holds the type; `execute` passes the slice |
| `kermit/src/bench_report.rs` | `REPORT_SCHEMA_VERSION = 7` |
| `python/kermit-lab/kermit_lab/__init__.py`, `loader.py` | `SCHEMA_VERSION = 7`, a v7 `MEANING_CHANGES` row |
| `python/kermit-lab/tests/test_loader.py`, `test_cli.py` | the v7 boundary tests |
| `docs/specs/bench-report-schema.md` | a v7 row and the versioning policy |
| `CLAUDE.md`, `ARCHITECTURE.md`, `docs/algorithms/{leapfrog,hash}-triejoin.md`, `kermit-algos/src/{sorted,hash}/mod.rs`, `optimiser/mod.rs` | docs |

---

### Task 1: `ExecutedQuery` (additive)

**Files:** create `kermit-algos/src/executed.rs`; modify `kermit-algos/src/lib.rs`.

- [ ] **Step 1: Write the tests first.** Create `kermit-algos/src/executed.rs` with the module doc
  and only this test module, then register it in `lib.rs` (Step 3's two lines), so the build fails
  on the missing type:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    /// The identity plan of `query`: its variables in canonical order.
    fn identity(query: &JoinQuery) -> QueryPlan {
        QueryPlan {
            variable_ordering: (0..analyse(query).num_vars).collect(),
        }
    }

    fn check(text: &str) -> Result<ExecutedQuery, ExecutedQueryError> {
        let query: JoinQuery = text.parse().expect("parse");
        let plan = identity(&query);
        ExecutedQuery::new(query, plan)
    }

    /// `Q() :- .`, which the parser cannot spell: no head term, no atom.
    fn empty_query() -> JoinQuery {
        let mut query: JoinQuery = "Q(c9) :- r(c1).".parse().expect("parse");
        query.head.terms.clear();
        query.body.clear();
        query
    }

    #[test]
    fn a_query_in_executed_form_is_accepted() {
        let executed = check("Q(X, Y, Z) :- R(X, Y), S(Y, Z), T(X, Z).").unwrap();
        assert_eq!(
            executed.to_string(),
            "Q(X, Y, Z) :- R(X, Y), S(Y, Z), T(X, Z)."
        );
        assert_eq!(executed.query().body.len(), 3);
        assert_eq!(executed.plan().variable_ordering, [0, 1, 2]);
        assert_eq!(executed.analysis().predicate_variables, [
            vec![0, 1],
            vec![1, 2],
            vec![0, 2]
        ]);
        assert_eq!(executed.atom_indexes(), [0, 1, 2]);
        assert_eq!(executed.index_count(), 3);
    }

    #[test]
    fn atoms_that_share_a_name_read_one_index() {
        let executed = check("Q(X, Y, Z) :- R(X, Y), R(Y, Z).").unwrap();
        assert_eq!(executed.atom_indexes(), [0, 0]);
        assert_eq!(executed.index_count(), 1);
    }

    #[test]
    fn indexes_are_numbered_by_first_appearance() {
        let executed = check("Q(X, Y, Z) :- S(X, Y), R(Y, Z), S(X, Z).").unwrap();
        assert_eq!(executed.atom_indexes(), [0, 1, 0]);
        assert_eq!(executed.index_count(), 2);
    }

    /// LFTJ used to answer `[[1], [3]]` over `{(1, 2), (3, 3)}`: it opened
    /// one level for the one variable and never read the second column.
    #[test]
    fn a_repeated_variable_is_rejected() {
        assert_eq!(
            check("Q(X) :- R(X, X).").unwrap_err(),
            ExecutedQueryError::RepeatedVariable {
                atom: 0,
                variable: "X".to_string(),
            }
        );
    }

    /// `validate` used to reject `R(X, Y, X)` as a column-order violation.
    #[test]
    fn a_non_adjacent_repeat_is_rejected_as_a_repeat() {
        assert_eq!(
            check("Q(X, Y) :- R(X, Y, X).").unwrap_err(),
            ExecutedQueryError::RepeatedVariable {
                atom: 0,
                variable: "X".to_string(),
            }
        );
    }

    #[test]
    fn a_constant_is_rejected() {
        assert_eq!(
            check("Q(X) :- R(X, c5).").unwrap_err(),
            ExecutedQueryError::Constant { atom: 0, column: 1 }
        );
    }

    #[test]
    fn a_placeholder_is_rejected() {
        assert_eq!(
            check("Q(X) :- S(X), R(X, _).").unwrap_err(),
            ExecutedQueryError::Placeholder { atom: 1, column: 1 }
        );
    }

    /// LFTJ used to panic on an empty leapfrog here, and Hash Triejoin to
    /// return no rows.
    #[test]
    fn an_unbound_head_variable_is_rejected() {
        assert_eq!(
            check("Q(X, Y) :- R(X).").unwrap_err(),
            ExecutedQueryError::UnboundVariable {
                variable: "Y".to_string(),
            }
        );
    }

    #[test]
    fn a_plan_the_columns_contradict_is_rejected() {
        let query: JoinQuery = "Q(X, Y) :- R(X, Y).".parse().unwrap();
        let plan = QueryPlan {
            variable_ordering: vec![1, 0],
        };
        assert_eq!(
            ExecutedQuery::new(query, plan).unwrap_err(),
            ExecutedQueryError::Plan(PlanError::ColumnOrderViolated {
                predicate: 0,
                earlier: 0,
                later: 1,
            })
        );
    }

    #[test]
    fn a_plan_that_is_not_a_permutation_is_rejected() {
        let query: JoinQuery = "Q(X, Y) :- R(X, Y).".parse().unwrap();
        let plan = QueryPlan {
            variable_ordering: vec![0],
        };
        assert_eq!(
            ExecutedQuery::new(query, plan).unwrap_err(),
            ExecutedQueryError::Plan(PlanError::NotAPermutation)
        );
    }

    #[test]
    fn the_first_violation_is_reported() {
        assert_eq!(
            check("Q(X, W) :- R(c1, X), S(X, X).").unwrap_err(),
            ExecutedQueryError::Constant { atom: 0, column: 0 }
        );
    }

    #[test]
    fn a_query_with_no_atoms_is_accepted() {
        let plan = QueryPlan {
            variable_ordering: vec![],
        };
        let executed = ExecutedQuery::new(empty_query(), plan).unwrap();
        assert!(executed.atom_indexes().is_empty());
        assert_eq!(executed.index_count(), 0);
        assert_eq!(executed.analysis().num_vars, 0);
    }

    #[test]
    fn errors_say_what_is_wrong() {
        let repeat = ExecutedQueryError::RepeatedVariable {
            atom: 0,
            variable: "X".to_string(),
        };
        assert_eq!(
            repeat.to_string(),
            "body atom 0 carries variable X more than once"
        );
        let plan = ExecutedQueryError::Plan(PlanError::NotAPermutation);
        assert!(plan.to_string().starts_with("invalid plan: "));
        assert!(std::error::Error::source(&plan).is_some());
        assert!(std::error::Error::source(&repeat).is_none());
    }
}
```

- [ ] **Step 2: Run them and see them fail.**
  `cargo test -p kermit-algos --lib executed`. Expected: the build fails because
  `ExecutedQuery` and `ExecutedQueryError` are not found.

- [ ] **Step 3: Implement.** Above the test module in `executed.rs`:

```rust
//! The executed query: the one form the joins accept.
//!
//! [`crate::plan_indexes`] rewrites a query into this form: one atom per
//! index, over distinct variables, with no constant and no `_`, every head
//! variable carried by an atom, and a plan valid for it. Both joins are
//! correct only on this form, so [`ExecutedQuery::new`] checks it once and
//! the joins take the type instead of a [`JoinQuery`]. The type also holds
//! what each join used to recompute on every call: the query's analysis and
//! its checked plan.

use {
    crate::{
        analysis::{analyse, QueryAnalysis},
        optimiser::{PlanError, QueryPlan},
    },
    kermit_parser::{JoinQuery, Term},
    std::fmt,
};

/// A query in executed form, with its analysis and a plan checked against
/// it.
///
/// Only [`new`](Self::new) makes one, so every value holds these
/// invariants:
/// - every body term is a variable, and no atom carries a variable twice;
/// - every head variable is carried by some atom;
/// - [`plan`](Self::plan) is valid for [`analysis`](Self::analysis).
///
/// The head's own shape is not checked: the joins do not read it, and
/// `kermit::db` validates it before planning. A query with no atoms is
/// valid: its one answer is the empty row.
#[derive(Clone, Debug)]
pub struct ExecutedQuery {
    query: JoinQuery,
    analysis: QueryAnalysis,
    plan: QueryPlan,
    atom_indexes: Vec<usize>,
    index_count: usize,
}

impl ExecutedQuery {
    /// Checks `query` and `plan`, and holds them together.
    ///
    /// The distinct atom names, numbered by first appearance, are the
    /// index ids: atom `j` reads index
    /// [`atom_indexes()[j]`](Self::atom_indexes). Under that numbering an
    /// atom `#k` of [`crate::plan_indexes`] reads index `k`.
    ///
    /// # Errors
    ///
    /// The first violation found, looking at each body atom column by
    /// column, then at each head variable in head order, then at the plan.
    pub fn new(query: JoinQuery, plan: QueryPlan) -> Result<Self, ExecutedQueryError> {
        for (atom, predicate) in query.body.iter().enumerate() {
            for (column, term) in predicate.terms.iter().enumerate() {
                match term {
                    | Term::Var(variable) => {
                        if predicate.terms[..column].contains(term) {
                            return Err(ExecutedQueryError::RepeatedVariable {
                                atom,
                                variable: variable.clone(),
                            });
                        }
                    },
                    | Term::Atom(_) => return Err(ExecutedQueryError::Constant { atom, column }),
                    | Term::Placeholder => {
                        return Err(ExecutedQueryError::Placeholder { atom, column });
                    },
                }
            }
        }
        for term in &query.head.terms {
            if let Term::Var(variable) = term {
                if !query.body.iter().any(|predicate| predicate.terms.contains(term)) {
                    return Err(ExecutedQueryError::UnboundVariable {
                        variable: variable.clone(),
                    });
                }
            }
        }
        let analysis = analyse(&query);
        plan.validate(&analysis).map_err(ExecutedQueryError::Plan)?;

        let mut names: Vec<&str> = Vec::new();
        let atom_indexes = query
            .body
            .iter()
            .map(|predicate| match names.iter().position(|&name| name == predicate.name) {
                | Some(id) => id,
                | None => {
                    names.push(&predicate.name);
                    names.len() - 1
                },
            })
            .collect();
        let index_count = names.len();
        Ok(ExecutedQuery {
            query,
            analysis,
            plan,
            atom_indexes,
            index_count,
        })
    }

    /// The query, as given.
    pub fn query(&self) -> &JoinQuery { &self.query }

    /// The query's analysis: its canonical variable numbering, and the
    /// variables each atom carries.
    pub fn analysis(&self) -> &QueryAnalysis { &self.analysis }

    /// The plan, valid for [`analysis`](Self::analysis).
    pub fn plan(&self) -> &QueryPlan { &self.plan }

    /// `atom_indexes()[j]`: the index body atom `j` reads.
    pub fn atom_indexes(&self) -> &[usize] { &self.atom_indexes }

    /// The number of distinct indexes the atoms read: a join is passed
    /// exactly this many.
    pub fn index_count(&self) -> usize { self.index_count }
}

impl fmt::Display for ExecutedQuery {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { fmt::Display::fmt(&self.query, f) }
}

/// Why a query is not in executed form; see [`ExecutedQuery::new`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecutedQueryError {
    /// A body atom holds a constant.
    Constant {
        /// The atom's position in the body.
        atom: usize,
        /// The constant's column.
        column: usize,
    },
    /// A body atom holds `_`.
    Placeholder {
        /// The atom's position in the body.
        atom: usize,
        /// The placeholder's column.
        column: usize,
    },
    /// A body atom carries a variable more than once.
    RepeatedVariable {
        /// The atom's position in the body.
        atom: usize,
        /// The variable's name.
        variable: String,
    },
    /// No body atom carries a head variable.
    UnboundVariable {
        /// The variable's name.
        variable: String,
    },
    /// The plan is not valid for the query.
    Plan(PlanError),
}

impl fmt::Display for ExecutedQueryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            | Self::Constant { atom, column } => write!(
                f,
                "body atom {atom} holds a constant at column {column}; the executed form has \
                 variables only"
            ),
            | Self::Placeholder { atom, column } => write!(
                f,
                "body atom {atom} holds `_` at column {column}; the executed form has named \
                 variables only"
            ),
            | Self::RepeatedVariable { atom, variable } => {
                write!(f, "body atom {atom} carries variable {variable} more than once")
            },
            | Self::UnboundVariable { variable } => {
                write!(f, "head variable {variable} is carried by no body atom")
            },
            | Self::Plan(error) => write!(f, "invalid plan: {error}"),
        }
    }
}

impl std::error::Error for ExecutedQueryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            | Self::Plan(error) => Some(error),
            | _ => None,
        }
    }
}
```

  In `kermit-algos/src/lib.rs`, add `mod executed;` after `mod analysis;`, and add
  `executed::{ExecutedQuery, ExecutedQueryError},` to the `pub use` list after the `analysis::`
  line.

- [ ] **Step 4: Run them and see them pass.** `cargo test -p kermit-algos --lib executed`: all 13
  pass. Then
  `RUSTFLAGS=-Dwarnings cargo clippy -p kermit-algos --all-targets` (clean) and
  `RUSTDOCFLAGS=-Dwarnings cargo doc -p kermit-algos --no-deps` (clean).

- [ ] **Step 5: Commit** `kermit-algos/src/executed.rs` and `kermit-algos/src/lib.rs`:
  `feat(algos): ExecutedQuery, the checked form a join accepts`.

---

### Task 2: the switch (one commit)

**Files:** `kermit-algos/src/{join_algo,index_plan,analysis}.rs`, `optimiser/plan.rs`,
`sorted/leapfrog_triejoin.rs`, `hash/hash_triejoin.rs`, and `kermit/src/db/lifecycle.rs`.

- [ ] **Step 1: Write the new join tests first.**
  - In `leapfrog_triejoin.rs`'s test module, import `crate::ExecutedQuery` and add:

```rust
    /// The empty conjunction: its one answer is the empty row.
    #[test]
    fn join_for_each_emits_one_empty_row_for_an_empty_body() {
        let mut query: JoinQuery = "Q(c1) :- R(c1).".parse().unwrap();
        query.head.terms.clear();
        query.body.clear();
        let plan = QueryPlan {
            variable_ordering: vec![],
        };
        let executed = ExecutedQuery::new(query, plan).unwrap();
        let none: [TreeTrie; 0] = [];
        let mut rows: Vec<Vec<usize>> = Vec::new();
        LeapfrogTriejoin::join_for_each(&executed, &none, |row| rows.push(row.to_vec()));
        assert_eq!(rows, vec![Vec::<usize>::new()]);
    }

    #[test]
    #[should_panic(expected = "LeapfrogTriejoin::join_for_each: 1 indexes for a query that reads 2")]
    fn join_for_each_panics_when_an_index_is_missing() {
        let r: TreeTrie = TreeTrie::from_tuples(2.into(), vec![vec![1, 2]]);
        let query: JoinQuery = "Q(X, Y, Z) :- R(X, Y), S(Y, Z).".parse().unwrap();
        let plan = QueryPlan {
            variable_ordering: vec![0, 1, 2],
        };
        let executed = ExecutedQuery::new(query, plan).unwrap();
        LeapfrogTriejoin::join_for_each(&executed, &[r], |_| {});
    }
```

  - In `hash_triejoin.rs`'s test module, import `crate::ExecutedQuery` and add the same two tests,
    renamed for Hash Triejoin, with `HashTrie` in place of `TreeTrie`. Use
    `use kermit_ds::{HashTrie, Relation};`, `let none: [HashTrie; 0] = [];`, and the expected
    message `"HashTriejoin::join_for_each: 1 indexes for a query that reads 2"`.
  - Migrate LFTJ's `join_for_each_emits_head_order_under_a_reordering_plan`: build
    `ExecutedQuery::new(query, plan).unwrap()`, call
    `LeapfrogTriejoin::join_for_each(&executed, &[r], |tuple| rows.push(tuple.to_vec()))`, and
    drop the `HashMap` import.
  - Migrate Hash Triejoin's tests:
    - The helper becomes
      `fn joined<DS: HashTrieIterable>(query: &ExecutedQuery, indexes: &[DS]) -> Vec<Vec<usize>>`,
      collecting into `Tuples::new(query.analysis().num_vars)`.
    - Delete `by_name`.
    - Each test builds `let executed = ExecutedQuery::new(query, plan).unwrap();` and passes its
      relations in first-appearance order: `joined(&executed, &[r, s])` and `&[r, s, t]`.
    - `payload_is_skipped_…` builds `let keys = [kr, ks, kt]; let all = [ar, as_, at];` before its
      loop and passes `&keys` / `&all` inside it.
    - `atoms_with_no_level_…` builds `r` by a closure
      `let r = || keys_trie(1, 0, vec![vec![1], vec![2]]);` and passes `&[r(), s]` and
      `&[r(), empty]`.
    - `emit_leaf_resets_each_cursor_on_carry` passes `&[r, s, t]` after its `Probe` is dropped.
    - Delete `a_variable_no_atom_carries_gives_no_rows`. Its case is
      `executed.rs`'s `an_unbound_head_variable_is_rejected`.

- [ ] **Step 2: Run and see the new tests fail.** `cargo test -p kermit-algos --lib` fails to
  compile: `join_for_each` still takes a plan, a `JoinQuery` and a map.

- [ ] **Step 3: Change the trait.** `kermit-algos/src/join_algo.rs` becomes:

```rust
//! This module defines the `JoinAlgo` trait, used as a base for join
//! algorithms.

use {crate::executed::ExecutedQuery, kermit_iters::JoinIterable};

/// The `JoinAlgo` trait is used as a base for join algorithms.
pub trait JoinAlgo<DS>
where
    DS: JoinIterable,
{
    /// Joins `indexes` as `query` says, passing each result tuple to `emit`
    /// in canonical (head-first) variable order.
    ///
    /// `indexes[id]` is the index [`ExecutedQuery::atom_indexes`] names. The
    /// query is in executed form, with its plan already checked against it
    /// (see [`ExecutedQuery`]), so an implementation reads the analysis and
    /// the plan from it and checks neither. A query with no atoms has one
    /// answer, the empty row, which `emit` receives once.
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
    /// `query.analysis().num_vars`: one column per variable of `query`, in
    /// canonical order.
    ///
    /// # Panics
    ///
    /// Implementations panic if `indexes.len() != query.index_count()`: the
    /// one input the type cannot check, and always a caller programming
    /// error (see [`assert_index_count`]).
    fn join_for_each<S: FnMut(&[usize])>(query: &ExecutedQuery, indexes: &[DS], emit: S);
}

/// The `# Panics` of [`JoinAlgo::join_for_each`], shared by the joins:
/// panics unless `indexes` is one index per id of `query`.
pub(crate) fn assert_index_count(algorithm: &str, query: &ExecutedQuery, indexes: usize) {
    assert!(
        indexes == query.index_count(),
        "{algorithm}::join_for_each: {indexes} indexes for a query that reads {}",
        query.index_count()
    );
}
```

- [ ] **Step 4: LeapfrogTriejoin.** Its `use` block keeps only what non-test code needs:
  `crate::{executed::ExecutedQuery, join_algo::{assert_index_count, JoinAlgo},
  sorted::leapfrog_join::LeapfrogJoinIter}` and the existing `kermit_iters` line. Drop `analyse`,
  `QueryPlan`, `JoinQuery` and `HashMap`; the test module imports its own. `join_for_each`
  becomes:

```rust
    fn join_for_each<S: FnMut(&[usize])>(query: &ExecutedQuery, indexes: &[DS], mut emit: S) {
        assert_index_count("LeapfrogTriejoin", query, indexes.len());
        if query.atom_indexes().is_empty() {
            // The empty conjunction: one answer, the empty row.
            emit(&[]);
            return;
        }
        let trie_iters: Vec<_> = query
            .atom_indexes()
            .iter()
            .map(|&id| indexes[id].trie_iter())
            .collect();
        let order = &query.plan().variable_ordering;
        let mut descents =
            LeapfrogTriejoinIter::new(order, &query.analysis().predicate_variables, trie_iters)
                .into_iter();

        // The triejoin yields each tuple in descent order: position `pos`
        // binds variable `order[pos]`. Scatter it into one scratch row in
        // canonical (head-first) order, so no result tuple is ever allocated
        // (issue #65).
        let mut tuple = vec![0usize; order.len()];
        while let Some(descent_tuple) = descents.advance() {
            for (&v, &key) in order.iter().zip(descent_tuple) {
                tuple[v] = key;
            }
            emit(&tuple);
        }
    }
```

- [ ] **Step 5: Hash Triejoin.** The non-test `use` block becomes
  `crate::{executed::ExecutedQuery, join_algo::{assert_index_count, JoinAlgo}}`, plus the
  `kermit_iters` line, without `analyse`, `QueryPlan`, `JoinQuery` or `HashMap`. Also check
  whether `QueryPlan` or `HashMap` is still used elsewhere in the file. `join_for_each` becomes:

```rust
    fn join_for_each<S: FnMut(&[usize])>(query: &ExecutedQuery, indexes: &[DS], emit: S) {
        assert_index_count("HashTriejoin", query, indexes.len());
        let predicate_variables = &query.analysis().predicate_variables;
        let variable_ordering = &query.plan().variable_ordering;
        let relations: Vec<&DS> = query
            .atom_indexes()
            .iter()
            .map(|&id| &indexes[id])
            .collect();
        // Each relation participates only at its levels' variables (#116).
        let levels: Vec<usize> = relations.iter().map(|relation| relation.levels()).collect();
        let variable_to_iter_map =
            build_variable_to_iter_map(variable_ordering, predicate_variables, &levels);
        let iters = relations
            .iter()
            .map(|relation| relation.hash_trie_iter())
            .collect();
        Probe::new(iters, predicate_variables, &variable_to_iter_map, emit).enumerate(0);
    }
```

  In `verify_and_construct`, the comment "Every variable is carried by some relation
  (`join_for_each` returns before any descent otherwise)" becomes "Every variable is carried by
  some relation (an [`ExecutedQuery`] holds no other)". If the empty-body test fails, add LFTJ's
  empty-body branch (the `emit(&[])` lines; `emit` must then be `mut`) right after the assert.

- [ ] **Step 6: `index_plan.rs`.**
  - The module doc's "so atom and index agree by construction and [`QueryPlan::validate`]
    passes." becomes "so atom and index agree by construction, and returns it as an
    [`ExecutedQuery`], whose constructor checks the plan."
  - Import `crate::executed::ExecutedQuery`.
  - `IndexPlan` becomes:

```rust
/// A planned query, ready to build and execute.
#[derive(Clone, Debug)]
pub struct IndexPlan {
    /// The query to execute, with its plan: the head unchanged, and
    /// `body[j]` the atom `#{id}` of the index the `j`-th access reads, over
    /// its variables in plan order, so atom `#k` reads index `k`.
    pub query: ExecutedQuery,
    /// The indexes to build, `indexes[id]`, with identical specs merged.
    pub indexes: Vec<IndexSpec>,
}
```

  - The end of `plan_indexes`, from `let plan = QueryPlan {`, becomes:

```rust
    let plan = QueryPlan {
        variable_ordering: plan_names.iter().map(|name| index_of[name]).collect(),
    };
    let query =
        ExecutedQuery::new(query, plan).expect("a rewritten, validated query is in executed form");
    debug_assert!(
        query
            .query()
            .body
            .iter()
            .zip(query.atom_indexes())
            .all(|(atom, &id)| atom.name == format!("{INDEX_ID_PREFIX}{id}")),
        "atom `#k` reads index k"
    );

    IndexPlan { query, indexes }
```

  - Migrate the tests by these substitutions, then drop any import the compiler reports unused:
    - `body_text(&out.query)` → `body_text(out.query.query())`
    - `out.query.head` → `out.query.query().head`
    - `out.query.body` → `out.query.query().body`
    - `out.plan.variable_ordering` → `out.query.plan().variable_ordering`
    - `out.plan.validate(&analyse(&out.query))` → `out.query.plan().validate(out.query.analysis())`
    - `old.validate(&analyse(&out.query))` → `old.validate(out.query.analysis())`
    - `canonical_names(&out.query)` → `canonical_names(out.query.query())`
    - the multi-line `out\n.plan\n.variable_ordering` → `out.query.plan().variable_ordering`

- [ ] **Step 7: the dead guard and a stale comment.**
  - In `optimiser/plan.rs`, `if earlier != later && position[earlier] > position[later] {`
    becomes `if position[earlier] > position[later] {`.
  - In `analysis.rs`'s pass-4 comment, delete the clause "`analyse` also runs inside every
    executor's `join_for_each`, which the `iteration` metric times." The comment then reads "It
    allocates only its result and copies no list."

- [ ] **Step 8: `kermit::db` (`kermit/src/db/lifecycle.rs`).**
  - Imports: add `ExecutedQuery`; drop `JoinQuery`, `QueryPlan` and `HashMap` if the compiler
    reports them unused.
  - `Prepared`'s fields `query: JoinQuery, plan: QueryPlan` become `executed: ExecutedQuery`.
  - Accessors:
    - `query()` returns `self.executed.query()`;
    - `plan()` returns `self.executed.plan()`;
    - add `/// The executed query, with its analysis and plan.` and
      `pub fn executed(&self) -> &ExecutedQuery { &self.executed }`;
    - `variable_order` reads `canonical_names(self.executed.query())` and
      `self.executed.plan().variable_ordering`;
    - `answer_header` reads `&self.executed.query().head`.
  - `prepare`: `let IndexPlan { query: executed, indexes } = plan_indexes(…);`, then
    `Ok(Prepared { executed, indexes, index_rows, checks, head_len, levels })`.
  - In `execute`'s doc, "When every atom was a check there is nothing to join, and the empty head
    row is emitted `multiplicity` times" becomes "When every atom was a check the executed query
    has no atom, and the join's one answer, the empty row, is emitted `multiplicity` times". Its
    body after the `multiplicity == 0` return becomes:

```rust
    // The executors emit every variable of the query they run in canonical
    // order, and `analyse` numbers the head's variables `0..head_len` in
    // head order, so the head is each row's prefix. Lending it allocates
    // nothing.
    let head_len = prepared.head_len;
    JA::join_for_each(&prepared.executed, &indexes.relations, |row| {
        let head = &row[..head_len];
        for _ in 0..multiplicity {
            emit(head);
        }
    });
```

- [ ] **Step 9: Run and see everything pass.**
  - `cargo test -p kermit-algos`: all pass, including the four new join tests. If Hash Triejoin's
    empty-body test failed, apply Step 5's branch.
  - `cargo test -p kermit --lib db::`, `--test join_tests`, `--test all_orders`,
    `--test result_allocation`, `--test lubm_mini_oracle` and `--test cli_query_errors`: all pass.
  - `RUSTFLAGS=-Dwarnings cargo clippy --workspace --all-targets`: clean.
  - `RUSTDOCFLAGS=-Dwarnings cargo doc --workspace --no-deps`: clean.

- [ ] **Step 10: Commit** all the Task 2 files:
  `refactor(algos)!: joins take an ExecutedQuery and their indexes by id`.

---

### Task 3: report schema v7

**Files:** `kermit/src/bench_report.rs`; `python/kermit-lab/kermit_lab/{__init__,loader}.py`;
`python/kermit-lab/tests/{test_loader,test_cli}.py`; `docs/specs/bench-report-schema.md`.

- [ ] **Step 1: Write the tests first.** In `test_loader.py`, after the flat-tuples boundary test:

```python
def test_refuses_to_mix_reports_across_the_executed_query_boundary(tmp_path: Path) -> None:
    old = _versioned_report(tmp_path / "v6.json", 6)
    new = _versioned_report(tmp_path / "v7.json", 7)
    with pytest.raises(SchemaError, match="refusing to mix schema_version 6"):
        load_reports([old, new])
    with pytest.raises(SchemaError, match="per-query setup"):
        load_reports([new, old])
    with pytest.raises(SchemaError, match="to compare build, space and bench ds metrics only"):
        load_reports([old, new])
    reports = load_reports([old, new], allow_mixed_schema=True)
    assert [r.schema_version for r in reports] == [6, 7]
```

  In `test_single_sided_loads_are_unaffected`, `(2, 3, 4, 5)` becomes `(2, 3, 4, 5, 6, 7)`. In
  `test_cli.py`'s exit-code-4 parametrisation, add `(6, 7)`.

- [ ] **Step 2: Run and see them fail.**
  `cd python/kermit-lab && uv run pytest tests/test_loader.py tests/test_cli.py -q`. Expected:
  the v7 report is refused as "> supported 6", and the new boundary test fails.

- [ ] **Step 3: Implement.**
  - `kermit_lab/__init__.py`: `SCHEMA_VERSION = 7`.
  - `loader.py`, after `VALUES_SCHEMA`:

```python
EXECUTED_QUERY_SCHEMA = 7
"""First schema version whose ``iteration`` and ``end_to_end`` exclude the
join's per-query setup: the query reaches the join as an ``ExecutedQuery``
built once, untimed, by ``prepare``, so no timed sample clones the query,
analyses it, checks its plan or maps its index names."""
```

    and, as the last row of `MEANING_CHANGES`:

```python
    (
        EXECUTED_QUERY_SCHEMA,
        "iteration and end_to_end exclude the join's per-query setup (query clone, "
        "analysis, plan check and index map)",
        ("build", "space", "bench ds metrics"),
    ),
```

  - `kermit/src/bench_report.rs`: `pub const REPORT_SCHEMA_VERSION: u32 = 7;`. Extend its doc
    comment if it lists the versions.
  - `docs/specs/bench-report-schema.md`:
    - The versioning bullet's "v3, v4 and v5 are such bumps." becomes "v3, v4, v5 and v7 are such
      bumps."
    - The refusal sentence gains ", or of version 7", and the reason list gains ", and at 7
      `iteration` and `end_to_end` did (they exclude the join's per-query setup)".
    - Append the change-log row: `| 7 | 2026-11-02 | What \`iteration\` and \`end_to_end\` measure
      changed, so they are not comparable with v6, and kermit-lab refuses to load v6 and v7
      reports together unless \`allow_mixed_schema=True\`. The query reaches the join as an
      \`ExecutedQuery\` (\`kermit-algos/src/executed.rs\`), built once and untimed by \`prepare\`:
      no timed sample clones the query, runs \`analyse\` or \`QueryPlan::validate\`, builds the
      name-to-index map or parses \`#k\` ids, about 0.5 µs per join on a 3-atom triangle and
      1.5 µs on an 8-atom chain (release, measured 2026-11-02). A query of constant checks now
      runs through the join, whose one answer is the empty row. \`build\`, \`space\` and every
      \`bench ds\` metric keep their meaning. No field or axis changed. |`

- [ ] **Step 4: Run and see them pass.**
  - `cargo build -p kermit`, then
    `cd python/kermit-lab && KERMIT_BIN=$(git rev-parse --show-toplevel)/target/debug/kermit uv run pytest -q`:
    all pass, including the real-binary contract test.
  - `cargo test -p kermit --lib bench_report`: the version-pin test passes.

- [ ] **Step 5: Commit** the five files: `feat(report)!: schema v7, iteration excludes the join's
  per-query setup`.

---

### Task 4: documentation

**Files:** `CLAUDE.md`, `ARCHITECTURE.md`, `docs/algorithms/leapfrog-triejoin.md`,
`docs/algorithms/hash-triejoin.md`, `kermit-algos/src/sorted/mod.rs`,
`kermit-algos/src/hash/mod.rs`, `kermit-algos/src/optimiser/mod.rs`.

- [ ] **Step 1: Edit each passage.**
  - `ARCHITECTURE.md`:
    - The `JoinAlgo` code block becomes the new signature.
    - Its following sentence becomes: "`query` is an `ExecutedQuery` (see
      `kermit-algos/src/executed.rs`): the executed form, its analysis and a plan checked when it
      was built, so implementations validate nothing. `indexes[id]` is the index atom `j` names
      through `query.atom_indexes()[j]`, and a slice of the wrong length panics."
    - The `execute` bullet's "maps ids to indexes, calls `JoinAlgo::join_for_each`" becomes
      "passes its indexes, in id order, to `JoinAlgo::join_for_each`".
    - The recipe's "The implementation runs over the executed query (…) and must validate the
      incoming `QueryPlan`." becomes "It receives an `ExecutedQuery`, whose type guarantees the
      executed form (every atom an index over distinct variables in plan order, no constants or
      `_`, every head variable carried) and a checked plan, and its indexes as `&[DS]` in id
      order; it validates nothing, calls `assert_index_count`, and emits one empty row for an
      empty body."
  - `leapfrog-triejoin.md`'s GAO bullet: "`join_for_each` checks the plan with
    [`QueryPlan::validate`](…) and panics on an invalid one." becomes "The plan arrives inside
    the [`ExecutedQuery`](../../kermit-algos/src/executed.rs), checked against the query when that
    was built, so `join_for_each` checks nothing."
  - `hash-triejoin.md`'s "Variable ordering arrives as a plan" bullet: "receives a `QueryPlan`
    produced ahead of time by a QueryOptimiser, validates it with QueryPlan::validate, and panics
    on an invalid plan." becomes "receives the plan inside an
    [`ExecutedQuery`](../../kermit-algos/src/executed.rs), produced ahead of time by a
    QueryOptimiser and checked against the query when the type was built."
  - `sorted/mod.rs` and `hash/mod.rs`: wherever the family contract says what an algorithm reads,
    add "It receives the query as an [`crate::ExecutedQuery`], so no constant, `_` or repeated
    variable reaches it." If `hash/mod.rs` has no such sentence, add it after its first
    paragraph.
  - `optimiser/mod.rs`'s module doc, at the `JoinAlgo::join_for_each` mention: the plan reaches
    the join inside an `ExecutedQuery`.
  - `CLAUDE.md`:
    - In the `QueryOptimiser` bullet, "`QueryPlan` is the only type crossing planner → executor"
      becomes "`QueryPlan` crosses planner → executor inside an `ExecutedQuery`
      (`kermit-algos/src/executed.rs`), the only form a join accepts".
    - Add to the `JoinAlgo<DS>` bullet: "; `join_for_each(&ExecutedQuery, &[DS], emit)`".
    - Recipe step 2: replace "The algorithm runs over the executed query that `prepare` builds:
      … and it must validate the incoming `QueryPlan`. It may rely on what `kermit::db`
      validation, the placeholder rewrite and access extraction guarantee: … each atom's columns
      follow the plan." with "It receives an `ExecutedQuery` (`kermit-algos/src/executed.rs`) and
      its indexes as `&[DS]` in id order (atom `j` reads `indexes[query.atom_indexes()[j]]`). The
      type guarantees one distinct variable per column, no constant or `_`, every head variable
      carried by an atom, and a plan already checked against the query. So read
      `query.analysis()` and `query.plan()`, validate nothing, call
      `join_algo::assert_index_count`, and emit one empty row for an empty body."
    - The "Load, prepare, build, execute" gotcha: "planning excluded" becomes "planning and the
      join's per-query setup excluded (v7: the `ExecutedQuery` is built at `prepare`)".
    - The JSON-reports gotcha:
      - "(currently `6`)" becomes "(currently `7`)";
      - "(v3, v4 and v5 are such bumps; v6 is a structural bump …)" gains "; v7 is a meaning bump:
        `iteration` and `end_to_end` exclude the join's per-query setup";
      - the refusal sentence gains ", or of v7";
      - "v5 and v6 reports load together" stays.

- [ ] **Step 2: Verify.** `RUSTDOCFLAGS=-Dwarnings cargo doc --workspace --no-deps` and
  `RUSTFLAGS=-Dwarnings cargo clippy --workspace --all-targets`: both clean.
  `grep -rn "must validate the incoming\|validates it with\|only type crossing" CLAUDE.md ARCHITECTURE.md docs/algorithms`
  prints nothing.

- [ ] **Step 3: Commit** the seven files: `docs: the executed query in the join contract`.

---

### Task 5: the gate

- [ ] Run each of these:
  - `cargo fmt --all --check`.
  - `cargo test --workspace` inside `nix develop`. Run it detached
    (`setsid nohup … & disown`, since it takes about 5 min) and confirm that
    `lubm_one_university_query_cardinalities_match_paper … ok` ran, not skipped.
  - The Task 4 clippy and doc commands.
  - kermit-lab's pytest with `KERMIT_BIN`.
- [ ] Check against the spec's acceptance criteria:
  - none of the four shapes can reach a join: `executed.rs`'s tests;
  - `execute` holds no clone or map: read its body;
  - the gate is green;
  - kermit-lab refuses a mixed v6/v7 load: Task 3's test.
