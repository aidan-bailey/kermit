//! [`BuiltWith`]: lifts a runtime [`BuildModeRelation::BuildMode`] value to
//! the type level, as [`Configured`](crate::Configured) does for a Config.
//!
//! Test suites in this workspace are macro-generated and name a relation by
//! a single type identifier. A build mode has no type-level identity, so
//! `BuiltWith<R, P>` pairs a relation `R` with a zero-sized marker
//! `P: BuildModeProvider<R::BuildMode>` that supplies the mode. The wrapper
//! forwards the sorted-family traits (`TrieIterable`, `Cardinality`,
//! `HeapSize`, `Projectable`, `JoinIterable`) to `R`; only `from_tuples`
//! differs, routing through `P::build_mode()`. A hash-family BuildMode would
//! also need a `HashTrieIterable` forward, as `Configured` has.
//!
//! Like `Configured`, this is test scaffolding shipped in the library so
//! that `kermit-ds` and `kermit` integration tests share one definition.

use {
    crate::{
        cardinality::Cardinality,
        heap_size::HeapSize,
        relation::{BuildModeRelation, Projectable, Relation, RelationHeader},
    },
    kermit_iters::{JoinIterable, TrieIterable, TrieIterator},
    std::{marker::PhantomData, ops::Deref},
};

/// A zero-sized marker that names one build mode.
pub trait BuildModeProvider<M> {
    /// The build mode this marker stands for.
    fn build_mode() -> M;
}

/// Declares a [`BuildModeProvider`] marker type.
///
/// ```
/// use kermit_ds::{
///     define_build_mode_provider, BuiltWith, ColumnTrie, ColumnTrieBuildMode, Relation,
/// };
///
/// define_build_mode_provider!(
///     Incremental,
///     ColumnTrieBuildMode,
///     ColumnTrieBuildMode::Incremental
/// );
///
/// type ColumnTrieIncremental = BuiltWith<ColumnTrie, Incremental>;
///
/// let r = ColumnTrieIncremental::from_tuples(2.into(), vec![vec![1, 2]]);
/// assert_eq!(r.header().arity(), 2);
/// ```
#[macro_export]
macro_rules! define_build_mode_provider {
    ($name:ident, $mode:ty, $value:expr $(,)?) => {
        /// `BuildModeProvider` marker declared by
        /// `define_build_mode_provider!`; supplies one fixed build mode.
        #[derive(Copy, Clone, Debug, Default)]
        pub struct $name;

        impl $crate::BuildModeProvider<$mode> for $name {
            fn build_mode() -> $mode { $value }
        }
    };
}

/// `R` built by the mode `P::build_mode()`.
///
/// Derefs to `R`. Unlike [`Configured`](crate::Configured), nothing in the
/// built relation records the mode — every mode builds the same relation —
/// so there is no value for the marker to drift from, and
/// [`Projectable::project`] may rebuild through the default mode without
/// changing anything observable.
pub struct BuiltWith<R, P> {
    inner: R,
    _provider: PhantomData<P>,
}

impl<R, P> BuiltWith<R, P> {
    fn wrap(inner: R) -> Self {
        Self {
            inner,
            _provider: PhantomData,
        }
    }

    /// Unwraps the relation.
    pub fn into_inner(self) -> R { self.inner }
}

impl<R, P> Deref for BuiltWith<R, P> {
    type Target = R;

    fn deref(&self) -> &R { &self.inner }
}

impl<R: JoinIterable, P> JoinIterable for BuiltWith<R, P> {}

impl<R: Projectable, P> Projectable for BuiltWith<R, P> {
    fn project(&self, columns: Vec<usize>) -> Self { Self::wrap(self.inner.project(columns)) }
}

