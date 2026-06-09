# Hash Trie Join Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a hash-trie-based worst-case-optimal join (`HashTrie` + `HashTriejoin`) as a sibling of `LeapfrogTriejoin` in kermit, following the SIGMOD 2020 hash-trie-join paper (Algorithms 2 + 3 verbatim, "correctness only" scope).

**Architecture:** New trait family `HashTrieIterator`/`HashTrieIterable` in `kermit-iters` (disjoint from `TrieIterator` since hash navigation isn't sorted). `HashTrie` storage in `kermit-ds` is a recursive enum of `Inner(HashTable<Node>) | Leaf(HashTable<Vec<Vec<usize>>>)` over a paper-faithful open-addressing hash table primitive. `HashTriejoin` algorithm in `kermit-algos` is a recursive `enumerate(i)` mirroring Algorithm 3, with hash-collision verification at the leaf level. Const-view rewrite gets a parallel `SingletonHashTrieIter` + `HashTrieIterKind` dispatch enum.

**Tech Stack:** Rust nightly, safe Rust only, `std::collections::hash_map::DefaultHasher` for hashing.

**Spec:** [`docs/specs/2026-05-26-hash-triejoin-design.md`](../specs/2026-05-26-hash-triejoin-design.md)

**Build commands used throughout:**

- `cargo build --verbose` — workspace build
- `cargo test --package <crate>` — single-crate tests
- `nix develop --command cargo fmt --all` — nightly fmt
- `cargo clippy --all-targets` — lint

**Commit discipline:** One commit per task. CLAUDE.md forbids `cargo fmt` outside `nix develop`.

---

## Phase 1 — `kermit-iters` trait family

### Task 1.1: Add `hash_attribute` function

**Files:** Create `kermit-iters/src/hash_trie.rs`. Pure function `hash_attribute(depth: usize, key: usize) -> u64` using `DefaultHasher`. 3 tests.

Commit: `feat(iters): add hash_attribute for hash-trie iterator family`

### Task 1.2: Add `HashTrieIterator` trait

`pub trait HashTrieIterator` with 8 methods: `key`, `next`, `lookup`, `size`, `at_end`, `open`, `up`, `leaf_tuples`. Does NOT extend `LinearIterator` (seek semantics incompatible).

Commit: `feat(iters): add HashTrieIterator trait`

### Task 1.3: Add `HashTrieIterable` trait

`pub trait HashTrieIterable: JoinIterable` with `hash_trie_iter() -> impl HashTrieIterator`.

Commit: `feat(iters): add HashTrieIterable trait`

### Task 1.4: Wire module into `kermit-iters/src/lib.rs`

Re-export `hash_attribute`, `HashTrieIterable`, `HashTrieIterator`.

Commit: `feat(iters): re-export HashTrieIterator family from crate root`

---

## Phase 2 — `HashTable<V>` primitive

### Task 2.1: Skeleton + `new`, `len`, `bucket_index`

**Files:** Create `kermit-ds/src/ds/hash_trie/mod.rs`, `kermit-ds/src/ds/hash_trie/hash_table.rs`. Capacity 4, `bucket_index(hash) = hash >> (64 - log2_capacity)`.

Commit: `feat(ds): scaffold HashTable primitive`

### Task 2.2: `get` / `get_mut` with linear probing

Commit: `feat(ds): HashTable::get/get_mut with linear probing`

### Task 2.3: `entry_or_insert_with` (pre-resize)

Commit: `feat(ds): HashTable::entry_or_insert_with (pre-resize)`

### Task 2.4: Resize on load factor > 0.7

`grow()` doubles `log2_capacity`, rehashes via `insert_during_grow`.

Commit: `feat(ds): HashTable resize on load factor > 0.7`

### Task 2.5: `iter()` over occupied buckets

Commit: `feat(ds): HashTable::iter over occupied buckets`

---

## Phase 3 — `HashTrieNode` + `HashTrie` storage

### Task 3.1: `HashTrieNode` enum

```rust
pub(crate) enum HashTrieNode {
    Inner(HashTable<HashTrieNode>),
    Leaf(HashTable<Vec<Vec<usize>>>),
}
```

Commit: `feat(ds): HashTrieNode recursive enum`

### Task 3.2: `HashTrie` struct + `Relation::new`/`header`

Add `pub(crate) fn root(&self) -> &HashTrieNode` accessor needed by Phase 4. Stub `Projectable` and `HeapSize` with `unimplemented!()`.

Commit: `feat(ds): HashTrie struct + Relation::new`

### Task 3.3: `Relation::insert` via recursive `insert_at`

Calls `hash_attribute(depth, key)` from `kermit-iters`. Algorithm 2 verbatim.

Commit: `feat(ds): HashTrie::insert via recursive insert_at`

### Task 3.4: `Relation::from_tuples` and `insert_all`

Loop `insert_at` over inputs.

Commit: `feat(ds): HashTrie::from_tuples and insert_all`

### Task 3.5: `collect_tuples` helper

`pub(crate) fn collect_tuples(&self) -> Vec<Vec<usize>>` — depth-first walk, used by Projectable.

Commit: `feat(ds): HashTrie::collect_tuples for projection and tests`

### Task 3.6: `HeapSize` impl

Recursively sums `HashTable::shell_heap_bytes` + leaf chain capacities.

Commit: `feat(ds): HashTrie HeapSize impl`

### Task 3.7: `Projectable` impl

Materializes via `collect_tuples`, projects, rebuilds via `from_tuples`. Does NOT use `project_via_trie_iter` (requires `TrieIterable`).

Commit: `feat(ds): HashTrie Projectable impl`

---

## Phase 4 — `HashTrieIter`

### Task 4.1: Struct + `new` + `open`

```rust
pub struct HashTrieIter<'a> {
    stack: Vec<(&'a HashTrieNode, usize)>,
    trie: &'a HashTrie,
}
```

Add HashTable helpers: `buckets_len`, `next_occupied`, `value_at`, `hash_at`.

Commit: `feat(ds): HashTrieIter scaffolding with open`

### Task 4.2: `key`, `next`, `at_end`, `size`

Commit: `feat(ds): HashTrieIter::{key,next,at_end,size}`

### Task 4.3: `lookup`

Add `HashTable::index_of` helper.

Commit: `feat(ds): HashTrieIter::lookup with HashTable::index_of`

### Task 4.4: `up`

Commit: `feat(ds): HashTrieIter::up`

### Task 4.5: `leaf_tuples`

Returns `Some(&[Vec<usize>])` only at Leaf level with occupied bucket.

Commit: `feat(ds): HashTrieIter::leaf_tuples`

### Task 4.6: `HashTrieIterable` impl on `HashTrie`

Returns `HashTrieIter::new(self)` via `impl HashTrieIterator`.

Commit: `feat(ds): HashTrie HashTrieIterable impl`

---

## Phase 5 — Crate-level wiring for `HashTrie`

### Task 5.1: Add `IndexStructure::HashTrie` variant + re-export

**Files:**
- Modify: `kermit-ds/src/ds/mod.rs`
- Modify: `kermit-ds/src/lib.rs`

Commit: `feat(ds): expose HashTrie via IndexStructure and crate root`

---

## Phase 6 — `kermit-algos`: singleton and dispatch enum

### Task 6.1: `SingletonHashTrieIter` skeleton

State machine `Root | AtValue | Exhausted`. Hash precomputed via `hash_attribute(0, value)`. 5 tests; `leaf_tuples` stubbed.

Commit: `feat(algos): SingletonHashTrieIter skeleton`

### Task 6.2: `SingletonHashTrieIter::leaf_tuples`

Cached `chain: Vec<Vec<usize>>` field, materialized in `new()` as `vec![vec![value]]`. 2 tests.

Commit: `feat(algos): SingletonHashTrieIter::leaf_tuples`

### Task 6.3: `HashTrieIterKind` dispatch enum

```rust
pub enum HashTrieIterKind<'a, R: HashTrieIterable> {
    Relation(&'a R),
    Singleton(SingletonHashTrieIter),
}
```

Plus `HashKindIter<IT>` delegating all `HashTrieIterator` methods. 2 tests.

Commit: `feat(algos): HashTrieIterKind dispatch enum`

---

## Phase 7 — `HashTriejoin` algorithm

### Task 7.1: `build_variable_index` helper (duplicate from LFTJ)

Per scope discipline (CLAUDE.md item 6), don't refactor LFTJ to share — duplicate the helper. Tests: `variable_index_triangle`, `variable_to_iter_map_triangle`.

Commit: `feat(algos): hash_triejoin query indexing helpers`

### Task 7.2: `verify_and_construct` helper

Validates leaf cross-product candidate; rejects on shared-variable disagreement (hash false positive). 3 tests.

Commit: `feat(algos): verify_and_construct for leaf-level emission`

### Task 7.3: `enumerate` + `emit_leaf` + `advance_cursor`

Algorithm 3 verbatim. 1 end-to-end test (`enumerate_unary_intersection`).

Commit: `feat(algos): enumerate + emit_leaf — Algorithm 3 verbatim`

### Task 7.4: `HashTriejoin` struct + `JoinAlgo` impl

`HashTriejoin: JoinAlgo<DS: HashTrieIterable>`. Add `JoinAlgorithm::HashTriejoin` enum variant + `FromStr` arm. 2 end-to-end tests.

Commit: `feat(algos): HashTriejoin entry point`

---

## Phase 8 — Test scaffolding

### Task 8.1: Sort both sides in `test_join`

**Files:** Modify `kermit/tests/common/utils.rs`.

```rust
let mut actual: Vec<Vec<usize>> = JA::join_iter(query, ds_map).collect();
actual.sort();
let mut expected = result;
expected.sort();
assert_eq!(actual, expected);
```

Widens assertion to multiset equality (relational algebra semantics).

Commit: `test: sort both sides in test_join for multiset equality`

### Task 8.2: Add HashTrie/HashTriejoin to `join_tests.rs`

`define_multiway_join_test_suite!(HashTrie, HashTriejoin);` — generates 11 tests. Total 33 tests in join_tests (11 × 3 pairs).

Commit: `test: define_multiway_join_test_suite!(HashTrie, HashTriejoin)`

---

## Phase 9 — CLI wiring

### Task 9.1: Add `HashTrie` + `HashTriejoin` to selectors

`IndexStructureSelector` and `JoinAlgorithmSelector` enums gain new variants + `expand()` arms. Update `expand_*` unit tests.

Commit: `feat(cli): add HashTrie/HashTriejoin to selectors`

### Task 9.2: `IndexStructureSelector::supports_algorithm` helper

Hash trie pairs with hash triejoin only; sorted tries pair with LFTJ only; `All` selectors are permissive. 1 test with 12 assertions.

Commit: `feat(cli): IndexStructureSelector::supports_algorithm`

### Task 9.3: Replace stub arms in `instantiate_database` and bench dispatch

The hash family uses a free function `hash_join`, not the `DB` trait (E0119 coherence). CLI dispatch builds `HashMap<String, HashTrie>` and calls `hash_join` for the hash family.

Commit: `feat(cli): wire HashTrie + HashTriejoin into bench dispatch`

### Task 9.4: `hash_join` free function in `kermit/src/db.rs`

```rust
pub fn hash_join<R: HashTrieIterable>(
    relations: &HashMap<String, R>,
    query: JoinQuery,
) -> Vec<Vec<usize>>
```

Mirrors `DatabaseEngine::join` body but uses `HashTrieIterKind` / `SingletonHashTrieIter`. 2 integration tests including const-view rewrite path.

Commit: `feat(cli): hash_join free function for HashTrie family`

---

## Phase 10 — Documentation

### Task 10.1: `docs/data-structures/hash-trie.md`

From TEMPLATE.md: representation, invariants, complexity table, worked micro-example, when-to-prefer.

Commit: `docs(data-structures): hash-trie.md per Priorities item 3`

### Task 10.2: `docs/algorithms/hash-triejoin.md`

From TEMPLATE.md: pseudocode mapping to Algorithm 3, invariants, complexity, worked triangle example.

Commit: `docs(algorithms): hash-triejoin.md per Priorities item 3`

---

## Final Phase — Verification

### Task F.1: Full workspace test + lint + fmt + doc + miri

1. `cargo test --workspace --verbose`
2. `RUSTFLAGS=-Dwarnings cargo clippy --all-targets --verbose`
3. `nix develop --command cargo fmt --all -- --check`
4. `RUSTDOCFLAGS=-Dwarnings cargo doc --workspace --no-deps`
5. `MIRIFLAGS="-Zmiri-disable-isolation" cargo miri test --package kermit-iters --package kermit-ds --package kermit-algos`

### Task F.2: CLI end-to-end smoke test

```bash
cargo build --release
cargo run --release -- bench list
cargo run --release -- bench run triangle -i hash-trie -a hash-triejoin
ls bench-runs/ | tail -1  # verify report produced
```

Should report contains `axes.algorithm == "HashTriejoin"` and `axes.data_structure == "HashTrie"`.

---

## Self-Review Notes

This plan is exhaustively decomposed (~30 tasks across 10 phases) because the work touches every crate in the workspace and follows the recognizable pattern from CLAUDE.md item 4. Each phase leaves the workspace in a buildable state. Tests at each task verify the immediate behavior; the F.1 / F.2 verification phase catches end-to-end issues.

**Notable design decisions encoded in the plan:**
- Spec-required `hash_attribute(depth, key)` mixed depth into the hash for "cross-attribute aliasing prevention". During Phase 7 implementation, this was found to be WRONG — different attribute positions live in physically distinct hash tables, so the safeguard was structurally unnecessary AND actively broke cross-relation lookups. The implementer fixed it by making `depth` a no-op while preserving the signature for ABI compatibility.
- `enumerate` opens iterators at entry and ascends at exit (the paper's pseudocode assumed pre-opened iterators, but `JoinAlgo::join_iter` constructs un-opened ones).
- Hash-family CLI dispatch uses a free function (not the `DB` trait) to sidestep Rust E0119 (overlapping `impl DB`).
