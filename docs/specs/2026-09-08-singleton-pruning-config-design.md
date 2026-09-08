# Singleton Pruning as the First Config Consumer

**Status:** Sections 1–6 are implemented on `aidanb/optimisations`
(commits `77dffa2`..`f17c812`). **Amendment 1 implemented** (commits
`d711043`..`185b386` plus the docs commit); acceptance record pending
Task 9/10 of `docs/plans/2026-09-08-pruning-layout-load-factor-config.md`.
Amendment 1 supersedes the classification of pruning; read it first.
**Resolves:** [#58](https://github.com/aidan-bailey/kermit/issues/58) — the optimization standard has one adopter; Config and BuildMode are unexercised.
**Paper:** SIGMOD 2020 "Combining Worst-Case Optimal and Traditional Binary Join Processing", §3.3.1, Figure 5.

---

## Goal

Land singleton pruning on `HashTrie` as a **Config** optimization, together
with the config-injection seam and the `define_multiway_join_test_suite_with_config!`
macro that `docs/specs/optimization-standard.md` prescribes but never
implemented. This exercises the Config half of the standard end-to-end: Rust
shape → CLI flag → bench-report axis → kermit-lab pivot → test obligation.

The BuildMode half stays unexercised. Its macro is not written here; it lands
with the first BuildMode consumer.

## Decisions taken during brainstorming

| Decision | Choice | Rejected alternatives |
|---|---|---|
| Resolution of #58 | Land a Config consumer (issue option 1) | Downgrade the spec; a generic mechanism with no consumer; a BuildMode consumer first |
| Config injection channel | Additive `ConfigurableRelation` trait + `Configured<R, P>` wrapper | Extending the core `Relation` trait (touches every sibling for one adopter); lifting config to a type parameter (turns Config into Layout) |
| Algorithm awareness | Pruning is transparent through `HashTrieIterator`; `HashTriejoin` untouched | Paper-faithful skip-levels short-circuit (changes trait + algorithm; deferred as the first `algo_config_*` candidate) |
| Singleton definition | Exactly one tuple (paper-faithful); duplicates and collisions unprune | One hash path (invariant would move with the Layout parameter) |
| *(Amendment 1)* Category of pruning | **Layout** (`HashTrie<H, P>`; the off instantiation compiles to the pre-pruning code) | Keep as Config and accept ~10 % on every pruning-off run; reshape the iterator frame to recover ~5 % |
| *(Amendment 1)* First Config consumer | **Load-factor cap** (`ds_config_load_factor`, default 0.7) | Initial-capacity hint; hash seed (both remain candidates) |

---

## Amendment 1 (2026-09-08): pruning is a Layout; the load-factor cap is the Config

### Why

The acceptance check in Section 5 failed: with pruning *off*, hash-join
iteration on `oxford-uniform-s3` is ~10 % slower than the pre-pruning
`HashTrie` (Section 2, "Measured"). Isolating builds attribute ~4 % to the
third `HashTrieNode` variant and ~5–6 % to the two-variant iterator frame.
That is the signature of a *shape* knob encoded as a *value* knob: every
non-user pays for the branches that make the shape possible.

Applying the standard's own aspect table to pruning's nature, not to our
code: it is fixed at construction (no runtime switching), it changes which
nodes exist (a representation invariant), and its results differ from the
default (so not a BuildMode). The paper's Figure 5 presents it as a
physical layout alongside pointer tagging. It is a Layout. The standard's
Config example was wrong, and its other example (lazy expansion) is wrong
for the same reason: an unexpanded node is a node state and needs a cell in
the node type that eager tries would carry for nothing.

The discriminating rule this amendment adopts, to be written into the
standard:

- **Layout** = a *shape*: which variants, encodings, or hash functions
  exist. Zero cost for non-users, by monomorphisation.
- **Config** = a *value* read on a path the code already takes: a
  threshold, a capacity, a seed. Replacing a constant adds no branch.
- **BuildMode** = a *process* yielding the same shape.

Under that rule none of the paper's seven optimisations is a Config. The
honest Config consumers on `HashTrie` are its tuning constants; the
load-factor cap (a compile-time 7/10 in `hash_table.rs`) is the smallest.

### A. Pruning as a second Layout dimension

**Type.** `HashTrie<H: HashStrategy = SipHashStrategy, P: PruningPolicy = NoPruning>`.
`PruningPolicy: LayoutOption` with two zero-sized implementors,
`NoPruning` (`NAME = "off"`) and `SingletonPruning` (`NAME = "on"`). Axis
`ds_layout_pruning: "off" | "on"`; kermit-lab back-fills `"off"`.

**Zero cost for `NoPruning`, by construction.** The policy carries the
singleton payload as an associated type, and the off policy's payload is
uninhabited:

```rust
pub trait PruningPolicy: LayoutOption {
    /// What a `HashTrieNode::Singleton` holds. `Vec<usize>` when pruning
    /// is on; an uninhabited type when it is off, so every `Singleton`
    /// arm is dead code in that instantiation.
    type Payload: SingletonPayload;
    const ENABLED: bool;
}

pub trait SingletonPayload {
    fn from_tuple(tuple: Vec<usize>) -> Self;
    fn tuple(&self) -> &Vec<usize>;
    fn into_tuple(self) -> Vec<usize>;
}

pub enum Never {}   // impl SingletonPayload for Never { … match *self {} … }
```

`HashTrieNode<P>` becomes `Inner(HashTable<HashTrieNode<P>>) | Leaf(…) | Singleton(P::Payload)`.
With `P = NoPruning` the `Singleton` arm in every accessor is
`| Singleton(never) => match *never {}` and the enum has two inhabited
variants, so the accessor matches and the enum layout are exactly the
pre-pruning ones. `insert_at` gates the prune/unprune paths on
`P::ENABLED`, a `const` the compiler folds. The iterator's frame does the
same: `Frame::Singleton(P::Frame)` with `NoPruning::Frame = Never`, so the
off iterator's frame collapses to the old `(node, idx)` pair. `Singleton`
frames for `SingletonPruning` keep Section 2's one-entry-table emulation.

**Invariant** is unchanged: with `SingletonPruning`, a child is `Singleton`
iff exactly one tuple lives below it; with `NoPruning` no `Singleton` can
be constructed (its payload has no values).

**What carries over from Sections 1–2 unchanged:** the prune and unprune
logic in `insert_at`, `new_table`, `collect_at`, `node_heap_bytes`, the
`Descent` model, all emulation semantics, and every pruning test (they move
to the `SingletonPruning` alias). The `#[cold]`/`#[inline]` attributes from
`94fcc5e` stay; they are harmless and protect the on instantiation.

**CLI.** `LayoutChoices` gains `--ds-layout-pruning <off|on>` (a
`PruningChoice` `ValueEnum`, default `off`), rejected on non-hash-trie
selectors by `validate_layout_choices` like the hasher flag.
`Execution::HashHtj { hasher, pruning, config }`; `dispatch_run_bench` and
`dispatch_ds_bench` monomorphise over the 2 × 2 (hasher × pruning) cells.
This is the first real test of the design doc's claim that one flag per
Layout dimension scales; if the four-arm match repeats itself, a small
`for_each_hash_trie_layout!` macro that expands the product is the
expected shape, not a runtime enum.

**Tests.** Four Layout aliases (`HashTrieSip`, `HashTrieFx`,
`HashTrieSipPruned = HashTrie<SipHashStrategy, SingletonPruning>`,
`HashTrieFxPruned`) each run `define_multiway_join_test_suite!` under both
optimisers, replacing the four `with_config` invocations for pruning. The
DS-level suites run on all four plus `HashTrieMod10Pruned`. Add a
compile-time guard that `size_of::<HashTrieNode<NoPruning>>()` equals
`size_of::<HashTrieNode<SingletonPruning>>()` (a `Vec` payload is smaller
than a `HashTable`, so the enum must not grow) and a
unit test that `NoPruning` never produces a `Singleton` (the invariant
walker with `prune = false`, retained).

**CLI smoke test.** `cli_hash_trie_layout_pruning.rs` (mirror of the hasher
smoke test): `--ds-layout-pruning on` records `ds_layout_pruning: "on"`,
absent records `"off"`, and the flag is rejected on `tree-trie`.

### B. Load-factor cap as the first Config consumer

**Value.** `HashTrieConfig { load_factor: LoadFactor }` replaces
`singleton_pruning`. `LoadFactor` is an exact fraction with a percent
denominator (`LoadFactor::percent(70)`, default), validated to `1..=99`,
so the resize test stays integer arithmetic exactly as today. Axis
`ds_config_load_factor`: the JSON number `0.7`; kermit-lab back-fills
`0.7`.

**Where it is read.** The existing comparison in
`HashTable::entry_or_insert_with`, `(len + 1) * DEN > cap * NUM`, with
`NUM`/`DEN` supplied by the caller instead of two `const`s. The cap is
passed down from `HashTrie` (which holds the config) through `insert_at`
into the table call; no per-table storage, so space is unchanged and the
insert path gains no branch. This is the property that makes it a Config:
non-users pay nothing, verified by the acceptance check below.

**CLI.** `--ds-config load-factor=<0.01..0.99>`; `singleton-pruning` is
removed from `HASH_TRIE_KEYS`. A value outside the open unit interval is a
usage error naming the range.

**Tests.** `define_config_provider!(LoadFactor50, …)` and `LoadFactor90`;
`define_multiway_join_test_suite_with_config!` on `HashTrieSip` and
`HashTrieFx` under both optimisers with `LoadFactor50` (the baseline
`Configured` runs stay for the default). DS-level suites on
`Configured<HashTrieSip, LoadFactor90>` (dense tables stress the probe
loops). Unit tests: a table resizes exactly when `(len + 1)` crosses the
configured cap (parametrised over 50, 70, 90); `heap_size_bytes` is
monotone non-increasing in the cap for a fixed tuple set; the CLI rejects
`0`, `1`, `1.5`, and `abc`. The `cli_hash_trie_config_choice.rs` smoke test
switches to `load-factor=0.5` and asserts the axis value `0.5`.

**Expected thesis result.** Space and iteration move in opposite
directions along this axis, and FxHash's poorer distribution on structured
keys should make it more sensitive to a high cap than SipHash — the first
2 × 2 (`ds_layout_hasher × ds_config_load_factor`) where the two metrics
disagree.

### C. Migration from the landed implementation

Keep unchanged: `ConfigurableRelation`, `Configured<R, P>`,
`define_config_provider!`, `define_multiway_join_test_suite_with_config!`,
`ConfigChoices` and its validation, `ExecutionFamily::build_relation` /
`load`, `read_csv` / `read_parquet`, the report-discipline tests.

Change: `HashTrieConfig`'s field; `HashTrie`, `HashTrieNode`,
`HashTrieIter` gain `P`; `hash_trie/mod.rs` exports the policy markers;
`kermit_iters::hash_strategy` gains nothing (the policy lives beside
`HashTrie` in `kermit-ds`, since only it has a meaning there — note this
differs from `HashStrategy`, which lives in `kermit-iters` because
`SingletonHashTrieIter` needs it); `LayoutChoices`, `Execution`, both
dispatchers; all pruning tests re-homed to the Layout alias; the
`with_config` invocations re-pointed at the load factor; kermit-lab
defaults (`ds_layout_pruning: "off"`, `ds_config_load_factor: 0.7`; delete
`ds_config_singleton_pruning`); the synthetic fixture axis in
`python/kermit-lab/tests` renamed to `ds_config_load_factor` so the
examples name a real axis.

Docs: `docs/data-structures/hash-trie.md` moves pruning under Layout
options and adds the load factor under Config flags;
`docs/specs/optimization-standard.md` adopts the shape/value/process rule,
corrects both Config examples, and updates its tables and walkthrough;
`CLAUDE.md`'s optimisation recipe cites the rule; the #58 comment is
amended with the reclassification.

### D. Acceptance

Same protocol as Section 2's measurements (`oxford-uniform-s3`,
`iteration`, three interleaved rounds against `9c67ee5`, TreeTrie control),
recorded in the spec:

