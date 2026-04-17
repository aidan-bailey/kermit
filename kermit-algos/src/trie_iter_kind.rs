//! Dispatch wrapper letting LFTJ hold a heterogeneous set of trie
//! iterators — both real relations (borrowed) and synthetic singletons
//! produced by the [Const-view rewrite](crate::const_rewrite).

use {
    crate::singleton::SingletonTrieIter,
    kermit_iters::{JoinIterable, LinearIterator, TrieIterable, TrieIterator, TrieIteratorWrapper},
};

/// Either borrows a real relation or owns a synthetic singleton.
///
/// [`TrieIterable::trie_iter`] dispatches to the appropriate inner
/// variant. Lifetime `'a` borrows the real relation; singletons carry
/// no reference.
pub enum TrieIterKind<'a, R: TrieIterable> {
    /// A real relation borrowed from the database.
    Relation(&'a R),
    /// A synthetic `Const_<id>` singleton introduced by the rewrite.
    Singleton(SingletonTrieIter),
}

/// Iterator produced by [`TrieIterKind::trie_iter`]; dispatches all
/// `TrieIterator` / `LinearIterator` methods to the inner variant.
pub enum KindIter<IT>
where
    IT: TrieIterator,
{
    /// Iterator from a real relation.
    Relation(IT),
    /// Iterator from a synthetic singleton.
    Singleton(SingletonTrieIter),
}

impl<IT> LinearIterator for KindIter<IT>
where
    IT: TrieIterator,
{
    fn key(&self) -> Option<usize> {
        match self {
            | Self::Relation(it) => it.key(),
            | Self::Singleton(it) => it.key(),
        }
    }

    fn next(&mut self) -> Option<usize> {
        match self {
            | Self::Relation(it) => it.next(),
            | Self::Singleton(it) => it.next(),
        }
    }

    fn seek(&mut self, seek_key: usize) -> bool {
        match self {
            | Self::Relation(it) => it.seek(seek_key),
            | Self::Singleton(it) => it.seek(seek_key),
        }
    }

    fn at_end(&self) -> bool {
        match self {
            | Self::Relation(it) => it.at_end(),
            | Self::Singleton(it) => it.at_end(),
        }
    }
}

impl<IT> TrieIterator for KindIter<IT>
where
    IT: TrieIterator,
{
    fn open(&mut self) -> bool {
        match self {
            | Self::Relation(it) => it.open(),
            | Self::Singleton(it) => it.open(),
        }
    }

    fn up(&mut self) -> bool {
        match self {
            | Self::Relation(it) => it.up(),
            | Self::Singleton(it) => it.up(),
        }
    }
}

impl<IT> IntoIterator for KindIter<IT>
where
    IT: TrieIterator,
{
    type IntoIter = TrieIteratorWrapper<Self>;
    type Item = Vec<usize>;

    fn into_iter(self) -> Self::IntoIter { TrieIteratorWrapper::new(self) }
}

impl<R: TrieIterable> JoinIterable for TrieIterKind<'_, R> {}

impl<R: TrieIterable> TrieIterable for TrieIterKind<'_, R> {
    fn trie_iter(&self) -> impl TrieIterator + IntoIterator<Item = Vec<usize>> {
        match self {
            | Self::Relation(r) => KindIter::Relation(r.trie_iter()),
            | Self::Singleton(s) => KindIter::Singleton(s.clone()),
        }
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        kermit_ds::{Relation, TreeTrie},
    };

    fn tree_from(tuples: Vec<Vec<usize>>) -> TreeTrie { TreeTrie::from_tuples(2.into(), tuples) }

    #[test]
    fn kind_relation_delegates_to_inner() {
        let trie = tree_from(vec![vec![1, 2], vec![3, 4]]);
        let kind: TrieIterKind<TreeTrie> = TrieIterKind::Relation(&trie);
        let mut it = kind.trie_iter();
        assert!(it.open());
        assert_eq!(it.key(), Some(1));
    }

    #[test]
    fn kind_singleton_delegates_to_inner() {
        let kind: TrieIterKind<TreeTrie> = TrieIterKind::Singleton(SingletonTrieIter::new(7));
        let mut it = kind.trie_iter();
        assert!(it.open());
        assert_eq!(it.key(), Some(7));
    }

    #[test]
    fn kind_singleton_seek_behaves_like_bare_singleton() {
        let kind: TrieIterKind<TreeTrie> = TrieIterKind::Singleton(SingletonTrieIter::new(7));
        let mut it = kind.trie_iter();
        it.open();
        assert!(!it.seek(8));
        assert!(it.at_end());
    }

    #[test]
    fn wrapper_on_kind_iter_yields_tuples() {
        let trie = tree_from(vec![vec![1, 2]]);
        let kind: TrieIterKind<TreeTrie> = TrieIterKind::Relation(&trie);
        let tuples: Vec<Vec<usize>> = kind.trie_iter().into_iter().collect();
        assert_eq!(tuples, vec![vec![1, 2]]);
    }

    #[test]
    fn wrapper_on_kind_singleton_yields_one_tuple() {
        let kind: TrieIterKind<TreeTrie> = TrieIterKind::Singleton(SingletonTrieIter::new(42));
        let tuples: Vec<Vec<usize>> = kind.trie_iter().into_iter().collect();
        assert_eq!(tuples, vec![vec![42]]);
    }
}
