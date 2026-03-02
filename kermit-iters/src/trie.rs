//! This module defines the `TrieIterator` trait.

use crate::{joinable::JoinIterable, linear::LinearIterator};

/// The `TrieIterator` trait, designed for iterators that traverse a trie-based
/// structure.
pub trait TrieIterator: LinearIterator {
    /// If there is a child iterator at the iterator's current position,
    /// repositions at said iterator and returns `true`, otherwise returns
    /// `false`.
    ///
    /// # Note
    /// If the iterator is positioned at the end, then this proceeds as if
    /// the iterator is positioned one step backwards.
    fn open(&mut self) -> bool;

    /// If there is a parent iterator at the iterator's current position,
    /// repositions at said iterator and returns `true`, otherwise returns
    /// `false`.
    ///
    /// # Note
    ///
    /// If the iterator is positioned at the end, then this proceeds as if
    /// the iterator is positioned one step backwards.
    fn up(&mut self) -> bool;
}

/// The `TrieIterable` trait is used to specify types that can be iterated
/// through the `TrieIterable` interface, and as such used in algorithms that
/// require such an iterator.
pub trait TrieIterable: JoinIterable {
    fn trie_iter(&self) -> impl TrieIterator + IntoIterator<Item = Vec<usize>>;
}

pub struct TrieIteratorWrapper<IT>
where
    IT: TrieIterator,
{
    iter: IT,
    stack: Vec<usize>,
}

impl<IT> TrieIteratorWrapper<IT>
where
    IT: TrieIterator,
{
    pub fn new(iter: IT) -> Self {
        TrieIteratorWrapper {
            iter,
            stack: vec![],
        }
    }

    fn up(&mut self) -> bool {
        if self.iter.up() {
            self.stack.pop();
            true
        } else {
            false
        }
    }

    fn down(&mut self) -> bool {
        if !self.iter.open() {
            return false;
        }
        self.stack.push(self.iter.key().unwrap());
        true
    }

    fn next_wrapper(&mut self) -> bool {
        if self.iter.at_end() {
            false
        } else if let Some(key) = self.iter.next() {
            self.stack.pop();
            self.stack.push(key);
            true
        } else {
            false
        }
    }

    fn next(&mut self) -> Option<Vec<usize>> {
        if !self.stack.is_empty() {
            while !self.next_wrapper() {
                if !self.up() {
                    return None;
                }
            }
        }

        while self.down() {}

        if self.stack.is_empty() {
            None
        } else {
            Some(self.stack.clone())
        }
    }
}

impl<IT> Iterator for TrieIteratorWrapper<IT>
where
    IT: TrieIterator,
{
    type Item = Vec<usize>;

    fn next(&mut self) -> Option<Self::Item> { self.next() }
}

#[cfg(test)]
mod tests {
    use {super::*, crate::linear::LinearIterator};

    // -- Mock trie infrastructure --

    struct MockNode {
        key: usize,
        children: Vec<MockNode>,
    }

    fn leaf(key: usize) -> MockNode {
        MockNode {
            key,
            children: vec![],
        }
    }

    fn node(key: usize, children: Vec<MockNode>) -> MockNode {
        MockNode {
            key,
            children,
        }
    }

    struct MockTrie {
        roots: Vec<MockNode>,
    }

