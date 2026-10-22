//! [`LeafRows`]: the rows of one hash-trie leaf chain, read from the batch
//! that holds them, and [`LeafCursor`], a position in one.

use {
    crate::{
        chain_links::{ChainLinks, ChainWalk},
        tuples::{RowId, Tuples},
    },
    std::{fmt, iter::Copied, slice},
};

/// A leaf chain's tuples: the chain's rows, in chain order, read from the
/// row-major buffer that holds them ([`Tuples`]'s layout).
///
/// A `HashTrie` keeps its relation's batch and names each tuple by its row
/// id (#111). A view names its chain's rows in one of two forms:
/// - **listed**: a slice of their ids ([`new`](Self::new),
///   [`from_parts`](Self::from_parts)), for a view that holds its own ids, such
///   as the one row id of a pruned singleton frame;
/// - **linked**: the chain's first row and length, the rest reached through the
///   batch's [`ChainLinks`] ([`linked`](Self::linked)), for a list threaded
///   through its trie's links (VLDB 2020 §3.3.2).
///
/// Either way it is a few borrows and two words, nothing copied. The rows it
/// lends borrow the buffer, not the view, so they outlive it.
///
/// Rows are read in chain order, by [`iter`](Self::iter) or through a
/// [`LeafCursor`]: [`first`](Self::first), [`advance`](Self::advance) and
/// [`row_at`](Self::row_at), each O(1). There is no random access, since the
/// `i`-th row of a linked chain is `i` links away. `Debug` prints the rows.
///
/// The accessors are `#[inline]`: this crate is separate from its callers and
/// the workspace builds without LTO, and a join's leaf loop calls
/// [`row_at`](Self::row_at) once per candidate.
///
/// ```
/// use kermit_iters::{ChainLinks, LeafRows, RowId, Tuples};
///
/// let tuples = Tuples::from(vec![vec![1, 2], vec![3, 4], vec![5, 6]]);
/// // Listed: the ids themselves.
/// let chain: [RowId; 2] = [2, 0];
/// let listed = LeafRows::new(&tuples, &chain);
/// // Linked: the same chain, threaded through one link per row.
/// let links = ChainLinks::new(tuples.len());
/// links.set(2, 0);
/// links.set(0, ChainLinks::NIL);
/// let linked = LeafRows::linked(&tuples, &links, 2, 2);
/// for leaf in [listed, linked] {
///     assert_eq!(leaf.len(), 2);
///     let mut at = leaf.first();
///     assert_eq!(leaf.row_at(at), [5, 6]);
///     assert!(leaf.advance(&mut at));
///     assert_eq!(leaf.row_at(at), [1, 2]);
///     assert!(!leaf.advance(&mut at));
///     assert_eq!(leaf.to_vecs(), vec![vec![5, 6], vec![1, 2]]);
/// }
/// ```
#[derive(Clone, Copy)]
pub struct LeafRows<'a> {
    data: &'a [usize],
    arity: usize,
    ids: Ids<'a>,
}

/// How a [`LeafRows`] names its chain's rows.
#[derive(Clone, Copy)]
enum Ids<'a> {
    /// The ids themselves, in chain order.
    Listed(&'a [RowId]),
    /// A chain threaded through `links`: `len` rows, the first `head`.
    Linked {
        links: &'a ChainLinks,
        head: RowId,
        len: usize,
    },
}

impl<'a> LeafRows<'a> {
    /// The rows `rows` of `tuples`, in the order the ids are listed.
    #[inline]
    pub fn new(tuples: &'a Tuples, rows: &'a [RowId]) -> Self {
        Self::from_parts(tuples.as_flat(), tuples.arity(), rows)
    }

    /// The rows `rows` of the row-major buffer `data`, whose rows hold
    /// `arity` values each: a listed view over a buffer that is not a whole
    /// [`Tuples`].
    ///
    /// `data.len()` must be a multiple of `arity` (empty when `arity` is 0),
    /// which a debug build asserts in O(1). The ids are not checked here:
    /// that would cost O(chain) per candidate. Reading a row whose id is past
    /// the end of `data` panics.
    #[inline]
    pub fn from_parts(data: &'a [usize], arity: usize, rows: &'a [RowId]) -> Self {
        // At arity 0 only the empty buffer is a multiple of it.
        debug_assert!(
            data.len().is_multiple_of(arity),
            "LeafRows::from_parts: {} values are not whole rows of arity {arity}",
            data.len()
        );
        Self {
            data,
            arity,
            ids: Ids::Listed(rows),
        }
    }

