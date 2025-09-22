use {
    super::implementation::{TreeTrie, TrieNode},
    kermit_derive::IntoTrieIter,
    kermit_iters::{KeyType, LinearIterator, TrieIterable, TrieIterator, TrieIteratorWrapper},
};

/// An iterator over the nodes of a `TreeTrie`.
#[derive(IntoTrieIter)]
struct TreeTrieIter<'a, KT: KeyType> {
    pos: usize,
    trie: &'a TreeTrie<KT>,
    stack: Vec<(&'a TrieNode<KT>, usize)>,
}

impl<'a, KT: KeyType> TreeTrieIter<'a, KT> {
    fn new(trie: &'a TreeTrie<KT>) -> Self {
        Self {
            pos: 0,
            trie,
            stack: Vec::new(),
        }
    }

    fn siblings(&self) -> Option<&'a Vec<TrieNode<KT>>> {
        if self.stack.is_empty() {
            None
        } else if self.stack.len() == 1 {
            Some(self.trie.children())
        } else {
            Some(self.stack[self.stack.len() - 2].0.children())
        }
    }
}

impl<KT: KeyType> LinearIterator for TreeTrieIter<'_, KT> {
    type KT = KT;

    fn key(&self) -> Option<KT> { Some(self.siblings()?.get(self.pos)?.key()) }

    fn next(&mut self) -> Option<KT> {
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

    fn seek(&mut self, seek_key: KT) -> bool {
        if self.at_end() {
            return false;
        }

        if let Some(current_key) = self.key() {
            if current_key > seek_key {
                panic!("The sought key must be ≥ the key at the current position.");
            } else {
                let siblings = self
                    .siblings()
                    .expect("If there exists a key, there should ALWAYS be at least one sibling");

                while (!self.at_end()) && seek_key > siblings[self.pos].key() {
                    self.pos += 1;
                }

                if self.at_end() {
                    false
                } else {
                    self.stack.pop();
                    self.stack.push((&siblings[self.pos], self.pos));
                    true
                }
            }
        } else {
            false
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

impl<KT: KeyType> TrieIterator for TreeTrieIter<'_, KT> {
    fn open(&mut self) -> bool {
        if let Some((node, _)) = self.stack.last() {
            if let Some(child) = node.children().first() {
                self.stack.push((child, 0));
                self.pos = 0;
                true
            } else {
                false
            }
        } else if self.trie.children().is_empty() {
            false
        } else {
            self.stack.push((&self.trie.children()[0], 0));
            self.pos = 0;
            true
        }
    }

    fn up(&mut self) -> bool {
        if self.stack.pop().is_some() {
            self.pos = if let Some((_, i)) = self.stack.last() {
                *i
            } else {
                0
            };
            true
        } else {
            false
        }
    }
}

impl<KT: KeyType> TrieIterable for TreeTrie<KT> {
    fn trie_iter(&self) -> impl TrieIterator<KT = KT> + IntoIterator<Item = Vec<KT>> {
        TreeTrieIter::new(self)
    }
}
