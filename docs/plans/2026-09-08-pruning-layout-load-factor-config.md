# Pruning as Layout, Load Factor as Config — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement Amendment 1 of `docs/specs/2026-09-08-singleton-pruning-config-design.md`: move singleton pruning from a runtime Config to a zero-cost Layout parameter on `HashTrie`, and make the hash-table load-factor cap the first real Config consumer.

**Architecture:** `HashTrie<H, P>` gains a `PruningPolicy` marker `P` whose associated payload type is uninhabited for `NoPruning`, so every `Singleton` arm in the node and the iterator frame is dead code in the off instantiation and it compiles back to the pre-pruning machine code. `HashTrieConfig` loses `singleton_pruning` and gains `load_factor`, an exact percent fraction threaded into the existing resize comparison. All Config plumbing landed earlier (`ConfigurableRelation`, `Configured`, `ConfigChoices`, `build_relation`) is reused unchanged.

**Tech Stack:** Rust nightly workspace, clap 4 derive, `paste`, serde_json, Python/uv (kermit-lab).

**Read first:** the spec's Amendment 1 (Sections "Why", A, B, C, D) and the current files named in each task. Sections 1–6 of the spec describe the code as it is on the branch today.

**Conventions for every task:**

- Run `cargo fmt` only as `nix develop --command cargo fmt --all`. Stable rustfmt rewrites 30+ files.
- Build and test with `CARGO_BUILD_JOBS=2` in the foreground (the host's background-task monitor kills parallel cargo jobs under page-cache pressure). One heavy step per command.
- CI: `cargo clippy --all-targets -- -Dwarnings`, `RUSTDOCFLAGS=-Dwarnings cargo doc --workspace --no-deps`, `nix develop --command cargo fmt --all -- --check`, `nix develop --command cargo miri test` on the library crates (miri is only on PATH inside `nix develop`). Backtick identifiers in `///` docs.
- Commit messages: conventional commits ending with
  ```
  Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_0113pPzevVrcidsKvjdYT5VH
  ```
- Do not modify `TreeTrie`, `ColumnTrie`, `HashTriejoin`, `kermit-iters/src/hash_trie.rs`, or `kermit-algos`.

---

## File structure

| File | Responsibility | Action |
|---|---|---|
| `kermit-ds/src/ds/hash_trie/pruning.rs` | `PruningPolicy`, `SingletonPayload`, `SingletonFrame`, `Never`, `NoPruning`, `SingletonPruning` | Create |
| `kermit-ds/src/ds/hash_trie/config.rs` | `HashTrieConfig { load_factor }`, `LoadFactor` | Modify |
| `kermit-ds/src/ds/hash_trie/hash_table.rs` | `entry_or_insert_with` takes the cap | Modify |
| `kermit-ds/src/ds/hash_trie/node.rs` | `HashTrieNode<P>` with `Singleton(P::Payload)` | Modify |
| `kermit-ds/src/ds/hash_trie/implementation.rs` | `HashTrie<H, P>`; `insert_at` gated on `P::ENABLED`; axes emit `ds_layout_pruning` | Modify |
| `kermit-ds/src/ds/hash_trie/hash_trie_iter.rs` | `HashTrieIter<'a, H, P>`; `Frame::Singleton(P::Frame<'a>)` | Modify |
| `kermit-ds/src/ds/hash_trie/mod.rs`, `ds/mod.rs`, `lib.rs` | exports | Modify |
| `kermit-ds/src/configured.rs` | tests use a load-factor provider | Modify |
| `kermit-ds/tests/hash_trie_tests.rs`, `parquet_tests.rs` | pruned Layout aliases; load-factor `Configured` aliases | Modify |
| `kermit/src/main.rs` | `PruningChoice`, `--ds-layout-pruning`, `with_hash_trie_layout!`, `load-factor` key | Modify |
| `kermit/src/execution.rs` | `Execution::HashHtj { hasher, pruning, config }`, `HashHtj<H, P>` | Modify |
| `kermit/tests/join_tests.rs` | four Layout aliases; `with_config` on load factor | Modify |
| `kermit/tests/cli_hash_trie_layout_pruning.rs` | smoke test | Create |
| `kermit/tests/cli_hash_trie_config_choice.rs` | switch to `load-factor` | Modify |
| `python/kermit-lab/kermit_lab/defaults.py`, `tests/*` | axis defaults and fixture rename | Modify |
| `docs/data-structures/hash-trie.md`, `docs/specs/optimization-standard.md`, `docs/specs/bench-report-schema.md`, `CLAUDE.md`, the design spec | docs | Modify |

---

### Task 1: the pruning policy module (additive, no consumers yet)

**Files:**
- Create: `kermit-ds/src/ds/hash_trie/pruning.rs`
- Modify: `kermit-ds/src/ds/hash_trie/mod.rs`, `kermit-ds/src/ds/mod.rs`, `kermit-ds/src/lib.rs`

- [ ] **Step 1: Write the failing tests**

Create `kermit-ds/src/ds/hash_trie/pruning.rs` with only:

```rust
//! Pruning policy — the second Layout dimension of [`HashTrie`](super::HashTrie).
//!
//! Singleton pruning (SIGMOD 2020 §3.3.1, Figure 5) stores a subtrie that
//! holds exactly one tuple as that tuple instead of one hash table per
//! remaining level. It is a *shape*: fixed at construction, it changes which
//! node variants exist. Encoding it as a type parameter lets the `NoPruning`
//! instantiation compile to the pre-pruning code — the `Singleton` payload
//! and the iterator's singleton frame are uninhabited there, so every arm
//! that handles them is dead code the compiler removes. Bench axis:
//! `ds_layout_pruning`.

#[cfg(test)]
mod tests {
    use {super::*, kermit_iters::LayoutOption};

    #[test]
    fn policy_names_are_the_axis_values() {
        assert_eq!(<NoPruning as LayoutOption>::NAME, "off");
        assert_eq!(<SingletonPruning as LayoutOption>::NAME, "on");
    }

    #[test]
    fn enabled_matches_the_marker() {
        assert!(!NoPruning::ENABLED);
        assert!(SingletonPruning::ENABLED);
    }

    #[test]
    fn vec_payload_round_trips_the_tuple() {
        let p = <Vec<usize> as SingletonPayload>::from_tuple(vec![1, 2, 3]);
        assert_eq!(p.tuple(), &vec![1, 2, 3]);
        assert_eq!(p.into_tuple(), vec![1, 2, 3]);
    }

    #[test]
    fn never_is_uninhabited() {
        assert_eq!(std::mem::size_of::<Never>(), 0);
        // A reference to an uninhabited payload can never be produced; the
        // `Option` below is `None` by construction and the branch is dead.
        let none: Option<&Never> = None;
        assert!(none.is_none());
    }

    #[test]
    fn frame_emulates_a_one_entry_table() {
        let tuple = vec![7, 8, 9];
        let mut f = <SingletonFrameOn<'_> as SingletonFrame<'_>>::new(&tuple, 1, 0xBEEF);
        assert_eq!(f.depth(), 1);
        assert_eq!(f.key(), Some(0xBEEF));
        assert!(!f.at_end());
        assert!(!f.lookup(0xDEAD));
        assert!(f.at_end());
        assert_eq!(f.key(), None);
        assert!(f.lookup(0xBEEF));
        assert!(!f.at_end());
        f.exhaust();
        assert!(f.at_end());
        assert_eq!(f.tuple(), &tuple);
    }
}
```

Register the module in `kermit-ds/src/ds/hash_trie/mod.rs` (add `mod pruning;` and export):

```rust
pub use {
    config::HashTrieConfig,
    implementation::HashTrie,
    pruning::{NoPruning, PruningPolicy, SingletonPruning},
};
```

Re-export from `kermit-ds/src/ds/mod.rs` (`hash_trie::{HashTrie, HashTrieConfig, NoPruning, PruningPolicy, SingletonPruning}`) and from `kermit-ds/src/lib.rs` (`ds::{ColumnTrie, HashTrie, HashTrieConfig, IndexStructure, NoPruning, PruningPolicy, SingletonPruning, TreeTrie}`).

- [ ] **Step 2: Run tests to verify they fail**

Run: `CARGO_BUILD_JOBS=2 cargo test -p kermit-ds pruning::tests`
Expected: compile errors (`NoPruning`, `SingletonFrame` not found).

- [ ] **Step 3: Write the implementation** (above the test module)

```rust
use kermit_iters::LayoutOption;

/// The tuple a `HashTrieNode::Singleton` stores. `Vec<usize>` when pruning
/// is on; [`Never`] when it is off, which makes the variant uninhabited.
pub trait SingletonPayload {
    fn from_tuple(tuple: Vec<usize>) -> Self;
    fn tuple(&self) -> &Vec<usize>;
    fn into_tuple(self) -> Vec<usize>;
}

/// The iterator's stand-in for the one-entry table a pruned level would
/// have held. One implementor per policy; the `NoPruning` one is [`Never`],
/// so the iterator's singleton arms vanish in that instantiation.
pub trait SingletonFrame<'a>: Sized {
    /// A frame at `depth` for `tuple`, positioned on its single entry.
    /// `hash == H::hash(tuple[depth])`, computed once by the caller.
    fn new(tuple: &'a Vec<usize>, depth: usize, hash: u64) -> Self;
    fn depth(&self) -> usize;
    fn tuple(&self) -> &'a Vec<usize>;
    /// `Some(hash)` unless exhausted.
    fn key(&self) -> Option<u64>;
    /// Advance past the single entry.
    fn exhaust(&mut self);
    /// Position on the entry iff `hash` matches; otherwise exhaust.
    fn lookup(&mut self, hash: u64) -> bool;
    fn at_end(&self) -> bool;
}

/// Compile-time pruning policy of a [`HashTrie`](super::HashTrie).
pub trait PruningPolicy: LayoutOption + Copy + Default + 'static {
    type Payload: SingletonPayload;
    type Frame<'a>: SingletonFrame<'a>;
    /// Folded by the compiler: `insert_at`'s prune and unprune branches are
    /// `if P::ENABLED { … }`.
    const ENABLED: bool;
}

/// An uninhabited type. A `Singleton(Never)` variant can never be built, so
/// the enum containing it has the layout of the enum without it.
#[derive(Copy, Clone, Debug)]
pub enum Never {}

impl SingletonPayload for Never {
    fn from_tuple(_tuple: Vec<usize>) -> Self {
        unreachable!("NoPruning never constructs a Singleton (P::ENABLED is false)")
    }

    fn tuple(&self) -> &Vec<usize> { match *self {} }

    fn into_tuple(self) -> Vec<usize> { match self {} }
}

impl<'a> SingletonFrame<'a> for Never {
    fn new(_tuple: &'a Vec<usize>, _depth: usize, _hash: u64) -> Self {
        unreachable!("NoPruning never pushes a singleton frame")
    }

    fn depth(&self) -> usize { match *self {} }

    fn tuple(&self) -> &'a Vec<usize> { match *self {} }

    fn key(&self) -> Option<u64> { match *self {} }

    fn exhaust(&mut self) { match *self {} }

    fn lookup(&mut self, _hash: u64) -> bool { match *self {} }

    fn at_end(&self) -> bool { match *self {} }
}

impl SingletonPayload for Vec<usize> {
    fn from_tuple(tuple: Vec<usize>) -> Self { tuple }

    fn tuple(&self) -> &Vec<usize> { self }

    fn into_tuple(self) -> Vec<usize> { self }
}

/// Level `depth` of a pruned subtrie holding `tuple`; `exhausted` plays the
/// role of a table frame's past-end bucket index.
#[derive(Debug)]
pub struct SingletonFrameOn<'a> {
    // `&Vec`, not `&[usize]`: `leaf_tuples` returns `&[Vec<usize>]` via
    // `slice::from_ref`.
    tuple: &'a Vec<usize>,
    depth: usize,
    hash: u64,
    exhausted: bool,
}

impl<'a> SingletonFrame<'a> for SingletonFrameOn<'a> {
    fn new(tuple: &'a Vec<usize>, depth: usize, hash: u64) -> Self {
        debug_assert!(
            depth < tuple.len(),
            "singleton frame at depth {depth} below a {}-attribute tuple",
            tuple.len()
        );
        Self {
            tuple,
            depth,
            hash,
            exhausted: false,
        }
    }

    fn depth(&self) -> usize { self.depth }

    fn tuple(&self) -> &'a Vec<usize> { self.tuple }

    fn key(&self) -> Option<u64> { (!self.exhausted).then_some(self.hash) }

    fn exhaust(&mut self) { self.exhausted = true; }

    fn lookup(&mut self, hash: u64) -> bool {
        self.exhausted = hash != self.hash;
        !self.exhausted
    }

    fn at_end(&self) -> bool { self.exhausted }
}

/// Pruning off: the pre-pruning `HashTrie`, bit for bit. Bench axis value
/// `"off"`. The default `P`.
#[derive(Copy, Clone, Default, Debug)]
pub struct NoPruning;

impl LayoutOption for NoPruning {
    const NAME: &'static str = "off";
}

impl PruningPolicy for NoPruning {
    type Frame<'a> = Never;
    type Payload = Never;

    const ENABLED: bool = false;
}

/// Singleton pruning on (paper Figure 5). Bench axis value `"on"`.
#[derive(Copy, Clone, Default, Debug)]
pub struct SingletonPruning;

impl LayoutOption for SingletonPruning {
    const NAME: &'static str = "on";
}

impl PruningPolicy for SingletonPruning {
    type Frame<'a> = SingletonFrameOn<'a>;
    type Payload = Vec<usize>;

    const ENABLED: bool = true;
}
```

- [ ] **Step 4: Run tests**

Run: `CARGO_BUILD_JOBS=2 cargo test -p kermit-ds pruning::tests`
Expected: 5 passed. Then `cargo clippy -p kermit-ds --all-targets -- -Dwarnings` (an unused-import or dead-code warning on the new module is possible; the items are `pub` and re-exported, so none is expected) and `RUSTDOCFLAGS=-Dwarnings cargo doc -p kermit-ds --no-deps`.

- [ ] **Step 5: Commit**

```bash
git add kermit-ds/src/ds/hash_trie/pruning.rs kermit-ds/src/ds/hash_trie/mod.rs kermit-ds/src/ds/mod.rs kermit-ds/src/lib.rs
git commit -m "feat(ds): add the PruningPolicy Layout markers with an uninhabited off payload"
```

---

### Task 2: `LoadFactor` on `HashTrieConfig`, threaded into `HashTable` (additive)

**Files:**
- Modify: `kermit-ds/src/ds/hash_trie/config.rs`
- Modify: `kermit-ds/src/ds/hash_trie/hash_table.rs`
- Modify: `kermit-ds/src/ds/hash_trie/implementation.rs` (the two `entry_or_insert_with` calls and the tests)

`singleton_pruning` stays on the struct in this task; Task 4 removes it.

- [ ] **Step 1: Write the failing tests**

Append to the `tests` module in `config.rs`:

```rust
    #[test]
    fn default_load_factor_is_seventy_percent() {
        let lf = HashTrieConfig::default().load_factor;
        assert_eq!(lf, LoadFactor::percent(70).unwrap());
        assert_eq!(lf.numerator(), 70);
        assert_eq!(lf.denominator(), 100);
        assert!((lf.as_f64() - 0.7).abs() < 1e-12);
    }

    #[test]
    fn load_factor_rejects_out_of_range_percentages() {
        assert!(LoadFactor::percent(0).is_err());
        assert!(LoadFactor::percent(100).is_err());
        assert!(LoadFactor::percent(1).is_ok());
        assert!(LoadFactor::percent(99).is_ok());
    }

    #[test]
    fn axes_report_load_factor_as_a_number() {
        let cfg = HashTrieConfig {
            load_factor: LoadFactor::percent(50).unwrap(),
            ..HashTrieConfig::default()
        };
        assert!(cfg
            .axes()
            .contains(&("load_factor", Value::from(0.5_f64))));
    }
```

Append to the `tests` module in `hash_table.rs` (it exists; check its imports and match them):

```rust
    /// With cap `p`%, the table doubles exactly on the insert that would
    /// push `(len + 1) / capacity` above `p / 100`.
    #[test]
    fn resizes_exactly_at_the_configured_cap() {
        for percent in [50u8, 70, 90] {
            let lf = super::super::config::LoadFactor::percent(percent).unwrap();
            let mut t: HashTable<usize> = HashTable::new();
            let mut last_cap = t.buckets_len();
            let mut hash: u64 = 1;
            for _ in 0..64 {
                let before_len = t.len();
                t.entry_or_insert_with(hash, lf, || 0);
                if t.buckets_len() != last_cap {
                    // Doubled on this insert: the *previous* occupancy plus one
                    // must have exceeded the cap for the old capacity.
                    assert!(
                        (before_len + 1) * 100 > last_cap * usize::from(percent),
                        "{percent}%: grew early at len {before_len}, cap {last_cap}"
                    );
                    last_cap = t.buckets_len();
                } else {
                    assert!(
                        (before_len + 1) * 100 <= last_cap * usize::from(percent),
                        "{percent}%: should have grown at len {before_len}, cap {last_cap}"
                    );
                }
                hash = hash.wrapping_mul(0x9E37_79B9_7F4A_7C15).wrapping_add(1);
            }
        }
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `CARGO_BUILD_JOBS=2 cargo test -p kermit-ds config::tests hash_table::tests`
Expected: compile errors (`LoadFactor` missing; `entry_or_insert_with` arity).

- [ ] **Step 3: Implement `LoadFactor`** in `config.rs`

Replace the struct and impl with:

```rust
use {kermit_iters::ConfigOption, serde_json::Value, std::fmt};

/// Maximum table occupancy before a level's hash table doubles, as an exact
/// percentage so the resize test stays integer arithmetic:
/// `(len + 1) * 100 > capacity * percent`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LoadFactor {
    percent: u8,
}

