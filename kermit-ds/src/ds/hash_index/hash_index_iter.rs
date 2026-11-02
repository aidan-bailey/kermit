//! The [`KeyedIterable`] impl of [`HashIndex`]: a scan over the rows, and a
//! probe that walks one bucket's chain.

use {
    super::implementation::{HashIndex, NIL},
    kermit_iters::{HashStrategy, KeyedIterable, RowId, Tuples},
};

/// The rows a probe lends: one bucket's chain, each row kept only if its key
/// columns equal the probe's key. A key of no columns has no chains, so it
/// walks every row in order.
struct Probe<'a> {
    rows: &'a Tuples,
    /// The index's per-row links; unused (and empty) for a key of no columns.
    links: &'a [RowId],
    key: &'a [usize],
    /// The next row to consider, or `NIL`.
    at: RowId,
}

impl<'a> Iterator for Probe<'a> {
    type Item = &'a [usize];

    fn next(&mut self) -> Option<&'a [usize]> {
        let rows: &'a Tuples = self.rows;
        while self.at != NIL {
            let position = self.at as usize;
            let row = rows.row(self.at);
            self.at = if self.key.is_empty() {
                // No key columns, no chains: every row, in order.
                if position + 1 < rows.len() {
                    self.at + 1
                } else {
                    NIL
                }
            } else {
                self.links[position]
            };
            // Other keys share the bucket: only a row whose key columns equal
            // the probe's key is lent.
            if row.starts_with(self.key) {
                return Some(row);
            }
        }
        None
    }
}

impl<H: HashStrategy> KeyedIterable for HashIndex<H> {
    fn key_len(&self) -> usize { self.key_len }

    /// Every row, in source order.
    fn scan(&self) -> impl Iterator<Item = &[usize]> + '_ { self.rows.rows() }

    /// Hashes `key`, then walks the bucket's chain, which holds its rows in
    /// source order. Allocates nothing.
    fn probe<'a>(&'a self, key: &'a [usize]) -> impl Iterator<Item = &'a [usize]> + 'a {
        assert_eq!(
            key.len(),
            self.key_len,
            "probe: a key of {} values on an index keyed on {} columns",
            key.len(),
            self.key_len
        );
        let at = if self.key_len == 0 {
            if self.rows.is_empty() {
                NIL
            } else {
                0
            }
        } else {
            self.directory[self.bucket(key)]
        };
        Probe {
            rows: &self.rows,
            links: &self.next,
            key,
            at,
        }
    }
}