1. `HashTrie<Sip, NoPruning>` with the default load factor is within the
   control's spread of the baseline on `binary-join` and `triangle`. This
   is the claim the amendment exists to make true; if it fails, the
   uninhabited-payload encoding did not collapse and the plan stops.
2. `HashTrie<Sip, SingletonPruning>` reproduces Section 2's pruning-on
   numbers (space −56 % / −22 %, insertion ~−9 %).
3. `load-factor=0.7` is indistinguishable from the constant; `0.5` and
   `0.9` move space and iteration in opposite directions.

### Out of scope for the amendment

Unchanged from the original list, plus: any other tuning value (initial
capacity, seed) and any change to how `Configured` or the CLI config
parser work.

---

## Sections 1–6: the implemented Config design (superseded in part)

The sections below record what landed. Section 3's `HashTrieConfig`
field and every `ds_config_singleton_pruning` reference are superseded by
Amendment 1; everything else stands.

## Section 1: the data structure

### New node variant

```rust
pub(crate) enum HashTrieNode {
    Inner(HashTable<HashTrieNode>),
    Leaf(HashTable<Vec<Vec<usize>>>),
    /// Pruned subtrie: exactly one tuple lives below this point.
    Singleton(Vec<usize>),
}
```

A `Singleton` may appear anywhere an `Inner` table holds a child (depths
`1..arity`). The root is always a table, so the empty-stack `open()` path is
unchanged.

