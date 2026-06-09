# Hash Trie Join — Design

**Date:** 2026-05-26
**Status:** Design
**Source paper:** Freitag, Bandle, Schmidt, Kemper, Neumann. *Combining Worst-Case Optimal and Traditional Binary Join Processing*. SIGMOD 2020. Local copy: `hashtrijoin.pdf`.

## 1. Goal

Add a hash-trie-based worst-case-optimal join to kermit as a sibling of `LeapfrogTriejoin`. The thesis platform gains a hash-based comparison point against the existing comparison-based (sorted) tries.

Concretely:

- A new `HashTrie` index structure in `kermit-ds`, sibling of `TreeTrie` and `ColumnTrie`.
- A new `HashTriejoin` algorithm in `kermit-algos`, sibling of `LeapfrogTriejoin`.
- Full integration: CLI selectors, const-view rewrite, test suite, per-component docs.

## 2. Scope

**In scope.** Algorithms 2 (build) and 3 (probe) from §3.2 of the paper, expressed in safe Rust. Eager incremental insert (one tuple at a time, paper-faithful hash table primitive). Test parity with existing `(DS, Algorithm)` pairs via `define_multiway_join_test_suite!`. CLI wiring (`-i hash-trie -a hash-triejoin`). Per-component docs.

**Out of scope** (deferred to follow-up changes):

- Pointer tagging (§3.3.1) — cache optimization using upper 16 bits of pointers.
- Singleton pruning (Figure 5) — sub-trie compression for single-tuple paths.
- Lazy child expansion (Figure 6) — deferred nested-table allocation.
- Morsel-driven parallel build.
- The hybrid query optimizer (Algorithm 4 / §4) — kermit has no SQL optimizer, so this is structurally inapplicable.
- Pure performance benchmarking against EmptyHeaded / LevelHeaded.

Optimizations are deferred so the first cut reads line-for-line like Algorithms 2 and 3 (CLAUDE.md Priorities item 2).

## 3. Decisions

| Decision | Choice | Rationale |
|---|---|---|
| Scope | Correctness only | Aligned with Priorities item 2 ("read like the paper"). Optimizations layer cleanly on top later. |
| Trait location | `kermit-iters/src/hash_trie.rs` | Peer of `LinearIterator`/`TrieIterator` families. |
| Hash table primitive | Paper-faithful open addressing + linear probing | Reader can map §3.3.1 directly to Rust. |
| Resize policy | Initial 4 buckets; double on load factor > 0.7 | Paper's `1.25·|L|` pre-sizing converts to LF ≈ 0.8; 0.7 gives some headroom. |
| Node structure | Recursive enum `Inner(HashTable<HashTrieNode>) \| Leaf(HashTable<Vec<Vec<usize>>>)` | Most readable mapping to Figure 3 of the paper. |
| Hash function | `std::collections::hash_map::DefaultHasher` (SipHash) | No new dependency. |
| Insert model | Eager incremental in place (resize as needed) | Maintains `Relation::insert` contract. |
| Iterator naming | Kermit-idiomatic (`key`, `next`, `open`, `up`, `at_end`) + paper-new (`lookup`, `size`, `leaf_tuples`) | Shared operations match `TrieIterator` vocabulary. |
| Result emission | Eager collect into `Vec<Vec<usize>>` then return `.into_iter()` | Preserves Algorithm 3's recursive shape without nightly generators. |
| Const-view rewrite | New `SingletonHashTrieIter` + `HashTrieIterKind` dispatch enum | Mirrors the existing `SingletonTrieIter` + `TrieIterKind` pattern. |
| Test ordering | Sort both sides in `test_join` | Relational algebra is multiset. |

The cross-cutting invariant: `hash_attribute(depth, key)` must produce identical hashes for `HashTrie` insertion and `SingletonHashTrieIter` lookup. It lives at the trait-definition site (`kermit-iters`) and is imported by both consumers.

