//! `HashTrieIter` — navigates a `HashTrie` via the [`HashTrieIterator`]
//! interface.
//!
//! # Position model
//!
//! `stack` carries one [`Frame`] per opened level, root first. The deepest
//! frame is the iterator's current position. Empty stack = not yet opened
//! (pre-root).
//!
//! A [`Frame::Table`] is `(node, bucket_index)` on an `Inner` / `Leaf`
//! node. A [`Frame::Singleton`] carries the singleton frame type the
//! pruning policy `P` chooses: under `SingletonPruning` it emulates the
//! one-entry table a pruned level would have held (see `pruning.rs`), and
//! under `NoPruning` it is uninhabited, so the variant vanishes and a
//! frame is exactly the `(node, bucket_index)` pair.
//!
//! On every `open`, we descend either into the root (when the stack is
//! empty) or into the child of the current bucket. In the table case we
//! then advance to the first occupied bucket within that node; if the node
//! has no occupied buckets, the new frame sits past-end and `at_end`
//! returns true. A singleton frame needs no such advance — its single
//! entry is ready as soon as the frame is built.

use {
    super::{
        implementation::HashTrie,
        node::HashTrieNode,
        pruning::{NoPruning, PruningPolicy, SingletonFrame, SingletonPayload},
    },
    crate::relation::Relation,
    kermit_iters::{HashStrategy, HashTrieIterator, SipHashStrategy},
};

/// One opened level of the trie.
enum Frame<'a, P: PruningPolicy> {
    /// A bucket within an `Inner` or `Leaf` node — never a `Singleton`.
    Table {
        node: &'a HashTrieNode<P>,
        idx: usize,
    },
    /// Level `depth` of a pruned subtrie; the frame type is `P::Frame`,
    /// which is `Never` under `NoPruning`, so this variant is uninhabited
    /// there and the enum is the bare table pair.
    Singleton(P::Frame<'a>),
}

impl<P: PruningPolicy> Frame<'_, P> {
    fn at_end(&self) -> bool {
        match self {
            | Frame::Table {
                node,
                idx,
            } => *idx >= node.buckets_len(),
            | Frame::Singleton(s) => s.at_end(),
        }
    }
}

/// What `open` found below the current position.
enum Descent<'a, P: PruningPolicy> {
    /// A table node to descend into.
    Node(&'a HashTrieNode<P>),
    /// Stay inside a pruned subtrie: emulate the next level down for
    /// `tuple`. The depth is the one `open` already computed.
    Deeper(&'a Vec<usize>),
    /// Nothing below (leaf level, empty bucket, or exhausted).
    Blocked,
}

/// Stack-based iterator over a [`HashTrie`].
///
/// Generic over the same [`HashStrategy`] `H` as the underlying
/// [`HashTrie`]; `H` hashes the emulated keys of pruned levels, so the
/// keys a singleton frame reports agree with the ones a materialised
/// table would have stored.
///
/// See the module docs for the position model.
pub struct HashTrieIter<'a, H: HashStrategy = SipHashStrategy, P: PruningPolicy = NoPruning> {
    stack: Vec<Frame<'a, P>>,
    trie: &'a HashTrie<H, P>,
}

impl<'a, H: HashStrategy, P: PruningPolicy> HashTrieIter<'a, H, P> {
    /// Construct a fresh iterator positioned before the root.
    pub(crate) fn new(trie: &'a HashTrie<H, P>) -> Self {
        Self {
            stack: Vec::new(),
            trie,
        }
    }

    fn arity(&self) -> usize { self.trie.header().arity() }

