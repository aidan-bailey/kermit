# HashTrie Lazy Child Expansion Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add lazy child expansion to `HashTrie` as a third Layout parameter
`E: ExpansionPolicy` (`--ds-layout-expansion eager|lazy`, axis
`ds_layout_expansion`), with the cold-`iteration` / as-built-`space` bench
methodology. Issue #92; spec
`docs/specs/2026-10-05-hash-trie-lazy-expansion-design.md` (commit 472e369).

**Architecture:**
- **Node state.** `HashTrieNode<P, E>` gains an `Unexpanded(E::Pending<HashTrieNode<P, E>>)` variant. Its payload is `Box<LazyChild<N>>` (a `RefCell` of pending tuples plus a `OnceCell` of the built table) under `LazyExpansion`, and the uninhabited `Never` under `EagerExpansion`, so eager tries compile to today's code.
- **Expansion.** Under lazy, insert stops below the root. `HashTrieIter::open` expands a child one level at a time, through `HashTrie::resolve`.
- **Bench.** `ExecutionFamily::JOIN_MUTATES` makes `bench run` probe only fresh builds of a lazy family, so `space` always sees the relations as built.

**Tech Stack:** Rust nightly (workspace toolchain), `std::cell::{OnceCell, RefCell}`, clap, Criterion 0.8, pytest (kermit-lab).

---

## Read before starting

- **Spec refinements found while planning** (verified with a standalone
  rustc prototype in the session scratchpad):
  1. **Payload over the node type.** The payload is generic over the
     *node type*, `type Pending<N>: PendingChild<N>`, not over `P` as the
     spec sketched (`E::Pending<P>`). The prototype showed why: a public
     trait method returning `&HashTrieNode` trips `private_interfaces` under
     `-Dwarnings` (`HashTrieNode` is `pub(crate)`). Making the node `pub`
     only moves the error onto `HashTable`. Abstracting over `N` keeps both
     traits free of crate-private types, still compiles to the same
     layout, and keeps eager tries `Sync`. The spec's fallback is not
     needed.
  2. **Warm allocation cells.** The spec says `result_allocation.rs`
     "already runs one unmeasured join first, so the measured join is
     warm". That is false for lazy cells: each `htj_allocations(..)` call
     builds *fresh* relations. Lazy cells therefore use a warm helper that
     joins twice over the same relations (Task 5).

  Task 8 records both refinements in the spec.