## 4. Trait surface

New module `kermit-iters/src/hash_trie.rs`:

```rust
pub trait HashTrieIterator {
    fn key(&self) -> Option<u64>;
    fn next(&mut self) -> Option<u64>;
    fn lookup(&mut self, hash: u64) -> bool;
    fn size(&self) -> usize;
    fn at_end(&self) -> bool;
    fn open(&mut self) -> bool;
    fn up(&mut self) -> bool;
    fn leaf_tuples(&self) -> Option<&[Vec<usize>]>;
}

pub trait HashTrieIterable: JoinIterable {
    fn hash_trie_iter(&self) -> impl HashTrieIterator;
}

/// Stable, deterministic hash. Depth is mixed in so different attribute
/// positions hash into disjoint spaces.
pub fn hash_attribute(depth: usize, key: usize) -> u64;
```

Notable properties:
- `HashTrieIterator` does **not** extend `LinearIterator` — `seek` semantics are incompatible.
- `HashTrieIterable` does **not** require `IntoIterator<Item = Vec<usize>>`.
- `hash_attribute` is public from `kermit-iters` and is the single source of truth.

## 5. Data structure layout

Files under `kermit-ds/src/ds/hash_trie/`:

### 5.1 `hash_table.rs` — paper-faithful primitive

```rust
pub(crate) struct HashTable<V> {
    log2_capacity: u32,
    len: usize,
    buckets: Vec<Option<Entry<V>>>,
}

pub(crate) struct Entry<V> {
    pub hash: u64,
    pub value: V,
}
```

### 5.2 `node.rs` — recursive structure

```rust
pub(crate) enum HashTrieNode {
    Inner(HashTable<HashTrieNode>),
    Leaf(HashTable<Vec<Vec<usize>>>),
}
```

### 5.3 `implementation.rs` — `HashTrie` and traits

```rust
pub struct HashTrie {
    header: RelationHeader,
    root: HashTrieNode,
}
```

Trait impls: `Relation`, `JoinIterable`, `Projectable`, `HeapSize`.

### 5.4 `hash_trie_iter.rs` — iterator

```rust
pub struct HashTrieIter<'a> {
    stack: Vec<(&'a HashTrieNode, usize)>,
    trie:  &'a HashTrie,
}
```

### 5.5 Insertion (Algorithm 2)

`HashTrie::insert(tuple)` walks the trie level by level via recursive `insert_at`. At each level, hashes the corresponding attribute and uses `HashTable::entry_or_insert_with` to either find the existing child or allocate a new node.

## 6. The algorithm

Files under `kermit-algos/src/`:

### 6.1 `hash_triejoin.rs` — Algorithm 3

```rust
pub struct HashTriejoin {}

impl<DS: HashTrieIterable> JoinAlgo<DS> for HashTriejoin {
    fn join_iter(
        query: JoinQuery, datastructures: HashMap<String, &DS>,
    ) -> impl Iterator<Item = Vec<usize>> {
        // build_variable_index + build_variable_to_iter_map
        // enumerate(0, arity, &mut iters, ..., &mut output);
        // output.into_iter()
    }
}
```

The recursive `enumerate(i, ...)` maps directly to Algorithm 3:
- **i < arity:** pick `i_scan = argmin_{idx ∈ i_join} iters[idx].size()`. Loop over scan's hashes; probe; on full match, open all, recurse, up all. Advance via next().
- **i == arity:** cross-product the `leaf_tuples()` of every iterator in `i_join`; verify each candidate's join condition; emit verified results.

### 6.2 `hash_singleton.rs` — `SingletonHashTrieIter`

Mirror of `SingletonTrieIter`. State machine: `Root | AtValue | Exhausted`. Stores the constant `value: usize` plus a cached `hash = hash_attribute(0, value)`.

### 6.3 `hash_trie_iter_kind.rs` — dispatch enum

