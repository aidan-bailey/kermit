//! Keyed index iterator family — counterpart to
//! [`TrieIterator`](crate::TrieIterator) and
//! [`HashTrieIterator`](crate::HashTrieIterator) for binary hash joins.
//!
//! A keyed index stores one relation's rows under a key, a prefix of their
//! columns, and is probed by exact match: [`probe`](KeyedIterable::probe)
//! lends every row whose key columns equal a given key. It is the build side
//! of a hash join. A left-deep plan `[R1, …, Rm]` is evaluated by pipelining:
//! the first relation is scanned, and every later one is a hash table keyed on
//! the attributes it shares with the relations before it, probed once per
//! partial result (Wang et al., "Free Join: Unifying Worst-Case Optimal and
//! Traditional Joins", SIGMOD 2023, §2.2, Example 2.2).
//!
//! # Not a trie family
//!
//! The two trie families navigate a relation level by level: a
//! [`TrieIterator`](crate::TrieIterator) has `open` and `up` and a
//! least-upper-bound `seek` over sorted keys, and a
//! [`HashTrieIterator`](crate::HashTrieIterator) has `open`, `up` and an exact
//! `lookup` of one level's hash. A keyed index has no levels to descend: one
//! `probe` call names the whole key and lends whole rows, key columns and
//! payload alike, so an algorithm has nothing to verify afterwards. The
//! contracts differ, so `KeyedIterable` is a parallel family rather than a
//! subtrait of either. The fork propagates up the stack, as the hash family's
//! did: a separate algorithm (`BinaryHashJoin`), a separate join entry point
//! and separate test suites.
//!
//! The key length is a property of the built index, taken from the header's
//! level count ([`TrieLevels::ProbeKeys`](crate::TrieLevels::ProbeKeys) plans
//! it as the columns earlier atoms of the join order bind).

use crate::joinable::JoinIterable;

/// A relation stored as an index keyed on its leading columns and probed by
/// exact match, the unit a binary hash join is built from.
///
/// Both iterators borrow the index, lend rows in source order, and allocate
/// nothing. A structure that keeps duplicate rows lends each copy.
pub trait KeyedIterable: JoinIterable {
    /// How many leading columns form the key (`RelationHeader::levels()` at
    /// build). `0` means the index has no key: it lends every row to every
    /// probe, the case of a join order's first atom and of a cross product.
    fn key_len(&self) -> usize;

    /// Every stored row, in source order.
    fn scan(&self) -> impl Iterator<Item = &[usize]> + '_;

    /// The stored rows whose first [`key_len`](Self::key_len) values equal
    /// `key`, in source order.
    ///
    /// # Panics
    ///
    /// Panics unless `key.len() == self.key_len()`. With `key_len() == 0` the
    /// key is empty and every row is lent.
    fn probe<'a>(&'a self, key: &'a [usize]) -> impl Iterator<Item = &'a [usize]> + 'a;
}

#[cfg(test)]
mod tests {
    use crate::{JoinIterable, KeyedIterable};

    /// The smallest keyed index: the rows in a `Vec`, probed by a filtering
    /// scan. It pins the trait's shape, not any structure's behaviour.
    struct Scanned {
        key_len: usize,
        rows: Vec<Vec<usize>>,
    }

    impl JoinIterable for Scanned {}

    impl KeyedIterable for Scanned {
        fn key_len(&self) -> usize { self.key_len }

        fn scan(&self) -> impl Iterator<Item = &[usize]> + '_ {
            self.rows.iter().map(Vec::as_slice)
        }

        fn probe<'a>(&'a self, key: &'a [usize]) -> impl Iterator<Item = &'a [usize]> + 'a {
            assert_eq!(key.len(), self.key_len, "probe: a key of the wrong length");
            self.scan().filter(move |row| row.starts_with(key))
        }
    }

    fn fixture(key_len: usize) -> Scanned {
        Scanned {
            key_len,
            rows: vec![vec![1, 10], vec![2, 20], vec![1, 11]],
        }
    }

    /// What an algorithm does with the trait: only bounds, no structure.
    fn probed<I: KeyedIterable>(index: &I, key: &[usize]) -> Vec<Vec<usize>> {
        index.probe(key).map(<[usize]>::to_vec).collect()
    }

    fn scanned<I: KeyedIterable>(index: &I) -> Vec<Vec<usize>> {
        index.scan().map(<[usize]>::to_vec).collect()
    }

    #[test]
    fn probe_lends_the_rows_of_a_key_in_scan_order() {
        let index = fixture(1);
        assert_eq!(index.key_len(), 1);
        assert_eq!(probed(&index, &[1]), vec![vec![1, 10], vec![1, 11]]);
        assert_eq!(probed(&index, &[2]), vec![vec![2, 20]]);
        assert!(probed(&index, &[3]).is_empty());
    }

    #[test]
    fn a_key_of_no_columns_lends_every_row() {
        let index = fixture(0);
        assert_eq!(index.key_len(), 0);
        assert_eq!(probed(&index, &[]), scanned(&index));
        assert_eq!(scanned(&index).len(), 3);
    }
}
