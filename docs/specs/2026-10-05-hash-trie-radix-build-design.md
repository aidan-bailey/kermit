# HashTrie Radix-Partitioned Build

**Date:** 2026-10-05
**Status:** Approved design, not yet implemented
**Scope:** Issue #91. `HashTrie` gains its first **BuildMode** under
[`optimization-standard.md`](optimization-standard.md): `radix:K`, a build
that radix-partitions the tuples on the hash of their first attribute
before inserting them (SIGMOD 2020 §3.3.2). The current build becomes the
named default, `serial`. `--ds-build` becomes a keyed flag, since two
structures now have build modes.

## Motivation

`HashTrie::from_tuples_with_config`
(`kermit-ds/src/ds/hash_trie/implementation.rs:265`) inserts the tuples one
at a time, in input order. Each insert probes the root at a random bucket
and then descends into a random subtrie, so on a large relation nearly
every step of the build misses cache.

The paper partitions during materialization "based on the hash values of
the first join key attribute", which is "critical to achieve acceptable
cache performance during the remainder of the build and probe phases". It
adapts Balkesen et al.'s radix partitioning to morsel-driven parallelism.
Single-threaded, the only possible gain is cache locality, and that gain is
unproven here. The mode is therefore an ablation knob, measured against the
serial build before anyone relies on it. It is also the natural first step
of a parallel HashTrie build, in which threads build disjoint partitions.

## Decisions

