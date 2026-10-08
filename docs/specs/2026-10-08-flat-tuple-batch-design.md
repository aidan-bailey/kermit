# Flat Tuple Batches: One Buffer per Relation, Row Ids in HashTrie

**Date:** 2026-10-08
**Status:** Design approved in conversation (2026-10-08), section by section; not yet
planned.
**Scope:** Issue #111. Every relation's tuples travel from the file readers to every
structure's build as one row-major buffer (`Tuples`) instead of a `Vec<usize>` per
tuple. TreeTrie and ColumnTrie build from row slices. HashTrie keeps the buffer it is
given, and its chains, singletons and pending lists hold row ids into it.
`HashTrieIterator::leaf_tuples` returns a view of those rows. This is "Spec A" of two.
"Spec B" is #101's layer 3: HashTrie copies its buffer into hash partitions and threads
Algorithm 2's lists through per-row chain links. It is designed separately, after this
one lands.
**Related:**
- #111 (this issue);
- #101 (why partitioned builds keep the input-order penalty, and layer 3);
- #105 (the paper-convergence board);
- #107 (Algorithm 2; its record names the per-chain `Vec` as kermit's);
- #112 (jemalloc);
- #94 (the parallel builds and their glibc plateau);
- #106 (pointer tagging);
- the #101 locality run, `kermit-bench-runs/hash-trie-locality-2026-10-08/`.

## Summary

Each tuple is a `Vec<usize>` today, one heap allocation each:
- made when a file is read, and again whenever a benchmark clones its input;
- freed inside every timed TreeTrie and ColumnTrie build;
- kept by HashTrie as the contents of its leaf chains.

This design replaces that with:
- **`Tuples`** (in `kermit-iters`): `arity`, a row count and a row-major `Vec<usize>`. Every constructor takes `impl Into<Tuples>`, and the readers produce it directly.
- **TreeTrie and ColumnTrie** sort the buffer's rows in place with one shared sort and build from row slices.
- **HashTrie** owns the buffer it is given. A chain is a `Vec<RowId>`, a pruned singleton a `RowId`, and a lazy pending list a `Vec<RowId>`, where `RowId = u32`.
- **`LeafRows<'a>`**, a view of a chain (row ids plus the buffer), is what `HashTrieIterator::leaf_tuples` returns. HashTriejoin reads rows through it.

Each structure's build is the same algorithm as before; only its tuples change form.

## Motivation

