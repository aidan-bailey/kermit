//! Mirror of [`crate::trie_iter_kind::TrieIterKind`] for the hash-trie
//! algorithm family. Lets the `hash_join` machinery hold a
//! heterogeneous mix of real `HashTrieIterable` relations and synthetic
//! `Const_<id>` singletons under one type.

use {
    crate::hash_singleton::SingletonHashTrieIter,
    kermit_iters::{HashTrieIterable, HashTrieIterator, JoinIterable},
};

/// Either borrows a real `HashTrieIterable` relation or owns a synthetic
/// singleton.
///
/// [`HashTrieIterable::hash_trie_iter`] dispatches to the appropriate
/// inner variant. Lifetime `'a` borrows the real relation; singletons
/// carry no reference.
pub enum HashTrieIterKind<'a, R: HashTrieIterable> {
    /// A real relation borrowed from the database.
    Relation(&'a R),
    /// A synthetic `Const_<id>` singleton introduced by the rewrite.
    Singleton(SingletonHashTrieIter),
}

/// Iterator produced by [`HashTrieIterKind::hash_trie_iter`]; dispatches
/// all [`HashTrieIterator`] methods to the inner variant.
pub enum HashKindIter<IT>
where
    IT: HashTrieIterator,
{
    /// Iterator from a real relation.
    Relation(IT),
    /// Iterator from a synthetic singleton.
    Singleton(SingletonHashTrieIter),
}

impl<IT> HashTrieIterator for HashKindIter<IT>
where
    IT: HashTrieIterator,
{
    fn key(&self) -> Option<u64> {
        match self {
            | Self::Relation(it) => it.key(),
            | Self::Singleton(it) => it.key(),
        }
    }

    fn next(&mut self) -> Option<u64> {
        match self {
            | Self::Relation(it) => it.next(),
            | Self::Singleton(it) => it.next(),
        }
    }

    fn lookup(&mut self, hash: u64) -> bool {
        match self {
            | Self::Relation(it) => it.lookup(hash),
            | Self::Singleton(it) => it.lookup(hash),
        }
    }

    fn size(&self) -> usize {
        match self {
            | Self::Relation(it) => it.size(),
            | Self::Singleton(it) => it.size(),
        }
    }

    fn at_end(&self) -> bool {
        match self {
            | Self::Relation(it) => it.at_end(),
            | Self::Singleton(it) => it.at_end(),
        }
    }

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

    fn leaf_tuples(&self) -> Option<&[Vec<usize>]> {
        match self {
            | Self::Relation(it) => it.leaf_tuples(),
            | Self::Singleton(it) => it.leaf_tuples(),
        }
    }
}

impl<R: HashTrieIterable> JoinIterable for HashTrieIterKind<'_, R> {}

impl<R: HashTrieIterable> HashTrieIterable for HashTrieIterKind<'_, R> {
    fn hash_trie_iter(&self) -> impl HashTrieIterator {
        match self {
            | Self::Relation(r) => HashKindIter::Relation(r.hash_trie_iter()),
            | Self::Singleton(s) => HashKindIter::Singleton(s.clone()),
        }
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        kermit_ds::{HashTrie, Relation},
    };

    #[test]
    fn relation_variant_delegates_open() {
        let r = HashTrie::from_tuples(2.into(), vec![vec![1, 2]]);
        let kind: HashTrieIterKind<HashTrie> = HashTrieIterKind::Relation(&r);
        let mut it = kind.hash_trie_iter();
        assert!(it.open());
    }

    #[test]
    fn singleton_variant_delegates_key() {
        use kermit_iters::{HashStrategy, SipHashStrategy};
        let hash = SipHashStrategy::hash(7);
        let kind: HashTrieIterKind<HashTrie> =
            HashTrieIterKind::Singleton(SingletonHashTrieIter::new(7, hash));
        let mut it = kind.hash_trie_iter();
        assert!(it.open());
        assert_eq!(it.key(), Some(hash));
    }
}