/// A load factor outside the open unit interval.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidLoadFactor(pub u8);

impl fmt::Display for InvalidLoadFactor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "load factor must be between 1% and 99%, got {}%", self.0)
    }
}

impl std::error::Error for InvalidLoadFactor {}

impl LoadFactor {
    /// The pre-Config constant: 70 %.
    pub const DEFAULT_PERCENT: u8 = 70;

    /// A cap of `percent` %, which must lie in `1..=99`.
    pub fn percent(percent: u8) -> Result<Self, InvalidLoadFactor> {
        if (1..=99).contains(&percent) {
            Ok(Self {
                percent,
            })
        } else {
            Err(InvalidLoadFactor(percent))
        }
    }

    /// The cap as a fraction `numerator / denominator`.
    pub fn numerator(self) -> usize { usize::from(self.percent) }

    pub fn denominator(self) -> usize { 100 }

    /// The cap as the decimal the bench axis reports.
    pub fn as_f64(self) -> f64 { f64::from(self.percent) / 100.0 }
}

impl Default for LoadFactor {
    fn default() -> Self {
        Self {
            percent: Self::DEFAULT_PERCENT,
        }
    }
}

/// Runtime values read by `HashTrie` while it is built.
///
/// A Config is a *value* on a path the code already takes (the resize
/// comparison runs on every insert); replacing a constant with it adds no
/// branch, so non-users pay nothing. See the shape/value/process rule in
/// `docs/specs/optimization-standard.md`.
#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
pub struct HashTrieConfig {
    /// Singleton pruning. *Superseded*: becomes the `P` Layout parameter in
    /// the next change and is removed from this struct.
    pub singleton_pruning: bool,
    /// Occupancy cap before a level's table doubles. Bench axis
    /// `ds_config_load_factor` (e.g. `0.7`).
    pub load_factor: LoadFactor,
}

