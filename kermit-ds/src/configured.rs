//! [`Configured`]: lifts a runtime [`ConfigurableRelation::Config`] value to
//! the type level.
//!
//! Test suites in this workspace are macro-generated and name a relation by
//! a single type identifier (`define_multiway_join_test_suite!(HashTrieSip,
//! …)`). A Config value has no type-level identity, so `Configured<R, P>` pairs
//! a relation `R` with a zero-sized marker `P: ConfigProvider<R::Config>` that
//! supplies the value. The wrapper delegates every trait to `R`; only the
//! two constructors differ, routing through `P::config()`.
//!
//! This is test scaffolding shipped in the library so that `kermit-ds` and
//! `kermit` integration tests share one definition. The data structure
//! itself never sees the marker — its config stays a runtime value, which
//! is what makes it Config rather than Layout.

use {
    crate::{
        cardinality::Cardinality,
        heap_size::HeapSize,
        relation::{ConfigurableRelation, Projectable, Relation, RelationHeader},
    },
    kermit_iters::{
        HasOptimizationAxes, HashTrieIterable, HashTrieIterator, JoinIterable, TrieIterable,
        TrieIterator,
    },
    serde_json::Value,
    std::{collections::BTreeMap, marker::PhantomData, ops::Deref},
};

/// A zero-sized marker that names one configuration value.
pub trait ConfigProvider<C> {
    /// The configuration this marker stands for.
    fn config() -> C;
}

/// Declares a [`ConfigProvider`] marker type.
///
/// ```
/// use {
///     kermit_ds::{
///         define_config_provider, ConfigurableRelation, Configured, HashTrie, HashTrieConfig,
///         Relation,
///     },
///     kermit_iters::SipHashStrategy,
/// };
///
/// define_config_provider!(PruningOn, HashTrieConfig, HashTrieConfig {
///     singleton_pruning: true,
/// });
///
/// type HashTrieSipPruned = Configured<HashTrie<SipHashStrategy>, PruningOn>;
///
/// let r = HashTrieSipPruned::from_tuples(2.into(), vec![vec![1, 2]]);
/// assert!(r.config().singleton_pruning);
/// ```
#[macro_export]
macro_rules! define_config_provider {
    ($name:ident, $config:ty, $value:expr $(,)?) => {
        #[doc = concat!(
                    "`ConfigProvider` marker supplying a fixed `",
                    stringify!($config),
                    "` value.",
                )]
        #[derive(Copy, Clone, Debug, Default)]
        pub struct $name;

        impl $crate::ConfigProvider<$config> for $name {
            fn config() -> $config { $value }
        }
    };
}

/// `R` built with the configuration `P::config()`.
///
/// Derefs to `R`, so inherent methods (`HashTrie::collect_tuples`) are
/// reachable; the trait impls below forward to `R`'s.
///
/// `config()` (reached by auto-deref to `R`) reports the inner relation's
/// runtime config. It equals `P::config()` because `wrap` is private and
/// [`Relation::new`] / [`Relation::from_tuples`] are the only constructors —
/// keep it that way, or the type-level marker and the value can drift apart.
/// In particular, `Configured` deliberately does **not** implement
/// [`ConfigurableRelation`]: its config-carrying constructors take an
/// arbitrary value that `P` cannot vouch for.
pub struct Configured<R, P> {
    inner: R,
    _provider: PhantomData<P>,
}

impl<R, P> Configured<R, P> {
    fn wrap(inner: R) -> Self {
        Self {
            inner,
            _provider: PhantomData,
        }
    }

    /// Unwraps the configured relation.
    pub fn into_inner(self) -> R { self.inner }
}

impl<R, P> Deref for Configured<R, P> {
    type Target = R;

    fn deref(&self) -> &R { &self.inner }
}

impl<R: JoinIterable, P> JoinIterable for Configured<R, P> {}