- **Per-tuple allocation sits in the timed paths.** It is in every `insertion` setup clone (10⁷ allocations at 10⁷ tuples) and in the frees of every sorted-trie build. TreeTrie also re-`collect()`s a tuple's suffix at every level of `insert_into_children`. Under glibc the frees capped TreeTrie `parallel:16` at about 1.9× (#94; workers free buffers the main thread allocated). jemalloc (#112) removed that cap from the binary, but not the allocations.
- **The sort chases pointers.** TreeTrie and ColumnTrie sort `Vec<Vec<usize>>` with a comparator that dereferences two heap pointers per comparison (sort IPC 0.45–1.2, #111's profile).
- **The paper's build starts from a materialised buffer.** §3.3.2: "the incoming tuples are materialized contiguously in an in-memory buffer prior to running Algorithm 2", in a fixed-length layout, with 8 bytes per tuple reserved for the chain pointer that threads the leaf lists. Its leaves point into that memory. kermit's leaves own scattered per-tuple allocations.
- **The #101 locality run (2026-10-08) located the input-order penalty in tuple memory.** It separated the order a build receives tuples (G grouped, S shuffled) from the order their values were allocated, on `friendof`, 15 replicates, uncontended. For `bulk`:
  - GG 304 ms;
  - SS 2.32×;
  - GS 3.07× (grouped arrival, scattered memory, and the slowest);
  - SG 2.42×.

  DRAM fills rose from 0.44 to about 2.1 per tuple. Fixing that is Spec B: partition the buffer as the paper does. Spec B needs a buffer, which is this design.

## Decisions

Made in conversation, 2026-10-08:

1. **Two specs: #111 first, #101 layer 3 after it.** The API breaks once, here, and Spec B builds on this storage.
2. **HashTrie chains hold row ids, in input order.** The alternatives were:
   - **Contiguous ranges:** a random-read gather in every build, `insert` breaks contiguity, and kermit's own invention.
   - **Chain links now (the paper's Figure 3):** the paper uses them only inside its partitioned buffer. Without that buffer, the parallel builds would write links of scattered rows into one shared array, which needs atomics, since the workspace has no `unsafe`.

   Links arrive with the partitioned copy in Spec B, as the paper pairs them.
3. **Row-major layout,** as #111 proposes and as the paper materialises tuples: a tuple is one slice.
4. **`RowId = u32`:** a relation holds at most `u32::MAX` (≈ 4.29 × 10⁹) rows (the paper's largest input is 1.2 billion edges). The paper's pointers are 8 bytes; 4-byte ids are kermit's and are documented as such.
5. **`Tuples` and `LeafRows` live in `kermit-iters`,** the crate both `kermit-ds` and `kermit-algos` depend on. No new crate.
6. **Constructors take `impl Into<Tuples>` and `insert` takes `impl AsRef<[usize]>`,** so the ~500 `vec![vec![…]]` test fixtures compile unchanged. Nothing uses `dyn Relation`, so generic methods cost nothing.

## Background: where tuples flow today

From a census of the workspace (2026-10-08):

- **kermit-ds API.** `Relation::{from_tuples, insert, insert_all}`, `ConfigurableRelation::from_tuples_with_config`, `BuildModeRelation::from_tuples_with_build_mode`, `ConfiguredBuildModeRelation::from_tuples_with_config_and_build_mode`, `read_csv`, `read_parquet` and `HashTrie::collect_tuples` all take or return `Vec<Vec<usize>>`. `TupleScan::scan_tuples` and `TrieIteratorWrapper::advance` already lend `&[usize]`.
- **HashTrie stores the input `Vec`s:**
  - `Leaf(HashTable<Vec<Vec<usize>>>)`;
  - `Singleton(P::Payload)` with `Payload = Vec<usize>` under `SingletonPruning`;
  - `LazyChild { pending: RefCell<Vec<Vec<usize>>> }`.

  `heap_size_bytes` counts every tuple's `Vec`. Each build mode moves tuples:
  - `bulk`'s `TupleList`;
  - `radix`'s `(usize, Vec<usize>)`;
  - `morsel::scatter`'s `Positioned`;
  - `presized`'s `push_in_run` hand-back.
- **The join reads chains as `&[Vec<usize>]`.** `HashTrieIterator::leaf_tuples` is implemented by `HashTrieIter`, `HashTrieIterKind`, `SingletonHashTrieIter` (a cached `vec![vec![value]]`) and `EqualitySelectionHashTrieIter`. The last clones every surviving tuple per leaf bucket visited. HashTriejoin's `emit_leaf` takes `chain(k).len()` and `chain(k)[i]` in an odometer.
- **TreeTrie and ColumnTrie keep no input `Vec`s.** Both sort with the same hand-rolled comparator, deliberately, so the sort costs the same in each (`column_trie/implementation.rs`). TreeTrie's `parallel:N` scatters tuples through `morsel::scatter`.
- **The binary clones per tuple** in:
  - `load_with_tuples`;
  - every `iter_batched` setup in `bench ds` and `bench run`;
  - `IndexSpec::permute_all` (a `Vec` per tuple per column-order copy);
  - `SortedTrieFamily::tuple_count` (`trie_iter().into_iter().count()`).

## Design

### `Tuples` and `RowId` (`kermit-iters/src/tuples.rs`)

```rust
/// A row's index in its batch.
pub type RowId = u32;

/// Tuples of one arity, row-major: row r is data[r*arity .. (r+1)*arity].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tuples {
    arity: usize,
    len: usize,        // explicit, so a nullary relation keeps its row count
    data: Vec<usize>,
}

impl Tuples {
    pub fn new(arity: usize) -> Self;
    pub fn with_capacity(arity: usize, rows: usize) -> Self;
    pub fn arity(&self) -> usize;
    pub fn len(&self) -> usize;
    pub fn is_empty(&self) -> bool;
    pub fn row(&self, r: RowId) -> &[usize];
    pub fn rows(&self) -> impl ExactSizeIterator<Item = &[usize]>; // len empty slices at arity 0
    pub fn push(&mut self, row: &[usize]);                          // panics on a wrong arity
    pub fn as_flat(&self) -> &[usize];
    pub fn sort(&mut self);                                         // lexicographic, in place
    pub fn heap_size_bytes(&self) -> usize;                         // data capacity × 8
}

impl From<Vec<Vec<usize>>> for Tuples { … } // arity from the first row; panics on mixed
                                            // arity; an empty Vec is an empty batch
```

**Rules:**
- `push` panics once the row count would exceed `u32::MAX`.
- **`sort`:**
  - **Arity 1–4:** sorts `data.as_chunks_mut::<N>()` with `sort_unstable`. Arrays compare lexicographically, so the order is today's comparator's.
  - **Other arities:** sorts a `RowId` permutation by row comparison, then gathers into a new buffer.
- A batch handed to a constructor is accepted when it is empty or its arity equals the header's. An empty literal therefore needs no arity.

### API (`kermit-ds`)

| Item | Today | After |
| --- | --- | --- |
| `Relation::from_tuples(header, …)` | `Vec<Vec<usize>>` | `impl Into<Tuples>` |
| `Relation::insert(&mut self, …)` | `Vec<usize>` | `impl AsRef<[usize]>` |
| `Relation::insert_all(&mut self, …)` | `Vec<Vec<usize>>` | `impl Into<Tuples>` |
| `from_tuples_with_config` / `_with_build_mode` / `_with_config_and_build_mode` | `Vec<Vec<usize>>` | `impl Into<Tuples>` |
| `read_csv`, `read_parquet` | `(RelationHeader, Vec<Vec<usize>>)` | `(RelationHeader, Tuples)` |
| `HashTrie::collect_tuples` | `Vec<Vec<usize>>` | `Tuples` (a copy of its buffer) |
| `project_via_trie_iter` | collects `Vec<Vec<usize>>` | collects a `Tuples` through `advance` |

**Readers:**
- `read_csv` parses each record into the buffer with no per-row allocation.
- `read_parquet` transposes each record batch's columns into rows.

Both return a buffer whose capacity equals its length, shrinking once after reading, outside any timed region. HashTrie's `space` then does not depend on reader growth. A clone is exact anyway.

**Unchanged:** the output side (`JoinAlgo::join_iter`, and `lftj_join` / `hash_join` returning `Vec<Vec<usize>>`). It is a join's result, not a relation's input.

### HashTrie: owned buffer, row-id chains

```rust
pub struct HashTrie<H, P, E> {
    header: RelationHeader,
    tuples: Tuples,              // every stored tuple, arrival order; tuple_count = tuples.len()
    root: HashTrieNode<P, E>,
    config: HashTrieConfig,
    _layout: PhantomData<(H, P, E)>,
}
// Leaf(HashTable<Vec<RowId>>)       a chain: its tuples' row ids, input order
// Singleton(P::Payload)             SingletonPruning: Payload = RowId; NoPruning: Never
// Unexpanded(..)                    pending list: Vec<RowId>
```

**Changes:**
- **Singletons:** `SingletonPayload`'s `from_tuple` / `tuple` / `into_tuple` become `from_row` / `row` / `into_row` over `RowId`.
- **Pending lists:** `PendingChild` over `Vec<RowId>`.
- **Build modes** pass row ids where they passed `Vec<usize>`:

| Mode | Change |
| --- | --- |
| `bulk` | `TupleList = Vec<RowId>`; the root list is `0..len`; `group` hashes `tuples.row(id)[depth]`. The batch is passed down by reference. |
| `incremental`, `Relation::insert` | push the row onto the buffer, then `insert_at(id)` |
| `radix:K` | `Indexed = (usize, Vec<usize>)` becomes `RowId`: a row's id *is* its input position, which the first-appearance merge orders by |
| `parallel:N`, `presized:N` | `morsel::scatter` emits row ids per partition; `push_in_run` hands back a `RowId`. Workers push ids into their own lists, so nothing shared is written. |
| lazy `resolve` | reads rows from `self.tuples` (it already runs on `&self`) |

- **Identity.** Two builds of one input own identical buffers, so equal id lists mean equal tuples. `assert_same_trie` and `assert_same_chain` compare ids, as strictly as they compare tuples today. Every mode stays array-identical (`presized:N` equivalent, Amendment 2).
- **Reading back:**
  - `for_each_tuple` / `scan_tuples` iterate the buffer in input order: no trie walk, and still no lazy expansion.
  - `collect_tuples` copies the buffer.
  - `project` builds its projected buffer from it.

  Statistics need only the multiset, so the order change is harmless. It is noted in the doc.
- **`space`** counts the buffer (capacity × 8), the id lists (capacity × 4) and the tables. Per tuple, at arity 2: 24 B of `Vec` header in the chain plus a 16 B buffer today, against 16 B in the buffer plus a 4 B id after. Each chain is still one `Vec`: the recorded divergence that Spec B removes.
- **Locality is unchanged.** Rows stay in arrival order, so on shuffled input the child pass still reads a key's rows at random. That is two dependent loads per tuple before (header, buffer) and after (id, row). The #101 penalty is not expected to move.

### The leaf view and the join (`kermit-iters`, `kermit-algos`)

```rust
#[derive(Clone, Copy, Debug)]
pub struct LeafRows<'a> {
    data: &'a [usize],
    arity: usize,
    rows: &'a [RowId],
}
impl<'a> LeafRows<'a> {
    pub fn new(tuples: &'a Tuples, rows: &'a [RowId]) -> Self;
    pub fn from_parts(data: &'a [usize], arity: usize, rows: &'a [RowId]) -> Self;
    pub fn len(&self) -> usize;
    pub fn is_empty(&self) -> bool;
    pub fn row(&self, i: usize) -> &'a [usize];          // the chain's i-th tuple
    pub fn ids(&self) -> &'a [RowId];
    pub fn iter(&self) -> impl ExactSizeIterator<Item = &'a [usize]>;
    pub fn to_vecs(&self) -> Vec<Vec<usize>>;            // for tests and diagnostics
}

pub trait HashTrieIterator: … {
    …
    fn leaf_tuples(&self) -> Option<LeafRows<'_>>;
}
```

**Implementors:**
- **`HashTrieIter`:**
  - **At a leaf:** `LeafRows::new(&trie.tuples, chain)`.
  - **At a pruned singleton at leaf depth:** the same over `slice::from_ref(id)`. `SingletonFrame` and `Descent::Deeper` borrow `&'a RowId` where they borrow `&'a Vec<usize>`.
- **`SingletonHashTrieIter`:** `chain: Vec<Vec<usize>>` becomes `data: [usize; 1]` and `rows: [RowId; 1] = [0]`, via `from_parts`. The struct no longer holds a heap `Vec`.
- **`EqualitySelectionHashTrieIter`:** `refresh_leaf` filters the inner view's ids into an owned `Vec<RowId>`. `leaf_tuples` returns `from_parts(inner.data, inner.arity, &self.ids)`. Ids are copied instead of tuples.
- **`HashTrieIterKind`:** delegates.
- **HashTriejoin's `emit_leaf`:** `chain(k)[i].as_slice()` becomes `chain(k).row(i)`. The odometer, `chain_lens` and `verify_and_construct` are unchanged, and the leaf product still allocates nothing per result.

### TreeTrie and ColumnTrie

**Both** sort the buffer with `Tuples::sort`, one function, which keeps the invariant that the sort costs the same in each.

**TreeTrie:**
- `from_tuples` inserts each sorted row as a slice. `insert_into_children(children, tuple: &[usize])` recurses on `&tuple[1..]`, with no allocation per level.
- The buffer is dropped once, at the end: one free where there were n.
- **`parallel:N`:**
  - `first_key_splitters` reads the buffer.
  - `morsel::scatter` emits row ids per partition (`Partition { segments: Vec<Vec<RowId>> }`, input order within a partition).
  - Each worker gathers its partition's rows into a local `Tuples` (ascending ids, so mostly forward reads), sorts it and builds its subtrie.

**ColumnTrie:**
- `from_sorted` iterates row slices. Its `previous: Option<Vec<usize>>` becomes the previous row's index.
- `incremental` (`internal_insert(&[usize])`) is already slice-based.

Neither keeps the buffer, so their `space` is unchanged.

### The binary (`kermit/src`)

**Signatures:**
- `RelationFamily::build_relation(header, Tuples)`;
- `ExecutionFamily::build_from_tuples(Vec<(RelationHeader, Tuples)>)`;
- `add_index(…, &Tuples)`;
- `db::build_index(spec, base, &Tuples)`;
- `IndexSpec::permute_all(&Tuples) -> Tuples`, one buffer per copy;
- `compute_join` takes `impl Into<Tuples>` per input.

**Copies:**
- `load_with_tuples` keeps one buffer copy.
- `kermit join --column-orders any` keeps a `Tuples` per relation.
- Every `iter_batched` setup clone in `bench ds` and `bench run` is one `Vec<usize>` copy, so `insertion` times the build alone.
- `SortedTrieFamily::tuple_count` returns `Cardinality::tuple_count`. That is already a bound of `SortedTrieRelation`, and TreeTrie and ColumnTrie answer it from a stored count, so nothing is walked or allocated.

## Parity with the paper

| Element (VLDB 2020 §3.3.1–3.3.2, Fig. 3) | Today | After this design | After Spec B |
| --- | --- | --- | --- |
| Tuples in one contiguous buffer, fixed-length layout | ✗ a `Vec` per tuple | ✓ the relation's buffer, row-major | ✓ |
| Leaves, singletons and lazy children refer to tuple memory | ✗ they own the tuples | ✓ by row id (4 B; the paper's pointers are 8 B) | ✓ |
| A leaf is a list threaded through an 8-byte chain pointer per tuple | ✗ a `Vec` per chain | ✗ a `Vec<RowId>` per chain (kermit's) | ✓ |
| Tuples partitioned by the first key's hash as they are materialised | ✗ | ✗ arrival order | ✓ |

The paper's buffer is a copy its build makes. Here HashTrie takes the relation's buffer without copying, so nothing new enters the timed build. Spec B adds the partitioned copy, and with it the paper's cost.

## Placement under the standard

This is not an optimisation with an axis. It changes the representation every structure uses, unconditionally, like #112's allocator. No `--ds-*` flag selects the old form, and a non-user would have nothing to opt out of.

- **Old and new are told apart** by the report's `schema_version` (below) and by binary A/Bs.
- **Priority 6.** It touches all three structures by necessity, as one declared cross-cutting change. No build algorithm changes; only the tuples' form does.

## CLI, reports and kermit-lab

- **No CLI change.**
- **`REPORT_SCHEMA_VERSION` 3 → 4, and kermit-lab's `SCHEMA_VERSION` with it** (the pin test checks they match). `insertion` times a different input path and sort, and HashTrie's `space` counts a different representation: both are rules for a bump. kermit-lab then refuses to mix v3 and v4 reports without `allow_mixed_schema=True`.

## Testing

- **Existing suites run unchanged,** through `From<Vec<Vec<usize>>>` and `AsRef<[usize]>`:
  - the 16-pattern join suites (all aliases, configs, build modes and column orders);
  - `relation_trie_test_suite!`, `hash_trie_test_suite!` and `parquet_test_suite!`;
  - the BuildMode identity and `space` identity tests;
  - `result_allocation.rs`;
  - `column_orders_*`;
  - the LUBM oracles.

  Tests that compare a chain with tuples use `LeafRows::to_vecs`.
- **New `Tuples` tests:**
  - arity 0 with a nonzero count;
  - an empty `From`;
  - a push with the wrong arity panics;
  - `rows()`;
  - `sort` against today's comparator on seeded random batches at arities 1–5, which also covers the permutation fallback;
  - `row` past the end panics.
- **New `LeafRows` tests,** through all four `HashTrieIterator` implementors: leaf, pruned singleton, constant view and selection view.
- **New allocation tests** (the binary's tests already count allocations):
  - reading N rows (CSV and Parquet), cloning a batch and handing it to a constructor allocate O(1) in N;
  - each build allocates no more than its own nodes, tables and chains, with a bound that fails if any per-tuple allocation returns.

## Measurement

A replicated A/B in the #107 style, after landing, in its own run directory:
- old binary (this design's base) against new;
- 5 replicates with alternating order, a distinct `--name` per step, the quiet gate and the host sampler.

**Parts:**
- **A — `insertion` and `space`:**
  - TreeTrie `serial` and `parallel:16`;
  - ColumnTrie `bulk` and `incremental`;
  - HashTrie `bulk`, `radix:12`, `parallel:16` and `presized:16` (with `root-capacity=tuples`);
  - on the 12 relations of the 2026-10-06 runs and the shuffled `friendof`.
- **B — glibc:** TreeTrie `parallel:{1,16}` on binary-1e7 with `--no-default-features` binaries. This is #111's original criterion: the plateau should lift.
- **C — `iteration`:** `oxford-uniform-s3` triangle on the four HashTrie Layout cells, with TreeTrie as the control. The leaf product now reads through `LeafRows`, so iteration must not regress.

**Expected and not expected:**
- **Expected:**
  - HashTrie `space` about halves per tuple;
  - every `insertion` gets faster;
  - eager `iteration` holds within the codegen bound.
- **Not expected:** #101's order penalty moving. Rows stay in arrival order, and Spec B owns that.

## Out of scope

- **Spec B (#101 layer 3):**
  - the partitioned copy;
  - chain links;
  - removing the per-chain `Vec`;
  - `LeafRows` gaining a walk.

  It also answers #107's open question, whether to store one-tuple lists inline: a chain head lives inside its bucket.
- Pointer tagging (#106).
- The join's output representation.
- A columnar batch (ColumnTrie and Parquet would like one; the paper and HashTrie read whole tuples).

## Sequencing

1. This design lands on its own, CI-green.
2. Then Part A–C's measurement.
3. Spec B is brainstormed after the measurement, against this storage.

The full sweep's binary predates this change, so its `insertion` and `space` numbers stay v3 and are re-baselined separately when the user chooses.

## Acceptance

1. No per-tuple heap allocation between reading a file and building any structure, except HashTrie's chain `Vec`s. Pinned by the allocation tests.
2. Every build mode builds the identical structure. The identity tests pass unchanged.
3. HashTrie stores no `Vec<usize>` per tuple. `leaf_tuples` returns `LeafRows`, and the leaf product allocates nothing per result.
4. The CI gate passes: test, clippy (`-Dwarnings`), fmt, doc (`-Dwarnings`) and python.
5. `REPORT_SCHEMA_VERSION` and kermit-lab's `SCHEMA_VERSION` are both 4.
6. The documentation below is updated.

## Documentation

- **ARCHITECTURE.md** and **CLAUDE.md**:
  - `Tuples` and `LeafRows` in the key traits;
  - `Relation`'s constructor signatures;
  - `leaf_tuples`;
  - the JSON-report gotcha's schema version.
- **`docs/data-structures/hash-trie.md`:**
  - Representation;
  - `HeapSize`;
  - the Construction table's "the lists themselves" row (still kermit's, now `Vec<RowId>`, until Spec B);
  - the parity rows;
  - `scan_tuples`' order.
- **`tree-trie.md`, `column-trie.md` and `parallel-build.md`:** the shared sort, row slices and id scatter.
- **`docs/algorithms/hash-triejoin.md`:** the leaf product reads `LeafRows`.
- **`docs/specs/bench-report-schema.md`:** v4, and what changed.
- **BENCHMARKING.md:** v3 and v4 `insertion` and `space` are not comparable.