**Invariant (stated once, tested directly):** a subtrie is `Singleton` iff
`config.singleton_pruning` is on and the subtrie holds exactly one tuple. The
final shape is independent of insertion order.

`HashTrieNode` does not grow: `Vec<usize>` is smaller than `HashTable`, so
the enum size is unchanged.

### Insert

`HashTrie::insert_at` gains the config as a parameter (it is an associated fn).

- Pruning **off**: the existing code path runs untouched. The default
  configuration is byte-for-byte today's structure.
- Pruning **on**, empty `Inner` bucket: store `Singleton(tuple)` instead of
  allocating a child table and recursing.
- Pruning **on**, bucket holds `Singleton(old)`: **unprune**. Take `old` out,
  replace the node with a fresh `Inner` or `Leaf` for that depth, then
  recursively insert `old` and the new tuple. Recursion handles everything
  below: tuples that diverge one level down become two singletons; tuples
  that share hashes unprune again; a duplicate or a full collision bottoms
  out in a leaf chain of two.

### Other node walkers

- `collect_at`: push the singleton's tuple.
- `node_heap_bytes`: count `tuple.capacity() * size_of::<usize>()`.
- `Projectable::project`: propagate `self.config` into the projected trie.
- Table-only accessors on `HashTrieNode` (`buckets_len`, `next_occupied`,
  `hash_at`, `index_of`, `len`): gain `Singleton` arms that panic naming the
  invariant. The iterator (Section 2) never places a singleton in a table
  frame, so this is an internal-invariant panic of the kind the codebase
  already accepts (cf. the LFTJ `open`-after-`at_end` discipline).

