# Optimization Standard Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Establish kermit-wide standard for adding optimizations to data structures and algorithms (Layout / Config / BuildMode categories), and validate it by refactoring HashTrie's hash function into a generic Layout option (`HashTrie<H: HashStrategy>` with `SipHashStrategy` and `FxHashStrategy`).

**Architecture:** New trait family in `kermit-iters::optimization` defines four traits (`LayoutOption`, `ConfigOption`, `BuildMode`, `HasOptimizationAxes`). A separate `kermit-iters::hash_strategy` module introduces the first Layout consumer (`HashStrategy` trait + two concrete strategies). HashTrie is genericized over `H: HashStrategy` with a default of `SipHashStrategy` to preserve backwards compatibility. SingletonHashTrieIter decouples from `H` by storing a precomputed hash. CLI gains `--ds-layout-hasher <choice>` flag with conditional validation. Bench-report axes grow `ds_layout_hasher` under the standard's prefix convention.

**Tech Stack:** Rust nightly, safe Rust only, `std::collections::hash_map::DefaultHasher` for Sip, new `rustc-hash` dependency for Fx, `clap` for CLI parsing.

**Spec:** [`docs/specs/2026-05-27-optimization-standard-design.md`](../specs/2026-05-27-optimization-standard-design.md)

**Build commands used throughout:**

- `cargo build --verbose` — workspace build
- `cargo test --package <crate>` — single-crate tests
- `nix develop --command cargo fmt --all` — nightly fmt (required inside `nix develop`)
- `cargo clippy --all-targets` — lint

**Commit discipline:** One commit per task. CLAUDE.md forbids `cargo fmt` outside `nix develop`.

---

## Phase 1 — `kermit-iters` trait family + hasher strategy

### Task 1.1: Add `optimization` module with four traits

**Files:**
- Create: `kermit-iters/src/optimization.rs`

Add traits `LayoutOption`, `ConfigOption`, `BuildMode`, `HasOptimizationAxes`. Wire into `kermit-iters/src/lib.rs` with re-exports.

Commit: `feat(iters): add optimization trait family`

### Task 1.2: Add `hash_strategy` module with two concrete strategies

**Files:**
- Create: `kermit-iters/src/hash_strategy.rs`
- Modify: `kermit-iters/Cargo.toml` (add `rustc-hash = "2"`)

Define `HashStrategy: LayoutOption + Copy + Default + 'static`. Concrete `SipHashStrategy` (NAME = "sip") and `FxHashStrategy` (NAME = "fxhash"). 5 unit tests:
- `sip_strategy_name_is_sip`
- `fxhash_strategy_name_is_fxhash`
- `sip_strategy_is_deterministic`
- `fxhash_strategy_is_deterministic`
- `sip_and_fxhash_produce_different_hashes`

Commit: `feat(iters): add HashStrategy trait + Sip/FxHash strategies`

### Task 1.3: Remove legacy `hash_attribute` function

**Files:**
- Modify: `kermit-iters/src/hash_trie.rs` (remove `hash_attribute` + 3 tests)
- Modify: `kermit-iters/src/lib.rs` (remove re-export)

**This task intentionally breaks the workspace build.** Phases 2.2 and 3.1 will restore it by updating the call sites.

Commit: `refactor(iters): remove legacy hash_attribute function`

---

## Phase 2 — Refactor HashTrie to be generic over `H`

### Task 2.1: Add `PhantomData<H>` field and generic parameter to HashTrie

**Files:**
- Modify: `kermit-ds/src/ds/hash_trie/implementation.rs`

- `pub struct HashTrie<H: HashStrategy = SipHashStrategy>` with `_hasher: PhantomData<H>`.
- All `impl HashTrie` blocks become `impl<H: HashStrategy> HashTrie<H>`.
- `insert_at` calls `H::hash(key)` instead of `hash_attribute(depth, key)`.
- `JoinIterable`, `Relation`, `Projectable`, `HeapSize`, `HashTrieIterable` impls generic.

**Caveat:** Rust default generic parameters resolve in type position but not path expressions. Existing tests calling `HashTrie::new(...)` need `let trie: HashTrie = ...` type annotations.

Commit: `refactor(ds): genericize HashTrie over H: HashStrategy`

### Task 2.2: Make `HashTrieIter` generic over `H`

**Files:**
- Modify: `kermit-ds/src/ds/hash_trie/hash_trie_iter.rs`

`pub struct HashTrieIter<'a, H: HashStrategy = SipHashStrategy>` with `&'a HashTrie<H>` field. Helper methods (`node_capacity`, etc.) don't need `H` — they operate on `&HashTrieNode`.

