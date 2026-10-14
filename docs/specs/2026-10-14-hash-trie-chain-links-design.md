# HashTrie Chain Links: Algorithm 2's Lists Threaded Through a Link per Row

**Date:** 2026-10-14
**Status:** Design approved in conversation (2026-10-14), section by section. Not yet planned.
**Scope:** Issue #101, layer 3, second half ("B2"). Every HashTrie list stops being a
`Vec<RowId>`. That covers Algorithm 2's per-bucket lists, the leaf chains, lazy pending
lists and the radix family's scratch lists. Each becomes a chain threaded through one link
per row, kept in an array beside the tuple buffer: the paper's chain pointer (VLDB 2020
§3.3.2, Figure 3). `LeafRows` learns to walk a chain, and HashTriejoin's `emit_leaf` moves
from indices to cursors.
**Related:**
- #101: layer 3. B1, the partitioned materialisation, landed at a589c64
  (`docs/specs/2026-10-08-hash-trie-partitioned-materialisation-design.md`).
- #105: the paper-convergence board.
- #107: Algorithm 2. Its A/B measured `bulk` at 1.62× `incremental`'s build time on
  `price`, which this design targets.
- #111: flat tuple batches and 4-byte row ids.
- #92: lazy child expansion.
- Amendment 2 in `docs/specs/2026-10-05-parallel-build-design.md`.

## Summary

The trie gets one `ChainLinks` array: a 4-byte "next" id per row, beside its `Tuples`
buffer. A list becomes a `Chain { head, tail, len }` stored in its bucket, and pushing a row
appends it by rewriting two links. Algorithm 2 moves each row from its parent's list to
exactly one child list by rewriting its link, as the paper describes. A build then
allocates per table, never per list. `LeafRows` gains a linked form and a cursor.
`emit_leaf` advances one cursor per relation along the links, and still allocates nothing
per result row.

Every list keeps input order. `bulk` therefore stays array-identical to `incremental`,
`links` included, and the radix family stays equivalent to `bulk` exactly as B1 left it.
No answer, report field or flag changes.

## Motivation

