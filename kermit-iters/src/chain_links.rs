//! [`ChainLinks`]: one link per row of a tuple batch, which threads a hash
//! trie's lists through the batch, and [`ChainWalk`], one list's rows in
//! chain order.

use {
    crate::tuples::RowId,
    std::sync::atomic::{AtomicU32, Ordering},
};

/// One link per row of a tuple batch: the id of the next row in that row's
/// list, or [`NIL`](Self::NIL).
///
/// A hash trie's lists are chains threaded through these links: Algorithm
/// 2's per-bucket lists, the leaf chains and a lazy child's pending rows.
/// This is the paper's tuple chain pointer (Freitag et al., VLDB 2020,
/// §3.3.2 and Figure 3): a list names its first row, each row's link names
/// the next, and a row moves to another list when its link is rewritten, as
/// Algorithm 2 moves "each tuple in the input linked list L … to exactly one
/// of the linked lists that are processed recursively" (§3.2.2). A build
/// therefore allocates one array of links, never a list per bucket. The
/// paper reserves 8 bytes inside each tuple; a 4-byte [`RowId`] in an array
/// beside the batch is kermit's.
///
/// **Why atomics.** Only so that lazy child expansion can re-thread a
/// child's rows through a shared reference (`&self`, inside an iterator's
/// `open`) without `unsafe`. Every access is `Relaxed`. No two threads ever
/// write one link: every row is in exactly one list at a time, and one
/// thread threads each list. Where a parallel build's workers thread rows,
/// the scoped spawn and join that start and end them order every write
/// before the reads that follow. On x86-64 and AArch64 a relaxed load or
/// store compiles to a plain one.
///
/// **`NIL` is never a row.** A batch holds at most `u32::MAX` rows
/// ([`Tuples`](crate::Tuples)), so the largest id is `u32::MAX − 1`.
///
/// ```
/// use kermit_iters::{ChainLinks, RowId};
///
/// // Rows 0, 2 and 3 of a four-row batch, threaded into one chain through a
/// // shared reference.
/// let links = ChainLinks::new(4);
/// links.set(0, 2);
/// links.set(2, 3);
/// links.set(3, ChainLinks::NIL);
/// let chain: Vec<RowId> = links.walk(0, 3).collect();
/// assert_eq!(chain, [0, 2, 3]);
/// ```
#[derive(Debug, Default)]
pub struct ChainLinks(Vec<AtomicU32>);

impl ChainLinks {
    /// The link of a list's last row: there is no next row.
    pub const NIL: RowId = RowId::MAX;

    /// `rows` links, all zero, for a build that threads every row before it
    /// reads any link: Algorithm 2 pushes every row onto a list at the root
    /// first, so the starting values are never observed.
    ///
    /// `vec![0; rows]` asks the allocator for zeroed memory (`alloc_zeroed`),
    /// and the `map` into atomics is collected in place: `AtomicU32` has
    /// `u32`'s size and alignment, so the allocation is reused.
    pub fn new(rows: usize) -> Self {
        Self(vec![0u32; rows].into_iter().map(AtomicU32::new).collect())
    }

    /// No links, with room for `rows`: for the builds that place one row at
    /// a time and [`push`](Self::push) its link first.
    pub fn with_capacity(rows: usize) -> Self { Self(Vec::with_capacity(rows)) }

    /// The number of links: one per row of the batch they thread.
    pub fn len(&self) -> usize { self.0.len() }

    /// Whether there are no links.
    pub fn is_empty(&self) -> bool { self.0.is_empty() }

    /// Row `row`'s link: the next row of its list, or [`NIL`](Self::NIL).
    ///
    /// # Panics
    ///
    /// Panics if `row` has no link.
    #[inline]
    pub fn get(&self, row: RowId) -> RowId { self.0[row as usize].load(Ordering::Relaxed) }

    /// Sets row `row`'s link to `next`, through a shared reference.
    ///
    /// # Panics
    ///
    /// Panics if `row` has no link.
    #[inline]
    pub fn set(&self, row: RowId, next: RowId) {
        self.0[row as usize].store(next, Ordering::Relaxed);
    }

    /// Appends one link, `next`, for a row just appended to the batch.
    /// `insert` passes [`NIL`](Self::NIL): the row is on no list yet.
    pub fn push(&mut self, next: RowId) { self.0.push(AtomicU32::new(next)) }