Mirror of `TrieIterKind`:

```rust
pub enum HashTrieIterKind<'a, R: HashTrieIterable> {
    Relation(&'a R),
    Singleton(SingletonHashTrieIter),
}
```

### 6.4 Verification at the leaf

```rust
fn verify_and_construct(
    candidate: &[&Vec<usize>],
    predicate_variables: &[Vec<usize>],
    arity: usize,
) -> Option<Vec<usize>> {
    let mut result: Vec<Option<usize>> = vec![None; arity];
    for (rel_idx, tuple) in candidate.iter().enumerate() {
        for (col, &var_idx) in predicate_variables[rel_idx].iter().enumerate() {
            let v = tuple[col];
            match result[var_idx] {
                None => result[var_idx] = Some(v),
                Some(existing) if existing != v => return None,
                _ => {}
            }
        }
    }
    Some(result.into_iter().map(Option::unwrap).collect())
}
```

## 7. Integration

### 7.1 Test scaffolding

`kermit/tests/common/utils.rs` — modify `test_join` to sort both sides before asserting equality. This widens the assertion from order-sensitive to multiset-sensitive.

`kermit/tests/join_tests.rs` adds:

```rust
define_multiway_join_test_suite!(HashTrie, HashTriejoin);
```

### 7.2 CLI selectors (`kermit/src/main.rs`)

```rust
enum IndexStructureSelector { All, ColumnTrie, HashTrie, TreeTrie }
enum JoinAlgorithmSelector  { All, HashTriejoin, LeapfrogTriejoin }
```

Plus `IndexStructureSelector::supports_algorithm` helper.

### 7.3 `DatabaseEngine::join` (`kermit/src/db.rs`)

A free function `hash_join<R: HashTrieIterable>(...)` mirrors `DatabaseEngine::join` for the hash family. Sidesteps Rust E0119 (overlapping `impl DB`).

### 7.4 Crate registration

Standard wire-up: `mod`, `pub use`, enum variants + `FromStr` arms.

### 7.5 Documentation

- `docs/data-structures/hash-trie.md` — from TEMPLATE.md.
- `docs/algorithms/hash-triejoin.md` — from TEMPLATE.md.

### 7.6 CLI integration smoke test

Exercises `bench ds -i hash-trie -a hash-triejoin`.

## 8. Invariants

1. **Cross-module hash consistency.** `hash_attribute(depth, k)` returns the same `u64` regardless of caller.
2. **Trie depth = arity.** Every `HashTrie` has `arity` levels.
3. **Leaf chains preserve all colliding tuples.** Two distinct tuples with the same hash signature end up in the same leaf chain — both checked at leaf verification.
4. **`HashTriejoin::join_iter` respects the const-view rewrite.** Singletons dispatched through `HashTrieIterKind::Singleton`.
5. **`Relation::insert` works incrementally.** `from_tuples` and `new` + repeated `insert` produce equal multisets.
6. **`HashTriejoin` is only applied to `HashTrieIterable` data.** Type-enforced.

## 9. Out of scope (explicit)

- Pointer tagging, singleton pruning, lazy child expansion.
- Parallel build.
- Lazy result iteration (stack-based suspend/resume).
- Hash function knob (swappable `BuildHasher`).
- Variable-length / string keys.
- Hybrid query optimizer.

## 10. References

- Paper: `hashtrijoin.pdf` in the repository root.
- Existing parallel: `kermit-algos/src/leapfrog_triejoin.rs`, `kermit-ds/src/ds/tree_trie/`, `kermit/tests/join_tests.rs`.
- Component-doc skeletons: `docs/algorithms/TEMPLATE.md`, `docs/data-structures/TEMPLATE.md`. Existing parallel docs (`leapfrog-triejoin.md`, `tree-trie.md`) as worked examples of the template.
- CLAUDE.md "Extending the System" — the recipe this change follows.