    /// The frame that opening `child` at `depth` produces, positioned on
    /// its first entry (or past-end if it has none).
    fn frame_for(child: &'a HashTrieNode<P>, depth: usize) -> Frame<'a, P> {
        match child {
            | HashTrieNode::Singleton(payload) => Self::singleton_frame(payload.tuple(), depth),
            | table => Frame::Table {
                node: table,
                idx: table.next_occupied(0),
            },
        }
    }

    // `&'a Vec`, not `&'a [usize]`: `SingletonFrame::new` stores the borrow
    // and hands it back from `tuple()`, which feeds `leaf_tuples`'
    // `&[Vec<usize>]` via `slice::from_ref`.
    #[allow(clippy::ptr_arg)]
    fn singleton_frame(tuple: &'a Vec<usize>, depth: usize) -> Frame<'a, P> {
        Frame::Singleton(P::Frame::new(tuple, depth, H::hash(tuple[depth])))
    }

    /// Where `open` would go from the current position.
    ///
    /// Matches on `*node` and copies the singleton's `tuple` reference out
    /// of the frame so the returned references carry the trie lifetime
    /// `'a`, not the shorter borrow of `self.stack`.
    fn descent(&self) -> Descent<'a, P> {
        match self.stack.last() {
            | None => Descent::Node(self.trie.root()),
            | Some(Frame::Table {
                node,
                idx,
            }) => match *node {
                | HashTrieNode::Inner(t) => match t.value_at(*idx) {
                    | Some(child) => Descent::Node(child),
                    | None => Descent::Blocked, // current bucket empty / past-end
                },
                | HashTrieNode::Leaf(_) => Descent::Blocked,
                | HashTrieNode::Singleton(_) => unreachable!(
                    "Table frame holds a Singleton; frame_for routes pruned subtries to \
                     Frame::Singleton"
                ),
            },
            | Some(Frame::Singleton(s)) => {
                // The top frame stands at depth `stack.len() - 1`, so the
                // level below it exists only while `stack.len() < arity`.
                if s.at_end() || self.stack.len() >= self.arity() {
                    Descent::Blocked
                } else {
                    Descent::Deeper(s.tuple())
                }
            },
        }
    }
}

impl<H: HashStrategy, P: PruningPolicy> HashTrieIterator for HashTrieIter<'_, H, P> {
    fn key(&self) -> Option<u64> {
        match self.stack.last()? {
            | Frame::Table {
                node,
                idx,
            } => node.hash_at(*idx),
            | Frame::Singleton(s) => s.key(),
        }
    }

    fn next(&mut self) -> Option<u64> {
        match self.stack.last_mut()? {
            | Frame::Table {
                node,
                idx,
            } => {
                // Start from idx + 1 (paper's "advance"), find next occupied
                // or past-end.
                *idx = node.next_occupied(*idx + 1);
                if *idx >= node.buckets_len() {
                    return None;
                }
                node.hash_at(*idx)
            },
            | Frame::Singleton(s) => {
                s.exhaust();
                None
            },
        }
    }

    fn lookup(&mut self, hash: u64) -> bool {
        let Some(frame) = self.stack.last_mut() else {
            return false;
        };
        match frame {
            | Frame::Table {
                node,
                idx,
            } => match node.index_of(hash) {
                | Some(i) => {
                    *idx = i;
                    true
                },
                | None => {
                    // Move to past-end; the caller's loop should exit.
                    *idx = node.buckets_len();
                    false
                },
            },
            | Frame::Singleton(s) => s.lookup(hash),
        }
    }

    fn size(&self) -> usize {
        match self.stack.last() {
            | Some(Frame::Table {
                node, ..
            }) => node.len(),
            | Some(Frame::Singleton(_)) => 1,
            | None => 0,
        }
    }

    fn at_end(&self) -> bool { self.stack.last().is_none_or(Frame::at_end) }

    fn open(&mut self) -> bool {
        // No `at_end` guard (unlike `ColumnTrieIter::open`): the stack top
        // holds a resolved frame, so a past-end bucket yields `Blocked` and
        // open returns false, with no offset arithmetic to overshoot.
        let depth = self.stack.len();
        let frame = match self.descent() {
            | Descent::Node(child) => Self::frame_for(child, depth),
            | Descent::Deeper(tuple) => Self::singleton_frame(tuple, depth),
            | Descent::Blocked => return false,
        };
        let opened = !frame.at_end();
        self.stack.push(frame);
        opened
    }

    fn up(&mut self) -> bool { self.stack.pop().is_some() }

    fn leaf_tuples(&self) -> Option<&[Vec<usize>]> {
        match self.stack.last()? {
            | Frame::Table {
                node,
                idx,
            } => match *node {
                | HashTrieNode::Leaf(t) => t.value_at(*idx).map(|v| v.as_slice()),
                | HashTrieNode::Inner(_) => None,
                | HashTrieNode::Singleton(_) => unreachable!(
                    "Table frame holds a Singleton; frame_for routes pruned subtries to \
                     Frame::Singleton"
                ),
            },
            // The top frame stands at depth `stack.len() - 1`, so it is the
            // leaf level exactly when `stack.len() == arity`.
            | Frame::Singleton(s) => (!s.at_end() && self.stack.len() == self.arity())
                .then(|| std::slice::from_ref(s.tuple())),
        }
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::{ds::hash_trie::implementation::HashTrie, relation::Relation},
    };