| Question | Decision |
|---|---|
| Category | **BuildMode**: a process that yields the same structure. |
| Equivalence | **Exact identity**, as the standard requires today: the same contents, bucket positions, capacities and `HeapSize`. No amendment to the standard. |
| How identity is kept | Each partition is built into a scratch root. The distinct root keys then move into the real root in the order they first appeared in the input (see "Why the trie is identical"). |
| Modes | `serial` (default: today's build, one `insert_at` per tuple in input order) and `radix:K` (K radix bits, 2^K partitions). |
| `K` | Required, in `1..=16`. A bare `radix` is a usage error. |
| Partitioned levels | The root only. |
| CLI | `--ds-build` takes `structure=mode` pairs: `--ds-build hash-trie=radix:8,column-trie=incremental`. The bare form `--ds-build incremental` is **rejected**, with a suggestion of the keyed spelling. |
| Default's name | `serial`. kermit-lab back-fills it on HashTrie rows that lack `ds_build_mode`. |
| Axis source | The bench family, as for ColumnTrie. Every HashTrie report now carries `ds_build_mode`. |
| Schema version | Stays at 3. The axis is additive and no metric changes meaning (#84's precedent). |
| `kermit join` | Still takes no `--ds-build`: a build mode cannot change answers. |
| Issue edits | This branch does not comment on or close #91. The A/B results are drafted for the user to post. |

Rejected alternatives:
- **Amend the standard.** Fill the root region by region, which gives the
  same capacities, contents, occupied buckets and total probe displacement,
  but a different placement inside each probe cluster, and so a different
  iteration and join-row order. The user chose identity.
- **History-independent probing.** Ordered or Robin Hood probing would make
  every insertion order give the same table. That would change the default
  HashTrie's layout and invalidate its prior measurements (Priorities
  item 6), so it is a separate Layout change.
- **A shared `--ds-build` value set** (`--ds-build radix:8`, with the value
  deciding which structures it reaches). The user chose explicit keys.

## Design: `kermit-ds`

### The mode type

`kermit-ds/src/ds/hash_trie/build_mode.rs`, beside ColumnTrie's
`column_trie/build_mode.rs`:

```rust
/// How a `HashTrie` is built from a known set of tuples. Every mode builds
/// the identical trie (issue #91).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum HashTrieBuildMode {
    /// One `insert_at` per tuple, in input order: Algorithm 2 of the paper.
    #[default]
    Serial,
    /// Radix-partition on the top `bits` of the first attribute's hash,
    /// build each partition separately, then merge the roots.
    Radix(RadixBits),
}

/// The number of radix bits, `1..=16`: from 2 to 65,536 partitions. A
/// tuple's partition fits a `u16`, which is what bounds the range.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RadixBits(u8);
```

- `RadixBits::new(bits) -> Result<Self, _>` rejects 0 and anything above
  16. `get()` returns the bits.
- `BuildMode::axis_value` returns `"serial"` or `"radix:<bits>"`.
- `FromStr` parses exactly those strings. Its errors name the accepted
  forms: `serial`, `radix:<bits>` with bits in 1..=16.
- It is not a clap `ValueEnum`, because `radix` carries a parameter. The
  keyed CLI parser calls `FromStr`.
- Re-exported from `kermit_ds` beside `ColumnTrieBuildMode`.

### The constructors

`HashTrie` is the first structure with both a Config and a BuildMode. Each
trait's constructor fixes the other axis to its default, so one inherent
constructor takes both:

```rust
impl<H: HashStrategy, P: PruningPolicy> HashTrie<H, P> {
    pub fn from_tuples_with_config_and_build_mode(
        header: RelationHeader, config: HashTrieConfig, mode: HashTrieBuildMode,
        tuples: Vec<Vec<usize>>,
    ) -> Self;
}
```

- Its `Serial` arm calls `ConfigurableRelation::from_tuples_with_config`,
  whose body is today's loop. Its `Radix` arm runs the radix build. The
  delegation runs from the combined constructor to the serial one, never
  the reverse, so the serial build's code does not change and its
  `insertion` numbers stay valid. Only the visibility of `insert_at` and
  `make_root` widens, because the radix build lives in its own module.
- The new `BuildModeRelation for HashTrie<H, P>` (`type BuildMode =
  HashTrieBuildMode`) calls the combined constructor with
  `HashTrieConfig::default()`.
- `Relation::from_tuples` keeps calling `from_tuples_with_config` with the
  default config, so it gets both defaults.

### The radix build

For `radix:K`, with load factor `lf` taken from the config throughout:

1. **Check arity.** Assert every tuple's arity first, with the serial
   path's panic message.
2. **Partition, stably.** One radix pass in the style of Balkesen et al.
   - The histogram pass computes each tuple's partition,
     `(H::hash(tuple[0]) >> (64 - K)) as u16`, records it in a `Vec<u16>`,
     and counts the partition sizes.
   - The scatter pass consumes the input and pushes each
     `(input index, tuple)` onto its partition's `Vec`, which is allocated
     at its exact histogram size.
   - Order within a partition is input order.
3. **Build each non-empty partition into a scratch root.**
   - The scratch root is `make_root(arity)`, the real root's kind: `Inner`
     for arity ≥ 2, `Leaf` for arity 1.
   - It is filled by calling the unchanged
     `insert_at(&mut scratch, 0, arity, tuple, lf)` for each of the
     partition's tuples, in order.
   - When the scratch table's `len()` grows, that tuple introduced a new
     root key, and `(input index, hash)` is appended to the partition's
     arrival list.
4. **Merge.**
   - Before a scratch table is consumed, look up each arrival's bucket
     position (`index_of`).
   - Then take its buckets (a new consuming `HashTable` accessor) and
     collect `(first index, hash, value)` per arrival. No per-partition
     sort is needed.
   - Concatenate every partition's triples, sort them by first index, and
     insert each into the real root with
     `root.entry_or_insert_with(hash, lf, || value)`.
5. **Count.** `tuple_count = n`.

Empty input makes no partitions and leaves the empty root that
`make_root(arity)` built, as the serial build does.

### Why the trie is identical

- **Inserting an existing key changes nothing.** `entry_or_insert_with`
  returns an existing entry before its resize check (`hash_table.rs:155`).
  So a table's final state (bucket positions, `len`, capacity and doubling
  history) depends only on the order in which its *new* keys arrive.
- **The real root gets the same new keys in the same order.** The serial
  build's root receives each distinct key at that key's first appearance,
  and so does the merge. Under singleton pruning the serial build first
  calls `table.get(hash)`, which is read-only.
- **Every subtrie gets the same insert calls.** A root key's subtrie only
  ever receives that key's tuples. The partition is stable, so they arrive
  in input order. The scratch root then makes the same `insert_at` calls the
  serial root makes for those tuples. That includes every `Singleton`
  creation and unprune under `SingletonPruning`, and every chain push for
  arity 1, so tables, chains, `Singleton`s and all `Vec` capacities match.
- **Merging never reallocates.** Moving a finished subtrie into the root,
  and any later doubling of the root, moves `Vec` headers only.
- **Full-hash collisions are covered.** Distinct keys with equal hashes
  share a partition and a root entry, exactly as in the serial build.

### What it costs, and where it could gain

- **Possible gain:** all n per-tuple probes and descents happen inside one
  partition's scratch root and subtries, a working set of roughly 1/2^K of
  the trie. Only the D merge inserts, one per distinct first-attribute
  hash, touch the real root at random.
