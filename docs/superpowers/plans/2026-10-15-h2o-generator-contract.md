# H2O Generator (#95, push 2): Names Contract

This contract binds the interface for push 2's plan (`2026-10-15-h2o-generator.md`). Every
drafted story takes its names, signatures, paths, behaviour and pinned values from here. A
decision this contract doesn't make goes back to the design session; the drafter doesn't
make it.

- **Spec:** `docs/specs/2026-10-15-h2o-benchmark-design.md` (Part 2, at bbaef2e), and its
  Amendment 2 (this push).
- **Layer contract (landed, frozen):** `2026-10-15-generate-layer-contract.md` (bbaef2e).
  The root re-exports are `kermit_generate::{Generator, Generated, GeneratedMeta,
  RelationSink, RelationWriter, RelationRecord, run, output_dir, OutputDir}`. `Target` is
  `kermit_rdf::generator::Target { out_dir, bench_name, tag, spec_hash }` (`Copy`, `Debug`).
  `run` requires the last component of `out_dir` to equal `bench_name`, and one term per
  column in every body atom.
- **Base: SNAP phase 2 lands first.** Push 2 executes on `aidanb/h2o` after merging an
  `origin/master` that contains #96's phase 2. SNAP's phase 2 carries `GenTarget` (verbatim
  from the spec, 1e717b4 on `aidanb/snap`) and the materialize `{e:#}` fix with its test
  (4cc9518). Push 2 adds neither, and uses `GenTarget` as SNAP's `bench gen snap` does.
  Drafters quote the post-SNAP tree from `aidanb/snap` (74668ed): read with
  `git show aidanb/snap:<path>`, and check with
  `plan_premises.py --rev aidanb/snap`. Files where the base matters:
  `kermit-bench/src/{definition,lib}.rs`, `kermit-generate/src/lib.rs`,
  `kermit/src/{main,materialize}.rs`, `CLAUDE.md`, `ARCHITECTURE.md`, `README.md`,
  `USAGE.md`, `benchmarks/README.md` and `docs/benchmarks/README.md`. If push 2 has to land
  first after all, the plan comes back here to be re-based. It is never improvised.

## Story order (every commit builds and passes its story's tests)