impl<R: Projectable, P> Projectable for Configured<R, P> {
    // Rewrapping keeps `P` honest only because `R::project` preserves the
    // inner config (as `HashTrie` does); a structure whose `project` rebuilds
    // via plain `from_tuples` would silently downgrade to the default config.
    fn project(&self, columns: Vec<usize>) -> Self { Self::wrap(self.inner.project(columns)) }
}

impl<R, P> Relation for Configured<R, P>
where
    R: ConfigurableRelation,
    P: ConfigProvider<R::Config>,
{
    fn header(&self) -> &RelationHeader { self.inner.header() }

    fn new(header: RelationHeader) -> Self { Self::wrap(R::with_config(header, P::config())) }

    fn from_tuples(header: RelationHeader, tuples: Vec<Vec<usize>>) -> Self {
        Self::wrap(R::from_tuples_with_config(header, P::config(), tuples))
    }

    fn insert(&mut self, tuple: Vec<usize>) { self.inner.insert(tuple) }

    fn insert_all(&mut self, tuples: Vec<Vec<usize>>) { self.inner.insert_all(tuples) }
}

impl<R: HeapSize, P> HeapSize for Configured<R, P> {
    fn heap_size_bytes(&self) -> usize { self.inner.heap_size_bytes() }
}

impl<R: Cardinality, P> Cardinality for Configured<R, P> {
    fn tuple_count(&self) -> usize { self.inner.tuple_count() }
}

impl<R: HashTrieIterable, P> HashTrieIterable for Configured<R, P> {
    fn hash_trie_iter(&self) -> impl HashTrieIterator { self.inner.hash_trie_iter() }
}

impl<R: TrieIterable, P> TrieIterable for Configured<R, P> {
    fn trie_iter(&self) -> impl TrieIterator + IntoIterator<Item = Vec<usize>> {
        self.inner.trie_iter()
    }
}

impl<R: HasOptimizationAxes, P> HasOptimizationAxes for Configured<R, P> {
    fn optimization_axes(&self) -> BTreeMap<String, Value> { self.inner.optimization_axes() }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::{
            ds::{HashTrie, HashTrieConfig},
            relation::{ConfigurableRelation, Projectable, Relation},
        },
        kermit_iters::{HasOptimizationAxes, HashTrieIterable, HashTrieIterator, SipHashStrategy},
    };

    crate::define_config_provider!(PruningOn, HashTrieConfig, HashTrieConfig {
        singleton_pruning: true,
    });

    type Pruned = Configured<HashTrie<SipHashStrategy>, PruningOn>;

    #[test]
    fn constructors_inject_the_provider_config() {
        let a = Pruned::new(2.into());
        assert!(a.config().singleton_pruning);
        let b = Pruned::from_tuples(2.into(), vec![vec![1, 2]]);
        assert!(b.config().singleton_pruning);
        assert_eq!(b.collect_tuples(), vec![vec![1, 2]]);
    }

    #[test]
    fn delegated_traits_reach_the_inner_relation() {
        let r = Pruned::from_tuples(2.into(), vec![vec![1, 2], vec![3, 4]]);
        assert_eq!(r.header().arity(), 2);
        assert_eq!(crate::Cardinality::tuple_count(&r), 2);
        assert!(crate::HeapSize::heap_size_bytes(&r) > 0);
        let mut it = r.hash_trie_iter();
        assert!(it.open());
        assert_eq!(it.size(), 2);
        assert_eq!(
            r.optimization_axes().get("ds_config_singleton_pruning"),
            Some(&serde_json::Value::Bool(true))
        );
        let p = r.project(vec![0]);
        assert!(p.config().singleton_pruning);
    }

    #[test]
    fn inserts_reach_the_inner_relation() {
        let mut r = Pruned::new(2.into());
        r.insert(vec![1, 2]);
        r.insert_all(vec![vec![3, 4]]);
        let mut tuples = r.collect_tuples();
        tuples.sort();
        assert_eq!(tuples, vec![vec![1, 2], vec![3, 4]]);
        assert_eq!(crate::Cardinality::tuple_count(&r), 2);
    }
}