impl ConfigOption for HashTrieConfig {
    fn axes(&self) -> Vec<(&'static str, Value)> {
        vec![
            ("singleton_pruning", Value::Bool(self.singleton_pruning)),
            ("load_factor", Value::from(self.load_factor.as_f64())),
        ]
    }
}
```

Update the existing `axes_report_singleton_pruning_suffix_and_value` test to assert with `contains` instead of whole-vector equality. Export `LoadFactor` and `InvalidLoadFactor` from `hash_trie/mod.rs`, `ds/mod.rs`, and `lib.rs` beside `HashTrieConfig`.

- [ ] **Step 4: Thread the cap into `HashTable`**

In `hash_table.rs`: delete `LOAD_FACTOR_NUM` / `LOAD_FACTOR_DEN`; add `use super::config::LoadFactor;`; change the signature and the comparison:

```rust
    /// Insert at `hash`, or return a `&mut V` to the existing entry. The
    /// `default` closure is invoked only if the slot is currently empty.
    ///
    /// Resizes the table when the occupancy would exceed `load_factor`
    /// (see [`grow`]). The cap is a parameter rather than a field so that
    /// varying it costs no space per table.
    pub fn entry_or_insert_with<F: FnOnce() -> V>(
        &mut self, hash: u64, load_factor: LoadFactor, default: F,
    ) -> &mut V {
```

and

```rust
        // About to insert a new entry. Check the cap first:
        //   LF > p/100  ⇔  (len + 1) / capacity > p / 100
        //              ⇔  (len + 1) * 100 > capacity * p
        if (self.len + 1) * load_factor.denominator() > cap * load_factor.numerator() {
```

Update the struct doc ("grows by doubling when occupancy exceeds the configured load factor, 70 % by default") and the `grow` doc. Update every existing call in `hash_table.rs` tests to pass `LoadFactor::default()`.

In `implementation.rs`, `insert_at` gains `load_factor: LoadFactor` after `prune: bool`, and both `entry_or_insert_with` calls pass it; the two call sites pass `self.config.load_factor` / `config.load_factor`. Extend `HasOptimizationAxes` nothing (the `for (suffix, value) in self.config.axes()` loop already emits the new key). Update the `optimization_axes_include_config_flag` and the key-set tests to include `ds_config_load_factor`.

- [ ] **Step 5: Run tests**

Run: `CARGO_BUILD_JOBS=2 cargo test -p kermit-ds` then clippy and doc for the crate.
Expected: all pass, including `resizes_exactly_at_the_configured_cap`.

- [ ] **Step 6: Commit**

```bash
git add kermit-ds/src/ds/hash_trie/config.rs kermit-ds/src/ds/hash_trie/hash_table.rs kermit-ds/src/ds/hash_trie/implementation.rs kermit-ds/src/ds/hash_trie/mod.rs kermit-ds/src/ds/mod.rs kermit-ds/src/lib.rs
git commit -m "feat(ds): make the HashTable load-factor cap a HashTrieConfig value (ds_config_load_factor)"
```

---

### Task 3: `--ds-config load-factor=<0.01..0.99>` (additive)

**Files:**
- Modify: `kermit/src/main.rs` (`ConfigChoices`, its tests)
- Modify: `kermit/tests/cli_hash_trie_config_choice.rs`

- [ ] **Step 1: Write the failing tests**

In the `tests` module of `main.rs`, add:

```rust
    #[test]
    fn config_choices_parse_load_factor() {
        let half = ConfigChoices {
            ds_config: vec!["load-factor=0.5".into()],
        };
        assert_eq!(
            half.hash_trie_config_resolved().unwrap().load_factor,
            LoadFactor::percent(50).unwrap()
        );
        assert_eq!(
            ConfigChoices::default()
                .hash_trie_config_resolved()
                .unwrap()
                .load_factor,
            LoadFactor::default()
        );
    }

    #[test]
    fn config_choices_reject_bad_load_factors() {
        for bad in ["0", "1", "1.5", "-0.2", "0.555", "abc"] {
            let choices = ConfigChoices {
                ds_config: vec![format!("load-factor={bad}")],
            };
            let msg = choices
                .hash_trie_config_resolved()
                .unwrap_err()
                .to_string();
            assert!(msg.contains("load-factor"), "{bad}: {msg}");
        }
    }
```

Update `every_advertised_hash_trie_key_is_accepted` so each key gets a valid sample value: build the pair from a small table `[("singleton-pruning", "true"), ("load-factor", "0.5")]` and assert the table's keys equal `HASH_TRIE_KEYS`.

In `cli_hash_trie_config_choice.rs`, add:

```rust
#[test]
fn cli_bench_ds_with_load_factor_records_axis() {
    let (output, report) = run_bench_ds("hash-trie", &["--ds-config", "load-factor=0.5"]);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(axes_of(&report)["ds_config_load_factor"], 0.5);
}

#[test]
fn cli_bench_ds_default_load_factor_is_seventy_percent() {
    let (output, report) = run_bench_ds("hash-trie", &[]);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(axes_of(&report)["ds_config_load_factor"], 0.7);
}
```

- [ ] **Step 2: Run to verify failure**

Run: `CARGO_BUILD_JOBS=2 cargo test -p kermit --bin kermit config_choices`
Expected: compile error / unknown-key failure.

- [ ] **Step 3: Implement**

In `ConfigChoices`: `const HASH_TRIE_KEYS: &'static [&'static str] = &["singleton-pruning", "load-factor"];` and a new match arm:

```rust
                | "load-factor" => {
                    config.load_factor = parse_load_factor(value)
                        .map_err(|why| anyhow::anyhow!("--ds-config {key}: {why}"))?;
                },
```

with, beside `ConfigChoices`:

```rust
/// Parses `--ds-config load-factor=<decimal>`: a value in (0, 1) with at
/// most two decimal places, mapped onto `LoadFactor`'s exact percent.
fn parse_load_factor(value: &str) -> Result<LoadFactor, String> {
    let v: f64 = value
        .parse()
        .map_err(|_| format!("expected a decimal in (0, 1), got {value:?}"))?;
    if !v.is_finite() || v <= 0.0 || v >= 1.0 {
        return Err(format!("expected a decimal in (0, 1), got {value:?}"));
    }
    let scaled = v * 100.0;
    let percent = scaled.round();
    if (scaled - percent).abs() > 1e-9 {
        return Err(format!("at most two decimal places are supported, got {value:?}"));
    }
    // 0 < v < 1 and integral*100 ⇒ 1..=99; `percent` cannot fail here, but
    // keep the constructor's check as the single source of the range.
    LoadFactor::percent(percent as u8).map_err(|e| e.to_string())
}
```

Import `LoadFactor` from `kermit_ds`.

- [ ] **Step 4: Run tests**

Run: `CARGO_BUILD_JOBS=2 cargo test -p kermit` then clippy for the crate.
Expected: all pass, including the two new smoke tests.

- [ ] **Step 5: Commit**

```bash
git add kermit/src/main.rs kermit/tests/cli_hash_trie_config_choice.rs
git commit -m "feat(kermit): accept --ds-config load-factor and report ds_config_load_factor"
```

---

### Task 4: pruning becomes the `P` Layout parameter (two commits, one implementer)

The workspace does not compile between the two commits; run only crate-level checks after the first, workspace checks after the second.

**Files (commit A, kermit-ds):**
- Modify: `node.rs`, `implementation.rs`, `hash_trie_iter.rs`, `config.rs`, `configured.rs`, `kermit-ds/tests/hash_trie_tests.rs`, `kermit-ds/tests/parquet_tests.rs`

**Files (commit B, kermit):**
- Modify: `kermit/src/execution.rs`, `kermit/src/main.rs`, `kermit/tests/join_tests.rs`, `kermit/tests/cli_hash_trie_config_choice.rs`

#### Commit A

- [ ] **Step A1: Write the failing tests** (they define the target API)

In `implementation.rs` tests, replace the `PRUNE` const and `pruned` helper with:

```rust
    use super::super::pruning::{NoPruning, SingletonPayload, SingletonPruning};

    type Pruned = HashTrie<SipHashStrategy, SingletonPruning>;

    fn pruned(arity: usize, tuples: Vec<Vec<usize>>) -> Pruned {
        Pruned::from_tuples(arity.into(), tuples)
    }
```

and make the walker generic:

```rust
    fn check_pruning_invariant<P: PruningPolicy>(root: &HashTrieNode<P>) -> usize {
        assert!(!matches!(root, HashTrieNode::Singleton(_)), "the root is never a Singleton");
        check_pruning_invariant_at(root, &mut Vec::new())
    }

    fn check_pruning_invariant_at<P: PruningPolicy>(
        node: &HashTrieNode<P>, prefix: &mut Vec<u64>,
    ) -> usize {
        match node {
            | HashTrieNode::Singleton(payload) => {
                assert!(P::ENABLED, "Singleton found with pruning off");
                let tuple = payload.tuple();
                for (d, &h) in prefix.iter().enumerate() {
                    assert_eq!(
                        <SipHashStrategy as HashStrategy>::hash(tuple[d]),
                        h,
                        "Singleton {tuple:?} is misplaced at depth {d}"
                    );
                }
                1
            },
            | HashTrieNode::Leaf(table) => table.iter().map(|(_, chain)| chain.len()).sum(),
            | HashTrieNode::Inner(table) => table
                .iter()
                .map(|(hash, child)| {
                    prefix.push(hash);
                    let below = check_pruning_invariant_at(child, prefix);
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

Every pruning test then calls `check_pruning_invariant(&trie.root)` with no `prune` argument; `pruning_off_never_creates_singletons` builds a plain `HashTrie` (default `P = NoPruning`). Add:

```rust
    #[test]
    fn node_size_is_independent_of_the_pruning_policy() {
        assert_eq!(
            std::mem::size_of::<HashTrieNode<NoPruning>>(),
            std::mem::size_of::<HashTrieNode<SingletonPruning>>()
        );
    }

    #[test]
    fn optimization_axes_include_the_pruning_layout() {
        use kermit_iters::HasOptimizationAxes;
        let off: HashTrie = HashTrie::new(2.into());
        assert_eq!(
            off.optimization_axes().get("ds_layout_pruning"),
            Some(&serde_json::Value::String("off".into()))
        );
        let on = Pruned::new(2.into());
        assert_eq!(
            on.optimization_axes().get("ds_layout_pruning"),
            Some(&serde_json::Value::String("on".into()))
        );
        assert!(!on.optimization_axes().contains_key("ds_config_singleton_pruning"));
    }
```

Update the key-set assertions in the two hasher-axis tests to `["ds_config_load_factor", "ds_layout_hasher", "ds_layout_pruning"]`. Delete `with_config_stores_config_and_new_uses_default`'s pruning content and the `config_survives_insert_and_insert_all` pruning references; they now use `load_factor: LoadFactor::percent(50).unwrap()` as the non-default config.

In `hash_trie_iter.rs` tests: delete the `PRUNE` const and the `ConfigurableRelation` import; replace every `HashTrie::from_tuples_with_config(a.into(), PRUNE, t)` with `HashTrie::<SipHashStrategy, SingletonPruning>::from_tuples(a.into(), t)` (add the import). Add:

```rust
    #[test]
    fn off_frame_is_the_bare_table_pair() {
        // The `NoPruning` frame collapses to one inhabited variant, so the
        // stack entry is exactly the pre-pruning `(node, idx)` pair.
        assert_eq!(
            std::mem::size_of::<Frame<'static, NoPruning>>(),
            std::mem::size_of::<(&'static HashTrieNode<NoPruning>, usize)>()
        );
    }
```

In `config.rs`: remove `singleton_pruning` and its two tests; `axes()` returns only `("load_factor", …)`; drop the "Superseded" doc line.

In `configured.rs` tests: replace the `PruningOn` provider with

```rust
    crate::define_config_provider!(HalfFull, HashTrieConfig, HashTrieConfig {
        load_factor: LoadFactor::percent(50).unwrap(),
    });
    type HalfFullTrie = Configured<HashTrie<SipHashStrategy>, HalfFull>;
```

and assert `config().load_factor == LoadFactor::percent(50).unwrap()` and the axis `ds_config_load_factor == 0.5` where the tests asserted pruning before. Update the macro doctest the same way.

In `kermit-ds/tests/hash_trie_tests.rs`: replace the `PruningOn` block with

```rust
// ── Layout variant: singleton pruning on ────────────────────────────────
//
// Each hasher alias also runs with the pruning Layout on, so the iterator
// contract holds on emulated (pruned) levels as well as materialised ones.
// `Mod10` is the important case: full-collision tuples must unprune into a
// shared leaf chain.
type HashTrieSipPruned = HashTrie<SipHashStrategy, SingletonPruning>;
type HashTrieFxPruned = HashTrie<FxHashStrategy, SingletonPruning>;
type HashTrieMod10Pruned = HashTrie<Mod10HashStrategy, SingletonPruning>;

// ── Config variant: a dense load factor ─────────────────────────────────
define_config_provider!(NinetyPercent, HashTrieConfig, HashTrieConfig {
    load_factor: LoadFactor::percent(90).unwrap(),
});
type HashTrieSipDense = Configured<HashTrieSip, NinetyPercent>;

hash_trie_test_suite!(HashTrieSipDense, SipHashStrategy);
```

(keep the three pruned suite invocations; imports become `kermit_ds::{define_config_provider, Configured, HashTrie, HashTrieConfig, LoadFactor, SingletonPruning}`). In `parquet_tests.rs`: the pruned aliases become the Layout aliases (`sorted_tuples` is generic over `H` only; make it `fn sorted_tuples<H: HashStrategy, P: PruningPolicy>(relation: &HashTrie<H, P>)`) and the `sorted_tuples_pruned` collector is replaced by one for `Configured<HashTrieSip, NinetyPercent>`.

- [ ] **Step A2: Run to verify failure**

Run: `CARGO_BUILD_JOBS=2 cargo test -p kermit-ds`
Expected: compile errors throughout (`HashTrieNode<P>` etc.).

- [ ] **Step A3: `node.rs`**

```rust
use super::{hash_table::HashTable, pruning::PruningPolicy};

/// A node in a hash trie, parameterised by the pruning policy so that the
/// `Singleton` variant is uninhabited (and costs nothing) under `NoPruning`.
pub(crate) enum HashTrieNode<P: PruningPolicy> {
    Inner(HashTable<HashTrieNode<P>>),
    Leaf(HashTable<Vec<Vec<usize>>>),
    /// Pruned subtrie: exactly one tuple lives below this point. Never the
    /// root. Holds `P::Payload`: the tuple when pruning is on, `Never`
    /// when it is off.
    Singleton(P::Payload),
}

impl<P: PruningPolicy> HashTrieNode<P> {
```

All existing methods keep their bodies; the accessors' `Singleton` arms stay as `| HashTrieNode::Singleton(_) => Self::singleton_is_not_a_table(),` with the `#[cold]` helper. (Under `NoPruning` rustc omits the uninhabited variant from the layout and the arm is dead.)

- [ ] **Step A4: `implementation.rs`**

```rust
use {
    super::{
        config::{HashTrieConfig, LoadFactor},
        node::HashTrieNode,
        pruning::{NoPruning, PruningPolicy, SingletonPayload},
    },
    crate::relation::{ConfigurableRelation, Relation, RelationHeader},
    kermit_iters::{ConfigOption, HashStrategy, JoinIterable, LayoutOption, SipHashStrategy},
    std::marker::PhantomData,
};

pub struct HashTrie<H: HashStrategy = SipHashStrategy, P: PruningPolicy = NoPruning> {
    header: RelationHeader,
    root: HashTrieNode<P>,
    tuple_count: usize,
    config: HashTrieConfig,
    _layout: PhantomData<(H, P)>,
}
```

Every `impl<H: HashStrategy>` becomes `impl<H: HashStrategy, P: PruningPolicy>` and every `HashTrie<H>` / `HashTrieNode` in signatures becomes `HashTrie<H, P>` / `HashTrieNode<P>`. `collect_at`'s singleton arm: `out.push(payload.tuple().clone())`; `node_heap_bytes` likewise `payload.tuple().capacity() * size_of::<usize>()`. `insert_at`:

```rust
    /// Insert one tuple at the appropriate depth. Algorithm 2 of the paper,
    /// plus the singleton-pruning extension of §3.3.1 (Figure 5) when the
    /// policy `P` enables it. `P::ENABLED` is a constant, so under
    /// `NoPruning` both pruning branches are compiled out and this is the
    /// pre-pruning insert. An unprune is a single extra O(arity) chain, not
    /// a fan-out: the evicted tuple stops as a new `Singleton` where the two
    /// diverge while only the new tuple keeps descending.
    fn insert_at(
        node: &mut HashTrieNode<P>, depth: usize, arity: usize, tuple: Vec<usize>,
        load_factor: LoadFactor,
    ) {
        let key = tuple[depth];
        let hash = H::hash(key);
        match node {
            | HashTrieNode::Inner(table) => {
                if P::ENABLED && table.get(hash).is_none() {
                    // Fresh bucket: the subtrie below holds exactly one tuple,
                    // so store the tuple itself instead of one table per
                    // remaining level. The closure always runs — absence was
                    // just proven — so this is two O(1) probes, kept over a
                    // special-cased insert for readability.
                    table.entry_or_insert_with(hash, load_factor, || {
                        HashTrieNode::Singleton(P::Payload::from_tuple(tuple))
                    });
                    return;
                }
                let child_is_leaf = Self::is_leaf_depth(depth + 1, arity);
                let child = table.entry_or_insert_with(hash, load_factor, || {
                    HashTrieNode::new_table(child_is_leaf)
                });
                if P::ENABLED && matches!(child, HashTrieNode::Singleton(_)) {
                    // Unprune: a second tuple has arrived, so the subtrie no
                    // longer holds exactly one. Swap in the table this level
                    // would have had and re-insert the evicted tuple ahead of
                    // the new one; the recursion re-prunes where they diverge.
                    let replacement = HashTrieNode::new_table(child_is_leaf);
                    let HashTrieNode::Singleton(evicted) = std::mem::replace(child, replacement)
                    else {
                        unreachable!("matched Singleton above")
                    };
                    Self::insert_at(child, depth + 1, arity, evicted.into_tuple(), load_factor);
                }
                Self::insert_at(child, depth + 1, arity, tuple, load_factor);
            },
            | HashTrieNode::Leaf(table) => {
                let chain = table.entry_or_insert_with(hash, load_factor, Vec::new);
                chain.push(tuple);
            },
            | HashTrieNode::Singleton(_) => unreachable!(
                "insert_at descends through Inner/Leaf only; singletons are unpruned by the parent"
            ),
        }
    }
```

Call sites pass `self.config.load_factor` / `config.load_factor` (no `prune` argument). `with_config` sets `_layout: PhantomData`. `project` returns `HashTrie::<H, P>::from_tuples_with_config(...)`. `hash_trie_iter` returns `HashTrieIter::<H, P>::new(self)`. `HasOptimizationAxes`:

```rust
        axes.insert(
            "ds_layout_hasher".to_string(),
            serde_json::Value::String(<H as LayoutOption>::NAME.to_string()),
        );
        axes.insert(
            "ds_layout_pruning".to_string(),
            serde_json::Value::String(<P as LayoutOption>::NAME.to_string()),
        );
        for (suffix, value) in self.config.axes() {
            axes.insert(format!("ds_config_{suffix}"), value);
        }
```

Struct doc: add a `# Layout parameters` paragraph naming `H` and `P` and restate the invariant as "with `SingletonPruning`, a child is `Singleton` iff exactly one tuple lives below it; under `NoPruning` no `Singleton` can be constructed".

- [ ] **Step A5: `hash_trie_iter.rs`**

```rust
use {
    super::{
        implementation::HashTrie,
        node::HashTrieNode,
        pruning::{NoPruning, PruningPolicy, SingletonFrame, SingletonPayload},
    },
    crate::relation::Relation,
    kermit_iters::{HashStrategy, HashTrieIterator, SipHashStrategy},
};

/// One opened level of the trie.
enum Frame<'a, P: PruningPolicy> {
    /// A bucket within an `Inner` or `Leaf` node — never a `Singleton`.
    Table { node: &'a HashTrieNode<P>, idx: usize },
    /// Level `depth` of a pruned subtrie; `Never` under `NoPruning`, so
    /// this variant is uninhabited there and the frame is the bare pair.
    Singleton(P::Frame<'a>),
}

impl<P: PruningPolicy> Frame<'_, P> {
    fn at_end(&self) -> bool {
        match self {
            | Frame::Table { node, idx } => *idx >= node.buckets_len(),
            | Frame::Singleton(s) => s.at_end(),
        }
    }
}

enum Descent<'a, P: PruningPolicy> {
    Node(&'a HashTrieNode<P>),
    Deeper(&'a Vec<usize>),
    Blocked,
}

pub struct HashTrieIter<'a, H: HashStrategy = SipHashStrategy, P: PruningPolicy = NoPruning> {
    stack: Vec<Frame<'a, P>>,
    trie: &'a HashTrie<H, P>,
}
```

Method bodies keep their current logic with these substitutions: `frame_for`'s singleton arm is `HashTrieNode::Singleton(payload) => Self::singleton_frame(payload.tuple(), depth)`; `singleton_frame` is `Frame::Singleton(P::Frame::new(tuple, depth, H::hash(tuple[depth])))` (the `debug_assert` moved into `SingletonFrameOn::new`); in `descent`, the singleton arm reads `Some(Frame::Singleton(s)) => if s.at_end() || s.depth() + 1 >= self.arity() { Blocked } else { Deeper(s.tuple()) }`; `key` → `s.key()`; `next` → `{ s.exhaust(); None }`; `lookup` → `s.lookup(hash)`; `size` → `1`; `leaf_tuples` → `(!s.at_end() && s.depth() + 1 == self.arity()).then(|| std::slice::from_ref(s.tuple()))`. The `unreachable!` arms for a `Singleton` node inside a `Table` frame stay. `impl<H: HashStrategy, P: PruningPolicy> HashTrieIterator for HashTrieIter<'_, H, P>`.

Module doc: replace "A `Frame::Singleton` emulates …" with a sentence that the singleton frame type is chosen by `P` and is uninhabited under `NoPruning`.

- [ ] **Step A6: Run kermit-ds checks**

Run: `CARGO_BUILD_JOBS=2 cargo test -p kermit-ds`, then `cargo clippy -p kermit-ds --all-targets -- -Dwarnings`, `RUSTDOCFLAGS=-Dwarnings cargo doc -p kermit-ds --no-deps`.
Expected: all pass. If `off_frame_is_the_bare_table_pair` fails, the `Never` frame did not collapse; stop and report (the acceptance criterion in Task 9 would fail too).

- [ ] **Step A7: Commit A**

```bash
git add kermit-ds
git commit -m "feat(ds)!: singleton pruning becomes the P Layout parameter of HashTrie (ds_layout_pruning)"
```

#### Commit B

- [ ] **Step B1: `main.rs` — `PruningChoice`, the flag, validation**

Beside `HasherChoice`:

```rust
/// CLI-side selector for `--ds-layout-pruning`: the `PruningPolicy`
/// monomorphised into `HashTrie<H, P>`. `Off` is the pre-pruning structure.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, clap::ValueEnum)]
enum PruningChoice {
    #[default]
    Off,
    On,
}
```

`LayoutChoices` gains

```rust
    /// Singleton pruning Layout of `HashTrie<H, P>` (default: `off`). Only
    /// valid when `--indexstructure hash-trie` is selected.
    #[arg(long = "ds-layout-pruning", value_name = "PRUNING", value_enum)]
    hash_trie_pruning: Option<PruningChoice>,
```

with `hash_trie_pruning_resolved()` / `hash_trie_pruning_explicit()` mirroring the hasher pair. `validate_layout_choices` checks both flags and names the offending one:

```rust
    let explicit: &[(&str, bool)] = &[
        ("--ds-layout-hasher", layout.hash_trie_hasher_explicit()),
        ("--ds-layout-pruning", layout.hash_trie_pruning_explicit()),
    ];
    for (flag, given) in explicit {
        if *given && !matches!(indexstructure, IndexStructureSelector::HashTrie | IndexStructureSelector::All) {
            anyhow::bail!("{flag} is only valid with --indexstructure hash-trie (or all); got --indexstructure {indexstructure:?}");
        }
    }
    Ok(())
```

Add the macro that expands the 2 × 2 Layout product once:

```rust
/// Monomorphises `$body` over the `HashTrie` Layout cell selected at
/// runtime. Both dispatchers use it, so the product of Layout dimensions
/// lives in one place: adding a dimension means adding arms here only.
macro_rules! with_hash_trie_layout {
    ($hasher:expr, $pruning:expr, |$H:ident, $P:ident| $body:expr) => {
        match ($hasher, $pruning) {
            | (HasherChoice::Sip, PruningChoice::Off) => {
                type $H = SipHashStrategy;
                type $P = NoPruning;
                $body
            },
            | (HasherChoice::Sip, PruningChoice::On) => {
                type $H = SipHashStrategy;
                type $P = SingletonPruning;
                $body
            },
            | (HasherChoice::Fxhash, PruningChoice::Off) => {
                type $H = FxHashStrategy;
                type $P = NoPruning;
                $body
            },
            | (HasherChoice::Fxhash, PruningChoice::On) => {
                type $H = FxHashStrategy;
                type $P = SingletonPruning;
                $body
            },
        }
    };
}
```

`run_ds_bench_hash<H: HashStrategy, P: PruningPolicy>` builds `HashTrie::<H, P>` (three sites). `dispatch_ds_bench` gains `pruning: PruningChoice` and its hash arm becomes

```rust
        | IndexStructure::HashTrie => with_hash_trie_layout!(hasher, pruning, |H, P| {
            run_ds_bench_hash::<H, P>(relation, ds, metrics, queries_per_build, config, group_name, bench_args)
        }),
```

`dispatch_run_bench`'s two hash arms become one:

```rust
        | Execution::HashHtj { hasher, pruning, config } => {
            with_hash_trie_layout!(hasher, pruning, |H, P| run_benchmark(
                &HashHtj::<H, P>::new(hasher, pruning, config, optimiser),
                benchmark, optimiser, metrics, queries_per_build, query_filter, bench_args,
            ))
        },
```

`resolve_sweep`, `run_ds_bench_command`, `run_bench_run_command` thread `layout.hash_trie_pruning_resolved()`. Remove the `singleton-pruning` key, its match arm, and its tests from `ConfigChoices`; `HASH_TRIE_KEYS = &["load-factor"]`; `config_choices_reject_unknown_key_and_bad_value` uses `load-factor` for its malformed / repeated cases and `singleton-pruning=true` as the *unknown* key. Add tests `validate_layout_choices_rejects_explicit_pruning_on_non_hash_trie` (mirror of the hasher one, asserting the message names `--ds-layout-pruning`) and `pruning_choice_default_is_off`. Import `NoPruning, PruningPolicy, SingletonPruning, LoadFactor` from `kermit_ds`.

- [ ] **Step B2: `execution.rs`**

```rust
    HashHtj {
        hasher: HasherChoice,
        pruning: PruningChoice,
        config: HashTrieConfig,
    },
```

`for_pair` and `Sweep::expand` gain `pruning: PruningChoice` (after `hasher`). `HashHtj<H, P>`:

```rust
pub struct HashHtj<H, P> {
    hasher: HasherChoice,
    pruning: PruningChoice,
    config: HashTrieConfig,
    optimiser: Box<dyn kermit_algos::QueryOptimiser>,
    _layout: PhantomData<(H, P)>,
}

impl<H, P> HashHtj<H, P> {
    pub fn new(hasher: HasherChoice, pruning: PruningChoice, config: HashTrieConfig, optimiser: Optimiser) -> Self { … }
}

impl<H: HashStrategy + 'static, P: PruningPolicy> ExecutionFamily for HashHtj<H, P> {
    type Engine = HashMap<String, HashTrie<H, P>>;
    type Rel = HashTrie<H, P>;
    fn execution(&self) -> Execution { Execution::HashHtj { hasher: self.hasher, pruning: self.pruning, config: self.config } }
    fn build_relation(&self, header: RelationHeader, tuples: Vec<Vec<usize>>) -> HashTrie<H, P> {
        HashTrie::<H, P>::from_tuples_with_config(header, self.config, tuples)
    }
    // remaining methods: replace `HashTrie<H>` with `HashTrie<H, P>`; `join` stays `hash_join::<HashTrie<H, P>, H>(…)`.
}
```

Import `crate::PruningChoice` and `kermit_ds::{NoPruning, PruningPolicy, SingletonPruning}`. Update every test call (`for_pair(…, HasherChoice::Fxhash, PruningChoice::Off, HashTrieConfig::default())`, `HashHtj::<FxHashStrategy, NoPruning>::new(HasherChoice::Fxhash, PruningChoice::Off, config, …)`), replace the `singleton_pruning: true` configs with `load_factor: LoadFactor::percent(50).unwrap()` and the asserted axis with `ds_config_load_factor == 0.5`, and add:

```rust
    #[test]
    fn pruned_family_reports_the_pruning_layout() {
        let family = HashHtj::<kermit_iters::SipHashStrategy, SingletonPruning>::new(
            HasherChoice::Sip, PruningChoice::On, HashTrieConfig::default(), Optimiser::Lexicographic,
        );
        let header = RelationHeader::new("r", vec!["a".to_string(), "b".to_string()]);
        let rel = family.build_relation(header, vec![vec![1, 2]]);
        assert_eq!(
            HashHtj::<kermit_iters::SipHashStrategy, SingletonPruning>::optimization_axes(&rel)
                .get("ds_layout_pruning"),
            Some(&serde_json::Value::String("on".into()))
        );
    }
```

- [ ] **Step B3: `kermit/tests/join_tests.rs`**

```rust
mod common;

use {
    kermit_algos::{CardinalityOptimiser, HashTriejoin, LeapfrogTriejoin, LexicographicOptimiser},
    kermit_ds::{define_config_provider, ColumnTrie, HashTrie, HashTrieConfig, LoadFactor, SingletonPruning, TreeTrie},
    kermit_iters::{FxHashStrategy, SipHashStrategy},
};

// ── Layout aliases: hasher × pruning ────────────────────────────────────
type HashTrieSip = HashTrie<SipHashStrategy>;
type HashTrieFx = HashTrie<FxHashStrategy>;
type HashTrieSipPruned = HashTrie<SipHashStrategy, SingletonPruning>;
type HashTrieFxPruned = HashTrie<FxHashStrategy, SingletonPruning>;

define_multiway_join_test_suite!(TreeTrie, LeapfrogTriejoin, LexicographicOptimiser);
define_multiway_join_test_suite!(TreeTrie, LeapfrogTriejoin, CardinalityOptimiser);

define_multiway_join_test_suite!(ColumnTrie, LeapfrogTriejoin, LexicographicOptimiser);
define_multiway_join_test_suite!(ColumnTrie, LeapfrogTriejoin, CardinalityOptimiser);

define_multiway_join_test_suite!(HashTrieSip, HashTriejoin, LexicographicOptimiser);
define_multiway_join_test_suite!(HashTrieSip, HashTriejoin, CardinalityOptimiser);
define_multiway_join_test_suite!(HashTrieFx, HashTriejoin, LexicographicOptimiser);
define_multiway_join_test_suite!(HashTrieFx, HashTriejoin, CardinalityOptimiser);
define_multiway_join_test_suite!(HashTrieSipPruned, HashTriejoin, LexicographicOptimiser);
define_multiway_join_test_suite!(HashTrieSipPruned, HashTriejoin, CardinalityOptimiser);
define_multiway_join_test_suite!(HashTrieFxPruned, HashTriejoin, LexicographicOptimiser);
define_multiway_join_test_suite!(HashTrieFxPruned, HashTriejoin, CardinalityOptimiser);

// ── Config axis: load factor ────────────────────────────────────────────
// The default-config invocations above are the `ds_config_load_factor: 0.7`
// baseline; these are the alternate (standard: baseline + ≥1 per flag).
define_config_provider!(HalfFull, HashTrieConfig, HashTrieConfig {
    load_factor: LoadFactor::percent(50).unwrap(),
});

define_multiway_join_test_suite_with_config!(HashTrieSip, HashTriejoin, LexicographicOptimiser, HalfFull);
define_multiway_join_test_suite_with_config!(HashTrieSip, HashTriejoin, CardinalityOptimiser, HalfFull);
define_multiway_join_test_suite_with_config!(HashTrieFx, HashTriejoin, LexicographicOptimiser, HalfFull);
define_multiway_join_test_suite_with_config!(HashTrieFx, HashTriejoin, CardinalityOptimiser, HalfFull);
```

In `cli_hash_trie_config_choice.rs`: delete the two `singleton_pruning` tests; `cli_bench_ds_rejects_ds_config_on_tree_trie` uses `load-factor=0.5`; `cli_bench_ds_rejects_unknown_config_key` uses `singleton-pruning=true` as the unknown key and asserts the message mentions it.

- [ ] **Step B4: Workspace checks**

Run in order: `CARGO_BUILD_JOBS=2 cargo build --workspace --all-targets`; `CARGO_BUILD_JOBS=2 cargo test --workspace`; `cargo clippy --workspace --all-targets -- -Dwarnings`; `RUSTDOCFLAGS=-Dwarnings cargo doc --workspace --no-deps`.
Expected: green; `join_tests` reports 132 + 44 = 176 (12 Layout suites × 11 + 4 config suites × 11).

- [ ] **Step B5: Commit B**

```bash
git add kermit/src/execution.rs kermit/src/main.rs kermit/tests/join_tests.rs kermit/tests/cli_hash_trie_config_choice.rs
git commit -m "feat(kermit)!: --ds-layout-pruning and the hasher × pruning Layout dispatch"
```

---

### Task 5: CLI smoke test for the pruning Layout

**Files:**
- Create: `kermit/tests/cli_hash_trie_layout_pruning.rs`

- [ ] **Step 1: Write the test** (copy `run_bench_ds` and `axes_of` from `cli_hash_trie_config_choice.rs`; the mirror convention keeps each smoke file standalone)

```rust
//! CLI smoke test: `--ds-layout-pruning on` records `ds_layout_pruning: "on"`,
//! the absent flag records `"off"`, and the flag is rejected on a structure
//! without the dimension. Mirrors `cli_hash_trie_hasher_choice.rs`.

// … helpers …

#[test]
fn cli_bench_ds_with_pruning_on_records_axis() {
    let (output, report) = run_bench_ds("hash-trie", &["--ds-layout-pruning", "on"]);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let axes = axes_of(&report);
    assert_eq!(axes["ds_layout_pruning"], "on", "{axes}");
    assert_eq!(axes["ds_layout_hasher"], "sip", "{axes}");
}

#[test]
fn cli_bench_ds_default_pruning_is_off() {
    let (output, report) = run_bench_ds("hash-trie", &[]);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(axes_of(&report)["ds_layout_pruning"], "off");
}

#[test]
fn cli_bench_ds_rejects_pruning_flag_on_tree_trie() {
    let (output, _) = run_bench_ds("tree-trie", &["--ds-layout-pruning", "on"]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("--ds-layout-pruning"), "{stderr}");
}

#[test]
fn cli_bench_run_sweep_carries_pruning_only_to_hash_trie_cells() {
    // `-i all -a all --ds-layout-pruning on` runs the three valid cells; the
    // sorted cells have no pruning axis and the hash cell reports "on".
    let report = NamedTempFile::new().expect("failed to create temp report file");
    let output = Command::new(kermit_bin())
        .args(["bench", "--sample-size", "10", "--measurement-time", "1", "--warm-up-time", "1"])
        .arg("--report-json")
        .arg(report.path())
        .args(["run", "triangle", "-i", "all", "-a", "all", "-m", "space", "--ds-layout-pruning", "on"])
        .output()
        .expect("failed to execute kermit binary");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let text = std::fs::read_to_string(report.path()).expect("report file should exist");
    let reports: Vec<serde_json::Value> =
        serde_json::from_str(&text).expect("report should be valid JSON array");
    assert_eq!(reports.len(), 3, "three valid cells: {text}");
    for r in &reports {
        let axes = &r["axes"];
        if axes["data_structure"] == "HashTrie" {
            assert_eq!(axes["ds_layout_pruning"], "on", "{axes}");
        } else {
            assert!(axes.get("ds_layout_pruning").is_none(), "{axes}");
        }
    }
}
```

The `triangle` benchmark's data is committed under `benchmarks/`, so no network is needed.

- [ ] **Step 2: Run** `CARGO_BUILD_JOBS=2 cargo test -p kermit --test cli_hash_trie_layout_pruning` → 4 passed.
- [ ] **Step 3: Commit** `test(kermit): CLI smoke test for --ds-layout-pruning`.

---

### Task 6: kermit-lab

**Files:**
- Modify: `python/kermit-lab/kermit_lab/defaults.py`, `tests/test_defaults.py`, `tests/conftest.py`, `tests/test_frame.py`, `tests/test_plot.py`, `tests/test_cli.py`, `tests/test_presets.py`, `tests/test_axis_mapping.py`, `tests/test_render_all.py`

- [ ] **Step 1: Failing tests.** In `test_defaults.py` replace `test_singleton_pruning_defaults_to_false` with:

```python
def test_pruning_layout_defaults_to_off() -> None:
    df = pd.DataFrame({"ds_layout_pruning": [pd.NA, "on"]})
    out = apply_axis_defaults(df)
    assert out["ds_layout_pruning"].tolist() == ["off", "on"]
    assert AXIS_DEFAULTS["ds_layout_pruning"] == "off"


def test_load_factor_defaults_to_seventy_percent() -> None:
    df = pd.DataFrame({"ds_config_load_factor": [pd.NA, 0.5]})
    out = apply_axis_defaults(df)
    assert out["ds_config_load_factor"].tolist() == [0.7, 0.5]
    assert AXIS_DEFAULTS["ds_config_load_factor"] == 0.7
    assert "ds_config_singleton_pruning" not in AXIS_DEFAULTS
```

- [ ] **Step 2: Run** `cd python/kermit-lab && uv run pytest tests/test_defaults.py -q` → fails.
- [ ] **Step 3: Implement.** `AXIS_DEFAULTS = {"ds_layout_hasher": "sip", "ds_layout_pruning": "off", "ds_config_load_factor": 0.7}` with one comment each. In `conftest.py`'s `fixture_opt_tree`, replace the `pruning in (True, False)` loop with `load_factor in (0.5, 0.7)`, tag `f"{hasher}-lf{int(load_factor * 100)}"`, axis `"ds_config_load_factor": load_factor`, and update its docstring. Rename the axis in `test_frame.py` (`{0.5, 0.7}` instead of `{True, False}`), `test_plot.py`, `test_cli.py`, `test_presets.py`, `test_axis_mapping.py` (`marker_for("ds_config_load_factor", 0.5)`), `test_render_all.py` (`ablation-ds_config_load_factor`).
- [ ] **Step 4: Run** `uv run pytest -q` → all pass.
- [ ] **Step 5: Commit** `feat(kermit-lab): ds_layout_pruning and ds_config_load_factor defaults; fixtures name real axes`.

---

### Task 7: docs and the standard

**Files:** `docs/data-structures/hash-trie.md`, `docs/specs/optimization-standard.md`, `docs/specs/bench-report-schema.md`, `CLAUDE.md`, `docs/specs/2026-09-08-singleton-pruning-config-design.md`

- [ ] **Step 1: `optimization-standard.md`.** Add a "How to classify" subsection under "The three categories" stating the shape / value / process rule from Amendment 1 with the non-user-tax test. Correct the Config example to the load-factor cap and the Layout examples to include singleton pruning and lazy expansion; update both tables (`Examples`: `Hasher choice ✓, singleton pruning ✓, pointer encoding, lazy expansion` under Layout; `Load-factor cap ✓, initial capacity, hash seed` under Config); the CLI section (`--ds-layout-pruning on`, `--ds-config load-factor=0.5`); the report JSON example (`ds_layout_pruning: "off"`, `ds_config_load_factor: 0.7`); the prefix table example; the `ConfigOption` and `HasOptimizationAxes` code blocks (real `LoadFactor` code); the walkthrough (rewrite around the load factor, and add a second short walkthrough "adding a Layout dimension" around pruning with the uninhabited-payload trick and `with_hash_trie_layout!`); the ablation pandas examples (`ds_config_load_factor`); "Where to look" rows for `pruning.rs`; the status table (Layout: hasher, pruning; Config: load factor; BuildMode: none). Every remaining `hypothetical` must be BuildMode-only.
- [ ] **Step 2: `hash-trie.md`.** Representation: `Singleton(P::Payload)` with a note that it is uninhabited under `NoPruning`; iterator paragraph: frame type chosen by `P`; invariant bullet reworded for the Layout; a "Layout options" entry for pruning (CLI `--ds-layout-pruning`, aliases, axis values, the measured pruning-on effect from the spec, and the acceptance result from Task 9) and a "Config flags" entry for the load factor (CLI, axis, default, the space-vs-time expectation); replace the "10 % overhead" sentence with: "The `off` instantiation compiles to the pre-pruning code; the measured parity is recorded in the design spec's Amendment 1 acceptance record."; deferred follow-ups list lazy expansion as a Layout candidate.
- [ ] **Step 3: `bench-report-schema.md`.** Update the `ds_config_<flag>` example to `ds_config_load_factor` and add `ds_layout_pruning` beside `ds_layout_hasher`.
- [ ] **Step 4: `CLAUDE.md`.** Build Commands lines (`--ds-layout-pruning on`, `--ds-config load-factor=0.5`); the "Adding an optimization" step 1 cites the shape/value/process rule; the Config gotcha is rewritten (load factor is passed into the resize test; pruning is a Layout with an uninhabited off payload; `with_hash_trie_layout!` is where Layout dimensions multiply); Testing Patterns bullet for `with_config` names `HalfFull`.
- [ ] **Step 5: Spec status.** Set the design spec's status line to "Amendment 1 implemented" naming the commit range; the acceptance numbers are added by Task 10 after Task 9 measures them.
- [ ] **Step 6: Verify** with `RUSTDOCFLAGS=-Dwarnings cargo doc --workspace --no-deps` (unchanged code, but doc links) and `grep -n "singleton_pruning\|singleton-pruning\|PruningOn" docs CLAUDE.md` → only historical mentions inside the design spec's Sections 1–6.
- [ ] **Step 7: Commit** `docs: pruning as a Layout, load factor as the Config; classification rule in the standard`.

---

### Task 8: full verification

- [ ] `nix develop --command cargo fmt --all`, then `nix develop --command cargo fmt --all -- --check` twice (idempotence); commit `style: cargo fmt` if anything changed.
- [ ] `CARGO_BUILD_JOBS=2 cargo build --workspace --all-targets`; `CARGO_BUILD_JOBS=2 cargo test --workspace`; `cargo clippy --workspace --all-targets -- -Dwarnings`; `RUSTDOCFLAGS=-Dwarnings cargo doc --workspace --no-deps`; `nix develop --command bash -c 'MIRIFLAGS=-Zmiri-disable-isolation cargo miri test -p kermit-iters -p kermit-ds'` then `… -p kermit-algos`. All foreground, one per command.

---

### Task 9: performance acceptance (Amendment 1, Section D)

Protocol from `docs/specs/2026-09-08-singleton-pruning-config-design.md` Section 2 and the memory `project-perf-comparison-recipe`:

- [ ] Export the baseline `9c67ee5` with `git archive` into the scratchpad and build it `--release` (`CARGO_BUILD_JOBS=2`); build this branch `--release`.
- [ ] Three interleaved rounds, foreground, direct binaries, `oxford-uniform-s3`, `-m iteration`, queries `binary-join` and `triangle`, each with a distinct `--name`: baseline hash-trie; branch hash-trie `--ds-layout-pruning off`; branch `--ds-layout-pruning on`; branch `--ds-config load-factor=0.5` and `0.9`; baseline and branch tree-trie as the control.
- [ ] Also one `-m space` and one `-m insertion` run for `off`, `on`, `0.5`, `0.9`.
- [ ] Acceptance: `off` within the TreeTrie control's spread of the baseline on both queries (expected ≈ 60 µs and ≈ 2.1 ms); `on` reproduces −56 % / −22 % space and ≈ −9 % insertion; `0.5` vs `0.9` move space and iteration in opposite directions. Record the table in the spec (Task 7 Step 5) and the hash-trie doc.
- [ ] If `off` is not at parity, stop: report the numbers and the isolating experiments (HEAD with `Frame` collapsed vs not) rather than tuning further.

---

### Task 10: issue and memory

- [ ] Add an "Amendment 1 acceptance record" subsection to the design spec and the measured numbers to `hash-trie.md`'s pruning entry; commit `docs: record Amendment 1 acceptance measurements`.
- [ ] Comment on #58: the reclassification, the rule, the two axes now live (`ds_layout_pruning`, `ds_config_load_factor`), and the acceptance numbers.
- [ ] Update the memory `singleton-pruning-branch` to "resolved" with the outcome.