Commit: `refactor(ds): genericize HashTrieIter over H: HashStrategy`

### Task 2.3: Implement `HasOptimizationAxes` on `HashTrie<H>`

**Files:**
- Modify: `kermit-ds/src/ds/hash_trie/implementation.rs`
- Modify: `kermit-ds/Cargo.toml` (add `serde_json = "1"`)

```rust
impl<H: HashStrategy> HasOptimizationAxes for HashTrie<H> {
    fn optimization_axes(&self) -> BTreeMap<String, Value> {
        let mut axes = BTreeMap::new();
        axes.insert("ds_layout_hasher".to_string(), H::NAME.into());
        axes
    }
}
```

Add 2 tests verifying axes for Sip and Fx.

Commit: `feat(ds): HashTrie HasOptimizationAxes impl`

---

## Phase 3 — Update `SingletonHashTrieIter` and `hash_join`

### Task 3.1: `SingletonHashTrieIter::new` accepts precomputed hash

**Files:**
- Modify: `kermit-algos/src/hash_singleton.rs`

Signature changes from `new(value)` to `new(value, hash)`. Iterator no longer depends on `HashStrategy`. Update 7 existing tests with helper `fn h(v) = SipHashStrategy::hash(v)`.

Commit: `refactor(algos): SingletonHashTrieIter::new accepts precomputed hash`

### Task 3.2: Make `hash_join` generic over `H: HashStrategy`

**Files:**
- Modify: `kermit/src/db.rs`

```rust
pub fn hash_join<R, H>(
    relations: &HashMap<String, R>,
    query: JoinQuery,
) -> Vec<Vec<usize>>
where
    R: HashTrieIterable,
    H: HashStrategy,
```

Singleton hashes computed via `H::hash(id)`. Existing 2 tests gain turbofish.