    /// The `len` rows of `tuples` on the chain that starts at row `head`,
    /// threaded through `links`, in chain order: a hash trie's leaf chain,
    /// read along its links. `links` must hold one link per row of `tuples`,
    /// which a debug build asserts in O(1), and the chain must end at its
    /// `len`-th row, whose link is [`ChainLinks::NIL`]. `head` is not read
    /// when `len` is 0.
    #[inline]
    pub fn linked(tuples: &'a Tuples, links: &'a ChainLinks, head: RowId, len: usize) -> Self {
        debug_assert_eq!(
            links.len(),
            tuples.len(),
            "LeafRows::linked: {} links for {} rows",
            links.len(),
            tuples.len()
        );
        Self {
            data: tuples.as_flat(),
            arity: tuples.arity(),
            ids: Ids::Linked {
                links,
                head,
                len,
            },
        }
    }

    /// The number of rows in the chain.
    #[inline]
    pub fn len(&self) -> usize {
        match self.ids {
            | Ids::Listed(ids) => ids.len(),
            | Ids::Linked {
                len, ..
            } => len,
        }
    }

    /// Whether the chain holds no rows.
    #[inline]
    pub fn is_empty(&self) -> bool {
        match self.ids {
            | Ids::Listed(ids) => ids.is_empty(),
            | Ids::Linked {
                len, ..
            } => len == 0,
        }
    }

    /// A cursor at the chain's first row. An empty chain's is at no row, so
    /// check [`is_empty`](Self::is_empty) before reading through it.
    #[inline]
    pub fn first(&self) -> LeafCursor {
        match self.ids {
            | Ids::Listed(_) => LeafCursor(0),
            | Ids::Linked {
                head, ..
            } => LeafCursor(head as usize),
        }
    }

    /// Moves `cursor` to the chain's next row and returns `true`, or, from
    /// the last row, past the end and returns `false`. One link read for a
    /// linked chain, which ends at the row whose link is
    /// [`ChainLinks::NIL`]. [`first`](Self::first) restarts a cursor past
    /// the end; advancing it again instead is a bug (on a linked chain it
    /// panics).
    #[inline]
    pub fn advance(&self, cursor: &mut LeafCursor) -> bool {
        match self.ids {
            | Ids::Listed(ids) => {
                cursor.0 += 1;
                cursor.0 < ids.len()
            },
            | Ids::Linked {
                links, ..
            } => {
                let next = links.get(cursor.0 as RowId);
                cursor.0 = next as usize;
                next != ChainLinks::NIL
            },
        }
    }

    /// The row `cursor` is at.
    ///
    /// # Panics
    ///
    /// Panics if `cursor` is past the end of a listed chain, or if the row it
    /// names is past the buffer's last row, which a linked chain's
    /// past-the-end cursor ([`ChainLinks::NIL`]) is at every arity from 1. At
    /// arity 0 every id reads the one empty row, so that cursor reads it too.
    #[inline]
    pub fn row_at(&self, cursor: LeafCursor) -> &'a [usize] {
        let id = match self.ids {
            | Ids::Listed(ids) => ids[cursor.0],
            | Ids::Linked {
                ..
            } => cursor.0 as RowId,
        };
        row_of(self.data, self.arity, id)
    }

    /// The chain's row ids, in chain order.
    #[inline]
    pub fn ids(&self) -> impl ExactSizeIterator<Item = RowId> + 'a {
        match self.ids {
            | Ids::Listed(ids) => IdIter::Listed(ids.iter().copied()),
            | Ids::Linked {
                links,
                head,
                len,
            } => IdIter::Linked(links.walk(head, len)),
        }
    }

    /// The buffer the ids index.
    #[inline]
    pub fn data(&self) -> &'a [usize] { self.data }

    /// The number of values in each row.
    #[inline]
    pub fn arity(&self) -> usize { self.arity }

    /// Every row of the chain, in chain order.
    #[inline]
    pub fn iter(&self) -> impl ExactSizeIterator<Item = &'a [usize]> + 'a {
        let (data, arity) = (self.data, self.arity);
        self.ids().map(move |r| row_of(data, arity, r))
    }

    /// Every row as its own `Vec`, for tests and diagnostics.
    pub fn to_vecs(&self) -> Vec<Vec<usize>> { self.iter().map(<[usize]>::to_vec).collect() }
}

