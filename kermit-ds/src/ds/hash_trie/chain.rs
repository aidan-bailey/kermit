//! [`Chain`]: Algorithm 2's linked list `L` (Freitag et al., VLDB 2020,
//! §3.2.2), threaded through one link per row (§3.3.2, Figure 3; #101 B2).
//!
//! A chain is three values stored in its bucket: its first row, its last
//! row and its length. Each row's successor lives in the trie's
//! [`ChainLinks`], one 4-byte link per row of its buffer, so pushing a row
//! rewrites two links and allocates nothing. Algorithm 2 moves each row
//! from its parent's list to exactly one child list by rewriting its link,
//! so one link per row serves every level.
//!
//! Two things that are kermit's:
//!
//! - The links are row ids in an array beside the buffer, 4 bytes each, where
//!   the paper reserves an 8-byte chain pointer in each materialised tuple.
//! - A push appends at the tail, where line 7 ("push t onto the linked list
//!   stored in B"), read literally, prepends. Appending keeps every list in
//!   input order, so the bulk build stays array-identical to the per-tuple
//!   build (#107).
//!
//! Every push writes `NIL` into the pushed row's link, whatever it held, so
//! a chain always ends in `NIL` and the links are a function of the lists
//! alone.
//!
//! `Chain` is nominally `pub` in this private module: the lazy expansion
//! policy's public `PendingChild` trait (`expansion.rs`) holds one, and a
//! `pub(super)` type there would trip `private_interfaces`, the reason that
//! trait is generic over its node type.
//!
//! The functions are `#[inline]`: the builds that call them are generic, so
//! they are compiled in the crate that names a `HashTrie`, and the
//! workspace builds without LTO.

use {
    super::{config::LoadFactor, hash_table::HashTable},
    kermit_iters::{ChainLinks, ChainWalk, LeafRows, RowId, Tuples},
    std::num::NonZeroU32,
};

/// A list of rows threaded through the trie's `links`, in input order:
/// Algorithm 2's `L`. The rows are ids into the trie's buffer; each row's
/// successor is its link, and the tail's link is `NIL`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Chain {
    /// The first row, where a walk starts.
    head: RowId,
    /// The last row, where a push appends. Its link is `NIL`.
    tail: RowId,
    /// The number of rows: Algorithm 2 line 3's `|L|`, the pruning test and
    /// `LeafRows::len`. Never 0, so `Option<Entry<Chain>>`, a leaf bucket,
    /// uses it as its niche and stays 24 bytes.
    len: NonZeroU32,
}

impl Chain {
    /// The chain of the one row `row`. Writes `NIL` into `row`'s link.
    #[inline]
    pub(super) fn start(links: &ChainLinks, row: RowId) -> Self {
        links.set(row, ChainLinks::NIL);
        Self {
            head: row,
            tail: row,
            len: NonZeroU32::MIN,
        }
    }

    /// Appends `row`: the tail's link becomes `row`, and `row`'s link
    /// becomes `NIL`, whatever it held. `row` must be in no other chain; a
    /// build moves each row to its next list only after the walk of its old
    /// list has read its link (see [`walk`](Self::walk)).
    #[inline]
    pub(super) fn push(&mut self, links: &ChainLinks, row: RowId) {
        links.set(self.tail, row);
        links.set(row, ChainLinks::NIL);
        self.tail = row;
        self.len = self
            .len
            .checked_add(1)
            .expect("a chain holds at most `u32::MAX` rows");
    }

    /// The rows, in chain order: [`ChainLinks::walk`] from the head,
    /// counted by the length. The walk reads each row's link before it
    /// yields the row, so the caller may push each row it is given onto
    /// another chain over the same `links` while the walk runs: Algorithm
    /// 2's re-threading of a bucket's list into the next level's (lines 4–7
    /// over line 10's list).
    #[inline]
    pub(super) fn walk<'l>(&self, links: &'l ChainLinks) -> ChainWalk<'l> {
        links.walk(self.head, self.len())
    }

    /// The number of rows, at least 1: a chain is never empty, so it has no
    /// `is_empty`.
    #[inline]
    pub(super) fn len(&self) -> usize { self.len.get() as usize }

    /// The first row: a pruned `Singleton`'s row when the chain holds one.
    #[inline]
    pub(super) fn head(&self) -> RowId { self.head }

    /// The chain as a leaf's rows (`HashTrieIterator::leaf_tuples`): the
    /// linked `LeafRows` over `tuples`, walked through `links`.
    #[inline]
    pub(super) fn leaf_rows<'a>(&self, tuples: &'a Tuples, links: &'a ChainLinks) -> LeafRows<'a> {
        LeafRows::linked(tuples, links, self.head, self.len())
    }
}

/// Algorithm 2 line 7, and `insert_at`'s leaf step: pushes `row` onto the
/// chain in `hash`'s bucket of `lists`, starting that chain if the bucket is
/// new (the table first grows under `load_factor` if a new key would
/// overfill it). Returns whether it started one, i.e. whether `hash` is a
/// new key of `lists`.
#[inline]
pub(super) fn push_row(
    lists: &mut HashTable<Chain>, hash: u64, load_factor: LoadFactor, links: &ChainLinks,
    row: RowId,
) -> bool {
    let mut fresh = false;
    let chain = lists.entry_or_insert_with(hash, load_factor, || {
        fresh = true;
        Chain::start(links, row)
    });
    if !fresh {
        chain.push(links, row);
    }
    fresh
}

#[cfg(test)]
mod tests {
    use super::*;

    const NIL: RowId = ChainLinks::NIL;