impl<R, P> Relation for BuiltWith<R, P>
where
    R: BuildModeRelation,
    P: BuildModeProvider<R::BuildMode>,
{
    fn header(&self) -> &RelationHeader { self.inner.header() }

    /// An empty relation has nothing to build, so no mode applies.
    fn new(header: RelationHeader) -> Self { Self::wrap(R::new(header)) }

    fn from_tuples(header: RelationHeader, tuples: Vec<Vec<usize>>) -> Self {
        Self::wrap(R::from_tuples_with_build_mode(
            header,
            P::build_mode(),
            tuples,
        ))
    }

    fn insert(&mut self, tuple: Vec<usize>) { self.inner.insert(tuple) }

    fn insert_all(&mut self, tuples: Vec<Vec<usize>>) { self.inner.insert_all(tuples) }
}

impl<R: HeapSize, P> HeapSize for BuiltWith<R, P> {
    fn heap_size_bytes(&self) -> usize { self.inner.heap_size_bytes() }
}

impl<R: Cardinality, P> Cardinality for BuiltWith<R, P> {
    fn tuple_count(&self) -> usize { self.inner.tuple_count() }
}

impl<R: TrieIterable, P> TrieIterable for BuiltWith<R, P> {
    fn trie_iter(&self) -> impl TrieIterator + IntoIterator<Item = Vec<usize>> {
        self.inner.trie_iter()
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::ds::{ColumnTrie, ColumnTrieBuildMode},
    };

    crate::define_build_mode_provider!(
        Incremental,
        ColumnTrieBuildMode,
        ColumnTrieBuildMode::Incremental
    );

    type IncrementalTrie = BuiltWith<ColumnTrie, Incremental>;

    thread_local! {
        static CALLS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    }

    /// Counts how often a build asks it for a mode, so the test below can
    /// tell a `BuiltWith` that applies its provider from one that ignores it.
    struct Spy;

    impl BuildModeProvider<ColumnTrieBuildMode> for Spy {
        fn build_mode() -> ColumnTrieBuildMode {
            CALLS.with(|calls| calls.set(calls.get() + 1));
            ColumnTrieBuildMode::Incremental
        }
    }

    fn tuples_of(relation: &impl TrieIterable) -> Vec<Vec<usize>> {
        relation.trie_iter().into_iter().collect()
    }

    #[test]
    fn from_tuples_builds_the_same_relation() {
        let tuples = vec![vec![3, 4], vec![1, 2], vec![1, 2]];
        let r = IncrementalTrie::from_tuples(2.into(), tuples.clone());
        let plain: ColumnTrie = ColumnTrie::from_tuples(2.into(), tuples);
        assert_eq!(r.header().arity(), 2);
        assert_eq!(tuples_of(&r), vec![vec![1, 2], vec![3, 4]]);
        assert_eq!(Cardinality::tuple_count(&r), 2);
        assert_eq!(r.heap_size_bytes(), plain.heap_size_bytes());
    }

    /// Without this, a `from_tuples` that ignored `P` would pass every other
    /// test, and the `ColumnTrieIncremental` suites would silently run Bulk.
    #[test]
    fn from_tuples_asks_the_provider_for_its_mode() {
        CALLS.with(|calls| calls.set(0));
        let _empty = BuiltWith::<ColumnTrie, Spy>::new(2.into());
        assert_eq!(CALLS.with(|calls| calls.get()), 0);
        let _built = BuiltWith::<ColumnTrie, Spy>::from_tuples(2.into(), vec![vec![1, 2]]);
        assert_eq!(CALLS.with(|calls| calls.get()), 1);
    }

    #[test]
    fn projection_and_inserts_reach_the_inner_relation() {
        let r = IncrementalTrie::from_tuples(2.into(), vec![vec![1, 2], vec![3, 4]]);
        assert_eq!(tuples_of(&r.project(vec![1])), vec![vec![2], vec![4]]);
        let mut grown = IncrementalTrie::new(2.into());
        grown.insert(vec![3, 4]);
        grown.insert_all(vec![vec![1, 2]]);
        assert_eq!(tuples_of(&grown), vec![vec![1, 2], vec![3, 4]]);
    }
}