- **Environment rules** (from the project's working memory):
  - **Builds.** Run builds and tests in the foreground with
    `CARGO_BUILD_JOBS=2`, never with `run_in_background`; the harness kills
    background cargo when free memory is low.
  - **Formatting.** Run `cargo fmt` only as
    `nix develop --command cargo fmt --all`.
  - **Long jobs.** Launch anything longer than 10 minutes detached:
    `setsid nohup bash -c "...; echo EXIT=\$?" > log 2>&1 < /dev/null & disown`.
  - **Miri.** Run it inside `nix develop` with
    `MIRIFLAGS="-Zmiri-disable-isolation"`, after
    `export MIRI_SYSROOT=<scratch>/miri-sysroot`.
  - **Mutation checks.** Commit the fix first, apply the mutant with an
    exact reversible edit, check that it actually applied, then restore
    with the reverse edit, not `git checkout`.
- **Scope (Priorities item 6).** Touch no sibling structure or algorithm:
  `TreeTrie`, `ColumnTrie`, `HashTriejoin` and LFTJ code are unchanged.
- **Commit trailer.** Every commit ends with:

  ```
  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01J6E7x6SrHVdE3i25EsGTst
  ```

## File map

| File | Change |
|---|---|
| `kermit-ds/src/ds/hash_trie/expansion.rs` | **Create.** `ExpansionPolicy`, `EagerExpansion`, `LazyExpansion`, `PendingChild<N>`, `LazyChild<N>`, unit tests |
| `kermit-ds/src/ds/hash_trie/node.rs` | `HashTrieNode<P, E>`, `Unexpanded` variant, accessor panics |
| `kermit-ds/src/ds/hash_trie/implementation.rs` | `HashTrie<H, P, E>`, lazy insert, `expand_level`, `resolve`, walks, heap, axes, tests |
| `kermit-ds/src/ds/hash_trie/hash_trie_iter.rs` | `E` threaded through; `frame_for` resolves; tests |
| `kermit-ds/src/ds/hash_trie/pruning.rs` | Doc line on `Never`'s second use |
| `kermit-ds/src/ds/hash_trie/mod.rs`, `kermit-ds/src/ds/mod.rs`, `kermit-ds/src/lib.rs` | Re-exports |
| `kermit-ds/tests/hash_trie_tests.rs` | Lazy aliases, suites, trace-equivalence module |
| `kermit-ds/tests/parquet_tests.rs` | `E` on `sorted_tuples`, lazy aliases |
| `kermit/src/options.rs` | `ExpansionChoice`, `expansion_of`, `DsFlag::LayoutExpansion`, `LayoutChoices`, `DsChoices`, 8-arm macro, tests |
| `kermit/src/execution.rs` | `Execution::HashHtj.expansion`, `HashTrieFamily<H, P, E>`, `HashHtj<H, P, E>`, `ExecutionFamily::JOIN_MUTATES`, tests |
| `kermit/src/bench/run.rs`, `kermit/src/bench/ds.rs`, `kermit/src/main.rs` | Macro call sites; probe rule and `space` guard in `run_benchmark` |
| `kermit/tests/common/utils.rs` | `JoinEntry` impls gain `E` |
| `kermit/tests/join_tests.rs` | 4 lazy aliases × 2 optimisers, plus 1 config invocation |
| `kermit/tests/result_allocation.rs` | Warm lazy cells |
| `kermit/tests/cli_query_errors.rs` | 10 cells |
| `kermit/tests/cli_hash_trie_layout_expansion.rs` | **Create.** CLI smoke tests and the `space` regression test |
| `python/kermit-lab/kermit_lab/defaults.py`, `tests/test_defaults.py`, `tests/test_frame.py` | Back-fill `"eager"` |
| Docs | `hash-trie.md`, `optimization-standard.md`, `bench-report-schema.md`, `hash-triejoin.md`, `CLAUDE.md`, spec amendment |

---

### Task 1: The expansion Layout in `kermit-ds`

All of `kermit-ds/src` changes in one commit: an unused policy would fail
`clippy -Dwarnings` with dead code, so the policy, node, insert, expansion
and iterator land together. Work test-first within the task.

**Files:**
- Create: `kermit-ds/src/ds/hash_trie/expansion.rs`
- Modify: `kermit-ds/src/ds/hash_trie/{mod.rs,node.rs,implementation.rs,hash_trie_iter.rs,pruning.rs}`, `kermit-ds/src/ds/mod.rs`, `kermit-ds/src/lib.rs`

- [ ] **Step 1: Create the policy file**

`kermit-ds/src/ds/hash_trie/expansion.rs`:

```rust
//! Expansion policy — the third Layout dimension of
//! [`HashTrie`](super::HashTrie).
//!
//! Lazy child expansion (SIGMOD 2020 §3.3.1, Figure 6) builds only the root
//! table at construction. Every child below it keeps its tuples as a list
//! until a probe first opens it, and only then is that child's table built,
//! one level at a time. It is a *shape*: an unexpanded child is a node
//! state, so it is a Layout, not a Config. Under [`EagerExpansion`] the
//! state's payload is the uninhabited [`Never`], the `Unexpanded` variant
//! vanishes from the node's layout, and the instantiation compiles to the
//! eager code. Bench axis: `ds_layout_expansion`.
//!
//! The payload is generic over the node type `N` rather than naming
//! `HashTrieNode`. That keeps these two traits, public but in a private
//! module like `pruning.rs`'s, free of the crate-private node type, which
//! `private_interfaces` would reject.

use {
    super::pruning::Never,
    kermit_iters::LayoutOption,
    std::cell::{OnceCell, Ref, RefCell},
};

/// What a `HashTrieNode::Unexpanded` holds: `Box<LazyChild<N>>` under
/// [`LazyExpansion`], [`Never`] under [`EagerExpansion`].
pub trait PendingChild<N>: Sized {
    /// An unexpanded child holding `tuples`, in insertion order.
    fn from_tuples(tuples: Vec<Vec<usize>>) -> Self;
    /// The built table, once a probe has expanded this child.
    fn built(&self) -> Option<&N>;
    /// [`built`](Self::built) for `insert`, which holds the trie mutably.
    fn built_mut(&mut self) -> Option<&mut N>;
    /// The tuples not yet built into a table; empty once expanded.
    fn pending(&self) -> Ref<'_, Vec<Vec<usize>>>;
    /// Appends `tuple` to an unexpanded child. Callers check
    /// [`built_mut`](Self::built_mut) first.
    fn push(&mut self, tuple: Vec<usize>);
    /// The built table, building it first from the pending tuples if no
    /// probe has yet. The tuples are moved into `build`, never copied, so
    /// an expanded child stores each tuple once.
    fn expand(&self, build: impl FnOnce(Vec<Vec<usize>>) -> N) -> &N;
    /// Heap bytes of the payload itself (the box), excluding the pending
    /// tuples and the built table, which `heap_size_bytes` counts
    /// separately.
    fn own_heap_bytes(&self) -> usize;
}

/// Compile-time expansion policy of a [`HashTrie`](super::HashTrie).
///
/// As with `PruningPolicy`, the associated type and the constant must
/// agree: `LAZY == false` requires an uninhabited `Pending`, so no
/// `Unexpanded` node can exist. [`PendingChild`] lives in this private
/// module, so [`EagerExpansion`] and [`LazyExpansion`] are the only
/// policies and the pairing is checked here, once.
pub trait ExpansionPolicy: LayoutOption + Copy + Default + 'static {
    /// What a `HashTrieNode::Unexpanded` holds under this policy, for a
    /// node type `N`.
    type Pending<N>: PendingChild<N>;
    /// Folded by the compiler: `insert_at`'s lazy branches are
    /// `if E::LAZY { … }`.
    const LAZY: bool;
}

impl<N> PendingChild<N> for Never {
    fn from_tuples(_tuples: Vec<Vec<usize>>) -> Self {
        unreachable!("EagerExpansion never constructs an Unexpanded child (E::LAZY is false)")
    }

    fn built(&self) -> Option<&N> { match *self {} }

    fn built_mut(&mut self) -> Option<&mut N> { match *self {} }

    fn pending(&self) -> Ref<'_, Vec<Vec<usize>>> { match *self {} }

    fn push(&mut self, _tuple: Vec<usize>) { match *self {} }

    fn expand(&self, _build: impl FnOnce(Vec<Vec<usize>>) -> N) -> &N { match *self {} }

    fn own_heap_bytes(&self) -> usize { match *self {} }
}

/// An unexpanded child: its tuples until a probe reaches it, the table built
/// from them afterwards.
///
/// `RefCell` and `OnceCell` make lazy tries `!Sync` (still `Send`). Nothing
/// in the workspace shares a relation across threads; a parallel prober
/// would need a different cell.
pub struct LazyChild<N> {
    /// The tuples below this bucket, in insertion order. Moved into the
    /// build by expansion, leaving an empty, unallocated vector.
    pending: RefCell<Vec<Vec<usize>>>,
    /// The table this level would have held, once a probe has reached it.
    built: OnceCell<N>,
}

impl<N> PendingChild<N> for Box<LazyChild<N>> {
    fn from_tuples(tuples: Vec<Vec<usize>>) -> Self {
        Box::new(LazyChild {
            pending: RefCell::new(tuples),
            built: OnceCell::new(),
        })
    }

    fn built(&self) -> Option<&N> { self.built.get() }

    fn built_mut(&mut self) -> Option<&mut N> { self.built.get_mut() }

    fn pending(&self) -> Ref<'_, Vec<Vec<usize>>> { self.pending.borrow() }

    fn push(&mut self, tuple: Vec<usize>) {
        debug_assert!(self.built.get().is_none(), "push into an expanded child");
        self.pending.get_mut().push(tuple);
    }

    // A visitor that opens an iterator on the same trie while
    // `for_each_tuple` holds `pending()` makes `borrow_mut` panic here
    // (`BorrowMutError`): a loud internal panic, not a wrong answer. No
    // caller in the workspace does that.
    fn expand(&self, build: impl FnOnce(Vec<Vec<usize>>) -> N) -> &N {
        self.built
            .get_or_init(|| build(std::mem::take(&mut *self.pending.borrow_mut())))
    }

    fn own_heap_bytes(&self) -> usize { std::mem::size_of::<LazyChild<N>>() }
}

/// Every level built at construction: today's structure. Bench axis value
/// `"eager"`. The default `E`.
#[derive(Copy, Clone, Default, Debug)]
pub struct EagerExpansion;

impl LayoutOption for EagerExpansion {
    const NAME: &'static str = "eager";
}

impl ExpansionPolicy for EagerExpansion {
    type Pending<N> = Never;

    const LAZY: bool = false;
}

/// Lazy child expansion (paper Figure 6). Bench axis value `"lazy"`.
#[derive(Copy, Clone, Default, Debug)]
pub struct LazyExpansion;

impl LayoutOption for LazyExpansion {
    const NAME: &'static str = "lazy";
}

impl ExpansionPolicy for LazyExpansion {
    type Pending<N> = Box<LazyChild<N>>;

    const LAZY: bool = true;
}

#[cfg(test)]
mod tests {
    use super::*;

    type Lazy = Box<LazyChild<Vec<Vec<usize>>>>;

    #[test]
    fn policy_names_are_the_axis_values() {
        assert_eq!(<EagerExpansion as LayoutOption>::NAME, "eager");
        assert_eq!(<LazyExpansion as LayoutOption>::NAME, "lazy");
    }

    #[test]
    fn lazy_matches_the_marker() {
        const { assert!(!EagerExpansion::LAZY) };
        const { assert!(LazyExpansion::LAZY) };
    }

    #[test]
    fn expand_moves_the_pending_tuples_once() {
        let mut child = Lazy::from_tuples(vec![vec![1, 2]]);
        child.push(vec![3, 4]);
        assert!(child.built().is_none());
        assert_eq!(*child.pending(), vec![vec![1, 2], vec![3, 4]]);
        let mut calls = 0;
        let built = child.expand(|tuples| {
            calls += 1;
            tuples
        });
        assert_eq!(built, &vec![vec![1, 2], vec![3, 4]]);
        // A second expand returns the same table without building again.
        let again = child.expand(|_| unreachable!("already built"));
        assert!(std::ptr::eq(built, again));
        assert_eq!(calls, 1);
        assert!(child.pending().is_empty());
        assert_eq!(child.pending().capacity(), 0);
    }

    #[test]
    fn built_mut_reaches_the_expanded_table() {
        let mut child = Lazy::from_tuples(vec![vec![1]]);
        assert!(child.built_mut().is_none());
        child.expand(|tuples| tuples);
        child.built_mut().unwrap().push(vec![2]);
        assert_eq!(child.built(), Some(&vec![vec![1], vec![2]]));
    }

    #[test]
    fn own_heap_bytes_is_the_box() {
        let child = Lazy::from_tuples(Vec::new());
        assert_eq!(
            child.own_heap_bytes(),
            std::mem::size_of::<LazyChild<Vec<Vec<usize>>>>()
        );
    }
}
```

- [ ] **Step 2: Register the module and re-exports**

In `kermit-ds/src/ds/hash_trie/mod.rs`, add `mod expansion;` (keeping the
module list alphabetical: after `mod config;`), and extend the `pub use`:

```rust
pub use {
    config::{HashTrieConfig, InvalidLoadFactor, LoadFactor},
    expansion::{EagerExpansion, ExpansionPolicy, LazyExpansion},
    implementation::HashTrie,
    pruning::{NoPruning, PruningPolicy, SingletonPruning},
};
```

In `kermit-ds/src/ds/mod.rs`, extend the `hash_trie::{…}` re-export with
`EagerExpansion, ExpansionPolicy, LazyExpansion`. In `kermit-ds/src/lib.rs`,
extend the `ds::{…}` re-export the same way. Keep alphabetical order, so
`fmt` leaves them alone.

In `pruning.rs`, append this paragraph to `Never`'s doc comment:

```rust
/// Also the eager payload of `HashTrieNode::Unexpanded` (see
/// `expansion.rs`), for the same reason.
```

- [ ] **Step 3: Thread `E` through the node**

In `node.rs`:
- Change the `use` to
  `use super::{expansion::ExpansionPolicy, hash_table::HashTable, pruning::PruningPolicy};`.
- Change the enum to
  `pub(crate) enum HashTrieNode<P: PruningPolicy, E: ExpansionPolicy>`, with
  `Inner(HashTable<HashTrieNode<P, E>>)`.
- Add the new variant after `Singleton`:

```rust
    /// Unexpanded child (lazy child expansion, SIGMOD 2020 Figure 6): the
    /// tuples below this bucket, kept as a list until a probe first opens
    /// it, then the table built from them. Never the root. Holds
    /// `E::Pending<Self>`: `Box<LazyChild<Self>>` when lazy, the uninhabited
    /// `Never` when eager, so under `EagerExpansion` this variant costs
    /// nothing (pinned by `node_does_not_grow_under_the_expansion_policy`).
    /// `HashTrie::resolve` is the one place that expands it.
    Unexpanded(E::Pending<HashTrieNode<P, E>>),
```

Then:
- Change `impl<P: PruningPolicy> HashTrieNode<P>` to
  `impl<P: PruningPolicy, E: ExpansionPolicy> HashTrieNode<P, E>`.
- Add a second cold helper beside `singleton_is_not_a_table`:

```rust
    /// As [`singleton_is_not_a_table`](Self::singleton_is_not_a_table):
    /// `HashTrieIter` resolves an `Unexpanded` child before framing it.
    #[cold]
    #[inline(never)]
    fn unexpanded_is_not_a_table() -> ! {
        panic!("HashTrieNode table accessor called on an Unexpanded child (resolve it first)")
    }
```

- In each of the five forwarding accessors, add the arm
  `| HashTrieNode::Unexpanded(_) => Self::unexpanded_is_not_a_table(),`
  after the `Singleton` arm.
- Update the module doc's second paragraph to say that, besides
  `Singleton`, an `Unexpanded` child may stand in for a table at any depth
  `1..arity` when expansion is lazy.

- [ ] **Step 4: Write the failing size-pin and Sync tests**

In `implementation.rs`'s `mod tests`, next to
`node_does_not_grow_under_the_pruning_policy`, add:

```rust
    /// The eager node is laid out as if `Unexpanded` did not exist: the
    /// mirrors below are the node without that variant, for each pruning
    /// policy. `HashTable<V>`'s size does not depend on `V`. A lazy node is
    /// no larger either, because its payload is one boxed pointer. The
    /// frame counterpart is `off_frame_is_the_bare_table_pair` in
    /// `hash_trie_iter.rs`.
    #[test]
    fn node_does_not_grow_under_the_expansion_policy() {
        use {
            super::super::{
                expansion::{EagerExpansion, LazyExpansion},
                hash_table::HashTable,
            },
            std::mem::size_of,
        };
        #[allow(dead_code)]
        enum Mirror {
            Inner(HashTable<()>),
            Leaf(HashTable<Vec<Vec<usize>>>),
        }
        #[allow(dead_code)]
        enum PrunedMirror {
            Inner(HashTable<()>),
            Leaf(HashTable<Vec<Vec<usize>>>),
            Singleton(Vec<usize>),
        }
        assert_eq!(
            size_of::<HashTrieNode<NoPruning, EagerExpansion>>(),
            size_of::<Mirror>()
        );
        assert_eq!(
            size_of::<HashTrieNode<SingletonPruning, EagerExpansion>>(),
            size_of::<PrunedMirror>()
        );
        assert_eq!(
            size_of::<HashTrieNode<NoPruning, LazyExpansion>>(),
            size_of::<HashTrieNode<NoPruning, EagerExpansion>>()
        );
        assert_eq!(
            size_of::<HashTrieNode<SingletonPruning, LazyExpansion>>(),
            size_of::<HashTrieNode<SingletonPruning, EagerExpansion>>()
        );
    }

    /// Eager tries stay `Sync`: their `Unexpanded` payload is `Never` as a
    /// whole type, so no cell type appears in them. A compile-time pin.
    #[test]
    fn eager_tries_stay_sync() {
        fn sync<T: Sync>() {}
        sync::<HashTrie>();
        sync::<HashTrie<SipHashStrategy, SingletonPruning>>();
    }
```

Also update the existing `node_does_not_grow_under_the_pruning_policy` to
name `HashTrieNode<NoPruning, EagerExpansion>` and
`HashTrieNode<SingletonPruning, EagerExpansion>`, since the node no longer
has a default for `E`.

- [ ] **Step 5: Thread `E` through `HashTrie`**

In `implementation.rs`:
- Import `expansion::{EagerExpansion, ExpansionPolicy, PendingChild}`
  beside the `pruning` import.
- The struct becomes:

```rust
pub struct HashTrie<
    H: HashStrategy = SipHashStrategy,
    P: PruningPolicy = NoPruning,
    E: ExpansionPolicy = EagerExpansion,
> {
    header: RelationHeader,
    root: HashTrieNode<P, E>,
    tuple_count: usize,
    config: HashTrieConfig,
    _layout: PhantomData<(H, P, E)>,
}
```

  Keep the existing field doc comments.
- Every `impl<H: HashStrategy, P: PruningPolicy> … for HashTrie<H, P>`
  becomes `impl<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy> … for HashTrie<H, P, E>`,
  and every `HashTrieNode<P>` becomes `HashTrieNode<P, E>`. That covers
  `node_heap_bytes`, which becomes
  `fn node_heap_bytes<P: PruningPolicy, E: ExpansionPolicy>(node: &HashTrieNode<P, E>) -> usize`.
- `project` builds `HashTrie::<H, P, E>::from_tuples_with_config(…)`.
- `hash_trie_iter` returns
  `super::hash_trie_iter::HashTrieIter::<H, P, E>::new(self)`.
- In the struct's docs:
  - extend the "Layout parameters" section with a paragraph on `E`, in the
    style of the `P` paragraph: `LazyExpansion` keeps every child below the
    root as its tuples until a probe first opens it, while `EagerExpansion`
    (the default) makes `Unexpanded` uninhabited; bench axis
    `ds_layout_expansion`;
  - add an invariant: with `LazyExpansion`, every `Inner` bucket holds a
    `Singleton` (pruning on, exactly one tuple) or an `Unexpanded` child,
    never a table, and an expanded child's table matches the eager table
    at that position (see `expand_level`);
  - make the module doc say "three Layout dimensions" and add
    `ds_layout_expansion` to its axis list.

- [ ] **Step 6: Write the failing lazy unit tests**

Append this module to `implementation.rs`, after the pruning tests and
inside the file's `#[cfg(test)]` area (as a sibling `mod lazy_tests`):

```rust
#[cfg(test)]
mod lazy_tests {
    use {
        super::{
            super::expansion::{LazyChild, LazyExpansion, PendingChild},
            *,
        },
        crate::{cardinality::Cardinality, heap_size::HeapSize, relation::Projectable},
        kermit_iters::HasOptimizationAxes,
    };

    type Lazy = HashTrie<SipHashStrategy, NoPruning, LazyExpansion>;
    type LazyPruned = HashTrie<SipHashStrategy, super::super::pruning::SingletonPruning, LazyExpansion>;

    fn h(k: usize) -> u64 { <SipHashStrategy as HashStrategy>::hash(k) }

    /// The root's child for first attribute `k`.
    fn child<P: PruningPolicy>(
        trie: &HashTrie<SipHashStrategy, P, LazyExpansion>, k: usize,
    ) -> &HashTrieNode<P, LazyExpansion> {
        match &trie.root {
            | HashTrieNode::Inner(t) => t.get(h(k)).expect("bucket present"),
            | _ => panic!("arity >= 2 has an Inner root"),
        }
    }

    /// The pending tuples of an unexpanded child, or `None` for any other
    /// node or an expanded child.
    fn pending_of<P: PruningPolicy>(node: &HashTrieNode<P, LazyExpansion>) -> Option<Vec<Vec<usize>>> {
        match node {
            | HashTrieNode::Unexpanded(p) if p.built().is_none() => Some(p.pending().clone()),
            | _ => None,
        }
    }

    const TUPLES: [[usize; 3]; 4] = [[1, 2, 3], [1, 2, 4], [1, 5, 6], [7, 8, 9]];

    fn tuples() -> Vec<Vec<usize>> { TUPLES.iter().map(|t| t.to_vec()).collect() }

    #[test]
    fn lazy_build_leaves_every_root_child_unexpanded() {
        let trie = Lazy::from_tuples(3.into(), tuples());
        assert_eq!(
            pending_of(child(&trie, 1)),
            Some(vec![vec![1, 2, 3], vec![1, 2, 4], vec![1, 5, 6]])
        );
        assert_eq!(pending_of(child(&trie, 7)), Some(vec![vec![7, 8, 9]]));
    }

    #[test]
    fn lazy_pruned_build_keeps_one_tuple_children_as_singletons() {
        let trie = LazyPruned::from_tuples(3.into(), tuples());
        assert_eq!(
            pending_of(child(&trie, 1)),
            Some(vec![vec![1, 2, 3], vec![1, 2, 4], vec![1, 5, 6]])
        );
        assert!(matches!(child(&trie, 7), HashTrieNode::Singleton(_)));
    }

    #[test]
    fn a_second_tuple_turns_a_singleton_into_an_unexpanded_pair_in_arrival_order() {
        let mut trie = LazyPruned::from_tuples(2.into(), vec![vec![1, 2]]);
        trie.insert(vec![1, 3]);
        assert_eq!(pending_of(child(&trie, 1)), Some(vec![vec![1, 2], vec![1, 3]]));
    }

    #[test]
    fn resolve_expands_exactly_one_level() {
        let trie = Lazy::from_tuples(3.into(), tuples());
        let node = child(&trie, 1);
        let built = trie.resolve(node, 1);
        let HashTrieNode::Inner(level) = built else {
            panic!("depth 1 of an arity-3 trie is an Inner table")
        };
        assert_eq!(level.len(), 2); // second attributes 2 and 5
        for (_, grandchild) in level.iter() {
            assert!(pending_of(grandchild).is_some(), "grandchildren stay unexpanded");
        }
        assert!(pending_of(node).is_none(), "the child is expanded now");
        assert!(std::ptr::eq(trie.resolve(node, 1), built), "expansion happens once");
        assert!(pending_of(child(&trie, 7)).is_some(), "siblings are untouched");
    }

    #[test]
    fn resolve_returns_tables_and_singletons_unchanged() {
        let trie = LazyPruned::from_tuples(3.into(), tuples());
        let single = child(&trie, 7);
        assert!(std::ptr::eq(trie.resolve(single, 1), single));
        assert!(std::ptr::eq(trie.resolve(&trie.root, 0), &trie.root));
    }

    #[test]
    fn insert_reaches_the_built_table_after_expansion_and_the_list_before() {
        let mut trie = Lazy::from_tuples(2.into(), vec![vec![1, 2], vec![1, 3], vec![9, 9]]);
        trie.resolve(child(&trie, 1), 1);
        trie.insert(vec![1, 4]); // into the expanded child's table
        trie.insert(vec![9, 8]); // onto the unexpanded sibling's list
        let HashTrieNode::Unexpanded(p) = child(&trie, 1) else { panic!() };
        assert_eq!(p.built().expect("expanded").len(), 3);
        assert_eq!(pending_of(child(&trie, 9)), Some(vec![vec![9, 9], vec![9, 8]]));
        assert_eq!(trie.tuple_count(), 5);
        let mut all = trie.collect_tuples();
        all.sort();
        assert_eq!(all, vec![vec![1, 2], vec![1, 3], vec![1, 4], vec![9, 8], vec![9, 9]]);
    }

    #[test]
    fn walks_read_pending_tuples_without_expanding() {
        let trie = Lazy::from_tuples(3.into(), tuples());
        let before = trie.heap_size_bytes();
        let mut collected = trie.collect_tuples();
        collected.sort();
        assert_eq!(collected, tuples());
        let mut visited = 0;
        trie.for_each_tuple(|_| visited += 1);
        assert_eq!(visited, 4);
        assert!(pending_of(child(&trie, 1)).is_some());
        assert_eq!(trie.heap_size_bytes(), before, "a walk changes nothing");
    }

    /// Fully expanded, a lazy arity-2 trie is the eager trie plus one
    /// `LazyChild` box per root child. The tables are identical, the
    /// tuples are moved rather than copied, and the emptied pending lists
    /// hold no heap.
    #[test]
    fn expanded_heap_is_the_eager_heap_plus_one_box_per_child() {
        let tuples = vec![vec![1, 2], vec![1, 3], vec![4, 5]];
        let eager: HashTrie = HashTrie::from_tuples(2.into(), tuples.clone());
        let lazy = Lazy::from_tuples(2.into(), tuples);
        for k in [1, 4] {
            lazy.resolve(child(&lazy, k), 1);
        }
        let boxes = 2 * std::mem::size_of::<LazyChild<HashTrieNode<NoPruning, LazyExpansion>>>();
        assert_eq!(lazy.heap_size_bytes(), eager.heap_size_bytes() + boxes);
    }

    #[test]
    fn projection_of_a_lazy_trie_holds_the_projected_tuples() {
        let trie = Lazy::from_tuples(3.into(), tuples());
        let mut projected = trie.project(vec![2, 0]).collect_tuples();
        projected.sort();
        assert_eq!(projected, vec![vec![3, 1], vec![4, 1], vec![6, 1], vec![9, 7]]);
    }

    #[test]
    fn optimization_axes_include_the_expansion_layout() {
        let eager: HashTrie = HashTrie::new(2.into());
        assert_eq!(
            eager.optimization_axes().get("ds_layout_expansion"),
            Some(&serde_json::Value::String("eager".into()))
        );
        assert_eq!(
            Lazy::new(2.into()).optimization_axes().get("ds_layout_expansion"),
            Some(&serde_json::Value::String("lazy".into()))
        );
    }
}
```

Also update the two `optimization_axes_*` key-list tests
(`optimization_axes_default_strategy_reports_sip`,
`optimization_axes_fxhash_strategy_reports_fxhash`). Their sorted key list
becomes:

```rust
        assert_eq!(keys, vec![
            "ds_config_load_factor",
            "ds_layout_expansion",
            "ds_layout_hasher",
            "ds_layout_pruning"
        ]);
```

If `HeapSize` / `Cardinality` / `Projectable` are already imported by the
parent module's `use`, drop the duplicate from this `use` (clippy flags
unused imports).