| Story | Files | What |
|---|---|---|
| 1 | `kermit-generate/src/lib.rs`, `src/h2o/mod.rs`, `src/h2o/rng.rs`, `src/h2o/keys.rs` | random numbers and key generation |
| 2 | `kermit-bench/src/definition.rs`, `kermit-bench/src/lib.rs` | `H2oOrder`, `H2O_MIN_SCALE`, `DEFAULT_H2O_SEED` (types only, no `GeneratorSpec` change) |
| 3 | `kermit-generate/Cargo.toml`, `src/h2o/mod.rs`, `tests/h2o.rs` | the generator |
| 4 | `kermit-bench/src/definition.rs`, `kermit/src/materialize.rs` | `GeneratorSpec::H2o`, its `validate_generator` arm and the `dispatch` arm, in one commit (both matches are exhaustive), plus the cache test |
| 5 | `kermit/src/main.rs`, `kermit/tests/cli_gen_scale_range.rs` | `bench gen h2o` |
| 6 | `benchmarks/h2o.yml`, `benchmarks/h2o-sorted.yml`, `kermit/tests/h2o_generate.rs` | committed YAMLs and the end-to-end test |
| 7 | `docs/benchmarks/H2O.md`, `docs/benchmarks/README.md`, `benchmarks/README.md`, `USAGE.md`, `README.md`, `CLAUDE.md`, `ARCHITECTURE.md` | docs (no measured values) |
| gate | — | the final gate |
| 8 | `docs/benchmarks/H2O.md` | the 1e7 acceptance runs and their measurements (the manager runs it, in a heavy-run slot, after the user's go-ahead) |

Stories 1 and 2 share no file, but both run cargo, so they run one after the other. Story 7
runs no cargo and may run alongside any story.

## Story 1: random numbers and keys

**Module layout.** `kermit-generate/src/lib.rs` gains `pub mod h2o;` between `mod generator;`
and `mod meta;`. `kermit-generate/src/h2o/mod.rs` holds the module doc (what H2O is, the
attribution to db-benchmark's MPL-2.0 `_data/join-datagen.R` and the statement that this is
a reimplementation, not a copy, plus the six relations and three queries), and in Story 1
only:

```rust
// Story 3 uses every item of these modules; until then they are dead outside the tests.
#[allow(dead_code)]
mod keys;
#[allow(dead_code)]
mod rng;

pub use keys::KeyPlan;
```

Story 3 removes both `#[allow(dead_code)]` lines and their comment.

### `kermit-generate/src/h2o/rng.rs`

The module doc cites SplitMix64 (Steele, Lea & Flood, OOPSLA 2014; Vigna's reference
`splitmix64.c`), Lemire's nearly divisionless bounded draws (ACM TOMACS 2019), and
Fisher–Yates (Durstenfeld 1964). It also says why the generator is in-repo: `rand`'s `StdRng`
and its sampling algorithms are not value-stable across versions (#74).

```rust
/// SplitMix64's increment, the golden-ratio constant of `splitmix64.c`.
const GAMMA: u64 = 0x9e37_79b9_7f4a_7c15;

/// SplitMix64's output function. Each step is a bijection on `u64`, so the
/// whole function is one too.
fn mix(z: u64) -> u64; // z = (z ^ (z >> 30)) * 0xbf58_476d_1ce4_e5b9; z = (z ^ (z >> 27)) * 0x94d0_49bb_1331_11eb; z ^ (z >> 31), wrapping multiplies

/// SplitMix64, exactly as Vigna's `splitmix64.c`.
#[derive(Debug, Clone)]
pub(crate) struct SplitMix64 { state: u64 }

impl SplitMix64 {
    /// A generator whose state is `state`, which is `splitmix64.c`'s `x`.
    pub(crate) fn new(state: u64) -> Self;
    /// `splitmix64.c`'s `next()`: state += GAMMA (wrapping), return mix(state).
    pub(crate) fn next_u64(&mut self) -> u64;
    /// An unbiased value in `0..n`, by Lemire's method: m = next * n as u128,
    /// low = m as u64; if low < n { t = n.wrapping_neg() % n; while low < t, redraw };
    /// return (m >> 64) as u64.
    ///
    /// # Panics
    ///
    /// Panics if `n` is 0.
    pub(crate) fn bounded(&mut self, n: u64) -> u64;
    /// Fisher–Yates (Durstenfeld): for i from len − 1 down to 1, swap i with
    /// bounded(i + 1).
    pub(crate) fn shuffle<T>(&mut self, items: &mut [T]);
}

/// H2O's key columns, numbered as the streams number them. Renumbering one
/// changes every relation the generator writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum KeyColumn { Id1 = 0, Id2 = 1, Id3 = 2 }

/// The steps of key generation that draw random numbers, numbered as the
/// streams number them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Step { Pool = 0, Pad = 1, LeftShuffle = 2, RightShuffle = 3 }

/// The generator of one (column, step) pair under `seed`. Its state is
/// mix((seed as u64) << 32 | (column × 4 + step)). The input is injective in
/// (seed, column, step) and `mix` is a bijection, so every stream starts at
/// its own point on the 2⁶⁴ cycle, and no relation depends on the order in
/// which the others are generated.
pub(crate) fn stream(seed: u32, column: KeyColumn, step: Step) -> SplitMix64;
```

**Tests** (`mod tests` in `rng.rs`). Every pinned value was computed in the design session
(see the appendix). The SplitMix64 vectors come from Vigna's `splitmix64.c` itself, compiled
and run, and were not recalled.

- `splitmix64_matches_the_reference_implementation`: `SplitMix64::new(0)` gives
  `0xe220a8397b1dcdaf, 0x6e789e6aa1b965f4, 0x06c45d188009454f, 0xf88bb8a8724c81ec,
  0x1b39896a51a8749b`, and `SplitMix64::new(1234567)` gives `0x599ed017fb08fc85,
  0x2c73f08458540fa5, 0x883ebce5a3f27c77, 0x3fbef740e9177b3f, 0xe3b8346708cb5ecd`.
- `streams_are_seeded_by_seed_column_and_step`: the first `next_u64` of
  `stream(108, Id3, Pool)` is `0x44a79af1687ff9dc`, of `stream(108, Id1, Pad)` is
  `0xa1b06f1977ea1cb8`, and of `stream(108, Id2, RightShuffle)` is `0xde70fad60a6ec795`. The
  12 (column, step) pairs under seed 108 give 12 distinct first outputs, and
  `stream(109, Id3, Pool)` differs from `stream(108, Id3, Pool)`.
- `bounded_matches_the_reference_model`: from `SplitMix64::new(0)`, eight `bounded(10)` give
  `[8, 4, 0, 9, 1, 3, 1, 7]`. From a fresh `SplitMix64::new(0)`, eight
  `bounded((1 << 63) + 1)` give `[243808509735772839, 8954805688390271222,
  980875101213047373, 1603648013000153456, 7116260932800173470, 2266080580496311649,
  8780933256989530195, 3656771639923220100]`. That second case exercises the rejection
  loop: without the loop, the sequence starts `8147104208329303768`.
- `bounded_stays_in_range`: for n in `1, 2, 3, 7, 8, 9, (1 << 32) - 1, 1 << 32,
  (1 << 32) + 1, 1 << 63, u64::MAX`, 10,000 draws from `SplitMix64::new(7)` are all `< n`.
  At n = 1 every draw is 0.
- `bounded_is_uniform_on_a_small_range`: 100,000 `bounded(10)` draws from
  `SplitMix64::new(42)` have a chi-square statistic below 27.88 (df 9, p = 0.001). The
  reference model gives 14.8604, from counts `[10066, 9876, 10129, 9933, 10060, 10042,
  10104, 10155, 9839, 9796]`. Pin the counts too.
- `bounded_of_zero_panics`: `#[should_panic]`.
- `shuffle_yields_a_permutation`: `0..1000` shuffled with `SplitMix64::new(3)` sorts back to
  `0..1000` and is not the identity. An empty slice and a one-element slice are unchanged.

### `kermit-generate/src/h2o/keys.rs`

```rust
/// One H2O plan's cardinalities: `rows` = N, the rows of `x` and the keys of
/// `big`; `small` and `medium`, the keys of the other two right-hand tables.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct KeyPlan { rows: usize, small: usize, medium: usize }

impl KeyPlan {
    /// # Errors
    ///
    /// Unless 1 ≤ small ≤ rows, 1 ≤ medium ≤ rows and rows ≤ RowId::MAX.
    pub fn new(rows: usize, small: usize, medium: usize) -> anyhow::Result<Self>;
    /// (N, N / 1_000_000, N / 1_000), through `new`, so a scale below
    /// 1,000,000 is an error (`small` would be 0).
    pub fn for_scale(scale: u32) -> anyhow::Result<Self>;
    /// N.
    pub fn rows(&self) -> usize;
    /// `small`'s key count.
    pub fn small(&self) -> usize;
    /// `medium`'s key count.
    pub fn medium(&self) -> usize;
    /// The cardinality of `column`'s keys: `small`, `medium` or `rows`.
    pub(crate) fn cardinality(&self, column: KeyColumn) -> usize;
}

/// Key generation's step 1 for one column of cardinality n: the keys
/// 1..=n + n/10, shuffled (stream `Pool`). The first n − n/10 are shared, the
/// next n/10 are left-only (x's only), and the last n/10 are right-only.
pub(crate) struct KeyPool { keys: Vec<usize>, n: usize, seed: u32, column: KeyColumn }

impl KeyPool {
    pub(crate) fn new(n: usize, seed: u32, column: KeyColumn) -> Self;
    pub(crate) fn shared(&self) -> &[usize]; // keys[..n - n/10]
    pub(crate) fn left(&self) -> &[usize];   // keys[n - n/10..n]
    pub(crate) fn right(&self) -> &[usize];  // keys[n..]
    /// Step 2, x's column (upstream's `sample_all`): keys[..n], then rows − n
    /// draws keys[bounded(n)] (stream `Pad`), then shuffled (stream
    /// `LeftShuffle`). Every shared and left key appears. Panics if rows < n.
    pub(crate) fn x_column(&self, rows: usize) -> Vec<usize>;
    /// Step 3, the right-hand table: shared ++ right, shuffled (stream
    /// `RightShuffle`): exactly n unique keys.
    pub(crate) fn right_table(&self) -> Vec<usize>;
    /// Step 4: how many of `x`'s keys are shared, by a bitmap over 0..=n + n/10.
    pub(crate) fn matches(&self, x: &[usize]) -> u64;
}
```

- **Error messages** name the field, its value and the bound:
  `"H2O plan: small = {small} is outside 1..={rows}"`, the same for `medium`, and
  `"H2O plan: rows = {rows} exceeds RowId::MAX = {max}"`. `RowId` is
  `kermit_ds::RowId`.
- **Tests** (`mod tests` in `keys.rs`):
  - `pool_splits_have_the_right_sizes`: for n in 7, 20, 200 and 2000, shared has n − n/10
    keys, left n/10 and right n/10, and together they are 1..=n + n/10, each once. At n = 7,
    n/10 = 0, so left and right are empty.
  - `the_right_table_is_shared_and_right_without_duplicates`.
  - `x_column_covers_shared_and_left_and_never_right`: at (n, rows) = (20, 2000),
    (200, 2000) and (7, 50), the column has `rows` keys, contains every shared and left key,
    and contains no right key.
  - `the_x_column_at_n_equal_rows_is_a_permutation`: at n = rows = 2000 there is no
    padding, so x's column is a permutation of shared ++ left.
  - `matches_count_the_rows_with_a_shared_key`: equal to a `HashSet` count over `shared()`
    at n = 20 and 200. At n = 7 (no left key), it equals `rows`.
  - `key_plan_rejects_bad_cardinalities`: small 0, medium 0, small > rows, medium > rows,
    and rows = `RowId::MAX as usize + 1`. Each message names its field.
  - `key_plan_for_scale_divides_by_a_million_and_a_thousand`: 10,000,000 gives
    (10,000,000, 10, 10,000); 1,000,000 gives (1,000,000, 1, 1,000); 999,999 is an error.
  - `key_plan_serializes_its_three_cardinalities`: `serde_json` gives
    `{"rows":2000,"small":20,"medium":200}`.

## Story 2: kermit-bench types

In `kermit-bench/src/definition.rs`, after `impl SnapQuery`:

```rust
/// The smallest H2O scale: upstream's floor. Below N = 1e7, `small` (N / 1e6
/// keys) has no left-only or right-only key.
pub const H2O_MIN_SCALE: u32 = 10_000_000;

/// The default H2O seed: upstream's `set.seed(108)`.
pub const DEFAULT_H2O_SEED: u32 = 108;

/// The row order of every relation an H2O generator writes (#101's input-order axis).
///
/// The YAML and command-line spelling is kebab case (`order: sorted`);
/// [`H2oOrder::name`] returns it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum H2oOrder {
    /// The order the generator draws rows in, as upstream writes them.
    #[default]
    Shuffled,
    /// Each relation's rows sorted lexicographically (`Tuples::sort`), the
    /// order the sorted tries build in. The draws are the same as `Shuffled`'s.
    Sorted,
}

impl H2oOrder {
    /// Every order, in this order.
    pub const ALL: [H2oOrder; 2] = [Self::Shuffled, Self::Sorted];
    /// The YAML and CLI spelling: "shuffled", "sorted".
    pub fn name(self) -> &'static str;
}
```

- **Export:** add `H2oOrder`, `DEFAULT_H2O_SEED` and `H2O_MIN_SCALE` to
  `kermit-bench/src/lib.rs`'s `pub use definition::{…}`, sorted as rustfmt sorts them.
- **Tests** (`definition.rs`'s `mod tests`):
  - `h2o_order_names_match_their_serde_spelling`: every `ALL` member round-trips through
    `serde_yaml` as `name()`, and `ALL.map(H2oOrder::name)` is `["shuffled", "sorted"]`.
  - `h2o_order_defaults_to_shuffled`.
  - `an_unknown_h2o_order_fails_to_parse`: `random`, `Sorted` and `""`. The error names
    `shuffled`.

## Story 3: the generator

**Manifest:** `kermit-generate/Cargo.toml` `[dev-dependencies]` gains `sha2 = "0.11"`, the
workspace's version, for the golden test.

```rust
// kermit-generate/src/h2o/mod.rs
/// H2O's own `meta.json` fields, flattened beside the common ones.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct H2oExtras {
    /// The seed every stream was drawn from.
    pub seed: u32,
    /// The row order of every relation.
    pub order: H2oOrder,
    /// The cardinalities.
    pub plan: KeyPlan,
}

/// Generates an H2O join benchmark: six relations and the queries q1, q2 and
/// q5, each with the exact count it returns.
#[derive(Debug)]
pub struct H2oGenerator<'a> { target: Target<'a>, plan: KeyPlan, seed: u32, order: H2oOrder }

impl<'a> H2oGenerator<'a> {
    pub fn new(target: Target<'a>, plan: KeyPlan, seed: u32, order: H2oOrder) -> Self;
}

impl Generator for H2oGenerator<'_> {
    type Extras = H2oExtras;
    const KIND: &'static str = "h2o";
    fn target(&self) -> Target<'_> { self.target }
    fn generate(&self, sink: &mut RelationSink) -> anyhow::Result<Generated<H2oExtras>>;
}
```

- **Relations** (`RelationHeader::new(name, attrs)`), written in this order, which is also
  the order of `benchmark.yml`'s relations:

  | # | Relation | Columns | Rows | Data |
  |---|---|---|---|---|
  | 1 | `x_id3` | `id3` | rows | `x3` |
  | 2 | `big` | `id3` | rows | the `Id3` pool's right-hand table |
  | 3 | `x_id1` | `id1`, `id3` | rows | `x1` interleaved with `x3`: row i is `[x1[i], x3[i]]` |
  | 4 | `small` | `id1` | small | the `Id1` pool's right-hand table |
  | 5 | `x_id2` | `id2`, `id3` | rows | `x2` interleaved with `x3` |
  | 6 | `medium` | `id2` | medium | the `Id2` pool's right-hand table |

- **Steps** follow the spec's § Relation order, sorting and memory:
  1. Make the `Id3` pool, `x3` (kept), q5 = `matches(&x3)` and `big`, then drop the pool.
     Write `x_id3` from `Tuples::from_flat(1, rows, x3.clone())`, then `big`.
  2. Do the same for `Id1`. Build the interleaved `Vec` with capacity 2·rows, take it as
     `Tuples::from_flat(2, rows, …)` and drop `x1`. Write `x_id1`, then `small`.
  3. The same for `Id2` gives `x_id2` and `medium`.

  The counts are taken from the unsorted columns. Under `H2oOrder::Sorted`, each
  relation's `Tuples` is sorted (`Tuples::sort`) just before it is written; one private
  helper,
  `fn write(sink: &mut RelationSink, order: H2oOrder, name: &str, attrs: &[&str], tuples: Tuples) -> anyhow::Result<()>`,
  does the sort and the write. Another private helper,
  `fn interleave(left: &[usize], right: &[usize]) -> Vec<usize>`, builds the pairs.
- **Queries**, in this order:

  | Name | Query | Description |
  |---|---|---|
  | `q1` | `Q(K, I) :- small(K), x_id1(K, I).` | `H2O J1 q1 (small inner on int): few keys, each with many x rows` |
  | `q2` | `Q(K, I) :- medium(K), x_id2(K, I).` | `H2O J1 q2 (medium inner on int): thousands of keys, each with about a thousand x rows` |
  | `q5` | `Q(K) :- big(K), x_id3(K).` | `H2O J1 q5 (big inner on int): two dense key lists that overlap by 90%` |

  Each has `expected: Some(count)`.
- **`Generated::description`:** `format!("H2O db-benchmark join (J1): N = {rows}, small = {small}, medium = {medium}, seed {seed}, {order} rows", order = self.order.name())`.
- **Extras:** `H2oExtras { seed, order, plan }`.
- **Tests** go in `kermit-generate/tests/h2o.rs` and use only the public API: `run`,
  `H2oGenerator`, `KeyPlan` and `H2oExtras`, `kermit_ds::read_parquet`,
  `kermit_bench::H2oOrder`, and `kermit_rdf::generator::Target`. A helper generates
  `(plan, seed, order)` with `run` into `<tempdir>/h2o-test`, bench name `h2o-test` and tag
  `test`, with no spec hash. It returns the temp dir, the `GeneratedMeta<H2oExtras>`, the
  emitted `BenchmarkDefinition`, and each relation read back with `read_parquet`, by name.
  The reference plan is `KeyPlan::new(2000, 20, 200)` with seed 108.
  - `writes_six_relations_and_three_queries`: `benchmark.yml`'s relations are, in order,
    `x_id3, big, x_id1, small, x_id2, medium`, with the columns in the table. The queries
    are q1, q2 and q5, with the text in the table and an `expected` each. The meta's `kind`
    is `h2o`, and its extras equal `H2oExtras { seed: 108, order, plan }`.
  - `relation_sizes_follow_the_plan`: rows are 2000 / 2000 / 2000 / 20 / 2000 / 200, in
    table order. The `id3` column of `x_id1` and of `x_id2` equals `x_id3`'s, row for row,
    under `Shuffled`.
  - `expected_counts_equal_a_brute_force_join`, in both orders: count each right-hand
    table's keys into a `HashMap`, sum over x's rows, and compare with the `expected`
    `benchmark.yml` records. This shares no code with the bitmap.
  - `the_reference_plan_matches_its_golden_digests`, in both orders: each relation's digest
    is SHA-256 over its `as_flat()` values, each as `(v as u64).to_le_bytes()`, in order
    (D8: the data, not the file). The digests are those in the appendix. The counts are
    q1 = 1812, q2 = 1814 and q5 = 1800. The test's doc comment carries the spec's rule: if
    the digests must change, bump `META_SCHEMA_VERSION` and add an `h2o` floor to
    `OUTDATED_BEFORE`.
  - `the_same_spec_reproduces_and_another_seed_differs`: generating twice gives equal
    relations. Seed 109 gives a different `x_id3` and a different `x_id1`.
  - `sorted_relations_are_the_shuffled_relations_sorted`: each shuffled relation, after
    `Tuples::sort`, equals its sorted counterpart, and the `expected` counts are equal.
  - `a_plan_without_left_only_keys_generates`: `KeyPlan::new(50, 5, 9)`, where n/10 = 0 for
    small and medium. q1 = q2 = 50 (every x key is shared) and q5 = 45.

## Story 4: the spec variant and the dispatch arm

```rust
// kermit-bench/src/definition.rs, the last GeneratorSpec variant (after Snap)
    /// Drives the H2O generator (`kermit_generate::h2o::H2oGenerator`): six
    /// relations from db-benchmark's join data generator (J1) at `scale` rows,
    /// and the queries q1, q2 and q5 with their exact counts. All three
    /// fields are always serialized, so all three feed `spec_hash`.
    H2o {
        /// N, the rows of `x`. At least [`H2O_MIN_SCALE`].
        scale: u32,
        /// Default [`DEFAULT_H2O_SEED`] (upstream's `set.seed(108)`).
        #[serde(default = "default_h2o_seed")]
        seed: u32,
        /// Default [`H2oOrder::Shuffled`].
        #[serde(default)]
        order: H2oOrder,
    },

/// `GeneratorSpec::H2o`'s default `seed`.
fn default_h2o_seed() -> u32 { DEFAULT_H2O_SEED }
```

- `GeneratorSpec`'s doc line lists `kind: h2o` too.
- **`validate_generator` arm:** `scale < H2O_MIN_SCALE` gives `BenchError::Invalid` with the
  reason `"h2o generator scale {scale} is below the floor {H2O_MIN_SCALE}: upstream's join
  data generator needs N >= 1e7, below which `small` (N / 1e6 keys) has no left-only or
  right-only key"`. Seed and order need no check.
- **Spec tests** (`definition.rs`'s `mod tests`):
  - `h2o_spec_parses_with_its_defaults`: `kind: h2o` with `scale: 10000000` parses to
    `seed` 108 and `order` `Shuffled`, validates, and serializes to exactly
    `"kind: h2o\nscale: 10000000\nseed: 108\norder: shuffled\n"`.
  - `h2o_order_sorted_parses`.
  - `h2o_spec_hash_covers_every_field`: changing `scale`, `seed` or `order` changes
    `spec_hash`, and an omitted `seed` hashes the same as an explicit `seed: 108`.
  - `h2o_scale_below_the_floor_is_rejected`: 9,999,999 is `Invalid`, with a reason naming
    `10000000`; exactly 10,000,000 validates.
- **`kermit/src/materialize.rs`:** imports `kermit_bench::H2oOrder` and
  `kermit_generate::h2o::{H2oGenerator, KeyPlan}`. A `dispatch` arm after `Snap`'s:
  `| GeneratorSpec::H2o { scale, seed, order } => run_h2o(*scale, *seed, *order, bench_name, out_dir, spec_hash),`.

  ```rust
  /// Runs the H2O generator for a `kind: h2o` spec into `out_dir`, passing
  /// `spec_hash` through so it lands in the produced `meta.json`.
  fn run_h2o(
      scale: u32, seed: u32, order: H2oOrder, bench_name: &str, out_dir: &Path, spec_hash: &str,
  ) -> anyhow::Result<()> {
      let plan = KeyPlan::for_scale(scale)?;
      let target = Target { out_dir, bench_name, tag: bench_name, spec_hash: Some(spec_hash) };
      kermit_generate::run(&H2oGenerator::new(target, plan, seed, order))?;
      Ok(())
  }
  ```
- **Cache test** (`materialize.rs`'s `mod tests`, D11):
  `h2o_cache_misses_hits_drifts_and_regenerates`. It calls `materialize` with a temporary
  cache root, `force: false`, and a directly built definition named `h2o-cache` with
  `GeneratorSpec::H2o { scale: 1_000_000, seed: 108, order: Shuffled }`. That scale is below
  the floor, which `materialize` doesn't check, and gives `small` = 1.
  - The first call is a miss. It generates, and returns six relations and three queries,
    each with `expected`. `meta.json`'s `spec_hash` equals the spec's.
  - A marker file is then planted in the cache subdirectory, and `meta.json`'s bytes are
    saved.
  - The second call is a hit: the marker survives and `meta.json`'s bytes are unchanged.
  - Seed 109 without `force` is `BenchError::SpecDrift`.
  - Seed 109 with `force` regenerates: the marker is gone, and `meta.json`'s `spec_hash` is
    the new spec's.

  At 1e6 rows, debug-build generation takes seconds. That is accepted as the spec's
  chosen size.

## Story 5: `bench gen h2o`

```rust
// kermit/src/main.rs, after SnapQueryArg's From impl
/// The CLI spelling of `kermit_bench::H2oOrder`; see [`SnapGraphArg`].
/// `h2o_clap_names_match_the_spec_names` pins that the names agree.
#[derive(Clone, Copy, clap::ValueEnum)]
enum H2oOrderArg {
    /// Each relation's rows in the order they are drawn (upstream's)
    Shuffled,
    /// Each relation's rows sorted, the order the sorted tries build in
    Sorted,
}

impl From<H2oOrderArg> for kermit_bench::H2oOrder { … }

// GenSubcommand, after Snap
    /// Generate an H2O db-benchmark join benchmark (#95)
    H2o {
        /// N, the rows of the fact table `x` (at least 10000000, upstream's floor)
        #[arg(
            long,
            value_name = "N",
            value_parser = clap::value_parser!(u32).range(i64::from(kermit_bench::H2O_MIN_SCALE)..)
        )]
        scale: u32,

        /// Random seed (default: upstream's 108)
        #[arg(long, value_name = "SEED", default_value_t = kermit_bench::DEFAULT_H2O_SEED)]
        seed: u32,

        /// Row order of every relation
        #[arg(long, value_enum, default_value_t = H2oOrderArg::Shuffled)]
        order: H2oOrderArg,

        #[command(flatten)]
        target: GenTarget,
    },

/// `h2o-<scale>-<tag>`, as `bench gen lubm` names `lubm-<scale>-<tag>`.
fn h2o_bench_name(scale: u32, tag: &str) -> String;

/// Handler for `bench gen h2o`. It validates the equivalent declarative spec
/// (the floor and the name) before anything is written, prepares the output
/// directory with `GenTarget::prepare("h2o", …)`, and runs the generator with no
/// `spec_hash`, like every imperative `bench gen`. Seed and order go to
/// `meta.json`, not the name.
fn run_gen_h2o(scale: u32, seed: u32, order: H2oOrderArg, target: GenTarget) -> anyhow::Result<()>;
```

- `run_gen_h2o` follows `run_gen_snap`'s shape. It ends with
  `eprintln!("h2o benchmark written to {}", out.display())`. The `main` arm comes after
  `GenSubcommand::Snap`'s.
- **Unit tests** (`main.rs`'s `mod tests`):
  - `h2o_clap_names_match_the_spec_names`: as `snap_clap_names_match_the_spec_names`, over
    `H2oOrderArg` and `H2oOrder::ALL`.
  - `h2o_flags_default_and_enforce_the_floor`: `--scale 10000000 --tag t` parses to seed 108
    and order shuffled; `--order sorted --seed 7` parses. `--scale 9999999` and
    `--order random` are rejected.
  - `h2o_bench_name_is_scale_and_tag`: `h2o_bench_name(10_000_000, "dev")` is
    `"h2o-10000000-dev"`.
- **`kermit/tests/cli_gen_scale_range.rs`:** `h2o_scale_below_the_floor_is_a_usage_error`
  runs `bench gen h2o --scale 1000 --tag t` with a temporary `XDG_CACHE_HOME`. It exits 2,
  stderr contains `'1000'`, `--scale` and `10000000`, and the cache directory is empty. The
  module doc mentions h2o's floor.

## Story 6: committed YAMLs and the end-to-end test

```yaml
# benchmarks/h2o.yml
name: h2o
description: "H2O db-benchmark join (J1) at N = 1e7: q1, q2 and q5 over per-query projections of x, rows shuffled (#95)."
generator:
  kind: h2o
  scale: 10000000
```

`benchmarks/h2o-sorted.yml` is the same with `name: h2o-sorted`, the description ending
`rows sorted, #101's input-order axis (#95).`, and `order: sorted` under `generator`.

`kermit/tests/h2o_generate.rs` follows `snap_generate.rs`. It uses `mod common;`, with
`join_under_planner`, `load_parquet_relations` and `JoinEntry`, and the same three cells
(`TreeTrie<GallopingSeek>`, `ColumnTrie<GallopingSeek>` under `LeapfrogTriejoin`;
`HashTrie<SipHashStrategy>` under `HashTriejoin`).
- `committed_yamls_declare_h2o_at_the_floor`: `benchmarks/h2o.yml` and `h2o-sorted.yml`
  parse, validate, and declare `GeneratorSpec::H2o { scale: 10_000_000, seed: 108, order }`,
  with order `Shuffled` and `Sorted` respectively.
- `every_cell_optimiser_and_policy_returns_the_expected_counts`: the reference plan
  (`KeyPlan::new(2000, 20, 200)`, seed 108), in both orders, through `kermit_generate::run`.
  Every query runs on every cell × `Optimiser::value_variants()` ×
  `ColumnOrderPolicy::value_variants()`, and every count equals `expected`. A mismatch list
  is collected and asserted empty, as in SNAP's test.
- `the_emitted_counts_are_the_reference_models`: the emitted `expected` values are
  `[("q1", 1812), ("q2", 1814), ("q5", 1800)]` in both orders.

## Story 7: docs (no measured values)

- **`docs/benchmarks/H2O.md`**, new, in the style of `docs/benchmarks/SNAP.md`. It covers
  every bullet of the spec's § Documentation except the 1e7 measurements:
  - origin and attribution;
  - the deviations: the integer split, the per-query projections, the carried `id3`, the
    dropped q3/q4, payloads and NAs, and the in-repo RNG, which is why q1 and q2 differ from
    upstream's counts;
  - the `order` axis;
  - the queries and what each isolates;
  - the spec fields and the floor;
  - `bench gen h2o`;
  - memory: 8 B per column per row to load, about 4N words at peak to generate, and the
    `RowId::MAX` ceiling.

  It ends with a `## Measured at N = 1e7` section holding the one sentence `Story 8 of
  #95's push 2 records the acceptance runs here.`. Story 8 replaces that sentence.
- **`docs/benchmarks/README.md`:** an H2O bullet beside SNAP's, and an H2O column in the
  comparison table.
- **`benchmarks/README.md`:** `h2o` among the generator kinds, with a fields table:
  `scale` (required, ≥ 10000000), `seed` (default 108) and `order` (`shuffled` | `sorted`,
  default `shuffled`).
- **`USAGE.md`** and **`README.md`:** a `bench gen h2o --scale 10000000 --tag dev` line
  wherever SNAP added its `bench gen snap` line. Sentences that count the generators are
  updated.
- **`CLAUDE.md`:**
  - every `bench gen [watdiv|lubm|snap]` and `kind: watdiv|lubm|snap` list gains `h2o`;
  - a Build Commands line: `cargo run -- bench gen h2o --scale 10000000 --tag dev`;
  - the `kermit-generate` row's `h2o` (#95) gains its files (`h2o/mod.rs`, the generator;
    `h2o/rng.rs`, the random numbers; `h2o/keys.rs`, the key columns);
  - Benchmark Reference Docs gains `docs/benchmarks/H2O.md`;
  - a new gotcha: H2O's in-repo RNG gives each (column, step) its own SplitMix64 stream,
    and its golden data hashes (`kermit-generate/tests/h2o.rs`) follow the bump-and-floor
    rule (`META_SCHEMA_VERSION` and `OUTDATED_BEFORE`).
- **`ARCHITECTURE.md`:** the line SNAP's phase 2 changed for `snap` gains `h2o` the same
  way.

## Story 8: acceptance at N = 1e7 (the manager)

This story runs in a heavy-run slot that the co-ordinator allots, after the user's go-ahead,
on a release build inside `nix develop`. It uses a scratch `XDG_CACHE_HOME` on `/tb`, never
`~/.cache/kermit` and never the tmpfs scratchpad. Its runs:
1. `bench run h2o -i all -a all --verify -m space` and the same for `h2o-sorted`: every
   cell verifies, and the space reports are kept.
2. `bench gen h2o --scale 10000000 --tag acc1` and `--tag acc2` under the same scratch
   cache. The per-relation SHA-256s in the two `meta.json` files are identical to each
   other and to the `h2o` cache's.
3. Generation time and peak RSS for one `bench gen h2o` at 1e7, with `/usr/bin/time -v`.

The results go into H2O.md's `## Measured at N = 1e7`:
- q1, q2 and q5 at 1e7 (q5 = 9,000,000 exactly), beside upstream's 8,997,136 and 9,001,606
  and the reason they differ;
- space per structure per relation, in both orders;
- generation time and peak RSS;
- the per-structure ceilings on this host's 30 GB, extrapolated from the measured space
  (D7).

## Appendix: how the pinned values were computed

`h2o_oracle.py` was written in the design session from the spec alone. It shares no code
with the Rust and was run with Python 3 (`python3 -I h2o_oracle.py 2000 20 200 108`). It
reproduces Vigna's `splitmix64.c` vectors above, and checks its own counts against a
dictionary join. Its output for the reference plan (seed 108):

```
counts {'q1': 1812, 'q2': 1814, 'q5': 1800}
[shuffled]
  x_id3   rows=  2000 sha256=a6995949eb98f11444e9bd59e5353a46928a2648ae45bb9e8d254afb0399589c
  big     rows=  2000 sha256=daeb0faaa79972199c71600bf24ac7a8a9dba4c1d0e4a25204b2eec574010102
  x_id1   rows=  2000 sha256=3a9a6901e57520c8c51da77f54e373758cf47e20d5b29441b2814055a4b1fcab
  small   rows=    20 sha256=7a7ca985781668a8c510f5905ad5bd56e7fc6a471bac204accf59a6023bad7fa
  x_id2   rows=  2000 sha256=01ae30e97a521fced72f5faa1e26e2eaa2304b1bb5c6888d593e98a241a4eb97
  medium  rows=   200 sha256=ca2644e7d6a743f68a010af7c2ec46fced86a942bb724e2dba5a46af90f93255
[sorted]
  x_id3   rows=  2000 sha256=46d0bb7d56f4102f31d36ea3896df676d431bd73fc332b430c7d8d7be430db48
  big     rows=  2000 sha256=078cf7896fb356d148669cea09eb3dfaf06961c984e3fe397e886315f387ca85
  x_id1   rows=  2000 sha256=69239a8aac021928a0b533e5fede0b3aff66215bd8d0e1f2f47749156e1c2814
  small   rows=    20 sha256=c38798bf9a2d0d3f28a36a50aa81ef6e3b5fefc477f7dbc3029f0705b096a5aa
  x_id2   rows=  2000 sha256=136e352cc2d950268350010d22d50a4e23d0255e5bf2c564535bada8da8fbc6d
  medium  rows=   200 sha256=14135e2b2fc781278aed6f1d400f19d5f12f339f3cf0bf4f5b3f040547f11f03
first 5 of x_id3: [888, 1582, 633, 433, 38]
first 3 of x_id1: [(18, 888), (10, 1582), (9, 633)]
```

At (rows, small, medium) = (1,000,000, 1, 1,000) it gives q1 = 1,000,000, q2 = 899,809 and
q5 = 900,000. `materialize`'s cache test may pin these.

The script, kept so the values can be reproduced:

```python
M = (1 << 64) - 1
GAMMA = 0x9E3779B97F4A7C15

def mix(z):
    z = ((z ^ (z >> 30)) * 0xBF58476D1CE4E5B9) & M
    z = ((z ^ (z >> 27)) * 0x94D049BB133111EB) & M
    return z ^ (z >> 31)

class SplitMix64:
    def __init__(self, state): self.x = state & M
    def next_u64(self):
        self.x = (self.x + GAMMA) & M
        return mix(self.x)
    def bounded(self, n):            # Lemire 2019; t = (2^64 - n) mod n
        m = self.next_u64() * n; low = m & M
        if low < n:
            t = ((1 << 64) - n) % n
            while low < t:
                m = self.next_u64() * n; low = m & M
        return m >> 64
    def shuffle(self, items):
        for i in range(len(items) - 1, 0, -1):
            j = self.bounded(i + 1); items[i], items[j] = items[j], items[i]

ID1, ID2, ID3 = 0, 1, 2
POOL, PAD, LEFT_SHUFFLE, RIGHT_SHUFFLE = 0, 1, 2, 3

def stream(seed, column, step):
    return SplitMix64(mix(((seed << 32) | (column * 4 + step)) & M))

def key_column(n, rows, seed, column):
    keys = list(range(1, n + n // 10 + 1)); stream(seed, column, POOL).shuffle(keys)
    shared, right = keys[: n - n // 10], keys[n:]
    x = keys[:n]; pad = stream(seed, column, PAD)
    x += [keys[pad.bounded(n)] for _ in range(rows - n)]
    stream(seed, column, LEFT_SHUFFLE).shuffle(x)
    table = shared + right; stream(seed, column, RIGHT_SHUFFLE).shuffle(table)
    in_shared = set(shared)
    return x, table, sum(1 for k in x if k in in_shared)

# Relations: x_id3 = x3, big = table(id3), x_id1 = zip(x1, x3), small = table(id1),
# x_id2 = zip(x2, x3), medium = table(id2). Sorted = sorted(rows). The digest is
# SHA-256 over every value as struct.pack("<Q", v), row-major.
```