    /// The `len` rows of the chain that starts at row `head`, in chain
    /// order (see [`ChainWalk`]). `len` must be the chain's length; `head` is
    /// not read when `len` is 0.
    #[inline]
    pub fn walk(&self, head: RowId, len: usize) -> ChainWalk<'_> {
        ChainWalk {
            links: self,
            next: head,
            left: len,
        }
    }

    /// Heap bytes the links hold: their capacity, not their length, at 4
    /// bytes each.
    pub fn heap_size_bytes(&self) -> usize { self.0.capacity() * std::mem::size_of::<AtomicU32>() }

    /// Every link, in row order, for tests and diagnostics.
    pub fn to_vec(&self) -> Vec<RowId> {
        self.0
            .iter()
            .map(|link| link.load(Ordering::Relaxed))
            .collect()
    }
}

/// The rows of one chain, in chain order: [`ChainLinks::walk`].
///
/// It reads each row's link before it yields the row, so its caller may
/// re-thread the rows it is given, rewriting their links, while the walk is
/// still running. That is Algorithm 2's step (VLDB 2020 §3.2.2): a list is
/// walked, and each of its rows moves onto a child's list on the way.
///
/// It is counted: it yields the `len` rows it was given and never reads a
/// link past the last one's. A debug build reads the last row's link too,
/// before yielding that row, and asserts it is [`ChainLinks::NIL`].
pub struct ChainWalk<'a> {
    links: &'a ChainLinks,
    /// The row to yield next; meaningless once `left` is 0.
    next: RowId,
    /// How many rows remain to yield.
    left: usize,
}

impl Iterator for ChainWalk<'_> {
    type Item = RowId;

    #[inline]
    fn next(&mut self) -> Option<RowId> {
        if self.left == 0 {
            return None;
        }
        let row = self.next;
        self.left -= 1;
        if self.left > 0 {
            // Read before `row` is yielded: the caller may re-thread `row`
            // (rewrite its link) before it asks for the next one.
            self.next = self.links.get(row);
        } else {
            debug_assert_eq!(
                self.links.get(row),
                ChainLinks::NIL,
                "ChainWalk: the tail, row {row}, does not link to NIL"
            );
        }
        Some(row)
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) { (self.left, Some(self.left)) }
}

impl ExactSizeIterator for ChainWalk<'_> {}

#[cfg(test)]
mod tests {
    use super::*;

    /// Rows 2, 0 and 3 of a five-row batch threaded into one chain, as a
    /// list's pushes leave them: each link names the next row, and the
    /// tail's is `NIL`. Rows 1 and 4 are on no list and keep link 0.
    fn chain_2_0_3() -> ChainLinks {
        let links = ChainLinks::new(5);
        links.set(2, 0);
        links.set(0, 3);
        links.set(3, ChainLinks::NIL);
        links
    }

    /// `new` makes one zero link per row, in one allocation of exactly
    /// `rows` links.
    #[test]
    fn new_links_are_zero_one_per_row() {
        let links = ChainLinks::new(5);
        assert_eq!(links.len(), 5);
        assert!(!links.is_empty());
        assert_eq!(links.to_vec(), [0; 5]);
        assert_eq!(links.heap_size_bytes(), 5 * 4);
        let none = ChainLinks::new(0);
        assert!(none.is_empty());
        assert_eq!(none.heap_size_bytes(), 0);
    }

    /// `with_capacity` reserves room and holds no link until `push`; the
    /// per-row builds push one per row, so their links cost what `new`'s do.
    #[test]
    fn with_capacity_holds_no_link_until_pushed() {
        let mut links = ChainLinks::with_capacity(3);
        assert!(links.is_empty());
        assert_eq!(links.heap_size_bytes(), 3 * 4);
        links.push(ChainLinks::NIL);
        links.push(0);
        links.push(ChainLinks::NIL);
        assert_eq!(links.len(), 3);
        assert_eq!(links.to_vec(), [ChainLinks::NIL, 0, ChainLinks::NIL]);
        assert_eq!(
            links.heap_size_bytes(),
            ChainLinks::new(3).heap_size_bytes(),
            "three pushes fit the reserved room"
        );
        assert!(ChainLinks::default().is_empty());
    }

    /// `set` rewrites one link through a shared reference, and `get` reads
    /// it back; the other links do not move.
    #[test]
    fn set_rewrites_one_link_through_a_shared_reference() {
        let links = ChainLinks::new(3);
        let shared = &links;
        shared.set(1, 2);
        assert_eq!(links.get(1), 2);
        assert_eq!((links.get(0), links.get(2)), (0, 0));
        shared.set(1, ChainLinks::NIL);
        assert_eq!(links.to_vec(), [0, ChainLinks::NIL, 0]);
    }