- [ ] **Step 7: Run the tests to see them fail**

Run: `CARGO_BUILD_JOBS=2 cargo test -p kermit-ds --lib hash_trie 2>&1 | tail -20`

Expected: a compile failure. `resolve` does not exist yet, the `Inner` arm
does not handle laziness, and `HashTrieIter` lacks `E`.

- [ ] **Step 8: Implement lazy insert, expansion and `resolve`**

In `insert_at`, keep the existing pruning block as is. Directly after it,
add the lazy fresh-bucket block:

```rust
                if E::LAZY && table.get(hash).is_none() {
                    // Fresh bucket, pruning off: defer the child's table
                    // (Figure 6). The tuple waits in the child's list until a
                    // probe opens it; `resolve` then builds the table. With
                    // pruning on, the block above stored a `Singleton` first.
                    table.entry_or_insert_with(hash, load_factor, || {
                        HashTrieNode::Unexpanded(E::Pending::from_tuples(vec![tuple]))
                    });
                    return;
                }
```

After `let child = table.entry_or_insert_with(…new_table…)`, and before the
existing `if P::ENABLED && matches!(child, HashTrieNode::Singleton(_))`
unprune block, add:

```rust
                if E::LAZY {
                    // The bucket existed (absence returned above), so the
                    // closure did not run: under lazy expansion an `Inner`
                    // bucket holds a `Singleton` or an `Unexpanded` child,
                    // never a table.
                    match child {
                        | HashTrieNode::Unexpanded(pending) => match pending.built_mut() {
                            | Some(built) => {
                                Self::insert_at(built, depth + 1, arity, tuple, load_factor)
                            },
                            | None => pending.push(tuple),
                        },
                        | HashTrieNode::Singleton(_) => {
                            // A second tuple below a pruned bucket: the child
                            // becomes the unexpanded list of both. The evicted
                            // tuple goes first, as an eager unprune re-inserts it
                            // first, so expansion later builds the eager table.
                            let list = HashTrieNode::Unexpanded(E::Pending::from_tuples(
                                Vec::with_capacity(2),
                            ));
                            let HashTrieNode::Singleton(evicted) = std::mem::replace(child, list)
                            else {
                                unreachable!("matched Singleton above")
                            };
                            let HashTrieNode::Unexpanded(pending) = child else {
                                unreachable!("replaced by an Unexpanded child just above")
                            };
                            pending.push(evicted.into_tuple());
                            pending.push(tuple);
                        },
                        | HashTrieNode::Inner(_) | HashTrieNode::Leaf(_) => unreachable!(
                            "a lazy Inner bucket holds a Singleton or an Unexpanded child"
                        ),
                    }
                    return;
                }
```

Change the final `| HashTrieNode::Singleton(_) => unreachable!(…)` arm of
`insert_at`'s outer match to
`| HashTrieNode::Singleton(_) | HashTrieNode::Unexpanded(_) => unreachable!(…)`,
and extend its message: "…; singletons are unpruned and unexpanded children
appended to by the parent".

Add these two functions to the inherent `impl` block, after `insert_at`:

```rust
    /// Builds the table a lazy child at `depth` would have held, from its
    /// pending `tuples`, through the same `insert_at` construction uses.
    /// Two consequences:
    ///
    /// - The child's own children come out `Unexpanded` (or `Singleton`),
    ///   so each expansion builds exactly one level (Figure 6).
    /// - The tuples arrive in insertion order, the order eager construction
    ///   inserted them into this child, and linear probing places keys by
    ///   insertion order under the same load factor. So the table is the
    ///   eager table at this position, bucket for bucket, which the
    ///   trace-equivalence tests in `kermit-ds/tests/hash_trie_tests.rs`
    ///   pin.
    fn expand_level(
        tuples: Vec<Vec<usize>>, depth: usize, arity: usize, load_factor: LoadFactor,
    ) -> HashTrieNode<P, E> {
        let mut node = HashTrieNode::new_table(Self::is_leaf_depth(depth, arity));
        for tuple in tuples {
            Self::insert_at(&mut node, depth, arity, tuple, load_factor);
        }
        node
    }

    /// `node` as a probe sees it. An `Unexpanded` child is first built
    /// (once; later calls return the same table), and any other node is
    /// returned as is. `depth` is `node`'s depth.
    ///
    /// `HashTrieIter::open` is the only caller, so only a probe expands
    /// anything. Every read-only walk (`collect_tuples`, `for_each_tuple`,
    /// `heap_size_bytes`) reads the pending list instead.
    pub(crate) fn resolve<'t>(
        &'t self, node: &'t HashTrieNode<P, E>, depth: usize,
    ) -> &'t HashTrieNode<P, E> {
        match node {
            | HashTrieNode::Unexpanded(pending) => pending.expand(|tuples| {
                Self::expand_level(tuples, depth, self.header.arity(), self.config.load_factor)
            }),
            | other => other,
        }
    }
```

- [ ] **Step 9: Walks, heap and axes**

In `visit_at`, add the arm:

```rust
            | HashTrieNode::Unexpanded(pending) => match pending.built() {
                | Some(built) => Self::visit_at(built, visit),
                | None => {
                    for tuple in pending.pending().iter() {
                        visit(tuple);
                    }
                },
            },
```

In `collect_at`, add the arm:

```rust
            | HashTrieNode::Unexpanded(pending) => match pending.built() {
                | Some(built) => Self::collect_at(built, out),
                | None => out.extend(pending.pending().iter().cloned()),
            },
```

Add one sentence to `for_each_tuple`'s doc: "Under `LazyExpansion` an
unexpanded child lends its pending tuples in insertion order, and the walk
expands nothing. A `visit` that opens an iterator on this same trie and
reaches such a child panics (`BorrowMutError`)."

In `node_heap_bytes`, factor the leaf chain sum into a helper that the
`Leaf` arm and the new arm share:

```rust
/// Heap bytes of a list of tuples: the list's buffer plus each tuple's.
fn tuple_list_heap_bytes(list: &Vec<Vec<usize>>) -> usize {
    list.capacity() * std::mem::size_of::<Vec<usize>>()
        + list
            .iter()
            .map(|t| t.capacity() * std::mem::size_of::<usize>())
            .sum::<usize>()
}
```

The `Leaf` arm becomes
`shell + table.iter().map(|(_, chain)| tuple_list_heap_bytes(chain)).sum::<usize>()`.
Add the new arm:

```rust
        | HashTrieNode::Unexpanded(pending) => {
            let below = match pending.built() {
                | Some(built) => node_heap_bytes(built),
                | None => tuple_list_heap_bytes(&pending.pending()),
            };
            pending.own_heap_bytes() + below
        },
```

(`#[allow(clippy::ptr_arg)]` on `tuple_list_heap_bytes` if clippy asks:
the helper reads `capacity()`, which a slice does not have.)

In `optimization_axes`, add after the pruning entry:

```rust
        axes.insert(
            "ds_layout_expansion".to_string(),
            serde_json::Value::String(<E as LayoutOption>::NAME.to_string()),
        );
```

and add `ds_layout_expansion` (`"eager"` / `"lazy"`) to its doc comment.

- [ ] **Step 10: Thread `E` through the iterator and resolve on open**

In `hash_trie_iter.rs`:
- Import `expansion::{EagerExpansion, ExpansionPolicy}`.
- `enum Frame<'a, P: PruningPolicy, E: ExpansionPolicy>` with
  `node: &'a HashTrieNode<P, E>`; likewise `Descent<'a, P, E>`, and
  `impl<P: PruningPolicy, E: ExpansionPolicy> Frame<'_, P, E>`.
- `pub struct HashTrieIter<'a, H: HashStrategy = SipHashStrategy, P: PruningPolicy = NoPruning, E: ExpansionPolicy = EagerExpansion>`
  with `stack: Vec<Frame<'a, P, E>>` and `trie: &'a HashTrie<H, P, E>`.
  All impls gain `E`.
- `frame_for` becomes a method that resolves first:

```rust
    /// The frame that opening `child` at `depth` produces, positioned on
    /// its first entry (or past-end if it has none). An unexpanded child
    /// is built here, the one expansion site (`HashTrie::resolve`); the
    /// built node is always a table, so a frame never holds `Unexpanded`.
    fn frame_for(&self, child: &'a HashTrieNode<P, E>, depth: usize) -> Frame<'a, P, E> {
        let trie: &'a HashTrie<H, P, E> = self.trie;
        match trie.resolve(child, depth) {
            | HashTrieNode::Singleton(payload) => Self::singleton_frame(payload.tuple(), depth),
            | table => Frame::Table {
                node: table,
                idx: table.next_occupied(0),
            },
        }
    }
```

- In `open`: `| Descent::Node(child) => self.frame_for(child, depth),`.
- In `descent` and `leaf_tuples`, the `HashTrieNode::Singleton(_) => unreachable!(…)`
  arms on a `Table` frame become
  `HashTrieNode::Singleton(_) | HashTrieNode::Unexpanded(_) => unreachable!(…)`.
  Extend the message with "; open resolves unexpanded children first".
- Add to the module docs' position-model paragraph: "Under
  `LazyExpansion`, `open` first resolves an unexpanded child into its
  table (`HashTrie::resolve`), so frames, keys, lookups and leaf chains
  are those of the eager trie."
- Update the `off_frame_is_the_bare_table_pair` test:

```rust
        assert_eq!(
            std::mem::size_of::<Frame<'static, NoPruning, EagerExpansion>>(),
            std::mem::size_of::<(&'static HashTrieNode<NoPruning, EagerExpansion>, usize)>()
        );
```

- [ ] **Step 11: Add the iterator expansion tests**

Append to `hash_trie_iter.rs`'s `mod tests`:

```rust
    // ── Lazy expansion ─────────────────────────────────────────────────

    use crate::ds::hash_trie::expansion::{LazyExpansion, PendingChild};

    type Lazy = HashTrie<SipHashStrategy, NoPruning, LazyExpansion>;

    /// How many of the root's children a probe has expanded.
    fn expanded_root_children(trie: &Lazy) -> usize {
        match trie.root() {
            | HashTrieNode::Inner(t) => t
                .iter()
                .filter(|(_, c)| matches!(c, HashTrieNode::Unexpanded(p) if p.built().is_some()))
                .count(),
            | _ => panic!("arity >= 2 has an Inner root"),
        }
    }

    #[test]
    fn open_expands_only_the_child_it_enters() {
        let trie = Lazy::from_tuples(2.into(), vec![vec![1, 2], vec![1, 3], vec![4, 5]]);
        let mut it = HashTrieIter::new(&trie);
        assert!(it.open()); // root: always built
        assert_eq!(expanded_root_children(&trie), 0);
        assert!(it.open()); // first child: expanded now
        assert_eq!(expanded_root_children(&trie), 1);
        assert!(it.leaf_tuples().is_some());
        assert!(it.up());
        it.next();
        assert!(it.open()); // its sibling, after `up` and `next`
        assert_eq!(expanded_root_children(&trie), 2);
        assert!(it.up());
        assert!(it.up());
        assert!(it.open());
        assert!(it.open()); // re-entering an expanded child builds nothing new
        assert_eq!(expanded_root_children(&trie), 2);
    }

    #[test]
    fn a_failed_lookup_expands_nothing() {
        let trie = Lazy::from_tuples(2.into(), vec![vec![1, 2], vec![1, 3]]);
        let mut it = HashTrieIter::new(&trie);
        assert!(it.open());
        assert!(!it.lookup(h(99)));
        assert!(!it.open());
        assert_eq!(expanded_root_children(&trie), 0);
    }
```

