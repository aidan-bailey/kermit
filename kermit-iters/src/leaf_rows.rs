//! [`LeafRows`]: the rows of one hash-trie leaf chain, read through their
//! row ids from the batch that holds them.

use crate::tuples::{RowId, Tuples};

/// A leaf chain's tuples: the ids of its rows, in chain order, and the
/// row-major buffer they index ([`Tuples`]'s layout).
///
/// A `HashTrie` keeps its relation's batch and stores each leaf chain as
/// row ids into it (#111). A `LeafRows` reads such a chain: two borrows and
/// an arity, nothing copied. The rows it lends borrow the buffer, not the
/// view, so they outlive it.
///
/// The accessors are `#[inline]`: this crate is separate from its callers and
/// the workspace builds without LTO, and a join's leaf loop calls
/// [`row`](Self::row) once per candidate.
///
/// ```
/// use kermit_iters::{LeafRows, RowId, Tuples};
///
/// let tuples = Tuples::from(vec![vec![1, 2], vec![3, 4], vec![5, 6]]);
/// let chain: [RowId; 2] = [2, 0];
/// let leaf = LeafRows::new(&tuples, &chain);
/// assert_eq!(leaf.len(), 2);
/// assert_eq!(leaf.row(0), [5, 6]);
/// assert_eq!(leaf.to_vecs(), vec![vec![5, 6], vec![1, 2]]);
/// ```
#[derive(Clone, Copy, Debug)]
pub struct LeafRows<'a> {
    data: &'a [usize],
    arity: usize,
    rows: &'a [RowId],
}

impl<'a> LeafRows<'a> {
    /// The rows `rows` of `tuples`, in the order the ids are listed.
    #[inline]
    pub fn new(tuples: &'a Tuples, rows: &'a [RowId]) -> Self {
        Self::from_parts(tuples.as_flat(), tuples.arity(), rows)
    }

    /// The rows `rows` of the row-major buffer `data`, whose rows hold
    /// `arity` values each: a view over a buffer that is not a whole
    /// [`Tuples`], such as one value held inline.
    ///
    /// `data.len()` must be a multiple of `arity` (empty when `arity` is 0),
    /// which a debug build asserts in O(1). The ids are not checked here:
    /// that would cost O(chain) per candidate. Reading a row whose id is past
    /// the end of `data` panics.
    #[inline]
    pub fn from_parts(data: &'a [usize], arity: usize, rows: &'a [RowId]) -> Self {
        debug_assert!(
            if arity == 0 {
                data.is_empty()
            } else {
                data.len() % arity == 0
            },
            "LeafRows::from_parts: {} values are not whole rows of arity {arity}",
            data.len()
        );
        Self {
            data,
            arity,
            rows,
        }
    }

    /// The number of rows in the chain.
    #[inline]
    pub fn len(&self) -> usize { self.rows.len() }

    /// Whether the chain holds no rows.
    #[inline]
    pub fn is_empty(&self) -> bool { self.rows.is_empty() }

    /// The chain's `i`-th row.
    ///
    /// # Panics
    ///
    /// Panics if `i` is not below [`len`](Self::len), or if the row's id is
    /// past the buffer's last row. At arity 0 every id reads the one empty
    /// row, so the second case cannot arise.
    #[inline]
    pub fn row(&self, i: usize) -> &'a [usize] { row_of(self.data, self.arity, self.rows[i]) }

    /// The chain's row ids, in chain order.
    #[inline]
    pub fn ids(&self) -> &'a [RowId] { self.rows }

    /// The buffer the ids index.
    #[inline]
    pub fn data(&self) -> &'a [usize] { self.data }

    /// The number of values in each row.
    #[inline]
    pub fn arity(&self) -> usize { self.arity }

    /// Every row of the chain, in chain order.
    #[inline]
    pub fn iter(&self) -> impl ExactSizeIterator<Item = &'a [usize]> + 'a {
        let (data, arity, rows) = (self.data, self.arity, self.rows);
        rows.iter().map(move |&r| row_of(data, arity, r))
    }