Algorithm 2 (`bulk`, the default since #107) allocates a `Vec<RowId>` for every bucket's
list at every level. That is one heap allocation, and its growth steps, for each distinct
prefix of every tuple. #107's A/B measured `bulk` at 0.80–1.62× `incremental` by input
shape, worst on `price` (1.62×) and `friendof` (1.19×). Both builds allocate a list per
leaf chain, but `bulk` allocates one at every inner level too. #107 also left "inline
one-tuple lists" as an open decision.

The paper has no per-list allocation (§3.3.2): "In addition to the actual tuple data, we
reserve an additional 8 bytes of memory per tuple which is used later to store the tuple
chain pointer required by the linked lists (cf. Figure 3)." A bucket's 8-byte child slot
holds the head of its list. Algorithm 2 (§3.2.2) moves "each tuple in the input linked list
L … to exactly one of the linked lists that are processed recursively", so a single link
per tuple serves every level. This design is that, in safe Rust.

B1 made the radix family's partitions contiguous ranges of the buffer, so a chain's rows,
and their links, sit close together in memory. That is why B2 comes after B1.

## Decisions

The user chose each of these in conversation, from the options and trade-offs shown.

1. **The links are a separate 4-byte array, `links: ChainLinks`, one entry per row**,
   not an 8-byte slot inside each row as in the paper.
   - `Tuples`, the sorted tries and every row reader stay as they are.
   - It continues #111's 4-byte ids in place of the paper's 8-byte pointers.
   - A chain step reads two places (the link, then the row) where the paper reads one
     cache line. The links are dense, and after B1 a chain's rows are partition-local.
   - The in-row layout, and a Layout offering both, were declined. A measurement that shows
     the link's placement matters would reopen them.
2. **Lists append at the tail and keep input order.** The paper's line 7, "push t onto the
   linked list stored in B", read literally, prepends and reverses each level.
   - Appending keeps today's order everywhere. So `bulk` stays array-identical to
     `incremental` (#107), B1's merges and presized tail keep their reasoning, and join
     output order does not change.
   - The bucket holds `head` and `tail`, 8 bytes together, the size of the paper's child
     slot.
   - The paper does not say whether order matters.
3. **Every trie stores its links as `AtomicU32`, accessed `Relaxed`.**
   - A lazy trie must re-thread a child's rows inside `HashTrieIter::open`, which holds
     only `&self`. Atomics allow that without `unsafe`.
   - They also keep eager tries `Sync`, which a test pins, and give `LeafRows` one linked
     form.
   - On x86-64 and AArch64 a relaxed load or store compiles to a plain one. Eager tries'
     residual cost (LLVM is more conservative around atomics) is checked by a one-off
     measurement (see Measurement).
   - Declined: a link type chosen by the expansion policy (`Vec<RowId>` eager,
     `Vec<Cell<RowId>>` lazy). It needs a second `LeafRows` linked form and a match per
     chain step. Also declined: keeping `Vec` lists under lazy expansion.
4. **No schema bump.** `bulk`'s process changes inside one structure, as under #107. The
   result is a "4 (no bump)" row in `bench-report-schema.md` plus dated notes on HashTrie
   records measured before B2. TreeTrie and ColumnTrie reports stay mixable.

Defaults the user approved in the design sections:
- `Chain` carries `len`.
- Every push writes `NIL` into the pushed row's link.
- `LeafRows` keeps a `Listed` form for the views that hold their own ids.
- B1's deferred shared radix/parallel helper comes first.
- B1 and B2 are measured in one run.

## Background

**Lists today.** `bulk.rs` defines `TupleList = Vec<RowId>`, and it appears in these places:
- `group` (Algorithm 2 lines 3–7) pushes each row id onto its bucket's `Vec`;
- `build_nested` / `child` (lines 8–15) consume each `Vec`;
- a leaf is `HashTable<Vec<RowId>>`;
- a lazy child holds `RefCell<Vec<RowId>>`;
- `radix.rs` and `parallel.rs` build scratch `HashTable<TupleList>`s, and `presized:N`
  builds a presized table of lists.

`child` special-cases two short lazy lists' capacities, so `space` matches `incremental`'s.
A leaf bucket is `Option<Entry<Vec<RowId>>>`, 32 B, plus each list's heap buffer.

**Readers today.**
- `HashTrieIterator::leaf_tuples` lends a `LeafRows`: a slice of ids over the buffer, with
  random access `row(i)`.
- `emit_leaf` (Algorithm 3 lines 16–19) runs a mixed-base odometer of indices over the
  chains' lengths.
- The selection view (`EqualitySelectionHashTrieIter`) filters a chain into its own
  `Vec<RowId>`, and lends that slice over the inner chain's buffer.
- The constant view (`SingletonHashTrieIter`) lends a one-id slice over inline data.

## Design

### `ChainLinks` (kermit-iters)

```rust
/// One link per row of a tuple batch: the id of the next row in that row's list, or `NIL`.
pub struct ChainLinks(Vec<AtomicU32>);
impl ChainLinks {
    pub const NIL: RowId = RowId::MAX;
    pub fn new(rows: usize) -> Self;          // every link unset (see "Initial links")
    pub fn len(&self) -> usize;
    pub fn get(&self, row: RowId) -> RowId;    // load(Relaxed)
    pub fn set(&self, row: RowId, next: RowId); // store(Relaxed)
    pub fn push(&mut self, next: RowId);       // for `insert`
    pub fn heap_size_bytes(&self) -> usize;    // capacity × 4
}
```

- **One comment covers the atomics,** on the type itself: they exist only so lazy
  expansion can write through `&self`. No two threads ever write the same link, so they are
  never contended.
- **`NIL` can't be a row.** A batch holds at most `u32::MAX` rows (#111), so the largest id
  is `u32::MAX − 1`.
- **Invariant:** `links.len() == tuples.len()`. `insert` pushes one link per row it
  appends.
- **Initial links.** `new` allocates zeroed (`vec![0u32; rows]`, converted in place to
  atomics) and never fills with `NIL`. In every build from tuples, Algorithm 2 pushes each
  row at the root level before any link is read, so starting values are never observed.

### `Chain`, Algorithm 2's linked list (kermit-ds)

```rust
/// A list of rows threaded through `links`, in input order: Algorithm 2's `L`.
pub(super) struct Chain { head: RowId, tail: RowId, len: NonZeroU32 }
impl Chain {
    fn start(links: &ChainLinks, row: RowId) -> Self;    // links[row] = NIL
    fn push(&mut self, links: &ChainLinks, row: RowId);  // links[tail] = row; links[row] = NIL
    fn walk<'l>(&self, links: &'l ChainLinks) -> impl Iterator<Item = RowId> + 'l;
    fn len(&self) -> usize;
}
```

- **The walk reads a row's link before it yields the row.** Re-threading a chain into the
  next level's lists rewrites each row's link while the walk is still reading. The atomics
  let the walk and the writes share one `&ChainLinks`, but the walk must fetch "next"
  first. The doc states this, and a test pins it.
- **`len`** is needed for three things:
  - Algorithm 2 line 3's `|L|` (`child-capacity=tuples`);
  - the pruning test `len == 1`;
  - `LeafRows::len`.

  As a `NonZeroU32` it gives `Option<Entry<Chain>>` a niche, so a bucket is 24 B (8 for
  the hash, 12 for the chain), down from 32 B plus a heap allocation.
- **Every push writes `NIL`** into the pushed row's link. A chain then always ends in
  `NIL`, and `links` is a function of the lists alone. So `bulk` and `incremental` build
  equal `links` arrays, and the array-level identity tests can compare them whole. This
  costs one store per push; the measurement reports it.

**Where `Chain` replaces `Vec<RowId>`:**

| Today | After B2 |
| --- | --- |
| `TupleList = Vec<RowId>` (`bulk.rs`) | `Chain` (a new `chain.rs` in `hash_trie/`) |
| `Leaf(HashTable<Vec<RowId>>)` | `Leaf(HashTable<Chain>)` |
| `LazyChild { pending: RefCell<Vec<RowId>>, built: OnceCell<N> }` | `pending: Cell<Option<Chain>>`, taken by expansion |
| scratch `HashTable<TupleList>`: `group`, `radix::build_scratch_root`, `parallel::Entries::Chains`, presized's table of lists | `HashTable<Chain>`; `Entries::Chains(Vec<Arrival<Chain>>)` |
| `Singleton(RowId)` | unchanged |
| Algorithm 2's first list (`all_rows`, a range) | unchanged: never threaded |

Removed:
- the per-list heap allocations;
- `tuple_list_heap_bytes`;
- `child`'s lazy `shrink_to` special case, since a chain has no capacity.

### The builds

- **`bulk` (Algorithm 2).**
  - **Lines 3–7:** `group` walks its list (the whole input at the root) and pushes each row
    onto its bucket's chain: `Chain::start` in a fresh bucket, `push` otherwise.
  - **Lines 8–12:** `build_nested` and `child` take each bucket's `Chain`:
    - under pruning, `len == 1` gives `Singleton(head)`;
    - under lazy expansion, it gives `Unexpanded(chain)`;
    - otherwise the next table is grouped from `chain.walk(links)`.
  - **Line 15:** at the last attribute, the chains are the leaf.

  A row's link is rewritten once per level, and ends as its leaf-chain successor.
- **`insert`, and `incremental`.**
  - `insert` appends the row to the buffer and pushes a `NIL` link.
  - `insert_at` then places it:
    - a leaf bucket gets `chain.push`;
    - a fresh bucket gets `Chain::start`, or a `Singleton` or lazy pending chain as today;
    - a lazy pending child gets `push`;
    - an unprune re-threads the evicted row first, then the new row, as today.
- **Lazy `resolve`** takes the pending chain (`Cell::take`) and groups its walk into the
  child's table through `&self`. This is the case Decision 3 exists for.
- **The radix family.**
  - **First, a refactor:** one shared helper for `radix:K`'s and `parallel:N`'s partition,
    scratch-root and merge sequence (deferred from B1's final review). `radix:K` gains
    `parallel:N`'s early return on an empty batch. The chain changes then touch one copy.
  - **`radix:K` and `parallel:N`:** scratch roots are `HashTable<Chain>` over the trie's
    `links`. In `parallel:N` each worker threads only its own partition's rows. The merge
    moves `Chain` values and children unchanged: their ids are global.
  - **`presized:N`:**
    - workers push into their run's chains;
    - the calling thread pushes the deferred tail in `origin` order;
    - each run's worker builds its buckets' children, re-threading every row of those
      lists, including a deferred key's rows that live in another partition.
  - **Concurrency.** Every row is in exactly one list at a time, so each link has exactly
    one writer. Relaxed atomics make that race-free without proving which partition owns
    which links. Scoped-thread spawn and join supply the happens-before edges. No lock and
    no `split_at_mut` are needed.

**Allocation.** A build allocates per table, never per list:
- the tables;
- the buffer (and, in the radix family, B1's copy);
- one `links` array;
- the scratch structures that are already sized up front.

### Readers

**`LeafRows` (kermit-iters) has two internal forms.** `HashTrieIterator::leaf_tuples`
keeps its signature.

```rust
pub struct LeafRows<'a> { data: &'a [usize], arity: usize, ids: Ids<'a> }
enum Ids<'a> {
    Listed(&'a [RowId]),
    Linked { links: &'a ChainLinks, head: RowId, len: usize },
}
```

- **Constructors.** `new` and `from_parts` stay and build `Listed`. A new
  `linked(tuples, links, head, len)` builds `Linked`.
  - A trie's leaf buckets lend `Linked`.
  - A singleton frame, the constant view and the selection view lend `Listed`. The
    selection view's filtered ids are its own `Vec`, and it cannot thread links through
    the trie's shared array.
- **Reads.** `len`, `is_empty`, `iter()` (rows in order), `data()` and `arity()` stay.
  `ids()` changes from a slice to an iterator of ids. All of them walk either form.
- **A cursor replaces random access.** `row(i)` would be O(i) on a chain, so it is removed.
  In its place: `first() -> LeafCursor`, `advance(&mut LeafCursor) -> bool` (false past the
  end) and `row_at(LeafCursor) -> &'a [usize]`. `LeafCursor` is `Copy`: an index for
  `Listed`, the current id for `Linked`.

**`emit_leaf`** (Algorithm 3 lines 16–19, the cross product of the leaf lists):
- `LeafScratch` holds one `LeafCursor` per relation, and `chain_lens` goes.
- A chain's emptiness is `is_empty()`.
- A candidate reads `row_at(cursor[k])` for each relation k.
- The odometer advances `cursor[0]`. On overflow it resets that cursor to `first()` and
  carries into the next one.
- It allocates nothing per result row.

**The other readers.**
- The selection view filters by `chain.ids().zip(chain.iter())`.
- `for_each_tuple` walks each leaf chain.
- `heap_size_bytes` adds `links.heap_size_bytes()` and drops the per-list `Vec`s.
- `scan_tuples`, `collect_tuples` and `project` read the buffer and do not change.
- Navigation (`open`, `next`, `lookup`, `up`, `size`) never reads links.

### Equivalence

Amendment 2 asks for the same contents and capacities in every mode. That still holds,
because `links` is sized from the row count in every mode, so `heap_size_bytes` is equal.

| Pair | Before B2 | After B2 |
| --- | --- | --- |
| `bulk` / `incremental` | array-identical | array-identical, `links` included |
| `radix:K`, `parallel:N` / `bulk` | equivalent, root bucket for bucket (B1) | unchanged |
| `presized:N` / `bulk` | equivalent; the same for every N | unchanged |

## Parity with the paper

| Element (VLDB 2020 §3.2.2, §3.3.1–3.3.2, Fig. 3) | After B1 | After B2 |
| --- | --- | --- |
| A leaf is a list threaded through a chain pointer per tuple | ✗ a `Vec<RowId>` per chain | ✓ a link per tuple, with two departures (kermit's): a 4-byte id in an array beside the tuple rather than an 8-byte pointer inside it (Decision 1); lists in input order, appended rather than pushed (Decision 2) |
| Algorithm 2 moves each tuple to exactly one child list | ✗ copies its id into a new `Vec` | ✓ rewrites its link |
| No allocation per list | ✗ | ✓ |
| A bucket is 16 B (8-byte hash, 8-byte child slot) | ✗ 32 B for a leaf | ✗ 24 B for a leaf (the `Option` and `len`; kermit's) |
| Lazy expansion re-threads a child's tuples on its first probe | ✗ moves a `Vec` | ✓ through relaxed atomics (Decision 3) |

## Placement under the standard

This is a process and representation change inside HashTrie, with no new Layout, Config or
BuildMode axis. The modes, flags and axes do not change.

The change crosses three crates: `kermit-iters` (`ChainLinks`, `LeafRows`), `kermit-ds`
(`hash_trie/`) and `kermit-algos`'s hash family (`emit_leaf`, the selection view's `ids()`).
`LeafRows` is the contract between the structure and its algorithm, so this is one change
with explicit scope, not a sibling edit. TreeTrie, ColumnTrie, `morsel.rs`, the sorted
family and the binary's plumbing are untouched (Priority 6).

## Testing

**Identity, keeping today's guarantees.**
- `assert_same_trie` (array-level) compares tables bucket for bucket and chains as
  `(head, tail, len)`, and requires equal `links` arrays.
- `assert_same_contents`, `assert_equivalent_trie` and the radix family's checks walk each
  chain through its own trie's `links` and buffer. Chain order is checked, not only the
  multiset.
- B1's spare-capacity and insert-after-build tests keep passing under all four
  pruning × expansion Layouts.

**New unit tests.**
- **`ChainLinks` and `LeafRows`:** both forms; `len`, `iter` and `ids`; `first`, `advance`
  and `row_at` on one-row, many-row and empty chains. The doctest is updated.
- **`chain`:**
  - `start` and `push` keep input order;
  - every chain ends in `NIL`;
  - re-threading a chain into the next level's lists visits every row (the walk's
    read-before-yield rule).
- **`emit_leaf`:** a three-relation product with chains of lengths 1, 2 and 3 emits every
  candidate exactly once, including a cursor reset on carry.
- **Lazy:** `resolve` re-threads through `&self`. The eager-versus-lazy probe-trace tests
  and `column_distinct_leaves_a_lazy_trie_as_built` hold unchanged.
- **`Sync` and `Send`:** eager tries stay `Sync` and lazy tries `Send`. The existing pins
  hold, since `AtomicU32` is `Sync`.

**Suites that pass unchanged.**
- The 16 join patterns on all 8 HashTrie aliases × both optimisers, under `AnyOrders`, and
  the build-mode cells.
- `hash_trie_test_suite!`, `parquet_test_suite!` and the `cli_*` tests.
- The LUBM oracle.
- `result_allocation.rs`: zero allocations per result row.

**`tuple_allocation.rs`.** The HashTrie bounds are re-derived from the code. The 110 lists
that grow in each cell stop allocating, so each bound shrinks to the growth that still
varies with the row count. Bounds are tightened, never loosened.

**Mutation checks.** Each must fail a named test:
1. The walk reads a row's link after yielding it, which breaks re-threading.
2. `push` omits `links[row] = NIL`, so `bulk`'s and `incremental`'s `links` differ.
3. The cursor does not reset on carry, which drops candidates.
4. `resolve` groups the pending chain without re-threading it.
5. `presized:N`'s tail is pushed by new id rather than `origin` (B1's pin, re-run).

## Measurement

B1 was not measured before B2, so one run measures both, after B2 lands. It follows B1's
Parts A–C and the A/B recipe on a quiet host.
- **Binaries:** `d746b97` (#111), `a589c64` (B1) and B2's tip, each built by
  `git archive`.
- **Inputs:** the shapes #107 and #101 used (`price`, `friendof`, grouped and shuffled),
  and the #101 locality harness.
- **Cells:** `bulk`, `incremental`, `radix:12`, `parallel:N` and `presized:N`, under eager
  and lazy expansion, with TreeTrie as the control.
- **Metrics:** `insertion`, `space`, `iteration`, `end_to_end`.
- **Replicates:** at least 5, in alternating order.

**What it should show.**
- B2's `insertion` gain on `bulk`, the #107 1.62× case.
- Its `space` drop.
- Any `iteration` cost of the chain walks: a chain step is a dependent load, link then
  row, and the inner relation of a leaf product restarts its walk for each outer row.

**The eager atomics check.** This is a one-off, never landed. A scratch build makes
`ChainLinks` a plain `Vec<u32>` for eager tries only, with lazy disabled, and times it
against B2 on `insertion` and `iteration`. A real cost reopens Decision 3's declined
per-policy link type.

## Out of scope

- The paper's in-row 8-byte layout, or a Layout offering it (Decision 1; reopened only by a
  measurement).
- 16-byte buckets (dropping the `Option` and `len`), and #106's pointer-boxed children.
- A parallel prober. The atomics would serve one, but no prober is written.
- Two-pass radix partitioning (B1, Decision 4).

## Acceptance

- Every HashTrie list is a `Chain`, and no `Vec<RowId>` list remains in `hash_trie/`.
- `LeafRows` walks both forms, and `emit_leaf` uses cursors.
- `bulk` against `incremental` is array-identical, `links` included. The radix family is
  equivalent to `bulk` as B1 left it.
- The workspace gate passes: tests, clippy `-Dwarnings`, fmt, doc, the binary's private
  rustdoc (only the 2 known errors), and kermit-lab against a release binary.
- The scope check shows TreeTrie, ColumnTrie, `morsel.rs`, the sorted family and the
  binary's plumbing unchanged.
- The re-derived `tuple_allocation.rs` bounds pass.

## Documentation

- **`docs/data-structures/hash-trie.md`:**
  - the representation paragraph and parity table (rows above);
  - the Construction table's "the lists themselves" row;
  - the `from_tuples`, `insert` and `leaf_tuples` complexity rows;
  - the bucket size;
  - the lazy one-tuple-shrink paragraph, which goes.
- **`docs/algorithms/hash-triejoin.md`:** `emit_leaf`'s cursors.
- **`docs/data-structures/parallel-build.md`:** workers threading links, and how
  `presized:N` builds children across runs.
- **`CLAUDE.md`:** the Tuples / RowId / LeafRows bullet (`ChainLinks`, the two forms), and
  the `tuple_allocation.rs` mention.
- **`ARCHITECTURE.md`:** Construction.
- **`docs/specs/bench-report-schema.md`:** a "4 (no bump)" row naming HashTrie's
  `insertion`, `space`, `iteration`, `end_to_end` and `copies`.
- **Dated notes** on HashTrie records measured before B2.
- **The #105 board and #101:** drafts shown to the user, and posted only on their word.