- [ ] **Step 12: Run the kermit-ds tests to see them pass**

Run: `CARGO_BUILD_JOBS=2 cargo test -p kermit-ds 2>&1 | grep -E "^test result|FAILED|panicked" | head -20`

Expected: every `test result: ok`. The DS integration suites in
`kermit-ds/tests/` compile unchanged, because `E` defaults to eager.
`parquet_tests.rs`'s `sorted_tuples<H, P>` still compiles because
`HashTrie<H, P>` means `HashTrie<H, P, EagerExpansion>`.

If `expanded_heap_is_the_eager_heap_plus_one_box_per_child` fails, print
both numbers and the box size before changing anything. The equality rests
on equal node sizes (pinned in Step 4) and on chains growing through the
same `push` sequence. A difference names a real asymmetry in
`expand_level`, not a test to loosen.

- [ ] **Step 13: Lint, format, docs, and the downstream build**

Run each in turn:
- `nix develop --command cargo fmt --all`
- `CARGO_BUILD_JOBS=2 RUSTFLAGS=-Dwarnings cargo clippy -p kermit-ds --all-targets 2>&1 | tail -5`
- `RUSTDOCFLAGS=-Dwarnings cargo doc -p kermit-ds --no-deps 2>&1 | tail -3`
- `CARGO_BUILD_JOBS=2 cargo build --workspace --all-targets 2>&1 | tail -3`

Expected: no warnings; the workspace builds. The `kermit` crate is
unaffected so far, because every `HashTrie<H, P>` there means eager.

- [ ] **Step 14: Commit**

```bash
git add kermit-ds/src
git commit -m "feat(kermit-ds): lazy child expansion Layout for HashTrie (#92)

HashTrie<H, P, E> gains E: ExpansionPolicy. Under LazyExpansion every
child below the root keeps its tuples until HashTrieIter::open first
reaches it, then builds one level (SIGMOD 2020 Fig. 6) through the same
insert_at, so the table matches the eager one. EagerExpansion's payload
is the uninhabited Never: node and frame sizes are pinned unchanged and
eager tries stay Sync. Read-only walks and heap accounting never expand.

<trailer lines>"
```

---

### Task 2: DS-layer suites and trace equivalence

**Files:**
- Modify: `kermit-ds/tests/hash_trie_tests.rs`, `kermit-ds/tests/parquet_tests.rs`

- [ ] **Step 1: Add the lazy aliases and suites**

In `hash_trie_tests.rs`, extend the `kermit_ds` import with
`ConfigurableRelation, LazyExpansion, NoPruning, PruningPolicy`, and the
`kermit_iters` import with `HashTrieIterable, HashTrieIterator`. (The
collision module imports these itself; remove any duplicate there if clippy
flags it.) After the Config variant block, add:

```rust
// ── Layout variant: lazy child expansion ────────────────────────────────
//
// Each hasher × pruning alias again, with every child below the root built
// on the first `open` that reaches it. The traversal and lookup suites
// therefore expand nodes mid-iteration on every descent.
type HashTrieSipLazy = HashTrie<SipHashStrategy, NoPruning, LazyExpansion>;
type HashTrieFxLazy = HashTrie<FxHashStrategy, NoPruning, LazyExpansion>;
type HashTrieMod10Lazy = HashTrie<Mod10HashStrategy, NoPruning, LazyExpansion>;
type HashTrieSipPrunedLazy = HashTrie<SipHashStrategy, SingletonPruning, LazyExpansion>;
type HashTrieFxPrunedLazy = HashTrie<FxHashStrategy, SingletonPruning, LazyExpansion>;
type HashTrieMod10PrunedLazy = HashTrie<Mod10HashStrategy, SingletonPruning, LazyExpansion>;
// Expansion reads the load factor too: a lazy child is built under the cap
// the trie was configured with.
type HashTrieSipDenseLazy = Configured<HashTrieSipLazy, NinetyPercent>;

hash_trie_test_suite!(HashTrieSipLazy, SipHashStrategy);

hash_trie_test_suite!(HashTrieFxLazy, FxHashStrategy);

hash_trie_test_suite!(HashTrieMod10Lazy, Mod10HashStrategy);

hash_trie_test_suite!(HashTrieSipPrunedLazy, SipHashStrategy);

hash_trie_test_suite!(HashTrieFxPrunedLazy, FxHashStrategy);

hash_trie_test_suite!(HashTrieMod10PrunedLazy, Mod10HashStrategy);

hash_trie_test_suite!(HashTrieSipDenseLazy, SipHashStrategy);
```

- [ ] **Step 2: Add the trace-equivalence module**

Append to `hash_trie_tests.rs`:

```rust
/// A lazy trie, probed, must be indistinguishable from the eager trie built
/// from the same tuples: the same keys, sizes, `at_end` and leaf chains, in
/// the same order, after every operation. Expansion re-inserts a child's
/// tuples in insertion order under the same load factor, so the expanded
/// table *is* the eager one, and this is exact equality, not a multiset
/// comparison. This is what makes an eager-vs-lazy timing a one-variable
/// comparison.
mod lazy_expansion {
    use super::*;

    /// What a caller can observe after one operation.
    #[derive(Debug, PartialEq)]
    struct Obs {
        returned: Ret,
        key: Option<u64>,
        size: usize,
        at_end: bool,
        leaf: Option<Vec<Vec<usize>>>,
    }

    #[derive(Debug, PartialEq)]
    enum Ret {
        Bool(bool),
        Key(Option<u64>),
    }

    fn lcg(state: &mut u64) -> u64 {
        *state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        *state >> 33
    }

    /// One pseudo-random operation, chosen from `state`, then the
    /// observation. The choice depends only on `state` and on observations
    /// equal so far, so two equal iterators take the same path.
    fn step<H: HashStrategy>(it: &mut impl HashTrieIterator, state: &mut u64) -> Obs {
        let returned = match lcg(state) % 4 {
            | 0 => Ret::Bool(it.open()),
            | 1 if !it.at_end() => Ret::Key(it.next()),
            | 2 => Ret::Bool(it.lookup(H::hash((lcg(state) % 10) as usize))),
            | _ => Ret::Bool(it.up()),
        };
        Obs {
            returned,
            key: it.key(),
            size: it.size(),
            at_end: it.at_end(),
            leaf: it.leaf_tuples().map(<[Vec<usize>]>::to_vec),
        }
    }

    /// `n` tuples of `arity` attributes over `0..domain`: small enough to
    /// repeat prefixes and whole tuples (shared children, leaf chains,
    /// duplicates).
    fn random_tuples(seed: u64, n: usize, arity: usize, domain: u64) -> Vec<Vec<usize>> {
        let mut state = seed;
        (0..n)
            .map(|_| (0..arity).map(|_| (lcg(&mut state) % domain) as usize).collect())
            .collect()
    }

    fn assert_lazy_walks_like_eager<H: HashStrategy, P: PruningPolicy>(load_factor: u8) {
        let config = HashTrieConfig {
            load_factor: LoadFactor::percent(load_factor).unwrap(),
        };
        for arity in [2, 3] {
            for seed in 1..=20u64 {
                let tuples = random_tuples(seed, 80, arity, 6);
                let eager =
                    HashTrie::<H, P>::from_tuples_with_config(arity.into(), config, tuples.clone());
                let lazy = HashTrie::<H, P, LazyExpansion>::from_tuples_with_config(
                    arity.into(),
                    config,
                    tuples,
                );
                let (mut e, mut l) = (eager.hash_trie_iter(), lazy.hash_trie_iter());
                let (mut se, mut sl) = (seed, seed);
                for i in 0..400 {
                    assert_eq!(
                        step::<H>(&mut e, &mut se),
                        step::<H>(&mut l, &mut sl),
                        "arity {arity}, seed {seed}, step {i}"
                    );
                }
            }
        }
    }

    #[test]
    fn sip_lazy_walks_like_eager() { assert_lazy_walks_like_eager::<SipHashStrategy, NoPruning>(70); }

    #[test]
    fn fx_lazy_walks_like_eager() { assert_lazy_walks_like_eager::<FxHashStrategy, NoPruning>(70); }

    #[test]
    fn mod10_lazy_walks_like_eager() {
        assert_lazy_walks_like_eager::<Mod10HashStrategy, NoPruning>(70);
    }

    #[test]
    fn sip_pruned_lazy_walks_like_eager() {
        assert_lazy_walks_like_eager::<SipHashStrategy, SingletonPruning>(70);
    }

    #[test]
    fn fx_pruned_lazy_walks_like_eager() {
        assert_lazy_walks_like_eager::<FxHashStrategy, SingletonPruning>(70);
    }

    #[test]
    fn mod10_pruned_lazy_walks_like_eager() {
        assert_lazy_walks_like_eager::<Mod10HashStrategy, SingletonPruning>(70);
    }

    /// Expansion reads the configured cap: a lazy child built under 50 % or
    /// 90 % must match the eager table built under the same cap.
    #[test]
    fn lazy_walks_like_eager_under_other_load_factors() {
        assert_lazy_walks_like_eager::<SipHashStrategy, NoPruning>(50);
        assert_lazy_walks_like_eager::<Mod10HashStrategy, SingletonPruning>(90);
    }

    /// Two iterators interleaved on one lazy trie: whichever reaches a child
    /// first expands it, and the other must then find the same table there.
    #[test]
    fn a_second_iterator_sees_the_first_ones_expansion() {
        let tuples = random_tuples(7, 80, 3, 6);
        let eager = HashTrieSip::from_tuples(3.into(), tuples.clone());
        let lazy = HashTrieSipLazy::from_tuples(3.into(), tuples);
        let (mut e1, mut e2) = (eager.hash_trie_iter(), eager.hash_trie_iter());
        let (mut l1, mut l2) = (lazy.hash_trie_iter(), lazy.hash_trie_iter());
        let (mut se1, mut sl1, mut se2, mut sl2) = (11u64, 11u64, 23u64, 23u64);
        for i in 0..400 {
            assert_eq!(
                step::<SipHashStrategy>(&mut e1, &mut se1),
                step::<SipHashStrategy>(&mut l1, &mut sl1),
                "iterator 1, step {i}"
            );
            assert_eq!(
                step::<SipHashStrategy>(&mut e2, &mut se2),
                step::<SipHashStrategy>(&mut l2, &mut sl2),
                "iterator 2, step {i}"
            );
        }
    }
}
```

- [ ] **Step 3: Run them**

Run: `CARGO_BUILD_JOBS=2 cargo test -p kermit-ds --test hash_trie_tests 2>&1 | grep -E "^test result|FAILED|panicked" | head`

Expected: `test result: ok`.

**Mutation check.** Equality depends on the evicted tuple being pushed
before the new one. In `implementation.rs`, swap the two lines
`pending.push(evicted.into_tuple());` and `pending.push(tuple);`, confirm
with `git diff` that the swap applied, and re-run the
`lazy_expansion::*pruned*` tests. Expected: at least one FAILS (Mod10
collisions make the chain order visible). Swap the lines back and confirm
`git diff kermit-ds/src` is empty. If nothing fails, the trace is too weak:
raise the step count or lower the domain, and record why.

- [ ] **Step 4: Parquet aliases**

In `parquet_tests.rs`:
- Add `ExpansionPolicy, LazyExpansion, NoPruning` to the `kermit_ds`
  import.
- Generalise the helper:

```rust
fn sorted_tuples<H: kermit_iters::HashStrategy, P: PruningPolicy, E: ExpansionPolicy>(
    relation: &HashTrie<H, P, E>,
) -> Vec<Vec<usize>> {
```

- After the pruned block, add:

```rust
// ── Layout variant: lazy child expansion ────────────────────────────────
type HashTrieSipLazy = HashTrie<SipHashStrategy, NoPruning, LazyExpansion>;
type HashTrieFxLazy = HashTrie<FxHashStrategy, NoPruning, LazyExpansion>;
type HashTrieSipPrunedLazy = HashTrie<SipHashStrategy, SingletonPruning, LazyExpansion>;
type HashTrieFxPrunedLazy = HashTrie<FxHashStrategy, SingletonPruning, LazyExpansion>;

parquet_test_suite!(HashTrieSipLazy, sorted_tuples);

parquet_test_suite!(HashTrieFxLazy, sorted_tuples);

parquet_test_suite!(HashTrieSipPrunedLazy, sorted_tuples);

parquet_test_suite!(HashTrieFxPrunedLazy, sorted_tuples);
```

Run: `CARGO_BUILD_JOBS=2 cargo test -p kermit-ds --test parquet_tests 2>&1 | grep -E "^test result|FAILED"`

Expected: `test result: ok`.

- [ ] **Step 5: Clippy, fmt, commit**

```bash
nix develop --command cargo fmt --all
CARGO_BUILD_JOBS=2 RUSTFLAGS=-Dwarnings cargo clippy -p kermit-ds --all-targets 2>&1 | tail -3
git add kermit-ds/tests
git commit -m "test(kermit-ds): lazy HashTrie aliases and eager/lazy trace equivalence (#92)

<trailer lines>"
```

---

### Task 3: CLI, execution cells and dispatch

All of `kermit/src` changes in one commit: the macro, `Execution` and the
families must agree before anything compiles.

**Files:**
- Modify: `kermit/src/options.rs`, `kermit/src/execution.rs`, `kermit/src/bench/run.rs`, `kermit/src/bench/ds.rs`, `kermit/src/main.rs`

- [ ] **Step 1: `ExpansionChoice` and `expansion_of` in `options.rs`**

- Add `ExpansionPolicy` to the `kermit_ds` import.
- After `pruning_of`, add:

```rust
/// CLI-side selector for `--ds-layout-expansion`: the `ExpansionPolicy`
/// monomorphised into `HashTrie<H, P, E>`. `Eager` builds every level at
/// construction, the structure that existed before the parameter. `Lazy`
/// builds each child below the root on the first probe that reaches it
/// (SIGMOD 2020 Figure 6).
///
/// Like [`HasherChoice`], this flag only applies when the selected index
/// structure is `hash-trie`; `validate_layout_choices` rejects it elsewhere.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum ExpansionChoice {
    /// Every level built at construction (`EagerExpansion`), the default.
    #[default]
    Eager,
    /// Children built on first probe (`LazyExpansion`).
    Lazy,
}

impl ExpansionChoice {
    /// The choice that monomorphises to the [`ExpansionPolicy`] marker
    /// whose [`LayoutOption::NAME`] is `name`, or `None` if no CLI choice
    /// does. The counterpart of [`HasherChoice::from_layout_name`].
    pub(crate) fn from_layout_name(name: &str) -> Option<Self> {
        match name {
            | "eager" => Some(Self::Eager),
            | "lazy" => Some(Self::Lazy),
            | _ => None,
        }
    }
}

/// The `--ds-layout-expansion` label of the [`ExpansionPolicy`] a code path
/// was monomorphised over. The counterpart of [`hasher_of`].
///
/// # Panics
///
/// Panics if `E`'s layout name has no [`ExpansionChoice`], with the same
/// caveat about what the round-trip test covers; see [`hasher_of`].
pub(crate) fn expansion_of<E: ExpansionPolicy>() -> ExpansionChoice {
    let name = <E as LayoutOption>::NAME;
    ExpansionChoice::from_layout_name(name).unwrap_or_else(|| {
        panic!(
            "no --ds-layout-expansion choice for expansion policy {name:?} ({})",
            std::any::type_name::<E>()
        )
    })
}
```

- [ ] **Step 2: Flag plumbing in `options.rs`**

- **`DsFlag`.** Add the variant `/// `--ds-layout-expansion`.` /
  `LayoutExpansion,` after `LayoutPruning`. In `structures`, the arm
  becomes `| Self::LayoutHasher | Self::LayoutPruning | Self::LayoutExpansion | Self::Config => &[IndexStructure::HashTrie],`.
  In `Display`, add `| Self::LayoutExpansion => "--ds-layout-expansion",`.
- **`LayoutChoices`.** After `hash_trie_pruning`, add:

```rust
    /// Child-expansion Layout of `HashTrie<H, P, E>` (default: `eager`).
    /// Only valid when `--indexstructure hash-trie` is selected.
    #[arg(long = "ds-layout-expansion", value_name = "EXPANSION", value_enum)]
    hash_trie_expansion: Option<ExpansionChoice>,
```

  and the accessors, after the pruning pair:

```rust
    /// Returns the `ExpansionChoice` to monomorphise on, applying the
    /// `ExpansionChoice::default()` when none was supplied on the command
    /// line. Use this at dispatch sites.
    pub(crate) fn hash_trie_expansion_resolved(&self) -> ExpansionChoice {
        self.hash_trie_expansion.unwrap_or_default()
    }

    /// Returns whether the user explicitly passed `--ds-layout-expansion`.
    pub(crate) fn hash_trie_expansion_explicit(&self) -> bool {
        self.hash_trie_expansion.is_some()
    }
```

  In `given()`, add `(DsFlag::LayoutExpansion, self.hash_trie_expansion_explicit()),`
  after the pruning row.
- **Prose.** Update `LayoutChoices`' and `validate_layout_choices`' doc
  comments to list `--ds-layout-expansion` with hasher and pruning as
  `hash-trie` flags.
- **`DsChoices`.** Add the field
  `/// `--ds-layout-expansion`; reaches the hash-trie cell only.` /
  `pub expansion: ExpansionChoice,` after `pruning`, and in `resolve` set
  `expansion: layout.hash_trie_expansion_resolved(),`.

- [ ] **Step 3: The 8-arm macro**

Replace `with_hash_trie_layout!` with the following. Update the doc comment
above it: the third dimension doubled the arms to 8; before a fourth, use a
nested macro.

```rust
macro_rules! with_hash_trie_layout {
    ($hasher:expr, $pruning:expr, $expansion:expr, | $H:ident, $P:ident, $E:ident | $body:expr) => {
        match ($hasher, $pruning, $expansion) {
            | (
                $crate::options::HasherChoice::Sip,
                $crate::options::PruningChoice::Off,
                $crate::options::ExpansionChoice::Eager,
            ) => {
                type $H = ::kermit_iters::SipHashStrategy;
                type $P = ::kermit_ds::NoPruning;
                type $E = ::kermit_ds::EagerExpansion;
                $body
            },
            | (
                $crate::options::HasherChoice::Sip,
                $crate::options::PruningChoice::Off,
                $crate::options::ExpansionChoice::Lazy,
            ) => {
                type $H = ::kermit_iters::SipHashStrategy;
                type $P = ::kermit_ds::NoPruning;
                type $E = ::kermit_ds::LazyExpansion;
                $body
            },
            | (
                $crate::options::HasherChoice::Sip,
                $crate::options::PruningChoice::On,
                $crate::options::ExpansionChoice::Eager,
            ) => {
                type $H = ::kermit_iters::SipHashStrategy;
                type $P = ::kermit_ds::SingletonPruning;
                type $E = ::kermit_ds::EagerExpansion;
                $body
            },
            | (
                $crate::options::HasherChoice::Sip,
                $crate::options::PruningChoice::On,
                $crate::options::ExpansionChoice::Lazy,
            ) => {
                type $H = ::kermit_iters::SipHashStrategy;
                type $P = ::kermit_ds::SingletonPruning;
                type $E = ::kermit_ds::LazyExpansion;
                $body
            },
            | (
                $crate::options::HasherChoice::Fxhash,
                $crate::options::PruningChoice::Off,
                $crate::options::ExpansionChoice::Eager,
            ) => {
                type $H = ::kermit_iters::FxHashStrategy;
                type $P = ::kermit_ds::NoPruning;
                type $E = ::kermit_ds::EagerExpansion;
                $body
            },
            | (
                $crate::options::HasherChoice::Fxhash,
                $crate::options::PruningChoice::Off,
                $crate::options::ExpansionChoice::Lazy,
            ) => {
                type $H = ::kermit_iters::FxHashStrategy;
                type $P = ::kermit_ds::NoPruning;
                type $E = ::kermit_ds::LazyExpansion;
                $body
            },
            | (
                $crate::options::HasherChoice::Fxhash,
                $crate::options::PruningChoice::On,
                $crate::options::ExpansionChoice::Eager,
            ) => {
                type $H = ::kermit_iters::FxHashStrategy;
                type $P = ::kermit_ds::SingletonPruning;
                type $E = ::kermit_ds::EagerExpansion;
                $body
            },
            | (
                $crate::options::HasherChoice::Fxhash,
                $crate::options::PruningChoice::On,
                $crate::options::ExpansionChoice::Lazy,
            ) => {
                type $H = ::kermit_iters::FxHashStrategy;
                type $P = ::kermit_ds::SingletonPruning;
                type $E = ::kermit_ds::LazyExpansion;
                $body
            },
        }
    };
}
```

- [ ] **Step 4: `execution.rs`**

- **Imports.** Add `ExpansionChoice, expansion_of` to the `crate::options`
  import and `ExpansionPolicy` to the `kermit_ds` import.
- **`Execution::HashHtj`.** After `pruning`, add:

```rust
        /// The `--ds-layout-expansion` choice, in the same two roles as
        /// `hasher` and `pruning`.
        expansion: ExpansionChoice,
```

  Update the variant's doc to `HashTrie<H, P, E>`, with `E` chosen by
  `--ds-layout-expansion`.
- **`for_pair` / `for_structure`.** Destructure `expansion` from
  `DsChoices` and pass it into `Execution::HashHtj { hasher, pruning, expansion, config }`.
- **`HashTrieFamily<H, P, E>`.** `_layout: PhantomData<(H, P, E)>`;
  `impl<H, P, E>` for `new` / `Default`;
  `impl<H: HashStrategy + 'static, P: PruningPolicy, E: ExpansionPolicy> RelationFamily for HashTrieFamily<H, P, E>`,
  with `type Rel = HashTrie<H, P, E>`, every `HashTrie<H, P>` becoming
  `HashTrie<H, P, E>`, and `execution()` adding
  `expansion: expansion_of::<E>(),`. Update its doc to name
  `--ds-layout-expansion` with the other two labels.
- **`HashHtj<H, P, E>`.** It holds `structure: HashTrieFamily<H, P, E>`;
  both impls gain `E: ExpansionPolicy`; delegate calls name
  `HashTrieFamily::<H, P, E>`; `type Engine = BTreeMap<String, HashTrie<H, P, E>>`;
  and `join_for_each` calls `hash_join_for_each::<HashTrie<H, P, E>, H>(…)`.
- **`ExecutionFamily`.** Add, after `type Engine;`:

```rust
    /// Whether running a join can change the engine's relations: a lazy
    /// `HashTrie` builds each child a join first reaches (issue #92). When
    /// true, `bench run` never probes the engine it loaded. `--verify` and
    /// `iteration` run on fresh builds, so `space` measures the relations
    /// as built and each timed join pays its own expansion. Required, like
    /// `build_relation`: every family states what its joins do.
    const JOIN_MUTATES: bool;
```

  `TrieLftj`'s impl states `const JOIN_MUTATES: bool = false;`, and
  `HashHtj`'s states `const JOIN_MUTATES: bool = E::LAZY;`.

- [ ] **Step 5: Call sites**

Each of the three `Execution::HashHtj { hasher, pruning, config }` patterns
becomes `{ hasher, pruning, expansion, config }` with
`with_hash_trie_layout!(hasher, pruning, expansion, |H, P, E| …)`:

- `kermit/src/bench/run.rs` (`dispatch_run_bench`):
  `&HashHtj::<H, P, E>::new(config, optimiser)`.
- `kermit/src/bench/ds.rs` (`dispatch_ds_bench`):
  `&HashTrieFamily::<H, P, E>::new(config)`.
- `kermit/src/main.rs` (`load_query_runner`):
  `HashHtj::<H, P, E>::new(config, optimiser)`.

Then run
`grep -rn "Execution::HashHtj {" kermit/src`. Every construction site,
including tests, needs the new field; patterns ending in `..` do not.

- [ ] **Step 6: Update and add the `options.rs` tests**

- Replace `layout_macro_binds_the_marker_pair_its_arm_names` with:

```rust
    /// Every `with_hash_trie_layout!` arm binds the marker triple its
    /// `(HasherChoice, PruningChoice, ExpansionChoice)` pattern names.
    /// Reading the labels back out of the aliases the macro defines pins
    /// the eight cells against `hasher_of` / `pruning_of` / `expansion_of`,
    /// so a transposed arm fails here rather than silently mislabelling a
    /// bench report.
    #[test]
    fn layout_macro_binds_the_marker_triple_its_arm_names() {
        for &hasher in HasherChoice::value_variants() {
            for &pruning in PruningChoice::value_variants() {
                for &expansion in ExpansionChoice::value_variants() {
                    let bound = with_hash_trie_layout!(hasher, pruning, expansion, |H, P, E| (
                        hasher_of::<H>(),
                        pruning_of::<P>(),
                        expansion_of::<E>()
                    ));
                    assert_eq!(bound, (hasher, pruning, expansion));
                }
            }
        }
    }
```

- After `pruning_choices_round_trip_through_layout_names`, add:

```rust
    /// The same round trip for `--ds-layout-expansion` and `ExpansionPolicy`.
    #[test]
    fn expansion_choices_round_trip_through_layout_names() {
        const TABLE: &[(ExpansionChoice, &str)] = &[
            (ExpansionChoice::Eager, <EagerExpansion as LayoutOption>::NAME),
            (ExpansionChoice::Lazy, <LazyExpansion as LayoutOption>::NAME),
        ];
        for choice in ExpansionChoice::value_variants() {
            let (_, name) = TABLE
                .iter()
                .find(|(c, _)| c == choice)
                .expect("every ExpansionChoice has a marker");
            assert_eq!(ExpansionChoice::from_layout_name(name), Some(*choice));
        }
        assert_eq!(expansion_of::<EagerExpansion>(), ExpansionChoice::Eager);
        assert_eq!(expansion_of::<LazyExpansion>(), ExpansionChoice::Lazy);
    }

    #[test]
    fn validate_layout_choices_rejects_explicit_expansion_on_non_hash_trie() {
        let layout = LayoutChoices {
            hash_trie_expansion: Some(ExpansionChoice::Lazy),
            ..LayoutChoices::default()
        };
        assert!(validate_layout_choices(IndexStructureSelector::HashTrie, &layout).is_ok());
        assert!(validate_layout_choices(IndexStructureSelector::All, &layout).is_ok());
        for sel in [
            IndexStructureSelector::TreeTrie,
            IndexStructureSelector::ColumnTrie,
        ] {
            let msg = validate_layout_choices(sel, &layout)
                .unwrap_err()
                .to_string();
            assert!(msg.contains("--ds-layout-expansion"), "{sel:?}: {msg}");
            assert!(msg.contains("hash-trie"), "{sel:?}: {msg}");
        }
    }

    #[test]
    fn expansion_choice_default_is_eager() {
        assert_eq!(ExpansionChoice::default(), ExpansionChoice::Eager);
        assert_eq!(
            LayoutChoices::default().hash_trie_expansion_resolved(),
            ExpansionChoice::Eager
        );
        assert!(!LayoutChoices::default().hash_trie_expansion_explicit());
    }
```

  Add `EagerExpansion, LazyExpansion` to the test module's `kermit_ds`
  import.
- In `ds_flag_given_lists_exactly_the_flags_passed`, add
  `hash_trie_expansion: Some(ExpansionChoice::Lazy),` to the full
  `LayoutChoices`, and `DsFlag::LayoutExpansion` after
  `DsFlag::LayoutPruning` in the expected vector.
- In the `DsChoices::resolve` test (the one asserting
  `choices.pruning == PruningChoice::Off`), add
  `assert_eq!(choices.expansion, ExpansionChoice::Eager);`.
- Look for any other `LayoutChoices { … }` literal that lists every field
  without `..Default::default()` (e.g. around the `DsFlag` tests): the
  compiler names them all.

- [ ] **Step 7: Update the `execution.rs` and `run.rs` tests**