## Section 2: the iterator

The stack entry in `HashTrieIter` becomes a two-variant frame:

```rust
enum Frame<'a> {
    /// Today's `(node, bucket)` pair. `node` is always `Inner` or `Leaf`.
    Table { node: &'a HashTrieNode, idx: usize },
    /// Emulates the one-entry table a pruned level would have held.
    /// `hash == H::hash(tuple[depth])`, computed once when pushed.
    Singleton { tuple: &'a Vec<usize>, depth: usize, hash: u64, exhausted: bool },
}
```

Behaviour on a `Singleton` frame mirrors a one-entry table:

| Method | Behaviour |
|---|---|
| `key()` | `Some(hash)` unless exhausted |
| `next()` | set `exhausted`, return `None` |
| `lookup(h)` | `h == hash` → un-exhaust, `true`; else exhaust, `false` |
| `size()` | `1` |
| `at_end()` | `exhausted` |
| `open()` | if `depth + 1 < arity` and not exhausted: push `Singleton` at `depth + 1`, return `true`; else `false` |
| `up()` | pop (unchanged) |
| `leaf_tuples()` | at `depth + 1 == arity` and not exhausted: `Some(slice::from_ref(tuple))`; else `None` |

`open()` from a `Table` frame whose current bucket holds a `Singleton` pushes
a `Singleton` frame at `stack.len()` (the new depth). Arity comes from the
trie's header, which the iterator already borrows. `H` is already the
iterator's type parameter.

