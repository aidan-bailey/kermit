# Flat Tuple Batches (#111) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Every relation's tuples travel from the readers to every build as one row-major `Tuples` buffer, and HashTrie keeps that buffer with row-id chains read through a `LeafRows` view, with no per-tuple heap allocation left on the input path.

**Architecture:**
- `Tuples`, `RowId` and `LeafRows` live in `kermit-iters`, the crate every layer already depends on.
- Constructors take `impl Into<Tuples>`, so callers and fixtures keep compiling while each structure is moved onto rows behind a temporary `into_vecs` shim:
  - TreeTrie (T5) and ColumnTrie (T6) sort the buffer with one shared `Tuples::sort` and build from row slices;
  - HashTrie (T7) owns the buffer, holding `RowId`s in chains, singletons and pending lists, and its iterator family returns `LeafRows`.

  T8 then deletes the shim.
- The report schema goes to v4 (T10), because `insertion` and `space` measure a different representation.

**Tech Stack:** Rust nightly (workspace `rust-toolchain.toml`), the nix dev shell for rustfmt, `allocation_counter` for the allocation tests, Python kermit-lab (uv) for the schema pin.

**Spec:** `docs/specs/2026-10-08-flat-tuple-batch-design.md` (47950f0, plus the three amendments of 2026-10-08 recorded in its Status line: `HashTrie::for_each_tuple` stays a trie walk, ColumnTrie's `previous` is a borrowed row, and Part A of the measurement also times HashTrie's `iteration`). Read it first; this plan implements it task by task, and every decision it records is settled.

---

## Ground rules for the executor

- **Paths.** `WT=/tb/Source/Academia/kermit/.loom/worktrees/aidanb/hash-trie-paper-parity_18dc25f8653dd5c4`. Every command runs from `$WT` (the commit steps that write `git -C $WT …` and those that write plain `git …` mean the same tree). `SCRATCH` is the executing session's scratchpad.
- **Branch:** work on the loom branch `aidanb/hash-trie-paper-parity`. Never switch branches, never amend, never push.
- **Before Task 1:** the spec's 2026-10-08 amendments are uncommitted in the worktree, and this plan is new. Unless the user has already committed them, commit both first, by name (`git add docs/specs/2026-10-08-flat-tuple-batch-design.md docs/superpowers/plans/2026-10-08-flat-tuple-batch.md`; message `docs: plan #111, amend its spec`, with the two trailer lines below).
- **Host:** before any compile or test run, check that no timing run is active: `pgrep -af "[k]ermit-bench-runs/.*/bin/kermit"` must print nothing. If one is running, wait; a compile under a timing run contaminates it.
- **Builds:** run cargo in the foreground with `CARGO_BUILD_JOBS=2`. Long runs (the full workspace test) go through `setsid nohup … & disown`, with a `kill -0` watcher.
- **Formatting:** `nix develop --command cargo fmt --all` only. Stable rustfmt rewrites ~30 files.
- **Doc comments:** backtick every code identifier (clippy `doc_markdown` runs under `-Dwarnings`).
- **Commits.** Conventional commits referencing `(#111)`. Stage files by name. Every message ends with these two lines:
  ```
  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_015P4eTR34w48GpD7DvPPaee
  ```
- **Mutation checks** (the tasks name each mutant):
  1. Commit first.
  2. Apply the mutant as an exact edit.
  3. Confirm `git diff` shows it applied, and that the named test fails.
  4. Reverse the exact edit, never with `git checkout`.
  5. Re-run the test to green; `git status` must be clean.
- **Never write under `~/.cache/kermit`.**
- **Green tree:** every task leaves the workspace compiling with all tests passing. T3 to T7 rely on that: callers keep passing `Vec<Vec<usize>>`, which converts.

## File map

| File | Responsibility | Tasks |
| --- | --- | --- |
| `kermit-iters/src/tuples.rs` (new) | `RowId`, `Tuples`, `Tuples::sort`; the `into_vecs` shim and its removal | T1, T8 |
| `kermit-iters/src/leaf_rows.rs` (new) | `LeafRows` | T2 |
| `kermit-iters/src/hash_trie.rs` | `HashTrieIterator::leaf_tuples -> Option<LeafRows<'_>>` | T7 |
| `kermit-ds/src/relation.rs` | Relation-family signatures; `read_csv` / `read_parquet`; `project_via_trie_iter` | T3, T4, T6, T7 (a test helper) |
| `kermit-ds/src/{configured,built_with}.rs` | forwarding wrappers | T3, T7 (tests) |
| `kermit-ds/src/morsel.rs` | `scatter_rows`, `RowPartition`, `row_id` (old `scatter` deleted) | T5, T7 |
| `kermit-ds/src/ds/tree_trie/*` | sort + row-slice build, id scatter | T3, T5 |
| `kermit-ds/src/ds/column_trie/*` | sort + row-slice build | T3, T6 |
| `kermit-ds/src/ds/hash_trie/*` | owned buffer, `RowId` chains, every build mode, iterator | T3, T7 |
| `kermit-algos/src/orient.rs` | `permute_all(&Tuples) -> Tuples` | T4 |
| `kermit-algos/src/hash/*` | leaf views over `LeafRows`, `emit_leaf` | T7 |
| `kermit/src/{execution,db,main,lib}.rs`, `kermit/src/db/database.rs`, `kermit/src/bench/{ds,run}.rs` | plumbing to `Tuples` | T4 |
| `kermit/src/bench_report.rs`, `python/kermit-lab/kermit_lab/{__init__,loader}.py`, `docs/specs/bench-report-schema.md`, `docs/specs/optimization-standard.md`, CLAUDE.md's JSON-report gotcha | schema v4 | T10 |
| `kermit/tests/tuple_allocation.rs` (new), `kermit/src/execution.rs` tests | the input path allocates O(1) per relation | T9 |
| docs (ARCHITECTURE.md, CLAUDE.md, component docs, BENCHMARKING.md) | | T11 |

### Task 1: `Tuples` and `RowId` in kermit-iters

Additive: nothing uses the type yet. It is the batch every later task builds from, so the
behaviours T3, T5, T6 and T7 rely on are each pinned by a test here:
- `rows()` yields `len` empty slices at arity 0, `sort()` is a no-op there, and `push(&[])` works there;
- an empty `From<Vec<Vec<usize>>>` is an empty batch of arity 0 (T3's constructors accept it under any header);
- the row count never exceeds `RowId::MAX` (`push` and `from_flat` refuse);
- `sort()` orders rows exactly as the hand-rolled comparator TreeTrie and ColumnTrie use today.

**Files:**
- Create: `kermit-iters/src/tuples.rs`
- Modify: `kermit-iters/src/lib.rs` (crate doc after line 10; `mod` list lines 37–43; `pub use` block lines 45–53)
- Modify: `kermit-ds/src/lib.rs` (`pub use` block, lines 38–58)
- Test: `kermit-iters/src/tuples.rs` (`mod tests`)

- [ ] **Step 1: Write the failing tests.** Create `kermit-iters/src/tuples.rs` with the module doc and
  the test module only:

```rust
//! [`Tuples`]: a relation's tuples as one row-major buffer, and [`RowId`],
//! a row's index in it.
//!
//! Every relation is built from a `Tuples` batch (#111): a tuple is a slice
//! of one buffer, so `n` tuples cost one allocation, not `n`. The paper's
//! build starts from the same thing: "the incoming tuples are materialized
//! contiguously in an in-memory buffer prior to running Algorithm 2"
//! (Freitag et al., VLDB 2020, §3.3.2).

#[cfg(test)]
mod tests {
    use super::*;

    /// A 64-bit LCG (Knuth's MMIX constants), so the sort test is seeded
    /// without a `rand` dependency.
    struct Lcg(u64);

    impl Lcg {
        fn below(&mut self, n: usize) -> usize {
            self.0 = self
                .0
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            (self.0 >> 33) as usize % n
        }
    }

    /// The hand-rolled comparator `TreeTrie::from_tuples` and ColumnTrie's
    /// build sorted `Vec<Vec<usize>>` with before #111, copied verbatim.
    fn sort_like_the_sorted_tries_did(mut tuples: Vec<Vec<usize>>) -> Vec<Vec<usize>> {
        tuples.sort_unstable_by(|a, b| {
            for i in 0..a.len() {
                match a[i].cmp(&b[i]) {
                    | std::cmp::Ordering::Less => return std::cmp::Ordering::Less,
                    | std::cmp::Ordering::Greater => return std::cmp::Ordering::Greater,
                    | std::cmp::Ordering::Equal => continue,
                }
            }
            std::cmp::Ordering::Equal
        });
        tuples
    }

    /// `n` rows of `arity` values below `domain`: a small domain repeats
    /// prefixes and whole rows.
    fn random_rows(lcg: &mut Lcg, n: usize, arity: usize, domain: usize) -> Vec<Vec<usize>> {
        (0..n)
            .map(|_| (0..arity).map(|_| lcg.below(domain)).collect())
            .collect()
    }

    /// A nullary batch keeps its row count: its buffer is empty, its rows
    /// are empty slices, and sorting it changes nothing.
    #[test]
    fn nullary_rows_keep_their_count() {
        let mut tuples = Tuples::from(vec![vec![], vec![], vec![]]);
        assert_eq!((tuples.arity(), tuples.len()), (0, 3));
        assert!(!tuples.is_empty());
        assert!(tuples.as_flat().is_empty());
        assert_eq!(tuples.heap_size_bytes(), 0);
        assert_eq!(tuples.rows().len(), 3);
        assert!(tuples.rows().all(<[usize]>::is_empty));
        assert!(tuples.row(2).is_empty());
        tuples.push(&[]);
        tuples.sort();
        assert_eq!(tuples.len(), 4);
        assert_eq!(tuples.to_vecs(), vec![Vec::<usize>::new(); 4]);
    }

    /// An empty `Vec` has no first row to take an arity from: it is the
    /// empty batch of arity 0.
    #[test]
    fn an_empty_vec_is_an_empty_batch() {
        let tuples = Tuples::from(Vec::<Vec<usize>>::new());
        assert_eq!((tuples.arity(), tuples.len()), (0, 0));
        assert!(tuples.is_empty());
        assert_eq!(tuples.rows().len(), 0);
        assert_eq!(tuples, Tuples::default());
        assert_eq!(tuples.into_vecs(), Vec::<Vec<usize>>::new());
    }

    #[test]
    #[should_panic(expected = "in a batch of arity 2")]
    fn mixed_arity_vectors_panic() { let _ = Tuples::from(vec![vec![1, 2], vec![3]]); }

    #[test]
    #[should_panic(expected = "a row of arity 3 in a batch of arity 2")]
    fn a_push_of_the_wrong_arity_panics() { Tuples::new(2).push(&[1, 2, 3]); }

    #[test]
    #[should_panic(expected = "row 2 out of range for 2 rows")]
    fn a_row_past_the_end_panics() {
        let _ = Tuples::from(vec![vec![1, 2], vec![3, 4]]).row(2);
    }

    /// At arity 0 every row is the empty slice, which slicing would lend
    /// for any id: only the explicit check stops a read past the end.
    #[test]
    #[should_panic(expected = "row 2 out of range for 2 rows")]
    fn a_nullary_row_past_the_end_panics() { let _ = Tuples::from(vec![vec![], vec![]]).row(2); }

    #[test]
    fn rows_lend_every_row_in_order() {
        let tuples = Tuples::from(vec![vec![1, 2], vec![3, 4], vec![5, 6]]);
        assert_eq!((tuples.arity(), tuples.len()), (2, 3));
        let rows: Vec<Vec<usize>> = tuples.rows().map(<[usize]>::to_vec).collect();
        assert_eq!(rows, vec![vec![1, 2], vec![3, 4], vec![5, 6]]);
        assert_eq!(tuples.rows().len(), 3);
        assert_eq!(tuples.row(1), [3, 4]);
        assert_eq!(tuples.as_flat(), [1, 2, 3, 4, 5, 6]);
        // `From` reserves exactly the rows it copies.
        assert_eq!(tuples.heap_size_bytes(), 6 * std::mem::size_of::<usize>());
    }

    /// `sort` orders rows as the old comparator did: through arrays at
    /// arities 1–4, through a row-id permutation at 5. Empty batches keep
    /// their arity, and a small domain repeats prefixes and whole rows.
    #[test]
    fn sort_orders_rows_as_the_old_comparator_did() {
        let mut lcg = Lcg(0x111);
        for arity in 1..=5 {
            for n in [0, 1, 2, 17, 300] {
                for domain in [2, 5, 1000] {
                    let rows = random_rows(&mut lcg, n, arity, domain);
                    let mut tuples = Tuples::new(arity);
                    for row in &rows {
                        tuples.push(row);
                    }
                    tuples.sort();
                    assert_eq!((tuples.arity(), tuples.len()), (arity, n));
                    assert_eq!(
                        tuples.to_vecs(),
                        sort_like_the_sorted_tries_did(rows),
                        "arity {arity}, {n} rows over 0..{domain}"
                    );
                }
            }
        }
    }

    #[test]
    fn vectors_round_trip() {
        let rows = vec![vec![4, 5, 6], vec![1, 2, 3], vec![4, 5, 6]];
        let tuples = Tuples::from(rows.clone());
        assert_eq!(tuples.to_vecs(), rows);
        assert_eq!(Tuples::from(tuples.to_vecs()), tuples);
        assert_eq!(tuples.clone().into_vecs(), rows);
        let nullary = Tuples::from(vec![vec![]; 2]);
        assert_eq!(nullary.into_vecs(), vec![Vec::<usize>::new(); 2]);
    }

    #[test]
    fn from_flat_takes_the_buffer_as_rows() {
        let tuples = Tuples::from_flat(2, 3, vec![1, 2, 3, 4, 5, 6]);
        assert_eq!(
            tuples,
            Tuples::from(vec![vec![1, 2], vec![3, 4], vec![5, 6]])
        );
        assert_eq!(Tuples::from_flat(0, 5, Vec::new()).len(), 5);
    }

    #[test]
    #[should_panic(expected = "do not make 2 rows of arity 2")]
    fn from_flat_rejects_a_partial_row() { let _ = Tuples::from_flat(2, 2, vec![1, 2, 3]); }

    /// Nullary rows cost no memory, so the row limit is reachable in a test.
    #[test]
    #[should_panic(expected = "at most RowId::MAX rows")]
    fn a_push_past_the_last_row_id_panics() {
        let mut tuples = Tuples::from_flat(0, RowId::MAX as usize, Vec::new());
        tuples.push(&[]);
    }

    #[test]
    #[should_panic(expected = "exceed RowId::MAX")]
    fn from_flat_past_the_last_row_id_panics() {
        let _ = Tuples::from_flat(0, RowId::MAX as usize + 1, Vec::new());
    }

    #[test]
    fn capacity_is_counted_and_released() {
        let mut tuples = Tuples::with_capacity(3, 10);
        assert!(tuples.heap_size_bytes() >= 30 * std::mem::size_of::<usize>());
        tuples.push(&[1, 2, 3]);
        tuples.push(&[4, 5, 6]);
        tuples.shrink_to_fit();
        assert_eq!(tuples.heap_size_bytes(), 6 * std::mem::size_of::<usize>());
    }
}
```

  Register the module in `kermit-iters/src/lib.rs`. Before (lines 37–43):

```rust
mod hash_strategy;
mod hash_trie;
mod joinable;
mod key_type;
mod linear;
mod optimization;
mod trie;
```

  After:

```rust
mod hash_strategy;
mod hash_trie;
mod joinable;
mod key_type;
mod linear;
mod optimization;
mod trie;
mod tuples;
```

- [ ] **Step 2: Run the tests to verify they fail.**
  ```bash
  CARGO_BUILD_JOBS=2 cargo test -p kermit-iters tuples::
  ```
  Expected: compile errors, `failed to resolve: use of undeclared type Tuples` (E0433) and
  `cannot find type RowId` / `RowId` undeclared.

- [ ] **Step 3: Implement.** In `kermit-iters/src/tuples.rs`, between the module doc and
  `#[cfg(test)]`, insert:

```rust
/// A row's index in its [`Tuples`] batch.
///
/// Four bytes, so a batch holds at most `u32::MAX` (about 4.29 × 10⁹) rows;
/// the paper's largest input has 1.2 × 10⁹ tuples. The paper's tuple
/// pointers are 8 bytes; the 4-byte id is kermit's.
pub type RowId = u32;

/// Tuples of one arity, stored row-major: row `r` is
/// `data[r * arity..(r + 1) * arity]`.
///
/// The row count is kept explicitly, so a batch of nullary tuples (arity 0,
/// an empty buffer) still knows how many it holds. A batch never holds more
/// than [`RowId::MAX`] rows, so every row has a [`RowId`].
///
/// Built row by row ([`push`](Self::push)), from a flat buffer
/// ([`from_flat`](Self::from_flat)), or from one `Vec` per tuple (the
/// `From<Vec<Vec<usize>>>` impl, which test fixtures use).
///
/// ```
/// use kermit_iters::Tuples;
///
/// let mut tuples = Tuples::from(vec![vec![2, 1], vec![1, 3], vec![1, 2]]);
/// assert_eq!((tuples.arity(), tuples.len()), (2, 3));
/// tuples.sort();
/// assert_eq!(tuples.row(0), [1, 2]);
/// assert_eq!(tuples.as_flat(), [1, 2, 1, 3, 2, 1]);
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Tuples {
    arity: usize,
    len: usize,
    data: Vec<usize>,
}

impl Tuples {
    /// An empty batch of `arity`-ary tuples.
    pub fn new(arity: usize) -> Self {
        Self {
            arity,
            len: 0,
            data: Vec::new(),
        }
    }

    /// An empty batch of `arity`-ary tuples with room for `rows` rows
    /// before its buffer grows.
    ///
    /// # Panics
    ///
    /// Panics if `arity * rows` overflows `usize`.
    pub fn with_capacity(arity: usize, rows: usize) -> Self {
        let values = arity
            .checked_mul(rows)
            .expect("Tuples::with_capacity: arity × rows overflows usize");
        Self {
            arity,
            len: 0,
            data: Vec::with_capacity(values),
        }
    }

    /// The batch whose rows are `data` cut into `len` rows of `arity`
    /// values, in order. Takes the buffer without copying it.
    ///
    /// # Panics
    ///
    /// Panics if `len` exceeds [`RowId::MAX`], or if `data.len()` is not
    /// `arity * len`.
    pub fn from_flat(arity: usize, len: usize, data: Vec<usize>) -> Self {
        assert!(
            len <= RowId::MAX as usize,
            "Tuples::from_flat: {len} rows exceed RowId::MAX"
        );
        assert_eq!(
            Some(data.len()),
            arity.checked_mul(len),
            "Tuples::from_flat: {} values do not make {len} rows of arity {arity}",
            data.len()
        );
        Self { arity, len, data }
    }

    /// The number of values in each row.
    pub fn arity(&self) -> usize { self.arity }

    /// The number of rows.
    pub fn len(&self) -> usize { self.len }

    /// Whether the batch holds no rows.
    pub fn is_empty(&self) -> bool { self.len == 0 }

    /// Row `r`: its `arity` values.
    ///
    /// # Panics
    ///
    /// Panics if `r` is not below [`len`](Self::len), also at arity 0,
    /// where every row is the empty slice.
    pub fn row(&self, r: RowId) -> &[usize] {
        let r = r as usize;
        assert!(
            r < self.len,
            "Tuples::row: row {r} out of range for {} rows",
            self.len
        );
        &self.data[r * self.arity..(r + 1) * self.arity]
    }

    /// Every row, in order: [`len`](Self::len) slices of
    /// [`arity`](Self::arity) values (empty slices at arity 0).
    pub fn rows(&self) -> impl ExactSizeIterator<Item = &[usize]> + '_ {
        let (data, arity) = (self.data.as_slice(), self.arity);
        (0..self.len).map(move |r| &data[r * arity..(r + 1) * arity])
    }

    /// Appends `row`.
    ///
    /// # Panics
    ///
    /// Panics if `row.len()` is not [`arity`](Self::arity), or if the batch
    /// already holds [`RowId::MAX`] rows.
    pub fn push(&mut self, row: &[usize]) {
        assert_eq!(
            row.len(),
            self.arity,
            "Tuples::push: a row of arity {} in a batch of arity {}",
            row.len(),
            self.arity
        );
        assert!(
            self.len < RowId::MAX as usize,
            "Tuples::push: a batch holds at most RowId::MAX rows"
        );
        self.data.extend_from_slice(row);
        self.len += 1;
    }

    /// The whole buffer, row after row.
    pub fn as_flat(&self) -> &[usize] { &self.data }

    /// Sorts the rows lexicographically, in place: the order the sorted
    /// tries build in, and one function for both, so the sort costs the
    /// same in TreeTrie's build and ColumnTrie's.
    ///
    /// Arities 1–4 sort the buffer as `[usize; N]` arrays, which compare
    /// lexicographically, so a comparison reads two adjacent rows instead
    /// of following two heap pointers (#111's profile: the pointer-chasing
    /// sort ran at 0.45–1.2 IPC). Wider rows sort a permutation of row ids
    /// by row, then gather the rows into a new buffer of exactly their size.
    /// Arity 0 has nothing to order. Equal rows are identical, so the result
    /// does not depend on the sort being unstable: it is the order of the
    /// hand-rolled comparator the sorted tries used before #111.
    pub fn sort(&mut self) {
        match self.arity {
            | 0 => {},
            | 1 => sort_rows::<1>(&mut self.data),
            | 2 => sort_rows::<2>(&mut self.data),
            | 3 => sort_rows::<3>(&mut self.data),
            | 4 => sort_rows::<4>(&mut self.data),
            | _ => self.sort_by_permutation(),
        }
    }

    /// [`sort`](Self::sort) for rows wider than 4: sort the row ids by row,
    /// then gather the rows in that order into a new buffer.
    fn sort_by_permutation(&mut self) {
        let (data, arity) = (self.data.as_slice(), self.arity);
        let row = |r: RowId| &data[r as usize * arity..(r as usize + 1) * arity];
        // `len <= RowId::MAX`, so the cast is exact.
        let mut order: Vec<RowId> = (0..self.len as RowId).collect();
        order.sort_unstable_by(|&a, &b| row(a).cmp(row(b)));
        let mut sorted = Vec::with_capacity(data.len());
        for r in order {
            sorted.extend_from_slice(row(r));
        }
        self.data = sorted;
    }

    /// Releases the buffer's spare capacity, so that
    /// [`heap_size_bytes`](Self::heap_size_bytes) counts exactly the rows.
    pub fn shrink_to_fit(&mut self) { self.data.shrink_to_fit() }

    /// Heap bytes the buffer holds: its capacity, not its length, in
    /// `usize`s.
    pub fn heap_size_bytes(&self) -> usize { self.data.capacity() * std::mem::size_of::<usize>() }

    /// Every row as its own `Vec`, one allocation per row: the bridge for
    /// builds that still take one `Vec` per tuple, until they build from
    /// row slices (#111).
    pub fn into_vecs(self) -> Vec<Vec<usize>> { self.to_vecs() }

    /// Every row as its own `Vec`, for tests and diagnostics.
    pub fn to_vecs(&self) -> Vec<Vec<usize>> { self.rows().map(<[usize]>::to_vec).collect() }
}

/// Sorts `data` as rows of `N` values: `[usize; N]` compares
/// lexicographically, as rows do.
fn sort_rows<const N: usize>(data: &mut [usize]) {
    let (rows, rest) = data.as_chunks_mut::<N>();
    debug_assert!(rest.is_empty(), "the buffer holds whole rows");
    rows.sort_unstable();
}

/// One `Vec` per tuple, as test fixtures write them. The arity is the first
/// tuple's; an empty `Vec` is the empty batch of arity 0. The buffer is
/// reserved once, at exactly the rows' size.
///
/// # Panics
///
/// Panics if a tuple's arity differs from the first one's (see
/// [`Tuples::push`]).
impl From<Vec<Vec<usize>>> for Tuples {
    fn from(rows: Vec<Vec<usize>>) -> Self {
        let arity = rows.first().map_or(0, Vec::len);
        let mut tuples = Tuples::with_capacity(arity, rows.len());
        for row in &rows {
            tuples.push(row);
        }
        tuples
    }
}
```

  Notes for the implementer:
  - `slice::as_chunks_mut` is stable since Rust 1.88; the pinned nightly has it.
  - Do **not** add a second `From<Vec<_>>` (or `From<Vec<[usize; N]>>`) impl for `Tuples`. T3 relies on
    `from_tuples(h, vec![])` and `vec![vec![]; 3]` inferring their element type through this one impl.

- [ ] **Step 4: Run the tests to verify they pass.**
  ```bash
  CARGO_BUILD_JOBS=2 cargo test -p kermit-iters
  ```
  Expected: the 14 `tuples::tests` pass, plus the `Tuples` doctest; 0 failed.

- [ ] **Step 5: Export.** In `kermit-iters/src/lib.rs`, add a paragraph to the crate doc after line 10
  (`//! [`JoinIterable`] unifies data structures that may participate in joins.`):

```rust
//!
//! A relation is built from a [`Tuples`] batch: its tuples row-major in one
//! buffer, each row addressed by a [`RowId`].
```

  Then extend the `pub use` block. Before (lines 45–53):

```rust
pub use {
    hash_strategy::{FxHashStrategy, HashStrategy, SipHashStrategy},
    hash_trie::{HashTrieIterable, HashTrieIterator},
    joinable::JoinIterable,
    key_type::Key,
    linear::{LinearIterable, LinearIterator},
    optimization::{BuildMode, ConfigOption, HasOptimizationAxes, LayoutOption},
    trie::{TrieIterable, TrieIterator, TrieIteratorWrapper},
};
```

  After:

```rust
pub use {
    hash_strategy::{FxHashStrategy, HashStrategy, SipHashStrategy},
    hash_trie::{HashTrieIterable, HashTrieIterator},
    joinable::JoinIterable,
    key_type::Key,
    linear::{LinearIterable, LinearIterator},
    optimization::{BuildMode, ConfigOption, HasOptimizationAxes, LayoutOption},
    trie::{TrieIterable, TrieIterator, TrieIteratorWrapper},
    tuples::{RowId, Tuples},
};
```

  Re-export from kermit-ds so the binary and tests can name `kermit_ds::Tuples`. In
  `kermit-ds/src/lib.rs`, before (lines 49–50):

```rust
    heap_size::HeapSize,
    morsel::Threads,
```

  After:

```rust
    heap_size::HeapSize,
    kermit_iters::{RowId, Tuples},
    morsel::Threads,
```

- [ ] **Step 6: Lint, format, document.**
  ```bash
  CARGO_BUILD_JOBS=2 cargo build -p kermit-ds
  CARGO_BUILD_JOBS=2 cargo clippy --all-targets -- -D warnings
  nix develop --command cargo fmt --all
  RUSTDOCFLAGS=-Dwarnings cargo doc --workspace --no-deps
  ```
  Expected: all clean. The comparator copied into the test uses `for i in 0..a.len()`. It is
  verbatim from `kermit-ds/src/ds/tree_trie/implementation.rs:319-328`, which already passes this
  clippy gate, so leave it as written.

- [ ] **Step 7: Commit.**
  ```bash
  git -C $WT add kermit-iters/src/tuples.rs kermit-iters/src/lib.rs kermit-ds/src/lib.rs
  git -C $WT commit -m "feat(iters): Tuples, a row-major batch of one arity, and RowId (#111)

  A relation's tuples as one buffer: rows of one arity, row-major, with an
  explicit row count (nullary batches keep theirs) capped at RowId::MAX.
  sort() orders rows as the sorted tries' hand-rolled comparator does,
  through [usize; N] arrays at arities 1-4 and a row-id permutation above.
  From<Vec<Vec<usize>>> keeps every test fixture compiling. Unused yet.

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_015P4eTR34w48GpD7DvPPaee"
  ```

---

### Task 2: `LeafRows<'a>` in kermit-iters

Additive. T7 makes it `HashTrieIterator::leaf_tuples`' return type. The rows it lends borrow the
buffer, not the view (`row(i) -> &'a [usize]`). The leaf product (`emit_leaf`) and the selection view
need that, and `rows_outlive_the_view` pins it at compile time.

**Files:**
- Create: `kermit-iters/src/leaf_rows.rs`
- Modify: `kermit-iters/src/lib.rs` (crate doc paragraph from Task 1; `mod` list; `pub use` block)
- Modify: `kermit-ds/src/lib.rs` (the `kermit_iters::{RowId, Tuples}` line from Task 1)
- Test: `kermit-iters/src/leaf_rows.rs` (`mod tests`)

- [ ] **Step 1: Write the failing tests.** Create `kermit-iters/src/leaf_rows.rs`:

```rust
//! [`LeafRows`]: the rows of one hash-trie leaf chain, read through their
//! row ids from the batch that holds them.

use crate::tuples::{RowId, Tuples};

#[cfg(test)]
mod tests {
    use super::*;

    fn batch() -> Tuples { Tuples::from(vec![vec![1, 2], vec![3, 4], vec![5, 6], vec![7, 8]]) }

    #[test]
    fn a_chain_reads_its_rows_in_chain_order() {
        let tuples = batch();
        let ids: [RowId; 3] = [2, 0, 2];
        let leaf = LeafRows::new(&tuples, &ids);
        assert_eq!(leaf.len(), 3);
        assert!(!leaf.is_empty());
        assert_eq!(leaf.row(0), [5, 6]);
        assert_eq!(leaf.row(1), [1, 2]);
        assert_eq!(leaf.row(2), [5, 6]);
        assert_eq!(leaf.ids(), [2, 0, 2]);
        assert_eq!(leaf.arity(), 2);
        assert_eq!(leaf.data(), tuples.as_flat());
        assert_eq!(leaf.iter().len(), 3);
        let rows: Vec<&[usize]> = leaf.iter().collect();
        assert_eq!(rows, [leaf.row(0), leaf.row(1), leaf.row(2)]);
        assert_eq!(leaf.to_vecs(), vec![vec![5, 6], vec![1, 2], vec![5, 6]]);
        // A view is `Copy`: three borrows, nothing owned.
        let copy = leaf;
        assert_eq!(copy.to_vecs(), leaf.to_vecs());
    }

    #[test]
    fn from_parts_views_the_same_rows() {
        let tuples = batch();
        let ids: [RowId; 2] = [3, 1];
        let whole = LeafRows::new(&tuples, &ids);
        let parts = LeafRows::from_parts(tuples.as_flat(), tuples.arity(), &ids);
        assert_eq!(parts.to_vecs(), whole.to_vecs());
        assert_eq!(
            (parts.data(), parts.arity(), parts.ids()),
            (whole.data(), whole.arity(), whole.ids())
        );
    }

    /// One value held inline, with no batch behind it (the constant view's
    /// leaf in `kermit-algos`).
    #[test]
    fn a_view_needs_no_batch() {
        let value = [7];
        let only: [RowId; 1] = [0];
        let leaf = LeafRows::from_parts(&value, 1, &only);
        assert_eq!(leaf.to_vecs(), vec![vec![7]]);
    }

    #[test]
    fn an_empty_chain_lends_nothing() {
        let tuples = batch();
        let leaf = LeafRows::new(&tuples, &[]);
        assert_eq!(leaf.len(), 0);
        assert!(leaf.is_empty());
        assert_eq!(leaf.iter().next(), None);
        assert!(leaf.to_vecs().is_empty());
    }

    #[test]
    #[should_panic(expected = "index out of bounds")]
    fn a_row_past_the_chain_panics() {
        let tuples = batch();
        let ids: [RowId; 2] = [0, 1];
        let _ = LeafRows::new(&tuples, &ids).row(2);
    }

    /// Rows borrow the buffer, not the view: a caller keeps them after the
    /// view it read them through is gone. This compiles only if `row` and
    /// `iter` lend `&'a [usize]`.
    #[test]
    fn rows_outlive_the_view() {
        let tuples = batch();
        let ids: [RowId; 1] = [3];
        let row = {
            let leaf = LeafRows::new(&tuples, &ids);
            leaf.row(0)
        };
        let all: Vec<&[usize]> = {
            let leaf = LeafRows::new(&tuples, &ids);
            leaf.iter().collect()
        };
        assert_eq!(row, [7, 8]);
        assert_eq!(all, [row]);
    }
}
```

  Register it in `kermit-iters/src/lib.rs`. Before:

```rust
mod key_type;
mod linear;
```

  After:

```rust
mod key_type;
mod leaf_rows;
mod linear;
```

- [ ] **Step 2: Run the tests to verify they fail.**
  ```bash
  CARGO_BUILD_JOBS=2 cargo test -p kermit-iters leaf_rows::
  ```
  Expected: compile error, `failed to resolve: use of undeclared type LeafRows` (E0433). An
  `unused import: Tuples` warning may also appear.

- [ ] **Step 3: Implement.** In `kermit-iters/src/leaf_rows.rs`, between the `use` line and
  `#[cfg(test)]`, insert:

```rust
/// A leaf chain's tuples: the ids of its rows, in chain order, and the
/// row-major buffer they index ([`Tuples`]'s layout).
///
/// A `HashTrie` keeps its relation's batch and stores each leaf chain as
/// row ids into it (#111). A `LeafRows` reads such a chain: three borrows,
/// nothing copied. The rows it lends borrow the buffer, not the view, so
/// they outlive it.
///
/// ```
/// use kermit_iters::{LeafRows, RowId, Tuples};
///
/// let tuples = Tuples::from(vec![vec![1, 2], vec![3, 4], vec![5, 6]]);
/// let chain: [RowId; 2] = [2, 0];
/// let leaf = LeafRows::new(&tuples, &chain);
/// assert_eq!(leaf.len(), 2);
/// assert_eq!(leaf.row(0), [5, 6]);
/// assert_eq!(leaf.to_vecs(), vec![vec![5, 6], vec![1, 2]]);
/// ```
#[derive(Clone, Copy, Debug)]
pub struct LeafRows<'a> {
    data: &'a [usize],
    arity: usize,
    rows: &'a [RowId],
}

impl<'a> LeafRows<'a> {
    /// The rows `rows` of `tuples`, in the order the ids are listed.
    pub fn new(tuples: &'a Tuples, rows: &'a [RowId]) -> Self {
        Self::from_parts(tuples.as_flat(), tuples.arity(), rows)
    }

    /// The rows `rows` of the row-major buffer `data`, whose rows hold
    /// `arity` values each: a view over a buffer that is not a whole
    /// [`Tuples`], such as one value held inline. The ids are not checked
    /// here; reading a row past the end of `data` panics.
    pub fn from_parts(data: &'a [usize], arity: usize, rows: &'a [RowId]) -> Self {
        Self { data, arity, rows }
    }

    /// The number of rows in the chain.
    pub fn len(&self) -> usize { self.rows.len() }

    /// Whether the chain holds no rows.
    pub fn is_empty(&self) -> bool { self.rows.is_empty() }

    /// The chain's `i`-th row.
    ///
    /// # Panics
    ///
    /// Panics if `i` is not below [`len`](Self::len), or if the row's id is
    /// past the buffer's last row.
    pub fn row(&self, i: usize) -> &'a [usize] { row_of(self.data, self.arity, self.rows[i]) }

    /// The chain's row ids, in chain order.
    pub fn ids(&self) -> &'a [RowId] { self.rows }

    /// The buffer the ids index.
    pub fn data(&self) -> &'a [usize] { self.data }

    /// The number of values in each row.
    pub fn arity(&self) -> usize { self.arity }

    /// Every row of the chain, in chain order.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = &'a [usize]> + 'a {
        let (data, arity, rows) = (self.data, self.arity, self.rows);
        rows.iter().map(move |&r| row_of(data, arity, r))
    }

    /// Every row as its own `Vec`, for tests and diagnostics.
    pub fn to_vecs(&self) -> Vec<Vec<usize>> { self.iter().map(<[usize]>::to_vec).collect() }
}

/// Row `r` of the row-major `data`, whose rows hold `arity` values.
fn row_of(data: &[usize], arity: usize, r: RowId) -> &[usize] {
    let r = r as usize;
    &data[r * arity..(r + 1) * arity]
}
```

- [ ] **Step 4: Run the tests to verify they pass.**
  ```bash
  CARGO_BUILD_JOBS=2 cargo test -p kermit-iters
  ```
  Expected: the 6 `leaf_rows::tests` and the 14 `tuples::tests` pass, plus both doctests; 0 failed.

- [ ] **Step 5: Export.** In `kermit-iters/src/lib.rs`, extend Task 1's crate-doc paragraph. Before:

```rust
//! A relation is built from a [`Tuples`] batch: its tuples row-major in one
//! buffer, each row addressed by a [`RowId`].
```

  After:

```rust
//! A relation is built from a [`Tuples`] batch: its tuples row-major in one
//! buffer, each row addressed by a [`RowId`]. [`LeafRows`] reads a hash-trie
//! leaf chain's rows from such a buffer through their ids.
```

  In the `pub use` block, before:

```rust
    key_type::Key,
    linear::{LinearIterable, LinearIterator},
```

  After:

```rust
    key_type::Key,
    leaf_rows::LeafRows,
    linear::{LinearIterable, LinearIterator},
```

  In `kermit-ds/src/lib.rs`, before:

```rust
    kermit_iters::{RowId, Tuples},
```

  After:

```rust
    kermit_iters::{LeafRows, RowId, Tuples},
```

- [ ] **Step 6: Lint, format, document.**
  ```bash
  CARGO_BUILD_JOBS=2 cargo build -p kermit-ds
  CARGO_BUILD_JOBS=2 cargo clippy --all-targets -- -D warnings
  nix develop --command cargo fmt --all
  RUSTDOCFLAGS=-Dwarnings cargo doc --workspace --no-deps
  ```
  Expected: all clean.

- [ ] **Step 7: Commit.**
  ```bash
  git -C $WT add kermit-iters/src/leaf_rows.rs kermit-iters/src/lib.rs kermit-ds/src/lib.rs
  git -C $WT commit -m "feat(iters): LeafRows, a leaf chain's rows read through row ids (#111)

  A Copy view of a chain: its row ids and the row-major buffer they index.
  Rows borrow the buffer, not the view. from_parts serves views over a
  buffer that is not a whole batch (one value held inline). Unused until
  leaf_tuples returns it.

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_015P4eTR34w48GpD7DvPPaee"
  ```

---

### Task 3: Relation-family signatures take `impl Into<Tuples>` and `impl AsRef<[usize]>`

API only. Every structure converts at its public seam:
- builds call `Tuples::into_vecs(tuples.into())`;
- `insert` calls `tuple.as_ref()`;
- `insert_all` iterates `rows()`.

Every internal build function keeps its `Vec<Vec<usize>>` signature:
- TreeTrie's `from_tuples_parallel` / `build_parallel`;
- ColumnTrie's `from_sorted` / `from_sorted_by_insertion`;
- HashTrie's `from_tuples_in_bulk` / `from_tuples_incrementally` / `from_tuples_partitioned`.

Every structure therefore builds exactly what it built before.

What pins the unchanged behaviour:
- the identity tests in `kermit-ds/src/ds/hash_trie/{bulk,radix,parallel}.rs`, `tree_trie/implementation.rs` and `column_trie/implementation.rs`;
- the kermit-ds suites;
- the join suites in `kermit/tests/join_tests.rs`.

**Byte-identity note.** HashTrie's `heap_size_bytes` counts each stored tuple's `Vec` capacity, and
`identity.rs::assert_same_chain` compares it. `into_vecs` and `to_vec` give exact capacity. So does
every current input: `vec!` literals, `clone`, `to_vec`, and both readers' `Vec::with_capacity(arity)`.
`space` and the identity tests therefore do not move. If an identity test fails on
`"tuple {i} capacity"`, an input had slack capacity: report it, do not loosen the test.

**Cost note.** Until T5, TreeTrie's serial `from_tuples` loop calls `trie.insert(tuple)`, which now
copies the row (`to_vec`) once more. This costs time only, nothing measures it mid-plan, and T5
removes the loop.

**Files:**
- Modify: `kermit-ds/src/relation.rs`:
  - import, line 13;
  - `Relation`, lines 246–271;
  - `ConfigurableRelation::from_tuples_with_config`, lines 296–304;
  - `BuildModeRelation::from_tuples_with_build_mode`, lines 328–338;
  - `ConfiguredBuildModeRelation`, lines 345–350;
  - tests, appended to `mod tests` (ends at line 772).
- Modify: `kermit-ds/src/configured.rs` (import, lines 25–28; lines 127–133 and 146–155)
- Modify: `kermit-ds/src/built_with.rs` (import, lines 23–26; lines 115–125)
- Modify: `kermit-ds/src/ds/tree_trie/implementation.rs` (import, line 8; lines 302, 343–364, 376–385)
- Modify: `kermit-ds/src/ds/column_trie/implementation.rs`:
  - import, line 7;
  - lines 436–460 and 473–475;
  - tests at 875–879, 924–926 and 1013–1014.
- Modify: `kermit-ds/src/ds/hash_trie/implementation.rs`:
  - import, line 34;
  - lines 385–407, 418–422, 456–459, 621–630 and 639–644;
  - test at line 1802.
- Modify: `kermit-ds/src/ds/hash_trie/parallel.rs` (test, lines 643–652)
- Modify: `kermit-ds/src/ds/hash_trie/radix.rs` (test, lines 246–255)
- Modify: `kermit-ds/src/ds/tree_trie/tests.rs` (test, lines 288–290)
- Modify: `kermit-ds/tests/common/macros.rs` (lines 508, 544, 637)
- Modify: `kermit/tests/result_allocation.rs` (lines 553–558)

- [ ] **Step 1: Write the failing tests.** Append to `mod tests` in `kermit-ds/src/relation.rs`
  (after `read_csv_returns_header_and_tuples`, before the closing `}` at line 772):

```rust
    // ── constructors take a batch (#111) ──────────────────────────────

    crate::define_config_provider!(
        DefaultConfig,
        crate::HashTrieConfig,
        crate::HashTrieConfig::default()
    );
    crate::define_build_mode_provider!(
        ColumnIncremental,
        crate::ColumnTrieBuildMode,
        crate::ColumnTrieBuildMode::Incremental
    );

    /// A sorted trie's tuples, in its (sorted) iteration order.
    fn trie_rows(relation: &impl kermit_iters::TrieIterable) -> Vec<Vec<usize>> {
        relation.trie_iter().into_iter().collect()
    }

    /// A hash trie's tuples, sorted (a hash trie lends them in hash order).
    fn hash_rows(relation: &crate::ds::HashTrie) -> Vec<Vec<usize>> {
        let mut rows = relation.collect_tuples();
        rows.sort();
        rows
    }

    /// Every constructor takes a [`Tuples`] batch as well as one `Vec` per
    /// tuple, and `insert` takes a slice or an array as well as a `Vec`;
    /// every form builds the same relation.
    #[test]
    fn every_constructor_takes_a_batch() {
        use crate::{
            ds::{
                ColumnTrie, ColumnTrieBuildMode, HashTrie, HashTrieBuildMode, HashTrieConfig,
                TreeTrie, TreeTrieBuildMode,
            },
            BuiltWith, Cardinality, Configured, HeapSize, Threads,
        };
        let vecs = vec![vec![3, 4], vec![1, 2], vec![1, 2], vec![1, 5]];
        let batch = Tuples::from(vecs.clone());
        let two = || Threads::new(2).unwrap();

        // Sorted tries: a set.
        let tree: TreeTrie = TreeTrie::from_tuples(2.into(), vecs);
        let set = trie_rows(&tree);
        assert_eq!(set, vec![vec![1, 2], vec![1, 5], vec![3, 4]]);
        let tree_batch: TreeTrie = TreeTrie::from_tuples(2.into(), batch.clone());
        assert_eq!(trie_rows(&tree_batch), set);
        assert_eq!(tree_batch.heap_size_bytes(), tree.heap_size_bytes());
        let tree_parallel: TreeTrie = TreeTrie::from_tuples_with_build_mode(
            2.into(),
            TreeTrieBuildMode::Parallel(two()),
            batch.clone(),
        );
        assert_eq!(trie_rows(&tree_parallel), set);
        let mut tree_inserted: TreeTrie = TreeTrie::new(2.into());
        for row in batch.rows() {
            tree_inserted.insert(row);
        }
        tree_inserted.insert([1, 5]);
        assert_eq!(trie_rows(&tree_inserted), set);
        let mut tree_all: TreeTrie = TreeTrie::new(2.into());
        tree_all.insert_all(batch.clone());
        assert_eq!(trie_rows(&tree_all), set);

        let column: ColumnTrie = ColumnTrie::from_tuples(2.into(), batch.clone());
        assert_eq!(trie_rows(&column), set);
        let column_incremental: ColumnTrie = ColumnTrie::from_tuples_with_build_mode(
            2.into(),
            ColumnTrieBuildMode::Incremental,
            batch.clone(),
        );
        assert_eq!(trie_rows(&column_incremental), set);
        let built_with =
            BuiltWith::<ColumnTrie, ColumnIncremental>::from_tuples(2.into(), batch.clone());
        assert_eq!(trie_rows(&built_with), set);
        let mut column_all: ColumnTrie = ColumnTrie::new(2.into());
        column_all.insert_all(batch.clone());
        column_all.insert(&[1, 5][..]);
        assert_eq!(trie_rows(&column_all), set);

        // The hash trie: a multiset.
        let multiset = vec![vec![1, 2], vec![1, 2], vec![1, 5], vec![3, 4]];
        let hash: HashTrie = HashTrie::from_tuples(2.into(), batch.clone());
        assert_eq!(hash_rows(&hash), multiset);
        let hash_config: HashTrie =
            HashTrie::from_tuples_with_config(2.into(), HashTrieConfig::default(), batch.clone());
        assert_eq!(hash_rows(&hash_config), multiset);
        let hash_incremental: HashTrie = HashTrie::from_tuples_with_build_mode(
            2.into(),
            HashTrieBuildMode::Incremental,
            batch.clone(),
        );
        assert_eq!(hash_rows(&hash_incremental), multiset);
        let hash_both: HashTrie = HashTrie::from_tuples_with_config_and_build_mode(
            2.into(),
            HashTrieConfig::default(),
            HashTrieBuildMode::Parallel(two()),
            batch.clone(),
        );
        assert_eq!(hash_rows(&hash_both), multiset);
        let configured = Configured::<HashTrie, DefaultConfig>::from_tuples(2.into(), batch.clone());
        assert_eq!(hash_rows(&configured), multiset);
        let mut hash_inserted: HashTrie = HashTrie::new(2.into());
        for row in batch.rows() {
            hash_inserted.insert(row);
        }
        assert_eq!(hash_rows(&hash_inserted), multiset);
        hash_inserted.insert_all(batch);
        assert_eq!(hash_inserted.tuple_count(), 8);
    }

    /// A batch carries one arity, so a mixed-arity input is rejected while
    /// it converts, before any build sees it.
    #[test]
    #[should_panic(expected = "in a batch of arity 2")]
    fn mixed_arity_vectors_are_rejected_before_the_build() {
        use crate::ds::TreeTrie;
        let _: TreeTrie = TreeTrie::from_tuples(2.into(), vec![vec![1, 2], vec![3]]);
    }
```

- [ ] **Step 2: Run the tests to verify they fail.**
  ```bash
  CARGO_BUILD_JOBS=2 cargo test -p kermit-ds --lib relation::tests
  ```
  Expected compile errors:
  - E0308 `expected Vec<Vec<usize>>, found Tuples` at each `batch.clone()` argument;
  - E0308 `expected Vec<usize>, found &[usize]` at `tree_inserted.insert(row)`;
  - E0308 `expected Vec<usize>, found [{integer}; 2]` at `.insert([1, 5])`.

- [ ] **Step 3: Change the traits** in `kermit-ds/src/relation.rs`.

  Import, before (line 13):

```rust
    kermit_iters::JoinIterable,
```

  After:

```rust
    kermit_iters::{JoinIterable, Tuples},
```

  `Relation`, before (lines 246–271):

```rust
    /// Creates a relation populated with `tuples`, matching `header`.
    /// Implementations may sort or deduplicate during bulk construction;
    /// prefer this over `new` followed by repeated `insert` calls when all
    /// tuples are known up front.
    ///
    /// # Panics
    ///
    /// Panics if any tuple's length does not equal `header.arity()`.
    fn from_tuples(header: RelationHeader, tuples: Vec<Vec<usize>>) -> Self;

    /// Inserts a tuple. Duplicate tuples are silently absorbed (the relation
    /// behaves as a set).
    ///
    /// # Panics
    ///
    /// Panics if `tuple.len() != self.header().arity()`.
    fn insert(&mut self, tuple: Vec<usize>);

    /// Inserts every tuple in `tuples`. Equivalent to calling
    /// [`insert`](Self::insert) in a loop; provided so implementations can
    /// specialise bulk insertion.
    ///
    /// # Panics
    ///
    /// Panics if any tuple's length does not match the relation's arity.
    fn insert_all(&mut self, tuples: Vec<Vec<usize>>);
```

  After:

```rust
    /// Creates a relation populated with `tuples`, matching `header`.
    /// `tuples` is a [`Tuples`] batch, or anything that converts into one:
    /// test fixtures pass one `Vec<usize>` per tuple, whose conversion
    /// panics on mixed arity. An empty batch is accepted under any header.
    /// Implementations may sort or deduplicate during bulk construction;
    /// prefer this over `new` followed by repeated `insert` calls when all
    /// tuples are known up front.
    ///
    /// # Panics
    ///
    /// Panics if `tuples` is not empty and its arity does not equal
    /// `header.arity()`.
    fn from_tuples(header: RelationHeader, tuples: impl Into<Tuples>) -> Self;

    /// Inserts a tuple, given as anything that lends a slice of keys
    /// (`&[usize]`, `Vec<usize>`, an array). Duplicate tuples are silently
    /// absorbed (the relation behaves as a set).
    ///
    /// # Panics
    ///
    /// Panics if the tuple's length is not `self.header().arity()`.
    fn insert(&mut self, tuple: impl AsRef<[usize]>);

    /// Inserts every row of `tuples`. Equivalent to calling
    /// [`insert`](Self::insert) on each row in a loop; provided so
    /// implementations can specialise bulk insertion.
    ///
    /// # Panics
    ///
    /// Panics if `tuples` is not empty and its arity does not match the
    /// relation's arity.
    fn insert_all(&mut self, tuples: impl Into<Tuples>);
```

  `ConfigurableRelation`, before (lines 296–304):

```rust
    /// Creates a relation populated with `tuples` under `config`. Same
    /// contract as [`Relation::from_tuples`].
    ///
    /// # Panics
    ///
    /// Panics if any tuple's length does not equal `header.arity()`.
    fn from_tuples_with_config(
        header: RelationHeader, config: Self::Config, tuples: Vec<Vec<usize>>,
    ) -> Self;
```

  After:

```rust
    /// Creates a relation populated with `tuples` under `config`. Same
    /// contract as [`Relation::from_tuples`].
    ///
    /// # Panics
    ///
    /// Panics if `tuples` is not empty and its arity does not equal
    /// `header.arity()`.
    fn from_tuples_with_config(
        header: RelationHeader, config: Self::Config, tuples: impl Into<Tuples>,
    ) -> Self;
```

  `BuildModeRelation`, before (lines 330–338):

```rust
    /// # Panics
    ///
    /// Panics if any tuple's length does not equal `header.arity()`, or if
    /// `mode` has a prerequisite the default config lacks (`HashTrie`'s
    /// `presized:N` requires `root-capacity=tuples`).
    fn from_tuples_with_build_mode(
        header: RelationHeader, mode: Self::BuildMode, tuples: Vec<Vec<usize>>,
    ) -> Self;
```

  After:

```rust
    /// # Panics
    ///
    /// Panics if `tuples` is not empty and its arity does not equal
    /// `header.arity()`, or if `mode` has a prerequisite the default config
    /// lacks (`HashTrie`'s `presized:N` requires `root-capacity=tuples`).
    fn from_tuples_with_build_mode(
        header: RelationHeader, mode: Self::BuildMode, tuples: impl Into<Tuples>,
    ) -> Self;
```

  `ConfiguredBuildModeRelation`, before (lines 347–350):

```rust
    fn from_tuples_with_config_and_build_mode(
        header: RelationHeader, config: Self::Config, mode: Self::BuildMode,
        tuples: Vec<Vec<usize>>,
    ) -> Self;
```

  After:

```rust
    fn from_tuples_with_config_and_build_mode(
        header: RelationHeader, config: Self::Config, mode: Self::BuildMode,
        tuples: impl Into<Tuples>,
    ) -> Self;
```

  Leave these unchanged; they compile as they are:
  - `project_via_trie_iter` (lines 202–225): it passes an annotated `Vec<Vec<usize>>`. It is T6's to rewrite.
  - the `RelationFileExt` blanket impl (lines 552–567): `read_csv` / `read_parquet` still return `Vec<Vec<usize>>` until T4.
  - None of these traits has a default method.

- [ ] **Step 4: Forward through the wrappers.**

  `kermit-ds/src/configured.rs`, import, before (lines 25–28):

```rust
    kermit_iters::{
        HasOptimizationAxes, HashTrieIterable, HashTrieIterator, JoinIterable, TrieIterable,
        TrieIterator,
    },
```

  After:

```rust
    kermit_iters::{
        HasOptimizationAxes, HashTrieIterable, HashTrieIterator, JoinIterable, TrieIterable,
        TrieIterator, Tuples,
    },
```

  Before (lines 127–133):

```rust
    fn from_tuples(header: RelationHeader, tuples: Vec<Vec<usize>>) -> Self {
        Self::wrap(R::from_tuples_with_config(header, P::config(), tuples))
    }

    fn insert(&mut self, tuple: Vec<usize>) { self.inner.insert(tuple) }

    fn insert_all(&mut self, tuples: Vec<Vec<usize>>) { self.inner.insert_all(tuples) }
```

  After:

```rust
    fn from_tuples(header: RelationHeader, tuples: impl Into<Tuples>) -> Self {
        Self::wrap(R::from_tuples_with_config(header, P::config(), tuples))
    }

    fn insert(&mut self, tuple: impl AsRef<[usize]>) { self.inner.insert(tuple) }

    fn insert_all(&mut self, tuples: impl Into<Tuples>) { self.inner.insert_all(tuples) }
```

  Before (lines 146–148):

```rust
    fn from_tuples_with_build_mode(
        header: RelationHeader, mode: R::BuildMode, tuples: Vec<Vec<usize>>,
    ) -> Self {
```

  After:

```rust
    fn from_tuples_with_build_mode(
        header: RelationHeader, mode: R::BuildMode, tuples: impl Into<Tuples>,
    ) -> Self {
```

  `kermit-ds/src/built_with.rs`, import, before (lines 23–26):

```rust
    kermit_iters::{
        HasOptimizationAxes, HashTrieIterable, HashTrieIterator, JoinIterable, TrieIterable,
        TrieIterator,
    },
```

  After:

```rust
    kermit_iters::{
        HasOptimizationAxes, HashTrieIterable, HashTrieIterator, JoinIterable, TrieIterable,
        TrieIterator, Tuples,
    },
```

  Before (lines 115–125):

```rust
    fn from_tuples(header: RelationHeader, tuples: Vec<Vec<usize>>) -> Self {
        Self::wrap(R::from_tuples_with_build_mode(
            header,
            P::build_mode(),
            tuples,
        ))
    }

    fn insert(&mut self, tuple: Vec<usize>) { self.inner.insert(tuple) }

    fn insert_all(&mut self, tuples: Vec<Vec<usize>>) { self.inner.insert_all(tuples) }
```

  After:

```rust
    fn from_tuples(header: RelationHeader, tuples: impl Into<Tuples>) -> Self {
        Self::wrap(R::from_tuples_with_build_mode(
            header,
            P::build_mode(),
            tuples,
        ))
    }

    fn insert(&mut self, tuple: impl AsRef<[usize]>) { self.inner.insert(tuple) }

    fn insert_all(&mut self, tuples: impl Into<Tuples>) { self.inner.insert_all(tuples) }
```

- [ ] **Step 5: Convert at each structure's seam.** Every conversion is spelled
  `Tuples::into_vecs(tuples.into())` or `let tuples: Tuples = tuples.into();`, never
  `tuples.into().into_vecs()`. A method call on a bare `.into()` leaves the target type to
  inference.

  **TreeTrie** (`kermit-ds/src/ds/tree_trie/implementation.rs`). Import, before (line 8):

```rust
    kermit_iters::{HasOptimizationAxes, JoinIterable},
```

  After:

```rust
    kermit_iters::{HasOptimizationAxes, JoinIterable, Tuples},
```

  `from_tuples`, before (lines 302–303):

```rust
    fn from_tuples(header: RelationHeader, mut tuples: Vec<Vec<usize>>) -> Self {
        if tuples.is_empty() {
```

  After (the rest of the body unchanged):

```rust
    fn from_tuples(header: RelationHeader, tuples: impl Into<Tuples>) -> Self {
        // One `Vec` per tuple, the form this build takes until it builds
        // from row slices (#111).
        let mut tuples = Tuples::into_vecs(tuples.into());
        if tuples.is_empty() {
```

  `insert` and `insert_all`, before (lines 343–364):

```rust
    fn insert(&mut self, tuple: Vec<usize>) {
        assert_eq!(
            tuple.len(),
            self.header().arity(),
            "tuple arity must match relation arity"
        );
        if insert_into_children(&mut self.children, tuple) {
            self.tuple_count += 1;
        }
    }

    /// Inserts every tuple in `tuples`.
    ///
    /// # Panics
    ///
    /// Panics if any tuple's arity does not match the relation's arity
    /// (propagated from [`insert`](Self::insert)).
    fn insert_all(&mut self, tuples: Vec<Vec<usize>>) {
        for tuple in tuples {
            self.insert(tuple);
        }
    }
```

  After:

```rust
    fn insert(&mut self, tuple: impl AsRef<[usize]>) {
        let tuple = tuple.as_ref();
        assert_eq!(
            tuple.len(),
            self.header().arity(),
            "tuple arity must match relation arity"
        );
        if insert_into_children(&mut self.children, tuple.to_vec()) {
            self.tuple_count += 1;
        }
    }

    /// Inserts every row of `tuples`.
    ///
    /// # Panics
    ///
    /// Panics if any tuple's arity does not match the relation's arity
    /// (propagated from [`insert`](Self::insert)).
    fn insert_all(&mut self, tuples: impl Into<Tuples>) {
        let tuples: Tuples = tuples.into();
        for tuple in tuples.rows() {
            self.insert(tuple);
        }
    }
```

  `BuildModeRelation`, before (lines 376–385):

```rust
    fn from_tuples_with_build_mode(
        header: RelationHeader, mode: TreeTrieBuildMode, tuples: Vec<Vec<usize>>,
    ) -> Self {
        match mode {
            | TreeTrieBuildMode::Serial => Self::from_tuples(header, tuples),
            | TreeTrieBuildMode::Parallel(threads) => {
                Self::from_tuples_parallel(header, threads, tuples)
            },
        }
    }
```

  After:

```rust
    fn from_tuples_with_build_mode(
        header: RelationHeader, mode: TreeTrieBuildMode, tuples: impl Into<Tuples>,
    ) -> Self {
        match mode {
            | TreeTrieBuildMode::Serial => Self::from_tuples(header, tuples),
            | TreeTrieBuildMode::Parallel(threads) => {
                // One `Vec` per tuple until the build reads rows (#111).
                Self::from_tuples_parallel(header, threads, Tuples::into_vecs(tuples.into()))
            },
        }
    }
```

  **ColumnTrie** (`kermit-ds/src/ds/column_trie/implementation.rs`). Import, before (line 7):

```rust
    kermit_iters::{HasOptimizationAxes, JoinIterable},
```

  After:

```rust
    kermit_iters::{HasOptimizationAxes, JoinIterable, Tuples},
```

  Before (lines 436–460):

```rust
    fn from_tuples(header: RelationHeader, tuples: Vec<Vec<usize>>) -> Self {
        Self::from_tuples_with_build_mode(header, ColumnTrieBuildMode::default(), tuples)
    }

    /// Inserts a single tuple. Duplicate tuples are silently absorbed.
    ///
    /// # Panics
    ///
    /// Panics if `tuple.len()` does not match the relation's arity.
    fn insert(&mut self, tuple: Vec<usize>) {
        assert_eq!(
            tuple.len(),
            self.header().arity(),
            "tuple arity must match relation arity"
        );
        if self.internal_insert(&tuple) {
            self.tuple_count += 1;
        }
    }

    fn insert_all(&mut self, tuples: Vec<Vec<usize>>) {
        for tuple in tuples {
            self.insert(tuple);
        }
    }
```

  After:

```rust
    fn from_tuples(header: RelationHeader, tuples: impl Into<Tuples>) -> Self {
        Self::from_tuples_with_build_mode(header, ColumnTrieBuildMode::default(), tuples)
    }

    /// Inserts a single tuple. Duplicate tuples are silently absorbed.
    ///
    /// # Panics
    ///
    /// Panics if the tuple's length does not match the relation's arity.
    fn insert(&mut self, tuple: impl AsRef<[usize]>) {
        let tuple = tuple.as_ref();
        assert_eq!(
            tuple.len(),
            self.header().arity(),
            "tuple arity must match relation arity"
        );
        if self.internal_insert(tuple) {
            self.tuple_count += 1;
        }
    }

    fn insert_all(&mut self, tuples: impl Into<Tuples>) {
        let tuples: Tuples = tuples.into();
        for tuple in tuples.rows() {
            self.insert(tuple);
        }
    }
```

  Before (lines 473–476):

```rust
    fn from_tuples_with_build_mode(
        header: RelationHeader, mode: ColumnTrieBuildMode, mut tuples: Vec<Vec<usize>>,
    ) -> Self {
        let arity = header.arity();
```

  After (the rest of the body unchanged):

```rust
    fn from_tuples_with_build_mode(
        header: RelationHeader, mode: ColumnTrieBuildMode, tuples: impl Into<Tuples>,
    ) -> Self {
        // One `Vec` per tuple, the form this build takes until it builds
        // from row slices (#111).
        let mut tuples = Tuples::into_vecs(tuples.into());
        let arity = header.arity();
```

  **HashTrie** (`kermit-ds/src/ds/hash_trie/implementation.rs`). Import, before (line 34):

```rust
    kermit_iters::{ConfigOption, HashStrategy, JoinIterable, LayoutOption, SipHashStrategy},
```

  After:

```rust
    kermit_iters::{
        ConfigOption, HashStrategy, JoinIterable, LayoutOption, SipHashStrategy, Tuples,
    },
```

  `Relation`, before (lines 385–407):

```rust
    fn from_tuples(header: RelationHeader, tuples: Vec<Vec<usize>>) -> Self {
        Self::from_tuples_with_config(header, HashTrieConfig::default(), tuples)
    }

    fn insert(&mut self, tuple: Vec<usize>) {
        assert_eq!(
            tuple.len(),
            self.header.arity(),
            "tuple arity {} does not match relation arity {}",
            tuple.len(),
            self.header.arity()
        );
        let arity = self.header.arity();
        Self::insert_at(&mut self.root, 0, arity, tuple, self.config.load_factor);
        self.tuple_count += 1;
    }

    fn insert_all(&mut self, tuples: Vec<Vec<usize>>) {
        for tuple in tuples {
            self.insert(tuple);
        }
    }
```

  After:

```rust
    fn from_tuples(header: RelationHeader, tuples: impl Into<Tuples>) -> Self {
        Self::from_tuples_with_config(header, HashTrieConfig::default(), tuples)
    }

    fn insert(&mut self, tuple: impl AsRef<[usize]>) {
        let tuple = tuple.as_ref();
        assert_eq!(
            tuple.len(),
            self.header.arity(),
            "tuple arity {} does not match relation arity {}",
            tuple.len(),
            self.header.arity()
        );
        let arity = self.header.arity();
        Self::insert_at(
            &mut self.root,
            0,
            arity,
            tuple.to_vec(),
            self.config.load_factor,
        );
        self.tuple_count += 1;
    }

    fn insert_all(&mut self, tuples: impl Into<Tuples>) {
        let tuples: Tuples = tuples.into();
        for tuple in tuples.rows() {
            self.insert(tuple);
        }
    }
```

  `ConfigurableRelation`, before (lines 418–422):

```rust
    fn from_tuples_with_config(
        header: RelationHeader, config: HashTrieConfig, tuples: Vec<Vec<usize>>,
    ) -> Self {
        Self::from_tuples_in_bulk(header, config, tuples)
    }
```

  After:

```rust
    fn from_tuples_with_config(
        header: RelationHeader, config: HashTrieConfig, tuples: impl Into<Tuples>,
    ) -> Self {
        // One `Vec` per tuple, the form this build takes until it holds row
        // ids (#111).
        Self::from_tuples_in_bulk(header, config, Tuples::into_vecs(tuples.into()))
    }
```

  The inherent constructor, before (lines 456–460):

```rust
    pub fn from_tuples_with_config_and_build_mode(
        header: RelationHeader, config: HashTrieConfig, mode: HashTrieBuildMode,
        tuples: Vec<Vec<usize>>,
    ) -> Self {
        match mode {
```

  After (the `match` and every arm unchanged):

```rust
    pub fn from_tuples_with_config_and_build_mode(
        header: RelationHeader, config: HashTrieConfig, mode: HashTrieBuildMode,
        tuples: impl Into<Tuples>,
    ) -> Self {
        // One `Vec` per tuple, the form these builds take until they hold
        // row ids (#111).
        let tuples = Tuples::into_vecs(tuples.into());
        match mode {
```

  `BuildModeRelation`, before (lines 621–623):

```rust
    fn from_tuples_with_build_mode(
        header: RelationHeader, mode: HashTrieBuildMode, tuples: Vec<Vec<usize>>,
    ) -> Self {
```

  After (the body, a call to the inherent constructor, unchanged):

```rust
    fn from_tuples_with_build_mode(
        header: RelationHeader, mode: HashTrieBuildMode, tuples: impl Into<Tuples>,
    ) -> Self {
```

  `ConfiguredBuildModeRelation`, before (lines 639–644):

```rust
    fn from_tuples_with_config_and_build_mode(
        header: RelationHeader, config: HashTrieConfig, mode: HashTrieBuildMode,
        tuples: Vec<Vec<usize>>,
    ) -> Self {
        HashTrie::<H, P, E>::from_tuples_with_config_and_build_mode(header, config, mode, tuples)
    }
```

  After:

```rust
    fn from_tuples_with_config_and_build_mode(
        header: RelationHeader, config: HashTrieConfig, mode: HashTrieBuildMode,
        tuples: impl Into<Tuples>,
    ) -> Self {
        HashTrie::<H, P, E>::from_tuples_with_config_and_build_mode(header, config, mode, tuples)
    }
```

  `Projectable::project` (lines 649–675) is unchanged: it passes an annotated `Vec<Vec<usize>>`.

- [ ] **Step 6: Fix the call sites that stop compiling.** These are workspace-wide (every crate, `src`
  and `tests`, inline `#[cfg(test)]` modules, doctests). Each one hands an un-annotated `.collect()` to
  a constructor: `?B: FromIterator<_> + Into<Tuples>` is ambiguous (E0283/E0282). Annotate each one.

  1. `kermit-ds/tests/common/macros.rs:508` (`seek_high_fan_out_lands_on_least_upper_bound`) and
     `:544` (`seek_varied_distances_land_on_least_upper_bound`). Before:
     ```rust
                     let tuples = (0..FAN_OUT).map(|i| vec![1, 2 * i]).collect();
     ```
     After:
     ```rust
                     let tuples: Vec<Vec<usize>> = (0..FAN_OUT).map(|i| vec![1, 2 * i]).collect();
     ```
  2. `kermit-ds/tests/common/macros.rs:637`. Before:
     ```rust
                 let tuples = (0..FAN_OUT).map(|k| vec![k]).collect();
     ```
     After:
     ```rust
                 let tuples: Vec<Vec<usize>> = (0..FAN_OUT).map(|k| vec![k]).collect();
     ```
  3. `kermit-ds/src/ds/column_trie/implementation.rs:925–926`
     (`seek_hands_the_strategy_only_the_unpassed_keys`). Before:
     ```rust
             let trie: ColumnTrie<SpySeek> =
                 ColumnTrie::from_tuples(1.into(), (0..10).map(|k| vec![k]).collect());
     ```
     After:
     ```rust
             let trie: ColumnTrie<SpySeek> =
                 ColumnTrie::from_tuples(1.into(), (0..10).map(|k| vec![k]).collect::<Vec<_>>());
     ```
  4. `kermit-ds/src/ds/column_trie/implementation.rs:1013–1014` (`more_tuples_means_more_heap`).
     Before:
     ```rust
             let large: ColumnTrie =
                 ColumnTrie::from_tuples(2.into(), (0..100).map(|i| vec![i, i + 1]).collect());
     ```
     After:
     ```rust
             let rows: Vec<Vec<usize>> = (0..100).map(|i| vec![i, i + 1]).collect();
             let large: ColumnTrie = ColumnTrie::from_tuples(2.into(), rows);
     ```
  5. `kermit-ds/src/ds/tree_trie/tests.rs:289–290`
     (`seek_hands_the_strategy_only_the_unpassed_siblings`). Before:
     ```rust
         let trie: TreeTrie<SpySeek> =
             TreeTrie::from_tuples(1.into(), (0..10).map(|k| vec![k]).collect());
     ```
     After:
     ```rust
         let trie: TreeTrie<SpySeek> =
             TreeTrie::from_tuples(1.into(), (0..10).map(|k| vec![k]).collect::<Vec<_>>());
     ```
  6. `kermit-ds/src/ds/hash_trie/implementation.rs:1802` (`check_root_is_presized`). Before:
     ```rust
                     let unary = (0..n).map(|k| vec![k]).collect();
     ```
     After:
     ```rust
                     let unary: Vec<Vec<usize>> = (0..n).map(|k| vec![k]).collect();
     ```
  7. `kermit/tests/result_allocation.rs:553–558` (`descent_relations`). Before:
     ```rust
         let r = (0..=dead_ends).map(|x| vec![x, x]).collect();
         let s = (0..DESCENT_ROWS)
             .map(|z| vec![0, z])
             .chain((1..=dead_ends).map(|x| vec![x, DESCENT_ROWS]))
             .collect();
         let t = (0..DESCENT_ROWS).map(|z| vec![z]).collect();
     ```
     After:
     ```rust
         let r: Vec<Vec<usize>> = (0..=dead_ends).map(|x| vec![x, x]).collect();
         let s: Vec<Vec<usize>> = (0..DESCENT_ROWS)
             .map(|z| vec![0, z])
             .chain((1..=dead_ends).map(|x| vec![x, DESCENT_ROWS]))
             .collect();
         let t: Vec<Vec<usize>> = (0..DESCENT_ROWS).map(|z| vec![z]).collect();
     ```

  **Checked and compiling unchanged** (do not edit):
  - **Empty literals.** Each infers its element type through the single `From<Vec<Vec<usize>>>` impl:
    the reflexive `From<Tuples>` cannot unify with `Vec<_>`, so exactly one impl applies. If one is
    flagged anyway (E0282/E0283 after someone added a second `From<Vec<_>>`), write
    `Vec::<Vec<usize>>::new()` there.
    - `vec![]` arguments: `kermit/src/db/validation.rs:609`, `kermit-algos/src/hash/selection.rs:367`,
      `kermit-algos/src/sorted/leapfrog_triejoin.rs:547`, `kermit-ds/src/ds/hash_trie/parallel.rs:636`
      and `:638`, `kermit-ds/src/ds/hash_trie/implementation.rs:925`,
      `kermit-ds/src/ds/column_trie/implementation.rs:845`.
    - `vec![vec![]; 3]` at `column_trie/implementation.rs:857`.
  - **`vec![vec![…]]` literals:** every `vec![vec![…]]` argument (about 500 sites).
  - **Annotated or helper-typed bindings:**
    - helper return types: `edges()`, `s_tuples`, `reversed_r`, `mutual_edges`, `identity::{rows, inputs}`, `random_tuples`, `Relations`;
    - typed parameters: `kermit/tests/common/utils.rs:231,309`, `column_orders_any.rs:178`, `column_orders_equivalence.rs:98,114`;
    - `kermit/src/{db.rs,db/database.rs,execution.rs,lib.rs}`, whose arguments are all `Vec<Vec<usize>>`-typed.
  - **`PendingChild::from_tuples`** (`expansion.rs:207`, `implementation.rs:266,295`, `bulk.rs:133`):
    a different, crate-private trait, unchanged in T3.
  - **Single-tuple inserts.** Every `insert(vec![…])`, `insert(t.clone())` and `insert(tuple)` call
    (`column_trie/implementation.rs:548–983`, `tree_trie/{tests.rs,implementation.rs}`,
    `hash_trie/implementation.rs:853–1623`, `configured.rs:231`, `built_with.rs:218`):
    `Vec<{integer}>: AsRef<[usize]>` infers through `AsRef<[T]> for Vec<T>`.
  - **Batch inserts:** `insert_all(…)` at `configured.rs:232`, `built_with.rs:219`, `hash_trie/implementation.rs:935,1149,1772`, `macros.rs:1129`.
  - **Not affected:** no code names `from_tuples` or `insert` as a function value or with turbofish, and nothing uses `dyn Relation`.
  - **`compute_join`** (`kermit/src/lib.rs:39–83`) passes a `Vec<Vec<usize>>`; its signature is T4's.

- [ ] **Step 7: Fix the tests whose mixed-arity input now panics in `From`.** A batch carries one arity,
  so `vec![vec![1, 2], vec![3]]` reaching a public constructor now panics in `Tuples::push`
  ("a row of arity 1 in a batch of arity 2") before the build's own arity check. Each of these tests
  checks that a *build* rejects a wrong arity. Keep that, and build the wrong arity another way: a
  uniform batch of arity 3 under a header of arity 2. That reaches the build's existing check
  unchanged ("from_tuples: tuple arity 3 does not match header arity 2"), so each test's `expected`
  string stays as it is. Mixed arity is now pinned by `Tuples`' `mixed_arity_vectors_panic` (T1) and
  `relation::tests::mixed_arity_vectors_are_rejected_before_the_build` (Step 1).

  1. `kermit-ds/src/ds/column_trie/implementation.rs:875–879`. Before:
     ```rust
         #[test]
         #[should_panic(expected = "does not match header arity")]
         fn bulk_build_rejects_a_tuple_of_the_wrong_arity() {
             let _: ColumnTrie = ColumnTrie::from_tuples(2.into(), vec![vec![1, 2], vec![3]]);
         }
     ```
     After:
     ```rust
         /// A batch has one arity (mixed arity is `Tuples`' to reject), so the
         /// wrong arity here is the whole batch's.
         #[test]
         #[should_panic(expected = "does not match header arity")]
         fn bulk_build_rejects_a_tuple_of_the_wrong_arity() {
             let _: ColumnTrie = ColumnTrie::from_tuples(2.into(), vec![vec![1, 2, 3], vec![4, 5, 6]]);
         }
     ```
  2. `kermit-ds/src/ds/hash_trie/parallel.rs:643–652` (`parallel_build_rejects_a_wrong_arity`).
     Before:
     ```rust
                 HashTrieBuildMode::Parallel(threads(2)),
                 vec![vec![1, 2], vec![3]],
     ```
     After:
     ```rust
                 HashTrieBuildMode::Parallel(threads(2)),
                 vec![vec![1, 2, 3], vec![4, 5, 6]],
     ```
  3. `kermit-ds/src/ds/hash_trie/radix.rs:246–255` (`radix_build_rejects_a_wrong_arity`). Before:
     ```rust
                 HashTrieBuildMode::Radix(RadixBits::new(4).unwrap()),
                 vec![vec![1, 2], vec![3]],
     ```
     After:
     ```rust
                 HashTrieBuildMode::Radix(RadixBits::new(4).unwrap()),
                 vec![vec![1, 2, 3], vec![4, 5, 6]],
     ```

  These `should_panic` tests pass unchanged (every `should_panic(expected = …arity…)` in the
  workspace, checked):
  - `hash_trie/implementation.rs:904` `insert_wrong_arity_panics`: `insert(vec![1])` reaches
    `insert`'s own assert ("tuple arity 1 does not match relation arity 2").
  - `hash_trie/implementation.rs:1154` `from_tuples_with_config_wrong_arity_panics`: `vec![vec![1]]`
    is a uniform batch of arity 1, so `assert_arities` fires ("from_tuples: tuple arity 1 …").
  - `hash_trie/parallel.rs:1030`, `hash_trie/bulk.rs:418`: single-arity input, prerequisite messages.
  - These call internal builds that still take `Vec<Vec<usize>>` in T3, so `From` never runs:
    - `hash_trie/bulk.rs:432` `bulk_build_rejects_a_wrong_arity` (mixed arity into `from_tuples_in_bulk`);
    - `tree_trie/implementation.rs:625` `parallel_build_rejects_a_first_tuple_of_the_wrong_arity` and
      `:631` `parallel_build_rejects_mixed_arity` (into `from_tuples_parallel`).

    T5 and T7 must settle them when those functions take a `Tuples`.

- [ ] **Step 8: Build every target, then run the tests.**
  ```bash
  CARGO_BUILD_JOBS=2 cargo build --workspace --all-targets
  ```
  Expected: success. Any remaining E0282/E0283 is a call site Step 6 missed: annotate it the same way
  and add it to the task report.
  ```bash
  CARGO_BUILD_JOBS=2 cargo test -p kermit-ds --lib relation::tests
  CARGO_BUILD_JOBS=2 cargo test -p kermit-iters
  CARGO_BUILD_JOBS=2 cargo test -p kermit-ds
  CARGO_BUILD_JOBS=2 cargo test -p kermit-algos
  CARGO_BUILD_JOBS=2 cargo test -p kermit
  ```
  Expected:
  - `every_constructor_takes_a_batch` and `mixed_arity_vectors_are_rejected_before_the_build` pass;
  - every crate passes with 0 failed, including the doctests and the kermit-ds identity tests;
  - `cargo test -p kermit` (the join suites) is the longest step.

- [ ] **Step 9: Lint, format, document.**
  ```bash
  CARGO_BUILD_JOBS=2 cargo clippy --all-targets -- -D warnings
  nix develop --command cargo fmt --all
  RUSTDOCFLAGS=-Dwarnings cargo doc --workspace --no-deps
  ```
  Expected: all clean. `cargo fmt` may rewrap the `insert_at(…)` call and the three edited test
  lines. The `[`Tuples`]` links in `relation.rs` resolve through its new import.

- [ ] **Step 10: Commit.**
  ```bash
  git -C $WT add kermit-ds/src/relation.rs kermit-ds/src/configured.rs kermit-ds/src/built_with.rs \
    kermit-ds/src/ds/tree_trie/implementation.rs kermit-ds/src/ds/tree_trie/tests.rs \
    kermit-ds/src/ds/column_trie/implementation.rs kermit-ds/src/ds/hash_trie/implementation.rs \
    kermit-ds/src/ds/hash_trie/parallel.rs kermit-ds/src/ds/hash_trie/radix.rs \
    kermit-ds/tests/common/macros.rs kermit/tests/result_allocation.rs
  git -C $WT commit -m "refactor(ds)!: relation constructors take impl Into<Tuples> (#111)

  Relation::from_tuples / insert_all, from_tuples_with_config,
  from_tuples_with_build_mode and from_tuples_with_config_and_build_mode
  take impl Into<Tuples>; insert takes impl AsRef<[usize]>. Configured and
  BuiltWith forward. Each structure converts at its seam with
  Tuples::into_vecs, so every build is unchanged until it moves to rows.

  Callers keep passing Vec<Vec<usize>>. Un-annotated collect() arguments
  are annotated. Three wrong-arity tests now pass a uniform batch of the
  wrong arity, since a mixed-arity input panics while it converts.

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_015P4eTR34w48GpD7DvPPaee"
  ```

---

### Task 4: Readers return one buffer; the binary hands `Tuples` over

**Precondition.** T1–T3 have landed: `kermit_iters::{Tuples, RowId, LeafRows}` exist and
`kermit_ds` re-exports them; every Relation-family constructor takes `impl Into<Tuples>`
and `insert` takes `impl AsRef<[usize]>`. Line numbers below are at `47950f0`; each edit
quotes enough text to find it after T1–T3 moved lines.

**Files:**
- Modify: `kermit-ds/src/relation.rs` (`read_csv` 438–468, `read_parquet` 499–548; tests
  716–771). The `RelationFileExt` blanket impl (550–567) is **unchanged**: it hands the
  reader's `Tuples` to `R::from_tuples`, which takes `impl Into<Tuples>` since T3.
- Modify: `kermit-algos/src/orient.rs` (`IndexSpec::permute` 62–68 removed,
  `permute_all` 70–73; imports 17–26; test 513–524).
- Modify: `kermit/src/execution.rs` (imports 35–39; `SortedTrieRelation::build_with`
  107–108, 126–130, 153–157; `RelationFamily::build_relation` 372; `load_with_tuples`
  388–404; `read_relation` 461–476; `ExecutionFamily::build_from_tuples` 503–507;
  `add_index` 554–561; `SortedTrieFamily` 598–600 and 609; `HashTrieFamily` 664–671;
  `TrieLftj` 718–720, 753, 778–784; `HashHtj` 822–824, 859, 884–890; tests listed in
  Step 11).
- Modify: `kermit/src/db/database.rs` (imports 3–8, `build_index` 158–168, tests 319,
  358, 396, 405).
- Modify: `kermit/src/db.rs` (test imports 418, test helper `add_copies` 826–838).
- Modify: `kermit/src/lib.rs` (imports 22–26, `compute_join` 28–56).
- Modify: `kermit/src/bench/ds.rs` (import 16, 40–44).
- Modify: `kermit/src/bench/run.rs` (import 25, 97–136, 284–288).
- Modify: `kermit/src/main.rs` (import 19, `build_join_runner` 626).
- Test (modify): `kermit/tests/common/utils.rs` (imports 18–21, `Inputs` 214, `test_join`
  308), `kermit/tests/subject_position_constant.rs` (35, 53),
  `kermit/tests/column_orders_any.rs` (16–19, 168, 400),
  `kermit/tests/column_orders_equivalence.rs` (16–19, 97),
  `kermit/tests/result_allocation.rs` (57–61, 208).

Every test that constructs or receives one of the changed types (all edited below):

| Test site | Why it changes |
|---|---|
| `kermit-ds/src/relation.rs`: `read_csv_returns_header_and_tuples`, `empty_parquet_keeps_its_arity_from_the_schema` | compare a `Tuples` |
| `kermit-algos/src/orient.rs`: `specs_permute_tuples_and_describe_themselves` | `permute` removed, `permute_all(&Tuples)` |
| `kermit/src/execution.rs` tests at 1261, 1310–1326, 1346, 1368, 1418, 1445–1446, 1471, 1488, 1512, 1546, 1620–1626 (`Spy::build_with`), 1641, 1686, 1840, 1900, 2080, 2093, 2136 | `build_relation(_, Tuples)`, `build_from_tuples(Vec<(_, Tuples)>)`, `add_index(.., &Tuples)`, `load_with_tuples -> (_, Tuples)` |
| `kermit/src/db/database.rs` tests at 319, 358, 396, 405 | `build_index(.., &Tuples)` |
| `kermit/src/db.rs` test helper `add_copies` (826–838), used by `under_any_a_cyclic_query_runs_over_a_copy`, `the_planner_sees_base_names_under_any`, `a_selection_on_a_reoriented_atom_keeps_its_equality` | `build_index(.., &Tuples)` |
| `kermit/tests/common/utils.rs` (`Inputs`, `load_parquet_relations`, `test_join`, `join_under_planner`); through them every `mod common;` test binary, incl. `watdiv_correctness.rs`, `lubm_cardinalities.rs`, `lubm_mini_oracle.rs` (types flow, no edit) | `Inputs` holds `Tuples` |
| `kermit/tests/subject_position_constant.rs` `join`, `kermit/tests/column_orders_any.rs` `check` and `a_symmetric_relation_still_gets_its_redundant_copy`, `kermit/tests/column_orders_equivalence.rs` `rows` | build an `Inputs` / call `permute_all` |
| `kermit/tests/result_allocation.rs` `database_with_copies` | `build_index(.., &Tuples)` |

- [ ] **Step 1: Write the failing reader tests.** In `kermit-ds/src/relation.rs` `mod tests`:
  replace `read_csv_returns_header_and_tuples` (762–771) with the version below, add
  `assert_eq!(tuples.arity(), 2);` after `assert!(tuples.is_empty());` in
  `empty_parquet_keeps_its_arity_from_the_schema` (758), and append the four new tests
  before the closing `}` of the module:

```rust
    #[test]
    fn read_csv_returns_header_and_tuples() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("edge.csv");
        std::fs::write(&path, "a,b\n1,2\n3,4\n").unwrap();
        let (header, tuples) = read_csv(&path).unwrap();
        assert_eq!(header.name(), "edge");
        assert_eq!(header.arity(), 2);
        assert_eq!(tuples, Tuples::from(vec![vec![1, 2], vec![3, 4]]));
    }

    /// Both readers hand back a buffer at its exact size: a `HashTrie` keeps
    /// it, so its `space` must not depend on how the reader grew it (#111).
    /// Five 2-ary rows are 10 values, which a doubling buffer holds in 16.
    #[test]
    fn readers_return_a_buffer_sized_to_its_rows() {
        let dir = tempfile::tempdir().unwrap();
        let rows: Vec<Vec<i64>> = (0..5).map(|i| vec![i, 10 + i]).collect();
        let csv = dir.path().join("edge.csv");
        let body: String = rows.iter().map(|r| format!("{},{}\n", r[0], r[1])).collect();
        std::fs::write(&csv, format!("a,b\n{body}")).unwrap();
        let parquet = dir.path().join("edge.parquet");
        write_parquet(&parquet, &["a", "b"], &rows);
        for (reader, tuples) in [
            ("read_csv", read_csv(&csv).unwrap().1),
            ("read_parquet", read_parquet(&parquet).unwrap().1),
        ] {
            assert_eq!(tuples.len(), 5, "{reader}");
            assert_eq!(tuples.arity(), 2, "{reader}");
            assert_eq!(
                tuples.heap_size_bytes(),
                5 * 2 * std::mem::size_of::<usize>(),
                "{reader}"
            );
        }
    }

    /// More rows than one 1 024-row record batch, so the transpose writes at
    /// three offsets into the buffer, and every row keeps file order.
    #[test]
    fn read_parquet_transposes_every_batch_in_file_order() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("triple.parquet");
        let rows: Vec<Vec<i64>> = (0..2_500).map(|i| vec![i, 7 * i % 13, 2_500 - i]).collect();
        write_parquet(&path, &["s", "p", "o"], &rows);
        let (header, tuples) = read_parquet(&path).unwrap();
        assert_eq!(header.arity(), 3);
        let want: Vec<Vec<usize>> = rows
            .iter()
            .map(|row| row.iter().map(|&v| usize::try_from(v).unwrap()).collect())
            .collect();
        assert_eq!(tuples.to_vecs(), want);
    }

    /// A negative value has no `usize`, in any column.
    #[test]
    fn read_parquet_rejects_a_negative_value() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("signed.parquet");
        write_parquet(&path, &["a", "b"], &[vec![1, 2], vec![3, -4]]);
        assert!(matches!(
            read_parquet(&path),
            Err(RelationError::InvalidData(_))
        ));
    }

    /// A header-only CSV is an empty batch of the header's arity, as an empty
    /// Parquet file is of its schema's.
    #[test]
    fn an_empty_csv_keeps_its_arity_from_the_header() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("empty.csv");
        std::fs::write(&path, "s,o\n").unwrap();
        let (header, tuples) = read_csv(&path).unwrap();
        assert_eq!(header.arity(), 2);
        assert!(tuples.is_empty());
        assert_eq!(tuples.arity(), 2);
    }
```

- [ ] **Step 2: Run them to verify they fail.**
  ```bash
  CARGO_BUILD_JOBS=2 cargo test -p kermit-ds --lib relation::tests
  ```
  Expected: compile errors in `relation::tests`, e.g. `no method named heap_size_bytes
  found for struct Vec<Vec<usize>>`, `no method named to_vecs`, `no method named arity`,
  and `mismatched types` for `assert_eq!(tuples, Tuples::from(..))`.

- [ ] **Step 3: Implement the readers.** `relation.rs` already imports
  `kermit_iters::Tuples` since T3 (its trait signatures name it); confirm with
  `grep -n 'Tuples' kermit-ds/src/relation.rs | head -3`. Replace `read_csv` (438–468, doc
  included) with:

```rust
/// Reads a CSV file into a header (attribute names from the header row,
/// relation name from the file stem) and its tuples, one row-major buffer in
/// file order. Shared by [`RelationFileExt::from_csv`] and by callers that
/// need to build with a non-default configuration
/// ([`ConfigurableRelation::from_tuples_with_config`]).
///
/// Every record is parsed through one reused [`csv::StringRecord`] straight
/// into the buffer, so nothing is allocated per row (#111). The buffer grows
/// by doubling and is shrunk to its exact size once, at the end.
///
/// # Errors
///
/// Same conditions as [`RelationFileExt::from_csv`].
pub fn read_csv<P: AsRef<Path>>(filepath: P) -> Result<(RelationHeader, Tuples), RelationError> {
    let (header, mut rdr) = open_csv(filepath.as_ref())?;
    let arity = header.arity();
    let mut data: Vec<usize> = Vec::new();
    let mut rows = 0;
    let mut record = csv::StringRecord::new();
    // `flexible(false)` (see `open_csv`) makes every record as wide as the
    // header, so the buffer holds exactly `arity` values per row.
    while rdr.read_record(&mut record)? {
        for (col_idx, field) in record.iter().enumerate() {
            let value = field.parse::<usize>().map_err(|_| {
                RelationError::InvalidData(format!(
                    "row {rows}, column {col_idx}: cannot parse {field:?} as usize",
                ))
            })?;
            data.push(value);
        }
        rows += 1;
    }
    let mut tuples = Tuples::from_flat(arity, rows, data);
    // A `HashTrie` keeps this buffer, so its `space` must not depend on how
    // the reader grew it: one exact reallocation, outside every timed region.
    tuples.shrink_to_fit();
    Ok((header, tuples))
}
```

  Replace `read_parquet` (499–548, doc included) with:

```rust
/// Reads a Parquet file into a header (column names from the schema,
/// relation name from the file stem) and its tuples, one row-major buffer in
/// file order. Counterpart of [`read_csv`]. Shared by
/// [`RelationFileExt::from_parquet`] and by callers that need to build with a
/// non-default configuration ([`ConfigurableRelation::from_tuples_with_config`]).
///
/// The footer's row count sizes the buffer once, and each record batch's
/// columns are transposed into it, so nothing is allocated per row (#111).
///
/// # Errors
///
/// Same conditions as [`RelationFileExt::from_parquet`].
pub fn read_parquet<P: AsRef<Path>>(
    filepath: P,
) -> Result<(RelationHeader, Tuples), RelationError> {
    let (header, builder) = open_parquet(filepath.as_ref())?;
    let arity = header.arity();
    let expected_rows =
        usize::try_from(builder.metadata().file_metadata().num_rows()).unwrap_or(0);
    let reader = builder.build()?;
    let mut data: Vec<usize> = Vec::with_capacity(expected_rows * arity);
    let mut rows = 0;
    for batch_result in reader {
        let batch = batch_result?;
        // A batch is columnar and the buffer row-major: column `c` fills
        // position `c` of each of the batch's rows.
        let start = data.len();
        data.resize(start + batch.num_rows() * arity, 0);
        let block = &mut data[start..];
        for (col_idx, column) in batch.columns().iter().enumerate() {
            let values = column
                .as_primitive::<arrow::datatypes::Int64Type>()
                .values();
            for (row_idx, &value) in values.iter().enumerate() {
                block[row_idx * arity + col_idx] = usize::try_from(value).map_err(|_| {
                    RelationError::InvalidData("failed to convert Parquet value to usize".into())
                })?;
            }
        }
        rows += batch.num_rows();
    }
    let mut tuples = Tuples::from_flat(arity, rows, data);
    // A no-op when the footer's count was right; otherwise the one exact
    // reallocation `read_csv` also makes.
    tuples.shrink_to_fit();
    Ok((header, tuples))
}
```

- [ ] **Step 4: Run the kermit-ds tests.**
  ```bash
  CARGO_BUILD_JOBS=2 cargo test -p kermit-ds --lib relation::tests
  CARGO_BUILD_JOBS=2 cargo test -p kermit-ds
  ```
  Expected: all pass (`relation::tests` 19 tests, of which 4 new). `from_csv_rejects_non_integer_values`
  still finds `row 1` and `column 1`: `rows` is the 0-based index `enumerate` gave.
  The `parquet_test_suite!` round-trips (through `from_parquet`, the unchanged blanket
  impl) pin that every structure still loads what the readers return.

- [ ] **Step 5: Write the failing `permute_all` tests.** In `kermit-algos/src/orient.rs`
  replace `specs_permute_tuples_and_describe_themselves` (513–524) with:

```rust
    #[test]
    fn specs_permute_tuples_and_describe_themselves() {
        let spec = IndexSpec::new("edge", vec![1, 0]);
        let copy = spec.permute_all(&Tuples::from(vec![vec![1, 2], vec![3, 4]]));
        assert_eq!(copy, Tuples::from(vec![vec![2, 1], vec![4, 3]]));
        // One buffer at its exact size: four values, one allocation.
        assert_eq!(copy.heap_size_bytes(), 4 * std::mem::size_of::<usize>());
        assert_eq!(spec.describe(), "edge (1, 0)");
        assert!(is_index_predicate(&spec.name));
        assert!(!is_index_predicate("edge"));
    }

    /// A 3-ary rotation keeps file order, and the copy of an empty base has
    /// the copy's arity, whatever arity the empty batch carried.
    #[test]
    fn permute_all_keeps_file_order_and_the_copys_arity() {
        let spec = IndexSpec::new("r", vec![2, 0, 1]);
        let base = Tuples::from(vec![vec![1, 2, 3], vec![4, 5, 6]]);
        assert_eq!(spec.permute_all(&base).to_vecs(), vec![
            vec![3, 1, 2],
            vec![6, 4, 5]
        ]);
        let empty = spec.permute_all(&Tuples::from(Vec::<Vec<usize>>::new()));
        assert!(empty.is_empty());
        assert_eq!(empty.arity(), 3);
    }
```

- [ ] **Step 6: Run to verify they fail.**
  ```bash
  CARGO_BUILD_JOBS=2 cargo test -p kermit-algos --lib orient::tests
  ```
  Expected: compile error `cannot find type Tuples in this scope` (and, once imported,
  `mismatched types: expected &[Vec<usize>], found &Tuples`).

- [ ] **Step 7: Implement `permute_all`.** In the module `use` block (17–26) add
  `kermit_iters::Tuples,` after the `crate::{..}` entry. Replace `permute` and
  `permute_all` (61–73) with the one method below. `permute` had no caller outside
  `permute_all` and the test replaced in Step 5, and it allocated a `Vec` per tuple:

```rust
    /// Every tuple of the base relation, reordered into the copy's columns,
    /// in the same order: one buffer at its exact size, so a copy costs one
    /// allocation however many tuples it holds (#111).
    pub fn permute_all(&self, tuples: &Tuples) -> Tuples {
        let arity = self.permutation.len();
        let mut data = Vec::with_capacity(tuples.len() * arity);
        for tuple in tuples.rows() {
            data.extend(self.permutation.iter().map(|&column| tuple[column]));
        }
        Tuples::from_flat(arity, tuples.len(), data)
    }
```

- [ ] **Step 8: Run.**
  ```bash
  CARGO_BUILD_JOBS=2 cargo test -p kermit-algos --lib orient::tests
  ```
  Expected: all `orient::tests` pass (one new). The workspace does not compile yet:
  `kermit` still passes `&[Vec<usize>]`. Steps 9–11 fix that.

- [ ] **Step 9: Plumb `Tuples` through `kermit/src/execution.rs`.** This is a pure
  refactor of the binary's types: no build, count or label changes. Each edit:

  Imports (35–39): add `Tuples` to the `kermit_ds::{..}` list:
```rust
    kermit_ds::{
        BuildModeRelation, Cardinality, ColumnTrie, ColumnTrieBuildMode, ExpansionPolicy, HashTrie,
        HashTrieBuildMode, HashTrieConfig, HeapSize, IndexStructure, PruningPolicy, Relation,
        RelationFileExt, RelationHeader, SeekStrategy, TreeTrie, TreeTrieBuildMode, Tuples,
    },
```

  `SortedTrieRelation::build_with` (107–108), before / after:
```rust
    /// Builds one relation from `tuples` by `build`.
    fn build_with(header: RelationHeader, build: Self::BuildMode, tuples: Vec<Vec<usize>>) -> Self;
```
```rust
    /// Builds one relation from `tuples` by `build`.
    fn build_with(header: RelationHeader, build: Self::BuildMode, tuples: Tuples) -> Self;
```

  The `TreeTrie<S>` impl (126–130) and the `ColumnTrie<S>` impl (153–157): in each
  `fn build_with(header: RelationHeader, build: …BuildMode, tuples: Vec<Vec<usize>>,) -> Self`
  replace `tuples: Vec<Vec<usize>>` with `tuples: Tuples`; the bodies
  (`Self::from_tuples_with_build_mode(header, build, tuples)`) are unchanged.

  `RelationFamily::build_relation` (372), before / after:
```rust
    fn build_relation(&self, header: RelationHeader, tuples: Vec<Vec<usize>>) -> Self::Rel;
```
```rust
    fn build_relation(&self, header: RelationHeader, tuples: Tuples) -> Self::Rel;
```
  `load` (383–386) is unchanged; its types now flow from `read_relation`.

  `load_with_tuples` (388–404), replace doc and body with:
```rust
    /// Loads one relation file like [`load`](Self::load), and also returns
    /// the tuples it was built from, in the order the reader produced them.
    ///
    /// These are the input of every metric that rebuilds a relation
    /// (`insertion`, `end_to_end`). A structure's own iteration order is not
    /// neutral input — it is the sorted tries' best case, and it made the
    /// hash trie's build quadratic (issue #66) — whereas file order is the
    /// same for every structure and every Layout, so all of them build from
    /// identical input. The relation is built from a copy of the reader's
    /// buffer, one allocation, and the reader's buffer is returned (#111).
    ///
    /// # Errors
    ///
    /// As [`load`](Self::load).
    fn load_with_tuples(&self, path: &Path) -> anyhow::Result<(Self::Rel, Tuples)> {
        let (header, tuples) = read_relation(path)?;
        Ok((self.build_relation(header, tuples.clone()), tuples))
    }
```

  `read_relation` (468), before / after (body unchanged):
```rust
fn read_relation(path: &Path) -> anyhow::Result<(RelationHeader, Vec<Vec<usize>>)> {
```
```rust
fn read_relation(path: &Path) -> anyhow::Result<(RelationHeader, Tuples)> {
```

  `ExecutionFamily::build_from_tuples` (507), before / after:
```rust
    fn build_from_tuples(&self, inputs: Vec<(RelationHeader, Vec<Vec<usize>>)>) -> Self::Engine;
```
```rust
    fn build_from_tuples(&self, inputs: Vec<(RelationHeader, Tuples)>) -> Self::Engine;
```

  `ExecutionFamily::add_index` (558–561), before / after:
```rust
    fn add_index(
        &self, engine: &mut Self::Engine, spec: IndexSpec, base: &RelationHeader,
        tuples: &[Vec<usize>],
    );
```
```rust
    fn add_index(
        &self, engine: &mut Self::Engine, spec: IndexSpec, base: &RelationHeader, tuples: &Tuples,
    );
```

  `SortedTrieFamily` (598–600 and 609), after:
```rust
    fn build_relation(&self, header: RelationHeader, tuples: Tuples) -> R {
        R::build_with(header, self.build, tuples)
    }
```
```rust
    /// The trie's stored count ([`Cardinality`], a bound of
    /// `SortedTrieRelation`), so counting walks and allocates nothing.
    fn tuple_count(rel: &R) -> usize { Cardinality::tuple_count(rel) }
```
  (before: `fn tuple_count(rel: &R) -> usize { rel.trie_iter().into_iter().count() }`;
  both count distinct tuples, pinned by `scan_agrees_with_tuple_count_in_every_family`
  and kermit-ds's `tuple_count_matches_iteration_count` tests.)

  `HashTrieFamily::build_relation` (664–671), after:
```rust
    fn build_relation(&self, header: RelationHeader, tuples: Tuples) -> HashTrie<H, P, E> {
        HashTrie::<H, P, E>::from_tuples_with_config_and_build_mode(
            header,
            self.config,
            self.build,
            tuples,
        )
    }
```

  `TrieLftj` (718–720, 753, 778–784), after:
```rust
    fn build_relation(&self, header: RelationHeader, tuples: Tuples) -> R {
        self.structure.build_relation(header, tuples)
    }
```
```rust
    fn build_from_tuples(&self, inputs: Vec<(RelationHeader, Tuples)>) -> Self::Engine {
```
  (the body is unchanged)
```rust
    fn add_index(
        &self, engine: &mut Self::Engine, spec: IndexSpec, base: &RelationHeader, tuples: &Tuples,
    ) {
        let copy = self.build_relation(index_header(&spec, base), spec.permute_all(tuples));
        engine.add_index(spec, copy);
    }
```

  `HashHtj` (822–824, 859, 884–890), after:
```rust
    fn build_relation(&self, header: RelationHeader, tuples: Tuples) -> HashTrie<H, P, E> {
        self.structure.build_relation(header, tuples)
    }
```
```rust
    fn build_from_tuples(&self, inputs: Vec<(RelationHeader, Tuples)>) -> Self::Engine {
```
  (the body is unchanged)
```rust
    fn add_index(
        &self, engine: &mut Self::Engine, spec: IndexSpec, base: &RelationHeader, tuples: &Tuples,
    ) {
        let copy = self.build_relation(index_header(&spec, base), spec.permute_all(tuples));
        engine.add_index(spec, copy);
    }
```

- [ ] **Step 10: Plumb the rest of the binary and the library.**

  `kermit/src/db/database.rs` imports (6): `kermit_ds::{Cardinality, Relation, RelationHeader, Tuples},`.
  `build_index` (164–168), after (doc 158–163 unchanged):
```rust
pub fn build_index<R: Relation>(spec: &IndexSpec, base: &RelationHeader, tuples: &Tuples) -> R {
    R::from_tuples(index_header(spec, base), spec.permute_all(tuples))
}
```
  Its tests: at 319, 358, 396 and 405 replace `build_index(&spec, &header, &edges())` with
  `build_index(&spec, &header, &Tuples::from(edges()))` (`Tuples` reaches the test module
  through `use super::*`; `edges()` stays a `Vec`, which `store` also takes).

  `kermit/src/db.rs` test imports (418): `kermit_ds::{HashTrie, Relation, TreeTrie, Tuples},`.
  In `add_copies` (826–838) the helper keeps its `Vec` map, so its three callers are
  unchanged; before / after of the one line (833):
```rust
            let copy = build_index(spec, &base, &tuples[spec.base.as_str()]);
```
```rust
            let copy = build_index(spec, &base, &Tuples::from(tuples[spec.base.as_str()].clone()));
```

  `kermit/src/lib.rs` imports (22–26), after:
```rust
use {
    kermit_algos::{CatalogStats, JoinAlgo, JoinQuery, LexicographicOptimiser, QueryOptimiser},
    kermit_ds::{Relation, Tuples},
    std::collections::HashMap,
};
```
  `compute_join` (28–56): replace the doc's first line and the signature and the
  `relations` binding; the rest of the body (58–86) is unchanged. A `Vec<Vec<Vec<usize>>>`
  argument still compiles (`Vec<Vec<usize>>: Into<Tuples>`), and `compute_join::<R, JA>`
  still names both generics (an argument-position `impl Trait` does not join the
  turbofish):
```rust
/// Convenience function that builds relations from raw tuple batches and runs a
/// join, returning the result tuples.
///
/// Constructs a synthetic Datalog query from the `variables` and
/// `rel_variables` mappings, builds one `R` per input relation, and executes
/// the join via `JA`.
///
/// Calls [`JoinAlgo::join_iter`] directly, so neither the const-view nor
/// the selection rewrite runs: `rel_variables` must contain no repeated
/// index within one entry. Use [`db::lftj_join`] / [`db::hash_join`] for
/// queries that need them.
pub fn compute_join<R, JA>(
    input: Vec<impl Into<Tuples>>, variables: Vec<usize>, rel_variables: Vec<Vec<usize>>,
) -> Vec<Vec<usize>>
where
    R: Relation,
    JA: JoinAlgo<R>,
{
    // Each relation's arity is its batch's: an empty `Vec` converts to an
    // empty batch of arity 0, as `k` was 0 for an empty input before.
    let relations: Vec<_> = input
        .into_iter()
        .map(|tuples| {
            let tuples: Tuples = tuples.into();
            R::from_tuples(tuples.arity().into(), tuples)
        })
        .collect();
```

  `kermit/src/bench/ds.rs` import (16): `kermit_ds::{IndexStructure, Relation, Tuples},`.
  Lines 40–44, after:
```rust
    // The insertion / end-to-end closures below rebuild from the tuples in
    // file order rather than reading them back off the built relation: file
    // order is the same whichever structure and Layout ran, so every one of
    // them is fed identical input (see `RelationFamily::load_with_tuples`).
    // Each setup clone copies one buffer (#111).
    let (relation, tuples): (F::Rel, Tuples) = family.load_with_tuples(relation_path)?;
```
  (the clones at 82, 87, 116 and 124 are unchanged.)

  `kermit/src/bench/run.rs` import (25): `kermit_ds::{HeapSize, Relation, RelationHeader, Tuples},`.
  Line 112, before / after:
```rust
    let mut build_inputs: Vec<(RelationHeader, Vec<Vec<usize>>)> = Vec::new();
```
```rust
    let mut build_inputs: Vec<(RelationHeader, Tuples)> = Vec::new();
```
  Line 125, before / after:
```rust
    let input_of = |base: &str| -> &(RelationHeader, Vec<Vec<usize>>) {
```
```rust
    let input_of = |base: &str| -> &(RelationHeader, Tuples) {
```
  Lines 286–287 (the `insertion` setup), after:
```rust
                    b.iter_batched(
                        // One buffer copy per relation, untimed (#111).
                        || build_inputs.clone(),
```
  `spec.permute_all(tuples)` at 324 now takes the `&Tuples` `input_of` returns; the
  `build_from_tuples(build_inputs.clone())` calls at 235, 356 and 398–404 are unchanged.

  `kermit/src/main.rs` import (19): `kermit_ds::{IndexStructure, Relation, RelationHeader, Tuples},`.
  In `build_join_runner` (626), before / after:
```rust
    let mut inputs: BTreeMap<String, (RelationHeader, Vec<Vec<usize>>)> = BTreeMap::new();
```
```rust
    let mut inputs: BTreeMap<String, (RelationHeader, Tuples)> = BTreeMap::new();
```
  (`family.add_index(&mut engine, spec, header, tuples)` at 643 now passes `&Tuples`.)

- [ ] **Step 11: Update the tests that build these types.**

  `kermit/src/execution.rs` `mod tests` (`Tuples` arrives through `use super::*`):

  | Line | Before | After |
  |---|---|---|
  | 1261 | `family.build_from_tuples(vec![(header, vec![vec![1, 2]])])` | `family.build_from_tuples(vec![(header, Tuples::from(vec![vec![1, 2]]))])` |
  | 1315 and 1326 | `assert_eq!(tuples, file_order);` | `assert_eq!(tuples.to_vecs(), file_order);` |
  | 1346 | `family.build_relation(header, vec![vec![1, 2], vec![1, 2], vec![3, 4]])` | `family.build_relation(header, vec![vec![1, 2], vec![1, 2], vec![3, 4]].into())` |
  | 1368 | `let tuples = \|\| vec![vec![1, 2], vec![1, 2], vec![1, 3], vec![4, 5]];` | `let tuples = \|\| Tuples::from(vec![vec![1, 2], vec![1, 2], vec![1, 3], vec![4, 5]]);` |
  | 1418 | `family.build_relation(header, vec![vec![1, 2]])` | `family.build_relation(header, vec![vec![1, 2]].into())` |
  | 1445–1446 | `let tuples = (0..100).map(\|b\| vec![1, b]).collect();` / `family.build_relation(header, tuples).heap_size_bytes()` | `let tuples: Vec<Vec<usize>> = (0..100).map(\|b\| vec![1, b]).collect();` / `family.build_relation(header, tuples.into()).heap_size_bytes()` |
  | 1471 | `.build_relation(header, vec![vec![1, 1]; 3])` | `.build_relation(header, vec![vec![1, 1]; 3].into())` |
  | 1488 | `family.build_relation(header, vec![vec![1, 2]])` | `family.build_relation(header, vec![vec![1, 2]].into())` |
  | 1512 | `let edges = \|\| vec![vec![1, 2], vec![1, 3], vec![2, 3]];` | `let edges = \|\| Tuples::from(vec![vec![1, 2], vec![1, 3], vec![2, 3]]);` |
  | 1546 | `let edges = vec![vec![1, 2], vec![2, 3], vec![1, 3], vec![3, 4], vec![2, 4]];` | `let edges = Tuples::from(vec![vec![1, 2], vec![2, 3], vec![1, 3], vec![3, 4], vec![2, 4]]);` |
  | 1622 (`Spy::build_with`) | `header: RelationHeader, build: ColumnTrieBuildMode, tuples: Vec<Vec<usize>>,` | `header: RelationHeader, build: ColumnTrieBuildMode, tuples: Tuples,` |
  | 1641 | `let tuples = \|\| vec![vec![1, 2]];` | `let tuples = \|\| Tuples::from(vec![vec![1, 2]]);` |
  | 1686 | `let tuples = \|\| vec![vec![1, 2], vec![2, 1], vec![3, 4]];` | `let tuples = \|\| Tuples::from(vec![vec![1, 2], vec![2, 1], vec![3, 4]]);` |
  | 1840 | `let tuples = \|\| vec![vec![1, 2], vec![1, 3], vec![2, 4]];` | `let tuples = \|\| Tuples::from(vec![vec![1, 2], vec![1, 3], vec![2, 4]]);` |
  | 1900 | `let tuples = \|\| vec![vec![1, 2], vec![2, 1], vec![3, 4]];` | `let tuples = \|\| Tuples::from(vec![vec![1, 2], vec![2, 1], vec![3, 4]]);` |
  | 2080 | `family.build_relation(RelationHeader::new_positional("r", 2), vec![vec![1, 2]])` | `family.build_relation(RelationHeader::new_positional("r", 2), vec![vec![1, 2]].into())` |
  | 2093 | `let edges = vec![vec![1, 2], vec![1, 3], vec![2, 3]];` | `let edges = Tuples::from(vec![vec![1, 2], vec![1, 3], vec![2, 3]]);` |
  | 2136 | `let edges = \|\| vec![vec![1, 2], vec![2, 1]];` | `let edges = \|\| Tuples::from(vec![vec![1, 2], vec![2, 1]]);` |

  (`\|` in the table is a literal `|`.) `Spy::build_with`'s body
  (`<Spy as Relation>::from_tuples(header, tuples)`) is unchanged. Line 1445 is the only
  un-annotated `.collect()` in `kermit/src` or `kermit/tests` that reaches a parameter
  this task retypes (`Tuples` has no `FromIterator`); the collects T3 already annotated
  (`result_allocation.rs` 553–558 and the kermit-ds sites) are not touched again.

  `kermit/tests/common/utils.rs`: imports (18–21) gain `Tuples`:
```rust
    kermit_ds::{
        BuildModeProvider, BuiltWith, Cardinality, ConfigProvider, Configured, ExpansionPolicy,
        HashTrie, HashTrieBuildMode, HashTrieConfig, PruningPolicy, Relation, RelationHeader,
        Tuples,
    },
```
  `Inputs` (214), after:
```rust
pub type Inputs = BTreeMap<String, (RelationHeader, Tuples)>;
```
  `load_parquet_relations` (220–237) is unchanged: `read_parquet` now returns the
  `Tuples` `Inputs` holds. In `test_join` (308), before / after:
```rust
            inputs.insert(format!("R{i}"), (header.clone(), tuples.clone()));
```
```rust
            inputs.insert(format!("R{i}"), (header.clone(), Tuples::from(tuples.clone())));
```

  `kermit/tests/subject_position_constant.rs`: import (35)
  `kermit_ds::{Cardinality, HashTrie, Relation, RelationHeader, TreeTrie, Tuples},`; line 53,
  before / after:
```rust
    let inputs: Inputs = BTreeMap::from([("edge".to_string(), (header.clone(), edges()))]);
```
```rust
    let inputs: Inputs =
        BTreeMap::from([("edge".to_string(), (header.clone(), Tuples::from(edges())))]);
```

  `kermit/tests/column_orders_any.rs`: add `Tuples` to the `kermit_ds::{..}` import
  (16–19). Line 168, before / after:
```rust
            (name.to_string(), (header, tuples.clone()))
```
```rust
            (name.to_string(), (header, Tuples::from(tuples.clone())))
```
  Line 400, before / after (the rest of the test is unchanged):
```rust
    let mut copied = specs[0].permute_all(base);
```
```rust
    let mut copied = specs[0].permute_all(&Tuples::from(base.clone())).to_vecs();
```

  `kermit/tests/column_orders_equivalence.rs`: add `Tuples` to the `kermit_ds::{..}`
  import (16–19). Line 97, before / after:
```rust
        inputs.insert(name.clone(), (header.clone(), tuples.clone()));
```
```rust
        inputs.insert(name.clone(), (header.clone(), Tuples::from(tuples.clone())));
```

  `kermit/tests/result_allocation.rs`: add `Tuples` to the `kermit_ds::{..}` import
  (57–61). Line 208, before / after:
```rust
        let copy = build_index(&spec, &r, &reversed_r());
```
```rust
        let copy = build_index(&spec, &r, &Tuples::from(reversed_r()));
```

- [ ] **Step 12: Run everything this task touched.** Behaviour must not change: the
  pins are `load_with_tuples_returns_file_order` (file order survives),
  `sorted_families_build_with_their_mode` / `tree_trie_families_build_with_their_mode` /
  `hash_families_build_with_their_mode` / `hash_trie_families_build_with_their_parallel_mode`
  (every route still reaches the family's mode, copies included),
  `families_build_copies_through_build_relation`,
  `required_indexes_follow_the_familys_planner`,
  `scan_agrees_with_tuple_count_in_every_family` (the new sorted `tuple_count`),
  `build_index_permutes_the_tuples_in_file_order`, `build_index_keeps_config_and_build_mode`,
  the `column_orders_*` suites, `subject_position_constant`, `result_allocation`, and the
  CLI tests.
  ```bash
  CARGO_BUILD_JOBS=2 cargo check --workspace --all-targets
  CARGO_BUILD_JOBS=2 cargo test -p kermit-ds -p kermit-algos
  CARGO_BUILD_JOBS=2 cargo test -p kermit
  CARGO_BUILD_JOBS=2 cargo clippy --all-targets -- -D warnings
  RUSTDOCFLAGS=-Dwarnings cargo doc --workspace --no-deps
  ```
  Expected: all green; the test counts equal the T3 baseline plus the 5 new tests of
  Steps 1 and 5 (4 in kermit-ds, 1 in kermit-algos; the replaced tests keep their names).

- [ ] **Step 13: Commit.**
  ```bash
  nix develop --command cargo fmt --all
  git add kermit-ds/src/relation.rs kermit-algos/src/orient.rs kermit/src/execution.rs \
    kermit/src/db/database.rs kermit/src/db.rs kermit/src/lib.rs kermit/src/bench/ds.rs \
    kermit/src/bench/run.rs kermit/src/main.rs kermit/tests/common/utils.rs \
    kermit/tests/subject_position_constant.rs kermit/tests/column_orders_any.rs \
    kermit/tests/column_orders_equivalence.rs kermit/tests/result_allocation.rs
  git commit -m "$(cat <<'EOF'
refactor: readers and the binary hand relations over as one Tuples buffer (#111)

read_csv parses into one buffer through a reused record; read_parquet
sizes its buffer from the footer and transposes each record batch; both
shrink to the exact size once. The binary's families, copies
(IndexSpec::permute_all), db::build_index, compute_join and every bench
setup carry Tuples, so each clone is one Vec<usize>. The sorted families
count through Cardinality instead of walking the trie.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_015P4eTR34w48GpD7DvPPaee
EOF
)"
  ```

---
### Notes shared by Tasks 5 and 6

- **State assumed.** T1–T4 have landed. `Tuples`, `RowId` and `LeafRows` exist in
  `kermit-iters` and are re-exported by `kermit-ds`. Every constructor takes
  `impl Into<Tuples>` and `insert` takes `impl AsRef<[usize]>`. TreeTrie and ColumnTrie
  convert with the temporary `Tuples::into_vecs()` shim at the top of their
  constructors.
- **Line numbers** are cited at `47950f0`, before T3. T3 shifts them by a few lines, so
  locate each edit by the function name given with it. Every code step below shows the
  whole function after the edit. Replace the function as it stands after T3, whatever
  T3's shim looks like inside it.
- **`Tuples::sort` is the old comparator's order.** The hand-rolled comparator both
  tries use compares `a[i]` with `b[i]` for `i` in `0..a.len()`. Every row of a batch
  has the same length, so that is plain lexicographic order, the order `Tuples::sort`
  gives: `[usize; N]` and `<[usize]>::cmp` at every arity. T1's seeded test checks this
  against the old comparator at arities 1–5. Both sorts are unstable, but rows that
  compare equal are equal values. So the sorted sequence of values, and every structure
  built from it, is unchanged.
- **Space is unchanged by construction:**
  - **TreeTrie:** a node's children depend only on the tuple set. `Vec::insert` grows a
    list exactly as `push` does (`grow_one` when `len == capacity`), so the trie and its
    capacities do not depend on insertion order. Task 5's new pin test checks this.
  - **ColumnTrie:** `from_sorted` depends only on the sorted sequence. Its existing
    oracle (`insert_one_by_one`, unsorted) already checks this.
- **All commands run from the worktree root.**

---

### Task 5: TreeTrie builds from row slices; `morsel::scatter_rows`

**Files:**
- Modify: `kermit-ds/src/morsel.rs`
  - the module doc (1–18) and `use` (20);
  - add `RowPartition`, `RowBuckets`, `row_id` and `scatter_rows` after `scatter` (which ends at line 161);
  - tests (192–361).
- Modify: `kermit-ds/src/ds/tree_trie/implementation.rs`
  - `use` (1–15);
  - `insert_into_children` (17–38);
  - `first_key_splitters` (140–161);
  - `from_tuples_parallel` / `build_parallel` (180–278);
  - `from_tuples` (292–335), `insert` (337–352), `insert_all` (354–364);
  - `from_tuples_with_build_mode` (367–386);
  - `parallel_build_tests` (487–698).
- Test (unchanged, pins behaviour):
  - `ds::tree_trie::implementation::parallel_build_tests::{parallel_builds_are_identical_to_serial, build_modes_reach_their_builds}`;
  - `ds::tree_trie::tests::*`;
  - `kermit-ds/tests/trie_tests.rs` and `parquet_tests.rs`;
  - `kermit/tests/join_tests.rs` (the `TreeTrie*` suites and the `Parallel2` TreeTrie build-mode cells);
  - `kermit/tests/cli_tree_trie_build_mode.rs`;
  - `kermit/src/execution.rs` `tree_trie_families_build_with_their_mode` (the binary's test of the `test-hooks` record).

**Left alone:**
- **`morsel::scatter`, `Partition` and `Positioned`.** HashTrie's `parallel.rs` still uses them (lines 54, 168, 182–190, 298, 360–368) until T7 deletes them. Every item stays used, so nothing warns as dead code.
- **The `MORSEL_TUPLES` constant keeps its name.** HashTrie passes it too.

- [ ] **Step 1: Pin the trie against one insert per tuple, before touching the build.**

  Add these two tests to `mod parallel_build_tests` in
  `kermit-ds/src/ds/tree_trie/implementation.rs`, after `build_modes_reach_their_builds`
  (ends at 622). They use only the public API, so they compile against the post-T3 code.
  `Tuples` reaches the test module through `use super::*`, since T3 imports it into the
  parent.

  ```rust
      /// Every build mode builds the trie that one `insert` per tuple, in
      /// arrival order and unsorted, builds. A node's children, and so every
      /// `Vec` capacity, depend only on the tuple set (`Vec::insert` grows a
      /// list exactly as `push` does), so no build's sort, and no form the
      /// tuples arrive in, can change the trie or its `heap_size_bytes`
      /// (#111).
      #[test]
      fn builds_match_inserting_in_arrival_order() {
          let mut rng = Lcg(0x5EED);
          for arity in 0..=4 {
              for key_range in [1, 3, 50] {
                  for n in [0, 1, 17, 300] {
                      let tuples: Vec<Vec<usize>> = (0..n)
                          .map(|_| (0..arity).map(|_| rng.next_usize() % key_range).collect())
                          .collect();
                      let mut one_by_one: TreeTrie = TreeTrie::new(arity.into());
                      for tuple in &tuples {
                          one_by_one.insert(tuple);
                      }
                      let case = format!("arity {arity}, keys < {key_range}, n {n}");
                      for mode in [
                          TreeTrieBuildMode::Serial,
                          TreeTrieBuildMode::Parallel(threads(3)),
                      ] {
                          let built: TreeTrie =
                              TreeTrie::from_tuples_with_build_mode(arity.into(), mode, tuples.clone());
                          assert_identical(&built, &one_by_one, &format!("{case}, {mode:?}"));
                      }
                  }
              }
          }
      }

      /// A batch is accepted when it is empty, whatever its arity (an empty
      /// literal carries none), or when its arity is the header's.
      #[test]
      fn an_empty_batch_of_any_arity_builds_the_empty_trie() {
          for mode in [
              TreeTrieBuildMode::Serial,
              TreeTrieBuildMode::Parallel(threads(2)),
          ] {
              let built: TreeTrie =
                  TreeTrie::from_tuples_with_build_mode(2.into(), mode, Tuples::new(5));
              assert_identical(&built, &TreeTrie::new(2.into()), &format!("{mode:?}"));
          }
      }
  ```

- [ ] **Step 2: Run them on the current code. They pass, as characterisation tests.**

  ```bash
  CARGO_BUILD_JOBS=2 cargo test -p kermit-ds tree_trie::implementation::parallel_build_tests
  ```
  Expected: `test result: ok`, including `builds_match_inserting_in_arrival_order` and
  `an_empty_batch_of_any_arity_builds_the_empty_trie`, and 0 failed.

- [ ] **Step 3: Write the failing `scatter_rows` tests.**

  In `kermit-ds/src/morsel.rs`, inside `mod tests`:
  - Add the helper and three tests below directly after `scatter_moves_tuples_without_reallocating` (ends at 271).
  - In `empty_input_needs_no_work` (315–323), add the `scatter_rows` lines.

  They mirror `scatter_keeps_every_tuple_once_in_input_order` and the empty-input check.
  They are not trimmed for Miri, which CI no longer runs.

  ```rust
      /// `n` rows `[i % 7, i]`, as one batch.
      fn numbered_rows(n: usize) -> Tuples {
          let mut tuples = Tuples::with_capacity(2, n);
          for i in 0..n {
              tuples.push(&[i % 7, i]);
          }
          tuples
      }

      /// Every row's id lands exactly once, in the partition `partition_of`
      /// names, and each partition lists its ids in input order, whatever the
      /// thread count and however the morsels divide the input.
      #[test]
      fn scatter_rows_keeps_every_row_once_in_input_order() {
          let n = 1000;
          let input = numbered_rows(n);
          for t in [1, 2, 3, 8] {
              for morsel in [1, 4, 5, n, n + 1] {
                  let case = format!("threads {t}, morsel {morsel}");
                  let partitions = scatter_rows(threads(t), &input, morsel, 3, |row| row[0] % 3);
                  assert_eq!(partitions.len(), 3, "{case}");
                  let mut seen: Vec<RowId> = Vec::new();
                  for (p, partition) in partitions.iter().enumerate() {
                      let ids: Vec<RowId> = partition.ids().collect();
                      assert_eq!(partition.len(), ids.len(), "{case}: partition {p} length");
                      assert!(
                          ids.windows(2).all(|pair| pair[0] < pair[1]),
                          "{case}: partition {p} is out of input order"
                      );
                      for &id in &ids {
                          assert_eq!(input.row(id)[0] % 3, p, "{case}: row {id} in partition {p}");
                      }
                      seen.extend(ids);
                  }
                  seen.sort_unstable();
                  let every: Vec<RowId> = (0..).take(n).collect();
                  assert_eq!(seen, every, "{case}: every row exactly once");
              }
          }
      }

      /// A partition holds one segment per morsel that sent it any row, in
      /// morsel order, and no empty segment. Ten rows in morsels of 3 are rows
      /// 0–2, 3–5, 6–8 and 9; split by parity, the last morsel sends the even
      /// partition nothing.
      #[test]
      fn scatter_rows_cuts_one_segment_per_contributing_morsel() {
          let input = numbered_rows(10);
          let partitions = scatter_rows(threads(2), &input, 3, 2, |row| row[1] % 2);
          assert_eq!(partitions[0].segments, vec![vec![0, 2], vec![4], vec![6, 8]]);
          assert_eq!(partitions[1].segments, vec![
              vec![1],
              vec![3, 5],
              vec![7],
              vec![9]
          ]);
      }

      /// A nullary row is an empty slice but still has an id, so a batch of
      /// them scatters like any other.
      #[test]
      fn scatter_rows_sends_nullary_rows_by_id() {
          let mut input = Tuples::new(0);
          for _ in 0..5 {
              input.push(&[]);
          }
          let partitions = scatter_rows(threads(2), &input, 2, 2, |row| row.len());
          assert_eq!(partitions[0].ids().collect::<Vec<_>>(), vec![0, 1, 2, 3, 4]);
          assert_eq!(partitions[1].len(), 0);
      }
  ```

  `empty_input_needs_no_work` becomes:

  ```rust
      #[test]
      fn empty_input_needs_no_work() {
          assert!(dispatch(threads(4), Vec::<usize>::new(), |i| i).is_empty());
          let partitions = scatter(threads(4), Vec::new(), 8, 3, |_| 0);
          assert_eq!(partitions.len(), 3);
          assert!(partitions
              .into_iter()
              .all(|partition| partition.into_tuples().next().is_none()));
          let partitions = scatter_rows(threads(4), &Tuples::new(2), 8, 3, |_| 0);
          assert_eq!(partitions.len(), 3);
          assert!(partitions
              .iter()
              .all(|partition| partition.ids().next().is_none()));
      }
  ```

- [ ] **Step 4: Run them. They fail to compile.**

  ```bash
  CARGO_BUILD_JOBS=2 cargo test -p kermit-ds scatter_rows
  ```
  Expected: `error[E0425]: cannot find function `scatter_rows` in this scope`, and the
  same for `Tuples` / `RowId` until the import below exists.

- [ ] **Step 5: Implement `RowPartition` and `scatter_rows`.**

  In `kermit-ds/src/morsel.rs`, the module doc's list (lines 8–9) becomes:

  ```rust
  //! 1. [`scatter`] moves tuples into partitions, one morsel at a time
  //!    ([`scatter_rows`] sends the ids of a [`Tuples`] batch's rows instead,
  //!    leaving the batch where it is);
  //! 2. [`dispatch`] then runs one task per partition.
  ```

  Line 20, `use std::{num::NonZeroUsize, panic, sync::Mutex, thread};`, becomes:

  ```rust
  use {
      kermit_iters::{RowId, Tuples},
      std::{num::NonZeroUsize, panic, sync::Mutex, thread},
  };
  ```

  Insert after `scatter` (after line 161, before `dispatch`'s doc comment):

  ```rust
  /// The row ids [`scatter_rows`] sent to one partition: one segment per
  /// morsel that contributed any, in morsel order. Reading the segments in
  /// order reads the partition in input order, so its ids ascend.
  #[derive(Debug, Default)]
  pub(crate) struct RowPartition {
      pub(crate) segments: Vec<Vec<RowId>>,
  }

  impl RowPartition {
      /// How many rows the partition holds.
      pub(crate) fn len(&self) -> usize { self.segments.iter().map(Vec::len).sum() }

      /// The partition's row ids in input order, ascending.
      pub(crate) fn ids(&self) -> impl Iterator<Item = RowId> + '_ {
          self.segments.iter().flatten().copied()
      }
  }

  /// One morsel's row ids, split by partition.
  type RowBuckets = Vec<Vec<RowId>>;

  /// `position` as a [`RowId`]. A [`Tuples`] batch holds at most `RowId::MAX`
  /// rows, so every position up to its length fits. Crate-visible: HashTrie's
  /// builds convert their row positions with it too.
  pub(crate) fn row_id(position: usize) -> RowId {
      RowId::try_from(position).expect("a `Tuples` batch holds at most `RowId::MAX` rows")
  }

  /// Sends the id of every row of `tuples` to partition `partition_of(row)`,
  /// which must be below `partitions`. The batch stays where it is: workers
  /// read its rows through `&Tuples`, and only 4-byte ids move.
  ///
  /// `threads` workers take morsels of `morsel_rows` consecutive rows from a
  /// shared queue, so a worker that finishes early takes more. Every
  /// partition lists its ids in input order, whatever order the morsels ran
  /// in, as [`scatter`] lists its tuples.
  ///
  /// # Panics
  ///
  /// Panics if `morsel_rows` is zero, or if `partition_of` returns
  /// `partitions` or more, or panics itself.
  pub(crate) fn scatter_rows(
      threads: Threads, tuples: &Tuples, morsel_rows: usize, partitions: usize,
      partition_of: impl Fn(&[usize]) -> usize + Sync,
  ) -> Vec<RowPartition> {
      let len = tuples.len();
      let queue = Mutex::new((0..len).step_by(morsel_rows).enumerate());
      let mut morsels: Vec<(usize, RowBuckets)> = run_workers(threads, || {
          let mut scattered = Vec::new();
          while let Some((index, first)) = take_next(&queue) {
              let end = first + morsel_rows.min(len - first);
              let mut buckets: RowBuckets = (0..partitions).map(|_| Vec::new()).collect();
              for id in row_id(first)..row_id(end) {
                  buckets[partition_of(tuples.row(id))].push(id);
              }
              scattered.push((index, buckets));
          }
          scattered
      })
      .into_iter()
      .flatten()
      .collect();
      morsels.sort_unstable_by_key(|&(index, _)| index);

      let mut out: Vec<RowPartition> = (0..partitions).map(|_| RowPartition::default()).collect();
      for (_, buckets) in morsels {
          for (partition, bucket) in out.iter_mut().zip(buckets) {
              if !bucket.is_empty() {
                  partition.segments.push(bucket);
              }
          }
      }
      out
  }
  ```
  - `step_by(0)` panics as the `# Panics` section says, before any worker starts.
  - `row_id` converts each morsel's two bounds once, not each row. It is `pub(crate)` because T7's HashTrie builds import it (`crate::morsel::row_id`) rather than define a second copy.
  - `scatter_rows` has no non-test caller until Step 7. The non-test build may warn that it, `RowPartition` and `row_id` are unused until then; Step 11's clippy runs after Step 7.

- [ ] **Step 6: Run the `morsel` tests. They pass.**

  ```bash
  CARGO_BUILD_JOBS=2 cargo test -p kermit-ds morsel::
  ```
  Expected: `test result: ok`, with the three new `scatter_rows_*` tests,
  `empty_input_needs_no_work` and the existing `scatter`/`dispatch` tests, and 0 failed.

- [ ] **Step 7: TreeTrie builds from row slices.**

  All edits are in `kermit-ds/src/ds/tree_trie/implementation.rs`.

  The `use` block (1–15) becomes the following. Keep one `Tuples` import, wherever T3
  put it.

  ```rust
  use {
      super::build_mode::TreeTrieBuildMode,
      crate::{
          morsel::{dispatch, scatter_rows, Threads, MORSEL_TUPLES, PARTITIONS_PER_THREAD},
          relation::{BuildModeRelation, Relation, RelationHeader},
          seek::{seek_axes, GallopingSeek, SeekStrategy},
      },
      kermit_iters::{HasOptimizationAxes, JoinIterable, Tuples},
      serde_json::Value,
      std::{
          collections::BTreeMap,
          marker::PhantomData,
          ops::{Index, IndexMut},
      },
  };
  ```

  `insert_into_children` (17–38) becomes:

  ```rust
  /// Inserts a tuple into a sorted list of children nodes, recursing for the
  /// remaining keys on the rest of the slice, so no level allocates. Duplicate
  /// tuples are silently absorbed: when a key already exists at this level we
  /// descend into its children instead of allocating a new node. Returns
  /// `true` iff the tuple was not already present (some level created a new
  /// node).
  fn insert_into_children(children: &mut Vec<TrieNode>, tuple: &[usize]) -> bool {
      let Some((&key, rest)) = tuple.split_first() else {
          // Exhausted every key along an already-existing path — duplicate.
          return false;
      };

      match children.binary_search_by(|node| node.key().cmp(&key)) {
          | Ok(pos) => insert_into_children(children[pos].children_mut(), rest),
          | Err(pos) => {
              let mut new_node = TrieNode::new(key);
              insert_into_children(new_node.children_mut(), rest);
              children.insert(pos, new_node);
              true
          },
      }
  }
  ```
  `rest` is `&tuple[1..]`. Node creation and every `Vec` operation are as before.
  Only the per-level `collect()` of the suffix is gone.

  `first_key_splitters` (148–161) becomes the following. The doc comment at 140–147
  stays.

  ```rust
  fn first_key_splitters(tuples: &Tuples, partitions: usize) -> Vec<usize> {
      let stride = (tuples.len() / (SPLITTER_SAMPLES_PER_PARTITION * partitions)).max(1);
      let mut sample: Vec<usize> = tuples.rows().step_by(stride).map(|row| row[0]).collect();
      sample.sort_unstable();
      let mut splitters: Vec<usize> = (1..partitions)
          .map(|p| sample[p * sample.len() / partitions])
          .collect();
      splitters.dedup();
      splitters
  }
  ```

  The `impl<S: SeekStrategy> TreeTrie<S>` block that holds `from_tuples_parallel` and
  `build_parallel` (180–278) becomes:

  ```rust
  impl<S: SeekStrategy> TreeTrie<S> {
      /// The `parallel:N` build (`docs/data-structures/parallel-build.md`):
      /// the trie [`Relation::from_tuples`] builds, built on `threads` threads.
      fn from_tuples_parallel(header: RelationHeader, threads: Threads, tuples: Tuples) -> Self {
          Self::build_parallel(header, threads, MORSEL_TUPLES, tuples)
      }

      /// [`from_tuples_parallel`](Self::from_tuples_parallel) with the morsel
      /// size as a parameter, so tests can cut a small input into many
      /// morsels.
      ///
      /// 1. **Partition**: [`scatter_rows`] the row ids into first-key ranges
      ///    cut at [`first_key_splitters`]; the batch itself stays where it is.
      /// 2. **Build**: [`dispatch`] each partition to a worker, which gathers
      ///    its rows into a [`Tuples`] of its own (its ids ascend, so the reads
      ///    mostly move forward), sorts it, and inserts its rows one at a time,
      ///    as the serial build does with the whole batch.
      /// 3. **Assemble**: push every partition's top-level nodes onto the root,
      ///    in key order, one at a time.
      ///
      /// Sorted order restricted to a key range is that range sorted, so each
      /// subtree receives the serial build's sequence of inserts, and the root
      /// grows by one push per first key, as the serial insert-at-the-end
      /// does. Every node and every `Vec` capacity therefore matches.
      fn build_parallel(
          header: RelationHeader, threads: Threads, morsel_rows: usize, tuples: Tuples,
      ) -> Self {
          if tuples.is_empty() {
              return Self::new(header);
          }
          // The serial build's check, with its message.
          let arity = tuples.arity();
          assert_eq!(
              arity,
              header.arity(),
              "from_tuples: tuple arity {arity} does not match header arity {}",
              header.arity()
          );
          if arity == 0 {
              // A nullary tuple inserts nothing, so the serial build of any
              // number of them is the empty trie.
              return Self::new(header);
          }

          let splitters = first_key_splitters(&tuples, PARTITIONS_PER_THREAD * threads.get());
          let partitions = scatter_rows(
              threads,
              &tuples,
              morsel_rows,
              splitters.len() + 1,
              |row| splitters.partition_point(|&splitter| splitter <= row[0]),
          );
          #[cfg(any(test, feature = "test-hooks"))]
          PARALLEL_BUILDS.with(|builds| {
              let sizes = partitions.iter().map(|partition| partition.len()).collect();
              builds.borrow_mut().push((threads.get(), sizes));
          });
          let built = dispatch(threads, partitions, |partition| {
              let mut gathered = Tuples::with_capacity(arity, partition.len());
              for id in partition.ids() {
                  gathered.push(tuples.row(id));
              }
              // `Tuples::sort`, the serial build's sort (and ColumnTrie's), so
              // a comparison costs the same in both builds and `parallel:N`
              // against `serial` measures the build process, not a cheaper sort.
              gathered.sort();
              let mut nodes = Vec::new();
              let mut count = 0;
              for row in gathered.rows() {
                  if insert_into_children(&mut nodes, row) {
                      count += 1;
                  }
              }
              (nodes, count)
          });

          let mut trie = Self::new(header);
          for (nodes, count) in built {
              // One push per node, as the serial build inserts one node at a
              // time: `extend` or `append` would reserve in bulk and leave the
              // root with a capacity the serial build never has.
              for node in nodes {
                  trie.children.push(node);
              }
              trie.tuple_count += count;
          }
          trie
      }
  }
  ```
  - The old `assert!(tuples.iter().all(|tuple| tuple.len() == arity))` is gone. A `Tuples` cannot hold mixed arities, because its `From<Vec<Vec<usize>>>` rejects them.
  - Each worker frees its own `gathered` buffer, and the calling thread frees the input batch once, when the build returns.
  - The record keeps a closure over `partition.len()`. Naming `RowPartition::len` would need a `cfg`-gated import.

  In `impl<S: SeekStrategy> Relation for TreeTrie<S>`, `from_tuples` (doc and fn,
  292–335), `insert` (337–352) and `insert_all` (354–364) become:

  ```rust
      /// Builds a `TreeTrie` from a batch of tuples.
      ///
      /// Sorts the batch in place with [`Tuples::sort`] (lexicographic), the
      /// most efficient input order for the sorted-children invariant: every
      /// new key then lands at the end of its sibling list. Then inserts each
      /// row as a slice; the batch's one buffer is freed when the build returns.
      ///
      /// # Panics
      ///
      /// Panics if the batch is non-empty and its arity does not match
      /// `header.arity()`. A `Vec<Vec<usize>>` of mixed arities panics
      /// earlier, converting to [`Tuples`].
      fn from_tuples(header: RelationHeader, tuples: impl Into<Tuples>) -> Self {
          let mut tuples: Tuples = tuples.into();
          if tuples.is_empty() {
              return Self::new(header);
          }

          let arity = tuples.arity();
          assert_eq!(
              arity,
              header.arity(),
              "from_tuples: tuple arity {arity} does not match header arity {}",
              header.arity()
          );

          // One sort, `Tuples::sort`, in every sorted-trie build (`parallel:N`'s
          // workers and ColumnTrie's builds run it too), so a comparison costs
          // the same in each and `insertion` compares build routines, not sorts.
          tuples.sort();

          let mut trie = Self::new(header);
          for tuple in tuples.rows() {
              trie.insert(tuple);
          }
          trie
      }

      /// Inserts a single tuple, preserving the sorted-children invariant.
      /// Duplicate tuples are silently absorbed.
      ///
      /// # Panics
      ///
      /// Panics if `tuple.len()` does not match the arity of the relation.
      fn insert(&mut self, tuple: impl AsRef<[usize]>) {
          let tuple = tuple.as_ref();
          assert_eq!(
              tuple.len(),
              self.header().arity(),
              "tuple arity must match relation arity"
          );
          if insert_into_children(&mut self.children, tuple) {
              self.tuple_count += 1;
          }
      }

      /// Inserts every tuple in `tuples`.
      ///
      /// # Panics
      ///
      /// Panics if any tuple's arity does not match the relation's arity
      /// (propagated from [`insert`](Self::insert)).
      fn insert_all(&mut self, tuples: impl Into<Tuples>) {
          let tuples: Tuples = tuples.into();
          for tuple in tuples.rows() {
              self.insert(tuple);
          }
      }
  ```

  `from_tuples_with_build_mode` (376–385; its doc at 370–375 stays) becomes:

  ```rust
      fn from_tuples_with_build_mode(
          header: RelationHeader, mode: TreeTrieBuildMode, tuples: impl Into<Tuples>,
      ) -> Self {
          let tuples: Tuples = tuples.into();
          match mode {
              | TreeTrieBuildMode::Serial => Self::from_tuples(header, tuples),
              | TreeTrieBuildMode::Parallel(threads) => {
                  Self::from_tuples_parallel(header, threads, tuples)
              },
          }
      }
  ```

- [ ] **Step 8: Update the tests that call the private build or the splitters.**

  All edits are in `mod parallel_build_tests`:

  1. **`parallel_builds_are_identical_to_serial` (581–586).** The `build_parallel` call
     takes a `Tuples`:
     ```rust
                                  let parallel: TreeTrie = TreeTrie::build_parallel(
                                      arity.into(),
                                      threads(t),
                                      morsel,
                                      Tuples::from(tuples.clone()),
                                  );
     ```
  2. **`parallel_build_rejects_a_first_tuple_of_the_wrong_arity` (624–628)** becomes:
     ```rust
         #[test]
         #[should_panic(expected = "does not match header arity")]
         fn parallel_build_rejects_a_first_tuple_of_the_wrong_arity() {
             let _: TreeTrie =
                 TreeTrie::from_tuples_parallel(2.into(), threads(2), vec![vec![1, 2, 3]].into());
         }
     ```
  3. **Delete `parallel_build_rejects_mixed_arity` (630–635).** It tests the deleted
     `assert!(… tuple.len() == arity)`, by that message. A mixed-arity batch can no longer
     reach the build: converting it to `Tuples` panics, and T1's `From` test pins that.
  4. **`splitters_are_increasing_first_keys` (637–653)** becomes:
     ```rust
         /// The splitters are strictly increasing first keys, at most
         /// `partitions − 1` of them.
         #[test]
         fn splitters_are_increasing_first_keys() {
             let tuples =
                 Tuples::from((0..1000).map(|i| vec![(i * 7) % 100, i]).collect::<Vec<_>>());
             let splitters = first_key_splitters(&tuples, 8);
             assert!(
                 !splitters.is_empty() && splitters.len() <= 7,
                 "{splitters:?}"
             );
             assert!(
                 splitters.windows(2).all(|pair| pair[0] < pair[1]),
                 "{splitters:?}"
             );
             assert!(splitters.iter().all(|&s| s < 100), "{splitters:?}");
             let one_key = Tuples::from(vec![vec![5, 1], vec![5, 2]]);
             assert_eq!(first_key_splitters(&one_key, 4), vec![5]);
         }
     ```
  5. **`splitters_share_out_the_tuples` (655–697)** becomes the following. The rows are
     pushed, so the 100 000-row fixture allocates one buffer.
     ```rust
         /// The splitters share out the tuples, not the distinct keys: no
         /// partition holds much more than one share plus the run of its
         /// heaviest key (a key's tuples cannot be split). Spacing the splitters
         /// over distinct keys instead puts most of a skewed input in one
         /// partition.
         #[test]
         #[cfg_attr(miri, ignore = "large inputs; plain arithmetic, no threads")]
         fn splitters_share_out_the_tuples() {
             let partitions = 8;
             let largest_partition = |tuples: &Tuples| {
                 let splitters = first_key_splitters(tuples, partitions);
                 let mut counts = vec![0usize; splitters.len() + 1];
                 for row in tuples.rows() {
                     counts[splitters.partition_point(|&s| s <= row[0])] += 1;
                 }
                 counts.into_iter().max().unwrap()
             };

             // Uniform first keys: no key is heavy.
             let mut rng = Lcg(7);
             let mut uniform = Tuples::with_capacity(1, 100_000);
             for _ in 0..100_000 {
                 uniform.push(&[rng.next_usize() % 10_000]);
             }
             let share = uniform.len() / partitions;
             let largest = largest_partition(&uniform);
             assert!(
                 2 * largest <= 3 * share,
                 "uniform: largest {largest}, share {share}"
             );

             // Skewed first keys: key k appears 10 000 / (k + 1) times, so the
             // heaviest keys have the lowest ids.
             let mut skewed = Tuples::new(1);
             for k in 0..1000usize {
                 for _ in 0..10_000 / (k + 1) {
                     skewed.push(&[k]);
                 }
             }
             let share = skewed.len() / partitions;
             let heaviest = 10_000;
             let largest = largest_partition(&skewed);
             assert!(
                 2 * largest <= 2 * heaviest + 3 * share,
                 "skewed: largest {largest}, share {share}, heaviest key {heaviest}"
             );
         }
     ```
  `build_modes_reach_their_builds` needs no edit. It still expects:
  - `vec![(2, vec![8; 8])]`: the same partition function over the same rows;
  - worker runs `[2, 2]`: one `run_workers` in `scatter_rows`, one in `dispatch`.

- [ ] **Step 9: Run the pinning tests.**

  ```bash
  CARGO_BUILD_JOBS=2 cargo test -p kermit-ds tree_trie
  CARGO_BUILD_JOBS=2 cargo test -p kermit-ds morsel::
  CARGO_BUILD_JOBS=2 cargo test -p kermit-ds --test trie_tests
  CARGO_BUILD_JOBS=2 cargo test -p kermit-ds --test parquet_tests
  CARGO_BUILD_JOBS=2 cargo test -p kermit --bin kermit tree_trie_families_build_with_their_mode
  CARGO_BUILD_JOBS=2 cargo test -p kermit --test cli_tree_trie_build_mode
  CARGO_BUILD_JOBS=2 cargo test -p kermit --test join_tests
  ```
  Expected: every `test result: ok`, 0 failed. The identity tests that pin the build are:
  - `parallel_builds_are_identical_to_serial`: every seed, arity 0–4, morsels of 7 and `MORSEL_TUPLES`;
  - `builds_match_inserting_in_arrival_order`;
  - `build_modes_reach_their_builds`;
  - `an_empty_batch_of_any_arity_builds_the_empty_trie`.

- [ ] **Step 10: Check that TreeTrie no longer uses the shim or the comparator.**

  ```bash
  rg -n 'into_vecs|sort_unstable_by\(|Vec<Vec<usize>>' kermit-ds/src/ds/tree_trie/implementation.rs
  ```
  Expected: hits only inside `mod parallel_build_tests`, in the test fixtures'
  `let tuples: Vec<Vec<usize>>` lines. Nothing outside the tests matches.

- [ ] **Step 11: Lint, format, doc.**

  ```bash
  CARGO_BUILD_JOBS=2 cargo clippy --all-targets -- -D warnings
  nix develop --command cargo fmt --all
  RUSTDOCFLAGS=-Dwarnings cargo doc --workspace --no-deps
  ```
  Expected: no warnings. `fmt` may re-wrap the new lines; re-run the Step 9 tests if it
  touched anything beyond whitespace.

- [ ] **Step 12: Mutation checks.**

  For each mutant:
  1. Apply it as an exact edit.
  2. Confirm `git diff` shows it.
  3. Run the named test and see it fail.
  4. Reverse the exact edit, never with `git checkout`.
  5. Re-run the test to green.

  The mutants:
  - **`morsel.rs`:** `buckets[partition_of(tuples.row(id))].push(id);` → `buckets[0].push(id);`
    - Fails: `scatter_rows_keeps_every_row_once_in_input_order` and `scatter_rows_cuts_one_segment_per_contributing_morsel`.
    - Run: `CARGO_BUILD_JOBS=2 cargo test -p kermit-ds scatter_rows`.
  - **`implementation.rs`:** `for id in partition.ids() {` → `for id in partition.ids().skip(1) {`
    - Fails: `parallel_builds_are_identical_to_serial` (rows lost).
    - Run: `CARGO_BUILD_JOBS=2 cargo test -p kermit-ds parallel_builds_are_identical_to_serial`.
  - **`implementation.rs`:** `|row| splitters.partition_point(|&splitter| splitter <= row[0]),` → `|_| 0,`
    - Fails: `build_modes_reach_their_builds`, whose record shows `[64, 0, …]`. The identity tests pass under this mutant, because one partition still builds the same trie. That is why the record test exists.
    - Run: `CARGO_BUILD_JOBS=2 cargo test -p kermit-ds build_modes_reach_their_builds`.

  Expected: `git status` shows only this task's intended edits afterwards.

  Deleting `gathered.sort();` survives every test. TreeTrie's insert accepts any order and
  builds the same trie, so the sort buys only speed. That is the property
  `builds_match_inserting_in_arrival_order` states. Do not count it as a missed mutant.

- [ ] **Step 13: Commit.**

  ```bash
  git add kermit-ds/src/morsel.rs kermit-ds/src/ds/tree_trie/implementation.rs
  git commit -m "$(cat <<'EOF'
  refactor(tree-trie): build from row slices; scatter row ids (#111)

  TreeTrie sorts its Tuples batch in place with Tuples::sort and inserts
  each row as a slice: insert_into_children recurses on the rest of the
  slice instead of collecting a Vec per level, and the batch is freed
  once. parallel:N scatters row ids through the new morsel::scatter_rows
  (RowPartition: per-morsel segments, input order); each worker gathers
  its rows into a local Tuples, sorts it and builds its subtrie. The old
  morsel::scatter stays for HashTrie until it moves to row ids.

  Every build still equals one insert per tuple in arrival order, pinned
  by a new test beside the serial/parallel identity tests.

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_015P4eTR34w48GpD7DvPPaee
  EOF
  )"
  ```

---

### Task 6: ColumnTrie builds from row slices

**Files:**
- Modify: `kermit-ds/src/ds/column_trie/implementation.rs`
  - `use` (1–10);
  - `from_sorted_by_insertion` (282–293);
  - `from_sorted` (295–354);
  - `from_tuples` (431–438), `insert` (440–454), `insert_all` (456–460);
  - `from_tuples_with_build_mode` (463–506);
  - test module `use` (535–543);
  - `bulk_build_rejects_a_tuple_of_the_wrong_arity` (875–879), plus three new tests.
- Modify: `kermit-ds/src/relation.rs`, `project_via_trie_iter` (195–226). The spec's API
  table moves it to a `Tuples` collected through `advance`. No contract task names it,
  and only TreeTrie and ColumnTrie use it, so it lands here.
- Test (unchanged, pins behaviour), in `ds::column_trie::implementation::tests`:
  - `bulk_and_incremental_builds_are_identical` (bulk vs incremental vs unsorted one-by-one inserts, arrays and capacities and `heap_size_bytes`);
  - `bulk_build_of_duplicates_stores_one_tuple`;
  - `bulk_build_of_no_tuples_is_the_empty_trie`;
  - `bulk_build_of_nullary_tuples_stores_nothing`;
  - `bulk_build_matches_the_documented_example`.

  Elsewhere:
  - `ds::column_trie::implementation::heap_size_tests::heap_size_is_deterministic_across_rebuilds`;
  - `built_with::tests::{from_tuples_builds_the_same_relation, projection_and_inserts_reach_the_inner_relation}`;
  - `test_project` and `test_project_with_named_attributes` in both tries;
  - `kermit-ds/tests/{trie_tests,parquet_tests}.rs`;
  - `kermit/tests/join_tests.rs` (the `ColumnTrie*` suites and the `Incremental` build-mode cells);
  - `kermit/tests/cli_column_trie_build_mode.rs`.

**No parallel build to change.** `ColumnTrieBuildMode` is `Incremental | Bulk`
(`column_trie/build_mode.rs:10–20`). #103's parallel build is parked.

- [ ] **Step 1: Pin the batch rule before the refactor.**

  In `mod tests` of `kermit-ds/src/ds/column_trie/implementation.rs`:
  - Add `Tuples` to the `kermit_iters` import (542). It becomes
    `kermit_iters::{HasOptimizationAxes, LinearIterator, TrieIterable, TrieIterator, Tuples},`.
  - **Replace `bulk_build_rejects_a_tuple_of_the_wrong_arity`** (875–879) as T3 Step 7
    left it: a uniform arity-3 batch under an arity-2 header, since a mixed-arity input
    no longer reaches the build (`From<Vec<Vec<usize>>>` rejects it first, and T1 tests
    that). The first test below is that test, renamed for what it now checks and with
    the full panic message; the other two are new.
  - Put these three tests in its place:

  ```rust
      #[test]
      #[should_panic(expected = "from_tuples: tuple arity 3 does not match header arity 2")]
      fn bulk_build_rejects_a_batch_of_the_wrong_arity() {
          let _: ColumnTrie = ColumnTrie::from_tuples(2.into(), vec![vec![1, 2, 3], vec![4, 5, 6]]);
      }

      #[test]
      #[should_panic(expected = "from_tuples: tuple arity 3 does not match header arity 2")]
      fn incremental_build_rejects_a_batch_of_the_wrong_arity() {
          let _: ColumnTrie = ColumnTrie::from_tuples_with_build_mode(
              2.into(),
              ColumnTrieBuildMode::Incremental,
              vec![vec![1, 2, 3]],
          );
      }

      /// A batch is accepted when it is empty, whatever its arity (an empty
      /// literal carries none), or when its arity is the header's.
      #[test]
      fn an_empty_batch_of_any_arity_builds_the_empty_trie() {
          for mode in [ColumnTrieBuildMode::Bulk, ColumnTrieBuildMode::Incremental] {
              let built: ColumnTrie =
                  ColumnTrie::from_tuples_with_build_mode(2.into(), mode, Tuples::new(5));
              assert_identical(&built, &ColumnTrie::new(2.into()), &format!("{mode:?}"));
          }
      }
  ```

- [ ] **Step 2: Run them on the current code. They pass.**

  On the post-T3 code:
  - the per-tuple assert after `into_vecs()` fires with the same text;
  - `Tuples::new(5).into_vecs()` is an empty `Vec`.

  ```bash
  CARGO_BUILD_JOBS=2 cargo test -p kermit-ds column_trie::implementation::tests
  ```
  Expected: `test result: ok`, 0 failed.

- [ ] **Step 3: ColumnTrie builds from row slices.**

  All edits are in `kermit-ds/src/ds/column_trie/implementation.rs`.

  The `use` block (1–10) becomes the following. Keep one `Tuples` import, wherever T3
  put it.

  ```rust
  use {
      super::build_mode::ColumnTrieBuildMode,
      crate::{
          relation::{BuildModeRelation, Relation, RelationHeader},
          seek::{seek_axes, GallopingSeek, SeekStrategy},
      },
      kermit_iters::{HasOptimizationAxes, JoinIterable, Tuples},
      serde_json::Value,
      std::{collections::BTreeMap, fmt, marker::PhantomData},
  };
  ```

  `from_sorted_by_insertion` (287–293; its doc at 282–286 stays) becomes:

  ```rust
      fn from_sorted_by_insertion(header: RelationHeader, sorted: &Tuples) -> Self {
          let mut trie = Self::new(header);
          for tuple in sorted.rows() {
              trie.insert(tuple);
          }
          trie
      }
  ```

  `from_sorted` (314–354; its doc at 295–313 stays) becomes:

  ```rust
      fn from_sorted(header: RelationHeader, sorted: &Tuples) -> Self {
          let mut trie = Self::new(header);
          if sorted.is_empty() {
              return trie;
          }
          let arity = trie.header.arity();
          // A non-empty trie's root layer holds exactly one interval.
          if let Some(root) = trie.layers.first_mut() {
              root.open_interval();
          }
          // The last row stored, borrowed from the batch: no copy per row.
          let mut previous: Option<&[usize]> = None;
          for tuple in sorted.rows() {
              // The depth at which this tuple leaves its predecessor's path:
              // the number of leading keys the two share.
              let divergence_depth = previous.map_or(0, |prev| common_prefix_len(prev, tuple));
              if let Some(prev) = previous {
                  debug_assert!(prev <= tuple, "from_sorted: tuples are not sorted");
              }
              if divergence_depth == arity {
                  // Equal to its predecessor, so already stored.
                  continue;
              }
              for (depth, &key) in tuple.iter().enumerate().skip(divergence_depth) {
                  let layer = &mut trie.layers[depth];
                  if depth > divergence_depth {
                      // The key just appended one layer up is a new parent,
                      // so its children start here.
                      layer.open_interval();
                  }
                  layer.push_key(key);
              }
              trie.tuple_count += 1;
              previous = Some(tuple);
          }
          trie
      }
  ```
  - **`previous` is a borrowed row slice, not a row index.** It meets the spec's aim, no
    `Vec` per row, without the id arithmetic an index needs: no `enumerate`, no `RowId`
    conversion, no `sorted.row(prev)` per step. The batch outlives the loop, so the
    borrow is free.
  - Every push and `open_interval` happens in the order it did before, so the arrays and
    capacities are unchanged.

  In `impl<S: SeekStrategy> Relation for ColumnTrie<S>`, `from_tuples` (431–438),
  `insert` (440–454) and `insert_all` (456–460) become:

  ```rust
      /// Builds with the default [`ColumnTrieBuildMode`], `Bulk`.
      ///
      /// # Panics
      ///
      /// As [`from_tuples_with_build_mode`](BuildModeRelation::from_tuples_with_build_mode).
      fn from_tuples(header: RelationHeader, tuples: impl Into<Tuples>) -> Self {
          Self::from_tuples_with_build_mode(header, ColumnTrieBuildMode::default(), tuples)
      }

      /// Inserts a single tuple. Duplicate tuples are silently absorbed.
      ///
      /// # Panics
      ///
      /// Panics if `tuple.len()` does not match the relation's arity.
      fn insert(&mut self, tuple: impl AsRef<[usize]>) {
          let tuple = tuple.as_ref();
          assert_eq!(
              tuple.len(),
              self.header().arity(),
              "tuple arity must match relation arity"
          );
          if self.internal_insert(tuple) {
              self.tuple_count += 1;
          }
      }

      fn insert_all(&mut self, tuples: impl Into<Tuples>) {
          let tuples: Tuples = tuples.into();
          for tuple in tuples.rows() {
              self.insert(tuple);
          }
      }
  ```

  `from_tuples_with_build_mode` (doc and fn, 466–506) becomes:

  ```rust
      /// Sorts the batch in place with [`Tuples::sort`], then builds the layers
      /// by `mode`: one pass over the sorted rows for `Bulk` (see
      /// `from_sorted`), one `insert` per row for `Incremental` (see
      /// `from_sorted_by_insertion`).
      ///
      /// # Panics
      ///
      /// Panics if the batch is non-empty and its arity does not match
      /// `header.arity()`. A `Vec<Vec<usize>>` of mixed arities panics
      /// earlier, converting to [`Tuples`].
      fn from_tuples_with_build_mode(
          header: RelationHeader, mode: ColumnTrieBuildMode, tuples: impl Into<Tuples>,
      ) -> Self {
          let mut tuples: Tuples = tuples.into();
          // An empty batch carries no arity of its own (an empty literal has
          // none), so only a non-empty one must match the header.
          if !tuples.is_empty() {
              let arity = tuples.arity();
              assert_eq!(
                  arity,
                  header.arity(),
                  "from_tuples: tuple arity {arity} does not match header arity {}",
                  header.arity()
              );
          }
          // `Tuples::sort`, the one sort every sorted-trie build runs (TreeTrie's
          // `serial` and `parallel:N` builds too): a comparison costs the same in
          // every ColumnTrie build and in TreeTrie's, so a change in the
          // `insertion` metric measures the build routine alone. Its order is
          // the lexicographic order the hand-rolled comparator gave before #111.
          tuples.sort();
          match mode {
              | ColumnTrieBuildMode::Incremental => Self::from_sorted_by_insertion(header, &tuples),
              | ColumnTrieBuildMode::Bulk => Self::from_sorted(header, &tuples),
          }
      }
  ```
  - **Panic messages:** the text `from_tuples: tuple arity 3 does not match header arity 2`
    is unchanged.
  - **Gone, `Checked before the sort: its comparator indexes b by a's length`:** a batch's
    rows all have one length, so no comparator can index past one.

- [ ] **Step 4: Run the pinning tests.**

  ```bash
  CARGO_BUILD_JOBS=2 cargo test -p kermit-ds column_trie
  CARGO_BUILD_JOBS=2 cargo test -p kermit-ds built_with
  CARGO_BUILD_JOBS=2 cargo test -p kermit-ds --test trie_tests
  CARGO_BUILD_JOBS=2 cargo test -p kermit-ds --test parquet_tests
  CARGO_BUILD_JOBS=2 cargo test -p kermit --test cli_column_trie_build_mode
  CARGO_BUILD_JOBS=2 cargo test -p kermit --test join_tests
  ```
  Expected: every `test result: ok`, 0 failed. Debug builds run `from_sorted`'s sortedness
  `debug_assert!`, so these runs also check that `Tuples::sort` sorts.

- [ ] **Step 5: `project_via_trie_iter` collects a `Tuples` through `advance`.**

  In `kermit-ds/src/relation.rs`, replace the doc and body of `project_via_trie_iter`
  (195–226). `Tuples` is in scope since T3, whose signatures in this file name it.

  ```rust
  /// Default trie-iter-based projection shared by the sorted tries.
  ///
  /// Lends every tuple through the trie iterator, copies the requested
  /// columns into one [`Tuples`] batch (no `Vec` per tuple), then rebuilds via
  /// `from_tuples`. Backends that can do better (e.g. a column-store that can
  /// keep the existing layer arrays for the requested columns) may override
  /// [`Projectable::project`] with their own implementation.
  pub(crate) fn project_via_trie_iter<R>(rel: &R, columns: Vec<usize>) -> R
  where
      R: Relation + kermit_iters::TrieIterable,
  {
      let current_header = rel.header();
      let projected_attrs: Vec<String> = columns
          .iter()
          .filter_map(|&col_idx| current_header.attrs().get(col_idx).cloned())
          .collect();

      let new_header = if projected_attrs.is_empty() {
          RelationHeader::new_nameless_positional(columns.len())
      } else {
          RelationHeader::new_nameless(projected_attrs)
      };

      let mut projected = Tuples::new(columns.len());
      let mut row = Vec::with_capacity(columns.len());
      let mut tuples = kermit_iters::TrieIteratorWrapper::new(rel.trie_iter());
      while let Some(tuple) = tuples.advance() {
          row.clear();
          row.extend(columns.iter().map(|&col_idx| tuple[col_idx]));
          projected.push(&row);
      }

      R::from_tuples(new_header, projected)
  }
  ```
  - **Relations of arity 0:** nothing is iterated, so the result is an empty batch whose
    arity is the header's, as before.
  - **An empty column list:** gives rows of arity 0, as before.

- [ ] **Step 6: Run the projection tests.**

  ```bash
  CARGO_BUILD_JOBS=2 cargo test -p kermit-ds project
  ```
  Expected: `test result: ok`, 0 failed. This covers:
  - `ds::tree_trie::tests::{test_project, test_project_with_named_attributes}`;
  - the two ColumnTrie `test_project*` tests;
  - `built_with::tests::projection_and_inserts_reach_the_inner_relation`;
  - `configured::tests` (`r.project(vec![0])`).

- [ ] **Step 7: Check that neither sorted trie keeps the shim or the comparator, then run the workspace.**

  ```bash
  rg -n 'into_vecs|sort_unstable_by\(' kermit-ds/src/ds/tree_trie kermit-ds/src/ds/column_trie kermit-ds/src/relation.rs
  CARGO_BUILD_JOBS=2 cargo test --workspace
  ```
  Expected: `rg` prints nothing. The test run ends with every `test result: ok` and 0
  failed. `kermit-algos`' sorted-family tests use both tries as dev-dependencies.

- [ ] **Step 8: Lint, format, doc.**

  ```bash
  CARGO_BUILD_JOBS=2 cargo clippy --all-targets -- -D warnings
  nix develop --command cargo fmt --all
  RUSTDOCFLAGS=-Dwarnings cargo doc --workspace --no-deps
  ```
  Expected: no warnings. `[`Tuples::sort`]` and `[`Tuples`]` resolve through the
  `kermit_iters` import.

- [ ] **Step 9: Mutation checks.**

  As in Task 5: apply an exact edit, see it fail, then reverse the exact edit.
  - **`from_sorted`:** delete `previous = Some(tuple);`
    - Fails: `bulk_and_incremental_builds_are_identical`, because every row re-appends its whole path.
    - Run: `CARGO_BUILD_JOBS=2 cargo test -p kermit-ds bulk_and_incremental_builds_are_identical`.
  - **`from_tuples_with_build_mode`:** `if !tuples.is_empty() {` → `if true {`
    - Fails: `an_empty_batch_of_any_arity_builds_the_empty_trie`, because the empty arity-5 batch is rejected.
    - Run: `CARGO_BUILD_JOBS=2 cargo test -p kermit-ds an_empty_batch_of_any_arity`.

  Expected: `git status` shows only this task's intended edits afterwards.

- [ ] **Step 10: Commit.**

  ```bash
  git add kermit-ds/src/ds/column_trie/implementation.rs kermit-ds/src/relation.rs
  git commit -m "$(cat <<'EOF'
  refactor(column-trie): build from row slices (#111)

  ColumnTrie sorts its Tuples batch with Tuples::sort, the one sort every
  sorted-trie build runs, and builds from row slices: from_sorted keeps
  the previous row as a slice borrowed from the batch, not a Vec, and
  from_sorted_by_insertion inserts slices. An empty batch of any arity is
  accepted; a non-empty one must match the header's arity. Arrays and
  capacities are unchanged (bulk == incremental == unsorted inserts).

  project_via_trie_iter (both sorted tries) collects one Tuples batch
  through TrieIteratorWrapper::advance instead of a Vec per tuple.

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_015P4eTR34w48GpD7DvPPaee
  EOF
  )"
  ```

---

### Task 7: HashTrie owns its buffer; chains, singletons and pending lists hold row ids; `leaf_tuples` returns `LeafRows`

One compile unit. The node type (`Leaf(HashTable<Vec<RowId>>)`, a `RowId`
singleton payload) breaks `HashTrieIter`, and the change to
`HashTrieIterator::leaf_tuples` breaks all four implementors at once, so
nothing builds until every step is in:
- **Steps 1–12, storage:** the node payloads, every build mode, lazy
  `resolve`, `HeapSize`, the read-back paths, the identity helpers, and the
  deletion of the old `morsel::scatter`.
- **Steps 13–21, the iterator family and the join:**
  `HashTrieIterator::leaf_tuples` → `LeafRows`, `hash_trie_iter.rs`,
  `SingletonFrame` in `pruning.rs`, and `kermit-algos/src/hash/*`.
- **Step 22** sweeps, **Step 23** builds and runs everything once,
  **Step 24** commits, **Step 25** runs the mutation checks.

> **Line numbers** are those of the tree before T1 (commit `47950f0`); T3
> changes the constructor signatures in `implementation.rs` and T5 edits
> `morsel.rs`, which shifts them by a few lines. Every edit quotes enough of
> the old code, or names the item, to find it.
>
> **State assumed from T1–T6:**
> - `kermit_iters::{Tuples, RowId, LeafRows}` exist with the contract API;
>   `LeafRows` lives in `kermit-iters/src/leaf_rows.rs`, and its `row(&self,
>   i) -> &'a [usize]` and `data(&self) -> &'a [usize]` return the view's
>   lifetime `'a`, not the `&self` borrow. `emit_leaf` (Step 21) and the
>   selection view's `leaf_tuples` (Step 19) call them on temporaries and
>   keep the results.
> - `Relation::{from_tuples, insert_all}`,
>   `ConfigurableRelation::from_tuples_with_config` and the two build-mode
>   constructors take `impl Into<Tuples>`; `Relation::insert` takes
>   `impl AsRef<[usize]>`. HashTrie's public constructors convert with
>   `Tuples::into_vecs(tuples.into())` and hand `Vec<Vec<usize>>` to the
>   private builds, which still take `Vec`.
> - T3 made `radix.rs:247` and `parallel.rs:644` pass a uniform arity-3
>   batch under an arity-2 header.
> - `morsel::scatter_rows`, `RowPartition` (Debug + Default; `ids()`,
>   `len()`) and `pub(crate) fn row_id(usize) -> RowId` exist beside the old
>   `scatter`, which only HashTrie still calls (TreeTrie moved to
>   `scatter_rows` in T5). morsel.rs keeps the name `MORSEL_TUPLES` and T5's
>   private `type RowBuckets`. HashTrie's `bulk.rs` and `implementation.rs`
>   import `crate::morsel::row_id`; no second copy is defined.
> - A bulk chain keeps its ids in input order (the spec's "a chain is a
>   `Vec<RowId>`, in input order"). Step 16's new test pins it.
>
> **`pruning.rs` is edited twice.** Step 3 edits `SingletonPayload` (lines
> 15–24, 83–91 and 109–115, the `Payload` line 187, and the test at
> 208–213) and adds `RowId` to the line-13 import. Step 14 edits
> `SingletonFrame` (26–47), its `Never` impl (93–107), `SingletonFrameOn`
> (117–157) and the test at 224–238, and reuses that import.
>
> **`for_each_tuple` stays a trie walk** (the spec's amendment of
> 2026-10-08). It is what `bench ds`'s `iteration` and `end_to_end` time
> (`kermit/src/execution.rs:409-418`, `kermit/src/bench/ds.rs:31-32`, issue
> #79), so it walks the trie as today and reads each row from the buffer
> through its chain, singleton or pending id. Only `TupleScan::scan_tuples`
> (statistics) scans the buffer in input order; `collect_tuples` copies the
> buffer.

**Files:**
- Modify: `kermit-iters/src/hash_trie.rs` (8, 31–32, 67–70)
- Modify: `kermit-ds/src/ds/hash_trie/pruning.rs` (13, 1–24, 26–47, 83–91, 93–107, 109–115, 117–157, 185–190; tests 208–213, 224–238)
- Modify: `kermit-ds/src/ds/hash_trie/node.rs` (lines 1–47)
- Modify: `kermit-ds/src/ds/hash_trie/expansion.rs` (lines 1–127, tests 158–213)
- Modify: `kermit-ds/src/ds/hash_trie/bulk.rs` (lines 1–149, tests 236–268, 272–318, 431–438)
- Modify: `kermit-ds/src/ds/hash_trie/implementation.rs` (lines 21–376, 378–425, 435–604, 606–631, 647–696, 735–772; tests 783–1468, 1510–1698, 1700–1780)
- Modify: `kermit-ds/src/ds/hash_trie/radix.rs` (lines 1–159)
- Modify: `kermit-ds/src/ds/hash_trie/parallel.rs` (lines 1–58, 90–134, 145–207, 216–237, 247–379; tests 451–463, 654–696, 738–805, 896–915, 938–966, 1050–1060)
- Modify: `kermit-ds/src/ds/hash_trie/identity.rs` (whole file)
- Modify: `kermit-ds/src/ds/hash_trie/hash_trie_iter.rs` (27–36, 63–72, 103–158, 230–265; tests 473, 514, 563, 575; two new tests after 661)
- Modify: `kermit-ds/src/morsel.rs` (module doc lines 1–18; delete lines 68–90 and 116–161; `scatter_rows`' doc; delete tests 206–271; test 315–323)
- Modify: `kermit-ds/src/configured.rs` (tests 208, 233–235, 268–270)
- Modify: `kermit-ds/src/relation.rs` (T3's test helper `hash_rows`)
- Modify: `kermit-algos/src/hash/singleton.rs` (12, 21–40, 51–58, 117–123, test 182–188)
- Modify: `kermit-algos/src/hash/selection.rs` (16, 52–56, 68–74, 83–93, 104–115, 172–186; tests 235, 275, 350; one new test after 363)
- Modify: `kermit-algos/src/hash/iter_kind.rs` (12, 110–116, test 178)
- Modify: `kermit-algos/src/hash/hash_triejoin.rs` (187–226, test 440–454)
- Test: `kermit-ds/tests/common/macros.rs` (714–718, 1075–1080, 1094–1100, 1130–1133, 1248)
- Test: `kermit-ds/tests/parquet_tests.rs` (lines 57–70)
- Test: `kermit-ds/tests/hash_trie_tests.rs` (236, 241–245, 314–323, 337, 395)
- Test (unchanged, must pass): `kermit/tests/result_allocation.rs`, `kermit/tests/join_tests.rs` (HashTrie cells), `kermit/src/db.rs` `family_walk_tests`, `kermit/src/db/database.rs` `column_distinct_leaves_a_lazy_trie_as_built`

#### Why ids compare as strictly as tuples did (the identity argument)

Every constructor keeps the batch it is given as the trie's buffer, unchanged
and unreordered (`from_tuples_in_bulk`, `from_tuples_incrementally` and
`from_tuples_partitioned` all store `tuples` as received; only an empty batch
of the wrong arity is replaced by an empty batch of the header's). Two builds
of one input therefore own equal buffers, so two equal id lists name the same
tuples, in the same order. The identity tests compared chains, singletons and
pending lists tuple for tuple; comparing their ids is exactly as strict,
**provided the buffers are equal**. `assert_same_trie` and
`assert_equivalent_trie` therefore first assert `a.tuples() == b.tuples()`
and equal buffer capacity, so the premise is checked, not assumed. The one
test whose two tries own buffers of different capacity
(`the_default_builds_the_trie_it_built_before`: `from_tuples` against
`with_config` + `insert_all`, where `insert` grows the buffer by amortised
doubling) compares the buffers' rows and the trie node for node, not the
total heap (Step 7.m).

**Chain ids stay in input order in every mode** (Step 16's new test reads
e.g. ids `[0, 2]`): `bulk` groups `0..n` in ascending order and every child
list keeps its parent list's order (a `Vec` push appends); `incremental`
inserts ids `0..n` in ascending order; `radix:K`/`parallel:N` partition
stably (`partition` scans ids in order; `scatter_rows` keeps input order per
partition) and their scratch roots group each partition in order;
`presized:N` pushes each partition in order and the deferred tail sorted by
id. In `presized:N` a key's rows go either all to its run or all to the tail:
once a probe for an absent key overflows its region, the region only fills
further, so every later row of that key overflows too.

#### Pending-list capacities over `Vec<RowId>` (re-derived)

std's `RawVec` grows to `max(2·cap, required, MIN_NON_ZERO_CAP)`, and
`MIN_NON_ZERO_CAP` is 4 for every element of 2 to 1024 bytes: the same 4 for
a 4-byte `RowId` as for the 24-byte `Vec<usize>` the lists held before.

| list | `incremental` (`insert_at`) | `bulk` (`Vec::new` + push) | `child` does |
|---|---|---|---|
| 1 id, pruning off | `vec![row]`: cap 1 | cap 4 | `shrink_to(1)` → 1 |
| 2 ids, pruning on (unprune) | `Vec::with_capacity(2)`: cap 2 | cap 4 | `shrink_to(2)` → 2 |
| 2 ids, pruning off | 1 → `max(2, 2, 4)` = 4 | 4 | nothing |
| 3–4 ids, pruning on | 2 → `max(4, 3, 4)` = 4 | 4 | nothing |
| ≥ 5 ids | 8, 16, … | 8, 16, … | nothing |

So `child`'s shrink rule is unchanged, and lazy lists stay byte-identical
across modes; `bulk_builds_the_incremental_trie_*` (lazy configs) pins it.
Leaf chains need no rule: both builds create them by `Vec::new` + push.

- [ ] **Step 1: Write the failing storage tests.** In
  `kermit-ds/src/ds/hash_trie/implementation.rs`, `mod tests`:

  Replace `collect_tuples_recovers_input_as_multiset` (lines 950–966) by:
  ```rust
      /// `collect_tuples` copies the trie's buffer: every stored tuple, in
      /// arrival order, an `insert`ed duplicate included (#111).
      #[test]
      fn collect_tuples_returns_the_stored_tuples_in_arrival_order() {
          let mut trie: HashTrie =
              HashTrie::from_tuples(2.into(), vec![vec![1, 2], vec![1, 3], vec![2, 4]]);
          assert_eq!(trie.collect_tuples().to_vecs(), vec![
              vec![1, 2],
              vec![1, 3],
              vec![2, 4]
          ]);
          trie.insert(vec![1, 2]);
          assert_eq!(trie.collect_tuples().to_vecs(), vec![
              vec![1, 2],
              vec![1, 3],
              vec![2, 4],
              vec![1, 2]
          ]);
      }
  ```
  After `heap_size_deterministic_across_rebuilds` (line 996), add:
  ```rust
      /// `space` after #111: the buffer, every table's bucket array, and four
      /// bytes per id slot of every chain. A tuple is counted once, in the
      /// buffer; no chain holds a `Vec` per tuple.
      #[test]
      fn heap_size_counts_the_buffer_the_tables_and_four_bytes_per_id() {
          use crate::HeapSize;
          let trie: HashTrie =
              HashTrie::from_tuples(2.into(), vec![vec![1, 2], vec![1, 3], vec![4, 5]]);
          let HashTrieNode::Inner(root) = &trie.root else {
              panic!("arity 2 has an Inner root")
          };
          let mut tables = root.shell_heap_bytes();
          let mut id_slots = 0;
          for (_, child) in root.iter() {
              let HashTrieNode::Leaf(leaf) = child else {
                  panic!("depth 1 of arity 2 is the leaf")
              };
              tables += leaf.shell_heap_bytes();
              id_slots += leaf.iter().map(|(_, chain)| chain.capacity()).sum::<usize>();
          }
          // Three one-row chains, each grown from empty to std's first
          // capacity of 4.
          assert_eq!(id_slots, 12);
          assert!(trie.tuples().heap_size_bytes() >= 6 * std::mem::size_of::<usize>());
          assert_eq!(
              trie.heap_size_bytes(),
              trie.tuples().heap_size_bytes() + tables + id_slots * std::mem::size_of::<RowId>()
          );
      }

      /// `insert` appends its tuple to the buffer and places the new row's id:
      /// a duplicate joins its twin's chain, after it.
      #[test]
      fn insert_appends_its_row_and_chains_the_new_id() {
          let mut trie: HashTrie = HashTrie::from_tuples(2.into(), vec![vec![1, 2]]);
          trie.insert(vec![1, 2]);
          assert_eq!(trie.tuples().to_vecs(), vec![vec![1, 2], vec![1, 2]]);
          let HashTrieNode::Inner(root) = &trie.root else {
              panic!("arity 2 has an Inner root")
          };
          let Some((_, HashTrieNode::Leaf(leaf))) = root.iter().next() else {
              panic!("one root key, whose child is the leaf")
          };
          let Some((_, chain)) = leaf.iter().next() else {
              panic!("one leaf key")
          };
          assert_eq!(chain, &vec![0, 1]);
      }
  ```
  Replace `for_each_tuple_visits_what_collect_tuples_returns` (lines 1440–1467) by
  two helpers and one test. `for_each_tuple` stays a trie walk (it is what `bench ds`
  times), so the test pins that it walks: the arrival order below interleaves the
  first keys, and only a walk lends each root bucket's tuples together.
  ```rust
      /// The tuples `for_each_tuple` lends, in the order it lends them.
      fn walked<P: PruningPolicy>(trie: &HashTrie<SipHashStrategy, P>) -> Vec<Vec<usize>> {
          let mut visited = Vec::new();
          trie.for_each_tuple(|t| visited.push(t.to_vec()));
          visited
      }

      /// The tuples `TupleScan::scan_tuples` lends, in the order it lends them.
      fn scanned<P: PruningPolicy>(trie: &HashTrie<SipHashStrategy, P>) -> Vec<Vec<usize>> {
          use crate::tuple_scan::TupleScan;
          let mut visited = Vec::new();
          trie.scan_tuples(|t| visited.push(t.to_vec()));
          visited
      }

      /// `for_each_tuple` walks the trie (`bench ds` times it, #79), reading
      /// each row through its chain or `Singleton` id: it lends the stored
      /// multiset, a duplicate in one leaf chain and (pruned) a `Singleton`
      /// subtrie included, one subtrie at a time. `scan_tuples` lends the
      /// same multiset straight from the buffer, in arrival order, which is
      /// what `collect_tuples` copies (#111).
      #[test]
      fn for_each_tuple_walks_the_trie_and_scan_tuples_reads_the_buffer() {
          // Arrival order interleaves first keys 1 and 7, so only a walk
          // visits each first key's tuples together.
          let tuples = vec![
              vec![1, 2, 3],
              vec![7, 8, 9],
              vec![1, 2, 3],
              vec![1, 5, 6],
              vec![1, 2, 4],
          ];
          let mut multiset = tuples.clone();
          multiset.sort();
          let plain: HashTrie = HashTrie::from_tuples(3.into(), tuples.clone());
          let compact = pruned(3, tuples.clone());
          for (name, mut walk, scan, collected) in [
              (
                  "NoPruning",
                  walked(&plain),
                  scanned(&plain),
                  plain.collect_tuples().to_vecs(),
              ),
              (
                  "SingletonPruning",
                  walked(&compact),
                  scanned(&compact),
                  compact.collect_tuples().to_vecs(),
              ),
          ] {
              assert_eq!(scan, tuples, "{name}: scan_tuples reads the buffer");
              assert_eq!(collected, tuples, "{name}: collect_tuples copies the buffer");
              // One run of first keys per root bucket: 1 and 7 hash apart, so
              // a walk lends two runs where the buffer holds three.
              let mut first_keys: Vec<usize> = walk.iter().map(|t| t[0]).collect();
              first_keys.dedup();
              assert_eq!(first_keys.len(), 2, "{name}: for_each_tuple walks the trie");
              walk.sort();
              assert_eq!(walk, multiset, "{name}: for_each_tuple lends the stored multiset");
          }
      }
  ```
  In `mod lazy_tests`, `lazy_build_leaves_every_root_child_unexpanded` (lines
  1554–1562) gains, before its closing brace:
  ```rust
          let HashTrieNode::Unexpanded(pending) = child(&trie, 1) else {
              panic!("a lazy root bucket holds an Unexpanded child")
          };
          assert_eq!(*pending.pending(), vec![0, 1, 2], "row ids, in input order");
  ```
  (Step 7.l rewrites that test's other lines for the new `pending_of`.)

- [ ] **Step 2: Run them to verify they fail.**
  ```bash
  CARGO_BUILD_JOBS=2 cargo test -p kermit-ds --lib hash_trie::implementation
  ```
  Expected: compile errors (`no method named to_vecs found for struct Vec`,
  `no method named tuples found`, `expected Vec<Vec<usize>>, found integer`
  on `vec![0, 1]`): the trie does not own a buffer yet.

- [ ] **Step 3: `pruning.rs`, the payload (Step 14 edits `SingletonFrame`).**
  Line 13: `use kermit_iters::LayoutOption;` →
  ```rust
  use kermit_iters::{LayoutOption, RowId};
  ```
  (Step 14 reuses it). Module doc lines 4–6:
  ```rust
  //! Singleton pruning (SIGMOD 2020 §3.3.1, Figure 5) stores a subtrie that
  //! holds exactly one tuple as that tuple's row id instead of one hash table
  //! per remaining level. It is a *shape*: fixed at construction, it changes which
  ```
  Lines 15–24 (the trait) become:
  ```rust
  /// What a `HashTrieNode::Singleton` stores: the [`RowId`] of its one tuple,
  /// in the trie's buffer, when pruning is on; [`Never`] when it is off, which
  /// makes the variant uninhabited.
  pub trait SingletonPayload {
      /// Wrap `row` as the payload of a pruned subtrie.
      fn from_row(row: RowId) -> Self;
      /// The id of the row stored below the pruned node. A reference into the
      /// node, so an iterator frame can hold it for the trie's lifetime and
      /// lend it as a one-id chain (`slice::from_ref`).
      fn row(&self) -> &RowId;
      /// Consume the payload, yielding the id it stored.
      fn into_row(self) -> RowId;
  }
  ```
  `PruningPolicy`'s doc, line 54: "so the prune and unprune paths can actually
  store a tuple" → "so the prune and unprune paths can actually store a row
  id". Lines 83–91 (`impl SingletonPayload for Never`):
  ```rust
  impl SingletonPayload for Never {
      fn from_row(_row: RowId) -> Self {
          unreachable!("NoPruning never constructs a Singleton (P::ENABLED is false)")
      }

      fn row(&self) -> &RowId { match *self {} }

      fn into_row(self) -> RowId { match self {} }
  }
  ```
  Lines 109–115 (`impl SingletonPayload for Vec<usize>`):
  ```rust
  impl SingletonPayload for RowId {
      fn from_row(row: RowId) -> Self { row }

      fn row(&self) -> &RowId { self }

      fn into_row(self) -> RowId { self }
  }
  ```
  Line 187, in `impl PruningPolicy for SingletonPruning`:
  `type Payload = Vec<usize>;` → `type Payload = RowId;`.
  Test lines 208–213:
  ```rust
      #[test]
      fn row_payload_round_trips_the_id() {
          let p = <RowId as SingletonPayload>::from_row(7);
          assert_eq!(p.row(), &7);
          assert_eq!(p.into_row(), 7);
      }
  ```
  **What `SingletonFrame` must borrow (for Step 14):** `&'a RowId`, taken
  from `payload.row()` on a node the iterator borrows for `'a` (or from the
  frame above it, `Descent::Deeper`), never a copy: `leaf_tuples` at a pruned
  leaf is `LeafRows::new(trie.tuples(), slice::from_ref(row))`, and an
  emulated level's hash is `H::hash(trie.tuples().row(*row)[depth])`.

- [ ] **Step 4: `node.rs`.** Line 14:
  ```rust
  use {
      super::{expansion::ExpansionPolicy, hash_table::HashTable, pruning::PruningPolicy},
      kermit_iters::RowId,
  };
  ```
  Module doc lines 1–3:
  ```rust
  //! Recursive node type for the hash trie. Inner levels carry child node
  //! tables; the deepest (leaf) level carries chains of row ids; a pruned
  //! subtrie carries its single tuple's row id directly. Every row id indexes
  //! the trie's buffer (`HashTrie::tuples`, #111).
  ```
  Lines 23–46 (the three variant docs and `Leaf`'s type):
  ```rust
      /// Leaf level: hash table whose values are chains. Each chain holds the
      /// row ids, in input order, of the tuples whose attribute hashes match
      /// the path of hashes from the root to this bucket; the tuples
      /// themselves live in the trie's buffer.
      Leaf(HashTable<Vec<RowId>>),
      /// Pruned subtrie: exactly one tuple lives below this point, so the
      /// remaining levels are not materialised. The iterator emulates them
      /// from the tuple (see `hash_trie_iter.rs`). Never the root.
      ///
      /// Holds `P::Payload`: the tuple's row id when pruning is on, the
      /// uninhabited `Never` when it is off. rustc's layout omits uninhabited
      /// variants, so under `NoPruning` this variant costs nothing and every
      /// arm handling it compiles to the pre-pruning code path. The two size
      /// tests pin that: `node_does_not_grow_under_the_pruning_policy`
      /// (`implementation.rs`) and `off_frame_is_the_bare_table_pair`
      /// (`hash_trie_iter.rs`).
      Singleton(P::Payload),
      /// Unexpanded child (lazy child expansion, SIGMOD 2020 Figure 6): the
      /// row ids of the tuples below this bucket, kept as a list until a
      /// probe first opens it, then the table built from them. Never the
      /// root. Holds `E::Pending<Self>`: `Box<LazyChild<Self>>` when lazy, the
      /// uninhabited `Never` when eager, so under `EagerExpansion` this variant
      /// costs nothing (pinned by `node_does_not_grow_under_the_expansion_policy`).
      /// `HashTrie::resolve` is the one place that expands it.
      Unexpanded(E::Pending<HashTrieNode<P, E>>),
  ```

- [ ] **Step 5: `expansion.rs`.** Imports (lines 18–22):
  ```rust
  use {
      super::pruning::Never,
      kermit_iters::{LayoutOption, RowId},
      std::cell::{OnceCell, Ref, RefCell},
  };
  ```
  Module doc lines 4–7: "Every child below it keeps its tuples as a list
  until a probe first opens it" → "Every child below it keeps its tuples' row
  ids as a list until a probe first opens it". Replace lines 24–46 (the
  trait) by:
  ```rust
  /// What a `HashTrieNode::Unexpanded` holds: `Box<LazyChild<N>>` under
  /// [`LazyExpansion`], [`Never`] under [`EagerExpansion`].
  pub trait PendingChild<N>: Sized {
      /// An unexpanded child holding `rows`, the row ids of its tuples in the
      /// trie's buffer, in insertion order.
      fn from_rows(rows: Vec<RowId>) -> Self;
      /// The built table, once a probe has expanded this child.
      fn built(&self) -> Option<&N>;
      /// [`built`](Self::built) for `insert`, which holds the trie mutably.
      fn built_mut(&mut self) -> Option<&mut N>;
      /// The row ids not yet built into a table; empty once expanded.
      fn pending(&self) -> Ref<'_, Vec<RowId>>;
      /// Appends `row` to an unexpanded child. Callers check
      /// [`built_mut`](Self::built_mut) first.
      fn push(&mut self, row: RowId);
      /// The built table, building it first from the pending ids if no
      /// probe has yet. The list is moved into `build`, never copied, so an
      /// expanded child keeps no list.
      fn expand(&self, build: impl FnOnce(Vec<RowId>) -> N) -> &N;
      /// Heap bytes of the payload itself (the box), excluding the pending
      /// ids and the built table, which `heap_size_bytes` counts separately.
      fn own_heap_bytes(&self) -> usize;
  }
  ```
  Lines 66–82 (`impl<N> PendingChild<N> for Never`):
  ```rust
  impl<N> PendingChild<N> for Never {
      fn from_rows(_rows: Vec<RowId>) -> Self {
          unreachable!("EagerExpansion never constructs an Unexpanded child (E::LAZY is false)")
      }

      fn built(&self) -> Option<&N> { match *self {} }

      fn built_mut(&mut self) -> Option<&mut N> { match *self {} }

      fn pending(&self) -> Ref<'_, Vec<RowId>> { match *self {} }

      fn push(&mut self, _row: RowId) { match *self {} }

      fn expand(&self, _build: impl FnOnce(Vec<RowId>) -> N) -> &N { match *self {} }

      fn own_heap_bytes(&self) -> usize { match *self {} }
  }
  ```
  Lines 84–127 (`LazyChild` and its impl):
  ```rust
  /// An unexpanded child: its tuples' row ids until a probe reaches it, the
  /// table built from them afterwards.
  ///
  /// `RefCell` and `OnceCell` make lazy tries `!Sync` (still `Send`). Nothing
  /// in the workspace shares a relation across threads; a parallel prober
  /// would need a different cell.
  pub struct LazyChild<N> {
      /// The row ids of the tuples below this bucket, in insertion order.
      /// Moved into the build by expansion, leaving an empty, unallocated
      /// vector.
      pending: RefCell<Vec<RowId>>,
      /// The table this level would have held, once a probe has reached it.
      built: OnceCell<N>,
  }

  impl<N> PendingChild<N> for Box<LazyChild<N>> {
      fn from_rows(rows: Vec<RowId>) -> Self {
          Box::new(LazyChild {
              pending: RefCell::new(rows),
              built: OnceCell::new(),
          })
      }

      fn built(&self) -> Option<&N> { self.built.get() }

      fn built_mut(&mut self) -> Option<&mut N> { self.built.get_mut() }

      fn pending(&self) -> Ref<'_, Vec<RowId>> { self.pending.borrow() }

      fn push(&mut self, row: RowId) {
          debug_assert!(self.built.get().is_none(), "push into an expanded child");
          self.pending.get_mut().push(row);
      }

      // A visitor that opens an iterator on the same trie while
      // `for_each_tuple` holds `pending()` makes `borrow_mut` panic here
      // (`BorrowMutError`): a loud internal panic, not a wrong answer. No
      // caller in the workspace does that.
      fn expand(&self, build: impl FnOnce(Vec<RowId>) -> N) -> &N {
          self.built
              .get_or_init(|| build(std::mem::take(&mut *self.pending.borrow_mut())))
      }

      fn own_heap_bytes(&self) -> usize { std::mem::size_of::<LazyChild<N>>() }
  }
  ```
  Tests (lines 158–213): `type Lazy = Box<LazyChild<Vec<Vec<usize>>>>;` →
  `type Lazy = Box<LazyChild<Vec<RowId>>>;`, and replace the three tests that
  use it:
  ```rust
      #[test]
      fn expand_moves_the_pending_rows_once() {
          let mut child = Lazy::from_rows(vec![0]);
          child.push(1);
          assert!(child.built().is_none());
          assert_eq!(*child.pending(), vec![0, 1]);
          let mut calls = 0;
          let built = child.expand(|rows| {
              calls += 1;
              rows
          });
          assert_eq!(built, &vec![0, 1]);
          // A second expand returns the same table without building again.
          let again = child.expand(|_| unreachable!("already built"));
          assert!(std::ptr::eq(built, again));
          assert_eq!(calls, 1);
          assert!(child.pending().is_empty());
          assert_eq!(child.pending().capacity(), 0);
      }

      #[test]
      fn built_mut_reaches_the_expanded_table() {
          let mut child = Lazy::from_rows(vec![0]);
          assert!(child.built_mut().is_none());
          child.expand(|rows| rows);
          child.built_mut().unwrap().push(1);
          assert_eq!(child.built(), Some(&vec![0, 1]));
      }

      #[test]
      fn own_heap_bytes_is_the_box() {
          let child = Lazy::from_rows(Vec::new());
          assert_eq!(
              child.own_heap_bytes(),
              std::mem::size_of::<LazyChild<Vec<RowId>>>()
          );
      }
  ```

- [ ] **Step 6: `bulk.rs`, Algorithm 2 over row ids.** Module doc lines 30–34
  (the first "kermit's" bullet):
  ```rust
  //! - A list is a `Vec<RowId>` per bucket: 4-byte ids of rows in the trie's
  //!   buffer (`Tuples`, #111). Umbra threads its lists through an 8-byte
  //!   chain pointer reserved in each materialised tuple (§3.3.2); that needs
  //!   the partitioned copy of the buffer (#101 layer 3).
  ```
  Lines 41–46 stay true as written ("every list keeps input order, because a
  `Vec` push appends"). Replace lines 48–149 (imports through
  `build_child_table`) by:
  ```rust
  use {
      super::{
          config::HashTrieConfig,
          expansion::{ExpansionPolicy, PendingChild},
          hash_table::HashTable,
          implementation::HashTrie,
          node::HashTrieNode,
          pruning::{PruningPolicy, SingletonPayload},
      },
      crate::morsel::row_id,
      kermit_iters::{HashStrategy, RowId, Tuples},
  };

  /// A list of row ids in input order: Algorithm 2's `L`. The rows live in
  /// the trie's buffer.
  pub(super) type TupleList = Vec<RowId>;

  /// Every row id of `tuples`, in input order: Algorithm 2's first list,
  /// which is never materialised. `row_id` (T5's, in `morsel.rs`) converts
  /// the length; a batch never holds more than `RowId::MAX` rows.
  pub(super) fn all_rows(tuples: &Tuples) -> std::ops::Range<RowId> { 0..row_id(tuples.len()) }

  impl<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy> HashTrie<H, P, E> {
      /// Algorithm 2: the table at `depth` over the rows of `tuples` that
      /// `list` names, allocated at `2^log2_capacity` buckets.
      pub(super) fn build(
          tuples: &Tuples, depth: usize, arity: usize, list: impl IntoIterator<Item = RowId>,
          log2_capacity: u32, config: HashTrieConfig,
      ) -> HashTrieNode<P, E> {
          let lists = Self::group(tuples, depth, list, log2_capacity, config);
          Self::build_nested(tuples, depth, arity, lists, config)
      }

      /// Lines 3–7: a table of `2^log2_capacity` buckets holding `list`'s row
      /// ids, each pushed onto the list in the bucket of its row's attribute
      /// hash at `depth`. The table grows under the load factor if its keys
      /// outgrow it, as `insert_at`'s tables do.
      pub(super) fn group(
          tuples: &Tuples, depth: usize, list: impl IntoIterator<Item = RowId>, log2_capacity: u32,
          config: HashTrieConfig,
      ) -> HashTable<TupleList> {
          let mut lists = HashTable::with_log2_capacity(log2_capacity);
          for row in list {
              let hash = H::hash(tuples.row(row)[depth]);
              lists
                  .entry_or_insert_with(hash, config.load_factor, Vec::new)
                  .push(row);
          }
          lists
      }

      /// Lines 8–15: each bucket's list replaced by the node built from it,
      /// in bucket order (line 9). At the last attribute each list is stored
      /// itself (line 15), so the table of lists is the leaf.
      pub(super) fn build_nested(
          tuples: &Tuples, depth: usize, arity: usize, lists: HashTable<TupleList>,
          config: HashTrieConfig,
      ) -> HashTrieNode<P, E> {
          // `>=`, as `make_root_sized`'s `arity <= 1`, so the unsupported nullary
          // root is a leaf under both builds.
          if depth + 1 >= arity {
              return HashTrieNode::Leaf(lists);
          }
          HashTrieNode::Inner(lists.map(|list| Self::child(tuples, depth + 1, arity, list, config)))
      }

      /// The node a bucket's `list` becomes, at `depth`: a `Singleton` when
      /// pruning is on and one tuple lives below (§3.3.1), an `Unexpanded`
      /// child under lazy expansion (§3.3.1; `HashTrie::resolve` builds its
      /// table by [`build_child_table`](Self::build_child_table) on the first
      /// probe), and otherwise the table line 11 builds.
      pub(super) fn child(
          tuples: &Tuples, depth: usize, arity: usize, mut list: TupleList, config: HashTrieConfig,
      ) -> HashTrieNode<P, E> {
          if P::ENABLED && list.len() == 1 {
              return HashTrieNode::Singleton(P::Payload::from_row(list[0]));
          }
          if E::LAZY {
              // `insert_at` starts a pending list as `vec![row]`, capacity 1,
              // or after an unprune as `Vec::with_capacity(2)`. A list grown
              // from empty reaches capacity 4 on its first push (std's
              // smallest non-zero capacity for a 4-byte `RowId`, as for the
              // 24-byte tuples the lists held before #111), and from there
              // both grow alike. Shrinking the two short cases keeps the trie
              // byte-identical to `incremental`'s, so `space` cannot move, and
              // keeps a one-row pending list at one slot. Both the first
              // capacity of 4 and `shrink_to`'s exact result are std
              // implementation details; the identity tests pin them.
              let first_capacity = if P::ENABLED {
                  2
              } else {
                  1
              };
              if list.len() == first_capacity {
                  list.shrink_to(first_capacity);
              }
              return HashTrieNode::Unexpanded(E::Pending::from_rows(list));
          }
          Self::build_child_table(tuples, depth, arity, list, config)
      }

      /// The table a bucket's `list` becomes at `depth` when it is built
      /// (line 11): eagerly by [`child`](Self::child), or on the first probe by
      /// `HashTrie::resolve` under lazy expansion. One definition, so a child
      /// expanded from a list is the eager child built from that list, size
      /// included.
      pub(super) fn build_child_table(
          tuples: &Tuples, depth: usize, arity: usize, list: TupleList, config: HashTrieConfig,
      ) -> HashTrieNode<P, E> {
          let log2_capacity = config.child_log2_capacity(list.len());
          Self::build(tuples, depth, arity, list, log2_capacity, config)
      }
  }
  ```
  (`build` is instantiated at `Range<RowId>` from the root and at
  `TupleList` from `build_child_table`; nothing recurses through a third
  type.)

  **Tests.** `check_identity` (lines 188–203) is unchanged: `inputs` now
  yields `Tuples` (Step 10), which `from_tuples_incrementally` and
  `from_tuples_in_bulk` take. Replace
  `bulk_keeps_chain_order_under_colliding_hashes` (lines 231–268) by:
  ```rust
      /// Chain order, explicitly: under the colliding strategy, first keys 1
      /// and 11 share a root bucket and last keys 3 and 13 share a leaf chain,
      /// so distinct tuples meet in one chain, which must list their ids in
      /// input order as the per-tuple build does. Under SipHash or FxHash a
      /// chain holds only equal tuples, so only a colliding fixture can see
      /// its order.
      #[test]
      fn bulk_keeps_chain_order_under_colliding_hashes() {
          let tuples = vec![vec![1, 3], vec![11, 13], vec![1, 13], vec![11, 3], vec![
              1, 3,
          ]];
          fn check<P: PruningPolicy, E: ExpansionPolicy>(tuples: &[Vec<usize>]) {
              for config in incremental_configs() {
                  let incremental = HashTrie::<Mod10HashStrategy, P, E>::from_tuples_incrementally(
                      2.into(),
                      config,
                      Tuples::from(tuples.to_vec()),
                  );
                  let bulk = HashTrie::<Mod10HashStrategy, P, E>::from_tuples_in_bulk(
                      2.into(),
                      config,
                      Tuples::from(tuples.to_vec()),
                  );
                  assert_same_trie(
                      &incremental,
                      &bulk,
                      &label::<Mod10HashStrategy, P, E>(2, config, "colliding chain"),
                  );
              }
          }
          check::<NoPruning, EagerExpansion>(&tuples);
          check::<SingletonPruning, EagerExpansion>(&tuples);
          check::<NoPruning, LazyExpansion>(&tuples);
          check::<SingletonPruning, LazyExpansion>(&tuples);
          // The eager trie's one chain holds all five ids, in input order.
          let bulk: HashTrie<Mod10HashStrategy> = HashTrie::from_tuples_in_bulk(
              2.into(),
              HashTrieConfig::default(),
              Tuples::from(tuples),
          );
          let HashTrieNode::Inner(root) = bulk.root() else {
              panic!("arity 2 has an Inner root")
          };
          assert_eq!(root.len(), 1, "1 and 11 share a root bucket");
          let Some((_, HashTrieNode::Leaf(leaf))) = root.iter().next() else {
              panic!("the root's one child is the leaf")
          };
          assert_eq!(leaf.len(), 1, "3 and 13 share a leaf chain");
          let Some((_, chain)) = leaf.iter().next() else {
              panic!("one chain")
          };
          assert_eq!(chain, &vec![0, 1, 2, 3, 4]);
      }
  ```
  In `bulk_builds_the_incremental_trie_on_wide_and_skewed_inputs`, line 293:
  ```rust
                  for (input, tuples) in [
                      ("random", Tuples::from(random)),
                      ("half one key", Tuples::from(skewed)),
                  ] {
  ```
  (both builds already receive `tuples.clone()`). **Rewrite** (not delete)
  `bulk_build_rejects_a_wrong_arity` (lines 431–438): `from_tuples_in_bulk`
  now takes `Tuples`, so its mixed-arity input (`vec![vec![1, 2], vec![3]]`)
  can no longer reach it (`Tuples::from` refuses it first; T1 tests that). It
  becomes a uniform arity-3 batch under an arity-2 header, as T3 made
  `radix.rs:247` and `parallel.rs:644`, so it still pins the build's own
  check (`batch_for_header`, Step 7.g):
  ```rust
      /// A batch of another arity. (A batch of mixed arities never reaches a
      /// build: `Tuples::from` refuses it.)
      #[test]
      #[should_panic(expected = "does not match header arity")]
      fn bulk_build_rejects_a_wrong_arity() {
          let _: HashTrie = HashTrie::from_tuples_in_bulk(
              2.into(),
              HashTrieConfig::default(),
              Tuples::from(vec![vec![1, 2, 3]]),
          );
      }
  ```
  `tuples_below` (lines 321–333) and `check_children_sized` are unchanged
  (they read lengths and pass `tuples.clone()`).

- [ ] **Step 7: `implementation.rs`, the struct and its own buffer.**

  **7.a Imports** (lines 21–37):
  ```rust
  use {
      super::{
          build_mode::HashTrieBuildMode,
          bulk::{all_rows, TupleList},
          config::{ChildCapacity, HashTrieConfig, LoadFactor, RootCapacity},
          expansion::{EagerExpansion, ExpansionPolicy, PendingChild},
          node::HashTrieNode,
          parallel,
          pruning::{NoPruning, PruningPolicy, SingletonPayload},
          radix,
      },
      crate::{
          morsel::row_id,
          relation::{
              BuildModeRelation, ConfigurableRelation, ConfiguredBuildModeRelation, Relation,
              RelationHeader,
          },
      },
      kermit_iters::{
          ConfigOption, HashStrategy, JoinIterable, LayoutOption, RowId, SipHashStrategy, Tuples,
      },
      std::marker::PhantomData,
  };
  ```

  **7.b Struct doc and struct** (lines 44–60 invariants, then 100–113).
  Append to the `# Invariants` list (after line 59):
  ```rust
  /// - Every [`RowId`] in the trie (chain entry, `Singleton`, pending list)
  ///   indexes a row of the buffer, and every row of the buffer is named by
  ///   exactly one of them.
  ```
  Insert before `# Construction` (line 61):
  ```rust
  /// # Storage
  ///
  /// The trie owns its tuples as one row-major buffer ([`Tuples`], #111), in
  /// arrival order: the batch a build is given, kept as is, then every row
  /// `insert` appends. Below the tables everything names a tuple by its
  /// [`RowId`]: a leaf chain is a `Vec<RowId>`, a pruned `Singleton` one
  /// `RowId`, an unexpanded child's pending list a `Vec<RowId>`. The paper's
  /// leaves likewise point into a materialised buffer (VLDB 2020 §3.3.2); its
  /// pointers are 8 bytes where these ids are 4, and it threads each list
  /// through the tuples where these are a `Vec` per list (both kermit's; the
  /// second until #101 layer 3).
  ///
  ```
  Struct (lines 100–113):
  ```rust
  pub struct HashTrie<
      H: HashStrategy = SipHashStrategy,
      P: PruningPolicy = NoPruning,
      E: ExpansionPolicy = EagerExpansion,
  > {
      header: RelationHeader,
      /// Every stored tuple, in arrival order; its length is the multiset
      /// count (duplicates count). Chains, singletons and pending lists hold
      /// ids of its rows.
      tuples: Tuples,
      root: HashTrieNode<P, E>,
      /// Runtime values, fixed at construction; read by `insert_at`.
      config: HashTrieConfig,
      _layout: PhantomData<(H, P, E)>,
  }
  ```
  The `tuple_count` field is gone; its five uses become `tuples.len()`
  (constructors below, `Cardinality`, and the test at line 1301, Step 7.k).

  **7.c `with_config_for`, `root`, `tuples`, read-back** (lines 130–229).
  Replace `with_config_for` (lines 130–149):
  ```rust
      /// A trie holding `config` and owning `tuples`, with an empty root sized
      /// for a build from them by [`HashTrieConfig::root_log2_capacity`]: 4
      /// buckets under `RootCapacity::Grow`, and under `Tuples` a capacity at
      /// which `tuples.len()` keys never make it grow. The per-tuple build
      /// passes its batch and then inserts every row (until it has, the buffer
      /// holds rows the root does not); [`ConfigurableRelation::with_config`]
      /// passes an empty batch. The bulk and partitioned builds size their
      /// roots by the same `root_log2_capacity`.
      pub(super) fn with_config_for(
          header: RelationHeader, config: HashTrieConfig, tuples: Tuples,
      ) -> Self {
          let root = Self::make_root_sized(header.arity(), config.root_log2_capacity(tuples.len()));
          Self {
              header,
              tuples,
              root,
              config,
              _layout: PhantomData,
          }
      }
  ```
  Keep `root()` (lines 151–153). Replace `collect_tuples`, `for_each_tuple`,
  `visit_at` and `collect_at` (lines 155–229) by:
  ```rust
      /// The trie's buffer: every stored tuple, in arrival order. Chains,
      /// singletons and pending lists hold ids of its rows; `HashTrieIter`
      /// lends a chain's rows from it (`LeafRows`).
      pub(crate) fn tuples(&self) -> &Tuples { &self.tuples }

      /// A copy of every stored tuple, in arrival order: the trie's buffer,
      /// cloned (#111). O(n · arity) time and space; to visit the tuples
      /// without copying them, use [`for_each_tuple`](Self::for_each_tuple)
      /// (a walk of the trie) or `TupleScan::scan_tuples` (the buffer).
      pub fn collect_tuples(&self) -> Tuples { self.tuples.clone() }

      /// Walk the trie depth-first, lending every stored tuple to `visit`:
      /// each leaf chain's rows in chain order, and each pruned `Singleton`'s
      /// row, read from the trie's buffer through its row id.
      ///
      /// The walk allocates nothing per tuple: O(n) time, O(arity) stack.
      /// The CLI's `bench ds` `iteration` and `end_to_end` metrics time this
      /// walk (issue #79), so it traverses the structure; scanning the buffer
      /// instead would time an array scan. `TupleScan::scan_tuples` is that
      /// scan, for callers that need only the multiset (#111).
      ///
      /// Under `LazyExpansion` an unexpanded child lends its pending rows in
      /// insertion order, and the walk expands nothing. A `visit` that opens
      /// an iterator on this same trie and reaches such a child panics
      /// (`BorrowMutError`).
      pub fn for_each_tuple<V: FnMut(&[usize])>(&self, mut visit: V) {
          Self::visit_at(&self.tuples, &self.root, &mut visit);
      }

      fn visit_at<V: FnMut(&[usize])>(tuples: &Tuples, node: &HashTrieNode<P, E>, visit: &mut V) {
          match node {
              | HashTrieNode::Inner(table) => {
                  for (_, child) in table.iter() {
                      Self::visit_at(tuples, child, visit);
                  }
              },
              | HashTrieNode::Leaf(table) => {
                  for (_, chain) in table.iter() {
                      for &row in chain {
                          visit(tuples.row(row));
                      }
                  }
              },
              | HashTrieNode::Singleton(payload) => visit(tuples.row(*payload.row())),
              | HashTrieNode::Unexpanded(pending) => match pending.built() {
                  | Some(built) => Self::visit_at(tuples, built, visit),
                  | None => {
                      for &row in pending.pending().iter() {
                          visit(tuples.row(row));
                      }
                  },
              },
          }
      }
  ```
  `collect_at` is gone: `collect_tuples` copies the buffer. `visit_at` is
  today's walk with each tuple read through its id (`tuples.row`), so the
  walk's order and its cost class are unchanged.

  **7.d `insert_at` on a row id** (lines 231–340). Doc first sentence
  (line 231): "Insert one tuple at the appropriate depth" → "Insert the row
  `row` of `tuples` at the appropriate depth". Body:
  ```rust
      pub(super) fn insert_at(
          tuples: &Tuples, node: &mut HashTrieNode<P, E>, depth: usize, arity: usize, row: RowId,
          load_factor: LoadFactor,
      ) {
          let key = tuples.row(row)[depth];
          let hash = H::hash(key);
          match node {
              | HashTrieNode::Inner(table) => {
                  if P::ENABLED && table.get(hash).is_none() {
                      // Fresh bucket: the subtrie below holds exactly one tuple,
                      // so store its row id instead of one table per remaining
                      // level. The closure always runs — absence was just
                      // proven — so this is two O(1) probes, kept over a
                      // special-cased insert for readability.
                      table.entry_or_insert_with(hash, load_factor, || {
                          HashTrieNode::Singleton(P::Payload::from_row(row))
                      });
                      return;
                  }
                  if E::LAZY && table.get(hash).is_none() {
                      // Fresh bucket, pruning off: defer the child's table
                      // (Figure 6). The row's id waits in the child's list
                      // until a probe opens it; `resolve` then builds the
                      // table. With pruning on, the block above stored a
                      // `Singleton` first.
                      table.entry_or_insert_with(hash, load_factor, || {
                          HashTrieNode::Unexpanded(E::Pending::from_rows(vec![row]))
                      });
                      return;
                  }
                  // The child lives at `depth + 1`; it is the leaf when that is
                  // the last attribute.
                  let child_is_leaf = Self::is_leaf_depth(depth + 1, arity);
                  let child = table.entry_or_insert_with(hash, load_factor, || {
                      HashTrieNode::new_table(child_is_leaf)
                  });
                  if E::LAZY {
                      // The bucket existed (absence returned above), so the
                      // closure did not run: under lazy expansion an `Inner`
                      // bucket holds a `Singleton` or an `Unexpanded` child,
                      // never a table.
                      match child {
                          | HashTrieNode::Unexpanded(pending) => match pending.built_mut() {
                              | Some(built) => {
                                  Self::insert_at(tuples, built, depth + 1, arity, row, load_factor)
                              },
                              | None => pending.push(row),
                          },
                          | HashTrieNode::Singleton(_) => {
                              // A second tuple below a pruned bucket: the child
                              // becomes the unexpanded list of both. The evicted
                              // id goes first, as an eager unprune re-inserts it
                              // first, so expansion later builds the eager table
                              // (under `child-capacity=grow`; see `resolve`).
                              let list = HashTrieNode::Unexpanded(E::Pending::from_rows(
                                  Vec::with_capacity(2),
                              ));
                              let HashTrieNode::Singleton(evicted) = std::mem::replace(child, list)
                              else {
                                  unreachable!("matched Singleton above")
                              };
                              let HashTrieNode::Unexpanded(pending) = child else {
                                  unreachable!("replaced by an Unexpanded child just above")
                              };
                              pending.push(evicted.into_row());
                              pending.push(row);
                          },
                          | HashTrieNode::Inner(_) | HashTrieNode::Leaf(_) => unreachable!(
                              "a lazy Inner bucket holds a Singleton or an Unexpanded child"
                          ),
                      }
                      return;
                  }
                  if P::ENABLED && matches!(child, HashTrieNode::Singleton(_)) {
                      // Unprune: a second tuple has arrived, so the subtrie no
                      // longer holds exactly one. Swap in the table this level
                      // would have had and re-insert the evicted row ahead of
                      // the new one; the recursion re-prunes wherever the two
                      // diverge.
                      let replacement = HashTrieNode::new_table(child_is_leaf);
                      let HashTrieNode::Singleton(evicted) = std::mem::replace(child, replacement)
                      else {
                          unreachable!("matched Singleton above")
                      };
                      Self::insert_at(
                          tuples,
                          child,
                          depth + 1,
                          arity,
                          evicted.into_row(),
                          load_factor,
                      );
                  }
                  Self::insert_at(tuples, child, depth + 1, arity, row, load_factor);
              },
              | HashTrieNode::Leaf(table) => {
                  let chain = table.entry_or_insert_with(hash, load_factor, Vec::new);
                  chain.push(row);
              },
              | HashTrieNode::Singleton(_) | HashTrieNode::Unexpanded(_) => {
                  unreachable!(
                      "insert_at descends through Inner/Leaf only; singletons are unpruned and \
                       unexpanded children appended to by the parent"
                  )
              },
          }
      }
  ```

  **7.e `resolve`** (lines 342–375): doc lines 363–365 → "`HashTrieIter::open`
  is the only caller, so only a probe expands anything. `for_each_tuple` and
  `heap_size_bytes` read the pending list, and `collect_tuples` and
  `scan_tuples` the buffer, so no read-only call expands a child." Body:
  ```rust
      pub(crate) fn resolve<'t>(
          &'t self, node: &'t HashTrieNode<P, E>, depth: usize,
      ) -> &'t HashTrieNode<P, E> {
          match node {
              | HashTrieNode::Unexpanded(pending) => pending.expand(|rows| {
                  Self::build_child_table(&self.tuples, depth, self.header.arity(), rows, self.config)
              }),
              | other => other,
          }
      }
  ```
  (It already runs on `&self`; the rows it reads, inserted ones included, are
  in `self.tuples`.)

  **7.f `Relation` and `ConfigurableRelation`** (lines 380–425; signatures
  as T3 left them):
  ```rust
  impl<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy> Relation for HashTrie<H, P, E> {
      fn header(&self) -> &RelationHeader { &self.header }

      fn new(header: RelationHeader) -> Self { Self::with_config(header, HashTrieConfig::default()) }

      fn from_tuples(header: RelationHeader, tuples: impl Into<Tuples>) -> Self {
          Self::from_tuples_with_config(header, HashTrieConfig::default(), tuples)
      }

      /// Appends `tuple` to the buffer, then places the new row's id by
      /// `insert_at`.
      fn insert(&mut self, tuple: impl AsRef<[usize]>) {
          let tuple = tuple.as_ref();
          let arity = self.header.arity();
          assert_eq!(
              tuple.len(),
              arity,
              "tuple arity {} does not match relation arity {}",
              tuple.len(),
              arity
          );
          self.tuples.push(tuple);
          let row = row_id(self.tuples.len() - 1);
          Self::insert_at(&self.tuples, &mut self.root, 0, arity, row, self.config.load_factor);
      }

      fn insert_all(&mut self, tuples: impl Into<Tuples>) {
          let tuples: Tuples = tuples.into();
          for tuple in tuples.rows() {
              self.insert(tuple);
          }
      }
  }

  impl<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy> ConfigurableRelation
      for HashTrie<H, P, E>
  {
      type Config = HashTrieConfig;

      fn with_config(header: RelationHeader, config: HashTrieConfig) -> Self {
          let arity = header.arity();
          Self::with_config_for(header, config, Tuples::new(arity))
      }

      fn from_tuples_with_config(
          header: RelationHeader, config: HashTrieConfig, tuples: impl Into<Tuples>,
      ) -> Self {
          Self::from_tuples_in_bulk(header, config, tuples.into())
      }

      fn config(&self) -> &HashTrieConfig { &self.config }
  }
  ```
  (`self.insert(tuple)` with `tuple: &[usize]`, which is `AsRef<[usize]>`;
  `&self.tuples` and `&mut self.root` are disjoint field borrows.)

  **7.g The build constructors** (lines 435–604). In
  `from_tuples_with_config_and_build_mode`'s `# Panics` (lines 449–455),
  "Panics if any tuple's length does not equal `header.arity()`" → "Panics if
  `tuples` holds rows of another arity than `header.arity()`". Its body, with
  T3's `into_vecs()` gone:
  ```rust
      pub fn from_tuples_with_config_and_build_mode(
          header: RelationHeader, config: HashTrieConfig, mode: HashTrieBuildMode,
          tuples: impl Into<Tuples>,
      ) -> Self {
          let tuples: Tuples = tuples.into();
          match mode {
              | HashTrieBuildMode::Bulk => Self::from_tuples_in_bulk(header, config, tuples),
              | HashTrieBuildMode::Incremental => {
                  // The prerequisite: `insert_at` creates a child on its first
                  // tuple, before the child's list is known, so it cannot size
                  // it. The CLI rejects the pair first; here it is a broken
                  // invariant, like a wrong arity.
                  assert_eq!(
                      config.child_capacity,
                      ChildCapacity::Grow,
                      "hash-trie=incremental requires child-capacity=grow; got child-capacity={}",
                      config.child_capacity.axis_value(),
                  );
                  Self::from_tuples_incrementally(header, config, tuples)
              },
              | HashTrieBuildMode::Radix(bits) => {
                  Self::from_tuples_partitioned(header, config, tuples, |tuples, log2, arity| {
                      let mut root = Self::make_root_sized(arity, log2);
                      radix::fill_root::<H, P, E>(tuples, &mut root, arity, bits, config);
                      root
                  })
              },
              | HashTrieBuildMode::Parallel(threads) => {
                  Self::from_tuples_partitioned(header, config, tuples, |tuples, log2, arity| {
                      let mut root = Self::make_root_sized(arity, log2);
                      parallel::fill_root::<H, P, E>(tuples, &mut root, arity, threads, config);
                      root
                  })
              },
              | HashTrieBuildMode::Presized(threads) => {
                  // The prerequisite
                  // (docs/specs/2026-10-07-dependent-optimisations-design.md):
                  // the regions are cut from a root sized before any tuple
                  // arrives. The CLI rejects the pair first; here it is a
                  // broken invariant, like a wrong arity.
                  assert_eq!(
                      config.root_capacity,
                      RootCapacity::Tuples,
                      "hash-trie=presized:{} requires root-capacity=tuples; got root-capacity={}",
                      threads.get(),
                      config.root_capacity.axis_value(),
                  );
                  Self::from_tuples_partitioned(header, config, tuples, |tuples, log2, arity| {
                      parallel::fill_presized_root::<H, P, E>(tuples, log2, arity, threads, config)
                  })
              },
          }
      }
  ```
  Replace `assert_arities` (lines 509–521), `from_tuples_in_bulk`,
  `from_tuples_incrementally` and `from_tuples_partitioned` (lines 523–603) by:
  ```rust
      /// The batch a trie of `arity` stores: `tuples` itself, or, when it holds
      /// no rows and has another arity, an empty batch of `arity`: an empty
      /// literal carries no arity, and `insert` must later push rows of the
      /// header's.
      ///
      /// # Panics
      ///
      /// Panics if `tuples` holds rows of another arity, with the message the
      /// per-tuple check gave before #111.
      fn batch_for_header(arity: usize, tuples: Tuples) -> Tuples {
          if tuples.is_empty() && tuples.arity() != arity {
              return Tuples::new(arity);
          }
          assert_eq!(
              tuples.arity(),
              arity,
              "from_tuples: tuple arity {} does not match header arity {}",
              tuples.arity(),
              arity,
          );
          tuples
      }

      /// The `bulk` build: Algorithm 2 from the root (`bulk.rs`) over every
      /// row id of `tuples`, in input order, the root sized as every build
      /// sizes it, by [`HashTrieConfig::root_log2_capacity`] (#88). The trie
      /// keeps `tuples` as its buffer.
      ///
      /// # Panics
      ///
      /// Panics if `tuples` holds rows of another arity than `header.arity()`.
      pub(super) fn from_tuples_in_bulk(
          header: RelationHeader, config: HashTrieConfig, tuples: Tuples,
      ) -> Self {
          #[cfg(test)]
          BULK_BUILDS.with(|n| n.set(n.get() + 1));
          let arity = header.arity();
          let tuples = Self::batch_for_header(arity, tuples);
          let log2_capacity = config.root_log2_capacity(tuples.len());
          let root = Self::build(&tuples, 0, arity, all_rows(&tuples), log2_capacity, config);
          Self {
              header,
              tuples,
              root,
              config,
              _layout: PhantomData,
          }
      }

      /// The `incremental` build: one [`insert_at`](Self::insert_at) per row,
      /// in input order. This is the build `from_tuples_with_config` ran
      /// before #107; since #111 it places ids of rows of the batch it keeps,
      /// and checks the batch's arity once, before building.
      ///
      /// # Panics
      ///
      /// Panics if `tuples` holds rows of another arity than `header.arity()`.
      pub(super) fn from_tuples_incrementally(
          header: RelationHeader, config: HashTrieConfig, tuples: Tuples,
      ) -> Self {
          let arity = header.arity();
          let tuples = Self::batch_for_header(arity, tuples);
          let mut trie = Self::with_config_for(header, config, tuples);
          for row in all_rows(&trie.tuples) {
              Self::insert_at(&trie.tuples, &mut trie.root, 0, arity, row, config.load_factor);
          }
          trie
      }

      /// What the partitioned builds share (`radix:K`, `parallel:N`,
      /// `presized:N`): the batch's arity check, then `fill`, which builds the
      /// root over the batch at the capacity every build gives it
      /// (`HashTrieConfig::root_log2_capacity`, presized under
      /// `root-capacity=tuples`). The trie keeps the batch as its buffer.
      fn from_tuples_partitioned(
          header: RelationHeader, config: HashTrieConfig, tuples: Tuples,
          fill: impl FnOnce(&Tuples, u32, usize) -> HashTrieNode<P, E>,
      ) -> Self {
          let arity = header.arity();
          let tuples = Self::batch_for_header(arity, tuples);
          let root = fill(&tuples, config.root_log2_capacity(tuples.len()), arity);
          Self {
              header,
              tuples,
              root,
              config,
              _layout: PhantomData,
          }
      }
  ```
  In `BuildModeRelation::from_tuples_with_build_mode`'s `# Panics` (line 616):
  "Panics if any tuple's length does not equal `header.arity()`" → "Panics if
  `tuples` holds rows of another arity than `header.arity()`". The two
  build-mode impls' bodies are as T3 left them.

  **7.h `project` from the buffer** (lines 669–674, after the header code):
  ```rust
          // One buffer for the projection, in arrival order; one scratch row,
          // reused, so nothing is allocated per tuple.
          let mut projected = Tuples::with_capacity(columns.len(), self.tuples.len());
          let mut row = Vec::with_capacity(columns.len());
          for tuple in self.tuples.rows() {
              row.clear();
              row.extend(columns.iter().map(|&c| tuple[c]));
              projected.push(&row);
          }
          HashTrie::<H, P, E>::from_tuples_with_config(new_header, self.config, projected)
  ```

  **7.i `HeapSize`, `TupleScan`, `Cardinality`** (lines 678–696):
  ```rust
  impl<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy> crate::heap_size::HeapSize
      for HashTrie<H, P, E>
  {
      /// The buffer (capacity × 8 bytes), every table's bucket array, and
      /// every id list's capacity × 4 bytes: the chains, and the pending lists
      /// of unexpanded children. A pruned `Singleton`'s id lives inside its
      /// parent's bucket and adds nothing; each tuple is counted once, in the
      /// buffer.
      fn heap_size_bytes(&self) -> usize {
          self.tuples.heap_size_bytes() + node_heap_bytes(&self.root)
      }
  }

  /// The trie's buffer, in arrival order (#111): statistics need only the
  /// multiset, so the scan reads the rows without walking the trie, and it
  /// probes nothing, so it expands no unexpanded child.
  impl<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy> crate::tuple_scan::TupleScan
      for HashTrie<H, P, E>
  {
      fn scan_tuples(&self, mut visit: impl FnMut(&[usize])) {
          for tuple in self.tuples.rows() {
              visit(tuple);
          }
      }
  }

  impl<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy> crate::cardinality::Cardinality
      for HashTrie<H, P, E>
  {
      fn tuple_count(&self) -> usize { self.tuples.len() }
  }
  ```
  Lines 735–772 (`node_heap_bytes`, `tuple_list_heap_bytes`):
  ```rust
  fn node_heap_bytes<P: PruningPolicy, E: ExpansionPolicy>(node: &HashTrieNode<P, E>) -> usize {
      match node {
          | HashTrieNode::Inner(table) => {
              let shell = table.shell_heap_bytes();
              let children: usize = table.iter().map(|(_, child)| node_heap_bytes(child)).sum();
              shell + children
          },
          | HashTrieNode::Leaf(table) => {
              let shell = table.shell_heap_bytes();
              let chains: usize = table
                  .iter()
                  .map(|(_, chain)| tuple_list_heap_bytes(chain))
                  .sum();
              shell + chains
          },
          // The id is stored in the bucket; its row is in the buffer.
          | HashTrieNode::Singleton(_) => 0,
          | HashTrieNode::Unexpanded(pending) => {
              let below = match pending.built() {
                  | Some(built) => node_heap_bytes(built),
                  | None => tuple_list_heap_bytes(&pending.pending()),
              };
              pending.own_heap_bytes() + below
          },
      }
  }

  /// Heap bytes of a list of row ids: its buffer's capacity. The rows live
  /// in the trie's buffer and are counted there, once.
  // `&Vec`, not a slice: the list's own buffer is counted by `capacity()`.
  #[allow(clippy::ptr_arg)]
  fn tuple_list_heap_bytes(list: &TupleList) -> usize {
      list.capacity() * std::mem::size_of::<RowId>()
  }
  ```

  **7.j `mod tests`, pruning invariant** (lines 1171–1229) and its callers:
  ```rust
      /// Walks `trie`, asserting the pruning invariant at every inner bucket
      /// and returning the number of tuples stored below the root.
      ///
      /// Invariant: under `SingletonPruning` a child is `Singleton` iff
      /// exactly one tuple lives below it; under `NoPruning` no `Singleton`
      /// exists. The root is never a `Singleton`, and a `Singleton` sits
      /// under the buckets its own row hashes to.
      fn check_pruning_invariant<P: PruningPolicy, E: ExpansionPolicy>(
          trie: &HashTrie<SipHashStrategy, P, E>,
      ) -> usize {
          assert!(
              !matches!(trie.root, HashTrieNode::Singleton(_)),
              "the root is never a Singleton"
          );
          check_pruning_invariant_at(&trie.tuples, &trie.root, &mut Vec::new())
      }

      /// Recursive half of [`check_pruning_invariant`]. `prefix` is the
      /// sequence of bucket hashes taken from the root down to `node`, so a
      /// `Singleton` reached here must hash to every one of them — that is
      /// what pins it to the right *place*, not merely the right count.
      fn check_pruning_invariant_at<P: PruningPolicy, E: ExpansionPolicy>(
          tuples: &Tuples, node: &HashTrieNode<P, E>, prefix: &mut Vec<u64>,
      ) -> usize {
          match node {
              | HashTrieNode::Singleton(payload) => {
                  assert!(P::ENABLED, "Singleton found with pruning off");
                  let tuple = tuples.row(*payload.row());
                  for (d, &expected) in prefix.iter().enumerate() {
                      assert_eq!(
                          <SipHashStrategy as HashStrategy>::hash(tuple[d]),
                          expected,
                          "Singleton {tuple:?} is misplaced at depth {d}"
                      );
                  }
                  1
              },
              | HashTrieNode::Leaf(table) => table.iter().map(|(_, chain)| chain.len()).sum(),
              | HashTrieNode::Unexpanded(_) => {
                  unreachable!("the pruning invariant is checked on eager tries")
              },
              | HashTrieNode::Inner(table) => table
                  .iter()
                  .map(|(hash, child)| {
                      prefix.push(hash);
                      let below = check_pruning_invariant_at(tuples, child, prefix);
                      prefix.pop();
                      if P::ENABLED {
                          assert_eq!(
                              matches!(child, HashTrieNode::Singleton(_)),
                              below == 1,
                              "child holding {below} tuple(s) has wrong pruning state"
                          );
                      }
                      below
                  })
                  .sum(),
          }
      }
  ```
  Every caller `check_pruning_invariant(&trie.root)` becomes
  `check_pruning_invariant(&trie)`: lines 1235, 1241, 1258, 1268, 1277, 1286,
  1296, 1298, 1300.

  **7.k `mod tests`, the other chain readers** (each `collect_tuples` now
  returns `Tuples`; inputs already in ascending order now come back exactly):
  - `project_drops_columns` (lines 1006–1010) →
    `assert_eq!(projected.collect_tuples().to_vecs(), vec![vec![1], vec![1], vec![2]]);`
    (keep the comment on multiset semantics).
  - `project_reorders_columns` (lines 1018–1020) →
    `assert_eq!(projected.collect_tuples().to_vecs(), vec![vec![2, 1], vec![4, 3]]);`
  - `project_preserves_config` (lines 1103–1105) →
    `assert_eq!(projected.collect_tuples().to_vecs(), vec![vec![2], vec![4]]);`
  - `pruning_arity_1_has_no_singletons` (1260–1262) →
    `assert_eq!(trie.collect_tuples().to_vecs(), vec![vec![1], vec![2]]);`
  - `unprune_when_second_tuple_diverges_one_level_down` (1269–1271) →
    `assert_eq!(trie.collect_tuples().to_vecs(), vec![vec![1, 2, 3], vec![1, 4, 5]]);`
  - `unprune_when_second_tuple_shares_hashes_to_the_leaf` (1278–1280) →
    `assert_eq!(trie.collect_tuples().to_vecs(), vec![vec![1, 2, 3], vec![1, 2, 4]]);`
  - `unprune_on_exact_duplicate_keeps_multiset` (1287–1289) →
    `assert_eq!(trie.collect_tuples().to_vecs(), vec![vec![1, 2], vec![1, 2]]);`
  - `incremental_insert_unprunes_like_bulk_build` line 1301:
    `assert_eq!(trie.tuple_count, 3);` → `assert_eq!(trie.tuples.len(), 3);`
  - `pruned_build_is_insertion_order_independent` line 1311:
    `let (mut a, mut b) = (forward.collect_tuples().to_vecs(), backward.collect_tuples().to_vecs());`
    (keep both sorts: `backward` arrived reversed).
  - `node_does_not_grow_under_the_pruning_policy` doc (line 1332): "the `Vec`
    payload is smaller than either table variant" → "the `RowId` payload is
    smaller than either table variant".
  - `node_does_not_grow_under_the_expansion_policy` mirrors (lines 1360–1370):
    ```rust
          #[allow(dead_code)]
          enum Mirror {
              Inner(HashTable<()>),
              Leaf(HashTable<Vec<RowId>>),
          }
          #[allow(dead_code)]
          enum PrunedMirror {
              Inner(HashTable<()>),
              Leaf(HashTable<Vec<RowId>>),
              Singleton(RowId),
          }
    ```
  - `pruned_and_plain_collect_the_same_tuples` (lines 1434–1437) →
    `assert_eq!(plain.collect_tuples(), compact.collect_tuples());` (both are
    the input, in arrival order).
  - `cardinality_tests` (1470–1508): unchanged (`Tuples::len`).

  **7.l `mod lazy_tests`** (lines 1539–1548), `pending_of` reads ids through
  the trie's buffer:
  ```rust
      /// The pending tuples of an unexpanded child, read through `trie`'s
      /// buffer, or `None` for any other node or an expanded child.
      fn pending_of<P: PruningPolicy>(
          trie: &HashTrie<SipHashStrategy, P, LazyExpansion>, node: &HashTrieNode<P, LazyExpansion>,
      ) -> Option<Vec<Vec<usize>>> {
          match node {
              | HashTrieNode::Unexpanded(p) if p.built().is_none() => Some(
                  p.pending()
                      .iter()
                      .map(|&row| trie.tuples().row(row).to_vec())
                      .collect(),
              ),
              | _ => None,
          }
      }
  ```
  Callers `pending_of(X)` → `pending_of(&trie, X)`: lines 1558, 1561, 1568,
  1579, 1595 (`pending_of(&trie, grandchild)`), 1599, 1605, 1629, 1654.
  `insert_reaches_the_built_table_after_expansion_and_the_list_before` lines
  1633–1641 →
  ```rust
          assert_eq!(trie.collect_tuples().to_vecs(), vec![
              vec![1, 2],
              vec![1, 3],
              vec![9, 9],
              vec![1, 4],
              vec![9, 8]
          ]);
  ```
  `walks_read_pending_tuples_without_expanding` (1644–1656) becomes:
  ```rust
      #[test]
      fn walks_read_pending_tuples_without_expanding() {
          use crate::tuple_scan::TupleScan;
          let trie = Lazy::from_tuples(3.into(), tuples());
          let before = trie.heap_size_bytes();
          assert_eq!(trie.collect_tuples().to_vecs(), tuples());
          // The trie walk reads each unexpanded child's pending row ids.
          let mut walked = Vec::new();
          trie.for_each_tuple(|t| walked.push(t.to_vec()));
          walked.sort();
          assert_eq!(walked, tuples());
          // The scan reads the buffer, in arrival order.
          let mut scanned = Vec::new();
          trie.scan_tuples(|t| scanned.push(t.to_vec()));
          assert_eq!(scanned, tuples());
          assert!(pending_of(&trie, child(&trie, 1)).is_some());
          assert_eq!(trie.heap_size_bytes(), before, "a walk changes nothing");
      }
  ```
  (`tuples()` is already sorted, so the sorted walk equals it.)
  `projection_of_a_lazy_trie_holds_the_projected_tuples` (1677–1681) →
  `assert_eq!(trie.project(vec![2, 0]).collect_tuples().to_vecs(), vec![vec![3, 1], vec![4, 1], vec![6, 1], vec![9, 7]]);`
  `expanded_heap_is_the_eager_heap_plus_one_box_per_child` is unchanged: both
  tries own equal buffers, and an expanded list is emptied to capacity 0.

  **7.m `mod root_capacity_tests`**, import (line 1711):
  `identity::{assert_same_node, assert_same_trie, inputs},` →
  `identity::{assert_same_node, inputs},`. `check_default_identity`
  (lines 1766–1780):
  ```rust
      fn check_default_identity<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>() {
          for arity in 1..=3 {
              for (input, tuples) in inputs(arity) {
                  let label = label::<H, P, E>(&format!("arity {arity}, {input}"));
                  let built = HashTrie::<H, P, E>::from_tuples(arity.into(), tuples.clone());
                  let mut inserted =
                      HashTrie::<H, P, E>::with_config(arity.into(), HashTrieConfig::default());
                  inserted.insert_all(tuples);
                  // `insert` grows the buffer by amortised doubling, so the two
                  // buffers hold the same rows at different capacities. The
                  // trie, which holds only ids, must be identical.
                  assert_eq!(built.tuples(), inserted.tuples(), "{label}: buffer");
                  assert_same_node(built.root(), inserted.root(), &label);
                  assert_eq!(built.tuple_count(), inserted.tuple_count(), "{label}: tuples");
              }
          }
      }
  ```
  `check_children_unchanged` (1884–1928) compares chains with
  `a.value_at(idx) == b.value_at(at)`: now id lists, unchanged code; it holds
  because both tries own clones of one batch.

  **Wrong-arity tests in this file:** `insert_wrong_arity_panics` (line 904)
  reaches `insert`'s own assert, message unchanged. 
  `from_tuples_with_config_wrong_arity_panics` (line 1154) passes the uniform
  batch `vec![vec![1]]`, which `Tuples::from` accepts (arity 1), and reaches
  `batch_for_header`, whose message is the old one ("from_tuples: tuple
  arity 1 does not match header arity 2"); leave both as T3 left them.

- [ ] **Step 8: `radix.rs` over row ids.** Module doc (lines 1–13):
  ```rust
  //! The `radix:K` build of a [`HashTrie`](super::HashTrie): radix-partition
  //! the row ids on their rows' first attribute's hash, group each partition
  //! into a scratch root and build its children by Algorithm 2 (`bulk.rs`),
  //! then merge the scratch roots into the real one (SIGMOD 2020 §3.3.2;
  //! issue #91).
  //!
  //! The result is the trie the bulk build makes, bucket for bucket and
  //! capacity for capacity. A table's final layout depends only on the order
  //! in which its *new* keys arrive — `HashTable::entry_or_insert_with`
  //! returns an existing entry before its resize check — so the merge inserts
  //! the distinct root keys in the order they first appear in the input, as
  //! the bulk build does. A row's id is its input position, so that order is
  //! the order of the ids that introduced the keys. Below the root nothing
  //! needs care: the partition is stable, so each root key's child is built
  //! from the same list, in input order, as in the bulk build.
  ```
  Lines 15–159:
  ```rust
  use {
      super::{
          build_mode::RadixBits,
          bulk::{all_rows, TupleList},
          config::{HashTrieConfig, LoadFactor},
          expansion::ExpansionPolicy,
          hash_table::HashTable,
          implementation::HashTrie,
          node::HashTrieNode,
          pruning::PruningPolicy,
      },
      kermit_iters::{HashStrategy, RowId, Tuples},
  };

  /// A root entry ready to merge: the id of the row whose key first
  /// introduced it (its input position), its hash, and its finished value.
  pub(super) type Arrival<V> = (RowId, u64, V);

  /// Fills the empty `root` with the rows of `tuples` by the `radix:bits`
  /// build. Every row must have `arity` attributes; the caller checks.
  pub(super) fn fill_root<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>(
      tuples: &Tuples, root: &mut HashTrieNode<P, E>, arity: usize, bits: RadixBits,
      config: HashTrieConfig,
  ) {
      // One of the two stays empty: the root is `Inner` for arity ≥ 2 (values
      // are subtries, or under lazy expansion their pending lists) and `Leaf`
      // for arity 1 (values are chains).
      let mut subtries = Vec::new();
      let mut chains = Vec::new();
      for partition in partition::<H>(tuples, bits) {
          if partition.is_empty() {
              continue;
          }
          let (scratch, first_seen) =
              build_scratch_root::<H, P, E>(tuples, partition, arity, config);
          match scratch {
              | HashTrieNode::Inner(table) => {
                  take_in_arrival_order(table, &first_seen, &mut subtries)
              },
              | HashTrieNode::Leaf(table) => take_in_arrival_order(table, &first_seen, &mut chains),
              | HashTrieNode::Singleton(_) | HashTrieNode::Unexpanded(_) => {
                  unreachable!("a root is never pruned or unexpanded")
              },
          }
      }
      match root {
          | HashTrieNode::Inner(table) => {
              insert_in_first_appearance_order(table, subtries, config.load_factor)
          },
          | HashTrieNode::Leaf(table) => {
              insert_in_first_appearance_order(table, chains, config.load_factor)
          },
          | HashTrieNode::Singleton(_) | HashTrieNode::Unexpanded(_) => {
              unreachable!("a root is never pruned or unexpanded")
          },
      }
  }

  /// Splits the row ids of `tuples` into `2^bits` partitions on the top
  /// `bits` of `H::hash(row[0])`. A histogram pass sizes each partition
  /// exactly; a second pass then sends every id to its partition. Stable —
  /// each partition lists its ids in input order — which is what keeps every
  /// child identical to the bulk build's.
  fn partition<H: HashStrategy>(tuples: &Tuples, bits: RadixBits) -> Vec<TupleList> {
      let shift = 64 - u32::from(bits.get());
      // `bits <= 16`, so a partition number fits a `u16`.
      let partition_of: Vec<u16> = tuples
          .rows()
          .map(|row| (H::hash(row[0]) >> shift) as u16)
          .collect();
      let mut sizes = vec![0usize; 1 << bits.get()];
      for &p in &partition_of {
          sizes[usize::from(p)] += 1;
      }
      let mut partitions: Vec<TupleList> = sizes.into_iter().map(Vec::with_capacity).collect();
      for (row, p) in all_rows(tuples).zip(partition_of) {
          partitions[usize::from(p)].push(row);
      }
      partitions
  }

  /// Builds one partition of row ids into a scratch root of the real root's
  /// kind by Algorithm 2 (`bulk.rs`): the partition's ids grouped at the root,
  /// recording for each key the id that introduced it and the key's hash, in
  /// arrival order; then each key's child built from its list, as the bulk
  /// build builds it.
  pub(super) fn build_scratch_root<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>(
      tuples: &Tuples, partition: impl IntoIterator<Item = RowId>, arity: usize,
      config: HashTrieConfig,
  ) -> (HashTrieNode<P, E>, Vec<(RowId, u64)>) {
      // Lines 3–7 at the root. The scratch root starts at 4 buckets and
      // grows: its entries move into the real root, so its size is never
      // seen.
      let mut lists: HashTable<TupleList> = HashTable::new();
      let mut first_seen = Vec::new();
      for row in partition {
          let hash = H::hash(tuples.row(row)[0]);
          let keys_before = lists.len();
          lists
              .entry_or_insert_with(hash, config.load_factor, Vec::new)
              .push(row);
          if lists.len() > keys_before {
              first_seen.push((row, hash));
          }
      }
      // Lines 8–15.
      let scratch = HashTrie::<H, P, E>::build_nested(tuples, 0, arity, lists, config);
      (scratch, first_seen)
  }

  /// Moves every entry out of `scratch` into `out`, tagged with the id of the
  /// row whose key first introduced it. Bucket positions are read before the
  /// table is consumed, and each bucket is taken exactly once.
  pub(super) fn take_in_arrival_order<V>(
      scratch: HashTable<V>, first_seen: &[(RowId, u64)], out: &mut Vec<Arrival<V>>,
  ) {
      let positions: Vec<usize> = first_seen
          .iter()
          .map(|&(_, hash)| {
              scratch
                  .index_of(hash)
                  .expect("every arrival is in its scratch table")
          })
          .collect();
      let mut buckets = scratch.into_buckets();
      for (&(first, hash), position) in first_seen.iter().zip(positions) {
          let entry = buckets[position].take().expect("each bucket is taken once");
          debug_assert_eq!(entry.hash, hash);
          out.push((first, hash, entry.value));
      }
  }

  /// Inserts the merged entries into the real root in the order their keys
  /// first appeared in the input — the order the bulk build inserts them —
  /// so the root's buckets, length and capacity are the bulk build's.
  fn insert_in_first_appearance_order<V>(
      root: &mut HashTable<V>, mut arrivals: Vec<Arrival<V>>, load_factor: LoadFactor,
  ) {
      // First ids are distinct, so the unstable sort is deterministic.
      arrivals.sort_unstable_by_key(|&(first, ..)| first);
      for (_, hash, value) in arrivals {
          root.entry_or_insert_with(hash, load_factor, || value);
      }
  }
  ```
  (`Indexed` is gone: a row's id *is* its input position.) Tests: unchanged
  code. `build_mode_relation_uses_the_default_config` gets `Tuples` from
  `rows` (Step 10) and passes `tuples.clone()` / `tuples` as before;
  `radix_build_rejects_a_wrong_arity` (line 247) stays as T3 left it: its
  uniform arity-3 batch under an arity-2 header reaches `batch_for_header`
  (Step 7.g) through `from_tuples_partitioned`, and the message ("from_tuples:
  tuple arity 3 does not match header arity 2") still contains the expected
  "tuple arity".

- [ ] **Step 9: `parallel.rs` through `scatter_rows`.** Module doc lines 7–9:
  ```rust
  //! 1. **Partition.** [`scatter_rows`] sends every row's id (its input
  //!    position) into one of P partitions by the top log₂P bits of the row's
  //!    first attribute's hash; P is four per thread, rounded up to a power of
  //!    two.
  ```
  Lines 18–19: "Each partition lists its tuples in input order (`scatter`
  keeps it)" → "Each partition lists its row ids in input order
  (`scatter_rows` keeps it)". Lines 33–35: "a tuple whose probe would cross
  its region's end is deferred to the calling thread, which pushes the
  deferred tuples in input order" → "a row whose probe would cross its
  region's end is deferred to the calling thread, which pushes the deferred
  rows in input order". Imports (lines 42–58):
  ```rust
  use {
      super::{
          bulk::TupleList,
          config::{HashTrieConfig, LoadFactor},
          expansion::ExpansionPolicy,
          hash_table::{home_bucket, BucketRun, HashTable, Overflow, RunEntry},
          implementation::HashTrie,
          node::HashTrieNode,
          pruning::PruningPolicy,
          radix::{self, Arrival},
      },
      crate::morsel::{
          dispatch, scatter_rows, RowPartition, Threads, MORSEL_TUPLES, PARTITIONS_PER_THREAD,
      },
      kermit_iters::{HashStrategy, RowId, Tuples},
      std::{cmp::Reverse, collections::BinaryHeap},
  };
  ```
  `Entries` (lines 90–134): doc "and `Leaf` for arity 1, so its values are
  chains" → "chains of row ids"; `Chains(Vec<Arrival<Vec<Vec<usize>>>>)` →
  `Chains(Vec<Arrival<TupleList>>)`; `take_from`'s parameter
  `first_seen: &[(usize, u64)]` → `first_seen: &[(RowId, u64)]`;
  `fn into_chains(self) -> Vec<Arrival<Vec<Vec<usize>>>>` →
  `fn into_chains(self) -> Vec<Arrival<TupleList>>`.
  Replace `fill_root` and `fill_root_in_morsels` (lines 145–207):
  ```rust
  /// Fills the empty `root` with the rows of `tuples` by the
  /// `parallel:threads` build. Every row must have `arity` attributes; the
  /// caller checks.
  pub(super) fn fill_root<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>(
      tuples: &Tuples, root: &mut HashTrieNode<P, E>, arity: usize, threads: Threads,
      config: HashTrieConfig,
  ) {
      fill_root_in_morsels::<H, P, E>(tuples, root, arity, threads, MORSEL_TUPLES, config);
  }

  /// [`fill_root`] with the morsel size as a parameter, so tests can cut a
  /// small input into many morsels.
  fn fill_root_in_morsels<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>(
      tuples: &Tuples, root: &mut HashTrieNode<P, E>, arity: usize, threads: Threads,
      morsel_tuples: usize, config: HashTrieConfig,
  ) {
      if tuples.is_empty() {
          // The bulk build of nothing is the empty root: start no worker.
          return;
      }
      // 1. Partition, on `threads` workers, by the top bits of each row's
      //    first attribute's hash; each partition lists its ids in input order.
      let bits = partition_bits(threads);
      let shift = 64 - bits;
      let partitions = scatter_rows(threads, tuples, morsel_tuples, 1 << bits, |row| {
          (H::hash(row[0]) >> shift) as usize
      });
      #[cfg(any(test, feature = "test-hooks"))]
      PARALLEL_BUILDS.with(|builds| {
          let sizes = partitions.iter().map(RowPartition::len).collect();
          builds.borrow_mut().push(ParallelBuild {
              threads: threads.get(),
              partition_sizes: sizes,
              deferred: None,
          });
      });
      // An empty partition would build and drop an empty scratch root; the
      // radix build skips them too.
      let partitions: Vec<RowPartition> = partitions
          .into_iter()
          .filter(|partition| partition.len() > 0)
          .collect();
      // 2. Build, on `threads` workers: each partition into a scratch root, whose
      //    entries then leave in the order their keys arrived.
      let entries = dispatch(threads, partitions, |partition| {
          let (scratch, first_seen) =
              radix::build_scratch_root::<H, P, E>(tuples, partition.ids(), arity, config);
          Entries::take_from(scratch, &first_seen)
      });
      // 3. Merge, on this thread, in first-appearance order.
      match root {
          | HashTrieNode::Inner(table) => {
              let lists = entries.into_iter().map(Entries::into_children).collect();
              merge_in_first_appearance_order(table, lists, config.load_factor);
          },
          | HashTrieNode::Leaf(table) => {
              let lists = entries.into_iter().map(Entries::into_chains).collect();
              merge_in_first_appearance_order(table, lists, config.load_factor);
          },
          | HashTrieNode::Singleton(_) | HashTrieNode::Unexpanded(_) => {
              unreachable!("a root is never pruned or unexpanded")
          },
      }
  }
  ```
  In `merge_in_first_appearance_order` (line 225):
  `let mut heads: BinaryHeap<Reverse<(usize, usize)>> = lists` →
  `let mut heads: BinaryHeap<Reverse<(RowId, usize)>> = lists` (a head is a
  first id and a list index). Replace lines 247–379 (`fill_presized_root`
  through `fill_runs`; `REGION_BUCKETS` at 239–245 is unchanged):
  ```rust
  /// Builds the root of `2^log2_capacity` buckets over the rows of `tuples` by
  /// `presized:threads` and returns it: the partitioned build of a presized
  /// root (the paper's partitioning and Algorithm 2; kermit's regions, tail
  /// and per-run children;
  /// `docs/specs/2026-10-06-hash-trie-presized-parallel-build-design.md`),
  /// with every child built by Algorithm 2 (#107). Every row must have
  /// `arity` attributes; the caller checks.
  pub(super) fn fill_presized_root<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>(
      tuples: &Tuples, log2_capacity: u32, arity: usize, threads: Threads, config: HashTrieConfig,
  ) -> HashTrieNode<P, E> {
      fill_presized_root_in::<H, P, E>(
          tuples,
          log2_capacity,
          arity,
          threads,
          MORSEL_TUPLES,
          REGION_BUCKETS,
          config,
      )
  }

  /// [`fill_presized_root`] with the morsel and region sizes as parameters,
  /// so tests can cut a small input into many morsels and regions.
  ///
  /// 1. **Partition**: `scatter_rows` by the top bits of each row's home
  ///    bucket, so partition k is a contiguous run of regions, in input order.
  /// 2. **Group** (Algorithm 2, lines 4–7): each worker takes a partition with
  ///    its run of a presized table of row-id lists, and pushes every row's id
  ///    onto the list in its bucket. Rows whose key's probe runs off its
  ///    region are deferred.
  /// 3. **Tail**: the calling thread pushes the deferred rows in input order
  ///    (ascending id), with ordinary probing. The paper does not say how a
  ///    probe crossing a partition's end is handled; this is kermit's answer.
  /// 4. **Children** (lines 8–12): each worker builds the children of its run's
  ///    buckets, by the bulk build's `child`. The paper does not say how the
  ///    recursion is spread over threads; one run per worker is kermit's choice.
  fn fill_presized_root_in<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>(
      tuples: &Tuples, log2_capacity: u32, arity: usize, threads: Threads, morsel_tuples: usize,
      region_buckets: usize, config: HashTrieConfig,
  ) -> HashTrieNode<P, E> {
      if tuples.is_empty() {
          return HashTrie::<H, P, E>::make_root_sized(arity, log2_capacity);
      }
      let capacity = 1usize << log2_capacity;
      let region_buckets = region_buckets.min(capacity);
      let parts = (PARTITIONS_PER_THREAD * threads.get())
          .next_power_of_two()
          .min(capacity / region_buckets);
      let shift = log2_capacity - parts.trailing_zeros();
      // 1. Partition by the home bucket's top bits: whole runs of regions.
      let partitions = scatter_rows(threads, tuples, morsel_tuples, parts, |row| {
          home_bucket(H::hash(row[0]), log2_capacity) >> shift
      });
      #[cfg(any(test, feature = "test-hooks"))]
      let partition_sizes: Vec<usize> = partitions.iter().map(RowPartition::len).collect();
      // 2. Group each run of regions in parallel.
      let mut lists: HashTable<TupleList> = HashTable::with_log2_capacity(log2_capacity);
      let mut deferred = fill_runs(&mut lists, threads, partitions, region_buckets, |run, row| {
          push_in_run::<H>(tuples, run, row)
      });
      #[cfg(any(test, feature = "test-hooks"))]
      PARALLEL_BUILDS.with(|builds| {
          builds.borrow_mut().push(ParallelBuild {
              threads: threads.get(),
              partition_sizes,
              deferred: Some(deferred.len()),
          });
      });
      // 3. The tail, in input order: a row's id is its input position. The
      //    table is presized for every row, so it does not grow here.
      deferred.sort_unstable();
      for row in deferred {
          lists
              .entry_or_insert_with(H::hash(tuples.row(row)[0]), config.load_factor, Vec::new)
              .push(row);
      }
      // 4. Every bucket's child, a run per worker. At arity 1 the lists are the
      //    chains, and the table of lists is the root. `arity <= 1`, as
      //    `build_nested` decides at depth 0 (`depth + 1 >= arity`).
      if arity <= 1 {
          return HashTrieNode::Leaf(lists);
      }
      HashTrieNode::Inner(lists.map_in_runs(parts, |runs| {
          dispatch(threads, runs, |run| {
              run.map(|list| HashTrie::<H, P, E>::child(tuples, 1, arity, list, config));
          });
      }))
  }

  /// Algorithm 2's lines 6–7 inside one run of a presized table of lists:
  /// `presized:N`'s root step, the same for every arity. Returns the row's id
  /// if its key's probe ran off its region, for the calling thread to push
  /// afterwards.
  fn push_in_run<H: HashStrategy>(
      tuples: &Tuples, run: &mut BucketRun<'_, TupleList>, row: RowId,
  ) -> Result<(), RowId> {
      match run.entry(H::hash(tuples.row(row)[0])) {
          | Ok(RunEntry::Occupied(list)) => list.push(row),
          | Ok(RunEntry::Vacant(bucket)) => bucket.insert(Vec::new()).push(row),
          | Err(Overflow) => return Err(row),
      }
      Ok(())
  }

  /// Lends `table`'s runs to the workers: partition k fills run k through
  /// `step`, in input order, and the ids `step` hands back are returned. An
  /// id is its row's input position, so the caller can restore input order.
  fn fill_runs<V: Send>(
      table: &mut HashTable<V>, threads: Threads, partitions: Vec<RowPartition>,
      region_buckets: usize, step: impl Fn(&mut BucketRun<'_, V>, RowId) -> Result<(), RowId> + Sync,
  ) -> Vec<RowId> {
      let parts = partitions.len();
      table.with_runs(parts, region_buckets, |runs| {
          let work: Vec<_> = partitions.into_iter().zip(runs).collect();
          dispatch(threads, work, |(partition, mut run)| {
              let mut deferred = Vec::new();
              for row in partition.ids() {
                  if let Err(row) = step(&mut run, row) {
                      deferred.push(row);
                  }
              }
              deferred
          })
          .into_iter()
          .flatten()
          .collect()
      })
  }
  ```
  **Tests** (`mod tests`; `Tuples` and `RowId` reach it through `super::*`).
  Imports (lines 383–401): add `bulk::all_rows,` to the
  `ds::hash_trie::{…}` list. `check_identity` lines 455–462:
  ```rust
                          fill_root_in_morsels::<H, P, E>(
                              &tuples,
                              &mut root,
                              arity,
                              threads(t),
                              7,
                              config,
                          );
  ```
  `check_one_run`, lines 676–690:
  ```rust
                          let log2 = config.root_log2_capacity(tuples.len());
                          let mut lists: HashTable<TupleList> = HashTable::with_log2_capacity(log2);
                          let tail: Vec<RowId> = lists.with_runs(1, 1 << log2, |mut runs| {
                              all_rows(&tuples)
                                  .filter_map(|row| push_in_run::<H>(&tuples, &mut runs[0], row).err())
                                  .collect()
                          });
                          for row in tail {
                              lists
                                  .entry_or_insert_with(
                                      H::hash(tuples.row(row)[0]),
                                      config.load_factor,
                                      Vec::new,
                                  )
                                  .push(row);
                          }
                          let root =
                              HashTrie::<H, P, E>::build_nested(&tuples, 0, arity, lists, config);
  ```
  `check_presized` (line 739) signature:
  `inputs: &dyn Fn(usize) -> Vec<(&'static str, Tuples)>,`; lines 771–779:
  ```rust
                              let small = fill_presized_root_in::<H, P, E>(
                                  &tuples,
                                  log2,
                                  arity,
                                  threads(t),
                                  7,
                                  8,
                                  config,
                              );
  ```
  `presized_parallel_builds_are_equivalent_on_large_and_skewed_inputs`,
  line 914: `vec![("random", Tuples::from(random)), ("half one key", Tuples::from(skewed))]`.
  `keys_that_cannot_fit_their_region_go_to_the_tail`, line 941:
  `let tuples = Tuples::from((0..16).map(|i| vec![i % 4, i]).collect::<Vec<Vec<usize>>>());`
  and lines 950–958:
  ```rust
          let root = fill_presized_root_in::<EndOfRegionHash, NoPruning, EagerExpansion>(
              &tuples,
              5,
              2,
              threads(2),
              7,
              8,
              config,
          );
  ```
  `presized_parallel_builds_are_the_same_for_every_n_on_dense_roots`,
  line 1059: `vec![("240 distinct keys", Tuples::from(distinct)), ("120 keys twice", Tuples::from(pairs))]`.
  `parallel_build_rejects_a_wrong_arity` (line 644) stays as T3 left it: its
  uniform arity-3 batch reaches `batch_for_header` in
  `from_tuples_partitioned` before any partitioning, and the message still
  contains the expected "does not match header arity". All other tests are
  unchanged.

- [ ] **Step 10: `identity.rs`, ids and buffers.** Replace the file by:
  ```rust
  //! Array-level identity checks for the HashTrie builds (#91, #94, #88).
  //! Every build mode must build the trie the bulk build does, the
  //! root-capacity Config must leave every subtrie as it was, and the
  //! `presized:N` build must be equivalent (Amendment 2), so the
  //! radix, parallel and root-capacity tests share these assertions and
  //! inputs.
  //!
  //! A trie keeps the batch it is built from as its buffer, unchanged, and its
  //! chains, singletons and pending lists hold row ids into it (#111). Two
  //! builds of one input therefore own equal buffers, so equal id lists name
  //! equal tuples, and comparing ids is as strict as comparing tuples was.
  //! [`assert_same_trie`] and [`assert_equivalent_trie`] compare the buffers
  //! first, so that premise is checked, not assumed.

  use {
      super::{
          bulk::TupleList,
          config::{ChildCapacity, HashTrieConfig, LoadFactor, RootCapacity},
          expansion::{ExpansionPolicy, PendingChild},
          hash_table::{home_bucket, HashTable},
          implementation::HashTrie,
          node::HashTrieNode,
          pruning::{PruningPolicy, SingletonPayload},
      },
      crate::{cardinality::Cardinality, heap_size::HeapSize, test_support::Lcg},
      kermit_iters::{HashStrategy, Tuples},
  };

  /// Asserts that two tables hold the same buckets: the same capacity
  /// (array length and allocation), the same length, the same hash in
  /// every bucket, and values that `same_value` accepts.
  fn assert_same_table<V>(
      a: &HashTable<V>, b: &HashTable<V>, path: &str, same_value: &dyn Fn(&V, &V, &str),
  ) {
      assert_eq!(a.buckets_len(), b.buckets_len(), "{path}: capacity");
      assert_eq!(
          a.shell_heap_bytes(),
          b.shell_heap_bytes(),
          "{path}: bucket allocation"
      );
      assert_eq!(a.len(), b.len(), "{path}: len");
      for idx in 0..a.buckets_len() {
          assert_eq!(a.hash_at(idx), b.hash_at(idx), "{path}: bucket {idx}");
          if let (Some(x), Some(y)) = (a.value_at(idx), b.value_at(idx)) {
              same_value(x, y, &format!("{path}/{idx}"));
          }
      }
  }

  /// Two chains or pending lists: the same row ids, in the same order, at
  /// the same capacity.
  // `&Vec`, not a slice: the comparison reads `capacity()`.
  #[allow(clippy::ptr_arg)]
  fn assert_same_chain(a: &TupleList, b: &TupleList, path: &str) {
      assert_eq!(a, b, "{path}: chain");
      assert_eq!(a.capacity(), b.capacity(), "{path}: chain capacity");
  }

  pub(super) fn assert_same_node<P: PruningPolicy, E: ExpansionPolicy>(
      a: &HashTrieNode<P, E>, b: &HashTrieNode<P, E>, path: &str,
  ) {
      match (a, b) {
          | (HashTrieNode::Inner(x), HashTrieNode::Inner(y)) => {
              assert_same_table(x, y, path, &|x, y, path| assert_same_node(x, y, path))
          },
          | (HashTrieNode::Leaf(x), HashTrieNode::Leaf(y)) => {
              assert_same_table(x, y, path, &|x, y, path| assert_same_chain(x, y, path))
          },
          | (HashTrieNode::Singleton(x), HashTrieNode::Singleton(y)) => {
              assert_eq!(x.row(), y.row(), "{path}: singleton");
          },
          | (HashTrieNode::Unexpanded(x), HashTrieNode::Unexpanded(y)) => {
              // Building expands nothing, so both children are still the
              // pending lists `insert_at` appended to.
              assert!(
                  x.built().is_none() && y.built().is_none(),
                  "{path}: expanded during the build"
              );
              assert_same_chain(&x.pending(), &y.pending(), &format!("{path}: pending"));
          },
          | _ => panic!("{path}: node variants differ"),
      }
  }

  /// The premise of every id comparison: the two tries own equal buffers,
  /// row for row and at the same capacity.
  fn assert_same_buffer<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>(
      a: &HashTrie<H, P, E>, b: &HashTrie<H, P, E>, label: &str,
  ) {
      assert_eq!(a.tuples(), b.tuples(), "{label}: buffer");
      assert_eq!(
          a.tuples().heap_size_bytes(),
          b.tuples().heap_size_bytes(),
          "{label}: buffer capacity"
      );
  }

  /// The array-level identity the standard requires of a BuildMode: the
  /// buffer, every table's buckets and capacity, every chain and its
  /// capacity, every singleton and pending list, the heap size and the tuple
  /// count.
  pub(super) fn assert_same_trie<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>(
      a: &HashTrie<H, P, E>, b: &HashTrie<H, P, E>, label: &str,
  ) {
      assert_same_buffer(a, b, label);
      assert_same_node(a.root(), b.root(), label);
      assert_eq!(
          a.heap_size_bytes(),
          b.heap_size_bytes(),
          "{label}: heap size"
      );
      assert_eq!(a.tuple_count(), b.tuple_count(), "{label}: tuple count");
  }

  /// Rows of up to three columns, cut to `arity`, as a batch whose capacity
  /// is exactly its rows, so a batch and its clone have equal capacities.
  pub(super) fn rows(arity: usize, rows: &[[usize; 3]]) -> Tuples {
      let mut tuples = Tuples::with_capacity(arity, rows.len());
      for row in rows {
          tuples.push(&row[..arity]);
      }
      tuples
  }

  /// Enough distinct first values to double the root several times (three
  /// times under Miri), with repeats so subtries and chains hold several
  /// tuples. The builds are safe Rust, so Miri runs a small matrix: the full
  /// one took almost five minutes there.
  const RANDOM_TUPLES: usize = if cfg!(miri) {
      48
  } else {
      3_000
  };

  /// Load factors under test, in percent. Miri runs the default only.
  pub(super) const LOAD_PERCENTS: &[u8] = if cfg!(miri) {
      &[70]
  } else {
      &[70, 50]
  };

  /// The configs a build-mode identity test runs under: each load factor in
  /// [`LOAD_PERCENTS`] with each root capacity and each child capacity.
  /// Every build mode must build the same trie under every config (#88,
  /// #107).
  pub(super) fn configs() -> Vec<HashTrieConfig> {
      let mut configs = Vec::new();
      for &percent in LOAD_PERCENTS {
          for root_capacity in [RootCapacity::Grow, RootCapacity::Tuples] {
              for child_capacity in [ChildCapacity::Grow, ChildCapacity::Tuples] {
                  configs.push(HashTrieConfig {
                      load_factor: LoadFactor::percent(percent).unwrap(),
                      root_capacity,
                      child_capacity,
                  });
              }
          }
      }
      configs
  }

  /// The [`configs`] the `incremental` build accepts: children that grow.
  pub(super) fn incremental_configs() -> Vec<HashTrieConfig> {
      configs()
          .into_iter()
          .filter(|config| config.child_capacity == ChildCapacity::Grow)
          .collect()
  }

  /// The shared inputs, each a batch of `arity` whose capacity is exactly its
  /// rows (see [`rows`]). The random rows are the draws made before #111, in
  /// the same order.
  pub(super) fn inputs(arity: usize) -> Vec<(&'static str, Tuples)> {
      let mut lcg = Lcg(0x91);
      let mut random = Tuples::with_capacity(arity, RANDOM_TUPLES);
      for _ in 0..RANDOM_TUPLES {
          let row = [
              lcg.next_usize() % (RANDOM_TUPLES / 4),
              lcg.next_usize() % 50,
              lcg.next_usize() % 7,
          ];
          random.push(&row[..arity]);
      }
      vec![
          ("empty", Tuples::new(arity)),
          ("one tuple", rows(arity, &[[1, 2, 3]])),
          (
              "duplicates",
              rows(arity, &[[1, 2, 3], [1, 2, 3], [1, 2, 3]]),
          ),
          (
              "interleaved",
              rows(arity, &[
                  [1, 2, 3],
                  [2, 3, 4],
                  [1, 5, 6],
                  [3, 1, 1],
                  [2, 3, 9],
                  [1, 2, 7],
              ]),
          ),
          ("random", random),
      ]
  }

  /// The equivalence Amendment 2 asks of a build that may place root keys in
  /// other buckets. The root has the same capacity, allocation and length,
  /// occupies the same buckets, and has the same total probe displacement.
  /// Every key's child or chain is array-identical.
  pub(super) fn assert_equivalent_root<P: PruningPolicy, E: ExpansionPolicy>(
      a: &HashTrieNode<P, E>, b: &HashTrieNode<P, E>, label: &str,
  ) {
      match (a, b) {
          | (HashTrieNode::Inner(x), HashTrieNode::Inner(y)) => {
              assert_equivalent_table(x, y, label, &|x, y, path| assert_same_node(x, y, path))
          },
          | (HashTrieNode::Leaf(x), HashTrieNode::Leaf(y)) => {
              assert_equivalent_table(x, y, label, &|x, y, path| assert_same_chain(x, y, path))
          },
          | _ => panic!("{label}: root variants differ"),
      }
  }
  ```
  followed by `assert_equivalent_table` unchanged (current lines 200–240), and:
  ```rust
  /// [`assert_equivalent_root`] for whole tries, plus the buffer, heap size
  /// and tuple count.
  pub(super) fn assert_equivalent_trie<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>(
      a: &HashTrie<H, P, E>, b: &HashTrie<H, P, E>, label: &str,
  ) {
      assert_same_buffer(a, b, label);
      assert_equivalent_root(a.root(), b.root(), label);
      assert_eq!(
          a.heap_size_bytes(),
          b.heap_size_bytes(),
          "{label}: heap size"
      );
      assert_eq!(a.tuple_count(), b.tuple_count(), "{label}: tuple count");
  }
  ```
  (`rows` is only called at arities 1–3, and `Tuples::with_capacity(arity,
  n)` reserves exactly `arity · n`, so every shared input has exact capacity
  and a clone of it is equal in capacity: the buffer-capacity assertion does
  not depend on how `Tuples::from` sizes a batch.)

- [ ] **Step 11: `morsel.rs`, delete the old scatter.** After Step 9 nothing
  calls `scatter`, `Partition` or `Positioned` (TreeTrie moved to
  `scatter_rows` in T5; check with the grep in Step 22). Module doc lines 1–18 become:
  ```rust
  //! Morsel-driven work distribution for the parallel builds (Leis et al.,
  //! *Morsel-Driven Parallelism*, SIGMOD 2014).
  //!
  //! The input is cut into small fixed-size *morsels*. The *dispatcher*, a
  //! mutex-guarded queue, hands the next unit of work to whichever worker is
  //! free, so a slow worker never stalls the rest. A build uses it twice:
  //!
  //! 1. [`scatter_rows`] sends row ids into partitions, one morsel at a time;
  //! 2. [`dispatch`] then runs one task per partition.
  //!
  //! Neither result depends on scheduling. A partition lists its row ids in
  //! input order, and task results come back in task order, so a parallel
  //! build can reproduce its serial counterpart exactly
  //! (`docs/data-structures/parallel-build.md`).
  //!
  //! The workers are [`std::thread::scope`] threads, the calling thread among
  //! them. Every worker is joined before a call returns, and a worker's panic
  //! is re-raised in the caller.
  ```
  Delete current lines 68–90 (`Positioned`, `Buckets`, `Partition` and its
  impl) and lines 116–161 (`scatter`, with its doc); the line numbers are at
  `47950f0`, and T5's edits above them shifted them by a few lines. T5's
  `RowPartition`, `type RowBuckets`, `pub(crate) fn row_id` and `scatter_rows`
  stay. In `scatter_rows`' doc comment, the sentence "Every partition lists
  its ids in input order, whatever order the morsels ran in, as [`scatter`]
  lists its tuples." becomes "Every partition lists its ids in input order,
  whatever order the morsels ran in." (the link's target is gone). In `mod tests`,
  delete `scatter_keeps_every_tuple_once_in_input_order` (lines 206–250) and
  `scatter_moves_tuples_without_reallocating` (lines 252–271): the tuples no
  longer move, and T5's `scatter_rows` tests cover the partition. Replace
  `empty_input_needs_no_work` (lines 315–323) by:
  ```rust
      #[test]
      fn empty_input_needs_no_work() {
          assert!(dispatch(threads(4), Vec::<usize>::new(), |i| i).is_empty());
          let partitions = scatter_rows(threads(4), &Tuples::new(2), 8, 3, |_| 0);
          assert_eq!(partitions.len(), 3);
          assert!(partitions.iter().all(|partition| partition.len() == 0));
      }
  ```
  (`Tuples` reaches the tests through `super::*`: T5 imports it for
  `scatter_rows`.)

- [ ] **Step 12: Tests outside the module that read `collect_tuples`.**
  `collect_tuples` returns `Tuples`; each caller reads rows with `to_vecs()`.
  - `kermit-ds/src/configured.rs` line 208:
    `assert_eq!(b.collect_tuples().to_vecs(), vec![vec![1, 2]]);`
    Lines 233–235 (`inserts_reach_the_inner_relation`) and 268–270
    (`built_with_stacks_on_configured`), each:
    ```rust
            assert_eq!(r.collect_tuples().to_vecs(), vec![vec![1, 2], vec![3, 4]]);
    ```
    (both inputs arrive in that order).
  - `kermit-ds/tests/common/macros.rs`, doc lines 714–718: "`HashTrie` passes
    a closure over `collect_tuples()` that sorts the (hash-ordered) result" →
    "`HashTrie` passes a closure over `collect_tuples()` that sorts the
    result (its buffer, in arrival order)". `hash_trie_construction_tests!`
    → `assert_round_trip` (lines 1094–1098):
    ```rust
                    // `HashTrie` has no tuple-shaped iterator; `collect_tuples`
                    // copies its buffer, in arrival order. Compare as sorted
                    // multisets, as for any multiset structure.
                    let mut collected = relation.collect_tuples().to_vecs();
                    collected.sort();
    ```
    `insert_matches_from_tuples` (lines 1130–1131):
    ```rust
                    let mut a = batch.collect_tuples().to_vecs();
                    let mut b = incremental.collect_tuples().to_vecs();
    ```
  - `kermit-ds/tests/parquet_tests.rs` lines 57–70:
    ```rust
    // `HashTrie` has no tuple-shaped iterator (it is `HashTrieIterable`, not
    // `TrieIterable`), so the round-trip is checked through `collect_tuples()`,
    // a copy of its buffer in arrival (file) order, sorted before comparison.
    // One invocation per Layout alias, matching `kermit/tests/join_tests.rs`.
    type HashTrieSip = HashTrie<SipHashStrategy>;
    type HashTrieFx = HashTrie<FxHashStrategy>;

    fn sorted_tuples<H: kermit_iters::HashStrategy, P: PruningPolicy, E: ExpansionPolicy>(
        relation: &HashTrie<H, P, E>,
    ) -> Vec<Vec<usize>> {
        let mut tuples = relation.collect_tuples().to_vecs();
        tuples.sort();
        tuples
    }
    ```
  - `kermit-ds/src/relation.rs`, T3's test helper `hash_rows` (in `mod tests`,
    beside `every_constructor_takes_a_batch`). Before:
    ```rust
        /// A hash trie's tuples, sorted (a hash trie lends them in hash order).
        fn hash_rows(relation: &crate::ds::HashTrie) -> Vec<Vec<usize>> {
            let mut rows = relation.collect_tuples();
            rows.sort();
            rows
        }
    ```
    After (without the change, `rows.sort()` would resolve to `Tuples::sort` and
    the function would return a `Tuples`):
    ```rust
        /// A hash trie's tuples, sorted (a hash trie keeps them in arrival order).
        fn hash_rows(relation: &crate::ds::HashTrie) -> Vec<Vec<usize>> {
            let mut rows = relation.collect_tuples().to_vecs();
            rows.sort();
            rows
        }
    ```
  - `kermit-ds/tests/hash_trie_tests.rs` `collect_tuples_recovers_colliding_tuples`
    (lines 314–323):
    ```rust
        #[test]
        fn collect_tuples_recovers_colliding_tuples() {
            let tuples = vec![vec![1, 2], vec![11, 12], vec![21, 2], vec![1, 12]];
            let trie = HashTrieMod10::from_tuples(2.into(), tuples.clone());
            assert_eq!(trie.collect_tuples().to_vecs(), tuples);
        }
    ```

- [ ] **Step 13: Change `HashTrieIterator::leaf_tuples` to return `LeafRows`**

In `kermit-iters/src/hash_trie.rs`, line 8:

```rust
use crate::joinable::JoinIterable;
```
becomes
```rust
use crate::{joinable::JoinIterable, LeafRows};
```

(`crate::LeafRows` is T2's crate-root re-export, so this line works whichever
file T2 put the type in.)

Lines 31–32, in the "Method semantics" list:

```rust
/// - [`leaf_tuples`](Self::leaf_tuples) — at the leaf level, the tuple chain at
///   the current bucket; `None` at inner levels.
```
become
```rust
/// - [`leaf_tuples`](Self::leaf_tuples) — at the leaf level, a [`LeafRows`]
///   view of the tuple chain at the current bucket; `None` at inner levels.
```

Lines 67–70:

```rust
    /// Tuple chain at the current leaf bucket. Returns `Some(&[…])` iff the
    /// current node is a leaf and the current bucket is occupied;
    /// otherwise `None`.
    fn leaf_tuples(&self) -> Option<&[Vec<usize>]>;
```
become
```rust
    /// Tuple chain at the current leaf bucket, as a [`LeafRows`] view: the
    /// chain's row ids, in chain order, over the tuple buffer they index.
    /// Returns `Some` iff the current node is a leaf and the current bucket
    /// is occupied; otherwise `None`. The view borrows the iterator, so
    /// reading a chain copies and allocates nothing.
    fn leaf_tuples(&self) -> Option<LeafRows<'_>>;
```

The rest of the file is unchanged, including the "Not a `LinearIterator`"
rationale and `HashTrieIterable`. The trait stays dyn-compatible: the suites'
`&mut dyn HashTrieIterator` closures still compile, because `LeafRows<'_>` is
a concrete return type.

- [ ] **Step 14: `SingletonFrame` borrows a `&'a RowId`**

In `kermit-ds/src/ds/hash_trie/pruning.rs`, line 13 already reads
`use kermit_iters::{LayoutOption, RowId};` (Step 3). Lines 26–47, the trait:

```rust
/// The iterator's stand-in for the one-entry table a pruned level would
/// have held. One implementor per policy; the `NoPruning` one is [`Never`],
/// so the iterator's singleton arms vanish in that instantiation.
pub trait SingletonFrame<'a>: Sized {
    /// A frame for the pruned subtrie's row `row`, positioned on its single
    /// entry. `hash` is the hash of the row's value at the frame's depth,
    /// computed once by the caller (`HashTrieIter::singleton_frame`, which
    /// reads the row and checks the depth).
    // A borrow of the payload's id, as the frame borrowed its tuple before:
    // `row()` hands it back with the trie lifetime, and `leaf_tuples` lends
    // it as a one-id chain via `slice::from_ref`.
    fn new(row: &'a RowId, hash: u64) -> Self;
    /// The row id of the tuple stored below the pruned node.
    fn row(&self) -> &'a RowId;
    /// `Some(hash)` unless exhausted.
    fn key(&self) -> Option<u64>;
    /// Advance past the single entry.
    fn exhaust(&mut self);
    /// Position on the entry iff `hash` matches; otherwise exhaust.
    fn lookup(&mut self, hash: u64) -> bool;
    /// Whether the single entry has been passed.
    fn at_end(&self) -> bool;
}
```

`new` loses its `depth` parameter. Its only use was the `debug_assert` in
`SingletonFrameOn::new`, which needs the row's length, and a frame holding
only an id cannot see that. The assert moves into
`HashTrieIter::singleton_frame` (Step 15), which reads the row anyway to hash it.

Lines 93–107, the `Never` impl:

```rust
impl<'a> SingletonFrame<'a> for Never {
    fn new(_row: &'a RowId, _hash: u64) -> Self {
        unreachable!("NoPruning never pushes a singleton frame")
    }

    fn row(&self) -> &'a RowId { match *self {} }

    fn key(&self) -> Option<u64> { match *self {} }

    fn exhaust(&mut self) { match *self {} }

    fn lookup(&mut self, _hash: u64) -> bool { match *self {} }

    fn at_end(&self) -> bool { match *self {} }
}
```

Lines 117–157, `SingletonFrameOn`:

```rust
/// One emulated level of a pruned subtrie holding the row `row`;
/// `exhausted` plays the role of a table frame's past-end bucket index.
///
/// The level itself is not stored: a frame's depth is its position in the
/// iterator's stack, which `HashTrieIter` reads off `stack.len()`.
#[derive(Debug)]
pub struct SingletonFrameOn<'a> {
    // Borrowed from the payload: `leaf_tuples` lends it as a one-id chain
    // via `slice::from_ref`.
    row: &'a RowId,
    hash: u64,
    exhausted: bool,
}

impl<'a> SingletonFrame<'a> for SingletonFrameOn<'a> {
    fn new(row: &'a RowId, hash: u64) -> Self {
        Self {
            row,
            hash,
            exhausted: false,
        }
    }

    fn row(&self) -> &'a RowId { self.row }

    fn key(&self) -> Option<u64> { (!self.exhausted).then_some(self.hash) }

    fn exhaust(&mut self) { self.exhausted = true; }

    fn lookup(&mut self, hash: u64) -> bool {
        self.exhausted = hash != self.hash;
        !self.exhausted
    }

    fn at_end(&self) -> bool { self.exhausted }
}
```

Test `frame_emulates_a_one_entry_table` (lines 224–238) becomes:

```rust
    #[test]
    fn frame_emulates_a_one_entry_table() {
        let row: RowId = 7;
        let mut f = <SingletonFrameOn<'_> as SingletonFrame<'_>>::new(&row, 0xBEEF);
        assert_eq!(f.key(), Some(0xBEEF));
        assert!(!f.at_end());
        assert!(!f.lookup(0xDEAD));
        assert!(f.at_end());
        assert_eq!(f.key(), None);
        assert!(f.lookup(0xBEEF));
        assert!(!f.at_end());
        f.exhaust();
        assert!(f.at_end());
        assert_eq!(f.row(), &row);
    }
```

`frame_is_a_borrow_a_hash_and_a_flag` (240–245) is unchanged and still
expects 24 bytes: a pointer, a `u64` and a `bool`.

- [ ] **Step 15: `HashTrieIter` lends `LeafRows` over the trie's buffer**

In `kermit-ds/src/ds/hash_trie/hash_trie_iter.rs`, lines 27–36 become:

```rust
use {
    super::{
        expansion::{EagerExpansion, ExpansionPolicy},
        implementation::HashTrie,
        node::HashTrieNode,
        pruning::{NoPruning, PruningPolicy, SingletonFrame, SingletonPayload},
    },
    crate::relation::Relation,
    kermit_iters::{HashStrategy, HashTrieIterator, LeafRows, RowId, SipHashStrategy},
};
```

Lines 63–72, `Descent`:

```rust
/// What `open` found below the current position.
enum Descent<'a, P: PruningPolicy, E: ExpansionPolicy> {
    /// A table node to descend into.
    Node(&'a HashTrieNode<P, E>),
    /// Stay inside a pruned subtrie: emulate the next level down for the
    /// row with id `*row`. The depth is the one `open` already computed.
    Deeper(&'a RowId),
    /// Nothing below (leaf level, empty bucket, or exhausted).
    Blocked,
}
```

Lines 103–158 (`frame_for`, `singleton_frame` and `descent`).
`singleton_frame` becomes a `&self` method, since it reads the row from the
trie's buffer to hash it:

```rust
    /// The frame that opening `child` at `depth` produces, positioned on
    /// its first entry (or past-end if it has none). An unexpanded child
    /// is built here, the one expansion site (`HashTrie::resolve`); the
    /// built node is always a table, so a frame never holds `Unexpanded`.
    fn frame_for(&self, child: &'a HashTrieNode<P, E>, depth: usize) -> Frame<'a, P, E> {
        let trie: &'a HashTrie<H, P, E> = self.trie;
        match trie.resolve(child, depth) {
            | HashTrieNode::Singleton(payload) => self.singleton_frame(payload.row(), depth),
            | table => Frame::Table {
                node: table,
                idx: table.next_occupied(0),
            },
        }
    }

    /// The frame for a pruned subtrie's row `*row` at `depth`: the
    /// one-entry table that level would have held, keyed by the hash of the
    /// row's `depth`-th value. `row` is borrowed from the payload with the
    /// trie lifetime `'a`, so `leaf_tuples` can lend it as a one-id chain.
    fn singleton_frame(&self, row: &'a RowId, depth: usize) -> Frame<'a, P, E> {
        let tuple = self.trie.tuples().row(*row);
        debug_assert!(
            depth < tuple.len(),
            "singleton frame at depth {depth} below a {}-attribute tuple",
            tuple.len()
        );
        Frame::Singleton(P::Frame::new(row, H::hash(tuple[depth])))
    }

    /// Where `open` would go from the current position.
    ///
    /// Matches on `*node` and copies the singleton's row-id reference out
    /// of the frame so the returned references carry the trie lifetime
    /// `'a`, not the shorter borrow of `self.stack`.
    fn descent(&self) -> Descent<'a, P, E> {
        match self.stack.last() {
            | None => Descent::Node(self.trie.root()),
            | Some(Frame::Table {
                node,
                idx,
            }) => match *node {
                | HashTrieNode::Inner(t) => match t.value_at(*idx) {
                    | Some(child) => Descent::Node(child),
                    | None => Descent::Blocked, // current bucket empty / past-end
                },
                | HashTrieNode::Leaf(_) => Descent::Blocked,
                | HashTrieNode::Singleton(_) | HashTrieNode::Unexpanded(_) => unreachable!(
                    "Table frame holds a Singleton or Unexpanded node; frame_for routes pruned \
                     subtries to Frame::Singleton and open resolves unexpanded children first"
                ),
            },
            | Some(Frame::Singleton(s)) => {
                // The top frame stands at depth `stack.len() - 1`, so the
                // level below it exists only while `stack.len() < arity`.
                if s.at_end() || self.stack.len() >= self.arity() {
                    Descent::Blocked
                } else {
                    Descent::Deeper(s.row())
                }
            },
        }
    }
```

The old `#[allow(clippy::ptr_arg)]` and its `&'a Vec` comment (lines 118–121)
are gone, because nothing borrows a `&Vec` any more.

Lines 234–239, in `open`:

```rust
        let frame = match self.descent() {
            | Descent::Node(child) => self.frame_for(child, depth),
            | Descent::Deeper(tuple) => Self::singleton_frame(tuple, depth),
            | Descent::Blocked => return false,
        };
```
becomes
```rust
        let frame = match self.descent() {
            | Descent::Node(child) => self.frame_for(child, depth),
            | Descent::Deeper(row) => self.singleton_frame(row, depth),
            | Descent::Blocked => return false,
        };
```

(`descent` returns references with lifetime `'a`, so no borrow of `self` is
live when `singleton_frame` takes `&self` and `push` takes `&mut self`.)

Lines 247–265, `leaf_tuples`:

```rust
    fn leaf_tuples(&self) -> Option<LeafRows<'_>> {
        let tuples = self.trie.tuples();
        match self.stack.last()? {
            | Frame::Table {
                node,
                idx,
            } => match *node {
                | HashTrieNode::Leaf(t) => {
                    t.value_at(*idx).map(|chain| LeafRows::new(tuples, chain))
                },
                | HashTrieNode::Inner(_) => None,
                | HashTrieNode::Singleton(_) | HashTrieNode::Unexpanded(_) => unreachable!(
                    "Table frame holds a Singleton or Unexpanded node; frame_for routes pruned \
                     subtries to Frame::Singleton and open resolves unexpanded children first"
                ),
            },
            // The top frame stands at depth `stack.len() - 1`, so it is the
            // leaf level exactly when `stack.len() == arity`. Its chain is
            // the one row the pruned subtrie holds.
            | Frame::Singleton(s) => (!s.at_end() && self.stack.len() == self.arity())
                .then(|| LeafRows::new(tuples, std::slice::from_ref(s.row()))),
        }
    }
```

Lifetimes: `self.trie` is a `&'a HashTrie`, so `tuples` is a `&'a Tuples`.
`t` is a `&'a HashTable<Vec<RowId>>` (it comes through `*node`), and
`s.row()` is a `&'a RowId`. `LeafRows` is covariant, so the `LeafRows<'a>`
shortens to the `'_` of `&self`. `chain` is a `&Vec<RowId>` and coerces to
`&[RowId]` at the call.

- [ ] **Step 16: `HashTrieIter`'s inline tests read chains through the view, plus two new tests**

Same file, tests module. Four assertions compared a chain with a
`&[Vec<usize>]`. `LeafRows` has no `PartialEq`, so they compare `to_vecs()`.

Line 473 (`singleton_frames_emulate_one_entry_tables_down_to_the_leaf`) and
line 575 (`lookup_hit_on_inner_singleton_then_open_reaches_the_leaf`):

```rust
        assert_eq!(it.leaf_tuples(), Some(&[vec![1, 2, 3]][..]));
```
becomes (both sites)
```rust
        assert_eq!(it.leaf_tuples().map(|rows| rows.to_vecs()), Some(vec![vec![1, 2, 3]]));
```

Line 514 (`open_from_table_into_singleton_child`):

```rust
            leaves.extend_from_slice(it.leaf_tuples().expect("singleton at leaf depth"));
```
becomes
```rust
            leaves.extend(it.leaf_tuples().expect("singleton at leaf depth").to_vecs());
```

Line 563 (`size_is_one_on_a_leaf_depth_singleton_frame`):

```rust
        assert_eq!(it.leaf_tuples(), Some(&[vec![1, 2]][..]));
```
becomes
```rust
        assert_eq!(it.leaf_tuples().map(|rows| rows.to_vecs()), Some(vec![vec![1, 2]]));
```

Unchanged, because they compile against `Option<LeafRows>` as written:
- lines 416, 435, 463, 469 and 489 (`.is_none()`);
- 641 (`.is_some()`);
- 425–427 (`.expect(..)` then `!chain.is_empty()`, where
  `LeafRows::is_empty` exists).

Append inside the tests module, after `a_failed_lookup_expands_nothing`
(before the module's closing brace at line 662):

```rust
    // ── Leaf views ─────────────────────────────────────────────────────

    #[test]
    fn a_leaf_chain_lends_row_ids_into_the_trie_buffer() {
        // The two copies of (1, 2) share one chain. Its ids are their
        // input positions, in input order, and its rows are read from the
        // trie's own buffer, never copied.
        let trie: HashTrie =
            HashTrie::from_tuples(2.into(), vec![vec![1, 2], vec![3, 4], vec![1, 2]]);
        let mut it = HashTrieIter::new(&trie);
        assert!(it.open());
        assert!(it.lookup(h(1)));
        assert!(it.open());
        assert!(it.lookup(h(2)));
        let rows = it.leaf_tuples().expect("the chain of (1, 2)");
        assert_eq!(rows.ids(), &[0, 2]);
        assert_eq!(rows.arity(), 2);
        assert_eq!(rows.to_vecs(), vec![vec![1, 2], vec![1, 2]]);
        assert_eq!(rows.data().as_ptr(), trie.tuples().as_flat().as_ptr());
    }

    #[test]
    fn singleton_frames_lend_their_row_id() {
        // (4, 5, 6) is alone below 4, so under pruning both levels below
        // the root are singleton frames. `frame_for` builds the first and
        // `open`'s `Descent::Deeper` the second. The leaf one lends its
        // payload's row id, 2, over the trie's buffer.
        let trie = Pruned::from_tuples(3.into(), vec![vec![1, 2, 3], vec![1, 2, 7], vec![4, 5, 6]]);
        let mut it = HashTrieIter::new(&trie);
        assert!(it.open());
        assert!(it.lookup(h(4)));
        assert!(it.open());
        assert!(it.open());
        assert!(matches!(it.stack.last(), Some(Frame::Singleton(_))));
        let rows = it.leaf_tuples().expect("singleton at leaf depth");
        assert_eq!(rows.ids(), &[2]);
        assert_eq!(rows.to_vecs(), vec![vec![4, 5, 6]]);
        assert_eq!(rows.data().as_ptr(), trie.tuples().as_flat().as_ptr());
    }
```

These are the spec's `LeafRows` tests for the leaf and pruned-singleton
implementors; Step 18 and Step 19 cover the constant and selection views. `h` and
`Pruned` are defined earlier in the module (lines 443–445). `it.stack` and
`Frame` are reachable because `tests` is a child module.

- [ ] **Step 17: The kermit-ds hash-family suites read chains through the view**

`kermit-ds/tests/common/macros.rs`, lines 1075–1080 (`hash_trie_test_helpers!`):

```rust
        /// A leaf chain as a sorted `Vec`, for comparison against literals.
        fn sorted(chain: &[Vec<usize>]) -> Vec<Vec<usize>> {
            let mut tuples = chain.to_vec();
            tuples.sort();
            tuples
        }
```
becomes
```rust
        /// A leaf chain's tuples as a sorted `Vec`, for comparison against
        /// literals.
        fn sorted(chain: kermit_iters::LeafRows<'_>) -> Vec<Vec<usize>> {
            let mut tuples = chain.to_vecs();
            tuples.sort();
            tuples
        }
```

Every `iter.leaf_tuples().map(sorted)` in the suite now type-checks
unchanged: lines 1210, 1287, 1292, 1321, 1426, 1431, 1446, 1464, 1497, 1502,
1530, 1599, 1604, 1610 and 1666. The `.is_none()` sites (1174, 1184, 1217,
1277, 1314, 1317 and 1546) are unchanged.

Line 1248 (the `siblings` test) indexes the chain:

```rust
                        assert_eq!(h(chain[0][0]), key);
```
becomes
```rust
                        assert_eq!(h(chain.row(0)[0]), key);
```

(Line 1247's `chain.len()` is unchanged. The `collect_tuples` sites in this
file, `assert_round_trip` at 1091–1102 and `insert_matches_from_tuples` at
1124–1135, are Step 12's.)

`kermit-ds/tests/hash_trie_tests.rs`. Line 236, in `mod hash_trie_collisions`:

```rust
        kermit_iters::{HashTrieIterable, HashTrieIterator},
```
becomes
```rust
        kermit_iters::{HashTrieIterable, HashTrieIterator, LeafRows},
```

Lines 241–245:

```rust
    fn sorted(chain: LeafRows<'_>) -> Vec<Vec<usize>> {
        let mut tuples = chain.to_vecs();
        tuples.sort();
        tuples
    }
```

(The `.map(sorted)` sites at 266, 268, 282, 296, 307 and 311 are unchanged.)

Line 337:

```rust
        let mut chain = it.leaf_tuples().expect("leaf chain").to_vec();
```
becomes
```rust
        let mut chain = it.leaf_tuples().expect("leaf chain").to_vecs();
```

Line 395, in `mod lazy_expansion`'s `step`:

```rust
            leaf: it.leaf_tuples().map(<[Vec<usize>]>::to_vec),
```
becomes
```rust
            leaf: it.leaf_tuples().map(|rows| rows.to_vecs()),
```

`Obs::leaf` stays `Option<Vec<Vec<usize>>>`, so the eager-versus-lazy walk
still compares chains exactly, tuple for tuple and in order.
`collect_tuples_recovers_colliding_tuples` (lines 314–323) is Step 12's.

- [ ] **Step 18: The constant view holds its chain inline**

`kermit-algos/src/hash/singleton.rs`, line 12:

```rust
use kermit_iters::{HashTrieIterable, HashTrieIterator, JoinIterable, LeafRows, RowId};
```

Lines 21–40 (the struct) become the struct below. The `#[allow(dead_code)]
value` sentinel goes too, since `data[0]` *is* the value:

```rust
/// A singleton "hash trie" containing one value.
///
/// Implements [`HashTrieIterator`] over a single-value relation. Used by
/// the const-view rewrite to expose a `Const_c<id> = {c<id>}` predicate as
/// if it were a real hash-trie-backed relation.
#[derive(Debug, Clone)]
pub struct SingletonHashTrieIter {
    hash: u64,
    /// The one-row tuple buffer: the value itself, as a unary tuple.
    data: [usize; 1],
    /// The chain [`HashTrieIterator::leaf_tuples`] lends over `data` once
    /// the iterator has been [`open`](HashTrieIterator::open)ed: its one
    /// row, id 0. Inline, like `data`, so neither the trait method nor
    /// constructing or cloning the iterator allocates.
    rows: [RowId; 1],
    state: State,
}
```

Lines 51–58, `new` (its doc comment, lines 43–50, is unchanged):

```rust
    pub fn new(value: usize, hash: u64) -> Self {
        Self {
            hash,
            data: [value],
            rows: [0],
            state: State::Root,
        }
    }
```

Lines 117–123, `leaf_tuples`:

```rust
    fn leaf_tuples(&self) -> Option<LeafRows<'_>> {
        if self.state == State::AtValue {
            Some(LeafRows::from_parts(&self.data, 1, &self.rows))
        } else {
            None
        }
    }
```

Test `leaf_tuples_returns_singleton_chain` (lines 182–188):

```rust
    #[test]
    fn leaf_tuples_returns_singleton_chain() {
        let mut it = SingletonHashTrieIter::new(42, h(42));
        it.open();
        let chain = it.leaf_tuples().expect("singleton's leaf chain after open");
        assert_eq!(chain.to_vecs(), vec![vec![42]]);
        assert_eq!(chain.ids(), &[0]);
        assert_eq!(chain.arity(), 1);
    }
```

`leaf_tuples_returns_none_before_open` (190–194) is unchanged.
`HashKindIter::Singleton(s.clone())` (`iter_kind.rs:125`) and
`kermit/src/db.rs:126` (`SingletonHashTrieIter::new(id, H::hash(id))`)
compile unchanged. A clone used to copy a `Vec<Vec<usize>>`, two heap
allocations per join per constant; it now copies 32 bytes.

- [ ] **Step 19: The selection view copies row ids, not tuples**

`kermit-algos/src/hash/selection.rs`, line 16:

```rust
use {
    crate::selection_rewrite::ColumnEquality,
    kermit_iters::{HashTrieIterator, LeafRows, RowId},
};
```

Lines 52–56, the `leaf` field, become two fields:

```rust
    /// Ids of the inner leaf chain's rows that satisfy every equality, in
    /// chain order; meaningful only while `at_leaf`. Owned, since the
    /// filter is the view's own. [`HashTrieIterator::leaf_tuples`] lends
    /// them over the inner chain's buffer, so a selected atom copies 4-byte
    /// ids, never tuples, once per leaf bucket visited, into an allocation
    /// it reuses across buckets.
    leaf_ids: Vec<RowId>,
    /// `true` iff the inner sat on an occupied leaf bucket when `leaf_ids`
    /// was last refreshed.
    at_leaf: bool,
```

Lines 68–74, in `new`:

```rust
        Self {
            inner,
            equalities: equalities.to_vec(),
            source_of,
            frames: Vec::new(),
            leaf_ids: Vec::new(),
            at_leaf: false,
        }
```

Lines 83–93, `refresh_leaf`:

```rust
    /// Recomputes the filtered leaf chain for the inner's current bucket:
    /// the ids of its rows that satisfy every equality, in chain order.
    fn refresh_leaf(&mut self) {
        self.leaf_ids.clear();
        let chain = self.inner.leaf_tuples();
        self.at_leaf = chain.is_some();
        if let Some(chain) = chain {
            let equalities = &self.equalities;
            self.leaf_ids.extend(
                chain
                    .ids()
                    .iter()
                    .zip(chain.iter())
                    .filter(|(_, t)| equalities.iter().all(|e| t[e.source] == t[e.repeat]))
                    .map(|(&id, _)| id),
            );
        }
    }
```

(This borrow-checks because `chain` borrows the field `self.inner` while
`self.at_leaf`, `self.equalities` and `self.leaf_ids` are distinct fields;
nothing calls a `&mut self` method while `chain` is live.)

Line 108, in `next`'s constrained branch, `self.leaf = None;` becomes
`self.at_leaf = false;`. Line 177, in `up`, `self.leaf = None;` becomes
`self.at_leaf = false;`.

Lines 181–186, `leaf_tuples`:

```rust
    fn leaf_tuples(&self) -> Option<LeafRows<'_>> {
        match self.constrained_top() {
            | Some((_, true)) => None,
            | _ if !self.at_leaf => None,
            | _ => {
                // The filtered ids index the inner chain's buffer: lend
                // that buffer with them.
                let chain = self.inner.leaf_tuples()?;
                Some(LeafRows::from_parts(chain.data(), chain.arity(), &self.leaf_ids))
            },
        }
    }
```

On the borrow: `self.inner.leaf_tuples()` borrows `self.inner` for the `'_`
of `&self`, and `chain.data()` returns that `'_` buffer (T2's
`data(&self) -> &'a [usize]`), not a borrow of the local `chain`. So the
result outlives `chain`, and `&self.leaf_ids` has the same `'_`. Whenever
`at_leaf` holds, the inner has not moved since `refresh_leaf` read a chain
there, so the `?` never fires. The `?` keeps the method total all the same.
`HashTriejoin`'s `emit_leaf` calls this once per candidate, which now means
one inner `leaf_tuples` call instead of one `as_deref`; only `Select_` atoms
pay it.

Tests. Line 235, in `collect`'s inner `go`:

```rust
                    out.extend(it.leaf_tuples().unwrap_or(&[]).iter().cloned());
```
becomes
```rust
                    if let Some(rows) = it.leaf_tuples() {
                        out.extend(rows.iter().map(<[usize]>::to_vec));
                    }
```

Line 275 (`constrained_level_is_a_one_bucket_table`):

```rust
        assert_eq!(it.leaf_tuples(), Some(&[vec![1, 1]][..]));
```
becomes
```rust
        assert_eq!(it.leaf_tuples().map(|rows| rows.to_vecs()), Some(vec![vec![1, 1]]));
```

Line 350 (`leaf_filter_rejects_hash_collision`):

```rust
        assert_eq!(it.leaf_tuples(), Some(&[][..]));
```
becomes
```rust
        assert_eq!(
            it.leaf_tuples().map(|rows| rows.to_vecs()),
            Some(Vec::<Vec<usize>>::new())
        );
```

(It is still `Some` with no rows: the bucket is admitted and the filter
rejects its only row. That is the old `Some(&[])`, and `emit_leaf` returns on
the zero length as before.)

New test, after `leaf_filter_keeps_true_diagonal_among_colliders` (ends at
line 363):

```rust
    #[test]
    fn selected_leaf_lends_the_inner_buffer_with_filtered_ids() {
        // All three tuples collide into one leaf chain, ids 0, 1 and 2. The
        // view keeps id 1, the true diagonal, and reads it from the
        // relation's buffer instead of copying the tuple.
        let r =
            CollidingHashTrie::from_tuples(2.into(), vec![vec![1, 11], vec![1, 1], vec![1, 21]]);
        let mut it = view(&r, &[eq(0, 1)]);
        assert!(it.open());
        assert!(it.open());
        let rows = it.leaf_tuples().expect("the admitted leaf bucket");
        assert_eq!(rows.ids(), &[1]);
        assert_eq!(rows.to_vecs(), vec![vec![1, 1]]);

        let mut plain = r.hash_trie_iter();
        assert!(plain.open());
        assert!(plain.open());
        let chain = plain.leaf_tuples().expect("the same bucket, unfiltered");
        assert_eq!(chain.ids(), &[0, 1, 2]);
        assert_eq!(rows.data().as_ptr(), chain.data().as_ptr());
    }
```

(Under `Mod10HashStrategy` the root has one bucket (every first value is 1),
and its child has one bucket (11, 1 and 21 all hash to 1). So the plain
iterator's two `open`s land on the bucket the view admits.)

- [ ] **Step 20: `HashKindIter` delegates the view**

`kermit-algos/src/hash/iter_kind.rs`, line 12:

```rust
    kermit_iters::{HashTrieIterable, HashTrieIterator, JoinIterable, LeafRows},
```

Lines 110–116:

```rust
    fn leaf_tuples(&self) -> Option<LeafRows<'_>> {
        match self {
            | Self::Relation(it) => it.leaf_tuples(),
            | Self::Singleton(it) => it.leaf_tuples(),
            | Self::Selection(it) => it.leaf_tuples(),
        }
    }
```

Line 178 (`selection_variant_delegates`):

```rust
        assert_eq!(it.leaf_tuples(), Some(&[vec![1, 1]][..]));
```
becomes
```rust
        assert_eq!(it.leaf_tuples().map(|rows| rows.to_vecs()), Some(vec![vec![1, 1]]));
```

- [ ] **Step 21: `emit_leaf` reads candidates with `chain(k).row(i)`**

`kermit-algos/src/hash/hash_triejoin.rs`, lines 187–226:

```rust
/// Algorithm 3 lines 16–19. Cross-product the leaf chains of every
/// iterator and emit each verified candidate.
///
/// Allocates nothing: each chain is re-borrowed through `leaf_tuples()` as
/// a `LeafRows` view (row ids over its relation's tuple buffer) rather than
/// collected, a candidate's tuples are slices of those buffers, and the
/// cursor and output row live in `scratch`.
fn emit_leaf<IT: HashTrieIterator, S: FnMut(&[usize])>(
    iters: &[IT], predicate_variables: &[Vec<usize>], scratch: &mut LeafScratch, emit: &mut S,
) {
    let chain = |k: usize| {
        iters[k]
            .leaf_tuples()
            .expect("at leaf level for every participating iter")
    };
    for (k, len) in scratch.chain_lens.iter_mut().enumerate() {
        *len = chain(k).len();
    }
    if scratch.chain_lens.contains(&0) {
        return;
    }

    scratch.cursor.fill(0);
    loop {
        let candidate = scratch
            .cursor
            .iter()
            .enumerate()
            .map(|(k, &i)| chain(k).row(i));
        if verify_and_construct(
            candidate,
            predicate_variables,
            &mut scratch.row,
            &mut scratch.bound,
        ) {
            emit(&scratch.row);
        }
        if !advance_cursor(&mut scratch.cursor, &scratch.chain_lens) {
            break;
        }
    }
}
```

The only code change is `chain(k)[i].as_slice()` → `chain(k).row(i)`, plus
the doc comment. `chain(k)` is a temporary `LeafRows<'_>` borrowed from
`iters`, and `row` returns the view's lifetime, so the `&[usize]` handed to
`verify_and_construct` (its `'t`) outlives the temporary, exactly as the old
slice did. The odometer, `chain_lens`, `advance_cursor` and
`verify_and_construct` are unchanged.

Test `verify_rejects_real_inner_level_collision`, lines 440–454:

```rust
        // Both chains hold one tuple; together they form the only candidate.
        let r_chain = r_it.leaf_tuples().expect("R at leaf");
        let s_chain = s_it.leaf_tuples().expect("S at leaf");
        assert_eq!(r_chain.to_vecs(), vec![vec![1, 2]]);
        assert_eq!(s_chain.to_vecs(), vec![vec![12, 3]]);
        let pv = vec![vec![0, 1], vec![1, 2]];
        let (mut row, mut bound) = (vec![0; 3], vec![false; 3]);
        assert!(
            !verify_and_construct([r_chain.row(0), s_chain.row(0)], &pv, &mut row, &mut bound),
            "Y = 2 in R but 12 in S: the leaf-level check must reject it"
        );
```

- [ ] **Step 22: Sweeps, and the `leaf_tuples` site list.**
  ```bash
  rg -n 'into_vecs' kermit-ds/src/ds/hash_trie
  rg -n '\bscatter\(|\bPositioned\b|\bPartition::|Vec<Partition>|into_tuples\(' kermit-ds/src
  rg -n 'tuple_count\b' kermit-ds/src/ds/hash_trie/implementation.rs
  rg -n 'from_tuple\(|\.tuple\(\)|into_tuple\(|from_tuples\(vec!\[tuple|Indexed' kermit-ds/src/ds/hash_trie
  rg -n 'fn row_id' kermit-ds/src
  rg -n 'Vec<Vec<usize>>' kermit-algos/src/hash kermit-iters/src/hash_trie.rs kermit-ds/src/ds/hash_trie/hash_trie_iter.rs kermit-ds/src/ds/hash_trie/pruning.rs
  ```
  Expected:
  - the first, second and fourth print nothing (the `\b`s keep
    `scatter_rows(` and `RowPartition::len` out of the second);
  - the third prints only the `Cardinality::tuple_count` impl and test calls
    of the method (`trie.tuple_count()`), no field;
  - `fn row_id` has exactly one hit, `pub(crate) fn row_id` in
    `kermit-ds/src/morsel.rs`;
  - the last prints exactly these, and no leaf chain is a `Vec<Vec<usize>>`
    any more:
    - `hash_triejoin.rs:25`, `build_variable_to_iter_map`'s return;
    - its tests' collected join output at lines 469, 487, 502 and 519;
    - `selection.rs`'s test helper `collect` (lines 226 and 228).

  **Every `leaf_tuples` site in the workspace** (`rg -n leaf_tuples`,
  2026-10-08):

  | Site | Change |
  | --- | --- |
  | `kermit-iters/src/hash_trie.rs:31,70` | Step 13: doc and signature |
  | `kermit-ds/src/ds/hash_trie/pruning.rs:33,124` | Step 14: comments rewritten |
  | `kermit-ds/src/ds/hash_trie/hash_trie_iter.rs:119,247` | Step 15 |
  | `hash_trie_iter.rs:416,435,463,469,489,641` | unchanged (`is_none` / `is_some`) |
  | `hash_trie_iter.rs:425` | unchanged (`expect`, then `is_empty`) |
  | `hash_trie_iter.rs:473,514,563,575` | Step 16: `to_vecs()` |
  | `kermit-ds/tests/common/macros.rs:1174,1184,1217,1277,1314,1317,1546` | unchanged |
  | `macros.rs:1210,1287,1292,1321,1426,1431,1446,1464,1497,1502,1530,1599,1604,1610,1666` | unchanged; Step 17 retypes `sorted` |
  | `macros.rs:1246` (read at 1247–1248) | Step 17: `chain.row(0)[0]` |
  | `kermit-ds/tests/hash_trie_tests.rs:266,268,282,296,307,311` | unchanged; Step 17 retypes `sorted` |
  | `hash_trie_tests.rs:337,395` | Step 17: `to_vecs()` |
  | `kermit-algos/src/hash/singleton.rs:29,34,117,183–193` | Step 18 |
  | `kermit-algos/src/hash/selection.rs:54,86,181,235,275,280,350` | Step 19 (280 unchanged) |
  | `kermit-algos/src/hash/iter_kind.rs:110–114,178` | Step 20 |
  | `kermit-algos/src/hash/hash_triejoin.rs:190,197,440,441` | Step 21 |
  | `ARCHITECTURE.md:135` | T11 (the trait listing) |
  | `docs/algorithms/hash-triejoin.md:49` | T11 (leaf product reads `LeafRows`) |
  | `docs/data-structures/hash-trie.md:110,135,142–146` | T11 (complexity row, worked example's `leaf_tuples()` answers) |
  | `CLAUDE.md:139,162` | names the method only; T11 adds `LeafRows` to the key traits |
  | `docs/specs/*`, `docs/plans/*`, `docs/superpowers/plans/*`, `docs/audits/*` | historical records, unchanged |

  No `kermit` crate code calls `leaf_tuples`, and no other type implements
  `HashTrieIterator`:
  - `Configured` and `BuiltWith` forward `hash_trie_iter()`, and their
    iterator is `HashTrieIter`.
  - `kermit/src/db.rs` only constructs the views.

  **Why `kermit/tests/result_allocation.rs` needs no edit.** Its join cells
  (`QUERY`, `DESCENT_QUERY` and `REVERSED_QUERY`) have no constants and no
  repeated variables. So every iterator is
  `HashKindIter::Relation(HashTrieIter)`, and per candidate the leaf product
  does only the following:
  - **`leaf_tuples()`** builds a `LeafRows` on the stack: two slice
    references and an arity, borrowed from `&trie.tuples` and either the
    chain's `Vec<RowId>` or, for a pruned singleton, `slice::from_ref` of the
    payload's id.
  - **`row(i)`** slices the trie's buffer.
  - **`verify_and_construct`** writes into `LeafScratch`, which
    `join_for_each` allocates once per join.

  None of these allocate, so the count does not grow with the result (the
  per-row check) or with the descents (the `*_descent_count` check). The two
  other views also allocate nothing per row, though these cells do not reach
  them:
  - **Constant view:** now allocates nothing at all, not even per join.
  - **Selection view:** allocates only when a chain outgrows `leaf_ids`'
    capacity, per bucket at most, never per result row.

  The lazy cells run warm, as before. The `*_scan_*` cells go through
  `HashTrie::for_each_tuple`, which still walks the trie (Step 7.c), reading
  each row through its id, and allocates nothing per tuple.

- [ ] **Step 23: Build and run the whole task once.**
  ```bash
  CARGO_BUILD_JOBS=2 cargo build --workspace --all-targets
  CARGO_BUILD_JOBS=2 cargo test -p kermit-iters
  CARGO_BUILD_JOBS=2 cargo test -p kermit-ds
  CARGO_BUILD_JOBS=2 cargo test -p kermit-algos
  CARGO_BUILD_JOBS=2 cargo test -p kermit --lib db::
  CARGO_BUILD_JOBS=2 cargo test -p kermit --bin kermit execution::
  CARGO_BUILD_JOBS=2 cargo test -p kermit --test result_allocation
  CARGO_BUILD_JOBS=2 cargo test -p kermit --test join_tests hashtriejoin
  CARGO_BUILD_JOBS=2 cargo test -p kermit --test cli_hash_trie_build_mode \
    --test cli_hash_trie_config_choice --test cli_hash_trie_hasher_choice \
    --test cli_hash_trie_layout_expansion --test cli_hash_trie_layout_pruning
  CARGO_BUILD_JOBS=2 cargo clippy --all-targets -- -D warnings
  nix develop --command cargo fmt --all
  RUSTDOCFLAGS=-Dwarnings cargo doc --workspace --no-deps
  ```
  (The join suites name tests with `$join_algorithm:lower`, so every HashTrie
  cell contains `hashtriejoin`, but not `hash_trie`.)

  Expected: all pass, clippy and doc clean. If `fmt` changed more than
  whitespace, re-run the kermit-ds and kermit-algos tests. What pins the
  change:
  - **Storage (Steps 1–12):**
    - the Step 1 tests;
    - `bulk_builds_the_incremental_trie_*` (lazy pending capacities),
      `radix_builds_the_bulk_trie_*`, `parallel_builds_the_bulk_trie_*`,
      `presized_parallel_builds_are_*`, `one_run_of_grouping_builds_the_bulk_root`,
      `tuples_sizes_every_child_from_its_list`,
      `the_default_builds_the_trie_it_built_before` and
      `tuples_leaves_every_subtrie_as_grow_builds_it` (ids and buffers);
    - the `lazy_tests` (pending ids, `resolve`, walks that do not expand);
    - `hash_scan_allocations` in `kermit/tests/result_allocation.rs` (the
      trie walk allocates nothing per tuple);
    - `column_distinct_leaves_a_lazy_trie_as_built` in
      `kermit/src/db/database.rs` (statistics through `TupleScan`, now a
      buffer scan, expand nothing) and `family_walk_tests` in
      `kermit/src/db.rs` (the scan and the walk lend the same multiset).
  - **Iterator and join (Steps 13–21):** the new tests
    `a_leaf_chain_lends_row_ids_into_the_trie_buffer`,
    `singleton_frames_lend_their_row_id` and
    `selected_leaf_lends_the_inner_buffer_with_filtered_ids`, with
    `leaf_tuples_returns_singleton_chain` rewritten. The existing tests pin
    that behaviour is unchanged:
    - the hash-family suites (`hash_trie_test_suite!` × every alias), with
      `lazy_expansion`'s exact eager-versus-lazy walk;
    - the selection and singleton unit tests;
    - the HashTriejoin collision tests;
    - the 16-pattern join suites on every HashTrie alias;
    - `result_allocation.rs`.

- [ ] **Step 24: Commit.**
  ```bash
  git add kermit-iters/src/hash_trie.rs \
    kermit-ds/src/ds/hash_trie/pruning.rs kermit-ds/src/ds/hash_trie/node.rs \
    kermit-ds/src/ds/hash_trie/expansion.rs kermit-ds/src/ds/hash_trie/bulk.rs \
    kermit-ds/src/ds/hash_trie/implementation.rs kermit-ds/src/ds/hash_trie/radix.rs \
    kermit-ds/src/ds/hash_trie/parallel.rs kermit-ds/src/ds/hash_trie/identity.rs \
    kermit-ds/src/ds/hash_trie/hash_trie_iter.rs kermit-ds/src/morsel.rs \
    kermit-ds/src/configured.rs kermit-ds/src/relation.rs \
    kermit-ds/tests/common/macros.rs kermit-ds/tests/parquet_tests.rs \
    kermit-ds/tests/hash_trie_tests.rs \
    kermit-algos/src/hash/singleton.rs kermit-algos/src/hash/selection.rs \
    kermit-algos/src/hash/iter_kind.rs kermit-algos/src/hash/hash_triejoin.rs
  git status --short   # nothing else modified
  git commit -m "$(cat <<'EOF'
  refactor(hash-trie)!: own the tuple buffer; chains hold row ids (#111)

  HashTrie keeps the Tuples batch it is built from as its buffer, in
  arrival order, and names every tuple below its tables by RowId: a leaf
  chain is a Vec<RowId>, a pruned Singleton a RowId, a lazy pending list a
  Vec<RowId>. Every build mode (bulk, incremental, radix, parallel,
  presized) and lazy resolve place ids instead of moving Vec<usize>s;
  parallel and presized scatter ids through morsel::scatter_rows, and the
  old morsel::scatter, Partition and Positioned are gone. space counts the
  buffer once plus 4 B per id slot. for_each_tuple still walks the trie
  (bench ds times it), reading rows through ids; scan_tuples and
  collect_tuples read the buffer in arrival order.

  HashTrieIterator::leaf_tuples returns a LeafRows view over the buffer.
  HashTrieIter lends chains and pruned singletons through it, the constant
  view holds its one row inline, the selection view filters ids instead of
  cloning tuples, and emit_leaf reads candidates with chain(k).row(i). The
  identity tests compare ids after checking the two buffers are equal.

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_015P4eTR34w48GpD7DvPPaee
  EOF
  )"
  ```

- [ ] **Step 25: Mutation checks (after the commit).** Follow the ground
  rules' procedure for each.
  - **MS1** `child` (`bulk.rs`): delete `list.shrink_to(first_capacity);`.
    Fails `bulk_builds_the_incremental_trie_under_siphash` ("pending: chain
    capacity"). Run `CARGO_BUILD_JOBS=2 cargo test -p kermit-ds bulk_builds_the_incremental_trie`.
  - **MS2** `tuple_list_heap_bytes`: `size_of::<RowId>()` →
    `size_of::<usize>()`. Fails
    `heap_size_counts_the_buffer_the_tables_and_four_bytes_per_id`. Run
    `CARGO_BUILD_JOBS=2 cargo test -p kermit-ds heap_size_counts_the_buffer`.
  - **MS3** `Relation::insert`: `row_id(self.tuples.len() - 1)` →
    `row_id(self.tuples.len() - 1).saturating_sub(1)`. Fails
    `insert_appends_its_row_and_chains_the_new_id` (`[0, 0]`, not `[0, 1]`).
    Run `CARGO_BUILD_JOBS=2 cargo test -p kermit-ds insert_appends_its_row`.
  - **MS4** `for_each_tuple`: `Self::visit_at(&self.tuples, &self.root, &mut visit);`
    → `self.tuples.rows().for_each(&mut visit);` (a buffer scan). Fails
    `for_each_tuple_walks_the_trie_and_scan_tuples_reads_the_buffer` (the
    first keys come back in three runs, not two). Run
    `CARGO_BUILD_JOBS=2 cargo test -p kermit-ds for_each_tuple_walks`.
  - **MI1** `selection.rs`: in `refresh_leaf`, replace
    `.filter(|(_, t)| equalities.iter().all(|e| t[e.source] == t[e.repeat]))`
    with `.filter(|_| true)`. Run
    `CARGO_BUILD_JOBS=2 cargo test -p kermit-algos selection`. It must fail
    `leaf_filter_rejects_hash_collision`,
    `leaf_filter_keeps_true_diagonal_among_colliders` and
    `selected_leaf_lends_the_inner_buffer_with_filtered_ids`. These are the
    colliding-hash tests. Under SipHash the constrained level alone already
    separates the diagonal, so `diagonal_view_yields_only_diagonal_tuples`
    stays green under this mutant.
  - **MI2** `hash_triejoin.rs`: in `emit_leaf`, replace `chain(k).row(i)`
    with `chain(k).row(0)`. Run
    `CARGO_BUILD_JOBS=2 cargo test -p kermit-algos hash_triejoin`. It must
    fail `join_algo_drops_leaf_chain_collision_false_positive`: R's shared
    chain `{1, 11}` would yield `(1)` at both cursor positions, both
    candidates would fail verification, and the only true row `(11)` would
    be lost.

#### Reference: every HashTrie test that reads chain contents directly

| Test (file:line) | Reads | Change | Steps |
|---|---|---|---|
| `implementation.rs` `collect_tuples_recovers_input_as_multiset` :950 | `collect_tuples` | renamed, exact arrival order (Step 1) | 1–12 |
| `implementation.rs` `project_drops_columns` :998, `project_reorders_columns` :1013, `project_preserves_config` :1092 | `collect_tuples` | `.to_vecs()`, exact (Step 7.k) | 1–12 |
| `implementation.rs` `check_pruning_invariant(_at)` :1171 + 9 callers | `payload.tuple()` | `tuples.row(*payload.row())`, takes the trie (Step 7.j) | 1–12 |
| `implementation.rs` `pruning_arity_1_has_no_singletons` :1253, three `unprune_*` :1265–1290 | `collect_tuples` | `.to_vecs()`, exact (Step 7.k) | 1–12 |
| `implementation.rs` `incremental_insert_unprunes_like_bulk_build` :1292 | `tuple_count` field | `tuples.len()` | 1–12 |
| `implementation.rs` `pruned_build_is_insertion_order_independent` :1304, `pruned_and_plain_collect_the_same_tuples` :1429 | `collect_tuples` | `.to_vecs()` / `Tuples` equality | 1–12 |
| `implementation.rs` `node_does_not_grow_under_the_expansion_policy` :1351 | mirrors of `Leaf`/`Singleton` | `Vec<RowId>` / `RowId` | 1–12 |
| `implementation.rs` `for_each_tuple_visits_what_collect_tuples_returns` :1444 | walk + `collect_tuples` | rewritten as `for_each_tuple_walks_the_trie_and_scan_tuples_reads_the_buffer`: the walk lends the multiset one subtrie at a time, the scan and `collect_tuples` the buffer in arrival order (Step 1) | 1–12 |
| `implementation.rs` `lazy_tests` :1510–1698 (`pending_of` and 9 callers, `insert_reaches…`, `walks_read…`, `projection_of_a_lazy…`) | pending lists, `collect_tuples` | `pending_of(&trie, …)` maps ids to rows; ids asserted (Step 1, Step 7.l) | 1–12 |
| `implementation.rs` `check_default_identity` :1766 | whole trie | buffer rows + nodes, not total heap (Step 7.m) | 1–12 |
| `implementation.rs` `check_children_unchanged` :1884 | root chains | none (ids; equal buffers) | 1–12 |
| `bulk.rs` `bulk_keeps_chain_order_under_colliding_hashes` :236 | chain order | reads the one chain's ids `[0, 1, 2, 3, 4]` (Step 6) | 1–12 |
| `bulk.rs` `tuples_below` :321, `check_children_sized` :362 | chain/pending lengths | none | 1–12 |
| `parallel.rs` `check_one_run` :658 | lists, tail | ids (Step 9) | 1–12 |
| `identity.rs` `assert_same_chain`, `assert_same_node` Singleton arm | chains, singletons, pending | ids + buffer premise (Step 10) | 1–12 |
| `expansion.rs` `expand_moves_the_pending_tuples_once` :177, `built_mut_reaches_the_expanded_table` :197, `own_heap_bytes_is_the_box` :206 | pending list | `Vec<RowId>` (Step 5) | 1–12 |
| `pruning.rs` `vec_payload_round_trips_the_tuple` :209 | payload | `row_payload_round_trips_the_id` (Step 3) | 1–12 |
| `morsel.rs` `scatter_*` :211, :256, `empty_input_needs_no_work` :316 | scattered tuples | deleted / `scatter_rows` (Step 11) | 1–12 |
| `configured.rs` :208, :233, :268 | `collect_tuples` | `.to_vecs()` (Step 12) | 1–12 |
| `tests/common/macros.rs` `assert_round_trip` :1090, `insert_matches_from_tuples` :1125 | `collect_tuples` | `.to_vecs()` (Step 12) | 1–12 |
| `tests/parquet_tests.rs` `sorted_tuples` :64 | `collect_tuples` | `.to_vecs()` (Step 12) | 1–12 |
| `tests/hash_trie_tests.rs` `collect_tuples_recovers_colliding_tuples` :315 | `collect_tuples` | exact arrival order (Step 12) | 1–12 |
| `hash_trie_iter.rs` tests :412–660 (`leaf_tuples_*`, pruned and lazy leaves) | `leaf_tuples` | `LeafRows` | 13–21 |
| `pruning.rs` `frame_emulates_a_one_entry_table` :225, `frame_is_a_borrow_a_hash_and_a_flag` :241 | `SingletonFrame` | `&RowId` | 13–21 |
| `tests/hash_trie_tests.rs` `hash_trie_collisions` :255–340, `lazy_expansion::step` :395 | `leaf_tuples` | `LeafRows::to_vecs` | 13–21 |
| `tests/common/macros.rs` `hash_trie_traversal_tests!` :1174–1426 | `leaf_tuples` | `LeafRows` | 13–21 |
| `kermit-algos/src/hash/{singleton,selection,iter_kind,hash_triejoin}.rs` tests | `leaf_tuples` | `LeafRows` | 13–21 |
| `kermit-ds/src/relation.rs` `hash_rows` (T3's test helper) | `collect_tuples` | `.to_vecs()` (Step 12) | 1–12 |
| `kermit/tests/*`, `kermit/src` tests | — | none read chains: `result_allocation.rs`, `db.rs` `family_walk_tests`, `execution.rs` `hash_family_*` and `database.rs` go through `for_each_tuple`, `scan_tuples`, `heap_size_bytes` and joins, and compare sorted multisets or relative sizes | — |

---

### Task 8: Remove the `into_vecs` shim; sweep leftovers

T3 added `Tuples::into_vecs` so each structure could keep its
`Vec<Vec<usize>>` build until it moved to rows. T5, T6 and T7 replaced
every function that called it, so its only users left are T1's own tests,
which use it as a second spelling of `to_vecs`. It goes, so nothing can
reach for a per-tuple copy again (Acceptance 1). `to_vecs` stays: it is the
contract's permanent helper for tests and diagnostics.

**Files:**
- Modify: `kermit-iters/src/tuples.rs` (the `into_vecs` method; tests
  `an_empty_vec_is_an_empty_batch` and `vectors_round_trip`)

- [ ] **Step 1: Find every user.**
  ```bash
  rg -n 'into_vecs' --glob '!docs/**'
  ```
  Expected, and nothing else:
  - `kermit-iters/src/tuples.rs`: the method itself, and three test lines
    (`assert_eq!(tuples.into_vecs(), …)` in `an_empty_vec_is_an_empty_batch`;
    `tuples.clone().into_vecs()` and `nullary.into_vecs()` in
    `vectors_round_trip`).

  If any other file prints, a structure still converts through the shim:
  stop and report it rather than deleting the method. The task that owns
  that structure (T5 TreeTrie, T6 ColumnTrie, T7 HashTrie) missed a site,
  and the fix belongs there, not here. (`docs/` holds only this plan and the
  spec, which name the shim on purpose.)

- [ ] **Step 2: Delete the method.** In `kermit-iters/src/tuples.rs`,
  delete:

```rust
    /// Every row as its own `Vec`, one allocation per row: the bridge for
    /// builds that still take one `Vec` per tuple, until they build from
    /// row slices (#111).
    pub fn into_vecs(self) -> Vec<Vec<usize>> { self.to_vecs() }

```

  `to_vecs` (the next method) is unchanged.

- [ ] **Step 3: Point T1's tests at `to_vecs`.** No test exists for the
  shim alone; two tests used it as an alternative spelling.

  In `an_empty_vec_is_an_empty_batch`, before:

```rust
        assert_eq!(tuples.into_vecs(), Vec::<Vec<usize>>::new());
```

  After:

```rust
        assert_eq!(tuples.to_vecs(), Vec::<Vec<usize>>::new());
```

  In `vectors_round_trip`, before:

```rust
        assert_eq!(Tuples::from(tuples.to_vecs()), tuples);
        assert_eq!(tuples.clone().into_vecs(), rows);
        let nullary = Tuples::from(vec![vec![]; 2]);
        assert_eq!(nullary.into_vecs(), vec![Vec::<usize>::new(); 2]);
```

  After (the line that duplicated `to_vecs()` goes):

```rust
        assert_eq!(Tuples::from(tuples.to_vecs()), tuples);
        let nullary = Tuples::from(vec![vec![]; 2]);
        assert_eq!(nullary.to_vecs(), vec![Vec::<usize>::new(); 2]);
```

- [ ] **Step 4: Sweep the leftovers.**
  ```bash
  rg -n 'into_vecs' --glob '!docs/**'
  rg -n 'sort_unstable_by\(' kermit-ds/src/ds
  for f in $(rg -l 'Vec<Vec<usize>>' kermit-ds/src --glob '!**/tests.rs'); do
    t=$(grep -n '^#\[cfg(test)\]' "$f" | head -1 | cut -d: -f1)
    awk -v t="${t:-999999}" -v f="$f" '/Vec<Vec<usize>>/ && NR < t {print f":"NR": "$0}' "$f"
  done
  ```
  Expected:
  - `into_vecs`: nothing.
  - `sort_unstable_by(`: nothing. At `47950f0` its only three hits under
    `kermit-ds/src/ds` were the hand-rolled tuple comparator
    (`tree_trie/implementation.rs:246,319`, `column_trie/implementation.rs:492`),
    which T5 and T6 replaced with `Tuples::sort`.
  - The loop prints every `Vec<Vec<usize>>` in kermit-ds outside its test
    modules. Expected: only doc-comment lines that name the
    `From<Vec<Vec<usize>>>` conversion (the `# Panics` sections of
    TreeTrie's `from_tuples` and ColumnTrie's `from_tuples_with_build_mode`).
    Any code line is a leftover per-tuple path: report it with its task.

- [ ] **Step 5: Run.**
  ```bash
  CARGO_BUILD_JOBS=2 cargo test -p kermit-iters
  CARGO_BUILD_JOBS=2 cargo build --workspace --all-targets
  ```
  Then the workspace, through the long-run procedure (it takes the longest
  of any step so far):
  ```bash
  setsid nohup env CARGO_BUILD_JOBS=2 cargo test --workspace > $SCRATCH/t8-test.log 2>&1 &
  echo $! > $SCRATCH/t8-test.pid; disown
  while kill -0 "$(cat $SCRATCH/t8-test.pid)" 2>/dev/null; do sleep 30; done
  grep -E '^test result|FAILED|panicked' $SCRATCH/t8-test.log | sort | uniq -c
  ```
  Expected: the 14 `tuples::tests` pass (none removed: Step 3 only changed
  assertions), the build succeeds, and every `test result:` line in the log
  reads `ok` with `0 failed`.

- [ ] **Step 6: Lint, format, document.**
  ```bash
  CARGO_BUILD_JOBS=2 cargo clippy --all-targets -- -D warnings
  nix develop --command cargo fmt --all
  RUSTDOCFLAGS=-Dwarnings cargo doc --workspace --no-deps
  ```
  Expected: all clean.

- [ ] **Step 7: Commit.**
  ```bash
  git add kermit-iters/src/tuples.rs
  git commit -m "$(cat <<'EOF'
  refactor(iters): remove the Tuples::into_vecs shim (#111)

  Every structure builds from rows since the TreeTrie, ColumnTrie and
  HashTrie changes, so nothing converts a batch back into one Vec per
  tuple. to_vecs stays for tests and diagnostics.

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_015P4eTR34w48GpD7DvPPaee
  EOF
  )"
  ```

---

### Task 9: Allocation tests: nothing per row from file to build

**How the counting works today.** `kermit`'s dev-dependency `allocation-counter = "0.8"`
declares a `#[global_allocator]` (`CountingAllocator`, in the crate's `allocator.rs`), so
every `kermit` test binary that names `allocation_counter` counts allocations through it:
the integration tests (`kermit/tests/result_allocation.rs`) and the binary's own unit tests
(`kermit/src/main.rs` `csv_sink_allocates_nothing_per_row`, 1372–1388; the binary's
jemalloc is `#[cfg(all(feature = "jemalloc", not(test)))]`, main.rs 57). `measure(f)`
counts the calling thread only. The allocator keeps `GlobalAlloc`'s default `realloc`,
which calls `alloc`, so every reallocation counts as one allocation: a `Vec` grown one
push at a time costs one allocation per growth step. No other crate in the workspace has a
counting allocator, so the tests live in `kermit`: a new integration test file, plus one
unit test in the binary for the binary's own hand-over (`load_with_tuples`).

Runs after T5–T8: it measures the final builds.

**Files:**
- Create: `kermit/tests/tuple_allocation.rs`
- Modify: `kermit/src/execution.rs` (`mod tests`: one test after
  `load_with_tuples_returns_file_order`, ~1306–1328 at `47950f0`)

- [ ] **Step 1: Write the integration tests.** Create `kermit/tests/tuple_allocation.rs`:

```rust
//! Issue #111: no tuple costs a heap allocation of its own between reading a
//! file and building any structure, except HashTrie's chains.
//!
//! Each test counts the calling thread's allocations with `allocation-counter`,
//! whose counting global allocator this test binary links, as
//! `result_allocation.rs` does. A reallocation counts as one allocation (the
//! counter keeps `GlobalAlloc`'s default `realloc`, an `alloc` plus a
//! `dealloc`), so a `Vec` grown one push at a time costs one allocation per
//! growth step: [`doublings`].
//!
//! - **Readers** are measured at two sizes ten times apart. A reader's one
//!   buffer doubles, so the larger read may cost ⌈log₂ 10⌉ = 4 more
//!   allocations; arrow decodes Parquet in 1 024-row record batches and
//!   allocates per batch, never per row. A per-row allocation costs 9 000
//!   more.
//! - **A batch clone** and **a reordered copy** (`IndexSpec::permute_all`)
//!   are one allocation each.
//! - **TreeTrie and ColumnTrie builds** are bounded by the heap objects the
//!   built trie owns: TreeTrie a children `Vec` per inner node, ColumnTrie two
//!   arrays per layer. A per-row allocation adds 10 000.
//! - **HashTrie builds** keep a chain `Vec` per distinct tuple, by design
//!   until #101's layer 3, so they are measured on the same 100 distinct
//!   tuples repeated once and 100 times: the tables and chains are the same,
//!   and only the row-id lists grow, by at most ⌈log₂ 100⌉ = 7 growth steps
//!   each. A per-row allocation adds 9 900.
//!
//! The parallel builds (`parallel:N`, `presized:N`) are not measured: the
//! counter sees only the calling thread, and their workers allocate on
//! threads of their own.

use {
    kermit_algos::IndexSpec,
    kermit_ds::{
        BuildModeRelation, Cardinality, ColumnTrie, ColumnTrieBuildMode, EagerExpansion,
        ExpansionPolicy, HashTrie, HashTrieBuildMode, LazyExpansion, NoPruning, PruningPolicy,
        RadixBits, Relation, SingletonPruning, TreeTrie, Tuples,
    },
    kermit_iters::SipHashStrategy,
    kermit_rdf::{parquet::write_relation, partition::PartitionedRelation},
    std::{fmt::Write, path::Path},
};

/// Allocations a `Vec` grown one push at a time makes to hold `len`
/// elements: std grows an empty `Vec` of 2- to 1 024-byte elements to 4
/// slots, then doubles it.
fn doublings(len: usize) -> u64 {
    let (mut capacity, mut allocations) = (0, 0);
    while capacity < len {
        capacity = (capacity * 2).max(4);
        allocations += 1;
    }
    allocations
}

/// ⌈log₂ ratio⌉: how many more growth steps a buffer `ratio` times longer
/// can need.
fn ceil_log2(ratio: usize) -> u64 { u64::from(ratio.next_power_of_two().trailing_zeros()) }

// ── Readers ─────────────────────────────────────────────────────────────

/// Rows of the smaller and the larger reader input, ten times apart.
const SMALL: usize = 1_000;
const LARGE: usize = 10_000;

/// `rows` 2-ary rows `(i, i % 7)`.
fn pairs(rows: usize) -> Vec<(usize, usize)> { (0..rows).map(|i| (i, i % 7)).collect() }

/// Writes `pairs(rows)` as `<dir>/r.csv` and `<dir>/r.parquet`. One file name
/// at every size, so the relation name the readers allocate is the same.
fn write_inputs(dir: &Path, rows: usize) {
    let mut csv = String::from("s,o\n");
    for (s, o) in pairs(rows) {
        writeln!(csv, "{s},{o}").unwrap();
    }
    std::fs::write(dir.join("r.csv"), csv).unwrap();
    let relation = PartitionedRelation {
        name: "r".into(),
        tuples: pairs(rows),
    };
    write_relation(&relation, &dir.join("r.parquet")).unwrap();
}

fn csv_tuples(path: &Path) -> Tuples { kermit_ds::read_csv(path).unwrap().1 }

fn parquet_tuples(path: &Path) -> Tuples { kermit_ds::read_parquet(path).unwrap().1 }

/// Allocations of one `read` of `path`, after checking it returned `rows`
/// rows. An unmeasured read runs first, so a one-time initialisation on the
/// read path cannot count against the size measured first.
fn read_allocations(read: fn(&Path) -> Tuples, path: &Path, rows: usize) -> u64 {
    drop(read(path));
    let mut tuples = None;
    let info = allocation_counter::measure(|| tuples = Some(read(path)));
    assert_eq!(tuples.expect("measured").len(), rows, "{path:?}");
    info.count_total
}

/// Allocations of `read` over `<file>` at [`SMALL`] and at [`LARGE`] rows.
fn small_and_large(read: fn(&Path) -> Tuples, file: &str) -> (u64, u64) {
    let (small, large) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    write_inputs(small.path(), SMALL);
    write_inputs(large.path(), LARGE);
    (
        read_allocations(read, &small.path().join(file), SMALL),
        read_allocations(read, &large.path().join(file), LARGE),
    )
}

/// One buffer grows with the rows; the record that holds each row is reused.
/// The buffer doubles, so ten times the rows costs at most ⌈log₂ 10⌉ = 4 more
/// allocations; 2 more allow for the final shrink and the record's growth.
#[test]
fn read_csv_allocates_independently_of_its_row_count() {
    let (small, large) = small_and_large(csv_tuples, "r.csv");
    let bound = small + ceil_log2(LARGE / SMALL) + 2;
    assert!(
        large <= bound,
        "read_csv: {small} allocations for {SMALL} rows, {large} for {LARGE}, over {bound}: \
         some row allocates"
    );
}

/// The buffer is sized once from the footer's row count. Arrow decodes 1 024
/// rows per record batch and allocates a few buffers per batch and column:
/// nine more batches here. The bound allows a quarter of an allocation per
/// extra row (250 per extra batch); one per row would be four times over it.
#[test]
fn read_parquet_allocates_per_batch_not_per_row() {
    let (small, large) = small_and_large(parquet_tuples, "r.parquet");
    let bound = small + (LARGE - SMALL) as u64 / 4;
    assert!(
        large <= bound,
        "read_parquet: {small} allocations for {SMALL} rows, {large} for {LARGE}, over \
         {bound}: some row allocates"
    );
}

// ── Copies ──────────────────────────────────────────────────────────────

/// First keys of the sorted-trie fixture.
const FIRST_KEYS: usize = 2_500;
/// Second keys per first key: what an empty `Vec`'s first allocation holds,
/// so each first key's children cost TreeTrie exactly one allocation.
const FAN_OUT: usize = 4;

/// `(x, y)` for `x < FIRST_KEYS`, `y < FAN_OUT`: 10 000 distinct rows, in
/// descending order so a build's sort moves every one. Exactly sized.
fn fanned_out() -> Tuples {
    let mut tuples = Tuples::with_capacity(2, FIRST_KEYS * FAN_OUT);
    for x in (0..FIRST_KEYS).rev() {
        for y in (0..FAN_OUT).rev() {
            tuples.push(&[x, y]);
        }
    }
    tuples
}

/// A batch clone is one `Vec<usize>` copy at the batch's exact size: what
/// every `bench ds` / `bench run` setup clone and `load_with_tuples` pay.
#[test]
fn cloning_a_batch_allocates_once() {
    let batch = fanned_out();
    let mut copy = None;
    let info = allocation_counter::measure(|| copy = Some(batch.clone()));
    assert_eq!(info.count_total, 1);
    let copy = copy.expect("measured");
    assert_eq!(copy, batch);
    assert_eq!(
        copy.heap_size_bytes(),
        batch.len() * batch.arity() * std::mem::size_of::<usize>()
    );
}

/// A reordered copy under `--column-orders any` is one buffer.
#[test]
fn a_reordered_copy_allocates_once() {
    let batch = fanned_out();
    let spec = IndexSpec::new("r", vec![1, 0]);
    let mut copy = None;
    let info = allocation_counter::measure(|| copy = Some(spec.permute_all(&batch)));
    assert_eq!(info.count_total, 1);
    assert_eq!(copy.expect("measured").len(), batch.len());
}

// ── Builds ──────────────────────────────────────────────────────────────

/// Room for a sort's scratch buffers (a permutation and a gather, at arities
/// above 4 only) and a constant or two. A per-row allocation adds 10 000.
const SCRATCH: u64 = 4;

/// TreeTrie owns one heap object per inner node, its children `Vec`: the
/// root's, grown one push per first key, and one per first key, whose
/// `FAN_OUT` leaves fit its first allocation. Leaves own nothing. Expected
/// count: 2 500 + 11.
#[test]
fn tree_trie_build_allocates_once_per_inner_node() {
    let tuples = fanned_out();
    let rows = tuples.len();
    let mut built: Option<TreeTrie> = None;
    let info = allocation_counter::measure(|| built = Some(TreeTrie::from_tuples(2.into(), tuples)));
    assert_eq!(built.expect("measured").tuple_count(), rows);
    let nodes = FIRST_KEYS as u64 + doublings(FIRST_KEYS);
    assert!(
        info.count_total <= nodes + SCRATCH,
        "TreeTrie: {} allocations for {rows} rows; its inner nodes account for {nodes}",
        info.count_total
    );
}

/// ColumnTrie owns its `layers` `Vec` and each layer's `data` and `interval`
/// arrays, each at most one entry per row and grown one push at a time, under
/// both build modes. Expected count: 1 + 11 + 1 + 13 + 11 = 37 of at most 57.
#[test]
fn column_trie_builds_allocate_only_their_layer_arrays() {
    for mode in [ColumnTrieBuildMode::Bulk, ColumnTrieBuildMode::Incremental] {
        let tuples = fanned_out();
        let rows = tuples.len();
        let mut built: Option<ColumnTrie> = None;
        let info = allocation_counter::measure(|| {
            built = Some(ColumnTrie::from_tuples_with_build_mode(2.into(), mode, tuples));
        });
        assert_eq!(built.expect("measured").tuple_count(), rows, "{mode:?}");
        let arrays = 1 + 2 * 2 * doublings(rows);
        assert!(
            info.count_total <= arrays + SCRATCH,
            "ColumnTrie {mode:?}: {} allocations for {rows} rows; its layer arrays account \
             for {arrays}",
            info.count_total
        );
    }
}

/// Distinct first keys, and distinct second keys under each, of the HashTrie
/// fixture: 100 distinct tuples.
const KEYS: usize = 10;
/// How many times the larger HashTrie input repeats the 100 tuples.
const REPEATS: usize = 100;

/// Every `(x, y)` with `x, y < KEYS`, the whole set `repeats` times over.
fn repeated(repeats: usize) -> Tuples {
    let mut tuples = Tuples::with_capacity(2, repeats * KEYS * KEYS);
    for _ in 0..repeats {
        for x in 0..KEYS {
            for y in 0..KEYS {
                tuples.push(&[x, y]);
            }
        }
    }
    tuples
}

/// Allocations of one `HashTrie<SipHashStrategy, P, E>` build of
/// `repeated(repeats)` by `mode` under the default config, after checking it
/// stored every row.
fn hash_build_allocations<P: PruningPolicy, E: ExpansionPolicy>(
    mode: HashTrieBuildMode, repeats: usize,
) -> u64 {
    let tuples = repeated(repeats);
    let rows = tuples.len();
    let mut built: Option<HashTrie<SipHashStrategy, P, E>> = None;
    let info = allocation_counter::measure(|| {
        built = Some(HashTrie::from_tuples_with_build_mode(2.into(), mode, tuples));
    });
    assert_eq!(built.expect("measured").tuple_count(), rows);
    info.count_total
}

/// Under the default `root-capacity=grow` every table's size depends on its
/// keys alone (`tuples` would size the root from the row count), so the
/// tables, the chains and the lazy pending lists are the same at both
/// repetitions. Only the row-id lists hold more ids: the root's list, one per
/// first key, one chain per distinct tuple and `radix:2`'s four partitions,
/// each by at most ⌈log₂ 100⌉ = 7 growth steps.
#[test]
fn hash_trie_builds_allocate_per_list_not_per_row() {
    let lists = (1 + KEYS + KEYS * KEYS + 4) as u64;
    let bound = lists * ceil_log2(REPEATS);
    let radix = HashTrieBuildMode::Radix(RadixBits::new(2).unwrap());
    type Measure = fn(HashTrieBuildMode, usize) -> u64;
    let cells: [(&str, Measure, HashTrieBuildMode); 6] = [
        (
            "eager",
            hash_build_allocations::<NoPruning, EagerExpansion>,
            HashTrieBuildMode::Bulk,
        ),
        (
            "pruned",
            hash_build_allocations::<SingletonPruning, EagerExpansion>,
            HashTrieBuildMode::Bulk,
        ),
        (
            "lazy",
            hash_build_allocations::<NoPruning, LazyExpansion>,
            HashTrieBuildMode::Bulk,
        ),
        (
            "pruned lazy",
            hash_build_allocations::<SingletonPruning, LazyExpansion>,
            HashTrieBuildMode::Bulk,
        ),
        (
            "eager",
            hash_build_allocations::<NoPruning, EagerExpansion>,
            HashTrieBuildMode::Incremental,
        ),
        (
            "eager",
            hash_build_allocations::<NoPruning, EagerExpansion>,
            radix,
        ),
    ];
    for (layout, allocations, mode) in cells {
        let (once, many) = (allocations(mode, 1), allocations(mode, REPEATS));
        assert!(
            many <= once + bound,
            "HashTrie {layout} {mode:?}: {once} allocations for {} rows, {many} for {}, more \
             than {bound} apart: some row allocates",
            KEYS * KEYS,
            REPEATS * KEYS * KEYS
        );
    }
}
```

- [ ] **Step 2: Write the binary's hand-over test.** In `kermit/src/execution.rs`
  `mod tests`, after `load_with_tuples_returns_file_order`:

```rust
    /// `load_with_tuples` reads one buffer, keeps it and builds from one copy,
    /// so nothing is allocated per row (#111). ColumnTrie is the probe: its
    /// build owns only its layer arrays. At most five buffers grow with the
    /// rows, the reader's and ColumnTrie's 2 · arity layer arrays, and each
    /// doubles, so ten times the rows costs each at most ⌈log₂ 10⌉ = 4 more
    /// allocations. A per-row allocation adds 9 000.
    #[test]
    fn load_with_tuples_allocates_independently_of_the_row_count() {
        let allocations = |rows: usize| {
            let dir = tempfile::tempdir().expect("tempdir");
            let path = dir.path().join("r.csv");
            let mut csv = String::from("a,b\n");
            for i in 0..rows {
                csv.push_str(&format!("{},{}\n", i / 4, i % 4));
            }
            std::fs::write(&path, csv).expect("write csv");
            let family = SortedTrieFamily::<ColumnTrie>::default();
            // Unmeasured: a one-time initialisation must not count.
            family.load_with_tuples(&path).expect("load");
            let mut loaded = None;
            let info = allocation_counter::measure(|| {
                loaded = Some(family.load_with_tuples(&path).expect("load"));
            });
            let (relation, tuples) = loaded.expect("measured");
            assert_eq!(tuples.len(), rows);
            assert_eq!(SortedTrieFamily::<ColumnTrie>::tuple_count(&relation), rows);
            info.count_total
        };
        let (small, large) = (allocations(1_000), allocations(10_000));
        assert!(
            large <= small + 5 * 4,
            "load_with_tuples: {small} allocations for 1 000 rows, {large} for 10 000"
        );
    }
```

- [ ] **Step 3: Run the tests.**
  ```bash
  CARGO_BUILD_JOBS=2 cargo test -p kermit --test tuple_allocation
  CARGO_BUILD_JOBS=2 cargo test -p kermit --bin kermit load_with_tuples_allocates
  ```
  Expected: 7 passed, then 1 passed. They pass on landing: T4–T8 are the change they
  pin. If a bound fails, the message prints both counts; compare with the expected counts
  in the doc comments (TreeTrie 2 511, ColumnTrie 37, CSV `large − small` = 4).
  The TreeTrie bound is tight: 2 511 against a bound of 2 515, and it rests on
  std growing an empty `Vec` to 4 slots and then doubling. If it fails by a
  margin that std's growth policy explains (a different first capacity or
  growth factor), report the counts and the explanation and stop. Never
  loosen the bound or `SCRATCH` to make it pass: a bound that silently grows
  is the per-tuple allocation this test exists to catch.

- [ ] **Step 4: Commit** (before the mutation checks, so they start from a clean tree).
  ```bash
  nix develop --command cargo fmt --all
  CARGO_BUILD_JOBS=2 cargo clippy --all-targets -- -D warnings
  git add kermit/tests/tuple_allocation.rs kermit/src/execution.rs
  git commit -m "$(cat <<'EOF'
test(kermit): no per-row allocation from file to build (#111)

The readers allocate a number of times independent of their row count
(CSV) or per record batch (Parquet); a batch clone and a reordered copy
are one allocation; TreeTrie and ColumnTrie builds allocate only the
nodes and layer arrays they own; HashTrie builds allocate per list and
chain, not per row; load_with_tuples hands one buffer over.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_015P4eTR34w48GpD7DvPPaee
EOF
)"
  ```

- [ ] **Step 5: Mutation check: a per-row allocation in a reader.** Apply in `kermit-ds/src/relation.rs` `read_csv` the exact edit
  `data.push(value);` → `data.extend(vec![value]);`. Run:
  ```bash
  git diff --stat   # must show kermit-ds/src/relation.rs changed
  CARGO_BUILD_JOBS=2 cargo test -p kermit --test tuple_allocation read_csv
  ```
  Expected: `read_csv_allocates_independently_of_its_row_count` FAILS (about 18 000 more
  allocations). Reverse the exact edit (`data.extend(vec![value]);` → `data.push(value);`),
  never with `git checkout`, re-run to green, and check `git status` is clean.

- [ ] **Step 6: Mutation check: a per-level allocation in TreeTrie.** In
  `kermit-ds/src/ds/tree_trie/implementation.rs` `insert_into_children` (T5's slice
  version), replace the argument `rest` of both recursive calls with
  `&rest.to_vec()`:
  `insert_into_children(children[pos].children_mut(), rest)` →
  `insert_into_children(children[pos].children_mut(), &rest.to_vec())`, and
  `insert_into_children(new_node.children_mut(), rest);` →
  `insert_into_children(new_node.children_mut(), &rest.to_vec());`. Run:
  ```bash
  git diff --stat   # must show the tree_trie implementation changed
  CARGO_BUILD_JOBS=2 cargo test -p kermit --test tuple_allocation tree_trie
  ```
  Expected: `tree_trie_build_allocates_once_per_inner_node` FAILS. Reverse the exact edit,
  re-run to green, `git status` clean.

---

---

### Task 10: Report schema 3 → 4

What changes meaning at v4 (spec § "CLI, reports and kermit-lab"): `insertion` and
`end_to_end` time a different input path and sort (no per-tuple allocations or frees, one
shared `Tuples::sort`), and HashTrie's `space` counts a different representation.
HashTrie's `bench ds` `iteration` still walks the trie (`HashTrie::for_each_tuple`, T7),
now reading rows through row ids: it may move, and the measurement after this plan
records by how much, but it is not a change of meaning. kermit-lab's loader refuses
mixes across one boundary today (`STREAMED_JOIN_SCHEMA = 3`); bumping `SCHEMA_VERSION`
alone would let it load v3 and v4 together, so the loader gains a second boundary.

**Files:**
- Modify: `kermit/src/bench_report.rs` (79–83; test 373)
- Modify: `kermit/tests/cli_join_tests.rs` (557, 642, 724)
- Modify: `python/kermit-lab/kermit_lab/__init__.py` (8)
- Modify: `python/kermit-lab/kermit_lab/loader.py` (24–27, 88–120)
- Modify: `python/kermit-lab/tests/test_loader.py` (177–182; new tests after it),
  `python/kermit-lab/tests/test_cli.py` (41–52)
- Modify: `python/kermit-lab/README.md` (169–174)
- Modify: `docs/specs/bench-report-schema.md` (3, 22, 49, 228–238, change log after 262)
- Modify: `docs/specs/optimization-standard.md` (259, the example report)
- Modify: `CLAUDE.md` (the "JSON bench reports" gotcha, line 302)

- [ ] **Step 1: Write the failing pins.** Rust: in `kermit/src/bench_report.rs` test (373)
  `assert_eq!(json[0]["schema_version"], 3);` → `assert_eq!(json[0]["schema_version"], 4);`,
  and in `kermit/tests/cli_join_tests.rs` the three `["schema_version"], 3)` (557, 642,
  724) → `["schema_version"], 4)`:
  ```bash
  sed -i 's/\["schema_version"\], 3);/["schema_version"], 4);/' kermit/tests/cli_join_tests.rs
  grep -c '\["schema_version"\], 4);' kermit/tests/cli_join_tests.rs   # expect 3
  ```
  Python: in `tests/test_loader.py` replace `test_single_sided_loads_are_unaffected`
  (177–182) and add two tests after it:

```python
def test_single_sided_loads_are_unaffected(tmp_path: Path) -> None:
    for version in (2, 3, 4):
        same = [_versioned_report(tmp_path / f"v{version}_{i}.json", version) for i in range(2)]
        assert len(load_reports(same)) == 2


def test_refuses_to_mix_reports_across_the_flat_tuples_boundary(tmp_path: Path) -> None:
    old = _versioned_report(tmp_path / "v3.json", 3)
    new = _versioned_report(tmp_path / "v4.json", 4)
    with pytest.raises(SchemaError, match="refusing to mix schema_version 3"):
        load_reports([old, new])
    with pytest.raises(SchemaError, match="flat tuple buffer"):
        load_reports([new, old])
    reports = load_reports([old, new], allow_mixed_schema=True)
    assert [r.schema_version for r in reports] == [3, 4]


def test_a_load_straddling_both_boundaries_names_the_first(tmp_path: Path) -> None:
    v2 = _versioned_report(tmp_path / "v2.json", 2)
    v4 = _versioned_report(tmp_path / "v4.json", 4)
    with pytest.raises(SchemaError, match="streamed, counted join"):
        load_reports([v2, v4])
```

  In `tests/test_cli.py` replace `test_mixed_schema_reports_exit_with_code_4` (41–52):

```python
@pytest.mark.parametrize("versions", [(2, 3), (3, 4)])
def test_mixed_schema_reports_exit_with_code_4(tmp_path: Path, versions: tuple[int, int]) -> None:
    paths = []
    for version in versions:
        p = tmp_path / f"v{version}.json"
        p.write_text(json.dumps([{"schema_version": version, "kind": "run",
                                  "metadata": [], "axes": {}, "criterion_groups": []}]))
        paths.append(str(p))
    rc = main(["scaling", *paths, "--criterion-root", str(tmp_path),
               "--out", str(tmp_path / "s.pdf")])
    assert rc == 4
```

- [ ] **Step 2: Run them to verify they fail.**
  ```bash
  CARGO_BUILD_JOBS=2 cargo test -p kermit --bin kermit bench_report
  CARGO_BUILD_JOBS=2 cargo test -p kermit --test cli_join_tests schema
  uv --directory python/kermit-lab sync --group test
  uv --directory python/kermit-lab run pytest tests/test_loader.py tests/test_cli.py -q
  ```
  Expected: the `bench_report` round-trip test and the cli_join report tests fail
  (`left: 3, right: 4`); pytest fails `test_single_sided_loads_are_unaffected` and the two
  new loader tests (`schema_version 4 > supported 3`), and `(3, 4)` of the CLI test.
  (Filter names for cli_join_tests: run the whole file if `schema` matches none.)

- [ ] **Step 3: Bump the Rust constant.** `kermit/src/bench_report.rs` 79–83, after:

```rust
/// Schema version for the JSON report. Bump on any breaking change to
/// [`BenchReport`] field names or value types, or to what a metric
/// measures. 3: `iteration` / `end_to_end` time a streamed join whose rows
/// are counted, never materialised (issue #65). 4: every structure builds
/// from one flat tuple buffer, so `insertion` and `end_to_end` time a
/// different input path and sort, and HashTrie's `space` counts row ids
/// into that buffer instead of a `Vec` per tuple (issue #111).
pub const REPORT_SCHEMA_VERSION: u32 = 4;
```

- [ ] **Step 4: Bump kermit-lab and give its loader the second boundary.**
  `python/kermit-lab/kermit_lab/__init__.py` line 8: `SCHEMA_VERSION = 3` →
  `SCHEMA_VERSION = 4`. In `kermit_lab/loader.py` replace lines 24–27
  (`STREAMED_JOIN_SCHEMA` and its docstring) with:

```python
STREAMED_JOIN_SCHEMA = 3
"""First schema version whose ``iteration`` / ``end_to_end`` phases time a
streamed join with counted, never-materialised rows (issue #65)."""

FLAT_TUPLES_SCHEMA = 4
"""First schema version whose structures build from one flat tuple buffer
(issue #111): ``insertion`` and ``end_to_end`` time a different input path
and sort, and HashTrie's ``space`` counts row ids into the buffer instead
of a ``Vec`` per tuple."""

MEANING_CHANGES: tuple[tuple[int, str, str], ...] = (
    (
        STREAMED_JOIN_SCHEMA,
        "the iteration and end_to_end phases time a streamed, counted join",
        "space",
    ),
    (
        FLAT_TUPLES_SCHEMA,
        "every structure builds from one flat tuple buffer, which changes insertion, "
        "end_to_end and HashTrie's space",
        "bench run iteration",
    ),
)
"""Every schema version from which a metric changed meaning: the version,
what changed, and what stays comparable across it. Reports on either side of
one measure different things, so one load may not mix them."""
```

  Replace `_refuse_mixed_schema` (88–99) with:

```python
def _refuse_mixed_schema(reports: Sequence[BenchReport]) -> None:
    for boundary, change, comparable in MEANING_CHANGES:
        older = next((r for r in reports if r.schema_version < boundary), None)
        newer = next((r for r in reports if r.schema_version >= boundary), None)
        if older is not None and newer is not None:
            raise SchemaError(
                f"refusing to mix schema_version {older.schema_version} ({older.source_path}) "
                f"with schema_version {newer.schema_version} ({newer.source_path}): from "
                f"v{boundary} {change}, so those values are not comparable with earlier "
                "reports. Load each side separately, or (Python API) pass "
                f"allow_mixed_schema=True to compare {comparable} only."
            )
```

  and in `load_reports`' docstring (107–108) replace
  ``Raises :class:`SchemaError` when the reports straddle :data:`STREAMED_JOIN_SCHEMA`,``
  with ``Raises :class:`SchemaError` when the reports straddle a version in :data:`MEANING_CHANGES`,``.
  Nothing else imports `STREAMED_JOIN_SCHEMA` (`grep -rn STREAMED_JOIN_SCHEMA python/kermit-lab --include=*.py`
  shows only `loader.py`).

- [ ] **Step 5: Docs pinned to the version.**

  `python/kermit-lab/README.md` 169–174, before:
```markdown
`kermit-lab` parses `BenchReport` JSON up to v3 (see
`docs/specs/bench-report-schema.md`). The loader refuses to parse unknown
major versions. It also refuses to mix reports from both sides of v3 in one
load, because `iteration` and `end_to_end` changed meaning there; pass
`allow_mixed_schema=True` to `kl.load` / `kl.load_samples` to override, for
example to compare `space` only. Fixed axis columns (`data_structure`, `algorithm`, `query`,
```
  after:
```markdown
`kermit-lab` parses `BenchReport` JSON up to v4 (see
`docs/specs/bench-report-schema.md`). The loader refuses to parse unknown
major versions. It also refuses to mix reports from both sides of v3, or of
v4, in one load: at v3 `iteration` and `end_to_end` changed meaning, and at
v4 `insertion`, `end_to_end` and HashTrie's `space` did (#111). Pass
`allow_mixed_schema=True` to `kl.load` / `kl.load_samples` to override, for
example to compare `space` across v3. Fixed axis columns (`data_structure`, `algorithm`, `query`,
```

  `docs/specs/bench-report-schema.md`: line 3 `**Current schema version:** \`3\`` →
  `` `4` ``; the example's `"schema_version": 3,` (22) → `"schema_version": 4,`; the field
  catalogue's ``Currently `3`.`` (49) → ``Currently `4`.``. Versioning policy (228–238):
  `  v3 is such a bump.` → `  v3 and v4 are such bumps.`, and replace
```markdown
- Consumers should refuse to parse if `schema_version` is missing or
  greater than the highest version they know about. kermit-lab also
  refuses to load reports from both sides of version 3 in one call,
  because the `iteration` and `end_to_end` values changed meaning there.
```
  with
```markdown
- Consumers should refuse to parse if `schema_version` is missing or
  greater than the highest version they know about. kermit-lab also
  refuses to load reports from both sides of version 3, or of version 4,
  in one call: at 3 the `iteration` and `end_to_end` values changed
  meaning, and at 4 `insertion`, `end_to_end` and HashTrie's `space` did.
```
  Append to the change log table, after the last `| 3 (no bump) | 2026-10-07 |` row:
```markdown
| 4       | 2026-10-08 | What `insertion`, `end_to_end` and HashTrie's `space` measure changed, so they are not comparable with v3, and kermit-lab refuses to load v3 and v4 reports together unless `allow_mixed_schema=True` (#111). Every relation's tuples travel from the reader to every structure's build as one row-major buffer (`Tuples`) instead of a `Vec` per tuple: each `insertion` / `end_to_end` setup clone copies one buffer; TreeTrie and ColumnTrie sort it with one shared `Tuples::sort` and free one buffer where they freed a `Vec` per tuple; and HashTrie keeps the buffer, so its `space` counts the buffer (capacity × 8 B), its row-id lists and chains (capacity × 4 B) and its tables, where it counted a `Vec` per tuple. HashTrie's `bench ds` `iteration` still walks the trie, now reading each row through its id, so it may move; #111's measurement records by how much. TreeTrie and ColumnTrie `space` and `bench run` / `bench join` `iteration` keep their meaning (the join's leaf product reads HashTrie chains through `LeafRows`; compare `iteration` within the codegen bound). No field or axis changed. |
```

  `docs/specs/optimization-standard.md` line 259, in "What the bench report looks
  like": `    "schema_version": 3,` → `    "schema_version": 4,`
  (`grep -n '"schema_version": 3' docs/specs/optimization-standard.md` must then
  print nothing).

  `CLAUDE.md`, "JSON bench reports" gotcha (302), three edits:
  - `(v3 is such a bump)` → `(v3 and v4 are such bumps)`;
  - ``Schema is versioned via `schema_version` (currently `3`)`` → ``(currently `4`)``;
  - replace ``kermit-lab refuses to load reports from both sides of v3 in one call (`allow_mixed_schema=True` overrides), because from v3 `iteration` / `end_to_end` time a streamed, counted join.``
    with ``kermit-lab refuses to load reports from both sides of v3, or of v4, in one call (`allow_mixed_schema=True` overrides), because from v3 `iteration` / `end_to_end` time a streamed, counted join, and from v4 every structure builds from one flat tuple buffer, which changes `insertion`, `end_to_end` and HashTrie's `space` (#111).``

- [ ] **Step 6: Run.**
  ```bash
  CARGO_BUILD_JOBS=2 cargo test -p kermit --bin kermit bench_report
  CARGO_BUILD_JOBS=2 cargo test -p kermit --test cli_join_tests
  uv --directory python/kermit-lab run pytest -q
  ```
  Expected: green, including `python_schema_version_matches_rust` (it reads
  `__init__.py`'s `SCHEMA_VERSION = 4` and compares with `REPORT_SCHEMA_VERSION`), the
  `(2, 3)` and `(3, 4)` CLI cases, and the existing v2/v3 loader and frame tests, whose
  messages still match (`refusing to mix schema_version 2`, `old.json`). For the
  real-binary contract test, also run it with the built binary:
  `KERMIT_BIN=$PWD/target/debug/kermit uv --directory python/kermit-lab run pytest tests/test_contract.py -q`.

- [ ] **Step 7: Commit.**
  ```bash
  git add kermit/src/bench_report.rs kermit/tests/cli_join_tests.rs \
    python/kermit-lab/kermit_lab/__init__.py python/kermit-lab/kermit_lab/loader.py \
    python/kermit-lab/tests/test_loader.py python/kermit-lab/tests/test_cli.py \
    python/kermit-lab/README.md docs/specs/bench-report-schema.md \
    docs/specs/optimization-standard.md CLAUDE.md
  git commit -m "$(cat <<'EOF'
feat(report,lab): schema 4; kermit-lab refuses to mix v3 and v4 (#111)

insertion and end_to_end now build from one flat tuple buffer and
HashTrie's space counts row ids into it, so those values are not
comparable with v3. The loader keeps a table of
meaning changes and refuses a load that straddles any of them unless
allow_mixed_schema=True; the CLI still exits 4.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_015P4eTR34w48GpD7DvPPaee
EOF
)"
  ```

---

### Task 11: Documentation

The spec's Documentation list, minus what T10 already did (the
`bench-report-schema.md` v4 entry, `optimization-standard.md`'s example and
CLAUDE.md's JSON-report gotcha): do not edit those again. Each edit below is
an exact before/after. Where a line is long, "before" is the exact substring
to replace; check each is unique with `grep -cF '<before>' <file>` (expect
`1`) before editing. Line numbers are at `47950f0`; no earlier task edits
these files except CLAUDE.md line 302 (T10).

The docs keep the parity wording the HashTrie direction requires (#105): the
buffer and leaves that point into it are the paper's (§3.3.2); the 4-byte
ids and the `Vec<RowId>` per chain are kermit's, the second until #101
layer 3.

**Files:**
- Modify: `ARCHITECTURE.md` (18, 135, 139, after 143, 155–157, 210–220, 226)
- Modify: `CLAUDE.md` (80, 139, 140, after 140, 151)
- Modify: `docs/algorithms/hash-triejoin.md` (49)
- Modify: `docs/data-structures/hash-trie.md` (7, 10–27, 39, 41, after 41, 45, 64, 98, 110–112, 118–146, 190, 217, 229, 241–244, 456–461, 484–485, 496)
- Modify: `docs/data-structures/tree-trie.md` (42)
- Modify: `docs/data-structures/column-trie.md` (35, 65)
- Modify: `docs/data-structures/parallel-build.md` (31, 44–59, 104, 130–135, 162–163, 175–178, 199–200, 269–277)
- Modify: `docs/specs/2026-10-06-column-trie-parallel-build-design.md` (after the decision table, line 39)
- Modify: `BENCHMARKING.md` (after 82)

- [ ] **Step 1: `ARCHITECTURE.md`.**

  Line 18, before:
  ```text
  ├── kermit-iters/    # Core iterator traits (no dependencies)
  ```
  After:
  ```text
  ├── kermit-iters/    # Core iterator traits and the tuple batch (no dependencies)
  ```

  Line 135, before:
  ```rust
      fn leaf_tuples(&self) -> Option<&[Vec<usize>]>;
  ```
  After:
  ```rust
      fn leaf_tuples(&self) -> Option<LeafRows<'_>>; // the chain: row ids over the tuple buffer
  ```

  Line 139, before (substring):
  ```text
  `HashTrie` exposes `collect_tuples()` for materialisation instead, and hashing means a descent can produce false positives, so the join must verify shared variables against the real tuples at the leaf.
  ```
  After:
  ```text
  `HashTrie` exposes `collect_tuples()` for materialisation instead (a copy of its tuple buffer), and hashing means a descent can produce false positives, so the join must verify shared variables against the real tuples at the leaf, which `leaf_tuples` lends as a [`LeafRows`](#tuple-batches-tuples-and-leafrows) view.
  ```

  After line 143 (the `TrieIteratorWrapper` paragraph) and before
  `### Data Structures (`kermit-ds`)`, insert:
  ```markdown

  #### Tuple batches: `Tuples` and `LeafRows`

  A relation's tuples travel from the file readers to every structure's build as one `Tuples` batch (`kermit-iters/src/tuples.rs`, #111): tuples of one arity, row-major in one `Vec<usize>`, with an explicit row count, so a batch of nullary tuples keeps its count. Each row is addressed by a `RowId` (`u32`), so a batch holds at most `u32::MAX` rows. `read_csv` and `read_parquet` return one at its exact size; every constructor takes `impl Into<Tuples>` (test fixtures pass one `Vec<usize>` per tuple, which converts); and every setup clone in `bench ds` and `bench run` copies one buffer. TreeTrie and ColumnTrie sort the batch in place with `Tuples::sort`, one sort for both, and build from row slices. HashTrie keeps the batch and stores row ids.

  `LeafRows<'a>` (`kermit-iters/src/leaf_rows.rs`) is a `Copy` view of one hash-trie leaf chain: its row ids, in chain order, over the buffer they index. `HashTrieIterator::leaf_tuples` returns it. The rows it lends borrow the buffer, not the view, so HashTriejoin's leaf product reads its candidates without copying or allocating.
  ```

  Lines 155–157, before:
  ```rust
      fn from_tuples(header: RelationHeader, tuples: Vec<Vec<usize>>) -> Self;
      fn insert(&mut self, tuple: Vec<usize>);
      fn insert_all(&mut self, tuples: Vec<Vec<usize>>);
  ```
  After:
  ```rust
      fn from_tuples(header: RelationHeader, tuples: impl Into<Tuples>) -> Self;
      fn insert(&mut self, tuple: impl AsRef<[usize]>);
      fn insert_all(&mut self, tuples: impl Into<Tuples>);
  ```

  Lines 210–220, before:
  ```rust
  struct HashTrie<H: HashStrategy = SipHashStrategy> {
      header: RelationHeader,
      root: HashTrieNode,
      tuple_count: usize,     // multiset: duplicates count
      _hasher: PhantomData<H>,
  }

  enum HashTrieNode {
      Inner(HashTable<HashTrieNode>),    // child nodes
      Leaf(HashTable<Vec<Vec<usize>>>),  // full materialised tuple chains
  }
  ```
  After:
  ```rust
  struct HashTrie<H: HashStrategy = SipHashStrategy> {
      header: RelationHeader,
      tuples: Tuples,         // every stored tuple, arrival order; len() is the multiset count
      root: HashTrieNode,
      _hasher: PhantomData<H>,
  }

  enum HashTrieNode {
      Inner(HashTable<HashTrieNode>),    // child nodes
      Leaf(HashTable<Vec<RowId>>),       // chains: ids of rows in `tuples`
  }
  ```

  Line 226, before:
  ```text
  - **Leaves hold whole tuples.** Because inner levels store only hashes, the real values are needed at the leaf to reject hash collisions.
  ```
  After:
  ```text
  - **Leaves reach whole tuples.** Because inner levels store only hashes, the real values are needed at the leaf to reject hash collisions. The trie keeps its relation's `Tuples` batch, and a chain holds the row ids of its tuples, read through `LeafRows` (#111). The ids are 4 bytes where the paper's tuple pointers are 8.
  ```

- [ ] **Step 2: `CLAUDE.md`.** Do not touch the "JSON bench reports"
  gotcha (T10 did).

  Line 80 (Workspace Architecture), before:
  ```text
  kermit-iters    → Core iterator traits (LinearIterator, TrieIterator). Zero dependencies.
  ```
  After:
  ```text
  kermit-iters    → Core iterator traits (LinearIterator, TrieIterator) and the tuple batch
                    (Tuples, RowId, LeafRows; #111). Zero dependencies.
  ```

  Line 139, before (substring):
  ```text
  (`u64` hash keys, exact-match `lookup`, plus `size`, `open`, `up`, `leaf_tuples`)
  ```
  After:
  ```text
  (`u64` hash keys, exact-match `lookup`, plus `size`, `open`, `up`, and `leaf_tuples`, which lends the current leaf chain as a `LeafRows` view)
  ```

  Line 140, before:
  ```text
  - **Relation**: JoinIterable + Projectable — core data abstraction (`new`, `from_tuples`, `insert`, `insert_all`, `header`)
  ```
  After (two bullets; the second is new):
  ```text
  - **Relation**: JoinIterable + Projectable — core data abstraction (`new`, `from_tuples(header, impl Into<Tuples>)`, `insert(impl AsRef<[usize]>)`, `insert_all(impl Into<Tuples>)`, `header`). The Config and BuildMode constructors below take `impl Into<Tuples>` too, so test fixtures still pass one `Vec<usize>` per tuple (`From<Vec<Vec<usize>>>`, which panics on mixed arity); an empty batch is accepted under any header.
  - **Tuples** / **RowId** / **LeafRows** (`kermit-iters/src/{tuples,leaf_rows}.rs`, re-exported by `kermit-ds`; #111): a relation's tuples as one row-major buffer of one arity, with an explicit row count (a nullary batch keeps its count) and at most `u32::MAX` rows, each addressed by a 4-byte `RowId`. `read_csv` / `read_parquet` return one at its exact size; `Tuples::sort` is the one sort of both sorted tries; `HashTrie` keeps its batch and holds row ids in its chains, singletons and pending lists. `LeafRows<'a>` is the `Copy` view of one chain (ids over the buffer) that `HashTrieIterator::leaf_tuples` returns; its rows borrow the buffer, so the leaf product allocates nothing. `to_vecs()` (both) is for tests and diagnostics.
  ```

  Line 151, before:
  ```text
  - **TupleScan**: lend every stored tuple without probing (`scan_tuples`); implemented by `HashTrie` and `Configured`. The hash family's statistics walk uses it because a lazy `HashTrie`'s probes build children (#92).
  ```
  After:
  ```text
  - **TupleScan**: lend every stored tuple without probing (`scan_tuples`); implemented by `HashTrie` and `Configured`. The hash family's statistics walk uses it because a lazy `HashTrie`'s probes build children (#92). `HashTrie` scans its tuple buffer in arrival order (#111); `HashTrie::for_each_tuple`, which `bench ds` times (#79), still walks the trie, reading rows through their ids.
  ```

- [ ] **Step 3: `docs/algorithms/hash-triejoin.md`.**

  Line 49, before (substring):
  ```text
  `emit_leaf` re-borrows each leaf chain through `leaf_tuples()` instead of collecting it, and the cross-product cursor
  ```
  After:
  ```text
  `emit_leaf` re-borrows each leaf chain through `leaf_tuples()` instead of collecting it, as a `LeafRows` view (the chain's row ids over its relation's tuple buffer, #111) whose `row(i)` is a slice of that buffer, and the cross-product cursor
  ```

- [ ] **Step 4: `docs/data-structures/hash-trie.md`, Representation.**

  Line 7, before (substring):
  ```text
  whose values are either child nodes (inner levels) or tuple chains (leaf level).
  ```
  After:
  ```text
  whose values are either child nodes (inner levels) or tuple chains (leaf level). A chain holds the row ids of its tuples; the tuples themselves live once, in the trie's buffer.
  ```

  Lines 10–27, before:
  ```rust
  HashTrie<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy> {
      header:      RelationHeader,
      root:        HashTrieNode<P, E>,
      tuple_count: usize,             // multiset size, for Cardinality
      config:      HashTrieConfig,
      _layout:     PhantomData<(H, P, E)>,
  }

  enum HashTrieNode<P: PruningPolicy, E: ExpansionPolicy> {
      Inner(HashTable<HashTrieNode<P, E>>),     // depths 0..arity-1
      Leaf(HashTable<Vec<Vec<usize>>>),         // depth arity-1
      Singleton(P::Payload),                    // pruned subtrie, depths 1..arity
      Unexpanded(E::Pending<HashTrieNode<P, E>>), // lazy child, depths 1..arity
  }

  struct LazyChild<N> {                         // E::Pending<N> under LazyExpansion (boxed)
      pending: RefCell<Vec<Vec<usize>>>,        // tuples below the bucket, insertion order
      built:   OnceCell<N>,                     // the table, once a probe reached it
  }
  ```
  After:
  ```rust
  HashTrie<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy> {
      header:      RelationHeader,
      tuples:      Tuples,            // every stored tuple, arrival order; len() = multiset size
      root:        HashTrieNode<P, E>,
      config:      HashTrieConfig,
      _layout:     PhantomData<(H, P, E)>,
  }

  enum HashTrieNode<P: PruningPolicy, E: ExpansionPolicy> {
      Inner(HashTable<HashTrieNode<P, E>>),     // depths 0..arity-1
      Leaf(HashTable<Vec<RowId>>),              // depth arity-1: chains of row ids
      Singleton(P::Payload),                    // pruned subtrie, depths 1..arity
      Unexpanded(E::Pending<HashTrieNode<P, E>>), // lazy child, depths 1..arity
  }

  struct LazyChild<N> {                         // E::Pending<N> under LazyExpansion (boxed)
      pending: RefCell<Vec<RowId>>,             // row ids below the bucket, insertion order
      built:   OnceCell<N>,                     // the table, once a probe reached it
  }
  ```

  Line 39, before (substring):
  ```text
  The `Singleton` payload is the second Layout parameter's associated type: `Vec<usize>` under `SingletonPruning`,
  ```
  After:
  ```text
  The `Singleton` payload is the second Layout parameter's associated type: the `RowId` of its one tuple under `SingletonPruning`,
  ```

  Line 41, before (substring):
  ```text
  every child below it keeps its tuples in `pending` until `HashTrieIter::open` first enters it, and `HashTrie::resolve` then moves them into a table built by Algorithm 2's `build` (`bulk.rs`), one level deep (its own children start unexpanded). `collect_tuples`, `for_each_tuple` and `heap_size_bytes` read `built` if present and `pending` otherwise, and never expand.
  ```
  After:
  ```text
  every child below it keeps its tuples' row ids in `pending` until `HashTrieIter::open` first enters it, and `HashTrie::resolve` then moves them into a table built by Algorithm 2's `build` (`bulk.rs`), one level deep (its own children start unexpanded). `for_each_tuple` and `heap_size_bytes` read `built` if present and `pending` otherwise; `collect_tuples` and `scan_tuples` read the buffer; none of them expands anything.
  ```

  After line 41, insert a new paragraph and table:
  ```markdown

  **The tuple buffer** (#111). The trie owns its relation's tuples as one row-major `Tuples` batch, in arrival order: the batch a build is given, kept without a copy, then every row `insert` appends. Below the tables every tuple is named by its `RowId` (`u32`, an index into the buffer): a leaf chain is a `Vec<RowId>` in input order, a pruned `Singleton` one `RowId`, a lazy pending list a `Vec<RowId>`. `HashTrieIterator::leaf_tuples` lends a chain as a `LeafRows` view over the buffer. Two builds of one input own equal buffers, so the BuildMode identity tests compare ids, after checking the buffers are equal. Reading back: `for_each_tuple` walks the trie depth-first, reading each row through its id (`bench ds` times it, #79); `TupleScan::scan_tuples` (statistics, which need only the multiset) scans the buffer in arrival order; `collect_tuples` copies the buffer.

  | Element (VLDB 2020 §3.3.1–3.3.2, Fig. 3) | kermit since #111 |
  | --- | --- |
  | Tuples in one contiguous buffer, fixed-length layout | ✓ the relation's buffer, row-major |
  | Leaves, singletons and lazy children refer to tuple memory | ✓ by row id (4 B, kermit's; the paper's pointers are 8 B) |
  | A leaf is a list threaded through an 8-byte chain pointer per tuple | ✗ a `Vec<RowId>` per chain (kermit's), until #101 layer 3 |
  | Tuples partitioned by the first key's hash as they are materialised | ✗ arrival order, until #101 layer 3 |

  The paper's buffer is a copy its build makes. HashTrie takes the relation's buffer without copying it, so the build pays nothing for it; #101 layer 3 adds the partitioned copy, and with it the paper's cost.
  ```

  Line 45, before (substring):
  ```text
  `SingletonFrameOn { tuple, hash, exhausted }` under `SingletonPruning`, emulating the one-entry table a pruned level would have held and hashing the tuple's attribute for that level once when the frame is pushed
  ```
  After:
  ```text
  `SingletonFrameOn { row, hash, exhausted }` under `SingletonPruning`, emulating the one-entry table a pruned level would have held and hashing the attribute of its row (read from the buffer through the row id) for that level once when the frame is pushed
  ```

- [ ] **Step 5: `hash-trie.md`, Construction and Complexity.**

  Line 64, before:
  ```text
  | the lists themselves | a `Vec` per bucket (kermit's); Umbra threads them through an 8-byte chain pointer in each tuple (§3.3.2), which needs the flat tuple buffer of #111 (#101 layer 3) |
  ```
  After:
  ```text
  | the lists themselves | a `Vec<RowId>` per bucket: 4-byte ids of rows in the trie's tuple buffer (#111; kermit's). Umbra threads them through an 8-byte chain pointer in each materialised tuple (§3.3.2), which needs the partitioned copy of the buffer (#101 layer 3) |
  ```

  Line 98, before (substring):
  ```text
  Algorithm 2, the default `bulk` build: n hashes and moves per level, a list allocation per inner bucket
  ```
  After:
  ```text
  Algorithm 2, the default `bulk` build: n hashes and row-id pushes per level (the tuples stay in the buffer), a list allocation of 4-byte ids per inner bucket
  ```

  Lines 110–112, before:
  ```text
  | `HashTrieIterator::leaf_tuples()` | O(1) | | slice of the current bucket's tuple chain |
  | `HeapSize::heap_size_bytes()` | O(node count) | | walks the trie recursively summing `HashTable` shell + tuple-chain bytes |
  | `for_each_tuple(visit)` | O(n) | O(a) stack | depth-first walk lending each stored tuple from its leaf chain or pruned `Singleton`, in `collect_tuples()` order; allocates nothing per tuple, so `bench ds` times it (issue #79). `collect_tuples()` is the same walk, cloning each tuple into a `Vec` |
  ```
  After:
  ```text
  | `HashTrieIterator::leaf_tuples()` | O(1) | | a `LeafRows` view: the current bucket's chain of row ids over the trie's buffer; copies nothing |
  | `HeapSize::heap_size_bytes()` | O(node count) | | the buffer (capacity × 8 B), plus a recursive walk summing each `HashTable` shell and each id list (chains and pending lists, capacity × 4 B). A tuple is counted once, in the buffer; at arity 2 that is 16 B plus a 4-byte id, where a `Vec` per tuple cost 40 B before #111 |
  | `for_each_tuple(visit)` | O(n) | O(a) stack | depth-first walk of the trie lending each stored tuple, read from the buffer through its chain or pruned `Singleton` id; allocates nothing per tuple, so `bench ds` times it (issue #79). `collect_tuples()` copies the buffer, in arrival order, and `TupleScan::scan_tuples` scans it in place: neither walks the trie |
  ```

- [ ] **Step 6: `hash-trie.md`, the worked micro-example** (lines 118–146).

  Line 118, before:
  ```text
  Tuples `{(1, 2), (1, 3), (2, 4)}` build (for some specific hash values — the actual hashes depend on the platform's `DefaultHasher`):
  ```
  After:
  ```text
  Tuples `(1, 2), (1, 3), (2, 4)`, arriving in that order as rows 0, 1 and 2 of the trie's buffer, build (for some specific hash values — the actual hashes depend on the platform's `DefaultHasher`):
  ```

  Lines 121–132, before:
  ```text
  HashTrie {
    arity: 2,
    root: Inner(HashTable {
      bucket[i₁]: Some(Entry { hash: h(1), value: Leaf(HashTable {
        bucket[j₁]: Some(Entry { hash: h(2), value: [[1, 2]] }),
        bucket[j₂]: Some(Entry { hash: h(3), value: [[1, 3]] }),
      })}),
      bucket[i₂]: Some(Entry { hash: h(2), value: Leaf(HashTable {
        bucket[j₃]: Some(Entry { hash: h(4), value: [[2, 4]] }),
      })}),
    })
  }
  ```
  After:
  ```text
  HashTrie {
    arity: 2,
    tuples: [1, 2, 1, 3, 2, 4],          // rows 0, 1, 2, row-major
    root: Inner(HashTable {
      bucket[i₁]: Some(Entry { hash: h(1), value: Leaf(HashTable {
        bucket[j₁]: Some(Entry { hash: h(2), value: [0] }),   // row 0 = (1, 2)
        bucket[j₂]: Some(Entry { hash: h(3), value: [1] }),   // row 1 = (1, 3)
      })}),
      bucket[i₂]: Some(Entry { hash: h(2), value: Leaf(HashTable {
        bucket[j₃]: Some(Entry { hash: h(4), value: [2] }),   // row 2 = (2, 4)
      })}),
    })
  }
  ```

  Line 135, before (substring):
  ```text
  collapses to `Singleton([2, 4])`
  ```
  After:
  ```text
  collapses to `Singleton(2)`, the row id of `(2, 4)`
  ```

  Line 137, before (substring):
  ```text
  bucket `i₁` holds `Unexpanded { pending: [[1, 2], [1, 3]] }` and bucket `i₂` holds `Unexpanded { pending: [[2, 4]] }`. Step 2 below builds the first child's `Leaf` table from its two tuples
  ```
  After:
  ```text
  bucket `i₁` holds `Unexpanded { pending: [0, 1] }` and bucket `i₂` holds `Unexpanded { pending: [2] }`. Step 2 below builds the first child's `Leaf` table from its two rows
  ```

  Lines 142, 143 and 146, three substrings, before / after each:
  ```text
  `leaf_tuples()` returns `&[[1, 2]]`.
  ```
  ```text
  `leaf_tuples()` returns a `LeafRows` over ids `[0]`, whose one row is `(1, 2)`.
  ```
  ```text
  `leaf_tuples()` returns `&[[1, 3]]`.
  ```
  ```text
  `leaf_tuples()` returns a `LeafRows` over ids `[1]`, whose one row is `(1, 3)`.
  ```
  ```text
  `leaf_tuples()` returns `&[[2, 4]]`. (Under `SingletonPruning`: a `Singleton` frame at depth 1 over `[2, 4]`, with the same `key()` and `leaf_tuples()`.)
  ```
  ```text
  `leaf_tuples()` returns a `LeafRows` over ids `[2]`, whose one row is `(2, 4)`. (Under `SingletonPruning`: a `Singleton` frame at depth 1 over row 2, with the same `key()` and `leaf_tuples()`.)
  ```

- [ ] **Step 7: `hash-trie.md`, Layout options and build modes.**

  Line 190, before (substring):
  ```text
  `P::Payload` is what `HashTrieNode::Singleton` holds: `Vec<usize>` when
  ```
  After:
  ```text
  `P::Payload` is what `HashTrieNode::Singleton` holds: a `RowId` when
  ```

  Line 217, before (substring):
  ```text
  table at construction. Every child below it keeps its tuples as a list
  ```
  After:
  ```text
  table at construction. Every child below it keeps its tuples' row ids as a list
  ```

  Line 229, before (substring):
  ```text
      - under `lazy`, a `Box<LazyChild>` with the pending tuples in a
  ```
  After:
  ```text
      - under `lazy`, a `Box<LazyChild>` with the pending row ids in a
  ```

  Lines 241–244, before:
  ```text
      `collect_tuples`, `for_each_tuple`, `heap_size_bytes`, `project` and the
      Parquet round-trip read the pending list and expand nothing. A
      `for_each_tuple` visitor that opens an iterator on the same trie and
      reaches a child being visited panics (`BorrowMutError`).
  ```
  After:
  ```text
      `for_each_tuple` and `heap_size_bytes` read the pending list;
      `collect_tuples`, `scan_tuples`, `project` and the Parquet round-trip
      read the buffer; none of them expands anything. A `for_each_tuple`
      visitor that opens an iterator on the same trie and reaches a child
      being visited panics (`BorrowMutError`).
  ```

  Lines 456–461, before:
  ```text
  1. **Partition.** A histogram pass, then a stable scatter pass, puts every
     tuple in one of 2^K partitions, together with its input index.
  2. **Scratch roots.** Each non-empty partition's tuples are grouped into a
     scratch root of the real root's kind (Algorithm 2, lines 4–7), recording
     the input index of the tuple that introduced each key; then each key's
     child is built from its list, as the bulk build builds it.
  ```
  After:
  ```text
  1. **Partition.** A histogram pass, then a stable scatter pass, puts every
     row's id in one of 2^K partitions. A row's id is its input index, so
     nothing else travels with it, and the rows stay in the buffer.
  2. **Scratch roots.** Each non-empty partition's ids are grouped into a
     scratch root of the real root's kind (Algorithm 2, lines 4–7), recording
     the id of the row that introduced each key; then each key's child is
     built from its list, as the bulk build builds it.
  ```

  Lines 484–485, before:
  ```text
  - transient memory: 2 bytes per tuple for its partition number, 32 bytes
    per tuple for the partitioned `(index, tuple)` pairs, and the scratch
  ```
  After:
  ```text
  - transient memory: 2 bytes per tuple for its partition number, 4 bytes
    per tuple for its row id in the partitioned lists (32 bytes of
    `(index, tuple)` pair before #111), and the scratch
  ```

  Line 496, before (substring):
  ```text
  step through `morsel::scatter` and its build step through
  ```
  After:
  ```text
  step through `morsel::scatter_rows` and its build step through
  ```

- [ ] **Step 8: `tree-trie.md` and `column-trie.md`.** (`tree-trie.md:25`,
  "one allocation per node", stays true: the build no longer allocates per
  tuple, and the nodes are unchanged.)

  `docs/data-structures/tree-trie.md` line 42, before:
  ```text
  | `from_tuples(n)` | O(n · a · log n) | O(n · a) | sort lexicographically, then insert |
  ```
  After:
  ```text
  | `from_tuples(n)` | O(n · a · log n) | O(n · a) | sort the batch in place with `Tuples::sort` (lexicographic; one sort for both sorted tries, #111), then insert each row as a slice: no allocation per tuple or per level, and the batch is freed once |
  ```

  `docs/data-structures/column-trie.md` line 35, before (substring):
  ```text
  `from_tuples` sorts the tuples lexicographically, then builds every layer in one pass (`ColumnTrie::from_sorted`).
  ```
  After:
  ```text
  `from_tuples` sorts the batch's rows in place with `Tuples::sort` (lexicographic; the sort TreeTrie's builds run too, so a comparison costs the same in both, #111), then builds every layer in one pass over the row slices (`ColumnTrie::from_sorted`), remembering the previous row as a slice borrowed from the batch.
  ```

  Line 65, before (substring):
  ```text
  sort lexicographically (O(n · a · log n)), then build every layer in one pass (O(n · a));
  ```
  After:
  ```text
  sort the batch in place with `Tuples::sort` (O(n · a · log n)), then build every layer in one pass over its row slices (O(n · a), no allocation per tuple);
  ```

- [ ] **Step 9: `parallel-build.md` (the row-id scatter).**

  Line 31, before:
  ```text
  The shared steps live in `kermit-ds/src/morsel.rs` (`scatter`, `dispatch`).
  ```
  After:
  ```text
  The shared steps live in `kermit-ds/src/morsel.rs` (`scatter_rows`, `dispatch`).
  `scatter_rows` sends the 4-byte row ids of a `Tuples` batch to partitions
  (`RowPartition`, its ids in input order); the batch itself never moves (#111).
  ```

  Lines 46–49, before:
  ```text
      partitions = scatter(tuples, morsels of 16 384)  // step 1, N workers
          // tuple t goes to partition_point(splitters, s <= t[0])
      built = dispatch(partitions):                    // step 2, N workers
          sort the partition; insert its tuples one at a time
  ```
  After:
  ```text
      partitions = scatter_rows(tuples, morsels of 16 384)  // step 1, N workers
          // row t's id goes to partition_point(splitters, s <= t[0])
      built = dispatch(partitions):                    // step 2, N workers
          gather the partition's rows into a local Tuples (ids ascend);
          sort it (Tuples::sort); insert its rows one at a time
  ```

  Lines 56–59, before:
  ```text
  - **Partition.** Workers take morsels from a mutex-guarded queue and move
    each tuple, without copying it, into its partition's bucket for that
    morsel. A partition is its buckets in morsel order, so it lists its tuples
    in input order.
  ```
  After:
  ```text
  - **Partition.** Workers take morsels from a mutex-guarded queue and push
    each row's id into its partition's bucket for that morsel; the rows stay
    in the batch. A partition is its buckets in morsel order, so it lists its
    ids in input order, ascending. Each build worker then gathers its
    partition's rows into a buffer of its own, reading mostly forward, and
    frees it when done; the calling thread frees the input batch once.
  ```

  Line 104, before:
  ```text
  | Build | O(n · a · log n): sorting and inserting, split across partitions | N workers |
  ```
  After:
  ```text
  | Build | O(n · a · log n): gathering, sorting and inserting, split across partitions | N workers |
  ```

  Lines 130–135, before:
  ```text
  | Partition | First keys | Tuples (input order) | After sorting and inserting |
  |---|---|---|---|
  | 0 | `< 1` | — | — |
  | 1 | `1` | `[1,2] [1,1]` | `1 → {1, 2}` |
  | 2 | `2` | `[2,9]` | `2 → {9}` |
  | 3 | `≥ 3` | `[3,1]` | `3 → {1}` |
  ```
  After:
  ```text
  | Partition | First keys | Row ids → rows (input order) | After gathering, sorting and inserting |
  |---|---|---|---|
  | 0 | `< 1` | — | — |
  | 1 | `1` | 1 → `[1,2]`, 3 → `[1,1]` | `1 → {1, 2}` |
  | 2 | `2` | 2 → `[2,9]` | `2 → {9}` |
  | 3 | `≥ 3` | 0 → `[3,1]` | `3 → {1}` |
  ```

  Lines 162–163, before:
  ```text
      partitions = scatter(tuples, morsels of 16 384)   // step 1, N workers
          // tuple t goes to partition H(t[0]) >> (64 − b), with its position
  ```
  After:
  ```text
      partitions = scatter_rows(tuples, morsels of 16 384)   // step 1, N workers
          // row t's id (its position) goes to partition H(t[0]) >> (64 − b)
  ```

  Lines 175–178, before:
  ```text
  - **Partition.** As for TreeTrie, but by the top b bits of the first
    attribute's hash (the radix build's rule; FxHash mixes its low bits
    poorly), so there are no splitters to sample, and each tuple keeps its
    input position.
  ```
  After:
  ```text
  - **Partition.** As for TreeTrie, but by the top b bits of the first
    attribute's hash (the radix build's rule; FxHash mixes its low bits
    poorly), so there are no splitters to sample. A row's id is its input
    position, so the partitions carry nothing else.
  ```

  Lines 199–200, before:
  ```text
  - `scatter` keeps each partition in input order, so each root key's subtrie
    is built from the same list, the same tuples in the same order. That
  ```
  After:
  ```text
  - `scatter_rows` keeps each partition in input order, so each root key's
    subtrie is built from the same list, the same row ids in the same order. That
  ```

  Lines 269–277, before:
  ```text
      partitions = scatter(tuples, morsels of 16 384)        // step 1, N workers
          // tuple t goes to partition home_bucket(H(t[0]), p) >> (p − log₂ P),
          // with its position: partition k is run k of the root
      root.with_runs(P, REGION_BUCKETS, |runs|               // step 2, N workers
          dispatch over (partition k, run k):
              for each tuple, in input order:
                  push_in_run(run k, tuple)        lines 6–7, onto its bucket's list
                      off the region's end → defer
      tail: push each deferred tuple, in input order, by ordinary probing
  ```
  After:
  ```text
      partitions = scatter_rows(tuples, morsels of 16 384)   // step 1, N workers
          // row t's id goes to partition home_bucket(H(t[0]), p) >> (p − log₂ P):
          // partition k is run k of the root
      root.with_runs(P, REGION_BUCKETS, |runs|               // step 2, N workers
          dispatch over (partition k, run k):
              for each row id, in input order:
                  push_in_run(run k, id)           lines 6–7, onto its bucket's list
                      off the region's end → defer
      tail: push each deferred id, in input (id) order, by ordinary probing
  ```

- [ ] **Step 10: The parked ColumnTrie parallel-build spec.** In
  `docs/specs/2026-10-06-column-trie-parallel-build-design.md`, after the
  decision table (its last row, `| Tracking | Issue #103, … |`, line 39),
  insert:
  ```markdown

  > **2026-10-08 (#111):** `morsel::scatter` no longer exists. `morsel::scatter_rows` replaced it: it sends a `Tuples` batch's row ids to partitions (`RowPartition`, input order), and each worker gathers its partition's rows into a local `Tuples` and sorts it with `Tuples::sort`, as TreeTrie's `parallel:N` does. Read "Shared steps" above and the pseudocode below with that substitution.
  ```

- [ ] **Step 11: `BENCHMARKING.md`.** After line 82 (the paragraph ending
  "`kermit-lab` refuses to load both kinds together unless you pass
  `allow_mixed_schema=True`."), insert:
  ```markdown

  Reports with `schema_version` 3 and 4 are not comparable in `insertion`,
  `end_to_end` or HashTrie's `space`. From v4 (#111) every relation's tuples
  reach every build as one row-major buffer (`Tuples`) instead of a `Vec` per
  tuple: each setup clone copies one buffer, TreeTrie and ColumnTrie sort it
  with one shared `Tuples::sort` and free one buffer where they freed a `Vec`
  per tuple, and HashTrie keeps the buffer and stores 4-byte row ids where it
  stored a `Vec` per tuple. TreeTrie and ColumnTrie `space` keep their meaning,
  and so does every `iteration`; HashTrie's `iteration` (the join's leaf
  product, and `bench ds`'s trie walk) now reads rows through row ids, so
  compare it across the boundary only through a measured A/B. The v4 row of
  [`docs/specs/bench-report-schema.md`](docs/specs/bench-report-schema.md)
  lists the changes, and `kermit-lab` refuses to load v3 and v4 reports
  together unless you pass `allow_mixed_schema=True`.
  ```

- [ ] **Step 12: Check the docs.**
  ```bash
  rg -n 'Vec<Vec<usize>>' ARCHITECTURE.md CLAUDE.md docs/data-structures docs/algorithms BENCHMARKING.md
  rg -n 'morsel::scatter\b|`scatter`|scatter\(' docs/data-structures ARCHITECTURE.md CLAUDE.md
  rg -n 'leaf_tuples\(\)` returns `&' docs/data-structures
  ```
  Expected:
  - the first prints only CLAUDE.md's new Relation bullet (it names the
    `From<Vec<Vec<usize>>>` conversion) and any line about the join's
    *output* (`lftj_join` / `hash_join` still return `Vec<Vec<usize>>`, which
    the spec leaves unchanged);
  - the second and third print nothing.

  These files are not built, so no cargo step is needed. Read the rendered
  `hash-trie.md` tables once (the new parity table and the edited complexity
  rows), since a stray `|` breaks a table silently.

- [ ] **Step 13: Commit.**
  ```bash
  git add ARCHITECTURE.md CLAUDE.md BENCHMARKING.md docs/algorithms/hash-triejoin.md \
    docs/data-structures/hash-trie.md docs/data-structures/tree-trie.md \
    docs/data-structures/column-trie.md docs/data-structures/parallel-build.md \
    docs/specs/2026-10-06-column-trie-parallel-build-design.md
  git commit -m "$(cat <<'EOF'
  docs: flat tuple batches and row-id chains (#111)

  ARCHITECTURE.md and CLAUDE.md name Tuples, RowId and LeafRows, the
  impl Into<Tuples> constructors and leaf_tuples' LeafRows view. hash-trie.md
  describes the owned buffer, row-id chains, singletons and pending lists,
  the new HeapSize, scan_tuples' order (for_each_tuple still walks the
  trie), the paper-parity rows, and the radix build's 4-byte ids.
  tree-trie.md, column-trie.md and parallel-build.md describe the shared
  Tuples::sort, row slices and the row-id scatter; the parked ColumnTrie
  parallel spec notes that scatter_rows replaced scatter. BENCHMARKING.md:
  v3 and v4 insertion and space are not comparable.

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_015P4eTR34w48GpD7DvPPaee
  EOF
  )"
  ```

---

## Final gate

Run after Task 11, on a quiet host. This is CI's PR gate plus the binary's
private-item docs.

- [ ] **Step 1: The host.**
  ```bash
  pgrep -af "[k]ermit-bench-runs/.*/bin/kermit|kermit (bench|ds)|perf record"
  ```
  Expected: nothing. If a timing run is active, wait for it (its chain log
  prints `CHAIN_EXIT=`); never compile under it.

- [ ] **Step 2: The whole workspace's tests**, detached, with a watcher.
  ```bash
  setsid nohup env CARGO_BUILD_JOBS=2 cargo test --workspace > $SCRATCH/gate-test.log 2>&1 &
  echo $! > $SCRATCH/gate-test.pid; disown
  while kill -0 "$(cat $SCRATCH/gate-test.pid)" 2>/dev/null; do sleep 30; done
  grep -E '^test result' $SCRATCH/gate-test.log \
    | awk '{p+=$4; f+=$6; i+=$8} END {print "cargo "p" passed, "f" failed, "i" ignored"}'
  grep -nE 'FAILED|panicked at|error(\[E[0-9]+\])?:' $SCRATCH/gate-test.log | head -20
  ```
  Expected: `0 failed`, and the second grep prints nothing. The count is the
  T3 baseline plus the tests T1–T11 added.

- [ ] **Step 3: Lints, format and docs.**
  ```bash
  RUSTFLAGS=-Dwarnings CARGO_BUILD_JOBS=2 cargo clippy --all-targets -- -D warnings
  nix develop --command cargo fmt --all -- --check
  RUSTDOCFLAGS=-Dwarnings CARGO_BUILD_JOBS=2 cargo doc --workspace --no-deps
  CARGO_BUILD_JOBS=2 cargo rustdoc -p kermit --bin kermit -- --document-private-items -D warnings
  ```
  Expected:
  - clippy, fmt and `cargo doc` clean. If `fmt --check` disagrees with CI
    later, suspect the flake's pinned nightly first (CLAUDE.md, "The flake's
    nightly must track CI's"): `nix flake update rust-overlay`, re-run, and
    commit `flake.lock` separately only with the user's agreement.
  - `cargo rustdoc --bin kermit` reports exactly the 2 errors known before
    this plan (the binary's private docs are never linted by CI, since the
    lib and the bin share the name `kermit`). If in doubt, run the same
    command in a `git archive 47950f0` copy under `$SCRATCH` and compare.
    Any further error is this plan's (T4 and T9 edit `execution.rs` and
    `bench/*`): fix it, and commit the fix on its own
    (`docs(kermit): … (#111)`, with the two trailer lines).

- [ ] **Step 4: kermit-lab against a release binary.**
  ```bash
  CARGO_BUILD_JOBS=2 cargo build --release -p kermit
  uv --directory python/kermit-lab sync --group test
  KERMIT_BIN=$WT/target/release/kermit uv --directory python/kermit-lab run pytest -q
  ```
  Expected: all pass, including the real-binary contract test (it reads a v4
  report) and `test_loader.py`'s v3/v4 boundary tests.

- [ ] **Step 5: Scope.**
  ```bash
  git diff 47950f0 --stat -- kermit-algos/src/sorted kermit-algos/src/optimiser kermit-parser kermit-bench kermit-rdf
  ```
  Expected: empty. The declared cross-cutting change (spec, "Placement under
  the standard") touches the three structures, the hash family's views and
  join, the readers and the binary's plumbing, and nothing else.

- [ ] **Step 6: Report to the user.** The commit SHAs (T1–T11), the gate's
  counts, the mutation checks (T5, T6, T7 Step 25, T9), the rustdoc error
  count, and anything done differently from this plan, with reasons.

**Landing is the user's call.** On their word only: `git fetch origin`,
`git merge origin/master` (never rebase), re-run Steps 1–4 if anything came
in, then `git push origin HEAD:master`. Never push without it.

---

## After this plan: the replicated measurement

The spec's § Measurement, in the #107 style, after landing, in its own run
directory (`/tb/Source/Academia/kermit-bench-runs/flat-tuples-<date>/`), and
only once the user says to run it.

**Binaries.** OLD is this design's base (`47950f0`, or the merge-base with
`origin/master` it lands on); NEW is the landing SHA. For each side:
`git archive <sha>` into `$RUN/src/<sha>`; build it in its own
`CARGO_TARGET_DIR` (`$RUN/target-<sha>`) through the worktree flake, so both
use one toolchain; copy the binary to `$RUN/bin/kermit-<sha>`; record its
sha256 in `$RUN/env.txt`. The two sha256s must differ: a shared target dir
once copied the same binary out twice, because `git archive` dates files at
the commit time and cargo saw the second tree as up to date. Part B also
needs a `--no-default-features` (glibc) binary per side, built the same way
into its own target dir.

**Precautions** (the `hash-trie-algorithm-2-2026-10-07` and
`hash-trie-locality-2026-10-08` runs are the templates; copy their
`run.sh`, `chain.sh` and `host_sampler.sh`):
- before every step, `lib.sh`'s quiet gate
  (`/tb/Source/Academia/kermit-bench-runs/build-2026-10-05/lib.sh`, at most
  1.5 busy cores over 10 s), then a memory guard sized from a probed peak;
- the run-local host sampler logging to `logs/host-samples.tsv` until the
  chain prints `CHAIN_EXIT=`;
- 5 replicates, outermost, with the binary order alternating per replicate;
- a distinct `--name` per step (#98: Criterion overwrites a group shared
  across runs) and a `--report-json` per step;
- `KERMIT_WORKSPACE=$RUN`, so Criterion output lands in `$RUN/target/criterion`;
- inputs read-only: unary and binary 1e3–1e7 from
  `../tree-trie-scaling-2026-10-05/data/`, WatDiv `friendof` and `price`
  from `~/.cache/kermit/benchmarks/watdiv-stress-100-test-1/` (read, never
  written: no `--force`, `bench clean` or `bench gen` under
  `~/.cache/kermit`), and the shuffled `friendof` of
  `../radix-shuffle-2026-10-05/data/`; `oxford-uniform-s3`'s YAML copied
  into `$RUN/benchmarks`;
- OLD writes schema-3 reports and NEW schema-4 ones, so kermit-lab refuses
  to load them together: the analysis passes `allow_mixed_schema=True`
  deliberately, and compares only like with like.

**Parts:**
- **A, `insertion` and `space`, plus `bench ds` `iteration` for HashTrie**
  (its trie walk now reads rows through ids), OLD vs NEW, `bench ds` with
  `--sample-size 10 --measurement-time 3 --warm-up-time 1`, on the 12
  relations of the 2026-10-06 runs and the shuffled `friendof`:
  - TreeTrie `serial` and `parallel:16`;
  - ColumnTrie `bulk` and `incremental`;
  - HashTrie `bulk`, `radix:12`, `parallel:16` and `presized:16` (with
    `--ds-config root-capacity=tuples`), each `-m insertion space iteration`.

  `space` must not move for TreeTrie and ColumnTrie (neither keeps the
  buffer); HashTrie's is expected to about halve per tuple.
- **B, glibc:** TreeTrie `parallel:{1,16}`, and `serial` as the baseline,
  `bench ds -m insertion` on `binary-1e7`, with the `--no-default-features`
  binaries. This is #111's original criterion: the 1.9× plateau should lift.
- **C, `iteration`:** `bench run oxford-uniform-s3 -q triangle -m insertion
  iteration space`, HashTrie pruning {off, on} × expansion {eager, lazy},
  and TreeTrie/LFTJ as the control (its ratio is the two binaries' codegen
  bound), with the #92 record's Criterion settings (20 / 2 s / 1 s). The
  leaf product now reads through `LeafRows`, so eager `iteration` must hold
  within the codegen bound.

**Expected:** HashTrie `space` about halves per tuple; every `insertion`
gets faster; eager `iteration` holds within the codegen bound. **Not
expected:** #101's order penalty moving (rows stay in arrival order; Spec B
owns that). Record the results in `docs/data-structures/hash-trie.md`,
`tree-trie.md`, `column-trie.md` and `parallel-build.md` as dated records,
and post them to #111, #101 and #105. The full sweep's binary predates this
change, so its `insertion` and `space` stay v3 and are re-baselined only when
the user chooses. Spec B (#101 layer 3) is brainstormed after this
measurement, against this storage.

---

## Merge notes

This plan was assembled from separately drafted parts. Where the parts, the
reconciliation list and the code disagreed, these choices were made:

1. **Task 7 is one task.** The storage draft (S1–S14) and the iterator draft
   (I1–I11) became Steps 1–12 and 13–21; their sweeps, test runs and
   mutants were folded into Steps 22–25, with one build-and-test step and one
   commit (Step 24, written for the merge: neither draft had one).
2. **`for_each_tuple` stays a trie walk** (spec amendment, reconciliation
   item 7). The storage draft had made it a buffer scan. Rewritten: Step 7.c
   (`visit_at` reads rows through ids), Step 7.e's `resolve` doc, Step 7.i
   (`scan_tuples` is now the buffer scan), Step 5's `expand` comment (restored:
   the walk still holds `pending()`), the Step 1 test (now
   `for_each_tuple_walks_the_trie_and_scan_tuples_reads_the_buffer`, which pins
   the walk by its runs of first keys), and Step 7.l's lazy test (keeps its
   old name, checks both reads). Mutant MS4 was added to show the walk is
   pinned. Task 10's v4 entries drop the "buffer scan" claim (item 14).
3. **One `row_id`** (item 5): T5's `morsel::row_id` is `pub(crate)`; Step 6
   deletes the storage draft's `bulk::row_id`, and `bulk.rs` and
   `implementation.rs` import `crate::morsel::row_id`. `all_rows` stays in
   `bulk.rs`.
4. **A call site no draft listed:** T3's test helper `hash_rows` in
   `kermit-ds/src/relation.rs` calls `collect_tuples()`, whose return type
   Task 7 changes. Step 12 updates it (`.to_vecs()`); otherwise `rows.sort()`
   would resolve to `Tuples::sort` and the helper would not compile.
5. **A dangling doc link:** T5's `scatter_rows` doc links [`scatter`], which
   Step 11 deletes; Step 11 also drops that sentence's link.
6. **Step 22's sweep regex** gained `\b`s: the drafted `Partition::` matched
   `RowPartition::len` in Step 9's code and would have failed its "prints
   nothing" expectation.
7. **Step 7.g** spells `let tuples: Tuples = tuples.into();`, the explicit form
   T3 sets as the convention, where the draft relied on inference.
8. **T9's TreeTrie mutant** (Step 6) named `&tuple[1..]`, but T5's
   `insert_into_children` recurses on `rest` from `split_first`; the mutant now
   edits `rest` in both recursive calls.
9. **T6 Step 1** renames T3's version of the ColumnTrie wrong-arity test
   rather than "deleting" a test T3 had already rewritten (item 10). Across
   T3, T5, T6 and T7 no two tasks now edit one test in contradictory ways:
   T3 makes `column_trie:875`, `parallel.rs:644` and `radix.rs:247` uniform
   arity-3 batches; T5 deletes TreeTrie's `parallel_build_rejects_mixed_arity`
   and rewrites `parallel_build_rejects_a_first_tuple_of_the_wrong_arity`;
   T6 renames the ColumnTrie test; Task 7 rewrites `bulk.rs:432` and leaves
   the radix and parallel ones as T3 left them.
10. **T5's "no non-test caller until Step 6"** said Step 6; TreeTrie first
    calls `scatter_rows` in Step 7.
11. **T9** states the tight TreeTrie bound and what to do if it fails
    (item 17). **T10** also bumps `docs/specs/optimization-standard.md:259`
    (item 16) and owns CLAUDE.md's JSON-report gotcha; Task 11 does not touch
    either.
12. **The header** fixes the file map (`LeafRows` in `leaf_rows.rs`, item 1),
    defines `$WT` (the T1–T3 commit steps use `git -C $WT`), adds the
    mutation procedure Task 7 refers to, and asks for the spec's uncommitted
    amendments to be committed with this plan before Task 1.
13. **Task 11** adds the spec's parity table (its "after this design"
    column) to `hash-trie.md`, which had no parity rows of its own;
    `tree-trie.md:25` ("one allocation per node") is left as is, since it is
    still true.