- In `execution.rs` tests:
  - Add `expansion: ExpansionChoice::Eager,` to the expected
    `Execution::HashHtj` in the `for_pair` test.
  - In `for_structure_agrees_with_for_pair`, add a loop
    `for &expansion in ExpansionChoice::value_variants() {` around the
    `DsChoices` literal and include `expansion` in it.
  - In `structure_markers_agree_with_join_families`, keep the pruned Fx
    pair and add the same assertion for
    `HashTrieFamily::<FxHashStrategy, SingletonPruning, LazyExpansion>` vs
    `HashHtj::<…, LazyExpansion>`.
  - In `families_report_their_own_execution`, add
    `expansion: ExpansionChoice::Eager,` to the expected value.
- Replace the labels helper in
  `hash_family_labels_are_derived_from_its_layout_types` with:

```rust
        fn labels<H: HashStrategy + 'static, P: PruningPolicy, E: ExpansionPolicy>(
        ) -> (HasherChoice, PruningChoice, ExpansionChoice) {
            match HashHtj::<H, P, E>::new(HashTrieConfig::default(), Optimiser::Lexicographic)
                .execution()
            {
                | Execution::HashHtj {
                    hasher,
                    pruning,
                    expansion,
                    ..
                } => (hasher, pruning, expansion),
                | other => panic!("hash family reported {other:?}"),
            }
        }
```

  Its assertions become the eight triples: the existing four with
  `EagerExpansion` / `ExpansionChoice::Eager` appended, plus the same four
  with `LazyExpansion` / `ExpansionChoice::Lazy`. Update its doc comment:
  "All eight".
- Add one test:

```rust
    /// Only a lazy HashTrie family's joins change its relations (#92).
    #[test]
    fn only_lazy_hash_families_mutate_on_join() {
        use kermit_iters::SipHashStrategy;
        const { assert!(!<TrieLftj<TreeTrie> as ExecutionFamily>::JOIN_MUTATES) };
        const { assert!(!<HashHtj<SipHashStrategy, NoPruning, EagerExpansion> as ExecutionFamily>::JOIN_MUTATES) };
        const { assert!(<HashHtj<SipHashStrategy, NoPruning, LazyExpansion> as ExecutionFamily>::JOIN_MUTATES) };
        const { assert!(<HashHtj<SipHashStrategy, SingletonPruning, LazyExpansion> as ExecutionFamily>::JOIN_MUTATES) };
    }
```

  Add `EagerExpansion, LazyExpansion` to the test module's `kermit_ds`
  import.
- In `run.rs`'s `resolve_sweep_rejects_a_flag_the_algorithm_leaves_without_a_cell`,
  add `LayoutExpansion` beside `LayoutPruning` in all three rows.

- [ ] **Step 8: Build, test, lint**

Run each in turn:
- `CARGO_BUILD_JOBS=2 cargo test -p kermit --bins 2>&1 | grep -E "^test result|FAILED|panicked|error\[" | head`
- `CARGO_BUILD_JOBS=2 cargo test -p kermit --lib 2>&1 | grep -E "^test result|FAILED|panicked|error\[" | head`
- `nix develop --command cargo fmt --all`
- `CARGO_BUILD_JOBS=2 RUSTFLAGS=-Dwarnings cargo clippy --workspace --all-targets 2>&1 | tail -5`

Expected: tests ok; no warnings. Integration tests that name
`HashTrie<H, P>` generically (`kermit/tests/common/utils.rs`,
`result_allocation.rs`) still compile, since they mean eager.

- [ ] **Step 9: Commit**

```bash
git add kermit/src
git commit -m "feat(cli): --ds-layout-expansion eager|lazy for hash-trie (#92)

ExpansionChoice and the ds_layout_expansion label derived from E,
DsFlag::LayoutExpansion, an 8-arm with_hash_trie_layout!, and
Execution::HashHtj { expansion }. ExecutionFamily gains JOIN_MUTATES
(E::LAZY for HashHtj, false for TrieLftj); run_benchmark reads it next.

<trailer lines>"
```

---

### Task 4: The bench probe rule (cold `iteration`, as-built `space`)

**Files:**
- Modify: `kermit/src/bench/run.rs`
- Create: `kermit/tests/cli_hash_trie_layout_expansion.rs`

- [ ] **Step 1: Write the failing CLI test**

Create `kermit/tests/cli_hash_trie_layout_expansion.rs`:

```rust
//! CLI smoke test: `--ds-layout-expansion lazy` records
//! `ds_layout_expansion: "lazy"`, the absent flag records `"eager"`, and
//! the flag is rejected on a structure without the dimension. Mirrors
//! `cli_hash_trie_layout_pruning.rs`.
//!
//! It also pins the probe rule of issue #92: a lazy family's loaded engine
//! is never probed. `run_benchmark` checks that its relations still have
//! their as-built footprint before measuring `space`, and fails the run if
//! `--verify` or `iteration` reached them.

mod common;

use common::cli::{axes_of, bench_ds, bench_run, reports_of};

fn assert_success(output: &std::process::Output) {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn cli_bench_ds_with_lazy_expansion_records_axis() {
    let (output, report) = bench_ds("hash-trie", &["--ds-layout-expansion", "lazy"]);
    assert_success(&output);
    let axes = axes_of(&report);
    assert_eq!(axes["ds_layout_expansion"], "lazy", "{axes}");
    assert_eq!(axes["ds_layout_pruning"], "off", "{axes}");
}

#[test]
fn cli_bench_ds_default_expansion_is_eager() {
    let (output, report) = bench_ds("hash-trie", &[]);
    assert_success(&output);
    assert_eq!(axes_of(&report)["ds_layout_expansion"], "eager");
}

#[test]
fn cli_bench_ds_rejects_expansion_flag_on_tree_trie() {
    let (output, _) = bench_ds("tree-trie", &["--ds-layout-expansion", "lazy"]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("--ds-layout-expansion"), "{stderr}");
}

#[test]
fn cli_bench_run_sweep_carries_expansion_only_to_hash_trie_cells() {
    let (output, report) = bench_run("triangle", &[
        "-i",
        "all",
        "-a",
        "all",
        "-m",
        "space",
        "--ds-layout-expansion",
        "lazy",
    ]);
    assert_success(&output);
    let reports = reports_of(&report);
    assert_eq!(reports.len(), 3, "three valid cells: {reports:?}");
    for r in &reports {
        let axes = &r["axes"];
        if axes["data_structure"] == "HashTrie" {
            assert_eq!(axes["ds_layout_expansion"], "lazy", "{axes}");
        } else {
            assert!(axes.get("ds_layout_expansion").is_none(), "{axes}");
        }
    }
}

/// `--verify` and `iteration` run before `space` here. Both would expand
/// the loaded engine if they probed it, and `run_benchmark`'s as-built
/// check would then fail the run. Triangle's vertex 1 has four out-edges,
/// so its child is a multi-tuple `Unexpanded` list under either pruning
/// setting.
#[test]
fn cli_bench_run_lazy_probes_leave_the_engine_as_built() {
    for pruning in ["off", "on"] {
        let (output, report) = bench_run("triangle", &[
            "-i",
            "hash-trie",
            "-a",
            "hash-triejoin",
            "--ds-layout-expansion",
            "lazy",
            "--ds-layout-pruning",
            pruning,
            "-m",
            "iteration",
            "space",
            "--verify",
        ]);
        assert_success(&output);
        let axes = axes_of(&report);
        assert_eq!(axes["verified"], true, "{axes}");
        assert_eq!(axes["ds_layout_expansion"], "lazy", "{axes}");
    }
}
```

- [ ] **Step 2: Run it to see the probe test fail**

Run: `CARGO_BUILD_JOBS=2 cargo test -p kermit --test cli_hash_trie_layout_expansion 2>&1 | grep -E "^test |test result"`

Expected: the four axis tests PASS, since Task 3 already wired the flag.
`cli_bench_run_lazy_probes_leave_the_engine_as_built` also passes at this
point, because nothing checks the engine yet. That is why Step 4 runs a
mutation check after the guard exists.

- [ ] **Step 3: Implement the rule in `run_benchmark`**

Add `kermit_ds::HeapSize` to the `kermit_ds` import of `run.rs`.

Replace the `rebuilds` computation:

```rust
    // Load each relation from disk exactly once; the family builds its
    // engine from these typed relations rather than re-reading the files.
    // Rebuilding metrics rebuild from each relation's tuples in file order,
    // kept here (see `RelationFamily::load_with_tuples`): `insertion`,
    // `end_to_end`, and, for a family whose joins mutate their relations,
    // `iteration` and `--verify`, which then never probe the loaded engine.
    // A run with no rebuilding metric keeps none: it would be a dead copy
    // of the whole workload.
    let rebuilds = metrics
        .iter()
        .any(|m| matches!(m, Metric::Insertion | Metric::EndToEnd))
        || (F::JOIN_MUTATES && (verify || metrics.contains(&Metric::Iteration)));
```

Right after `let relations = F::relations(&engine);`, add:

```rust
    // A family whose joins mutate their relations (a lazy HashTrie expands
    // what a join reaches, #92) never probes this engine: `--verify` and
    // `iteration` run on fresh builds, so `space` measures the relations
    // as built, whatever ran before it. Their footprint now is what each
    // `space` block checks against.
    let as_built_bytes: Option<Vec<usize>> =
        F::JOIN_MUTATES.then(|| relations.iter().map(|r| r.heap_size_bytes()).collect());
```

In the verify block, replace
`let actual = family.count(&engine, query_def.query.clone())?;` with:

```rust
                    let actual = if F::JOIN_MUTATES {
                        let fresh = family.build_from_tuples(build_inputs.clone());
                        family.count(&fresh, query_def.query.clone())?
                    } else {
                        family.count(&engine, query_def.query.clone())?
                    };
```

Replace the `iteration` block's `group.bench_function("iteration", …)` call
with:

```rust
                if F::JOIN_MUTATES {
                    // Cold (#92): each timed join runs on an engine built in
                    // the untimed setup, so it pays the expansion its own
                    // probes cause instead of finding it done by an earlier
                    // sample. The routine hands the engine back, so
                    // Criterion drops it after timing, and `PerIteration`
                    // keeps one fresh engine alive at a time.
                    group.bench_function("iteration", |b| {
                        b.iter_batched(
                            || {
                                (
                                    family.build_from_tuples(build_inputs.clone()),
                                    query_def.query.clone(),
                                )
                            },
                            |(fresh, q)| {
                                let rows = family.count(&fresh, q).expect(VALIDATED);
                                (fresh, rows)
                            },
                            criterion::BatchSize::PerIteration,
                        );
                    });
                } else {
                    group.bench_function("iteration", |b| {
                        b.iter_batched(
                            || query_def.query.clone(),
                            |q| family.count(&engine, q).expect(VALIDATED),
                            criterion::BatchSize::SmallInput,
                        );
                    });
                }
```