`HashTriejoin`, `SingletonHashTrieIter`, and the `HashTrieIterator` trait
are untouched.

### Performance expectations (predicted) and measurements (actual)

**Predicted, pruning off**: one predictable branch per inserted attribute
and one extra predictable branch per iterator call; expected to be inside
Criterion noise.

**Measured, pruning off** (2026-09-08, `oxford-uniform-s3`, `iteration`
metric, three interleaved rounds of the parent commit `9c67ee5` against
the implementation; TreeTrie/LFTJ on the same two binaries as a control
was flat at ~37 µs both sides):

| Query | baseline | this branch | overhead |
|---|---|---|---|
| binary-join | ~59.7 µs | ~65.5 µs | +10 % |
| triangle | ~2.10 ms | ~2.33 ms | +10 % |

The prediction was wrong at this scale: the whole join costs ~15 ns per
tuple, so one extra predictable branch per iterator call is visible. Two
isolating builds attribute the 10 % as roughly **4 % to the third
`HashTrieNode` variant** (three-arm accessor matches; the pre-Task-4
iterator on top of the new node type measures ~+4 %) and **5–6 % to the
two-variant `Frame` in `HashTrieIter`**. Before commit `94fcc5e` the total
was 15–22 %: the diverging panic helper in the accessors had stopped LLVM
inlining them into the probe loops; `#[cold]` on the helper and
`#[inline]` on the accessors recovered the rest. Insertion and space are
unchanged with pruning off.

**Measured, pruning on** (same protocol): space of the arity-3 relations
R/S/T falls from ~934 KiB to ~409 KiB (−56 %) and of the arity-2 U/V/W
from ~52 KiB to ~41 KiB (−22 %); unary P/Q are unchanged (no level to
prune). Insertion is ~8–10 % faster (4.5 → 4.05 ms). Iteration lands
between the two pruning-off numbers: triangle ~1.92 ms against the
branch's 2.24 ms and the baseline's 1.86 ms in the first run; binary-join
is level with pruning-off on this branch.

**Decision.** The acceptance check in Section 5 said an overhead outside
Criterion noise means the design is revisited before merge. The remaining
10 % is inherent to encoding a shape as a runtime value. Resolved by
**Amendment 1**: pruning becomes a Layout dimension whose off
instantiation compiles to the pre-pruning code, and the load-factor cap
becomes the Config consumer.

The Sip-vs-Fx hypothesis (pruning helps more under FxHash) is still
untested; it needs the 2×2 pivot in kermit-lab.

## Section 3: config plumbing

### `HashTrieConfig`

`kermit-ds/src/ds/hash_trie/config.rs`:

```rust
#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
pub struct HashTrieConfig {
    pub singleton_pruning: bool,
}

impl ConfigOption for HashTrieConfig {
    fn axes(&self) -> Vec<(&'static str, Value)> {
        vec![("singleton_pruning", self.singleton_pruning.into())]
    }
}
```

`HashTrie<H>` gains a `config: HashTrieConfig` field.

### `ConfigurableRelation` — the extension seam

New trait in `kermit-ds/src/relation.rs`:

```rust
pub trait ConfigurableRelation: Relation {
    type Config: ConfigOption;
    fn with_config(header: RelationHeader, config: Self::Config) -> Self;
    fn from_tuples_with_config(
        header: RelationHeader, config: Self::Config, tuples: Vec<Vec<usize>>,
    ) -> Self;
    fn config(&self) -> &Self::Config;
}
```