    // Tests pin `HashTrie` (no turbofish) — relying on the default
    // `<H = SipHashStrategy>` resolved in type position via the explicit
    // let-binding annotation.

    #[test]
    fn open_on_empty_trie_returns_false() {
        let trie: HashTrie = HashTrie::new(2.into());
        let mut it = HashTrieIter::new(&trie);
        assert!(!it.open());
    }

    #[test]
    fn open_descends_into_populated_root() {
        let trie: HashTrie = HashTrie::from_tuples(2.into(), vec![vec![1, 2]]);
        let mut it = HashTrieIter::new(&trie);
        assert!(it.open());
    }

    #[test]
    fn open_then_open_descends_two_levels() {
        let trie: HashTrie = HashTrie::from_tuples(2.into(), vec![vec![1, 2]]);
        let mut it = HashTrieIter::new(&trie);
        assert!(it.open()); // depth 1 (root level)
        assert!(it.open()); // depth 2 (leaf level)
    }

    #[test]
    fn open_three_times_fails_on_arity_2() {
        let trie: HashTrie = HashTrie::from_tuples(2.into(), vec![vec![1, 2]]);
        let mut it = HashTrieIter::new(&trie);
        it.open();
        it.open();
        // Already at leaf — can't descend further.
        assert!(!it.open());
    }

    #[test]
    fn key_returns_hash_at_current_bucket() {
        let trie: HashTrie = HashTrie::from_tuples(1.into(), vec![vec![42]]);
        let mut it = HashTrieIter::new(&trie);
        it.open();
        let h = it.key().expect("key after open should be Some");
        // Verify it matches the default-strategy (SipHash) hash of `42`.
        assert_eq!(h, <SipHashStrategy as HashStrategy>::hash(42));
    }

    #[test]
    fn at_end_after_advancing_past_last() {
        let trie: HashTrie = HashTrie::from_tuples(1.into(), vec![vec![42]]);
        let mut it = HashTrieIter::new(&trie);
        it.open();
        assert!(!it.at_end());
        // Single tuple => one occupied bucket. next() advances past it.
        it.next();
        assert!(it.at_end());
        assert!(it.key().is_none());
    }

    #[test]
    fn next_iterates_through_all_occupied_buckets() {
        // Insert several tuples whose attribute-0 hashes are likely distinct.
        // Because we can't predict bucket order without inspecting hashes,
        // assert the *set* of yielded hashes matches the expected set.
        let trie: HashTrie =
            HashTrie::from_tuples(1.into(), vec![vec![1], vec![2], vec![3], vec![4], vec![5]]);
        let mut it = HashTrieIter::new(&trie);
        it.open();
        let mut seen = std::collections::HashSet::new();
        while !it.at_end() {
            seen.insert(it.key().unwrap());
            it.next();
        }
        let expected: std::collections::HashSet<_> = (1..=5_usize)
            .map(<SipHashStrategy as HashStrategy>::hash)
            .collect();
        assert_eq!(seen, expected);
    }

    #[test]
    fn size_at_root_returns_distinct_count() {
        let trie: HashTrie = HashTrie::from_tuples(2.into(), vec![
            vec![1, 10],
            vec![1, 20], // same attr-0 hash as above
            vec![2, 30],
            vec![3, 40],
        ]);
        let mut it = HashTrieIter::new(&trie);
        it.open();
        // Three distinct attribute-0 values => three root-level buckets.
        assert_eq!(it.size(), 3);
    }

    #[test]
    fn lookup_hits_existing_hash() {
        let trie: HashTrie = HashTrie::from_tuples(2.into(), vec![vec![1, 2], vec![3, 4]]);
        let mut it = HashTrieIter::new(&trie);
        it.open();
        let h = <SipHashStrategy as HashStrategy>::hash(3);
        assert!(it.lookup(h));
        assert_eq!(it.key(), Some(h));
    }

    #[test]
    fn lookup_misses_unknown_hash() {
        let trie: HashTrie = HashTrie::from_tuples(2.into(), vec![vec![1, 2]]);
        let mut it = HashTrieIter::new(&trie);
        it.open();
        let h = <SipHashStrategy as HashStrategy>::hash(99);
        assert!(!it.lookup(h));
    }

    #[test]
    fn up_returns_to_parent_depth() {
        let trie: HashTrie = HashTrie::from_tuples(2.into(), vec![vec![1, 2]]);
        let mut it = HashTrieIter::new(&trie);
        it.open(); // depth 1
        let key_at_depth_1 = it.key();
        it.open(); // depth 2
        assert!(it.up()); // back to depth 1
        assert_eq!(it.key(), key_at_depth_1);
    }