**Note:** Rust forbids defaults on free-function type parameters (RFC #36887). Callers must specify `H` explicitly.

Commit: `refactor(cli): hash_join generic over H: HashStrategy`

---

## Phase 4 — CLI integration

### Task 4.1: Add `HasherChoice` and `LayoutChoices` in main.rs

**Files:**
- Modify: `kermit/src/main.rs`

```rust
#[derive(Copy, Clone, Debug, Default, clap::ValueEnum)]
pub enum HasherChoice { #[default] Sip, Fxhash }

#[derive(Clone, Debug, Default, clap::Args)]
pub struct LayoutChoices {
    #[arg(long = "ds-layout-hasher", value_enum)]
    pub hash_trie_hasher: Option<HasherChoice>,
}
```

Flatten into `BenchSubcommand::Run` and `BenchSubcommand::Ds` via `#[command(flatten)]`.

Commit: `feat(cli): add HasherChoice + LayoutChoices selector`

### Task 4.2: Dispatch — monomorphize over the chosen strategy

**Files:**
- Modify: `kermit/src/main.rs`

`run_ds_bench_hash<H>` and `run_benchmark_hash<H>` become generic. Dispatch sites:

```rust
(IndexStructure::HashTrie, JoinAlgorithm::HashTriejoin) => {
    match layout.hash_trie_hasher_resolved() {
        HasherChoice::Sip => run_benchmark_hash::<SipHashStrategy>(...)?,
        HasherChoice::Fxhash => run_benchmark_hash::<FxHashStrategy>(...)?,
    }
}
```

**This commit restores the workspace build.**

Commit: `feat(cli): monomorphize HashTrie dispatch over H`

### Task 4.3: Conditional validation for `--ds-layout-hasher`

**Files:**
- Modify: `kermit/src/main.rs`

Post-parse `validate_layout_choices` function. Rejects `--ds-layout-hasher` when DS isn't HashTrie. 3 unit tests:
- `validate_layout_choices_accepts_explicit_hasher_on_hash_trie_or_all`
- `validate_layout_choices_rejects_explicit_hasher_on_non_hash_trie`
- `validate_layout_choices_default_layout_passes_on_any_selector`

Commit: `feat(cli): conditional validation for --ds-layout-hasher`

---

## Phase 5 — Bench-report integration

### Task 5.1: Merge `optimization_axes` into the bench-report axes

**Files:**
- Modify: `kermit/src/main.rs` (in `run_ds_bench_hash` and `run_benchmark_hash`)
- Create: `kermit/tests/cli_hash_trie_hasher_choice.rs`

```rust
// After existing axes assembly:
axes.extend(relation.optimization_axes());
```

CLI smoke tests:
- `cli_bench_ds_with_fxhash_layout_records_axis` — asserts `ds_layout_hasher == "fxhash"`
- `cli_bench_ds_default_hasher_is_sip` — asserts `ds_layout_hasher == "sip"`

Commit: `feat(cli): bench-report axes include ds_layout_hasher`

---

## Phase 6 — Test scaffolding for Layout variants

### Task 6.1: Replace `define_multiway_join_test_suite!(HashTrie, …)` with explicit type aliases

**Files:**
- Modify: `kermit/tests/join_tests.rs`

```rust
type HashTrieSip = HashTrie<SipHashStrategy>;
type HashTrieFx = HashTrie<FxHashStrategy>;

define_multiway_join_test_suite!(TreeTrie, LeapfrogTriejoin);
define_multiway_join_test_suite!(ColumnTrie, LeapfrogTriejoin);
define_multiway_join_test_suite!(HashTrieSip, HashTriejoin);
define_multiway_join_test_suite!(HashTrieFx, HashTriejoin);
```

44 tests total (11 patterns × 4 DS/algo pairs).

Commit: `test: split HashTrie suite by Layout variant via type aliases`

---

## Phase 7 — Documentation

### Task 7.1: Create `docs/specs/optimization-standard.md`

Normative reference document covering the three categories, trait family, naming conventions, and adoption recipe.

Commit: `docs(specs): add optimization-standard.md (normative)`

### Task 7.2: Update CLAUDE.md

Append "Adding an optimization to a data structure or algorithm" subsection under "Extending the System". Extend Priorities item 1 with test obligation matrix.

Commit: `docs(claude.md): document optimization standard in 'Extending the System'`

### Task 7.3: Update `docs/specs/bench-report-schema.md`

Append "Standard axis prefixes" section documenting `ds_layout_*`, `ds_config_*`, `ds_build_mode`, and reserved `algo_*` prefixes.

Commit: `docs(specs): document standard axis prefixes in bench-report-schema`

### Task 7.4: Update `docs/data-structures/hash-trie.md`

Append "Optimizations" section listing HashTrie's currently-supported optimizations (hasher choice).

Commit: `docs(data-structures): add Optimizations section to hash-trie.md`

---

## Final Phase — Verification

### Task F.1: Full workspace test + lint + fmt + doc + miri

1. `cargo test --workspace --verbose` — all tests pass (modulo any pre-existing watdiv flakiness).
2. `RUSTFLAGS=-Dwarnings cargo clippy --all-targets --verbose` — zero warnings.
3. `nix develop --command cargo fmt --all -- --check` — clean.
4. `RUSTDOCFLAGS=-Dwarnings cargo doc --workspace --no-deps` — clean.
5. `MIRIFLAGS="-Zmiri-disable-isolation" cargo miri test --package kermit-iters --package kermit-ds --package kermit-algos` — passes.

### Task F.2: End-to-end CLI smoke test

```bash
# With fxhash
cargo run --release -- bench --report-json /tmp/bench-fxhash.json \
    ds --relation kermit/tests/fixtures/edge.csv \
    -i hash-trie --ds-layout-hasher fxhash \
    -m space --sample-size 10 --measurement-time 1 --warm-up-time 1

# Verify axis
cat /tmp/bench-fxhash.json | jq '.[0].axes."ds_layout_hasher"'
# Expected: "fxhash"

# Default sip
cargo run --release -- bench --report-json /tmp/bench-sip.json \
    ds --relation kermit/tests/fixtures/edge.csv \
    -i hash-trie \
    -m space --sample-size 10 --measurement-time 1 --warm-up-time 1

cat /tmp/bench-sip.json | jq '.[0].axes."ds_layout_hasher"'
# Expected: "sip"

# Incompatible pair rejected
cargo run --release -- bench ds --relation kermit/tests/fixtures/edge.csv \
    -i tree-trie --ds-layout-hasher fxhash
# Expected: error mentioning --ds-layout-hasher only valid with hash-trie
```

---

## Self-Review Notes

The plan threads a deliberate **broken-build window** through Phase 1.3 → Phase 4.2 (a 4-task arc). The first task removes `hash_attribute` from `kermit-iters`; subsequent tasks update each call site one crate at a time. The workspace doesn't build during this window. Each commit message clearly flags this so the implementer doesn't think the broken build is a regression.

The plan also intentionally **inverts an earlier decision** in Task 4.3: Task 4.1 sets `hash_trie_hasher: HasherChoice` with a clap default, Task 4.3 evolves it to `Option<HasherChoice>` + resolver methods to support conditional validation. The evolution is contained within the phase and each step is a working state.