Only `HashTrie<H>` implements it. `Relation::new` becomes
`with_config(header, HashTrieConfig::default())` and `Relation::from_tuples`
becomes `from_tuples_with_config(header, default, tuples)`, so existing
behaviour is unchanged. `TreeTrie` and `ColumnTrie` are not touched
(Priorities item 6).

### `Configured<R, P>` — lifting a config value to a type

New file `kermit-ds/src/configured.rs`, exported at the crate root as
`kermit_ds::{Configured, ConfigProvider}`:

```rust
pub trait ConfigProvider<C> {
    fn config() -> C;
}

pub struct Configured<R, P>(R, PhantomData<P>);
```

`Configured<R, P>` where `R: ConfigurableRelation`, `P: ConfigProvider<R::Config>`
delegates `Relation`, `Projectable`, `HeapSize`, `Cardinality`,
`HashTrieIterable`, `TrieIterable`, and `HasOptimizationAxes` to the inner
`R`. Its `Relation::new` / `from_tuples` route through `P::config()`.
`RelationFileExt` arrives for free via the blanket impl over `Relation`.

This exists so macro-generated test suites can paste a type name. The DS
itself never sees a marker type; its config stays a runtime value, which is
what makes it Config rather than Layout.

### Axes

`HasOptimizationAxes for HashTrie<H>` grows exactly as the standard's
example shows:

```rust
axes.insert("ds_layout_hasher".into(), H::NAME.into());
for (suffix, value) in self.config.axes() {
    axes.insert(format!("ds_config_{suffix}"), value);
}
```

Emitted key: `ds_config_singleton_pruning: bool`. This is the name
`python/kermit-lab/tests/` already uses as its canonical Config example.

## Section 4: CLI and bench dispatch

### Flag

A `ConfigChoices` clap group beside `LayoutChoices` in `kermit/src/main.rs`:

```
--ds-config <key>=<value>[,<key>=<value>...]
```

Parsing is two-stage:

1. Generic split into `(key, value)` string pairs (clap `value_delimiter = ','`).
2. A HashTrie-specific resolver accepting `singleton-pruning=true|false`.
   Unknown keys or malformed values are a usage error naming the accepted
   keys.

As with `--ds-layout-hasher`, passing `--ds-config` on a non-`hash-trie`
selector (other than `all`) is a usage error via `validate_config_choices`,
mirroring `validate_layout_choices`. A report can never carry a config axis
for a structure that ignored it.

Wired on `bench run` and `bench ds`, the two subcommands that carry `--ds-layout-hasher`; `join` / `bench join` take neither flag family.

### Dispatch

- `Execution::HashHtj(HasherChoice)` becomes
  `Execution::HashHtj { hasher: HasherChoice, config: HashTrieConfig }`.
  The enum stays `Copy`.
- `HashHtj<H>` (the `ExecutionFamily` impl in `kermit/src/execution.rs`)
  stores the config; `build_from_tuples` calls `from_tuples_with_config`.
- `bench ds` loads straight from a file. The CSV and Parquet readers inside
  the blanket `RelationFileExt` impl are factored into two public helpers in
  `kermit-ds/src/relation.rs` returning `(RelationHeader, Vec<Vec<usize>>)`;
  the blanket impl calls them, and `run_ds_bench_hash` loads tuples once and
  builds with the config.
- The report's `ds_config_*` axes come from `HasOptimizationAxes` on the
  relation that was actually built, matching how the layout axis works.

## Section 5: tests

### Join-level macros (`kermit/tests/common/macros.rs`)

```rust
define_config_provider!(PruningOn, HashTrieConfig, HashTrieConfig { singleton_pruning: true });

define_multiway_join_test_suite_with_config!(HashTrieSip, HashTriejoin, LexicographicOptimiser, PruningOn);
```

- `define_config_provider!(Name, ConfigType, expr)` declares the zero-sized
  marker and its `ConfigProvider<ConfigType>` impl.
- `define_multiway_join_test_suite_with_config!(Relation, Algo, Optimiser, Provider)`
  pastes `type [<$Relation $Provider>] = Configured<$Relation, $Provider>;`
  and invokes `define_multiway_join_test_suite!` on the alias.