    struct MockLevel<'a> {
        siblings: &'a [MockNode],
        pos: usize,
    }

    struct MockTrieIter<'a> {
        trie: &'a MockTrie,
        levels: Vec<MockLevel<'a>>,
    }

    impl<'a> MockTrieIter<'a> {
        fn new(trie: &'a MockTrie) -> Self {
            MockTrieIter {
                trie,
                levels: Vec::new(),
            }
        }
    }

    impl LinearIterator for MockTrieIter<'_> {
        fn key(&self) -> Option<usize> {
            let level = self.levels.last()?;
            level.siblings.get(level.pos).map(|n| n.key)
        }

        fn next(&mut self) -> Option<usize> {
            let level = self.levels.last_mut()?;
            level.pos += 1;
            level.siblings.get(level.pos).map(|n| n.key)
        }

        fn seek(&mut self, seek_key: usize) -> bool {
            if let Some(level) = self.levels.last_mut() {
                while level.pos < level.siblings.len() {
                    if level.siblings[level.pos].key >= seek_key {
                        return true;
                    }
                    level.pos += 1;
                }
            }
            false
        }

        fn at_end(&self) -> bool {
            match self.levels.last() {
                | Some(level) => level.pos >= level.siblings.len(),
                | None => true,
            }
        }
    }

    impl TrieIterator for MockTrieIter<'_> {
        fn open(&mut self) -> bool {
            if self.levels.is_empty() {
                if self.trie.roots.is_empty() {
                    return false;
                }
                self.levels.push(MockLevel {
                    siblings: &self.trie.roots,
                    pos: 0,
                });
                true
            } else {
                let level = self.levels.last().unwrap();
                if level.pos >= level.siblings.len() {
                    return false;
                }
                let node = &level.siblings[level.pos];
                if node.children.is_empty() {
                    return false;
                }
                self.levels.push(MockLevel {
                    siblings: &node.children,
                    pos: 0,
                });
                true
            }
        }

        fn up(&mut self) -> bool { self.levels.pop().is_some() }
    }

    fn collect_tuples(trie: &MockTrie) -> Vec<Vec<usize>> {
        let iter = MockTrieIter::new(trie);
        let mut wrapper = TrieIteratorWrapper::new(iter);
        let mut result = Vec::new();
        while let Some(tuple) = wrapper.next() {
            result.push(tuple);
        }
        result
    }

    #[test]
    fn empty_trie() {
        let trie = MockTrie {
            roots: vec![],
        };
        assert_eq!(collect_tuples(&trie), Vec::<Vec<usize>>::new());
    }

    #[test]
    fn single_unary_tuple() {
        let trie = MockTrie {
            roots: vec![leaf(1)],
        };
        assert_eq!(collect_tuples(&trie), vec![vec![1]]);
    }

    #[test]
    fn multiple_unary_tuples() {
        let trie = MockTrie {
            roots: vec![leaf(1), leaf(2), leaf(3)],
        };
        assert_eq!(collect_tuples(&trie), vec![vec![1], vec![2], vec![3]]);
    }

    #[test]
    fn single_binary_tuple() {
        let trie = MockTrie {
            roots: vec![node(1, vec![leaf(2)])],
        };
        assert_eq!(collect_tuples(&trie), vec![vec![1, 2]]);
    }

    #[test]
    fn binary_shared_prefix() {
        let trie = MockTrie {
            roots: vec![node(1, vec![leaf(2), leaf(3)])],
        };
        assert_eq!(collect_tuples(&trie), vec![vec![1, 2], vec![1, 3]]);
    }

    #[test]
    fn binary_disjoint_prefixes() {
        let trie = MockTrie {
            roots: vec![node(1, vec![leaf(2)]), node(3, vec![leaf(4)])],
        };
        assert_eq!(collect_tuples(&trie), vec![vec![1, 2], vec![3, 4]]);
    }

    #[test]
    fn binary_complex() {
        let trie = MockTrie {
            roots: vec![node(1, vec![leaf(3), leaf(4)]), node(2, vec![leaf(5)])],
        };
        assert_eq!(collect_tuples(&trie), vec![vec![1, 3], vec![1, 4], vec![
            2, 5
        ]],);
    }

    #[test]
    fn ternary_tuples() {
        let trie = MockTrie {
            roots: vec![
                node(1, vec![
                    node(2, vec![leaf(5), leaf(6)]),
                    node(3, vec![leaf(7)]),
                ]),
                node(4, vec![node(8, vec![leaf(9)])]),
            ],
        };
        assert_eq!(collect_tuples(&trie), vec![
            vec![1, 2, 5],
            vec![1, 2, 6],
            vec![1, 3, 7],
            vec![4, 8, 9],
        ],);
    }

    #[test]
    fn wide_trie() {
        let trie = MockTrie {
            roots: vec![node(1, (10..20).map(leaf).collect())],
        };
        let expected: Vec<Vec<usize>> = (10..20).map(|k| vec![1, k]).collect();
        assert_eq!(collect_tuples(&trie), expected);
    }

    #[test]
    fn iterator_trait_collect() {
        let trie = MockTrie {
            roots: vec![node(1, vec![leaf(2), leaf(3)]), node(4, vec![leaf(5)])],
        };
        let iter = MockTrieIter::new(&trie);
        let wrapper = TrieIteratorWrapper::new(iter);
        let result: Vec<Vec<usize>> = wrapper.collect();
        assert_eq!(result, vec![vec![1, 2], vec![1, 3], vec![4, 5]]);
    }

    #[test]
    fn exhaustion_returns_none() {
        let trie = MockTrie {
            roots: vec![leaf(1)],
        };
        let iter = MockTrieIter::new(&trie);
        let mut wrapper = TrieIteratorWrapper::new(iter);
        assert_eq!(wrapper.next(), Some(vec![1]));
        assert_eq!(wrapper.next(), None);
    }
}
