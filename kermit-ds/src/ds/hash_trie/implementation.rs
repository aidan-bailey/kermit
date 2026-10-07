//! `HashTrie`: a hash-based trie storing a relation as nested hash tables,
//! one per attribute. Implements `Relation`, `JoinIterable`, `Projectable`,
//! `HeapSize`, and `HashTrieIterable`.
//!
//! `HashTrie` is generic over three Layout dimensions: a
//! [`HashStrategy`](kermit_iters::HashStrategy) `H` (default
//! [`SipHashStrategy`](kermit_iters::SipHashStrategy)), which controls how
//! attribute values are hashed at every level of the trie, a
//! [`PruningPolicy`] `P` (default [`NoPruning`]), which decides whether the
//! `Singleton` node variant exists at all, and an [`ExpansionPolicy`] `E`
//! (default [`EagerExpansion`]), which decides whether children below the
//! root are built at construction or on the first probe that reaches them.
//! They are the layout axes emitted by
//! [`HasOptimizationAxes`](kermit_iters::HasOptimizationAxes) under
//! `ds_layout_hasher`, `ds_layout_pruning` and `ds_layout_expansion`.
//!
//! Runtime values live in [`HashTrieConfig`] (the Config axis, emitted as
//! `ds_config_<value>`); they reach the trie through
//! [`ConfigurableRelation`](crate::relation::ConfigurableRelation).

use {
    super::{
        build_mode::HashTrieBuildMode,
        config::{ChildCapacity, HashTrieConfig, LoadFactor, RootCapacity},
        expansion::{EagerExpansion, ExpansionPolicy, PendingChild},
        node::HashTrieNode,
        parallel,
        pruning::{NoPruning, PruningPolicy, SingletonPayload},
        radix,
    },
    crate::relation::{
        BuildModeRelation, ConfigurableRelation, ConfiguredBuildModeRelation, Relation,
        RelationHeader,
    },
    kermit_iters::{ConfigOption, HashStrategy, JoinIterable, LayoutOption, SipHashStrategy},
    std::marker::PhantomData,
};

/// A hash trie. Each path from root to leaf corresponds to one tuple's
/// hash signature; tuples sharing a complete signature (collisions on
/// every attribute) collect into a chain at the leaf — verification of
/// actual key equality is deferred to the join algorithm.
///
/// # Invariants
///
/// - The depth of every root-to-leaf path equals `header.arity()`.
/// - Inner nodes exist at depths `0..arity-1`; the leaf node at depth
///   `arity-1`.
/// - For arity = 0: undefined behavior (no nullary relations supported).
/// - With `SingletonPruning`, a child node is `Singleton` iff exactly one tuple
///   lives below it (order-independent). Under `NoPruning` no `Singleton` can
///   be constructed — its payload is uninhabited — and the structure is
///   identical to pre-pruning builds.
/// - With `LazyExpansion`, every `Inner` bucket holds a `Singleton` (pruning
///   on, exactly one tuple below it) or an `Unexpanded` child, never a table,
///   and an expanded child's table is the eager table at that position (see
///   `resolve`). Under `EagerExpansion` no `Unexpanded` child can be
///   constructed.
///
/// # Construction
///
/// Use `from_tuples` (batch) or `new` followed by `insert` (incremental).
/// `from_tuples` builds by Algorithm 2 of the paper (`bulk.rs`): a table's
/// tuples are grouped into its buckets, then each bucket's child is built
/// from its list. `insert` places one tuple by `insert_at`, which the
/// `incremental` build mode runs once per tuple; both build the identical
/// trie. `with_config` / `from_tuples_with_config`
/// (via [`ConfigurableRelation`](crate::relation::ConfigurableRelation)) are
/// the config-carrying constructors; `new` / `from_tuples` are thin wrappers
/// over them that supply the default configuration. A known set of tuples
/// can also be built by the `incremental`, `radix:K`, `parallel:N` and
/// `presized:N` BuildModes
/// ([`from_tuples_with_config_and_build_mode`](Self::from_tuples_with_config_and_build_mode),
/// or [`BuildModeRelation`]), which build the identical trie (`presized:N`
/// an equivalent one, Amendment 2).
/// `HashTrieConfig::root_capacity` decides the root's starting size: 4
/// buckets (`grow`, the default), or sized once from the tuples a build is
/// given (`tuples`), so that it never grows during that build (#88).
///
/// # Layout parameters
///
/// `H` is a [`HashStrategy`] selecting which hash function is used to
/// convert attribute values to `u64`. Defaults to
/// [`SipHashStrategy`] for backwards compatibility with pre-standard code.
/// Bench axis `ds_layout_hasher`.
///
/// `P` is a [`PruningPolicy`] selecting whether singleton pruning is part
/// of the representation: [`SingletonPruning`](super::SingletonPruning)
/// stores a one-tuple subtrie as that tuple, [`NoPruning`] (the default)
/// makes the `Singleton` variant uninhabited so the instantiation compiles
/// to the pre-pruning structure. Bench axis `ds_layout_pruning`.
///
/// `E` is an [`ExpansionPolicy`] selecting whether children are built
/// lazily: [`LazyExpansion`](super::LazyExpansion) keeps every child below
/// the root as its tuples until a probe first opens it (SIGMOD 2020
/// Figure 6), while [`EagerExpansion`] (the default) makes the
/// `Unexpanded` variant uninhabited, so the instantiation compiles to the
/// eager structure. Bench axis `ds_layout_expansion`.
pub struct HashTrie<
    H: HashStrategy = SipHashStrategy,
    P: PruningPolicy = NoPruning,
    E: ExpansionPolicy = EagerExpansion,
> {
    header: RelationHeader,
    root: HashTrieNode<P, E>,
    /// Number of stored tuples (multiset: duplicates count); maintained
    /// by `insert` and `from_tuples`.
    tuple_count: usize,
    /// Runtime values, fixed at construction; read by `insert_at`.
    config: HashTrieConfig,
    _layout: PhantomData<(H, P, E)>,
}

impl<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy> HashTrie<H, P, E> {
    /// Whether a node at `depth` is the leaf level, i.e. the last attribute.
    /// Written `depth + 1 == arity` rather than `depth == arity - 1` to avoid
    /// the `usize` underflow at `arity == 0`.
    pub(super) fn is_leaf_depth(depth: usize, arity: usize) -> bool { depth + 1 == arity }

    /// Construct the root node appropriate for `arity` — Inner for arity ≥ 2,
    /// Leaf for arity = 1 — at `2^log2_capacity` buckets. (The root sits at
    /// depth 0, so it is a leaf exactly when `is_leaf_depth(0, arity)`;
    /// `arity <= 1` matches that for every supported arity and additionally
    /// treats the unsupported nullary case as a leaf.)
    pub(super) fn make_root_sized(arity: usize, log2_capacity: u32) -> HashTrieNode<P, E> {
        HashTrieNode::new_table_sized(arity <= 1, log2_capacity)
    }

    /// An empty trie holding `config`, its root sized for a build from
    /// `tuple_count` tuples by [`HashTrieConfig::root_log2_capacity`]: 4
    /// buckets under `RootCapacity::Grow`, and under `Tuples` a capacity at
    /// which `tuple_count` keys never make it grow. The per-tuple build and
    /// [`ConfigurableRelation::with_config`] create their empty root here; the
    /// bulk and partitioned builds size theirs by the same
    /// `root_log2_capacity`. A trie created empty passes 0.
    pub(super) fn with_config_for(
        header: RelationHeader, config: HashTrieConfig, tuple_count: usize,
    ) -> Self {
        let root = Self::make_root_sized(header.arity(), config.root_log2_capacity(tuple_count));
        Self {
            header,
            root,
            // Counts the tuples inserted so far; the builds add to it.
            tuple_count: 0,
            config,
            _layout: PhantomData,
        }
    }

    /// Crate-visible accessor for the root node. Used by `HashTrieIter`
    /// (in the same crate) to navigate the trie via shared references.
    pub(crate) fn root(&self) -> &HashTrieNode<P, E> { &self.root }

    /// Walk the trie depth-first and return every materialized tuple.
    ///
    /// Used by [`crate::relation::Projectable::project`] and tests.
    /// Allocates a fresh `Vec<Vec<usize>>`; for large relations this is
    /// O(n · arity) in both time and space. To visit the tuples without
    /// materialising them, use [`for_each_tuple`](Self::for_each_tuple).
    pub fn collect_tuples(&self) -> Vec<Vec<usize>> {
        let mut out = Vec::new();
        Self::collect_at(&self.root, &mut out);
        out
    }