    /// Every row as its own `Vec`, for tests and diagnostics.
    pub fn to_vecs(&self) -> Vec<Vec<usize>> { self.iter().map(<[usize]>::to_vec).collect() }
}

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

    #[test]
    fn a_chain_reads_its_rows_in_chain_order() {
        let tuples = batch();
        // Not a palindrome, so a reversed `iter` shows.
        let ids: [RowId; 3] = [2, 0, 3];
        let leaf = LeafRows::new(&tuples, &ids);
        assert_eq!(leaf.len(), 3);
        assert!(!leaf.is_empty());
        assert_eq!(leaf.row(0), [5, 6]);
        assert_eq!(leaf.row(1), [1, 2]);
        assert_eq!(leaf.row(2), [7, 8]);
        assert_eq!(leaf.ids(), [2, 0, 3]);
        assert_eq!(leaf.arity(), 2);
        assert_eq!(leaf.data(), tuples.as_flat());
        assert_eq!(leaf.iter().len(), 3);
        let rows: Vec<&[usize]> = leaf.iter().collect();
        assert_eq!(rows, [leaf.row(0), leaf.row(1), leaf.row(2)]);
        assert_eq!(leaf.to_vecs(), vec![vec![5, 6], vec![1, 2], vec![7, 8]]);
        // A view is `Copy`: two borrows and an arity, nothing owned.
        let copy = leaf;
        assert_eq!(copy.to_vecs(), leaf.to_vecs());
    }

    #[test]
    fn from_parts_views_the_same_rows() {
        let tuples = batch();
        let ids: [RowId; 2] = [3, 1];
        let whole = LeafRows::new(&tuples, &ids);
        let parts = LeafRows::from_parts(tuples.as_flat(), tuples.arity(), &ids);
        assert_eq!(parts.to_vecs(), whole.to_vecs());
        assert_eq!(
            (parts.data(), parts.arity(), parts.ids()),
            (whole.data(), whole.arity(), whole.ids())
        );
    }

    /// One value held inline, with no batch behind it (the constant view's
    /// leaf in `kermit-algos`).
    #[test]
    fn a_view_needs_no_batch() {
        let value = [7];
        let only: [RowId; 1] = [0];
        let leaf = LeafRows::from_parts(&value, 1, &only);
        assert_eq!(leaf.to_vecs(), vec![vec![7]]);
    }

    #[test]
    fn an_empty_chain_lends_nothing() {
        let tuples = batch();
        let leaf = LeafRows::new(&tuples, &[]);
        assert_eq!(leaf.len(), 0);
        assert!(leaf.is_empty());
        assert_eq!(leaf.iter().next(), None);
        assert!(leaf.to_vecs().is_empty());
    }

    #[test]
    #[should_panic(expected = "index out of bounds")]
    fn a_row_past_the_chain_panics() {
        let tuples = batch();
        let ids: [RowId; 2] = [0, 1];
        let _ = LeafRows::new(&tuples, &ids).row(2);
    }

    #[test]
    #[should_panic(expected = "range end index 4 out of range for slice of length 2")]
    fn an_id_past_the_buffer_panics() { let _ = LeafRows::from_parts(&[1, 2], 2, &[1]).row(0); }

    /// Whole rows only: three values are not rows of arity 2. Asserted in
    /// debug builds only.
    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "3 values are not whole rows of arity 2")]
    fn a_buffer_of_partial_rows_panics_in_debug() {
        let _ = LeafRows::from_parts(&[1, 2, 3], 2, &[]);
    }

    /// At arity 0 every id reads the one empty row.
    #[test]
    fn a_nullary_chain_reads_empty_rows() {
        let leaf = LeafRows::from_parts(&[], 0, &[5, 9]);
        assert_eq!(leaf.to_vecs(), vec![Vec::<usize>::new(); 2]);
    }

    /// Rows, ids and the buffer borrow the buffer, not the view: a caller keeps
    /// them after the view it read them through is gone. This compiles only if
    /// `row`, `iter`, `data` and `ids` lend `'a` references.
    #[test]
    fn rows_outlive_the_view() {
        let tuples = batch();
        let ids: [RowId; 1] = [3];
        let row = {
            let leaf = LeafRows::new(&tuples, &ids);
            leaf.row(0)
        };
        let all: Vec<&[usize]> = {
            let leaf = LeafRows::new(&tuples, &ids);
            leaf.iter().collect()
        };
        let (data, chain) = {
            let leaf = LeafRows::new(&tuples, &ids);
            (leaf.data(), leaf.ids())
        };
        assert_eq!(row, [7, 8]);
        assert_eq!(all, [row]);
        assert_eq!(LeafRows::from_parts(data, 2, chain).row(0), [7, 8]);
    }
}