`kermit/tests/join_tests.rs` adds the `PruningOn` provider and four
invocations (`HashTrieSip`/`HashTrieFx` × `Lexicographic`/`Cardinality`).
The existing four HashTrie invocations remain as the pruning-off baseline.

### DS-level suites (`kermit-ds/tests/hash_trie_tests.rs`)

`hash_trie_test_suite!` and `parquet_test_suite!` run on `Configured`
aliases for `Sip`, `Fx`, and the colliding `Mod10` strategy with pruning
on. The `Mod10` case exercises unprune on a full hash collision (two distinct
tuples sharing every hash must end in a leaf chain of two).

### Unit tests (`kermit-ds`)

- **Pruning invariant:** after `from_tuples`, every subtrie with one tuple is
  `Singleton` and none with more is. Checked by a test-only walker.
- **Unprune paths:** second tuple diverging at the next level; sharing hashes
  down to the leaf; exact duplicate. Each asserts on `collect_tuples` and on
  node shapes.
- **Order independence:** two insertion orders of the same tuples give equal
  `heap_size_bytes` and `collect_tuples`.
- **Space:** pruning on gives strictly smaller `heap_size_bytes` than off for
  a relation with sparse fan-out.
- **Iterator emulation:** on singleton frames, `key`, `next`, `lookup` hit
  and miss, `size`, `at_end`, `open` to the last level, `leaf_tuples`
  one-element slice.
- **Propagation:** `project` preserves config; `optimization_axes` emits
  both keys with the config's value.

### CLI smoke test

`kermit/tests/cli_hash_trie_config_choice.rs`, mirroring
`cli_hash_trie_hasher_choice.rs`: `--ds-config singleton-pruning=true`
yields a report with the axis `true`; absent flag yields `false`; the flag
on `tree-trie` fails; an unknown key fails.

### Performance acceptance check (not a cargo test)

Run `bench run triangle -i hash-trie -a hash-triejoin` with the default
config on the parent commit and on the implementation commit. Compare with
`kl.bootstrap_ratio_ci`. Expected: the interval includes 1. If it excludes
1, Section 2's overhead claim is wrong and the design is revisited before
merge.

*Outcome:* the interval excluded 1 (see Section 2, "Measured"); `triangle`
alone was too small to trust (8 tuples, ~1.6 µs per join), so the
measurement moved to `oxford-uniform-s3` with a TreeTrie control. The
resolution is Amendment 1.

### kermit-lab

Add `"ds_config_singleton_pruning": False` to `AXIS_DEFAULTS` in
`python/kermit-lab/kermit_lab/defaults.py` (pre-standard runs had no
pruning, so the back-fill is semantically true). No other change; the
fixtures and tests already use this axis.

## Section 6: docs

- `docs/data-structures/hash-trie.md`: `Singleton` in the representation
  block; the pruning invariant; the iterator frame model; the Config flags
  entry filled in (CLI, axis, values, the Sip-vs-Fx hypothesis); the
  skip-levels short-circuit under a deferred-follow-ups note as the first
  `algo_config_*` candidate.
- `docs/specs/optimization-standard.md`: walkthrough step 7 replaced with
  the real macros; `ConfigurableRelation` and `Configured` added to the
  trait-family and where-to-look sections; status table updated.
- `CLAUDE.md`: Priorities item 1 sentence about the not-yet-implemented
  `with_config` macro updated; the BuildMode macro still noted as pending a
  consumer. The "Adding an optimization" recipe gains the
  `ConfigurableRelation` step.
- Closing comment on #58 pointing at this spec.

## Out of scope

- Lazy child expansion (needs interior mutability through `&self` probes).
- Pointer tagging (a second Layout dimension).
- Parallel build and radix partitioning (BuildMode) and their macro.
- The paper's skip-levels short-circuit (algorithm-side Config).
- Any change to `TreeTrie`, `ColumnTrie`, `HashTriejoin`, or the
  `HashTrieIterator` trait.
