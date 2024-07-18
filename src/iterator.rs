use crate::tuple_trie::{Node, Trie, TrieFields};

pub struct TrieIter<'a, KT: Ord> {
    pos: usize,
    trie: &'a Trie<KT>,
    stack: Vec<(&'a Node<KT>, usize)>,
}

impl<'a, KT: Ord> TrieIter<'a, KT> {
    pub fn new(trie: &'a Trie<KT>) -> Self {
        TrieIter {
            pos: 0,
            trie,
            stack: Vec::new(),
        }
    }

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

pub trait TrieIterator<KT: Ord> {
    /// Returns the key at the current iterator position
    fn key(&self) -> Result<&KT, &'static str>;
    /// Proceeds to the next key
    fn next(&mut self) -> Result<(), &'static str>;
    /// Position the iterator at a least
    /// upper bound for seekKey,
    /// i.e. the least key ≥ seekKey, or
    /// move to end if no such key exists.
    /// The sought key must be ≥ the
    /// key at the current position.
    fn seek(&mut self, seek_key: &KT) -> Result<(), &'static str>;
    /// True when iterator is at the end.
    fn at_end(&self) -> bool;
    /// Proceed to the first key at the
    /// next depth
    fn open(&mut self) -> Result<(), &'static str>;
    /// Return to the parent key at the
    /// previous depth
    fn up(&mut self) -> Result<(), &'static str>;
}

impl<'a, KT: Ord> TrieIterator<KT> for TrieIter<'a, KT> {
    fn key(&self) -> Result<&KT, &'static str> {
        if let Some((node, _)) = self.stack.last() {
            Ok(node.key())
        } else {
            Err("At root")
        }
    }

    fn next(&mut self) -> Result<(), &'static str> {
        if let Some(siblings) = self.siblings() {
            let newpos = self.pos + 1;
            if let Some(node) = siblings.get(newpos) {
                self.stack.pop();
                self.stack.push((node, newpos));
                self.pos = newpos;
                Ok(())
            } else {
                Err("At end")
            }
        } else {
            Err("At root")
        }
    }

    fn seek(&mut self, seek_key: &KT) -> Result<(), &'static str> {
        if let Ok(current_key) = self.key() {
            if current_key > seek_key {
                Err("The sought key must be ≥ the key at the current position.")
            } else {
                // If there exists a key, there should ALWAYS be at least one sibling
                // (i.e., the current node itself).
                let siblings = self.siblings().unwrap();
                while (!self.at_end()) && seek_key > siblings[self.pos].key() {
                    self.pos += 1;
                }
                Ok(())
            }
        } else {
            Ok(())
        }
    }

    fn at_end(&self) -> bool {
        if let Some(siblings) = self.siblings() {
            self.pos + 1 > siblings.len() - 1
        } else {
            true
        }
    }

    fn open(&mut self) -> Result<(), &'static str> {
        if let Some((node, _)) = self.stack.last() {
            if let Some(child) = node.children().first() {
                self.stack.push((child, 0));
                self.pos = 0;
                Ok(())
            } else {
                Err("Node is empty")
            }
        } else {
            if self.trie.is_empty() {
                Err("Empty trie")
            } else {
                self.stack.push((&self.trie.children()[0], 0));
                Ok(())
            }
        }
    }

    fn up(&mut self) -> Result<(), &'static str> {
        if self.stack.pop().is_none() {
            Err("At root")
        } else {
            self.pos = if let Some((_, i)) = self.stack.last() {
                *i
            } else {
                0
            };
            Ok(())
        }
    }
}
