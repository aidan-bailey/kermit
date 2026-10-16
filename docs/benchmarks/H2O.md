# H2O (db-benchmark join tasks)

H2O.ai's db-benchmark is a public comparison of data-frame and database engines. Its
join task ("J1") joins one fact table `x` of N rows, with integer keys `id1`, `id2` and
`id3`, to three right-hand tables, `small` (N / 10⁶ keys), `medium` (N / 10³ keys) and
`big` (N keys). Kermit regenerates that data itself and runs three of upstream's join
questions, q1, q2 and q5, as Datalog joins. They are acyclic key/foreign-key joins, the
classic hash-join workload, so H2O is the thesis's relational baseline beside the cyclic
SNAP graphs. q1 is almost pure leaf enumeration (a few keys with about a million children
each at N = 1e7), and q5 intersects two dense key lists that overlap by 90%, which
separates the seek strategies (#80). N is a scaling knob, and the `order` field writes
the same data shuffled or sorted: a controlled input-order axis on synthetic data for
#101. Every query carries an exact `expected` count, so `--verify` checks it.

Unlike LUBM and WatDiv, H2O has no RDF and no external generator, and unlike SNAP it
downloads nothing: a seeded generator in the repo (`kermit-generate`,
`kermit-generate/src/h2o/`) writes the Parquet files itself. For a side-by-side
comparison with the other benchmarks in the suite, see
[`README.md`](README.md#cross-benchmark-comparison).

## How to run

### On-the-fly generation

```bash
kermit bench gen h2o --scale N [--seed S] [--order shuffled|sorted] \
    --tag STR [--output-dir PATH]
```

Writes `~/.cache/kermit/benchmarks/h2o-<N>-<tag>/` and prints
`h2o benchmark written to <dir>` on stderr. Run it with the unmodified `kermit bench run`:

```bash
kermit bench gen h2o --scale 10000000 --tag dev
kermit bench run h2o-10000000-dev -i all -a all --verify
```

The bench name is collision-checked against committed YAMLs at runtime. Seed and order
go to `meta.json`, not to the name, so two generations that differ only in them need
different tags.

| Flag | Default | Notes |
|------|---------|-------|
| `--scale` | required | N, the rows of `x`. At least 10000000 (upstream's floor, see [Inference / preprocessing](#inference--preprocessing)); a smaller value is a usage error that exits 2 and names `--scale` and 10000000 |
| `--seed` | 108 | Upstream's `set.seed(108)`. Any `u32` |
| `--order` | `shuffled` | `shuffled` (the order the generator draws rows, as upstream writes them) or `sorted` (each relation's rows sorted; see [Row order](#row-order-the-order-axis)) |
| `--tag` | required | Suffix on the bench name |
| `--output-dir` | `~/.cache/kermit/benchmarks` | Custom output dirs are NOT auto-discovered by `bench list/fetch/run` |

### Declarative YAML spec (commit-and-run)

`benchmarks/h2o.yml` and `benchmarks/h2o-sorted.yml` are committed. `bench run h2o`
generates and caches the data on first invocation:

```yaml
# benchmarks/h2o.yml
name: h2o
description: "H2O db-benchmark join (J1) at N = 1e7: q1, q2 and q5 over per-query projections of x, rows shuffled (#95)."
generator:
  kind: h2o
  scale: 10000000
```

`h2o-sorted.yml` is the same with `name: h2o-sorted`, a description ending
`rows sorted, #101's input-order axis (#95).` and `order: sorted`.

Required: `scale`, at least 10000000. Optional: `seed` (default 108) and `order`
(`shuffled`, the default, or `sorted`). An unknown `order` is a parse error that lists
the valid names, and `BenchmarkDefinition::validate` rejects a `scale` below the floor.
All three fields feed the spec hash, and an omitted `seed` hashes the same as an
explicit `seed: 108`.

Editing the YAML and re-running errors with `SpecDrift` — pass `--force` to opt into
regenerating. See `benchmarks/README.md` for the full schema. Both YAMLs are N = 1e7
benchmarks, so `bench run --all` generates and runs them too.

## Pipeline

```
(scale, seed, order)             KeyPlan::for_scale: N, N / 10⁶, N / 10³
         │
         │ kermit-generate::h2o::rng   SplitMix64, one stream per (column, step)
         │ kermit-generate::h2o::keys  KeyPool: pool, x's column, right table, matches
         ▼
  for id3, then id1, then id2:
  x's column ──► count of rows with a shared key        right-hand table
         │                 │                                   │
         │ RelationSink::write (rows sorted first under order: sorted)
         ▼                 │                                   ▼
  x_id3 / x_id1 / x_id2    │                         big / small / medium
  .parquet                 ▼                         .parquet
         └──────► expected for q5 / q1 / q2 ◄───────────────┘
                           │
                           ▼
                benchmark.yml + meta.json
```

The three key columns are generated one at a time and dropped as soon as their
relations are written, which bounds the peak memory (see
[Practical scale ceiling](#practical-scale-ceiling)). The counts are taken from the
unsorted columns, by a bitmap that reads no `kermit-ds` or `kermit-algos` structure.

## Output layout

```
~/.cache/kermit/benchmarks/h2o-10000000-dev/
  meta.json        kind = "h2o"; written last, so its presence marks a finished cache
  benchmark.yml    six relations (file:// URL + SHA-256) and q1, q2, q5, each with `expected`
  x_id3.parquet    x_id3(id3): N rows
  big.parquet      big(id3): N rows
  x_id1.parquet    x_id1(id1, id3): N rows
  small.parquet    small(id1): N / 10⁶ rows
  x_id2.parquet    x_id2(id2, id3): N rows
  medium.parquet   medium(id2): N / 10³ rows
```

There is no `raw/` directory: nothing is downloaded. A declarative spec writes the same
layout into a directory named after the YAML (`h2o`). `meta.json` has the fields every
generated benchmark has (`kind`, `tag`, `generated_at_utc`, `spec_hash` for a
declarative run, and each relation's name, columns, rows and SHA-256) plus H2O's own:

| Field | Meaning |
|-------|---------|
| `seed` | The seed every random stream was drawn from |
| `order` | `shuffled` or `sorted`, the row order of every relation |
| `plan` | The cardinalities: `rows` (N), `small` and `medium` (the key counts of those two tables) |

## Inference / preprocessing

H2O has no input data: the generator draws it from the plan `(N, small, medium)`, where
`small` = N / 10⁶ and `medium` = N / 10³ (integer division), `big` has N keys and the
fact table has N rows.

### Key generation

For each key column (`id1`, `id2`, `id3`) with cardinality n (`small`, `medium` or N):

1. **Pool.** The keys 1..=n + n / 10, shuffled. The first n − n / 10 are *shared*, the
   next n / 10 are *left-only* (only `x` holds them) and the last n / 10 are *right-only*
   (only the right-hand table holds them). The integer split replaces R's `n * 0.9`, so
   every N is well-defined.
2. **`x`'s column** (upstream's `sample_all`). The first n keys of the pool (shared and
   left-only), then N − n draws from them, uniformly, then a shuffle. Every shared and
   left-only key appears at least once, and no right-only key does. For `id3`, n = N, so
   there is no padding and the column is a permutation.
3. **The right-hand table.** The shared keys and the right-only keys, shuffled: exactly n
   unique keys.
4. **The count.** The number of `x`'s rows whose key is shared, tested with a bitmap over
   0..=n + n / 10. That is the query's `expected`.

At N = 1e7, `small` has n = 10 keys (9 shared, 1 left-only, 1 right-only), `medium` has
10,000 (9,000 shared) and `big` has 10⁷ (9,000,000 shared). q5 is the `id3` count, and
`x`'s `id3` column is a permutation of the shared and left-only keys, so q5 = N − N / 10
for every seed: 9,000,000 at N = 1e7, as upstream's.

**The floor.** `scale` must be at least 10000000, upstream's floor. Below it, `small`
(N / 10⁶ keys) has fewer than 10 keys, so n / 10 = 0 and it has no left-only or
right-only key, which would make q1 a join in which every `x` row matches. The floor is a
property of the spec, not of the generator: `KeyPlan::new` accepts any plan with
1 ≤ small ≤ rows and 1 ≤ medium ≤ rows (and rows ≤ `RowId::MAX`), and the tests use
small plans (2,000 / 20 / 200, and one with no left-only key).

### Differences from upstream

The generator is a reimplementation of db-benchmark's `_data/join-datagen.R` that keeps
its structure, not a copy of it. It differs in these ways:

- **Integer split.** n − n / 10 shared keys instead of R's `n * 0.9`, so every N is
  well-defined.
- **Per-query projections.** Not one `x(id1, id2, id3)` but `x_id1(id1, id3)`,
  `x_id2(id2, id3)` and `x_id3(id3)`. A `_` is still a trie level, so a shared `x` would
  descend its unused columns on every query, under either column-order policy.
- **The carried `id3`.** Each `x` projection carries `id3`, which is unique per row.
  Every result row is therefore distinct and no relation holds a duplicate, so
  `expected` is the same for all three cells, HashTrie's multiset count included.
- **Dropped.** q3 (a left outer join), q4 (string keys, which under dictionary encoding
  are q2), the `v1`/`v2` payloads, the right-hand tables' extra columns and NAs.
- **The random-number generator.** Upstream draws from R's. Kermit's is in-repo
  (below), so its draws, and with them q1's and q2's counts, differ from upstream's. At
  N = 1e7 upstream's counts are q1 = 8,997,136, q2 = 9,001,606 and q5 = 9,000,000.
  Kermit's q5 is the same 9,000,000, by construction; its q1 and q2 land near 9.0
  million like upstream's but do not equal them. The values are recorded under
  [Measured at N = 1e7](#measured-at-n--1e7).

### Random numbers

`kermit-generate/src/h2o/rng.rs` holds the generator, in-repo because `rand`'s `StdRng`
and its sampling algorithms are not value-stable across versions, and a generator's
output must be a pure function of its spec (#74):

- **SplitMix64** (Steele, Lea and Flood, 2014; Vigna's `splitmix64.c`), tested against
  the reference implementation's output.
- **Bounded draws** by Lemire's nearly divisionless method (2019): unbiased values in
  `[0, n)` from a 128-bit multiply.
- **Shuffles** by Fisher–Yates (Durstenfeld, 1964): for `i` from `len − 1` down to 1,
  swap `i` with a draw in `[0, i]`.
- **Streams.** Every (column, step) pair has its own generator. The columns are `id1`,
  `id2`, `id3` (0, 1, 2) and the steps `Pool`, `Pad`, `LeftShuffle`, `RightShuffle`
  (0 to 3); a stream's state starts at SplitMix64's output function applied to
  `seed << 32 | (column × 4 + step)`. That input is injective and the function is a
  bijection, so each stream starts at its own point on the 2⁶⁴ cycle, and each
  relation's data depends only on `(plan, seed, order)`, not on the order the relations
  are generated or written in. The numbering is fixed: renumbering a column or a step
  changes every relation the generator writes.

### Row order (the `order` axis)

- **`shuffled`** (the default): rows are in the order the generator draws them, as
  upstream writes them.
- **`sorted`**: each relation's rows are sorted lexicographically (`Tuples::sort`, the
  sort both sorted tries build with) just before they are written.

The draws and the counts are the same in both modes, so a sorted relation equals its
shuffled counterpart sorted, and `expected` is identical. `x_id1` sorted is grouped by
`id1`, with ascending `id3` inside each group. HashTrie builds from the order its rows
arrive in, so the same data in the two orders separates a structure's cost from the
cost of input locality (#101). The order is a field of the spec, so the two orders are
two benchmarks with two caches (`h2o` and `h2o-sorted`).

## Workload reference

### The relations

| Relation | Columns | Rows | Holds |
|----------|---------|------|-------|
| `x_id3` | `id3` | N | `x`'s `id3` column: a permutation of the shared and left-only keys |
| `big` | `id3` | N | `big`'s keys: shared and right-only |
| `x_id1` | `id1`, `id3` | N | `x`'s `id1` column paired with its `id3` column |
| `small` | `id1` | N / 10⁶ | `small`'s keys |
| `x_id2` | `id2`, `id3` | N | `x`'s `id2` column paired with its `id3` column |
| `medium` | `id2` | N / 10³ | `medium`'s keys |

This is also the order the files are written and the order of `benchmark.yml`'s
relations. Keys are small positive integers, used as `usize` directly: there is no
dictionary.

### The queries

| Name | Datalog | Isolates |
|------|---------|----------|
| `q1` | `Q(K, I) :- small(K), x_id1(K, I).` | Leaf enumeration: 9 matching keys with about a million children each at 1e7. Upstream's "small inner on int" |
| `q2` | `Q(K, I) :- medium(K), x_id2(K, I).` | About 9,000 matching keys with about a thousand children each. Upstream's "medium inner on int" |
| `q5` | `Q(K) :- big(K), x_id3(K).` | A dense intersection of two key lists that overlap by 90%, which separates the seek strategies (#80). Upstream's "big inner on int" |

- **Every query is valid under `--column-orders stored`**: each atom's variables follow
  its stored column order, so no query needs a reordered copy and none is a
  `CyclicAttributeOrder`.
- **No query repeats a variable in an atom or uses a constant**, so none uses the
  selection or const rewrites; they run through the plain join path.
- **Counts are exact and the same on all three cells**, because no relation holds a
  duplicate row (see [Differences from upstream](#differences-from-upstream)).

### Verified counts

The reference plan is N = 2,000, `small` = 20, `medium` = 200 with seed 108 (the tests'
plan; it is below the spec's floor). Its counts, computed by a Python model written
from the spec alone, which shares no code with the Rust:

| | `q1` | `q2` | `q5` |
|---|---|---|---|
| reference plan, seed 108 | 1,812 | 1,814 | 1,800 |
| N = 1e7, seed 108 (`h2o`, `h2o-sorted`) | 8,999,658 | 9,000,406 | 9,000,000 |

They are the same in both row orders. `kermit/tests/h2o_generate.rs` pins them, and
`kermit-generate/tests/h2o.rs` pins SHA-256 digests of every relation's data.

### How `expected` is computed

Every query's `expected` is the count of step 4 of the key generation, taken from
`x`'s unsorted columns. It is a bitmap lookup over the key range, which shares no code
with the joins, so a bug in a trie, an intersection routine or an optimiser cannot make
the count wrong in the same way. The generator's tests check it against a brute-force
join (count each right-hand table's keys in a `HashMap`, then sum over `x`'s rows) in
both orders.

`bench run h2o -i all -a all --verify` then runs each query once and aborts on a
mismatch.

## Determinism

The output is a pure function of `(scale, seed, order)`: the same spec always gives the
same rows in the same order and the same counts. The Parquet bytes are identical when
regenerating with the same build; they can differ across builds, since the file's footer
records the Parquet writer's version. `meta.json` records the seed, the order, the
plan and each relation's SHA-256, and `benchmark.yml` carries the same digests, so
a bare `bench fetch`, or `bench fetch h2o-<N>-<tag>` with the cache's own name, verifies a
generated cache, and a cache can be identified after the fact. `bench fetch h2o` reads
the workspace YAML, which declares no relations, and reports 'No integrity hashes
declared.'

`kermit-generate/tests/h2o.rs` pins the data itself: SHA-256 digests over each relation's
values (not its file bytes) for the reference plan, in both orders. If the digests must
change, the output of an existing spec has changed: bump `META_SCHEMA_VERSION` and add an
`h2o` floor to `OUTDATED_BEFORE`, so that old caches show as `stale` and `bench run`
refuses them, rather than passing as hits.

## Practical scale ceiling

N is the cost. The queries stream every answer to a counter and never collect, but
timing them still enumerates every row, and q1 and q2 each return close to N − N / 10
rows.

| Quantity | Cost | Notes |
|----------|------|-------|
| Loading a relation | 8 B per column per row | Since #111 a relation is one flat `usize` buffer. The six relations hold about 6N values, so the loaded batches alone are about 48N bytes: 480 MB at N = 1e7. This is before any structure is built over them |
| Generating | about 4N words at the peak | The kept `id3` column, the `id1` (or `id2`) column and the 2N-word interleaved buffer for the two-column relation: about 3.2 GB at N = 1e8 |
| Row count | at most `RowId::MAX` = 4,294,967,295 | A relation's rows are addressed by a 4-byte `RowId`. `scale` is a `u32`, so its largest value is the same, and `KeyPlan::new` rejects a plan above it |

- **Space per structure and the per-structure ceilings** on this host's memory are
  recorded under [Measured at N = 1e7](#measured-at-n--1e7).
- **Never write a result to disk.** `kermit join` writes every row as CSV; q1 and q2 at
  1e7 each return about nine million rows. `bench run` only counts.
- **`bench run --all` includes both H2O benchmarks**, and generating each takes the time
  and memory above on first use.

## Vendored generator

Nothing is vendored: the data is generated from a seed, and kermit's code is a
reimplementation of upstream's, not a copy of it.

| Field | Value |
|---|---|
| Upstream | db-benchmark's join data generator, `_data/join-datagen.R` (MPL-2.0); <https://github.com/h2oai/db-benchmark> |
| Version pin | none: no upstream code is copied or run; the spec records the structure kermit keeps |
| Build toolchain | none (no external tool runs) |
| Vendor path | none |
| Committed vs fetched | nothing committed or fetched; the data is generated on first use and cached |

To regenerate, run `bench run --force <name>`, or delete the cache directory.

## Tests

| Test | What it validates | Gate |
|------|-------------------|------|
| `kermit-bench/src/definition.rs` (`h2o_*` and `an_unknown_h2o_order_fails_to_parse`) | `H2oOrder`'s names match their serde spelling and default to `shuffled`; an unknown order fails to parse and names `shuffled`; a spec parses with `seed` 108 and `order` `shuffled` and serialises to exactly that; `order: sorted` parses; `spec_hash` changes with `scale`, `seed` and `order`, and an omitted `seed` hashes as 108; 9,999,999 is rejected and 10,000,000 accepted | always |
| `kermit-generate/src/h2o/rng.rs` | SplitMix64 reproduces `splitmix64.c`'s outputs for two seeds; three streams' first outputs are pinned and the twelve (column, step) streams differ; `bounded` matches a reference model, including a case that exercises Lemire's rejection loop, stays in range at n = 1, 2ᵏ and 2ᵏ ± 1, and passes a chi-square check on n = 10; `bounded(0)` panics; a shuffle is a permutation and, for 10 and 20 elements from seed 3, equals a reference model's output | always |
| `kermit-generate/src/h2o/keys.rs` | The pool splits have the right sizes, including n = 7 where n / 10 = 0; the right-hand table is the shared and right-only keys with no duplicate; `x`'s column holds every shared and left-only key and no right-only key, and is a permutation at n = N; `matches` equals a `HashSet` count; `KeyPlan::new` rejects zero and oversize cardinalities and rows above `RowId::MAX`, and a deserialized plan is checked the same way (`small = 0` fails to parse); for 20 keys and seed 108 the pool, `x`'s column and the right-hand table equal a reference model's output; `for_scale` divides by 10⁶ and 10³ and rejects 999,999 | always |
| `kermit-generate/tests/h2o.rs` | Six relations and three queries, in order, with the right columns and sizes; each `expected` equals a brute-force join over the written relations, in both orders; the reference plan's data digests and counts are pinned; the same spec reproduces and another seed differs; a sorted relation is its shuffled counterpart sorted; a plan with no left-only key generates | always |
| `kermit/src/materialize.rs` (`h2o_cache_misses_hits_drifts_and_regenerates`) | A 1,000,000-row spec (below the floor, which `materialize` does not check) generates on a miss, is a hit the second time, raises `SpecDrift` after a seed change and regenerates under `force` | always |
| `kermit/src/main.rs` (`h2o_clap_names_match_the_spec_names`, `h2o_flags_default_and_enforce_the_floor`, `h2o_bench_name_is_scale_and_tag`) | The CLI's order names equal the spec's serde names; `--seed` and `--order` default as documented and `--scale` enforces the floor; the benchmark name is `h2o-<scale>-<tag>` | always |
| `kermit/tests/cli_gen_scale_range.rs` (`h2o_scale_below_the_floor_is_a_usage_error`) | `bench gen h2o --scale 1000` exits 2, names `'1000'`, `--scale` and 10000000, and writes no cache | always |
| `kermit/tests/h2o_generate.rs` | The committed YAMLs declare `kind: h2o` at 10000000 in the right order; the reference plan, in both orders, joins to its `expected` count for all three queries on the three cells × every optimiser × both `--column-orders` policies; the emitted counts are the model's 1,812, 1,814 and 1,800 | always |
| `bench run h2o -i all -a all --verify`, and the same for `h2o-sorted`, at N = 1e7 | Every cell verifies through the path `bench run` takes (`ExecutionFamily::load`), which the integration test does not | manual; release build inside `nix develop`; a heavy run, see [Measured at N = 1e7](#measured-at-n--1e7) |

## References

- G. L. Steele Jr., D. Lea, C. H. Flood. *Fast splittable pseudorandom number
  generators.* OOPSLA 2014.
- S. Vigna. `splitmix64.c`, the reference implementation: <https://prng.di.unimi.it/>
- D. Lemire. *Fast random integer generation in an interval.* ACM TOMACS 29(1), 2019.
- R. Durstenfeld. *Algorithm 235: Random permutation.* CACM 7(7), 1964.
- H2O.ai db-benchmark: <https://github.com/h2oai/db-benchmark>
- Design: `docs/specs/2026-10-15-h2o-benchmark-design.md`
- The generator: `kermit-generate/src/h2o/mod.rs`, `kermit-generate/src/h2o/rng.rs` and
  `kermit-generate/src/h2o/keys.rs`
- Sibling benchmark references: [`LUBM.md`](LUBM.md), [`WATDIV.md`](WATDIV.md),
  [`SNAP.md`](SNAP.md)

## Future work

- **q3, q4, the payload columns and NAs.** q3 (a left outer join) has no Datalog
  counterpart, q4 duplicates q2 under dictionary encoding, and the payloads and NAs do
  not reach a join on integer keys.
- **H2O in the thesis sweep.** The sweep's scripts name their benchmarks, and adding `h2o`
  and `h2o-sorted` to them is a separate decision (`bench run --all` already includes
  them).
- **A structure's memory ceiling at large N.** This page measures and documents the
  ceilings; fixing any structure's is out of scope.

## Measured at N = 1e7

These are the acceptance runs of 2026-10-15, at commit fdf658c. They used a release build
(jemalloc) on an AMD Ryzen 7 7700X (8 cores, 16 threads) with 30 GiB of RAM, with the
host's timing chain paused and no other heavy work running. The run directory is
`kermit-bench-runs/h2o-acceptance-2026-10-15`, outside the repository.

### Counts

| Spec | `q1` | `q2` | `q5` |
|---|---|---|---|
| `h2o` and `h2o-sorted` (seed 108) | 8,999,658 | 9,000,406 | 9,000,000 |
| `bench gen h2o --seed 7` | 9,000,061 | 9,001,863 | 9,000,000 |
| upstream db-benchmark, `set.seed(108)` | 8,997,136 | 9,001,606 | 9,000,000 |

`q5` is N − N / 10 by construction and equals upstream's count. `q1` and `q2` depend on
the draws. kermit's SplitMix64 streams are not R's generator, so the same seed draws
different keys (see [Random numbers](#random-numbers)). `bench run h2o -i all -a all
--verify` and the same run for `h2o-sorted` checked all three queries on all three
cells, and all 18 checks passed.

### Space

These numbers come from `bench run … -m space`, in bytes as `heap_size_bytes()` reports
them (MB = 10⁶ bytes). They are identical under `shuffled` and `sorted`, because each
structure holds the same contents with the same capacities.

| Relation | TreeTrie | ColumnTrie | HashTrie |
|---|---:|---:|---:|
| `x_id3` | 536.9 MB | 134.2 MB | 522.7 MB |
| `big` | 536.9 MB | 134.2 MB | 522.7 MB |
| `x_id1` | 335.5 MB | 134.2 MB | 703.3 MB |
| `small` | 512 B | 208 B | 504 B |
| `x_id2` | 400.6 MB | 134.5 MB | 692.4 MB |
| `medium` | 0.5 MB | 0.1 MB | 0.5 MB |
| **Total** | **1,810.4 MB** | **537.3 MB** | **2,441.6 MB** |
| Per unit of N | 181 B | 54 B | 244 B |

### Generation

`bench gen h2o --scale 10000000` took 2.2 s of wall time, with a peak RSS of 375 MB
(GNU `time -v`). That is close to the estimate of about 4N words, 320 MB. It wrote 338 MB
of Parquet (316 MB under `sorted`). Each `bench run --verify -m space` above took about
10 minutes of wall time, with a peak RSS of 2.5 GB.

### Determinism

Three generations of the shuffled spec gave identical per-relation SHA-256s in
`meta.json`: `bench run h2o`'s cache and two `bench gen h2o` runs. So did `bench gen h2o
--order sorted` and `h2o-sorted`'s cache. `bench gen h2o --seed 7 --order sorted`
recorded seed 7 and order `sorted` in its `meta.json`, and wrote different relations.

### Per-structure ceilings

Extrapolated linearly from the totals above, the six relations alone fill this host's
30 GiB at about N = 1.8 × 10⁸ for TreeTrie, 6.0 × 10⁸ for ColumnTrie, and 1.3 × 10⁸ for
HashTrie. These are upper bounds: they leave out the load buffers, the queries' working
memory and everything else on the host. All three are well below `RowId::MAX`.