- **Extra work:** the histogram and scatter passes; a second `H::hash` of
  the first attribute per tuple, because `insert_at` hashes it again and
  passing the hash in would change the serial path; an O(D log D) sort;
  and D scratch inserts.
- **Transient memory:** 2 bytes per tuple for the partition ids, and 32
  bytes per tuple for the partitioned `(index, Vec header)` pairs, held
  while the input buffer is consumed, plus the scratch tables.
- **Lasting memory:** none. The finished trie is identical, so `space` and
  every query-time metric are unchanged.

### The #66 input shape

A scratch root receives keys that share the top K bits of their hash.
That resembles the clustered input that made the build degrade about 20×
in #66. It is safe because `bucket_index` multiplies by `MULTIPLIERS[p]`
before taking the top `p` bits (`hash_table.rs:100`). Write the hash as
`c · 2^(64−K) + low`. Modulo 2^64 the product is then
`low · M + (c · M mod 2^K) · 2^(64−K)`: the keys' own `low · M`, which
spreads as it would for any key set, plus one constant shared by the whole
partition. Adding a constant shifts every key's top bits alike, so the
partition does not cluster. A probe-count test pins this (see Testing), and the
`insertion` A/B confirms it.

### `BuiltWith`

`BuiltWith<R, P>` gains a `HashTrieIterable` forward, as `Configured`
already has, so hash-family aliases such as
`BuiltWith<HashTrieSip, Radix2>` reach the join and DS suites.

## Design: `kermit` binary

### The cell, the family and the axis

- **The cell.**
  `Execution::HashHtj { hasher, pruning, config, build: HashTrieBuildMode }`.
  It stays `Copy`, since `HashTrieBuildMode: Copy`. `for_pair` and
  `for_structure` route the hash-trie mode into it.
- **The families.** `HashTrieFamily::new(config, build)` and
  `HashHtj::new(config, build, optimiser)`. `execution()` reports the
  family's `build`.
- **Construction.** `HashTrieFamily::build_relation` calls
  `from_tuples_with_config_and_build_mode`, so every route that builds a
  relation (`build_relation`, `load`, `load_with_tuples`,
  `build_from_tuples`) honours both the config and the mode.
- **The axis.** `build_mode_axes` emits
  `{"ds_build_mode": build.axis_value()}` on every HashTrie report, default
  `"serial"`.
- **Dispatch.** `dispatch_ds_bench` (`bench/ds.rs`), `dispatch_run_bench`
  (`bench/run.rs`) and `load_query_runner` (`main.rs`) pass the mode
  through. `with_hash_trie_layout!` is untouched: a BuildMode is not a
  Layout.

### CLI: keyed `--ds-build`

`BuildChoices` (`kermit/src/options.rs:540`) becomes:

```rust
#[arg(long = "ds-build", value_name = "STRUCTURE=MODE,...", value_delimiter = ',')]
ds_build: Vec<String>,
```

It resolves into
`BuildModes { column_trie: ColumnTrieBuildMode, hash_trie: HashTrieBuildMode }`,
which replaces `DsChoices.build`'s single `ColumnTrieBuildMode`. Each pair
is `key=mode`. The key is an `-i` name with a BuildMode axis
(`column-trie`, `hash-trie`); the mode is parsed by that structure's
parser. Usage errors:

| Input | Error |
|---|---|
| `--ds-build incremental` (no `=`) | expects `structure=mode` pairs; suggests `column-trie=incremental`, naming whichever structures have that mode, or `hash-trie=radix:8` as an example when none has |
| `tree-trie=…` | tree-trie has a single build process; keys with build modes: column-trie, hash-trie |
| `foo=…` | unknown structure key, listing the valid keys |
| a key given twice | key given more than once (as for `--ds-config`) |
| `hash-trie=bulk`, `column-trie=radix:8` | unknown mode for that structure, listing its modes |
| `hash-trie=radix`, `radix:0`, `radix:17`, `radix:x` | the accepted form, `radix:<bits>` with bits in 1..=16 |

**Reach.** `DsFlag::Build` becomes `DsFlag::Build(IndexStructure)`.
`DsFlag::given` emits one per key, and `DsFlag::structures` returns that
one structure. `Display` gives `--ds-build hash-trie`. The `-i` check
(`validate_ds_flags`) and the #86 `-a`-narrowing check (`resolve_sweep`)
need no other change:
- `-i column-trie --ds-build hash-trie=radix:8` is rejected by the `-i`
  check.