/// The chain's rows, in chain order: a linked view prints its own rows,
/// never its trie's whole links array.
impl fmt::Debug for LeafRows<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.iter()).finish()
    }
}

/// A position in a [`LeafRows`] chain: an index into a listed chain, the
/// current row's id in a linked one. [`LeafRows::first`] makes one,
/// [`LeafRows::advance`] moves it and [`LeafRows::row_at`] reads through it,
/// each in O(1). It is `Copy`, so a join keeps one per relation in its
/// scratch.
///
/// A cursor means something only for the chain it came from, through any
/// view of that chain: a trie lends a fresh `LeafRows` of the same chain each
/// time it is asked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LeafCursor(usize);

/// [`LeafRows::ids`]: a listed chain's ids, or a linked chain's walk.
enum IdIter<'a> {
    Listed(Copied<slice::Iter<'a, RowId>>),
    Linked(ChainWalk<'a>),
}

impl Iterator for IdIter<'_> {
    type Item = RowId;

    #[inline]
    fn next(&mut self) -> Option<RowId> {
        match self {
            | Self::Listed(ids) => ids.next(),
            | Self::Linked(walk) => walk.next(),
        }
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        match self {
            | Self::Listed(ids) => ids.size_hint(),
            | Self::Linked(walk) => walk.size_hint(),
        }
    }
}

impl ExactSizeIterator for IdIter<'_> {}

