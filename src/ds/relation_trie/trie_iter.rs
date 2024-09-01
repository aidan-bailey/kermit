use {
    crate::ds::relation_trie::{
        node::{Node, TrieFields},
        trie::RelationTrie,
    },
    crate::iters::{
        linear::LinearIterator,
        trie::{TrieIterable, TrieIterator},
    },
};

pub struct TrieIter<'a, KT: PartialOrd + PartialEq + Clone> {
    /// Current Node's index amongst its siblings.
    pos: usize,
    /// Trie that is being iterated.
    trie: &'a RelationTrie<KT>,
    /// Stack containing cursor's path down the trie.
    /// The tuples hold the Node and its index amongst its siblings.
    /// If the stack is empty, the cursor points to the root.
    /// If the stack is non-empty, the cursor points to the last element.
    stack: Vec<(&'a Node<KT>, usize)>,
}

impl<'a, KT: PartialOrd + PartialEq + Clone> TrieIter<'a, KT> {
    /// Construct a new Trie iterator.
    pub fn new(trie: &'a RelationTrie<KT>) -> Self {
        TrieIter {
            pos: 0,
            trie,
            stack: Vec::new(),
        }
    }

    /// Get the siblings of the node pointed to by the cursor (including the
    /// node).
    ///
    /// Returns None if the cursor points to the root.
    fn siblings(&self) -> Option<&'a Vec<Node<KT>>> {
        if self.stack.is_empty() {
            None
        } else if self.stack.len() == 1 {
            Some(self.trie.children())
        } else {
            Some(self.stack[self.stack.len() - 2].0.children())
        }
    }
}

impl<'a, KT: PartialOrd + PartialEq + Clone> LinearIterator<KT> for TrieIter<'a, KT> {
    fn key(&self) -> Option<&KT> {
        if self.at_end() {
            None
        } else {
            Some(
                self.stack
                    .last()
                    .expect("Not at the Root or the end")
                    .0
                    .key(),
            )
        }
    }

    fn next(&mut self) -> Option<&KT> {
        if let Some(siblings) = self.siblings() {
            self.pos += 1;
            if let Some(node) = siblings.get(self.pos) {
                self.stack.pop();
                self.stack.push((node, self.pos));
                return Some(node.key());
            }
        }
        None
    }

    fn seek(&mut self, seek_key: &KT) -> Option<&KT> {
        if self.at_end() {
            return None;
        }

        if let Some(current_key) = self.key() {
            if current_key > seek_key {
                panic!("The sought key must be ≥ the key at the current position.")
            } else {
                // If there exists a key, there should ALWAYS be at least one sibling
                // (i.e., the current node itself).
                let siblings = self
                    .siblings()
                    .expect("If there exists a key, there should ALWAYS be at least one sibling");

                while (!self.at_end()) && seek_key > siblings[self.pos].key() {
                    self.pos += 1;
                }

                if self.at_end() {
                    None
                } else {
                    self.stack.pop();
                    self.stack.push((&siblings[self.pos], self.pos));
                    Some(siblings[self.pos].key())
                }
            }
        } else {
            None
        }
    }

    fn at_end(&self) -> bool {
        if let Some(siblings) = self.siblings() {
            self.pos == siblings.len()
        } else {
            true
        }
    }
}

impl<'a, KT: PartialOrd + PartialEq + Clone> TrieIterator<KT> for TrieIter<'a, KT> {
    fn open(&mut self) -> Option<&KT> {
        if let Some((node, _)) = self.stack.last() {
            if let Some(child) = node.children().first() {
                self.stack.push((child, 0));
                self.pos = 0;
                Some(child.key())
            } else {
                None
            }
        } else if self.trie.is_empty() {
            None
        } else {
            self.stack.push((&self.trie.children()[0], 0));
            Some(self.trie.children()[0].key())
        }
    }

    fn up(&mut self) -> Option<&KT> {
        if self.stack.pop().is_none() {
            None
        } else {
            self.pos = if let Some((_, i)) = self.stack.last() {
                *i
            } else {
                0
            };
            self.key()
        }
    }
}

impl<'a, KT: PartialOrd + PartialEq + Clone> TrieIterable<'a, KT> for RelationTrie<KT> {
    fn trie_iter(&'a self) -> Box<dyn TrieIterator<KT> + 'a> { Box::new(TrieIter::<'a>::new(self)) }
}