- `-i all -a leapfrog-triejoin --ds-build hash-trie=radix:8` is rejected
  because the sweep keeps no hash-trie cell.
- `-i all --ds-build hash-trie=radix:8,column-trie=incremental` sets both
  structures' modes.

## Design: kermit-lab

- `SCOPED_AXIS_DEFAULTS[("ds_build_mode", "HashTrie")] = "serial"`: every
  pre-#91 HashTrie report was built serially.
- `test_contract.py` moves to `--ds-build column-trie=incremental`.
- The `ablation` preset already limits `ds_build_mode` to `insertion` and
  `end_to_end`, which stays correct because the trie is identical.

## Testing

### `kermit-ds`

1. **Array-level identity** (unit test with crate access to `root()`).
   An `assert_same_trie` walker compares two tries recursively:
   - per table: `buckets_len()`, the bucket `Vec`'s capacity, `len()`, and
     for every bucket index whether it is occupied and which hash it holds;
   - per leaf chain: its tuples in order, the chain's `capacity()`, and
     each tuple's `capacity()`;
   - per `Singleton`: its payload tuple and that tuple's capacity;
   - per trie: `heap_size_bytes()` and `tuple_count()`.

   It compares `serial` against `radix:K` over this matrix:
   - arity 1, 2 and 3;
   - `NoPruning` and `SingletonPruning`;
   - SipHash, FxHash, and a colliding mod-10 strategy (one partition, real
     full-hash collisions), defined in `test_support.rs`;
   - K ∈ {1, 4, 16};
   - load factor 0.7 and 0.5;
   - inputs: empty, one tuple, duplicates, interleaved keys, and an
     LCG-generated input large enough to double the root several times
     (smaller under Miri).

   **Mutant check:** merging in bucket order instead of first-appearance
   order must fail this test. Run the mutant against it before relying on
   it.
2. **Mode type.** `axis_value` and `FromStr` round-trip for `serial` and
   for each K; the default is `Serial`; `RadixBits` rejects 0 and 17 and
   accepts 1 and 16.
3. **#66 guard** (`hash_table.rs`, reusing `build_probes`). Absorbing the
   keys of one partition (the top K hash bits equal) costs at most 2× the
   probes of absorbing an unrestricted key set of the same size, for
   SipHash and FxHash.
4. **DS-layer suites.** `hash_trie_test_suite!` and `parquet_test_suite!`
   run on `HashTrieSipRadix2 = BuiltWith<HashTrieSip, Radix2>` in
   `kermit-ds/tests/hash_trie_tests.rs`.
5. **`BuiltWith`.** A hash-family provider spy (the mode is requested once
   per `from_tuples`, never by `new`) and the `HashTrieIterable` forward.

### `kermit`

6. **Join suites.** `kermit/tests/join_tests.rs` declares
   `define_build_mode_provider!(Radix2, HashTrieBuildMode,
   HashTrieBuildMode::Radix(RadixBits::new(2).unwrap()))`. It then runs
   `define_multiway_join_test_suite_for_build_mode!` on `HashTrieSip` and
   `HashTrieFxPruned`, each under `LexicographicOptimiser` and
   `CardinalityOptimiser`: four modules covering both hashers and both
   pruning policies. Two bits suit the 3–5 tuple fixtures: several
   partitions, several keys in each.
7. **The mode reaches the build.** Outputs are identical, so only a spy
   can tell. `HashTrieFamily` is concrete over `HashTrie<H, P>`, so #84's
   generic `Spy` relation does not apply. A counting `HashStrategy` does:
   `radix:K` hashes each tuple's first attribute once more than `serial`.
   The test runs `build_relation`, `load` and `build_from_tuples` on both
   families under each mode and checks the count.
8. **Cells and axes.** `for_pair` and `for_structure` carry the hash-trie
   mode, and `build_mode_axes` gives `"serial"` / `"radix:4"`.
9. **CLI unit tests** (`options.rs`): every row of the error table above;
   keyed parsing of both structures; `DsFlag::given` with keyed pairs; and
   `unreached_flag` for each key.
10. **CLI integration.**
    - New `kermit/tests/cli_hash_trie_build_mode.rs`: `bench ds -i
      hash-trie` without the flag reports `serial`; with
      `--ds-build hash-trie=radix:4` it reports `radix:4`; malformed values
      and wrong structures are rejected.
    - `cli_column_trie_build_mode.rs` and `cli_bench_run_ds_flag_reach.rs`
      move to the keyed spelling. The latter gains a narrowed-sweep case
      for the `hash-trie` key.