    #[test]
    fn up_returns_false_at_root() {
        let trie: HashTrie = HashTrie::from_tuples(2.into(), vec![vec![1, 2]]);
        let mut it = HashTrieIter::new(&trie);
        // up() with empty stack
        assert!(!it.up());
        it.open();
        // up() with stack depth 1 — pops, stack now empty; up() returns true
        // because we did pop something
        assert!(it.up());
        // now stack is empty, up returns false
        assert!(!it.up());
    }

    #[test]
    fn leaf_tuples_returns_none_at_inner_level() {
        let trie: HashTrie = HashTrie::from_tuples(2.into(), vec![vec![1, 2]]);
        let mut it = HashTrieIter::new(&trie);
        it.open(); // at Inner level
        assert!(it.leaf_tuples().is_none());
    }

    #[test]
    fn leaf_tuples_returns_some_at_leaf_level() {
        let trie: HashTrie = HashTrie::from_tuples(2.into(), vec![vec![1, 2], vec![1, 3]]);
        let mut it = HashTrieIter::new(&trie);
        it.open();
        it.open();
        let chain = it.leaf_tuples().expect("leaf_tuples Some at leaf");
        // The chain has at least one tuple at this bucket position.
        assert!(!chain.is_empty());
    }

    #[test]
    fn leaf_tuples_none_when_no_leaf_in_stack() {
        let trie: HashTrie = HashTrie::from_tuples(2.into(), vec![vec![1, 2]]);
        let it = HashTrieIter::new(&trie);
        // Pre-open — no stack entry at all.
        assert!(it.leaf_tuples().is_none());
    }

    // ── Singleton emulation ────────────────────────────────────────────

    use crate::ds::hash_trie::pruning::SingletonPruning;

    /// The pruned Layout of the default hasher.
    type Pruned = HashTrie<SipHashStrategy, SingletonPruning>;

    fn h(k: usize) -> u64 { <SipHashStrategy as HashStrategy>::hash(k) }

