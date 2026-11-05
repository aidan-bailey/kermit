//! The [`KeyedIterable`] impl of [`HashIndex`]: a scan over the rows, and a
//! probe that walks one bucket's chain.

use {
    super::implementation::{HashIndex, NIL},
    kermit_iters::{HashStrategy, KeyedIterable, RowId, Tuples},
};

/// The rows of one bucket's chain whose key columns equal the probe's key.
struct Chain<'a> {
    rows: &'a Tuples,
    /// The index's per-row links.
    links: &'a [RowId],
    key: &'a [usize],
    /// The next row of the chain, or `NIL`.
    at: RowId,
}

impl<'a> Iterator for Chain<'a> {
    type Item = &'a [usize];

    fn next(&mut self) -> Option<&'a [usize]> {
        while self.at != NIL {
            let row = self.rows.row(self.at);
            self.at = self.links[self.at as usize];
            // Other keys share the bucket: only a row whose key columns equal
            // the probe's key is lent.
            if row.starts_with(self.key) {
                return Some(row);
            }
        }
        None
    }
}

/// What a probe lends: every row of an index with no key columns, which has
/// no chains, or else the matching rows of one bucket's chain.
enum Probe<'a, Every> {
    Every(Every),
    Chain(Chain<'a>),
}

impl<'a, Every: Iterator<Item = &'a [usize]>> Iterator for Probe<'a, Every> {
    type Item = &'a [usize];

    fn next(&mut self) -> Option<&'a [usize]> {
        match self {
            | Self::Every(rows) => rows.next(),
            | Self::Chain(chain) => chain.next(),
        }
    }
}

impl<H: HashStrategy> KeyedIterable for HashIndex<H> {
    fn key_len(&self) -> usize { self.key_len }

    /// Every row, in source order.
    fn scan(&self) -> impl Iterator<Item = &[usize]> + '_ { self.rows.rows() }

    /// Hashes `key`, then walks the bucket's chain, which holds its rows in
    /// source order; an empty key lends every row instead. Allocates
    /// nothing.
    fn probe<'a>(&'a self, key: &'a [usize]) -> impl Iterator<Item = &'a [usize]> + 'a {
        assert_eq!(
            key.len(),
            self.key_len,
            "probe: a key of {} values on an index keyed on {} columns",
            key.len(),
            self.key_len
        );
        if key.is_empty() {
            return Probe::Every(self.rows.rows());
        }
        Probe::Chain(Chain {
            rows: &self.rows,
            links: &self.next,
            key,
            at: self.directory[self.bucket(key)],
        })
    }
}