    /// The rows `chain` walks to, in order.
    fn walked(chain: &Chain, links: &ChainLinks) -> Vec<RowId> { chain.walk(links).collect() }

    /// `start` and `push` keep input order, and chains share one links
    /// array: two chains pushed in turn each walk their own rows.
    #[test]
    fn start_and_push_keep_input_order() {
        let links = ChainLinks::new(5);
        let mut chain = Chain::start(&links, 3);
        assert_eq!((chain.head(), chain.len()), (3, 1));
        for row in [1, 4, 0] {
            chain.push(&links, row);
        }
        assert_eq!((chain.head(), chain.len()), (3, 4));
        assert_eq!(walked(&chain, &links), vec![3, 1, 4, 0]);
        assert_eq!(chain.walk(&links).len(), 4, "an exact-size walk");
        let shared = ChainLinks::new(4);
        let mut a = Chain::start(&shared, 0);
        let mut b = Chain::start(&shared, 1);
        a.push(&shared, 2);
        b.push(&shared, 3);
        assert_eq!(walked(&a, &shared), vec![0, 2]);
        assert_eq!(walked(&b, &shared), vec![1, 3]);
    }

    /// `start` and every `push` write `NIL` into the new row's link,
    /// whatever it held, so every chain ends in `NIL` and the links are a
    /// function of the lists alone. Row 2's link is set to a stale
    /// successor first, as a row an earlier level threaded holds one.
    #[test]
    fn every_chain_ends_in_nil() {
        let links = ChainLinks::new(4);
        links.set(2, 3);
        let mut chain = Chain::start(&links, 1);
        assert_eq!(links.get(1), NIL, "start");
        chain.push(&links, 2);
        assert_eq!(links.get(1), 2, "the old tail links to the new row");
        assert_eq!(links.get(2), NIL, "push overwrote the stale link");
        chain.push(&links, 0);
        // Row 3 is in no chain, so its link keeps `new`'s zero.
        assert_eq!(links.to_vec(), vec![NIL, 2, 0, 0]);
        assert_eq!(walked(&chain, &links), vec![1, 2, 0]);
    }

    /// Algorithm 2's re-threading: a walk over one chain pushes every row
    /// it yields onto one of two chains over the same links, here by
    /// parity. Each push rewrites the yielded row's link, so the walk must
    /// have read that link before it yielded the row (`ChainWalk`'s rule);
    /// a walk that read it afterwards would follow the new chain, not the
    /// old one.
    #[test]
    fn rethreading_a_chain_visits_every_row() {
        let links = ChainLinks::new(6);
        let mut all = Chain::start(&links, 0);
        for row in 1..6 {
            all.push(&links, row);
        }
        let mut lists: HashTable<Chain> = HashTable::new();
        for row in all.walk(&links) {
            push_row(
                &mut lists,
                u64::from(row % 2),
                LoadFactor::DEFAULT,
                &links,
                row,
            );
        }
        let parity = |hash: u64| walked(lists.get(hash).expect("both parities"), &links);
        assert_eq!(parity(0), vec![0, 2, 4]);
        assert_eq!(parity(1), vec![1, 3, 5]);
        assert_eq!(links.to_vec(), vec![2, 3, 4, 5, NIL, NIL]);
    }

    #[test]
    fn push_row_returns_whether_it_started_a_chain() {
        let links = ChainLinks::new(3);
        let mut lists: HashTable<Chain> = HashTable::new();
        assert!(
            push_row(&mut lists, 5, LoadFactor::DEFAULT, &links, 0),
            "a new key"
        );
        assert!(
            !push_row(&mut lists, 5, LoadFactor::DEFAULT, &links, 1),
            "a key it has"
        );
        assert!(
            push_row(&mut lists, 6, LoadFactor::DEFAULT, &links, 2),
            "another new key"
        );
        assert_eq!(lists.len(), 2);
        assert_eq!(walked(lists.get(5).expect("pushed"), &links), vec![0, 1]);
        assert_eq!(walked(lists.get(6).expect("pushed"), &links), vec![2]);
    }

    /// A leaf bucket holds its hash and its chain inline: 8 + 12 bytes,
    /// padded to 24, with `len`'s `NonZeroU32` as the `Option`'s niche. A
    /// `Vec<RowId>` list made it 32, plus a heap allocation per list.
    #[test]
    fn a_leaf_bucket_is_24_bytes() {
        use {super::super::hash_table::Entry, std::mem::size_of};
        assert_eq!(size_of::<Chain>(), 12);
        assert_eq!(size_of::<Option<Entry<Chain>>>(), 24);
        assert_eq!(size_of::<Option<Entry<Vec<RowId>>>>(), 32);
    }

    /// A chain lends its rows as a linked `LeafRows` over the buffer, in
    /// chain order, the rows borrowed, not copied.
    #[test]
    fn leaf_rows_lends_the_chains_rows() {
        let tuples = Tuples::from(vec![vec![1, 2], vec![3, 4], vec![5, 6], vec![7, 8]]);
        let links = ChainLinks::new(tuples.len());
        let mut chain = Chain::start(&links, 2);
        chain.push(&links, 0);
        chain.push(&links, 3);
        let leaf = chain.leaf_rows(&tuples, &links);
        assert_eq!(leaf.len(), 3);
        assert_eq!(leaf.ids().collect::<Vec<_>>(), vec![2, 0, 3]);
        assert_eq!(leaf.to_vecs(), vec![vec![5, 6], vec![1, 2], vec![7, 8]]);
        assert_eq!(leaf.data().as_ptr(), tuples.as_flat().as_ptr());
    }
}