    /// Walk the trie depth-first, lending every stored tuple to `visit` in
    /// the order [`collect_tuples`](Self::collect_tuples) returns them.
    ///
    /// Each tuple is borrowed from its leaf chain (or pruned `Singleton`)
    /// for that call only, so the walk allocates nothing per tuple: O(n)
    /// time, O(arity) stack. The CLI's `bench ds` `iteration` and
    /// `end_to_end` metrics time this walk (issue #79).
    ///
    /// Under `LazyExpansion` an unexpanded child lends its pending tuples in
    /// insertion order, and the walk expands nothing. A `visit` that opens
    /// an iterator on this same trie and reaches such a child panics
    /// (`BorrowMutError`).
    pub fn for_each_tuple<V: FnMut(&[usize])>(&self, mut visit: V) {
        Self::visit_at(&self.root, &mut visit);
    }

    fn visit_at<V: FnMut(&[usize])>(node: &HashTrieNode<P, E>, visit: &mut V) {
        match node {
            | HashTrieNode::Inner(table) => {
                for (_, child) in table.iter() {
                    Self::visit_at(child, visit);
                }
            },
            | HashTrieNode::Leaf(table) => {
                for (_, chain) in table.iter() {
                    for tuple in chain {
                        visit(tuple);
                    }
                }
            },
            | HashTrieNode::Singleton(payload) => visit(payload.tuple()),
            | HashTrieNode::Unexpanded(pending) => match pending.built() {
                | Some(built) => Self::visit_at(built, visit),
                | None => {
                    for tuple in pending.pending().iter() {
                        visit(tuple);
                    }
                },
            },
        }
    }

    fn collect_at(node: &HashTrieNode<P, E>, out: &mut Vec<Vec<usize>>) {
        match node {
            | HashTrieNode::Inner(table) => {
                for (_, child) in table.iter() {
                    Self::collect_at(child, out);
                }
            },
            | HashTrieNode::Leaf(table) => {
                for (_, chain) in table.iter() {
                    for tuple in chain {
                        out.push(tuple.clone());
                    }
                }
            },
            | HashTrieNode::Singleton(payload) => out.push(payload.tuple().clone()),
            | HashTrieNode::Unexpanded(pending) => match pending.built() {
                | Some(built) => Self::collect_at(built, out),
                | None => out.extend(pending.pending().iter().cloned()),
            },
        }
    }

    /// Insert one tuple at the appropriate depth, descending at once. It is
    /// used by the per-tuple build (`incremental`) and by `Relation::insert`.
    /// The paper's build, Algorithm 2, groups before it recurses (`bulk.rs`);
    /// under every config both builds accept, they build the identical trie.
    /// The singleton-pruning extension of §3.3.1 (Figure 5) applies when the
    /// policy `P` enables it. `P::ENABLED` is a constant, so under
    /// `NoPruning` both pruning branches are compiled out and this is the
    /// pre-pruning insert. An unprune is a single extra O(arity) chain, not
    /// a fan-out: the evicted tuple stops as a new `Singleton` where the two
    /// diverge while only the new tuple keeps descending.
    pub(super) fn insert_at(
        node: &mut HashTrieNode<P, E>, depth: usize, arity: usize, tuple: Vec<usize>,
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
                // The child lives at `depth + 1`; it is the leaf when that is
                // the last attribute.
                let child_is_leaf = Self::is_leaf_depth(depth + 1, arity);
                let child = table.entry_or_insert_with(hash, load_factor, || {
                    HashTrieNode::new_table(child_is_leaf)
                });
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
                            // tuple goes first, as an eager unprune re-inserts
                            // it first, so expansion later builds the eager
                            // table.
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
                if P::ENABLED && matches!(child, HashTrieNode::Singleton(_)) {
                    // Unprune: a second tuple has arrived, so the subtrie no
                    // longer holds exactly one. Swap in the table this level
                    // would have had and re-insert the evicted tuple ahead of
                    // the new one; the recursion re-prunes wherever the two
                    // diverge.
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
            | HashTrieNode::Singleton(_) | HashTrieNode::Unexpanded(_) => {
                unreachable!(
                    "insert_at descends through Inner/Leaf only; singletons are unpruned and \
                     unexpanded children appended to by the parent"
                )
            },
        }
    }

    /// `node` as a probe sees it. An `Unexpanded` child is first built
    /// (once; later calls return the same table), and any other node is
    /// returned as is. `depth` is `node`'s depth.
    ///
    /// The table is built by Algorithm 2 (`bulk.rs`) from the pending list,
    /// which keeps insertion order, so it is the eager table at this position,
    /// bucket for bucket: linear probing places keys by insertion order under
    /// the same load factor. Its own children come out `Unexpanded` (or
    /// `Singleton`), so each expansion builds exactly one level (§3.3.1;
    /// SIGMOD 2020 Figure 6). The trace-equivalence tests in
    /// `kermit-ds/tests/hash_trie_tests.rs` pin this.
    ///
    /// `HashTrieIter::open` is the only caller, so only a probe expands
    /// anything. Every read-only walk (`collect_tuples`, `for_each_tuple`,
    /// `heap_size_bytes`) reads the pending list instead.
    pub(crate) fn resolve<'t>(
        &'t self, node: &'t HashTrieNode<P, E>, depth: usize,
    ) -> &'t HashTrieNode<P, E> {
        match node {
            | HashTrieNode::Unexpanded(pending) => pending.expand(|tuples| {
                Self::build_child_table(depth, self.header.arity(), tuples, self.config)
            }),
            | other => other,
        }
    }
}

impl<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy> JoinIterable for HashTrie<H, P, E> {}

impl<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy> Relation for HashTrie<H, P, E> {
    fn header(&self) -> &RelationHeader { &self.header }

    fn new(header: RelationHeader) -> Self { Self::with_config(header, HashTrieConfig::default()) }

    fn from_tuples(header: RelationHeader, tuples: Vec<Vec<usize>>) -> Self {
        Self::from_tuples_with_config(header, HashTrieConfig::default(), tuples)
    }

    fn insert(&mut self, tuple: Vec<usize>) {
        assert_eq!(
            tuple.len(),
            self.header.arity(),
            "tuple arity {} does not match relation arity {}",
            tuple.len(),
            self.header.arity()
        );
        let arity = self.header.arity();
        Self::insert_at(&mut self.root, 0, arity, tuple, self.config.load_factor);
        self.tuple_count += 1;
    }

    fn insert_all(&mut self, tuples: Vec<Vec<usize>>) {
        for tuple in tuples {
            self.insert(tuple);
        }
    }
}

impl<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy> ConfigurableRelation
    for HashTrie<H, P, E>
{
    type Config = HashTrieConfig;

    fn with_config(header: RelationHeader, config: HashTrieConfig) -> Self {
        Self::with_config_for(header, config, 0)
    }

    fn from_tuples_with_config(
        header: RelationHeader, config: HashTrieConfig, tuples: Vec<Vec<usize>>,
    ) -> Self {
        Self::from_tuples_in_bulk(header, config, tuples)
    }

    fn config(&self) -> &HashTrieConfig { &self.config }
}