/// Row `r` of the row-major `data`, whose rows hold `arity` values.
#[inline]
fn row_of(data: &[usize], arity: usize, r: RowId) -> &[usize] {
    let r = r as usize;
    &data[r * arity..(r + 1) * arity]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn batch() -> Tuples { Tuples::from(vec![vec![1, 2], vec![3, 4], vec![5, 6], vec![7, 8]]) }

    /// `ids` threaded into one chain over `rows` links, in that order, as a
    /// trie's pushes leave it: each link names the next id and the last is
    /// `NIL`. Rows not in `ids` keep link 0.
    fn thread(rows: usize, ids: &[RowId]) -> ChainLinks {
        let links = ChainLinks::new(rows);
        for pair in ids.windows(2) {
            links.set(pair[0], pair[1]);
        }
        if let Some(&tail) = ids.last() {
            links.set(tail, ChainLinks::NIL);
        }
        links
    }

    /// The chain `ids` of `tuples` in both forms: listed, and linked through
    /// `links` (`thread(tuples.len(), ids)`).
    fn both<'a>(tuples: &'a Tuples, ids: &'a [RowId], links: &'a ChainLinks) -> [LeafRows<'a>; 2] {
        let head = ids.first().copied().unwrap_or(ChainLinks::NIL);
        [
            LeafRows::new(tuples, ids),
            LeafRows::linked(tuples, links, head, ids.len()),
        ]
    }

    #[test]
    fn a_chain_reads_its_rows_in_chain_order() {
        let tuples = batch();
        // Not a palindrome, so a reversed `iter` shows.
        let ids: [RowId; 3] = [2, 0, 3];
        let links = thread(tuples.len(), &ids);
        for leaf in both(&tuples, &ids, &links) {
            assert_eq!(leaf.len(), 3);
            assert!(!leaf.is_empty());
            assert_eq!(leaf.ids().len(), 3);
            assert_eq!(leaf.ids().collect::<Vec<_>>(), [2, 0, 3]);
            assert_eq!(leaf.arity(), 2);
            assert_eq!(leaf.data(), tuples.as_flat());
            assert_eq!(leaf.iter().len(), 3);
            let rows: Vec<&[usize]> = leaf.iter().collect();
            assert_eq!(rows, [&[5, 6][..], &[1, 2][..], &[7, 8][..]]);
            assert_eq!(leaf.to_vecs(), vec![vec![5, 6], vec![1, 2], vec![7, 8]]);
            // A view is `Copy`: borrows and two words, nothing owned.
            let copy = leaf;
            assert_eq!(copy.to_vecs(), leaf.to_vecs());
        }
    }

    /// `first`, `advance` and `row_at` walk a many-row chain in chain order,
    /// in either form, and `first` restarts a cursor that ran past the end.
    #[test]
    fn a_cursor_walks_a_chain_of_many_rows() {
        let tuples = batch();
        let ids: [RowId; 3] = [2, 0, 3];
        let links = thread(tuples.len(), &ids);
        for leaf in both(&tuples, &ids, &links) {
            let mut at = leaf.first();
            assert_eq!(leaf.row_at(at), [5, 6]);
            assert!(leaf.advance(&mut at));
            assert_eq!(leaf.row_at(at), [1, 2]);
            assert!(leaf.advance(&mut at));
            assert_eq!(leaf.row_at(at), [7, 8]);
            assert!(!leaf.advance(&mut at), "past the last row");
            at = leaf.first();
            assert_eq!(at, leaf.first());
            assert_eq!(leaf.row_at(at), [5, 6]);
        }
    }

    /// On a one-row chain the first row is the last: `advance` reports the
    /// end at once.
    #[test]
    fn a_cursor_on_a_one_row_chain_starts_at_its_last_row() {
        let tuples = batch();
        let ids: [RowId; 1] = [1];
        let links = thread(tuples.len(), &ids);
        for leaf in both(&tuples, &ids, &links) {
            let mut at = leaf.first();
            assert_eq!(leaf.row_at(at), [3, 4]);
            assert!(!leaf.advance(&mut at));
            assert_eq!(leaf.to_vecs(), vec![vec![3, 4]]);
        }
    }

    #[test]
    fn an_empty_chain_lends_nothing() {
        let tuples = batch();
        let links = thread(tuples.len(), &[]);
        for leaf in both(&tuples, &[], &links) {
            assert_eq!(leaf.len(), 0);
            assert!(leaf.is_empty());
            assert_eq!(leaf.ids().len(), 0);
            assert_eq!(leaf.ids().next(), None);
            assert_eq!(leaf.iter().next(), None);
            assert!(leaf.to_vecs().is_empty());
        }
    }

    /// An empty chain's `first` is at no row: reading through it panics,
    /// so a reader checks `is_empty` first, as `emit_leaf` does.
    #[test]
    #[should_panic(expected = "index out of bounds")]
    fn the_first_row_of_an_empty_listed_chain_panics() {
        let tuples = batch();
        let leaf = LeafRows::new(&tuples, &[]);
        let _ = leaf.row_at(leaf.first());
    }

    #[test]
    #[should_panic(expected = "out of range for slice of length 8")]
    fn the_first_row_of_an_empty_linked_chain_panics() {
        let tuples = batch();
        let links = ChainLinks::new(tuples.len());
        let leaf = LeafRows::linked(&tuples, &links, ChainLinks::NIL, 0);
        let _ = leaf.row_at(leaf.first());
    }

    #[test]
    #[should_panic(expected = "index out of bounds")]
    fn a_row_past_a_listed_chain_panics() {
        let tuples = batch();
        let ids: [RowId; 2] = [0, 1];
        let leaf = LeafRows::new(&tuples, &ids);
        let mut at = leaf.first();
        assert!(leaf.advance(&mut at));
        assert!(!leaf.advance(&mut at));
        let _ = leaf.row_at(at);
    }

    /// Past its end a linked cursor is `NIL`, which names no row of the
    /// buffer.
    #[test]
    #[should_panic(expected = "out of range for slice of length 8")]
    fn a_row_past_a_linked_chain_panics() {
        let tuples = batch();
        let ids: [RowId; 2] = [0, 1];
        let links = thread(tuples.len(), &ids);
        let leaf = LeafRows::linked(&tuples, &links, 0, 2);
        let mut at = leaf.first();
        assert!(leaf.advance(&mut at));
        assert!(!leaf.advance(&mut at));
        let _ = leaf.row_at(at);
    }

    #[test]
    fn from_parts_views_the_same_rows() {
        let tuples = batch();
        let ids: [RowId; 2] = [3, 1];
        let whole = LeafRows::new(&tuples, &ids);
        let parts = LeafRows::from_parts(tuples.as_flat(), tuples.arity(), &ids);
        assert_eq!(parts.to_vecs(), whole.to_vecs());
        assert_eq!((parts.data(), parts.arity()), (whole.data(), whole.arity()));
        assert!(parts.ids().eq(whole.ids()));
    }

    /// One value held in a bare buffer, with no batch behind it.
    #[test]
    fn a_view_needs_no_batch() {
        let value = [7];
        let only: [RowId; 1] = [0];
        let leaf = LeafRows::from_parts(&value, 1, &only);
        assert_eq!(leaf.to_vecs(), vec![vec![7]]);
    }

    /// One links array threads every chain of a trie: two chains over one
    /// batch interleave, and each linked view follows its own.
    #[test]
    fn linked_chains_share_one_links_array() {
        let tuples = batch();
        let links = ChainLinks::new(tuples.len());
        for (row, next) in [(0, 2), (2, ChainLinks::NIL), (3, 1), (1, ChainLinks::NIL)] {
            links.set(row, next);
        }
        let first = LeafRows::linked(&tuples, &links, 0, 2);
        let second = LeafRows::linked(&tuples, &links, 3, 2);
        assert_eq!(first.to_vecs(), vec![vec![1, 2], vec![5, 6]]);
        assert_eq!(second.to_vecs(), vec![vec![7, 8], vec![3, 4]]);
        assert_eq!(second.ids().collect::<Vec<_>>(), [3, 1]);
    }

    /// `Debug` prints the chain's rows in chain order, in either form: not
    /// the ids, and never a linked view's whole links array.
    #[test]
    fn debug_prints_the_rows_of_the_chain() {
        let tuples = batch();
        let ids: [RowId; 2] = [2, 0];
        let links = thread(tuples.len(), &ids);
        for leaf in both(&tuples, &ids, &links) {
            assert_eq!(format!("{leaf:?}"), "[[5, 6], [1, 2]]");
        }
        let links = thread(tuples.len(), &[]);
        for leaf in both(&tuples, &[], &links) {
            assert_eq!(format!("{leaf:?}"), "[]");
        }
    }

    #[test]
    #[should_panic(expected = "range end index 4 out of range for slice of length 2")]
    fn an_id_past_the_buffer_panics() {
        let leaf = LeafRows::from_parts(&[1, 2], 2, &[1]);
        let _ = leaf.row_at(leaf.first());
    }

    /// Whole rows only: three values are not rows of arity 2. Asserted in
    /// debug builds only.
    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "3 values are not whole rows of arity 2")]
    fn a_buffer_of_partial_rows_panics_in_debug() {
        let _ = LeafRows::from_parts(&[1, 2, 3], 2, &[]);
    }

    /// A linked view reads its rows' links from an array of one link per row
    /// of its batch. Asserted in debug builds only.
    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "LeafRows::linked: 3 links for 4 rows")]
    fn a_linked_view_needs_a_link_per_row_in_debug() {
        let tuples = batch();
        let links = ChainLinks::new(3);
        let _ = LeafRows::linked(&tuples, &links, 0, 0);
    }

    /// At arity 0 every id reads the one empty row, in either form.
    #[test]
    fn a_nullary_chain_reads_empty_rows() {
        let leaf = LeafRows::from_parts(&[], 0, &[5, 9]);
        assert_eq!(leaf.to_vecs(), vec![Vec::<usize>::new(); 2]);
        let tuples = Tuples::from_flat(0, 3, Vec::new());
        let links = thread(tuples.len(), &[2, 0]);
        let linked = LeafRows::linked(&tuples, &links, 2, 2);
        assert_eq!(linked.to_vecs(), vec![Vec::<usize>::new(); 2]);
        assert_eq!(linked.ids().collect::<Vec<_>>(), [2, 0]);
    }

    /// Rows, ids and the buffer borrow the buffer (and a linked view's
    /// links), not the view: a caller keeps them after the view it read them
    /// through is gone, in either form. This compiles only if `row_at`,
    /// `iter`, `ids` and `data` lend `'a`, not a borrow of the view.
    #[test]
    fn rows_outlive_the_view() {
        fn check<'a>(view: impl Fn() -> LeafRows<'a>) {
            let row = {
                let leaf = view();
                leaf.row_at(leaf.first())
            };
            let all: Vec<&[usize]> = {
                let leaf = view();
                leaf.iter().collect()
            };
            let (data, mut ids) = {
                let leaf = view();
                (leaf.data(), leaf.ids())
            };
            let mut rows = {
                let leaf = view();
                leaf.iter()
            };
            assert_eq!(row, [7, 8]);
            assert_eq!(all, [row]);
            assert_eq!(ids.next(), Some(3));
            assert_eq!(rows.next(), Some(row));
            assert_eq!(LeafRows::from_parts(data, 2, &[3]).to_vecs(), vec![vec![
                7, 8
            ]]);
        }
        let tuples = batch();
        let ids: [RowId; 1] = [3];
        let links = thread(tuples.len(), &ids);
        check(|| LeafRows::new(&tuples, &ids));
        check(|| LeafRows::linked(&tuples, &links, 3, 1));
    }
}