Keep the existing comment above (counted, never collected; issue #65).

At the top of the `if metrics.contains(&Metric::Space) {` block, before
building the Criterion instance, add:

```rust
            if let Some(as_built) = &as_built_bytes {
                let now: Vec<usize> = relations.iter().map(|r| r.heap_size_bytes()).collect();
                anyhow::ensure!(
                    now == *as_built,
                    "internal error: a probe reached the loaded engine of a {ds_name} cell whose \
                     joins mutate their relations, so `space` would measure them partly \
                     expanded; see `ExecutionFamily::JOIN_MUTATES`"
                );
            }
```

- [ ] **Step 4: Run, then mutation-check the guard**

Run: `CARGO_BUILD_JOBS=2 cargo test -p kermit --test cli_hash_trie_layout_expansion 2>&1 | grep -E "test result"`

Expected: `ok. 5 passed`.

Commit it first, so the mutant cannot cost the fix:

```bash
git add kermit/src/bench/run.rs kermit/tests/cli_hash_trie_layout_expansion.rs
git commit -m "feat(bench): lazy families never probe the loaded engine (#92)

iteration and --verify run on fresh builds when F::JOIN_MUTATES, so a
timed join pays its own expansion and space always sees the relations
as built; run_benchmark checks the as-built footprint before space.
Eager cells take today's path unchanged.

<trailer lines>"
```

**Mutant 1 (verify).** In `run.rs`, change
`let actual = if F::JOIN_MUTATES {` to `let actual = if false {`. Confirm
with `git diff --stat` that exactly one line changed, then re-run the probe
test. Expected: FAIL with "internal error: a probe reached the loaded
engine". Restore with the reverse edit; `git diff` is empty.

**Mutant 2 (iteration).** Change `if F::JOIN_MUTATES {` (the iteration
block's) to `if false {`, confirm, and re-run. Expected: FAIL in the same
way. Restore and confirm `git diff` is empty.

If either mutant passes, the guard is not reaching that path: fix the test,
not the guard, and re-run both mutants.

- [ ] **Step 5: Lint, then amend nothing**

Run: `CARGO_BUILD_JOBS=2 RUSTFLAGS=-Dwarnings cargo clippy -p kermit --all-targets 2>&1 | tail -3`
and `nix develop --command cargo fmt --all --check`.

If fmt or clippy changes anything, commit the fix as a separate
`style:` / `fix:` commit. Never amend: the branch may be shared.

---

### Task 5: Join-layer coverage

**Files:**
- Modify: `kermit/tests/common/utils.rs`, `kermit/tests/join_tests.rs`, `kermit/tests/result_allocation.rs`, `kermit/tests/cli_query_errors.rs`

- [ ] **Step 1: `JoinEntry` over `E`**

In `kermit/tests/common/utils.rs`, add `ExpansionPolicy` to the `kermit_ds`
import. The two impls become
`impl<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy> JoinEntry<HashTrie<H, P, E>> for HashTriejoin`
and
`impl<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy, C: ConfigProvider<HashTrieConfig>> JoinEntry<Configured<HashTrie<H, P, E>, C>> for HashTriejoin`.
Every `HashTrie<H, P>` inside them becomes `HashTrie<H, P, E>`.

- [ ] **Step 2: The join-layer aliases**

In `kermit/tests/join_tests.rs`, add `LazyExpansion, NoPruning` to the
`kermit_ds` import. Retitle the alias block `// ── Layout aliases: hasher × pruning × expansion ──…`
and add:

```rust
type HashTrieSipLazy = HashTrie<SipHashStrategy, NoPruning, LazyExpansion>;
type HashTrieFxLazy = HashTrie<FxHashStrategy, NoPruning, LazyExpansion>;
type HashTrieSipPrunedLazy = HashTrie<SipHashStrategy, SingletonPruning, LazyExpansion>;
type HashTrieFxPrunedLazy = HashTrie<FxHashStrategy, SingletonPruning, LazyExpansion>;
```

After the eight `HashTrie…` suite invocations, add:

```rust
define_multiway_join_test_suite!(HashTrieSipLazy, HashTriejoin, LexicographicOptimiser);
define_multiway_join_test_suite!(HashTrieSipLazy, HashTriejoin, CardinalityOptimiser);
define_multiway_join_test_suite!(HashTrieFxLazy, HashTriejoin, LexicographicOptimiser);
define_multiway_join_test_suite!(HashTrieFxLazy, HashTriejoin, CardinalityOptimiser);
define_multiway_join_test_suite!(HashTrieSipPrunedLazy, HashTriejoin, LexicographicOptimiser);
define_multiway_join_test_suite!(HashTrieSipPrunedLazy, HashTriejoin, CardinalityOptimiser);
define_multiway_join_test_suite!(HashTrieFxPrunedLazy, HashTriejoin, LexicographicOptimiser);
define_multiway_join_test_suite!(HashTrieFxPrunedLazy, HashTriejoin, CardinalityOptimiser);
```

After the existing `HalfFull` config invocations, add:

```rust
// Expansion builds each child under the configured cap too.
define_multiway_join_test_suite_with_config!(
    HashTrieSipLazy,
    HashTriejoin,
    LexicographicOptimiser,
    HalfFull
);
```

Run: `CARGO_BUILD_JOBS=2 cargo test -p kermit --test join_tests 2>&1 | grep -E "test result|FAILED"`

Expected: `ok`, with 16 × 9 = 144 more tests than before (8 lazy suite
invocations plus 1 config invocation).

- [ ] **Step 3: Warm allocation cells**

In `kermit/tests/result_allocation.rs`:
- Add `ExpansionPolicy, LazyExpansion` to the `kermit_ds` import.
- Generalise `htj_join_allocations<H, P>` to
  `htj_join_allocations<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>`
  over `&BTreeMap<String, HashTrie<H, P, E>>`, calling
  `hash_join_for_each::<HashTrie<H, P, E>, H>`.
- Add, after `htj_allocations`:

```rust
/// Allocations made by a *second* streamed HashTriejoin join of `query`
/// over the same lazy `relations`. The first join expands every child it
/// reaches, so the second allocates only what the join path itself does.
/// A cold lazy join allocates once per child it expands, by design
/// (issue #92), so the per-row and per-descent checks apply warm. The
/// eager cells' unmeasured first call builds fresh relations, which would
/// leave a lazy cell cold, hence this helper.
fn htj_warm_join_allocations<H: HashStrategy, P: PruningPolicy>(
    query: &str, relations: &BTreeMap<String, HashTrie<H, P, LazyExpansion>>, rows: usize,
) -> u64 {
    htj_join_allocations(query, relations, rows);
    htj_join_allocations(query, relations, rows)
}

/// [`htj_allocations`] for the lazy Layout, measured warm.
fn htj_lazy_allocations<H: HashStrategy, P: PruningPolicy>(fan_out: usize) -> u64 {
    let relations = relations::<HashTrie<H, P, LazyExpansion>>(fan_out);
    htj_warm_join_allocations(QUERY, &relations, XS * fan_out)
}

/// [`htj_descent_allocations`] for the lazy Layout, measured warm.
fn htj_lazy_descent_allocations<H: HashStrategy, P: PruningPolicy>(dead_ends: usize) -> u64 {
    let relations = descent_relations::<HashTrie<H, P, LazyExpansion>>(dead_ends);
    htj_warm_join_allocations(DESCENT_QUERY, &relations, DESCENT_ROWS)
}
```

  (`htj_lazy_descent_allocations` uses `DESCENT_QUERY` / `DESCENT_ROWS` /
  `descent_relations`, which are defined lower in the file. Place it after
  `htj_descent_allocations` so it reads in order.)
- Add the result-size cells after `hash_trie_fx_pruned_allocates_independently_of_result_size`:

```rust
#[test]
fn hash_trie_sip_lazy_allocates_independently_of_result_size() {
    assert_flat(
        "HashTrie<Sip, Lazy>/HTJ",
        htj_lazy_allocations::<SipHashStrategy, NoPruning>(SMALL),
        htj_lazy_allocations::<SipHashStrategy, NoPruning>(LARGE),
    );
}

#[test]
fn hash_trie_fx_lazy_allocates_independently_of_result_size() {
    assert_flat(
        "HashTrie<Fx, Lazy>/HTJ",
        htj_lazy_allocations::<FxHashStrategy, NoPruning>(SMALL),
        htj_lazy_allocations::<FxHashStrategy, NoPruning>(LARGE),
    );
}

#[test]
fn hash_trie_sip_pruned_lazy_allocates_independently_of_result_size() {
    assert_flat(
        "HashTrie<Sip, Pruned, Lazy>/HTJ",
        htj_lazy_allocations::<SipHashStrategy, SingletonPruning>(SMALL),
        htj_lazy_allocations::<SipHashStrategy, SingletonPruning>(LARGE),
    );
}

#[test]
fn hash_trie_fx_pruned_lazy_allocates_independently_of_result_size() {
    assert_flat(
        "HashTrie<Fx, Pruned, Lazy>/HTJ",
        htj_lazy_allocations::<FxHashStrategy, SingletonPruning>(SMALL),
        htj_lazy_allocations::<FxHashStrategy, SingletonPruning>(LARGE),
    );
}
```

- Add the four matching descent cells after the last HashTriejoin descent
  test. Name them
  `hash_trie_{sip,fx,sip_pruned,fx_pruned}_lazy_allocates_independently_of_descent_count`,
  each calling
  `assert_flat_in_descents("HashTrie<…, Lazy>/HTJ", htj_lazy_descent_allocations::<H, P>(FEW_DEAD_ENDS), htj_lazy_descent_allocations::<H, P>(MANY_DEAD_ENDS))`
  with the same `(H, P)` pairs as above.
- Add two scan cells after the hash scan tests: the scan never expands, so
  an as-built lazy trie is measured directly. Generalise
  `hash_scan_allocations<H, P>` to take `E: ExpansionPolicy` and build
  `HashTrie::<H, P, E>`, then turbofish the existing four calls with
  `NoPruning`/`SingletonPruning` and `kermit_ds::EagerExpansion`. Add:

```rust
#[test]
fn hash_trie_sip_lazy_scan_allocates_independently_of_relation_size() {
    hash_scan_allocations::<SipHashStrategy, NoPruning, LazyExpansion>(SMALL);
    assert_flat(
        "HashTrie<Sip, Lazy> scan",
        hash_scan_allocations::<SipHashStrategy, NoPruning, LazyExpansion>(SMALL),
        hash_scan_allocations::<SipHashStrategy, NoPruning, LazyExpansion>(LARGE),
    );
}

#[test]
fn hash_trie_sip_pruned_lazy_scan_allocates_independently_of_relation_size() {
    hash_scan_allocations::<SipHashStrategy, SingletonPruning, LazyExpansion>(SMALL);
    assert_flat(
        "HashTrie<Sip, Pruned, Lazy> scan",
        hash_scan_allocations::<SipHashStrategy, SingletonPruning, LazyExpansion>(SMALL),
        hash_scan_allocations::<SipHashStrategy, SingletonPruning, LazyExpansion>(LARGE),
    );
}
```

- Add a paragraph to the module doc's issue list: "Issue #92 adds the lazy
  expansion Layout. Its join cells measure a *second* join over the same
  relations, because a cold lazy join allocates once per child it expands,
  by design. The scan never expands, so its lazy cells measure the trie as
  built."

Run: `CARGO_BUILD_JOBS=2 cargo test -p kermit --test result_allocation 2>&1 | grep -E "test result|FAILED"`

Expected: `ok`.

**Sanity check of the warm helper.** Temporarily make
`htj_warm_join_allocations` return only the *first* call's count. Confirm
with `git diff` that the edit applied, then re-run
`hash_trie_sip_lazy_allocates_independently_of_result_size`. Expected:
FAIL: the cold join's expansion allocates more at `LARGE`, because bigger
child tables double more times. Restore with the reverse edit. If it
passes cold, the warm helper is not doing what its doc claims; say so in
the doc rather than leaving a false comment.

- [ ] **Step 4: `cli_query_errors.rs` cells**

- Change `const CELLS: [&[&str]; 6]` to `[&[&str]; 10]`.
- Append the four lazy hash cells, each an existing hash cell's arguments
  plus `"--ds-layout-expansion", "lazy"`: (sip, off), (sip, on),
  (fxhash, off), (fxhash, on). Example:

```rust
    &[
        "-i",
        "hash-trie",
        "-a",
        "hash-triejoin",
        "--ds-layout-hasher",
        "sip",
        "--ds-layout-pruning",
        "off",
        "--ds-layout-expansion",
        "lazy",
    ],
```

- Update the module doc: "on all ten cells: … HashTrie under Hash
  Triejoin with each hasher × pruning × expansion Layout". Update the
  `CELLS` doc: "The ten (structure, algorithm, Layout) cells".

Run: `RUST_BACKTRACE=0 CARGO_BUILD_JOBS=2 cargo test -p kermit --test cli_query_errors 2>&1 | grep -E "test result|FAILED"`

Expected: `ok`. Errors are byte-identical across all ten cells, and the
placeholder query returns the same rows on every cell. That second check is
the `kermit join` path through a lazy trie.

- [ ] **Step 5: fmt, clippy, commit**

```bash
nix develop --command cargo fmt --all
CARGO_BUILD_JOBS=2 RUSTFLAGS=-Dwarnings cargo clippy --workspace --all-targets 2>&1 | tail -3
git add kermit/tests
git commit -m "test(kermit): lazy HashTrie join suites, warm allocation cells, CLI cells (#92)

<trailer lines>"
```

---

### Task 6: kermit-lab back-fill

**Files:**
- Modify: `python/kermit-lab/kermit_lab/defaults.py`, `python/kermit-lab/tests/test_defaults.py`, `python/kermit-lab/tests/test_frame.py`

- [ ] **Step 1: Failing tests**

- In `tests/test_defaults.py`, add this row to the parametrize list of
  `test_hash_trie_axes_backfill_hash_trie_rows_only`:

```python
        # Pre-#92 reports built every level eagerly.
        ("ds_layout_expansion", "eager", "lazy"),
```

  and change that test's docstring to "no hasher, pruning, expansion or
  load factor".
- In `tests/test_frame.py`:
  - change `_HASH_TRIE_AXES` to
    `["ds_layout_hasher", "ds_layout_pruning", "ds_layout_expansion", "ds_config_load_factor"]`;
  - in `test_hash_trie_reports_without_the_axes_are_back_filled`, add
    `assert (old["ds_layout_expansion"] == "eager").all()`.
  - `test_hash_trie_axes_stay_off_sorted_trie_rows` selects
    `sorted_tries[_HASH_TRIE_AXES]`, which raises `KeyError` if the column
    is absent from the fixture frame. If the fixture has no
    `ds_layout_expansion` anywhere, keep that test on the old three-axis
    list: define a separate `_HASH_TRIE_BACKFILLED` list for the
    back-fill test rather than editing the shared one.

Run: `cd python/kermit-lab && uv run pytest tests/test_defaults.py tests/test_frame.py -q 2>&1 | tail -5`

Expected: FAIL on the new expansion assertions.

- [ ] **Step 2: The default**

In `kermit_lab/defaults.py`, after the pruning entry:

```python
    # Every HashTrie before issue #92 built all levels at construction.
    ("ds_layout_expansion", "HashTrie"): "eager",
```

Run the same pytest command. Expected: pass. Then run the whole suite:
`uv run pytest -q 2>&1 | tail -3`. Expected: all pass. No `AXIS_PHASES`
entry is added: expansion affects every phase, including `space`.

- [ ] **Step 3: Commit**

```bash
git add python/kermit-lab
git commit -m "feat(kermit-lab): back-fill ds_layout_expansion=eager on HashTrie rows (#92)

<trailer lines>"
```

---

### Task 7: Full gate

- [ ] **Step 1: The CI-equivalent checks, one per command (foreground, `CARGO_BUILD_JOBS=2`)**

```bash
CARGO_BUILD_JOBS=2 cargo test --workspace 2>&1 | grep -E "^test result|FAILED|panicked" | sort | uniq -c | tail
CARGO_BUILD_JOBS=2 RUSTFLAGS=-Dwarnings cargo clippy --all-targets 2>&1 | tail -3
nix develop --command cargo fmt --all --check
RUSTDOCFLAGS=-Dwarnings cargo doc --workspace --no-deps 2>&1 | tail -3
cd python/kermit-lab && KERMIT_BIN=$PWD/../../target/debug/kermit uv run pytest -q 2>&1 | tail -3
```

Expected: no FAILED; no warnings; fmt clean; docs clean; pytest green. If
`cargo test --workspace` exceeds 10 minutes, run it per crate (`-p kermit`
separately).

- [ ] **Step 2: Miri on `kermit-ds` (detached)**

```bash
export MIRI_SYSROOT=<scratchpad>/miri-sysroot
setsid nohup nix develop --command bash -c 'MIRIFLAGS="-Zmiri-disable-isolation" cargo miri setup && MIRIFLAGS="-Zmiri-disable-isolation" cargo miri test -p kermit-ds; echo EXIT=$?' > <scratchpad>/miri.log 2>&1 < /dev/null & disown
```

Wait with `until grep -q '^EXIT=' <scratchpad>/miri.log; do sleep 30; done`.
Expected: `EXIT=0`. Lazy expansion is the first `RefCell` / `OnceCell` in
`kermit-ds`; miri checks the borrows.

- [ ] **Step 3: Fix anything found in a new commit** (never amend).

---

### Task 8: Documentation

**Files:**
- Modify: `docs/data-structures/hash-trie.md`, `docs/specs/optimization-standard.md`, `docs/specs/bench-report-schema.md`, `docs/algorithms/hash-triejoin.md`, `CLAUDE.md`, `docs/specs/2026-10-05-hash-trie-lazy-expansion-design.md`

- [ ] **Step 1: `hash-trie.md`**

- **Layout entry.** Under the Layout flags, after "Singleton pruning",
  add:

```markdown
- **Lazy child expansion** (`ds_layout_expansion`): builds only the root
  table at construction. Every child below it keeps its tuples as a list
  until a probe first opens it, and then that child's table is built, one
  level at a time (paper §3.3.1, Figure 6). It is a *shape*: an unexpanded
  child is a node state that eager tries must not carry, so it is a Layout.
  - **CLI:** `-i hash-trie --ds-layout-expansion <eager|lazy>` (on
    `kermit join`, `bench join`, `bench run` and `bench ds`).
  - **Choices:**
    - `eager` (default; `EagerExpansion`, every level built at construction)
    - `lazy` (`LazyExpansion`)
  - **Type-level:** `HashTrie<H, P, E: ExpansionPolicy>` (in
    [`expansion.rs`](../../kermit-ds/src/ds/hash_trie/expansion.rs)).
    `HashTrieNode::Unexpanded` holds `E::Pending<Node>`:
    - under `lazy`, a `Box<LazyChild>` with the pending tuples in a
      `RefCell` and the built table in a `OnceCell`;
    - under `eager`, the uninhabited `Never`, so eager tries keep their node
      and frame sizes (pinned by
      `node_does_not_grow_under_the_expansion_policy` and
      `off_frame_is_the_bare_table_pair`) and stay `Sync`.

    Lazy tries are `!Sync`.
  - **Who expands:** only `HashTrieIter::open`, through `HashTrie::resolve`.
    `collect_tuples`, `for_each_tuple`, `heap_size_bytes`, `project` and the
    Parquet round-trip read the pending list and expand nothing.
  - **With pruning:** a bucket with one tuple below it is a `Singleton` and
    never expands; two or more make an `Unexpanded` list.
  - **Equivalence:** an expanded child is the eager table at that position,
    bucket for bucket, because its tuples are re-inserted in insertion order
    under the same load factor. The trace tests in
    `kermit-ds/tests/hash_trie_tests.rs` pin this.
  - **Bench methodology:** a lazy family never probes the engine
    `bench run` loaded. `iteration` builds a fresh engine per sample in
    untimed setup, so each timed join pays its own expansion (cold), and
    `--verify` also runs on a fresh build. `space` therefore always measures
    the relations as built. `end_to_end` with `--queries-per-build K > 1`
    shows the amortisation: the first query expands and the rest run warm.
  - **Test aliases:**
    - join layer: `HashTrieSipLazy`, `HashTrieFxLazy`,
      `HashTrieSipPrunedLazy`, `HashTrieFxPrunedLazy`;
    - DS layer: those plus `HashTrieMod10Lazy`, `HashTrieMod10PrunedLazy`
      and `HashTrieSipDenseLazy`.
  - **Bench axis value:** `"eager"` or `"lazy"`.
  - **Measured effect:** see Task 9's record (to be filled in by the measurement task).
```

- **Deferred follow-ups.** Delete the "Lazy child expansion" bullet.
- **Representation section.** Add a paragraph and a micro-example. For
  `R = {(1,2), (1,3), (4,5)}` under `lazy`, the root holds two buckets:
  `h(1) → Unexpanded[(1,2), (1,3)]` and `h(4) → Unexpanded[(4,5)]`. The
  first `open` into `h(1)` builds `Leaf{h(2): [(1,2)], h(3): [(1,3)]}` and
  leaves `h(4)` untouched.
- **Complexity table.** Under `lazy`, `insert` does O(1) work below the
  root while the child is unexpanded; the first `open` of a child with
  `k` tuples costs O(k); later `open`s cost O(1).
- **See also.** Update the "every Layout combination" sentence to the
  eight join-layer aliases.

- [ ] **Step 2: `optimization-standard.md`**

- Change "Five optimizations are implemented — three Layout dimensions,
  one Config value and one BuildMode" to "Six optimizations are
  implemented — four Layout dimensions, one Config value and one
  BuildMode".
- Add a row to the landed table:
  `| Lazy child expansion (eager/lazy) | Layout | `ds_layout_expansion` | §3.3.1, Fig 6 |`
- Delete the "Lazy child expansion" row from "Available to add".
- In the "Examples (potential)" row (line ~85), change `lazy expansion` to
  `lazy expansion ✓`.
- Add `kermit/tests/cli_hash_trie_layout_expansion.rs` to the CLI smoke
  tests row.

- [ ] **Step 3: `bench-report-schema.md`**

- Add `ds_layout_expansion` (`"eager"` / `"lazy"`, HashTrie only) where
  `ds_layout_pruning` is listed.
- Where `iteration` is defined, add: "One join against the relations in
  their as-built state. For a structure whose joins mutate it (a lazy
  HashTrie, #92), each sample runs on a fresh build made in untimed setup;
  every other structure reuses one engine, which is the same state. The
  schema version is unchanged (3): every pre-existing cell measures what it
  did."

- [ ] **Step 4: `hash-triejoin.md`**

Add one sentence where the doc describes `open`: "Over a lazy `HashTrie`
(`--ds-layout-expansion lazy`), `open` may build the child it enters; the
algorithm is unchanged and sees the eager trie's keys and chains."

- [ ] **Step 5: `CLAUDE.md`**

- **Priorities item 1.** "for `HashTrie` that is now hasher × pruning,
  i.e. the four aliases `HashTrieSip`, `HashTrieFx`, `HashTrieSipPruned`,
  `HashTrieFxPruned`" becomes "for `HashTrie` that is now hasher × pruning
  × expansion, i.e. the eight aliases `HashTrieSip`, `HashTrieFx`,
  `HashTrieSipPruned`, `HashTrieFxPruned` and each again with a `Lazy`
  suffix".
- **Build Commands.** After the pruning line, add:
  `cargo run -- bench run triangle -i hash-trie -a hash-triejoin --ds-layout-expansion lazy  # Lazy child expansion Layout axis`
- **Workspace architecture.** In the `kermit-ds` line, change "HashTrie
  (hash-based, generic over a HashStrategy)" to "HashTrie (hash-based,
  generic over a HashStrategy, PruningPolicy and ExpansionPolicy)".
- **Key Trait Hierarchy.** After the `SeekStrategy` bullet, add:
  `- **ExpansionPolicy**: HashTrie's third Layout (`kermit-ds/src/ds/hash_trie/expansion.rs`): `EagerExpansion` (default) or `LazyExpansion`, whose children below the root are built on the first `open` that reaches them; axis `ds_layout_expansion`.`
- **Singleton pruning gotcha.** Its sentence "a third *HashTrie* Layout
  dimension means new arms there and nowhere else" becomes "the hasher ×
  pruning × expansion product (8 arms) is expanded there and nowhere else;
  before a fourth dimension, use a nested macro".
- **New gotcha.** Add:
  `- **Lazy HashTrie joins mutate the relation**: under `--ds-layout-expansion lazy`, `HashTrieIter::open` builds each child it first reaches (`OnceCell` through `&self`), so a join changes `heap_size_bytes`. `ExecutionFamily::JOIN_MUTATES` (`E::LAZY` for `HashHtj`) makes `run_benchmark` run `iteration` and `--verify` on fresh builds and check the loaded engine's footprint before `space`; a new mutating family must state `true` there. `result_allocation.rs` measures lazy join cells warm (second join), because a cold one allocates per expanded child.`

- [ ] **Step 6: Spec amendment**

Append to `docs/specs/2026-10-05-hash-trie-lazy-expansion-design.md`:

```markdown
## Amendment 1 (planning, 2026-10-05)

Two refinements found while planning, both verified with a rustc
prototype before any code:

1. **Payload over the node type.** The payload is generic over the node
   type: `type Pending<N>: PendingChild<N>`, with `LazyChild<N>`.
   `E::Pending<P>` would have needed a public trait method returning the
   crate-private `HashTrieNode`, which `private_interfaces` rejects under
   `-Dwarnings`; making the node `pub` only moves the error onto
   `HashTable`. Layout, `Sync` and sizes are as designed, so the
   "compiler risk" fallback was not needed.
2. **Warm lazy allocation cells.** `result_allocation.rs` does not make a
   lazy join warm by itself: each helper call builds fresh relations. Lazy
   cells measure a second join over the same relations (Testing § Join
   layer is corrected by this).
```

- [ ] **Step 7: Verify and commit**

```bash
RUSTDOCFLAGS=-Dwarnings cargo doc --workspace --no-deps 2>&1 | tail -3
git add docs CLAUDE.md
git commit -m "docs: lazy child expansion Layout (#92)

<trailer lines>"
```

---

### Task 9: Measurement record (acceptance)

This is not code. It produces the numbers for `hash-trie.md`'s "Measured
effect" bullet. Follow the perf A/B recipe: run sequentially, never
alongside cargo; use a distinct `--name` per run; and put `--report-json`
and `--name` *before* `run`.

- [ ] **Step 1: Build both binaries**

```bash
SCR=<scratchpad>/bench92; mkdir -p $SCR/base
git archive 35216b6 | tar -x -C $SCR/base
(cd $SCR/base && CARGO_BUILD_JOBS=2 cargo build --release -p kermit 2>&1 | tail -1)
CARGO_BUILD_JOBS=2 cargo build --release -p kermit 2>&1 | tail -1
```

Check `kermit bench list` first. If `oxford-uniform-s3` is not cached,
**ask the user** before fetching or generating it: the shared cache rule
applies. Use a scratch `XDG_CACHE_HOME` if anything must be generated.

- [ ] **Step 2: Eager parity (5 replicates, alternating order)**

For r in 1..5, alternating base-first and new-first:

```bash
$BIN bench --name p92-$ARM-r$r --report-json $SCR/$ARM-r$r.json run oxford-uniform-s3 \
  -i hash-trie -a hash-triejoin -m insertion iteration space
```

Here `$BIN` is `$SCR/base/target/release/kermit` for `ARM=base` and
`./target/release/kermit` for `ARM=eager`. The new binary runs without the
flag, i.e. eager. Do the same with `-i tree-trie -a leapfrog-triejoin` as
the control (`ARM=base-tt` / `new-tt`).

Expected:
- `space` byte-identical between base and eager;
- `insertion` and `iteration` medians within the TreeTrie control's
  spread, given the documented ±3 % codegen bound.

- [ ] **Step 3: The lazy effect**

On the new binary, 5 replicates each, alternating arms:

- `--ds-layout-pruning {off,on}` × `--ds-layout-expansion {eager,lazy}` ×
  `-m insertion iteration space end_to_end`;
- `end_to_end` at `--queries-per-build 1` and `10`.

Analyse with kermit-lab (`kl.load`, `kl.summary`, `bootstrap_ratio_ci`),
reporting medians with ratios and their CIs, never a single Criterion
point estimate.

- [ ] **Step 4: Confound control (local patch, never committed)**

In `run.rs`, change the iteration block's `if F::JOIN_MUTATES {` to
`if true {` (eager now takes the cold path). Build into a separate target
dir (`CARGO_TARGET_DIR=$SCR/cold-eager`), run the eager arm's `iteration`
5×, and restore the line (`git diff` must be empty). The ratio of cold
eager to warm eager iteration bounds the cache-state term inside the
eager-vs-lazy `iteration` gap.

- [ ] **Step 5: Record and commit**

Replace the "Measured effect" placeholder bullet in `hash-trie.md` with:
- the date, commit, benchmark and host note;
- eager parity (space identical; times vs control);
- lazy vs eager ratios for `insertion`, as-built `space`, cold `iteration`
  and `end_to_end` (K = 1, 10), with pruning off and on;
- the confound bound.

```bash
git add docs/data-structures/hash-trie.md
git commit -m "docs(hash-trie): lazy expansion measurement record (#92)

<trailer lines>"
```

Tick the issue's checkboxes only after the user has seen the record.
Posting on the issue is the user's call.

---

## Self-review (done while writing)

- **Spec coverage.**
  - Decisions table: Task 1 covers the representation, laziness, pruning
    interaction and who expands. Task 4 covers `iteration` and `space`.
    Task 3 covers axis, flag, commands and dispatch. `Sync` is pinned in
    Task 1 Step 4. The schema stays at v3 (Task 8 Step 3). kermit-lab is
    Task 6.
  - Testing section: the size pins, construction shape, one-level
    expansion, insert after expansion, heap, policy and axis tests are in
    Task 1. Trace equivalence and the DS suites are Task 2. The join
    layer, the config invocation, allocations and the CLI cells are
    Task 5. The CLI smoke test and the space regression are Task 4.
  - Measurement is Task 9 and Documentation is Task 8.
  - The #81 interaction is not a code task here. It remains a check for
    whichever of #81 / #92 lands second, as the spec says; mention it to
    the user at landing.
- **Placeholders.** One measured-effect bullet is deliberately
  placeholder-shaped in Task 8 Step 1, and Task 9 Step 5 replaces it with
  numbers. Commit messages say `<trailer lines>`; substitute the two
  trailer lines from "Read before starting".
- **Name consistency.** The same names are used throughout: `PendingChild`
  with `from_tuples`, `built`, `built_mut`, `pending`, `push`, `expand`
  and `own_heap_bytes`; `LazyChild<N>`; `ExpansionPolicy::{Pending, LAZY}`;
  `HashTrie::{expand_level, resolve}`; `ExpansionChoice`; `expansion_of`;
  `DsFlag::LayoutExpansion`; `hash_trie_expansion{,_resolved,_explicit}`;
  `DsChoices.expansion`; `Execution::HashHtj.expansion`; and
  `ExecutionFamily::JOIN_MUTATES`.