### kermit-lab

11. The `serial` back-fill fills HashTrie rows only, and leaves rows that
    already carry `radix:K` alone.

## Documentation

- `docs/data-structures/hash-trie.md` § Build modes: `serial` and
  `radix:K`, covering the algorithm, the identity argument, the costs, the
  #66 note, the CLI and axis values, and the measured A/B. Add a
  `from_tuples` complexity row for `radix:K`.
- `docs/specs/optimization-standard.md`:
  - the keyed `--ds-build` examples;
  - BuildMode has two consumers;
  - `BuildModeRelation` is implemented by ColumnTrie and HashTrie;
  - walkthrough step 6 is rewritten: the CLI is keyed and `HashHtj` has a
    `build` slot;
  - radix partitioning moves from "Available to add" to implemented;
  - a note that a structure with both a Config and a BuildMode needs one
    constructor taking both.
- `docs/specs/bench-report-schema.md`: the HashTrie `ds_build_mode` values,
  plus a "3 (no bump)" history row for #91.
- `docs/data-structures/column-trie.md`, `USAGE.md`, `BENCHMARKING.md`,
  `ARCHITECTURE.md`, `docs/specs/benchmarking-architecture.md`, `CLAUDE.md`:
  the keyed spelling, `HashHtj { …, build }`, and `BuildModeRelation`'s
  implementors.

## Measurement (issue criterion 4)

- **Command:** `bench ds -i hash-trie -m insertion`, comparing
  `--ds-build hash-trie=serial` against `hash-trie=radix:{4,8,12}`, with
  SipHash and pruning off (the defaults).
- **Relations:**
  - WatDiv `friendOf` (4,491,142 tuples, the #84 relation), where tuples
    far outnumber distinct keys;
  - one relation for the other regime, where the number of distinct
    first-attribute values D is close to n. Pick it from the cached WatDiv
    relations as the largest with D / n ≥ 0.5, and name it and its ratio
    in the results.
- **Protocol:**
  - one binary, run directly;
  - a distinct `--name` per arm;
  - at least 5 replicates with alternating arm order;
  - a quiet host, checked by sampling `ps` before and during the run;
  - relations read from the existing cache; nothing is written under
    `~/.cache/kermit`.
- **Also check** that `space` is equal across arms. Identity guarantees it,
  but it costs nothing to confirm.
- **Record** the medians and spreads in `hash-trie.md`, and draft the issue
  comment for the user to post.
- **Decision.** Whether to keep the mode is the user's call after the A/B.
  A measured null or negative result is a valid ablation data point.

## Out of scope

- Partitioning levels below the root.
- Hashing the first attribute only once, which would change the serial
  path.
- Multi-pass radix partitioning to limit fan-out at large K.
- The parallel build. This design's shape (independent partition builds,
  then one sequential merge in first-appearance order) is meant to be its
  first step.
- History-independent hash tables.
- A default K for a bare `radix`.

## Phasing

1. **`kermit-ds`:** mode type, constructors, radix build, `BuiltWith`
   forward, and tests 1–5.
2. **`kermit`:** cell, families, axis, keyed CLI, and tests 6–10.
3. **kermit-lab and docs:** the back-fill, test 11, and every document
   above.
4. **Measurement:** the A/B, written into `hash-trie.md`.

Each phase ends with its crate's tests, clippy `-Dwarnings` and `cargo doc`
green. Before landing, run the full gate: `cargo test`, clippy, the
`nix develop` fmt check, `cargo doc`, miri on the non-excluded crates, and
kermit-lab pytest with `KERMIT_BIN` set.

## Coordination

#88 (capacity hint) and #89 (hash seed) add Configs to the same
`HashTrieConfig`, `HashTrieFamily` and `options.rs`. If either is in
flight, agree a landing order first.

## Acceptance (issue #91)

- [ ] Array-level identity test, capacities included: Testing 1.
- [ ] `define_multiway_join_test_suite_for_build_mode!` covers the mode
      under both optimisers: Testing 6.
- [ ] Axis `ds_build_mode = "radix:K"`, documented in `hash-trie.md`
      § Build modes.
- [ ] `insertion` A/B against the serial build, drafted for the issue:
      Measurement.
