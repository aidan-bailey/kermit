use {
    super::implementation::{TreeTrie, TrieNode},
    kermit_derive::IntoTrieIter,
    kermit_iters::{LinearIterator, TrieIterable, TrieIterator, TrieIteratorWrapper},
};

/// A [`TrieIterator`] over a [`TreeTrie`].
///
/// # Position model
///
/// `stack` holds `(node, sibling_index)` pairs from root to current depth;
/// `stack.last()` is the node we are currently positioned on. To list the
/// *siblings* of the current node we read the children of the parent:
/// `stack[len - 2]` for depth ≥ 2, or [`TreeTrie::children`] for depth 1.
///
/// `sibling_idx` is the authoritative cursor within the current sibling
/// list. It is **not** redundant with the deepest stack entry's index: when
/// the cursor advances past the last sibling, `sibling_idx` becomes
/// `siblings.len()` (past-end) while the stack entry keeps pointing at the
/// last *valid* node — so [`open`](TrieIterator::open) still descends into
/// that node's children after [`at_end`](LinearIterator::at_end), the
/// descent LFTJ relies on. Folding the cursor into the stack entry would
/// lose that past-end state.
///
/// Compare to [`ColumnTrieIter`](super::super::column_trie::ColumnTrie):
/// where `ColumnTrieIter` carries three integer coordinates over flat
/// arrays, `TreeTrieIter` walks pointer-linked nodes via the explicit
/// stack.
#[derive(IntoTrieIter)]
struct TreeTrieIter<'a> {
    /// Cursor within the current sibling list; may sit one past the last
    /// sibling (past-end). See the type-level docs for why this is not the
    /// same as the deepest stack entry's index.
    sibling_idx: usize,
    /// The trie being iterated.
    trie: &'a TreeTrie,
    /// Path from the root to the current depth.
    stack: Vec<(&'a TrieNode, usize)>,
}

impl<'a> TreeTrieIter<'a> {
    fn new(trie: &'a TreeTrie) -> Self {
        Self {
            sibling_idx: 0,
            trie,
            stack: Vec::new(),
        }
    }

    /// Returns the sibling list at the current depth, or `None` if the stack
    /// is empty (iterator not yet opened).
    ///
    /// Stack invariant: each entry is `(current_node, sibling_index)` from
    /// root to the current position. To get the siblings of the current node
    /// we need the *parent's* children list:
    /// - depth 1 → parent is the trie root, so siblings are `trie.children()`
    /// - depth 2+ → parent is at `stack[len - 2]`, so siblings are its children
    fn siblings(&self) -> Option<&'a Vec<TrieNode>> {
        if self.stack.is_empty() {
            None
        } else if self.stack.len() == 1 {
            Some(self.trie.children())
        } else {
            Some(self.stack[self.stack.len() - 2].0.children())
        }
    }
}

impl LinearIterator for TreeTrieIter<'_> {
    fn key(&self) -> Option<usize> { Some(self.siblings()?.get(self.sibling_idx)?.key()) }

    fn next(&mut self) -> Option<usize> {
        if let Some(siblings) = self.siblings() {
            if self.at_end() {
                return None;
            }
            self.sibling_idx += 1;
            if let Some(node) = siblings.get(self.sibling_idx) {
                // Update the deepest stack entry in place — same node/index the
                // pop-then-push idiom produced, without the churn.
                let top = self
                    .stack
                    .last_mut()
                    .expect("stack non-empty when siblings() is Some");
                *top = (node, self.sibling_idx);
                return Some(node.key());
            }
        }
        None
    }

    /// Advances to the least upper bound of `seek_key` among the current
    /// siblings. Returns `false` if no key `≥ seek_key` remains.
    ///
    /// # Panics
    ///
    /// Panics if `seek_key < self.key()`. `seek` is only valid for
    /// **forward** moves — every iterator in the [`LinearIterator`] contract
    /// is positioned at the start of a sorted sequence and may only advance.
    /// The caller is responsible for ensuring this; the join algorithms in
    /// `kermit-algos` enforce it via the leapfrog ring's invariants.
    fn seek(&mut self, seek_key: usize) -> bool {
        if self.at_end() {
            return false;
        }

        if let Some(current_key) = self.key() {
            if current_key > seek_key {
                panic!("seek_key must be ≥ the key at the current position");
            } else {
                let siblings = self
                    .siblings()
                    .expect("If there exists a key, there should ALWAYS be at least one sibling");

                while (!self.at_end()) && seek_key > siblings[self.sibling_idx].key() {
                    self.sibling_idx += 1;
                }

                if self.at_end() {
                    false
                } else {
                    // Update the deepest stack entry in place (see `next`).
                    let top = self
                        .stack
                        .last_mut()
                        .expect("stack non-empty when siblings() is Some");
                    *top = (&siblings[self.sibling_idx], self.sibling_idx);
                    true
                }
            }
        } else {
            false
        }
    }

    fn at_end(&self) -> bool {
        if let Some(siblings) = self.siblings() {
            self.sibling_idx >= siblings.len()
        } else {
            true
        }
    }
}

impl TrieIterator for TreeTrieIter<'_> {
    fn open(&mut self) -> bool {
        // No `at_end` guard (unlike `ColumnTrieIter::open`): the stack top
        // holds the resolved current node, so we descend into its
        // children directly rather than by offset arithmetic that could
        // overshoot a sibling slice.
        if let Some((node, _)) = self.stack.last() {
            if let Some(child) = node.children().first() {
                self.stack.push((child, 0));
                self.sibling_idx = 0;
                true
            } else {
                false
            }
        } else if self.trie.children().is_empty() {
            false
        } else {
            self.stack.push((&self.trie.children()[0], 0));
            self.sibling_idx = 0;
            true
        }
    }

    fn up(&mut self) -> bool {
        if self.stack.pop().is_some() {
            self.sibling_idx = if let Some((_, i)) = self.stack.last() {
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

impl TrieIterable for TreeTrie {
    fn trie_iter(&self) -> impl TrieIterator + IntoIterator<Item = Vec<usize>> {
        TreeTrieIter::new(self)
    }
}