#[cfg(test)]
thread_local! {
    /// How many tries this thread has built by Algorithm 2: the one
    /// record that tells `bulk` from `incremental`, which build the
    /// identical trie.
    pub(super) static BULK_BUILDS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

impl<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy> HashTrie<H, P, E> {
    /// Creates a trie holding `config`, populated with `tuples` and built by
    /// `mode` — the one constructor that takes both the Config and the
    /// BuildMode. Every mode builds an equivalent trie (issues #91, #94;
    /// `Presized` may place root keys in other buckets, Amendment 2), so
    /// `mode` changes only how long this takes.
    ///
    /// `Bulk` is [`ConfigurableRelation::from_tuples_with_config`]
    /// (Algorithm 2); `Incremental` is the per-tuple build; `Radix`
    /// partitions first (see `radix.rs`), `Parallel` runs
    /// the radix build's partition and build steps on threads, and
    /// `Presized` groups a presized root by region, then builds its children
    /// in runs (both in `parallel.rs`).
    ///
    /// # Panics
    ///
    /// Panics if any tuple's length does not equal `header.arity()`, if
    /// `mode` is `Presized` and `config.root_capacity` is not
    /// [`RootCapacity::Tuples`], or if `mode` is `Incremental` and
    /// `config.child_capacity` is not [`ChildCapacity::Grow`]: each mode's
    /// prerequisite.
    pub fn from_tuples_with_config_and_build_mode(
        header: RelationHeader, config: HashTrieConfig, mode: HashTrieBuildMode,
        tuples: Vec<Vec<usize>>,
    ) -> Self {
        match mode {
            | HashTrieBuildMode::Bulk => Self::from_tuples_in_bulk(header, config, tuples),
            | HashTrieBuildMode::Incremental => {
                // The prerequisite: `insert_at` creates a child on its first
                // tuple, before the child's list is known, so it cannot size
                // it. The CLI rejects the pair first; here it is a broken
                // invariant, like a wrong arity.
                assert_eq!(
                    config.child_capacity,
                    ChildCapacity::Grow,
                    "hash-trie=incremental requires child-capacity=grow; got child-capacity={}",
                    config.child_capacity.axis_value(),
                );
                Self::from_tuples_incrementally(header, config, tuples)
            },
            | HashTrieBuildMode::Radix(bits) => {
                Self::from_tuples_partitioned(header, config, tuples, |log2, arity, tuples| {
                    let mut root = Self::make_root_sized(arity, log2);
                    radix::fill_root::<H, P, E>(&mut root, arity, tuples, bits, config);
                    root
                })
            },
            | HashTrieBuildMode::Parallel(threads) => {
                Self::from_tuples_partitioned(header, config, tuples, |log2, arity, tuples| {
                    let mut root = Self::make_root_sized(arity, log2);
                    parallel::fill_root::<H, P, E>(&mut root, arity, tuples, threads, config);
                    root
                })
            },
            | HashTrieBuildMode::Presized(threads) => {
                // The prerequisite
                // (docs/specs/2026-10-07-dependent-optimisations-design.md):
                // the regions are cut from a root sized before any tuple
                // arrives. The CLI rejects the pair first; here it is a
                // broken invariant, like a wrong arity.
                assert_eq!(
                    config.root_capacity,
                    RootCapacity::Tuples,
                    "hash-trie=presized:{} requires root-capacity=tuples; got root-capacity={}",
                    threads.get(),
                    config.root_capacity.axis_value(),
                );
                Self::from_tuples_partitioned(header, config, tuples, |log2, arity, tuples| {
                    parallel::fill_presized_root::<H, P, E>(log2, arity, tuples, threads, config)
                })
            },
        }
    }

    /// The per-tuple build's arity check, with its message, for the builds
    /// that read their whole input before building any of it.
    fn assert_arities(arity: usize, tuples: &[Vec<usize>]) {
        for tuple in tuples {
            assert_eq!(
                tuple.len(),
                arity,
                "from_tuples: tuple arity {} does not match header arity {}",
                tuple.len(),
                arity,
            );
        }
    }

    /// The `bulk` build: Algorithm 2 from the root (`bulk.rs`), the root
    /// sized as every build sizes it, by
    /// [`HashTrieConfig::root_log2_capacity`] (#88).
    ///
    /// # Panics
    ///
    /// Panics if any tuple's length does not equal `header.arity()`.
    pub(super) fn from_tuples_in_bulk(
        header: RelationHeader, config: HashTrieConfig, tuples: Vec<Vec<usize>>,
    ) -> Self {
        #[cfg(test)]
        BULK_BUILDS.with(|n| n.set(n.get() + 1));
        let arity = header.arity();
        Self::assert_arities(arity, &tuples);
        let tuple_count = tuples.len();
        let root = Self::build(
            0,
            arity,
            tuples,
            config.root_log2_capacity(tuple_count),
            config,
        );
        Self {
            header,
            root,
            tuple_count,
            config,
            _layout: PhantomData,
        }
    }

    /// The `incremental` build: one [`insert_at`](Self::insert_at) per
    /// tuple, in input order. This is the build `from_tuples_with_config`
    /// ran before #107, unchanged.
    ///
    /// # Panics
    ///
    /// Panics if any tuple's length does not equal `header.arity()`.
    pub(super) fn from_tuples_incrementally(
        header: RelationHeader, config: HashTrieConfig, tuples: Vec<Vec<usize>>,
    ) -> Self {
        let arity = header.arity();
        let mut trie = Self::with_config_for(header, config, tuples.len());
        for tuple in tuples {
            assert_eq!(
                tuple.len(),
                arity,
                "from_tuples: tuple arity {} does not match header arity {}",
                tuple.len(),
                arity,
            );
            Self::insert_at(&mut trie.root, 0, arity, tuple, config.load_factor);
            // from_tuples bypasses insert(), so count here. If this loop is
            // ever refactored to route through insert(), drop this increment
            // or the counter double-counts.
            trie.tuple_count += 1;
        }
        trie
    }

    /// What the partitioned builds share (`radix:K`, `parallel:N`,
    /// `presized:N`): the per-tuple build's arity check, with its message,
    /// then `fill`, which builds the root at the capacity every build gives it
    /// (`HashTrieConfig::root_log2_capacity`, presized under
    /// `root-capacity=tuples`), then the multiset count the other builds keep.
    fn from_tuples_partitioned(
        header: RelationHeader, config: HashTrieConfig, tuples: Vec<Vec<usize>>,
        fill: impl FnOnce(u32, usize, Vec<Vec<usize>>) -> HashTrieNode<P, E>,
    ) -> Self {
        let arity = header.arity();
        Self::assert_arities(arity, &tuples);
        let tuple_count = tuples.len();
        let root = fill(config.root_log2_capacity(tuple_count), arity, tuples);
        Self {
            header,
            root,
            tuple_count,
            config,
            _layout: PhantomData,
        }
    }
}

impl<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy> BuildModeRelation
    for HashTrie<H, P, E>
{
    type BuildMode = HashTrieBuildMode;

    /// Builds with the default config; see
    /// [`from_tuples_with_config_and_build_mode`](HashTrie::from_tuples_with_config_and_build_mode).
    ///
    /// # Panics
    ///
    /// Panics if any tuple's length does not equal `header.arity()`, and for
    /// [`HashTrieBuildMode::Presized`], which requires `root-capacity=tuples`:
    /// this seam builds with the default config (`root-capacity=grow`), so
    /// use [`HashTrie::from_tuples_with_config_and_build_mode`] (or
    /// `BuiltWith<Configured<…>, …>`) instead.
    fn from_tuples_with_build_mode(
        header: RelationHeader, mode: HashTrieBuildMode, tuples: Vec<Vec<usize>>,
    ) -> Self {
        Self::from_tuples_with_config_and_build_mode(
            header,
            HashTrieConfig::default(),
            mode,
            tuples,
        )
    }
}

/// The inherent
/// [`from_tuples_with_config_and_build_mode`](HashTrie::from_tuples_with_config_and_build_mode),
/// so `BuiltWith` can stack on `Configured` in the test suites.
impl<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy> ConfiguredBuildModeRelation
    for HashTrie<H, P, E>
{
    fn from_tuples_with_config_and_build_mode(
        header: RelationHeader, config: HashTrieConfig, mode: HashTrieBuildMode,
        tuples: Vec<Vec<usize>>,
    ) -> Self {
        HashTrie::<H, P, E>::from_tuples_with_config_and_build_mode(header, config, mode, tuples)
    }
}

impl<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy> crate::relation::Projectable
    for HashTrie<H, P, E>
{
    fn project(&self, columns: Vec<usize>) -> Self {
        let arity = self.header.arity();
        for &c in &columns {
            assert!(
                c < arity,
                "project: column index {c} out of range for arity {arity}"
            );
        }
        // Build the projected header. Match the convention used by
        // project_via_trie_iter in `kermit-ds/src/relation.rs`.
        let projected_attrs: Vec<String> = columns
            .iter()
            .filter_map(|&c| self.header.attrs().get(c).cloned())
            .collect();
        let new_header = if projected_attrs.is_empty() {
            RelationHeader::new_nameless_positional(columns.len())
        } else {
            RelationHeader::new_nameless(projected_attrs)
        };
        let projected_tuples: Vec<Vec<usize>> = self
            .collect_tuples()
            .into_iter()
            .map(|tuple| columns.iter().map(|&c| tuple[c]).collect())
            .collect();
        HashTrie::<H, P, E>::from_tuples_with_config(new_header, self.config, projected_tuples)
    }
}

impl<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy> crate::heap_size::HeapSize
    for HashTrie<H, P, E>
{
    fn heap_size_bytes(&self) -> usize { node_heap_bytes(&self.root) }
}

/// The trie's own walk, [`HashTrie::for_each_tuple`], which reads unexpanded
/// children's pending tuples instead of building them.
impl<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy> crate::tuple_scan::TupleScan
    for HashTrie<H, P, E>
{
    fn scan_tuples(&self, visit: impl FnMut(&[usize])) { self.for_each_tuple(visit) }
}

impl<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy> crate::cardinality::Cardinality
    for HashTrie<H, P, E>
{
    fn tuple_count(&self) -> usize { self.tuple_count }
}

impl<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy> kermit_iters::HashTrieIterable
    for HashTrie<H, P, E>
{
    fn hash_trie_iter(&self) -> impl kermit_iters::HashTrieIterator {
        super::hash_trie_iter::HashTrieIter::<H, P, E>::new(self)
    }
}

impl<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy> kermit_iters::HasOptimizationAxes
    for HashTrie<H, P, E>
{
    /// The layout axes `ds_layout_hasher` (the strategy's
    /// `LayoutOption::NAME`, e.g. `"sip"` / `"fxhash"`) and
    /// `ds_layout_pruning` (`"off"` / `"on"`) and `ds_layout_expansion`
    /// (`"eager"` / `"lazy"`), plus one
    /// `ds_config_<value>` axis per [`HashTrieConfig`] value.
    fn optimization_axes(&self) -> std::collections::BTreeMap<String, serde_json::Value> {
        let mut axes = std::collections::BTreeMap::new();
        axes.insert(
            "ds_layout_hasher".to_string(),
            serde_json::Value::String(<H as LayoutOption>::NAME.to_string()),
        );
        axes.insert(
            "ds_layout_pruning".to_string(),
            serde_json::Value::String(<P as LayoutOption>::NAME.to_string()),
        );
        axes.insert(
            "ds_layout_expansion".to_string(),
            serde_json::Value::String(<E as LayoutOption>::NAME.to_string()),
        );
        for (suffix, value) in self.config.axes() {
            axes.insert(format!("ds_config_{suffix}"), value);
        }
        axes
    }
}

fn node_heap_bytes<P: PruningPolicy, E: ExpansionPolicy>(node: &HashTrieNode<P, E>) -> usize {
    match node {
        | HashTrieNode::Inner(table) => {
            let shell = table.shell_heap_bytes();
            let children: usize = table.iter().map(|(_, child)| node_heap_bytes(child)).sum();
            shell + children
        },
        | HashTrieNode::Leaf(table) => {
            let shell = table.shell_heap_bytes();
            let chains: usize = table
                .iter()
                .map(|(_, chain)| tuple_list_heap_bytes(chain))
                .sum();
            shell + chains
        },
        | HashTrieNode::Singleton(payload) => {
            payload.tuple().capacity() * std::mem::size_of::<usize>()
        },
        | HashTrieNode::Unexpanded(pending) => {
            let below = match pending.built() {
                | Some(built) => node_heap_bytes(built),
                | None => tuple_list_heap_bytes(&pending.pending()),
            };
            pending.own_heap_bytes() + below
        },
    }
}

/// Heap bytes of a list of tuples: the list's buffer plus each tuple's.
// `&Vec`, not a slice: the list's own buffer is counted by `capacity()`.
#[allow(clippy::ptr_arg)]
fn tuple_list_heap_bytes(list: &Vec<Vec<usize>>) -> usize {
    list.capacity() * std::mem::size_of::<Vec<usize>>()
        + list
            .iter()
            .map(|t| t.capacity() * std::mem::size_of::<usize>())
            .sum::<usize>()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Test let-bindings use `HashTrie` (the bare type), which resolves
    // through the `<H = SipHashStrategy>` default in type position. The
    // default cannot be selected from a path expression alone, so
    // type-annotated bindings are necessary on nightly Rust.

    /// `bulk` and `incremental` build the identical trie, so only this
    /// record shows which ran: the default, `from_tuples_with_config` and
    /// `Bulk` take Algorithm 2, and `Incremental` the per-tuple build.
    #[test]
    fn each_build_mode_runs_its_own_build() {
        let tuples = || vec![vec![1, 2], vec![1, 3], vec![2, 2]];
        let builds = |build: &dyn Fn()| {
            BULK_BUILDS.with(|n| n.set(0));
            build();
            BULK_BUILDS.with(std::cell::Cell::get)
        };
        assert_eq!(
            builds(&|| {
                let _: HashTrie = HashTrie::from_tuples(2.into(), tuples());
            }),
            1,
            "from_tuples"
        );
        assert_eq!(
            builds(&|| {
                let _: HashTrie = HashTrie::from_tuples_with_config(
                    2.into(),
                    HashTrieConfig::default(),
                    tuples(),
                );
            }),
            1,
            "from_tuples_with_config"
        );
        assert_eq!(
            builds(&|| {
                let _: HashTrie = HashTrie::from_tuples_with_build_mode(
                    2.into(),
                    HashTrieBuildMode::Bulk,
                    tuples(),
                );
            }),
            1,
            "bulk"
        );
        assert_eq!(
            builds(&|| {
                let _: HashTrie = HashTrie::from_tuples_with_build_mode(
                    2.into(),
                    HashTrieBuildMode::Incremental,
                    tuples(),
                );
            }),
            0,
            "incremental"
        );
    }

    #[test]
    fn new_arity_2_creates_inner_root() {
        let trie: HashTrie = HashTrie::new(2.into());
        assert_eq!(trie.header().arity(), 2);
        assert!(matches!(trie.root, HashTrieNode::Inner(_)));
    }

    #[test]
    fn new_arity_1_creates_leaf_root() {
        let trie: HashTrie = HashTrie::new(1.into());
        assert_eq!(trie.header().arity(), 1);
        assert!(matches!(trie.root, HashTrieNode::Leaf(_)));
    }

    #[test]
    fn insert_arity_1_populates_leaf() {
        let mut trie: HashTrie = HashTrie::new(1.into());
        trie.insert(vec![42]);
        // Verify by inspecting the root: should be a Leaf with one entry.
        match &trie.root {
            | HashTrieNode::Leaf(table) => assert_eq!(table.len(), 1),
            | _ => panic!("expected Leaf root"),
        }
    }

    #[test]
    fn insert_arity_2_builds_inner_then_leaf() {
        let mut trie: HashTrie = HashTrie::new(2.into());
        trie.insert(vec![1, 2]);
        match &trie.root {
            | HashTrieNode::Inner(root_table) => {
                assert_eq!(root_table.len(), 1);
                // Walk one level deeper and confirm it's a Leaf.
                let mut found_leaf = false;
                for (_, child) in root_table.iter() {
                    assert!(matches!(child, HashTrieNode::Leaf(_)));
                    if let HashTrieNode::Leaf(leaf_table) = child {
                        assert_eq!(leaf_table.len(), 1);
                        found_leaf = true;
                    }
                }
                assert!(found_leaf);
            },
            | _ => panic!("expected Inner root"),
        }
    }

    #[test]
    fn insert_two_tuples_sharing_first_attribute() {
        let mut trie: HashTrie = HashTrie::new(2.into());
        trie.insert(vec![1, 2]);
        trie.insert(vec![1, 3]);
        // Same attr-0 value => same hash at root => same child node; child has
        // two entries.
        match &trie.root {
            | HashTrieNode::Inner(root_table) => {
                assert_eq!(root_table.len(), 1);
                for (_, child) in root_table.iter() {
                    if let HashTrieNode::Leaf(leaf_table) = child {
                        assert_eq!(leaf_table.len(), 2);
                    }
                }
            },
            | _ => panic!("expected Inner root"),
        }
    }

    #[test]
    #[should_panic(expected = "tuple arity")]
    fn insert_wrong_arity_panics() {
        let mut trie: HashTrie = HashTrie::new(2.into());
        trie.insert(vec![1]);
    }

    #[test]
    fn from_tuples_arity_2_builds_correct_shape() {
        let trie: HashTrie =
            HashTrie::from_tuples(2.into(), vec![vec![1, 2], vec![1, 3], vec![2, 4]]);
        assert_eq!(trie.header().arity(), 2);
        // Same attr-0 inserts share a child; two distinct attr-0 values =>
        // two root-level entries.
        match &trie.root {
            | HashTrieNode::Inner(root_table) => assert_eq!(root_table.len(), 2),
            | _ => panic!("expected Inner root"),
        }
    }

    #[test]
    fn from_tuples_empty_input() {
        let trie: HashTrie = HashTrie::from_tuples(2.into(), vec![]);
        match &trie.root {
            | HashTrieNode::Inner(root_table) => assert_eq!(root_table.len(), 0),
            | _ => panic!("expected Inner root"),
        }
    }

    #[test]
    fn insert_all_equivalent_to_from_tuples_for_multiset_view() {
        let mut a: HashTrie = HashTrie::new(2.into());
        a.insert_all(vec![vec![1, 2], vec![1, 3], vec![2, 4]]);
        let b: HashTrie = HashTrie::from_tuples(2.into(), vec![vec![1, 2], vec![1, 3], vec![2, 4]]);
        // Compare via heap_size and tuple set (via Projectable when ready) —
        // for now confirm both have populated roots with the same length.
        let a_root_len = match &a.root {
            | HashTrieNode::Inner(t) => t.len(),
            | _ => unreachable!(),
        };
        let b_root_len = match &b.root {
            | HashTrieNode::Inner(t) => t.len(),
            | _ => unreachable!(),
        };
        assert_eq!(a_root_len, b_root_len);
    }

    #[test]
    fn collect_tuples_recovers_input_as_multiset() {
        let mut trie: HashTrie =
            HashTrie::from_tuples(2.into(), vec![vec![1, 2], vec![1, 3], vec![2, 4]]);
        let mut collected = trie.collect_tuples();
        collected.sort();
        assert_eq!(collected, vec![vec![1, 2], vec![1, 3], vec![2, 4]]);

        // Adding a tuple with full collision-equal hash signature: same value
        // at each attribute => same hash path => stored on the same leaf chain.
        trie.insert(vec![1, 2]);
        let mut collected = trie.collect_tuples();
        collected.sort();
        assert_eq!(collected, vec![vec![1, 2], vec![1, 2], vec![1, 3], vec![
            2, 4
        ]]);
    }

    #[test]
    fn collect_tuples_empty_trie() {
        let trie: HashTrie = HashTrie::new(2.into());
        assert!(trie.collect_tuples().is_empty());
    }

    #[test]
    fn heap_size_zero_for_empty_trie() {
        use crate::HeapSize;
        let trie: HashTrie = HashTrie::new(2.into());
        // Even an empty trie allocates initial 4-bucket tables, so heap size
        // is non-zero — what we check is determinism and ordering.
        let small = trie.heap_size_bytes();
        let big_trie: HashTrie =
            HashTrie::from_tuples(2.into(), vec![vec![1, 2], vec![1, 3], vec![2, 4], vec![
                3, 5,
            ]]);
        let big = big_trie.heap_size_bytes();
        assert!(big > small, "non-empty trie should be heavier than empty");
    }

    #[test]
    fn heap_size_deterministic_across_rebuilds() {
        use crate::HeapSize;
        let tuples = vec![vec![1, 2], vec![1, 3], vec![2, 4], vec![3, 5]];
        let trie_a: HashTrie = HashTrie::from_tuples(2.into(), tuples.clone());
        let trie_b: HashTrie = HashTrie::from_tuples(2.into(), tuples);
        assert_eq!(trie_a.heap_size_bytes(), trie_b.heap_size_bytes());
    }

    #[test]
    fn project_drops_columns() {
        use crate::relation::Projectable;
        let trie: HashTrie =
            HashTrie::from_tuples(2.into(), vec![vec![1, 2], vec![1, 3], vec![2, 4]]);
        // π_0 (first column only)
        let projected = trie.project(vec![0]);
        assert_eq!(projected.header().arity(), 1);
        let mut collected = projected.collect_tuples();
        collected.sort();
        // Duplicate `1`s collapse only if from_tuples deduplicates — HashTrie
        // is a multiset, so we expect duplicates to survive.
        assert_eq!(collected, vec![vec![1], vec![1], vec![2]]);
    }

    #[test]
    fn project_reorders_columns() {
        use crate::relation::Projectable;
        let trie: HashTrie = HashTrie::from_tuples(2.into(), vec![vec![1, 2], vec![3, 4]]);
        let projected = trie.project(vec![1, 0]);
        let mut collected = projected.collect_tuples();
        collected.sort();
        assert_eq!(collected, vec![vec![2, 1], vec![4, 3]]);
    }

    #[test]
    fn hash_trie_iter_returns_navigable_iterator() {
        use kermit_iters::{HashTrieIterable, HashTrieIterator};
        let trie: HashTrie = HashTrie::from_tuples(1.into(), vec![vec![1], vec![2], vec![3]]);
        let mut it = trie.hash_trie_iter();
        assert!(it.open());
        let mut seen = std::collections::HashSet::new();
        while !it.at_end() {
            seen.insert(it.key().unwrap());
            it.next();
        }
        assert_eq!(seen.len(), 3);
    }

    #[test]
    fn optimization_axes_default_strategy_reports_sip() {
        use kermit_iters::HasOptimizationAxes;
        let trie: HashTrie = HashTrie::new(2.into());
        let axes = trie.optimization_axes();
        let mut keys: Vec<&str> = axes.keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(keys, vec![
            "ds_config_child_capacity",
            "ds_config_load_factor",
            "ds_config_root_capacity",
            "ds_layout_expansion",
            "ds_layout_hasher",
            "ds_layout_pruning"
        ]);
        assert_eq!(
            axes.get("ds_layout_hasher"),
            Some(&serde_json::Value::String("sip".to_string())),
        );
    }

    #[test]
    fn optimization_axes_fxhash_strategy_reports_fxhash() {
        use kermit_iters::{FxHashStrategy, HasOptimizationAxes};
        let trie: HashTrie<FxHashStrategy> = HashTrie::new(2.into());
        let axes = trie.optimization_axes();
        let mut keys: Vec<&str> = axes.keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(keys, vec![
            "ds_config_child_capacity",
            "ds_config_load_factor",
            "ds_config_root_capacity",
            "ds_layout_expansion",
            "ds_layout_hasher",
            "ds_layout_pruning"
        ]);
        assert_eq!(
            axes.get("ds_layout_hasher"),
            Some(&serde_json::Value::String("fxhash".to_string())),
        );
    }

    #[test]
    fn with_config_stores_config_and_new_uses_default() {
        use crate::relation::ConfigurableRelation;
        let dense = HashTrieConfig {
            load_factor: LoadFactor::percent(50).unwrap(),
            ..HashTrieConfig::default()
        };
        let trie: HashTrie = HashTrie::with_config(2.into(), dense);
        assert_eq!(*trie.config(), dense);
        let plain: HashTrie = HashTrie::new(2.into());
        assert_eq!(*plain.config(), HashTrieConfig::default());
    }

    #[test]
    fn project_preserves_config() {
        use crate::relation::{ConfigurableRelation, Projectable};
        let dense = HashTrieConfig {
            load_factor: LoadFactor::percent(50).unwrap(),
            ..HashTrieConfig::default()
        };
        let trie: HashTrie =
            HashTrie::from_tuples_with_config(2.into(), dense, vec![vec![1, 2], vec![3, 4]]);
        let projected = trie.project(vec![1]);
        assert_eq!(*projected.config(), dense);
        let mut got = projected.collect_tuples();
        got.sort();
        assert_eq!(got, vec![vec![2], vec![4]]);
    }

    #[test]
    fn optimization_axes_include_config_value() {
        use {crate::relation::ConfigurableRelation, kermit_iters::HasOptimizationAxes};
        let dense = HashTrieConfig {
            load_factor: LoadFactor::percent(50).unwrap(),
            ..HashTrieConfig::default()
        };
        let trie: HashTrie = HashTrie::with_config(2.into(), dense);
        let axes = trie.optimization_axes();
        assert_eq!(
            axes.get("ds_config_load_factor"),
            Some(&serde_json::Value::from(0.5_f64))
        );
        assert_eq!(
            axes.get("ds_layout_hasher"),
            Some(&serde_json::Value::String("sip".into()))
        );
        let plain: HashTrie = HashTrie::new(2.into());
        assert_eq!(
            plain.optimization_axes().get("ds_config_load_factor"),
            Some(&serde_json::Value::from(0.7_f64))
        );
    }

    #[test]
    fn from_tuples_uses_default_config() {
        use crate::relation::ConfigurableRelation;
        let trie: HashTrie = HashTrie::from_tuples(2.into(), vec![vec![1, 2]]);
        assert_eq!(*trie.config(), HashTrieConfig::default());
    }

    #[test]
    fn config_survives_insert_and_insert_all() {
        use crate::relation::ConfigurableRelation;
        let dense = HashTrieConfig {
            load_factor: LoadFactor::percent(50).unwrap(),
            ..HashTrieConfig::default()
        };
        let mut trie: HashTrie = HashTrie::with_config(2.into(), dense);
        trie.insert(vec![1, 2]);
        assert_eq!(*trie.config(), dense);
        trie.insert_all(vec![vec![3, 4], vec![5, 6]]);
        assert_eq!(*trie.config(), dense);
    }

    #[test]
    #[should_panic(expected = "from_tuples: tuple arity")]
    fn from_tuples_with_config_wrong_arity_panics() {
        use crate::relation::ConfigurableRelation;
        let _: HashTrie =
            HashTrie::from_tuples_with_config(2.into(), HashTrieConfig::default(), vec![vec![1]]);
    }

    // ── Singleton pruning ──────────────────────────────────────────────

    use super::super::pruning::{NoPruning, SingletonPruning};

    type Pruned = HashTrie<SipHashStrategy, SingletonPruning>;

    fn pruned(arity: usize, tuples: Vec<Vec<usize>>) -> Pruned {
        Pruned::from_tuples(arity.into(), tuples)
    }

    /// Walks `root`, asserting the pruning invariant at every inner bucket
    /// and returning the number of tuples stored below it.
    ///
    /// Invariant: under `SingletonPruning` a child is `Singleton` iff
    /// exactly one tuple lives below it; under `NoPruning` no `Singleton`
    /// exists. The root is never a `Singleton`, and a `Singleton` sits
    /// under the buckets its own tuple hashes to.
    fn check_pruning_invariant<P: PruningPolicy, E: ExpansionPolicy>(
        root: &HashTrieNode<P, E>,
    ) -> usize {
        assert!(
            !matches!(root, HashTrieNode::Singleton(_)),
            "the root is never a Singleton"
        );
        check_pruning_invariant_at(root, &mut Vec::new())
    }

    /// Recursive half of [`check_pruning_invariant`]. `prefix` is the
    /// sequence of bucket hashes taken from the root down to `node`, so a
    /// `Singleton` reached here must hash to every one of them — that is
    /// what pins it to the right *place*, not merely the right count.
    fn check_pruning_invariant_at<P: PruningPolicy, E: ExpansionPolicy>(
        node: &HashTrieNode<P, E>, prefix: &mut Vec<u64>,
    ) -> usize {
        match node {
            | HashTrieNode::Singleton(payload) => {
                assert!(P::ENABLED, "Singleton found with pruning off");
                let tuple = payload.tuple();
                for (d, &expected) in prefix.iter().enumerate() {
                    assert_eq!(
                        <SipHashStrategy as HashStrategy>::hash(tuple[d]),
                        expected,
                        "Singleton {tuple:?} is misplaced at depth {d}"
                    );
                }
                1
            },
            | HashTrieNode::Leaf(table) => table.iter().map(|(_, chain)| chain.len()).sum(),
            | HashTrieNode::Unexpanded(_) => {
                unreachable!("the pruning invariant is checked on eager tries")
            },
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

    #[test]
    fn pruning_off_never_creates_singletons() {
        let trie: HashTrie =
            HashTrie::from_tuples(3.into(), vec![vec![1, 2, 3], vec![1, 2, 4], vec![5, 6, 7]]);
        assert_eq!(check_pruning_invariant(&trie.root), 3);
    }

    #[test]
    fn pruning_on_single_tuple_subtries_are_singletons() {
        let trie = pruned(3, vec![vec![1, 2, 3], vec![5, 6, 7]]);
        assert_eq!(check_pruning_invariant(&trie.root), 2);
        match &trie.root {
            | HashTrieNode::Inner(t) => {
                assert_eq!(t.len(), 2);
                for (_, child) in t.iter() {
                    assert!(matches!(child, HashTrieNode::Singleton(_)));
                }
            },
            | _ => panic!("expected Inner root"),
        }
    }

    #[test]
    fn pruning_arity_1_has_no_singletons() {
        // The root is the only node an arity-1 trie has, and the root is
        // never pruned — tuples land straight in leaf chains.
        let trie = pruned(1, vec![vec![1], vec![2]]);
        assert_eq!(check_pruning_invariant(&trie.root), 2);
        assert!(matches!(trie.root, HashTrieNode::Leaf(_)));
        let mut got = trie.collect_tuples();
        got.sort();
        assert_eq!(got, vec![vec![1], vec![2]]);
    }

    #[test]
    fn unprune_when_second_tuple_diverges_one_level_down() {
        let trie = pruned(3, vec![vec![1, 2, 3], vec![1, 4, 5]]);
        assert_eq!(check_pruning_invariant(&trie.root), 2);
        let mut got = trie.collect_tuples();
        got.sort();
        assert_eq!(got, vec![vec![1, 2, 3], vec![1, 4, 5]]);
    }

    #[test]
    fn unprune_when_second_tuple_shares_hashes_to_the_leaf() {
        let trie = pruned(3, vec![vec![1, 2, 3], vec![1, 2, 4]]);
        assert_eq!(check_pruning_invariant(&trie.root), 2);
        let mut got = trie.collect_tuples();
        got.sort();
        assert_eq!(got, vec![vec![1, 2, 3], vec![1, 2, 4]]);
    }

    #[test]
    fn unprune_on_exact_duplicate_keeps_multiset() {
        let trie = pruned(2, vec![vec![1, 2], vec![1, 2]]);
        assert_eq!(check_pruning_invariant(&trie.root), 2);
        let mut got = trie.collect_tuples();
        got.sort();
        assert_eq!(got, vec![vec![1, 2], vec![1, 2]]);
    }

    #[test]
    fn incremental_insert_unprunes_like_bulk_build() {
        let mut trie = Pruned::new(3.into());
        trie.insert(vec![1, 2, 3]);
        assert_eq!(check_pruning_invariant(&trie.root), 1);
        trie.insert(vec![1, 2, 4]);
        assert_eq!(check_pruning_invariant(&trie.root), 2);
        trie.insert(vec![9, 9, 9]);
        assert_eq!(check_pruning_invariant(&trie.root), 3);
        assert_eq!(trie.tuple_count, 3);
    }

    #[test]
    fn pruned_build_is_insertion_order_independent() {
        use crate::heap_size::HeapSize;
        let tuples = vec![vec![1, 2, 3], vec![1, 2, 4], vec![1, 5, 6], vec![7, 8, 9]];
        let forward = pruned(3, tuples.clone());
        let backward = pruned(3, tuples.into_iter().rev().collect());
        assert_eq!(forward.heap_size_bytes(), backward.heap_size_bytes());
        let (mut a, mut b) = (forward.collect_tuples(), backward.collect_tuples());
        a.sort();
        b.sort();
        assert_eq!(a, b);
    }

    #[test]
    fn pruning_shrinks_heap_size_for_sparse_fanout() {
        use crate::heap_size::HeapSize;
        let tuples: Vec<Vec<usize>> = (0..64).map(|i| vec![i, i + 1000, i + 2000]).collect();
        let plain: HashTrie = HashTrie::from_tuples(3.into(), tuples.clone());
        let compact = pruned(3, tuples);
        assert!(
            compact.heap_size_bytes() < plain.heap_size_bytes(),
            "pruned {} >= plain {}",
            compact.heap_size_bytes(),
            plain.heap_size_bytes()
        );
    }

    /// A growth guard, not the elision witness: the `Vec` payload is
    /// smaller than either table variant, so the pruned node was never
    /// going to be the larger of the two. What pins the claim that
    /// `NoPruning` costs nothing is `off_frame_is_the_bare_table_pair` in
    /// `hash_trie_iter.rs`.
    #[test]
    fn node_does_not_grow_under_the_pruning_policy() {
        use super::super::expansion::EagerExpansion;
        assert_eq!(
            std::mem::size_of::<HashTrieNode<NoPruning, EagerExpansion>>(),
            std::mem::size_of::<HashTrieNode<SingletonPruning, EagerExpansion>>()
        );
    }

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

    /// Every Layout's nodes are `Send`, so the `parallel:N` build can hand a
    /// worker's finished scratch root to the calling thread (#94). Lazy
    /// tries stay `!Sync` (their cells) but are `Send`. Generic, so it pins
    /// the policies' bounds rather than today's two policies of each kind.
    #[test]
    fn nodes_are_send_under_every_layout() {
        use super::super::{expansion::LazyExpansion, pruning::SingletonPruning};
        fn send<T: Send>() {}
        fn node_is_send<P: PruningPolicy, E: ExpansionPolicy>() { send::<HashTrieNode<P, E>>(); }
        node_is_send::<NoPruning, EagerExpansion>();
        node_is_send::<SingletonPruning, LazyExpansion>();
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
        assert!(!on
            .optimization_axes()
            .contains_key("ds_config_singleton_pruning"));
    }

    #[test]
    fn pruned_and_plain_collect_the_same_tuples() {
        let tuples = vec![vec![1, 2, 3], vec![1, 2, 4], vec![1, 5, 6], vec![7, 8, 9]];
        let plain: HashTrie = HashTrie::from_tuples(3.into(), tuples.clone());
        let compact = pruned(3, tuples);
        let (mut a, mut b) = (plain.collect_tuples(), compact.collect_tuples());
        a.sort();
        b.sort();
        assert_eq!(a, b);
    }

    /// `for_each_tuple` is the borrowed form of `collect_tuples` (issue
    /// #79): it must lend exactly the tuples `collect_tuples` returns, in the
    /// same order — a duplicate in one leaf chain, and (pruned) a
    /// `Singleton` subtrie, included.
    #[test]
    fn for_each_tuple_visits_what_collect_tuples_returns() {
        let tuples = vec![
            vec![1, 2, 3],
            vec![1, 2, 3],
            vec![1, 2, 4],
            vec![1, 5, 6],
            vec![7, 8, 9],
        ];
        let plain: HashTrie = HashTrie::from_tuples(3.into(), tuples.clone());
        let compact = pruned(3, tuples);
        for (name, collected, mut visited) in [
            ("NoPruning", plain.collect_tuples(), Vec::new()),
            ("SingletonPruning", compact.collect_tuples(), Vec::new()),
        ] {
            if name == "NoPruning" {
                plain.for_each_tuple(|t| visited.push(t.to_vec()));
            } else {
                compact.for_each_tuple(|t| visited.push(t.to_vec()));
            }
            assert_eq!(visited, collected, "{name}");
            assert_eq!(visited.len(), 5, "{name}");
        }
    }
}

#[cfg(test)]
mod cardinality_tests {
    use {super::*, crate::cardinality::Cardinality};

    #[test]
    fn empty_relation_has_zero_tuples() {
        let trie: HashTrie = HashTrie::new(2.into());
        assert_eq!(trie.tuple_count(), 0);
    }

    #[test]
    fn tuple_count_matches_collect_tuples_len() {
        let trie: HashTrie =
            HashTrie::from_tuples(2.into(), vec![vec![1, 2], vec![1, 3], vec![2, 4]]);
        assert_eq!(trie.tuple_count(), 3);
        assert_eq!(trie.collect_tuples().len(), 3);
    }

    #[test]
    fn duplicate_insert_counts_multiset_semantics() {
        // HashTrie is deliberately a multiset: duplicates append to leaf
        // chains (equality is deferred to the join algorithm), so the
        // count includes them — matching what iteration yields.
        let mut trie: HashTrie = HashTrie::from_tuples(2.into(), vec![vec![1, 2]]);
        trie.insert(vec![1, 2]);
        assert_eq!(trie.tuple_count(), 2);
        assert_eq!(trie.collect_tuples().len(), 2);
    }

    #[test]
    fn from_tuples_with_duplicates_counts_multiset_semantics() {
        // Same multiset guarantee, but pinned on the batch-construction
        // path: from_tuples maintains the counter itself (it bypasses
        // insert()), so duplicates must count there too.
        let trie: HashTrie = HashTrie::from_tuples(2.into(), vec![vec![1, 2], vec![1, 2]]);
        assert_eq!(trie.tuple_count(), 2);
        assert_eq!(trie.collect_tuples().len(), 2);
    }
}

#[cfg(test)]
mod lazy_tests {
    use {
        super::{
            super::{
                expansion::{LazyChild, LazyExpansion, PendingChild},
                pruning::SingletonPruning,
            },
            *,
        },
        crate::{cardinality::Cardinality, heap_size::HeapSize, relation::Projectable},
        kermit_iters::HasOptimizationAxes,
    };

    type Lazy = HashTrie<SipHashStrategy, NoPruning, LazyExpansion>;
    type LazyPruned = HashTrie<SipHashStrategy, SingletonPruning, LazyExpansion>;

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
    fn pending_of<P: PruningPolicy>(
        node: &HashTrieNode<P, LazyExpansion>,
    ) -> Option<Vec<Vec<usize>>> {
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
        assert_eq!(
            pending_of(child(&trie, 1)),
            Some(vec![vec![1, 2], vec![1, 3]])
        );
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
            assert!(
                pending_of(grandchild).is_some(),
                "grandchildren stay unexpanded"
            );
        }
        assert!(pending_of(node).is_none(), "the child is expanded now");
        assert!(
            std::ptr::eq(trie.resolve(node, 1), built),
            "expansion happens once"
        );
        assert!(
            pending_of(child(&trie, 7)).is_some(),
            "siblings are untouched"
        );
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
        let HashTrieNode::Unexpanded(p) = child(&trie, 1) else {
            panic!("a lazy root bucket holds an Unexpanded child")
        };
        assert_eq!(p.built().expect("expanded").len(), 3);
        assert_eq!(
            pending_of(child(&trie, 9)),
            Some(vec![vec![9, 9], vec![9, 8]])
        );
        assert_eq!(trie.tuple_count(), 5);
        let mut all = trie.collect_tuples();
        all.sort();
        assert_eq!(all, vec![
            vec![1, 2],
            vec![1, 3],
            vec![1, 4],
            vec![9, 8],
            vec![9, 9]
        ]);
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
        assert_eq!(projected, vec![vec![3, 1], vec![4, 1], vec![6, 1], vec![
            9, 7
        ]]);
    }

    #[test]
    fn optimization_axes_include_the_expansion_layout() {
        let eager: HashTrie = HashTrie::new(2.into());
        assert_eq!(
            eager.optimization_axes().get("ds_layout_expansion"),
            Some(&serde_json::Value::String("eager".into()))
        );
        assert_eq!(
            Lazy::new(2.into())
                .optimization_axes()
                .get("ds_layout_expansion"),
            Some(&serde_json::Value::String("lazy".into()))
        );
    }
}

/// The root-capacity Config (#88). Under `tuples` a build sizes the root
/// once, from its tuple count. Under `grow`, the default, nothing
/// changes.
#[cfg(test)]
mod root_capacity_tests {
    use {
        crate::{
            cardinality::Cardinality,
            ds::hash_trie::{
                config::{ChildCapacity, HashTrieConfig, LoadFactor, RootCapacity},
                expansion::{EagerExpansion, ExpansionPolicy, LazyExpansion},
                identity::{assert_same_node, assert_same_trie, inputs},
                implementation::HashTrie,
                node::HashTrieNode,
                pruning::{NoPruning, PruningPolicy, SingletonPruning},
            },
            relation::{ConfigurableRelation, Relation},
            test_support::Mod10HashStrategy,
        },
        kermit_iters::{FxHashStrategy, HashStrategy, SipHashStrategy},
    };

    fn config(percent: u8, root_capacity: RootCapacity) -> HashTrieConfig {
        HashTrieConfig {
            load_factor: LoadFactor::percent(percent).unwrap(),
            root_capacity,
            child_capacity: ChildCapacity::Grow,
        }
    }

    /// Load factors under test, in percent.
    const PERCENTS: &[u8] = &[50, 70, 95];

    /// Distinct keys in the D = n inputs. Small under Miri.
    const DISTINCT: usize = if cfg!(miri) {
        40
    } else {
        1_000
    };

    /// The root's capacity, as a log2.
    fn root_log2<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>(
        trie: &HashTrie<H, P, E>,
    ) -> u32 {
        trie.root().buckets_len().trailing_zeros()
    }

    fn label<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>(rest: &str) -> String {
        format!("{}/{}/{} {rest}", H::NAME, P::NAME, E::NAME)
    }

    /// Runs `$check` under every pruning × expansion Layout for each
    /// hasher listed.
    macro_rules! under_every_layout {
        ($check:ident, $($hasher:ty),+) => {$(
            $check::<$hasher, NoPruning, EagerExpansion>();
            $check::<$hasher, SingletonPruning, EagerExpansion>();
            $check::<$hasher, NoPruning, LazyExpansion>();
            $check::<$hasher, SingletonPruning, LazyExpansion>();
        )+};
    }

    /// At the default, `from_tuples` builds the trie that `with_config`
    /// plus one `insert` per tuple builds. That second path is the loop
    /// `from_tuples_with_config` ran before #88, and it never sees a
    /// tuple count, so the default build does not presize.
    fn check_default_identity<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>() {
        for arity in 1..=3 {
            for (input, tuples) in inputs(arity) {
                let built = HashTrie::<H, P, E>::from_tuples(arity.into(), tuples.clone());
                let mut inserted =
                    HashTrie::<H, P, E>::with_config(arity.into(), HashTrieConfig::default());
                inserted.insert_all(tuples);
                assert_same_trie(
                    &built,
                    &inserted,
                    &label::<H, P, E>(&format!("arity {arity}, {input}")),
                );
            }
        }
    }

    #[test]
    fn the_default_builds_the_trie_it_built_before() {
        under_every_layout!(
            check_default_identity,
            SipHashStrategy,
            FxHashStrategy,
            Mod10HashStrategy
        );
    }

    /// Under `tuples` the root ends at `root_log2_capacity(n)`. Where the
    /// keys are fewer than the tuples, growth from 4 would stop below
    /// that, so the root was presized. Where every key is distinct
    /// (D = n, the most keys n tuples can bring), `hash_table`'s
    /// `a_presized_table_never_grows_for_its_keys` shows that a table at
    /// that capacity takes them all without growing.
    fn check_root_is_presized<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>() {
        for &percent in PERCENTS {
            let config = config(percent, RootCapacity::Tuples);
            for n in [0, 1, 2, 3, DISTINCT] {
                let unary = (0..n).map(|k| vec![k]).collect();
                let trie = HashTrie::<H, P, E>::from_tuples_with_config(1.into(), config, unary);
                assert_eq!(
                    root_log2(&trie),
                    config.root_log2_capacity(n),
                    "{}",
                    label::<H, P, E>(&format!("{percent}%, {n} distinct unary keys"))
                );
            }
            for arity in 2..=3 {
                for (input, tuples) in inputs(arity) {
                    let n = tuples.len();
                    let trie =
                        HashTrie::<H, P, E>::from_tuples_with_config(arity.into(), config, tuples);
                    assert_eq!(
                        root_log2(&trie),
                        config.root_log2_capacity(n),
                        "{}",
                        label::<H, P, E>(&format!("{percent}%, arity {arity}, {input}"))
                    );
                }
            }
        }
    }

    #[test]
    fn tuples_sizes_the_root_from_the_tuple_count() {
        under_every_layout!(
            check_root_is_presized,
            SipHashStrategy,
            FxHashStrategy,
            Mod10HashStrategy
        );
    }

    /// With every first value distinct (D = n) and a load factor of at
    /// least 25 %, `tuples` presizes the root to the capacity `grow`
    /// reaches. It changes when the root reaches its size, not the size.
    /// Below 25 % one doubling can leave a small table over its cap
    /// (#104), so the two can differ there. The colliding strategy is
    /// left out, because its distinct keys share hashes (D < n).
    fn check_distinct_keys_reach_the_same_capacity<
        H: HashStrategy,
        P: PruningPolicy,
        E: ExpansionPolicy,
    >() {
        let most = if cfg!(miri) {
            40
        } else {
            300
        };
        for percent in [25, 50, 70, 95] {
            for n in 0..=most {
                let tuples: Vec<Vec<usize>> = (0..n).map(|k| vec![k, k % 7]).collect();
                let build = |root_capacity| {
                    HashTrie::<H, P, E>::from_tuples_with_config(
                        2.into(),
                        config(percent, root_capacity),
                        tuples.clone(),
                    )
                };
                assert_eq!(
                    root_log2(&build(RootCapacity::Tuples)),
                    root_log2(&build(RootCapacity::Grow)),
                    "{}",
                    label::<H, P, E>(&format!("{percent}%, {n} distinct first values"))
                );
            }
        }
    }

    #[test]
    fn distinct_keys_reach_the_capacity_grow_reaches() {
        under_every_layout!(
            check_distinct_keys_reach_the_same_capacity,
            SipHashStrategy,
            FxHashStrategy
        );
    }

    /// `tuples` sizes the root only. Under it, every root entry holds the
    /// subtrie (or chain) it holds under `grow`, array for array.
    fn check_children_unchanged<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>() {
        for &percent in PERCENTS {
            for arity in 1..=3 {
                for (input, tuples) in inputs(arity) {
                    let label = label::<H, P, E>(&format!("{percent}%, arity {arity}, {input}"));
                    let build = |root_capacity| {
                        HashTrie::<H, P, E>::from_tuples_with_config(
                            arity.into(),
                            config(percent, root_capacity),
                            tuples.clone(),
                        )
                    };
                    let (grow, presized) = (build(RootCapacity::Grow), build(RootCapacity::Tuples));
                    let (g, t) = (grow.root(), presized.root());
                    assert_eq!(g.len(), t.len(), "{label}: root keys");
                    assert_eq!(
                        grow.tuple_count(),
                        presized.tuple_count(),
                        "{label}: tuples"
                    );
                    for idx in 0..g.buckets_len() {
                        let Some(hash) = g.hash_at(idx) else {
                            continue;
                        };
                        let at = t
                            .index_of(hash)
                            .unwrap_or_else(|| panic!("{label}: root key {hash:#x} missing"));
                        match (g, t) {
                            | (HashTrieNode::Inner(a), HashTrieNode::Inner(b)) => assert_same_node(
                                a.value_at(idx).unwrap(),
                                b.value_at(at).unwrap(),
                                &format!("{label}/{hash:#x}"),
                            ),
                            | (HashTrieNode::Leaf(a), HashTrieNode::Leaf(b)) => assert_eq!(
                                a.value_at(idx),
                                b.value_at(at),
                                "{label}/{hash:#x}: chain"
                            ),
                            | _ => panic!("{label}: root kinds differ"),
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn tuples_leaves_every_subtrie_as_grow_builds_it() {
        under_every_layout!(
            check_children_unchanged,
            SipHashStrategy,
            FxHashStrategy,
            Mod10HashStrategy
        );
    }

    /// A trie created empty is sized for no tuples, which is 4 buckets
    /// under either value.
    #[test]
    fn a_trie_created_empty_starts_at_four_buckets() {
        for root_capacity in [RootCapacity::Grow, RootCapacity::Tuples] {
            let trie: HashTrie = HashTrie::with_config(2.into(), config(70, root_capacity));
            assert_eq!(trie.root().buckets_len(), 4, "{root_capacity:?}");
        }
    }

    /// `project` rebuilds through `from_tuples_with_config`, so a
    /// projection of a presized trie is presized for its own tuples. The
    /// projection keeps all 100 tuples (bag semantics) but holds only 3
    /// distinct values: grown, its root would stop at 2^3 buckets;
    /// presized for 100 tuples at 70 % it has 2^8.
    #[test]
    fn a_projection_is_presized_too() {
        use crate::relation::Projectable;
        let config = config(70, RootCapacity::Tuples);
        let tuples: Vec<Vec<usize>> = (0..100).map(|k| vec![k, k % 3]).collect();
        let trie: HashTrie = HashTrie::from_tuples_with_config(2.into(), config, tuples);
        let projected = trie.project(vec![1]);
        assert_eq!(projected.tuple_count(), 100);
        assert_eq!(root_log2(&projected), config.root_log2_capacity(100));
        assert_eq!(config.root_log2_capacity(100), 8);
    }
}