    #[test]
    #[should_panic(expected = "index out of bounds")]
    fn a_row_without_a_link_panics() { let _ = ChainLinks::new(2).get(2); }

    /// A walk yields its chain in chain order, exactly `len` rows, and stops.
    /// It never reads past the tail: the tail's link is `NIL`, and a walk that
    /// read on would index the links at `NIL` and panic.
    #[test]
    fn a_walk_yields_its_chain_in_chain_order() {
        let links = chain_2_0_3();
        assert_eq!(links.walk(2, 3).collect::<Vec<_>>(), [2, 0, 3]);
        let mut walk = links.walk(2, 3);
        assert_eq!(walk.len(), 3);
        assert_eq!(walk.next(), Some(2));
        assert_eq!(walk.size_hint(), (2, Some(2)));
        assert_eq!((walk.next(), walk.next()), (Some(0), Some(3)));
        assert_eq!(walk.len(), 0);
        assert_eq!(
            (walk.next(), walk.next()),
            (None, None),
            "a finished walk stays finished"
        );
        // A chain's tail alone is a chain of one row.
        assert_eq!(links.walk(3, 1).collect::<Vec<_>>(), [3]);
    }

    /// An empty walk reads no link at all, not even its head's: over no
    /// links, any read would panic.
    #[test]
    fn an_empty_walk_reads_nothing() {
        let none = ChainLinks::default();
        assert_eq!(none.walk(ChainLinks::NIL, 0).len(), 0);
        assert_eq!(none.walk(ChainLinks::NIL, 0).next(), None);
        assert_eq!(chain_2_0_3().walk(2, 0).count(), 0);
    }

    /// The rule the walk exists for: it reads a row's link before it yields
    /// the row, so the caller may re-thread each row as it gets it. Every row
    /// of the chain 0 → 1 → 2 → 3 → 4 → 5 moves, as it is yielded, onto one
    /// of two new chains by parity, the way Algorithm 2 moves a list's rows
    /// onto its children's lists: the moved row's link becomes `NIL`, and its
    /// new chain's old tail links to it. A walk that read a row's link only
    /// after yielding the row would follow the rewritten `NIL`.
    #[test]
    fn the_walk_reads_each_link_before_yielding_its_row() {
        let links = ChainLinks::new(6);
        for row in 0..5 {
            links.set(row, row + 1);
        }
        links.set(5, ChainLinks::NIL);
        // Each new chain's tail, by parity: `NIL` until it has a row.
        let mut tails = [ChainLinks::NIL; 2];
        let mut walked = Vec::new();
        for row in links.walk(0, 6) {
            assert!(
                (row as usize) < links.len(),
                "the walk yielded {row}, which is no row: it read a link after re-threading"
            );
            walked.push(row);
            let tail = &mut tails[row as usize % 2];
            if *tail != ChainLinks::NIL {
                links.set(*tail, row);
            }
            links.set(row, ChainLinks::NIL);
            *tail = row;
        }
        assert_eq!(walked, [0, 1, 2, 3, 4, 5]);
        assert_eq!(links.walk(0, 3).collect::<Vec<_>>(), [0, 2, 4]);
        assert_eq!(links.walk(1, 3).collect::<Vec<_>>(), [1, 3, 5]);
    }

    /// A walk given a `len` short of its chain ends on a row whose link is
    /// not `NIL`: a broken invariant, which a debug build catches before it
    /// yields that row.
    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "the tail, row 0, does not link to NIL")]
    fn a_tail_that_links_on_panics_in_debug() { let _ = chain_2_0_3().walk(2, 2).count(); }

    /// Two threads thread two halves of one batch through shared links.
    /// Each writes only its own rows' links, so relaxed stores race with
    /// nothing, and the scope's join orders them before the walks after it,
    /// as in the parallel builds.
    #[test]
    fn threads_thread_their_own_rows_through_shared_links() {
        let links = ChainLinks::new(8);
        std::thread::scope(|scope| {
            for half in [0..4, 4..8] {
                let links = &links;
                scope.spawn(move || {
                    let last = half.end - 1;
                    for row in half.start..last {
                        links.set(row, row + 1);
                    }
                    links.set(last, ChainLinks::NIL);
                });
            }
        });
        assert_eq!(links.walk(0, 4).collect::<Vec<_>>(), [0, 1, 2, 3]);
        assert_eq!(links.walk(4, 4).collect::<Vec<_>>(), [4, 5, 6, 7]);
    }
}