    #[test]
    fn off_frame_is_the_bare_table_pair() {
        // The `NoPruning` frame collapses to one inhabited variant, so the
        // stack entry is exactly the pre-pruning `(node, idx)` pair.
        assert_eq!(
            std::mem::size_of::<Frame<'static, NoPruning>>(),
            std::mem::size_of::<(&'static HashTrieNode<NoPruning>, usize)>()
        );
    }

    #[test]
    fn singleton_frames_emulate_one_entry_tables_down_to_the_leaf() {
        let trie = Pruned::from_tuples(3.into(), vec![vec![1, 2, 3]]);
        let mut it = HashTrieIter::new(&trie);
        assert!(it.open()); // root table, bucket for hash(1)
        assert_eq!(it.key(), Some(h(1)));
        assert!(it.leaf_tuples().is_none());

        assert!(it.open()); // singleton frame at depth 1
        assert_eq!(it.key(), Some(h(2)));
        assert_eq!(it.size(), 1);
        assert!(!it.at_end());
        assert!(it.leaf_tuples().is_none());

        assert!(it.open()); // singleton frame at depth 2 (leaf depth)
        assert_eq!(it.key(), Some(h(3)));
        assert_eq!(it.leaf_tuples(), Some(&[vec![1, 2, 3]][..]));
        assert!(!it.open()); // no level below the leaf

        assert!(it.up());
        assert_eq!(it.key(), Some(h(2)));
    }

    #[test]
    fn singleton_next_exhausts_and_open_on_exhausted_fails() {
        let trie = Pruned::from_tuples(2.into(), vec![vec![1, 2]]);
        let mut it = HashTrieIter::new(&trie);
        it.open();
        it.open();
        assert_eq!(it.next(), None);
        assert!(it.at_end());
        assert!(it.key().is_none());
        assert!(it.leaf_tuples().is_none());
        assert!(!it.open());
    }

    #[test]
    fn singleton_lookup_hit_positions_and_miss_exhausts() {
        let trie = Pruned::from_tuples(2.into(), vec![vec![1, 2]]);
        let mut it = HashTrieIter::new(&trie);
        it.open();
        it.open();
        assert!(!it.lookup(h(999)));
        assert!(it.at_end());
        assert!(it.lookup(h(2)));
        assert!(!it.at_end());
        assert_eq!(it.key(), Some(h(2)));
    }

    #[test]
    fn open_from_table_into_singleton_child() {
        let trie = Pruned::from_tuples(2.into(), vec![vec![1, 2], vec![3, 4]]);
        let mut it = HashTrieIter::new(&trie);
        assert!(it.open());
        let mut leaves = Vec::new();
        while !it.at_end() {
            assert!(it.open());
            leaves.extend_from_slice(it.leaf_tuples().expect("singleton at leaf depth"));
            it.up();
            it.next();
        }
        leaves.sort();
        assert_eq!(leaves, vec![vec![1, 2], vec![3, 4]]);
    }

    #[test]
    fn pruned_and_plain_iterators_yield_identical_hash_paths() {
        fn walk(
            it: &mut dyn HashTrieIterator, arity: usize, path: &mut Vec<u64>,
            out: &mut Vec<Vec<u64>>,
        ) {
            while let Some(k) = it.key() {
                path.push(k);
                if path.len() == arity {
                    out.push(path.clone());
                } else {
                    assert!(it.open());
                    walk(it, arity, path, out);
                    it.up();
                }
                path.pop();
                it.next();
            }
        }
        let tuples = vec![vec![1, 2, 3], vec![1, 2, 4], vec![1, 5, 6], vec![7, 8, 9]];
        let plain: HashTrie = HashTrie::from_tuples(3.into(), tuples.clone());
        let compact = Pruned::from_tuples(3.into(), tuples);
        let (mut a, mut b) = (Vec::new(), Vec::new());
        let mut ia = HashTrieIter::new(&plain);
        let mut ib = HashTrieIter::new(&compact);
        assert!(ia.open());
        assert!(ib.open());
        walk(&mut ia, 3, &mut Vec::new(), &mut a);
        walk(&mut ib, 3, &mut Vec::new(), &mut b);
        a.sort();
        b.sort();
        assert_eq!(a, b);
    }

    #[test]
    fn size_is_one_on_a_leaf_depth_singleton_frame() {
        let trie = Pruned::from_tuples(2.into(), vec![vec![1, 2]]);
        let mut it = HashTrieIter::new(&trie);
        it.open();
        it.open(); // singleton frame at leaf depth
        assert_eq!(it.size(), 1);
        assert_eq!(it.leaf_tuples(), Some(&[vec![1, 2]][..]));
    }

    #[test]
    fn lookup_hit_on_inner_singleton_then_open_reaches_the_leaf() {
        let trie = Pruned::from_tuples(3.into(), vec![vec![1, 2, 3]]);
        let mut it = HashTrieIter::new(&trie);
        it.open(); // root table
        it.open(); // singleton frame at depth 1 (inner)
        assert!(it.lookup(h(2)));
        assert!(it.open()); // singleton frame at depth 2 (leaf)
        assert_eq!(it.key(), Some(h(3)));
        assert_eq!(it.leaf_tuples(), Some(&[vec![1, 2, 3]][..]));
    }

    #[test]
    fn pruned_and_plain_iterators_answer_lookups_identically() {
        /// Probes every present hash at this level, plus one absent hash,
        /// recording `(lookup result, key())` and descending on hits.
        fn probe(
            it: &mut dyn HashTrieIterator, arity: usize, depth: usize,
            out: &mut Vec<(bool, Option<u64>)>,
        ) {
            let mut present = Vec::new();
            while let Some(k) = it.key() {
                present.push(k);
                it.next();
            }
            present.sort();
            for probe_hash in present.into_iter().chain([h(999)]) {
                let hit = it.lookup(probe_hash);
                out.push((hit, it.key()));
                if hit && depth + 1 < arity {
                    assert!(it.open());
                    probe(it, arity, depth + 1, out);
                    it.up();
                }
            }
        }
        let tuples = vec![vec![1, 2, 3], vec![1, 2, 4], vec![1, 5, 6], vec![7, 8, 9]];
        let plain: HashTrie = HashTrie::from_tuples(3.into(), tuples.clone());
        let compact = Pruned::from_tuples(3.into(), tuples);
        let (mut a, mut b) = (Vec::new(), Vec::new());
        let mut ia = HashTrieIter::new(&plain);
        let mut ib = HashTrieIter::new(&compact);
        assert!(ia.open());
        assert!(ib.open());
        probe(&mut ia, 3, 0, &mut a);
        probe(&mut ib, 3, 0, &mut b);
        assert_eq!(a, b);
        assert!(!a.is_empty());
    }
}
