//! [`Tuples`]: a relation's tuples as one row-major buffer, and [`RowId`],
//! a row's index in it.
//!
//! Every relation is built from a `Tuples` batch (#111): a tuple is a slice
//! of one buffer, so `n` tuples cost one allocation, not `n`. The paper's
//! build starts from the same thing: "the incoming tuples are materialized
//! contiguously in an in-memory buffer prior to running Algorithm 2"
//! (Freitag et al., VLDB 2020, §3.3.2).

/// A row's index in its [`Tuples`] batch.
///
/// Four bytes, so a batch holds at most `u32::MAX` (about 4.29 × 10⁹) rows;
/// the paper's largest input has 1.2 × 10⁹ tuples. The paper's tuple
/// pointers are 8 bytes; the 4-byte id is kermit's.
pub type RowId = u32;

/// Tuples of one arity, stored row-major: row `r` is
/// `as_flat()[r * arity..(r + 1) * arity]` (see [`as_flat`](Self::as_flat)).
///
/// The row count is kept explicitly, so a batch of nullary tuples (arity 0,
/// an empty buffer) still knows how many it holds. A batch never holds more
/// than [`RowId::MAX`] rows, so every row has a [`RowId`].
///
/// Built row by row ([`push`](Self::push)), from a flat buffer
/// ([`from_flat`](Self::from_flat)), or from one `Vec` per tuple (the
/// `From<Vec<Vec<usize>>>` impl, which test fixtures use).
///
/// ```
/// use kermit_iters::Tuples;
///
/// let mut tuples = Tuples::from(vec![vec![2, 1], vec![1, 3], vec![1, 2]]);
/// assert_eq!((tuples.arity(), tuples.len()), (2, 3));
/// tuples.sort();
/// assert_eq!(tuples.row(0), [1, 2]);
/// assert_eq!(tuples.as_flat(), [1, 2, 1, 3, 2, 1]);
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Tuples {
    arity: usize,
    len: usize,
    data: Vec<usize>,
}

impl Tuples {
    /// An empty batch of `arity`-ary tuples.
    pub fn new(arity: usize) -> Self {
        Self {
            arity,
            len: 0,
            data: Vec::new(),
        }
    }

    /// An empty batch of `arity`-ary tuples with room for `rows` rows
    /// before its buffer grows.
    ///
    /// # Panics
    ///
    /// Panics if `arity * rows` overflows `usize`.
    pub fn with_capacity(arity: usize, rows: usize) -> Self {
        let values = arity
            .checked_mul(rows)
            .expect("Tuples::with_capacity: arity × rows overflows usize");
        Self {
            arity,
            len: 0,
            data: Vec::with_capacity(values),
        }
    }

    /// The batch whose rows are `data` cut into `len` rows of `arity`
    /// values, in order. Takes the buffer without copying it.
    ///
    /// # Panics
    ///
    /// Panics if `len` exceeds [`RowId::MAX`], or if `data.len()` is not
    /// `arity * len`.
    pub fn from_flat(arity: usize, len: usize, data: Vec<usize>) -> Self {
        assert!(
            len <= RowId::MAX as usize,
            "Tuples::from_flat: {len} rows exceed RowId::MAX"
        );
        assert_eq!(
            Some(data.len()),
            arity.checked_mul(len),
            "Tuples::from_flat: {} values do not make {len} rows of arity {arity}",
            data.len()
        );
        Self {
            arity,
            len,
            data,
        }
    }

    /// The number of values in each row.
    #[inline]
    pub fn arity(&self) -> usize { self.arity }

    /// The number of rows.
    #[inline]
    pub fn len(&self) -> usize { self.len }

    /// Whether the batch holds no rows.
    #[inline]
    pub fn is_empty(&self) -> bool { self.len == 0 }

    /// Row `r`: its `arity` values.
    ///
    /// # Panics
    ///
    /// Panics if `r` is not below [`len`](Self::len), also at arity 0,
    /// where every row is the empty slice.
    #[inline]
    pub fn row(&self, r: RowId) -> &[usize] {
        let r = r as usize;
        assert!(
            r < self.len,
            "Tuples::row: row {r} out of range for {} rows",
            self.len
        );
        &self.data[r * self.arity..(r + 1) * self.arity]
    }

    /// Every row, in order: [`len`](Self::len) slices of
    /// [`arity`](Self::arity) values (empty slices at arity 0).
    #[inline]
    pub fn rows(&self) -> impl ExactSizeIterator<Item = &[usize]> + '_ {
        let (data, arity) = (self.data.as_slice(), self.arity);
        (0..self.len).map(move |r| &data[r * arity..(r + 1) * arity])
    }

    /// Appends `row`.
    ///
    /// # Panics
    ///
    /// Panics if `row.len()` is not [`arity`](Self::arity), or if the batch
    /// already holds [`RowId::MAX`] rows.
    #[inline]
    pub fn push(&mut self, row: &[usize]) {
        assert_eq!(
            row.len(),
            self.arity,
            "Tuples::push: a row of arity {} in a batch of arity {}",
            row.len(),
            self.arity
        );
        assert!(
            self.len < RowId::MAX as usize,
            "Tuples::push: a batch holds at most RowId::MAX rows"
        );
        self.data.extend_from_slice(row);
        self.len += 1;
    }

    /// The whole buffer, row after row.
    #[inline]
    pub fn as_flat(&self) -> &[usize] { &self.data }

    /// Sorts the batch's rows lexicographically: the order the sorted
    /// tries build in, and one function for both, so the sort costs the
    /// same in TreeTrie's build and ColumnTrie's.
    ///
    /// Arities 1–4 sort the buffer as `[usize; N]` arrays, which compare
    /// lexicographically, so a comparison reads two adjacent rows instead
    /// of following two heap pointers (#111's profile: the pointer-chasing
    /// sort ran at 0.45–1.2 IPC). Wider rows sort a permutation of row ids
    /// by row, then gather the rows into a new buffer of exactly their size:
    /// at arity 5 and above `sort` replaces the buffer, so it drops any spare
    /// capacity the old one held. Arity 0 has nothing to order. Equal rows are
    /// identical, so the result does not depend on the sort being unstable:
    /// it is the order of the hand-rolled comparator the sorted tries used
    /// before #111.
    pub fn sort(&mut self) {
        match self.arity {
            | 0 => {},
            | 1 => sort_rows::<1>(&mut self.data),
            | 2 => sort_rows::<2>(&mut self.data),
            | 3 => sort_rows::<3>(&mut self.data),
            | 4 => sort_rows::<4>(&mut self.data),
            | _ => self.sort_by_permutation(),
        }
    }

    /// [`sort`](Self::sort) for rows wider than 4: sort the row ids by row,
    /// then gather the rows in that order into a new buffer.
    fn sort_by_permutation(&mut self) {
        let (data, arity) = (self.data.as_slice(), self.arity);
        let row = |r: RowId| &data[r as usize * arity..(r as usize + 1) * arity];
        // `len <= RowId::MAX`, so the cast is exact.
        let mut order: Vec<RowId> = (0..self.len as RowId).collect();
        order.sort_unstable_by(|&a, &b| row(a).cmp(row(b)));
        let mut sorted = Vec::with_capacity(data.len());
        for r in order {
            sorted.extend_from_slice(row(r));
        }
        self.data = sorted;
    }

    /// Releases the buffer's spare capacity, so that
    /// [`heap_size_bytes`](Self::heap_size_bytes) counts exactly the rows.
    pub fn shrink_to_fit(&mut self) { self.data.shrink_to_fit() }

    /// Heap bytes the buffer holds: its capacity, not its length, in
    /// `usize`s.
    pub fn heap_size_bytes(&self) -> usize { self.data.capacity() * std::mem::size_of::<usize>() }

    /// Every row as its own `Vec`, for tests and diagnostics.
    pub fn to_vecs(&self) -> Vec<Vec<usize>> { self.rows().map(<[usize]>::to_vec).collect() }
}

/// Sorts `data` as rows of `N` values: `[usize; N]` compares
/// lexicographically, as rows do.
fn sort_rows<const N: usize>(data: &mut [usize]) {
    let (rows, rest) = data.as_chunks_mut::<N>();
    debug_assert!(rest.is_empty(), "the buffer holds whole rows");
    rows.sort_unstable();
}

/// One `Vec` per tuple, as test fixtures write them. The arity is the first
/// tuple's; an empty `Vec` is the empty batch of arity 0. The buffer is
/// reserved once, at exactly the rows' size.
///
/// This must stay the only `From<Vec<_>>` impl: an empty literal, as in
/// `from_tuples(header, vec![])`, infers its element type through it, and a
/// second one would leave that call ambiguous.
///
/// # Panics
///
/// Panics if a tuple's arity differs from the first one's, or if there are
/// more than [`RowId::MAX`] tuples (see [`Tuples::push`]).
impl From<Vec<Vec<usize>>> for Tuples {
    fn from(rows: Vec<Vec<usize>>) -> Self {
        let arity = rows.first().map_or(0, Vec::len);
        let mut tuples = Tuples::with_capacity(arity, rows.len());
        for (i, row) in rows.iter().enumerate() {
            assert_eq!(
                row.len(),
                arity,
                "Tuples::from: row {i} has arity {}, the first row has {arity}",
                row.len()
            );
            tuples.push(row);
        }
        tuples
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 64-bit LCG (Knuth's MMIX constants), so the sort test is seeded
    /// without a `rand` dependency.
    struct Lcg(u64);

    impl Lcg {
        fn below(&mut self, n: usize) -> usize {
            self.0 = self
                .0
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            (self.0 >> 33) as usize % n
        }
    }

    /// The hand-rolled comparator `TreeTrie::from_tuples` and ColumnTrie's
    /// build sorted `Vec<Vec<usize>>` with before #111, copied verbatim.
    fn sort_like_the_sorted_tries_did(mut tuples: Vec<Vec<usize>>) -> Vec<Vec<usize>> {
        tuples.sort_unstable_by(|a, b| {
            for i in 0..a.len() {
                match a[i].cmp(&b[i]) {
                    | std::cmp::Ordering::Less => return std::cmp::Ordering::Less,
                    | std::cmp::Ordering::Greater => return std::cmp::Ordering::Greater,
                    | std::cmp::Ordering::Equal => continue,
                }
            }
            std::cmp::Ordering::Equal
        });
        tuples
    }

    /// `n` rows of `arity` values below `domain`: a small domain repeats
    /// prefixes and whole rows.
    fn random_rows(lcg: &mut Lcg, n: usize, arity: usize, domain: usize) -> Vec<Vec<usize>> {
        (0..n)
            .map(|_| (0..arity).map(|_| lcg.below(domain)).collect())
            .collect()
    }

    /// A nullary batch keeps its row count: its buffer is empty, its rows
    /// are empty slices, and sorting it changes nothing.
    #[test]
    fn nullary_rows_keep_their_count() {
        let mut tuples = Tuples::from(vec![vec![], vec![], vec![]]);
        assert_eq!((tuples.arity(), tuples.len()), (0, 3));
        assert!(!tuples.is_empty());
        assert!(tuples.as_flat().is_empty());
        assert_eq!(tuples.heap_size_bytes(), 0);
        assert_eq!(tuples.rows().len(), 3);
        assert!(tuples.rows().all(<[usize]>::is_empty));
        assert!(tuples.row(2).is_empty());
        tuples.push(&[]);
        tuples.sort();
        assert_eq!(tuples.len(), 4);
        assert_eq!(tuples.to_vecs(), vec![Vec::<usize>::new(); 4]);
    }

    /// An empty `Vec` has no first row to take an arity from: it is the
    /// empty batch of arity 0.
    #[test]
    fn an_empty_vec_is_an_empty_batch() {
        let tuples = Tuples::from(Vec::<Vec<usize>>::new());
        assert_eq!((tuples.arity(), tuples.len()), (0, 0));
        assert!(tuples.is_empty());
        assert_eq!(tuples.rows().len(), 0);
        assert_eq!(tuples, Tuples::default());
        assert_eq!(tuples.to_vecs(), Vec::<Vec<usize>>::new());
    }

    #[test]
    #[should_panic(expected = "Tuples::from: row 1 has arity 1, the first row has 2")]
    fn mixed_arity_vectors_panic() { let _ = Tuples::from(vec![vec![1, 2], vec![3]]); }

    #[test]
    #[should_panic(expected = "a row of arity 3 in a batch of arity 2")]
    fn a_push_of_the_wrong_arity_panics() { Tuples::new(2).push(&[1, 2, 3]); }

    #[test]
    #[should_panic(expected = "row 2 out of range for 2 rows")]
    fn a_row_past_the_end_panics() { let _ = Tuples::from(vec![vec![1, 2], vec![3, 4]]).row(2); }

    /// At arity 0 every row is the empty slice, which slicing would lend
    /// for any id: only the explicit check stops a read past the end.
    #[test]
    #[should_panic(expected = "row 2 out of range for 2 rows")]
    fn a_nullary_row_past_the_end_panics() { let _ = Tuples::from(vec![vec![], vec![]]).row(2); }

    #[test]
    fn rows_lend_every_row_in_order() {
        let tuples = Tuples::from(vec![vec![1, 2], vec![3, 4], vec![5, 6]]);
        assert_eq!((tuples.arity(), tuples.len()), (2, 3));
        let rows: Vec<Vec<usize>> = tuples.rows().map(<[usize]>::to_vec).collect();
        assert_eq!(rows, vec![vec![1, 2], vec![3, 4], vec![5, 6]]);
        assert_eq!(tuples.rows().len(), 3);
        assert_eq!(tuples.row(1), [3, 4]);
        assert_eq!(tuples.as_flat(), [1, 2, 3, 4, 5, 6]);
        // `From` reserves exactly the rows it copies.
        assert_eq!(tuples.heap_size_bytes(), 6 * std::mem::size_of::<usize>());
    }

    /// `sort` orders rows as the old comparator did: through arrays at
    /// arities 1–4, through a row-id permutation at 5. Empty batches keep
    /// their arity, and a small domain repeats prefixes and whole rows.
    #[test]
    fn sort_orders_rows_as_the_old_comparator_did() {
        let mut lcg = Lcg(0x111);
        for arity in 1..=5 {
            for n in [0, 1, 2, 17, 300] {
                for domain in [2, 5, 1000] {
                    let rows = random_rows(&mut lcg, n, arity, domain);
                    let mut tuples = Tuples::new(arity);
                    for row in &rows {
                        tuples.push(row);
                    }
                    tuples.sort();
                    assert_eq!((tuples.arity(), tuples.len()), (arity, n));
                    assert_eq!(
                        tuples.to_vecs(),
                        sort_like_the_sorted_tries_did(rows),
                        "arity {arity}, {n} rows over 0..{domain}"
                    );
                }
            }
        }
    }

    #[test]
    fn vectors_round_trip() {
        let rows = vec![vec![4, 5, 6], vec![1, 2, 3], vec![4, 5, 6]];
        let tuples = Tuples::from(rows.clone());
        assert_eq!(tuples.to_vecs(), rows);
        assert_eq!(Tuples::from(tuples.to_vecs()), tuples);
        let nullary = Tuples::from(vec![vec![]; 2]);
        assert_eq!(nullary.to_vecs(), vec![Vec::<usize>::new(); 2]);
    }

    #[test]
    fn from_flat_takes_the_buffer_as_rows() {
        let tuples = Tuples::from_flat(2, 3, vec![1, 2, 3, 4, 5, 6]);
        assert_eq!(
            tuples,
            Tuples::from(vec![vec![1, 2], vec![3, 4], vec![5, 6]])
        );
        assert_eq!(Tuples::from_flat(0, 5, Vec::new()).len(), 5);
    }

    #[test]
    #[should_panic(expected = "do not make 2 rows of arity 2")]
    fn from_flat_rejects_a_partial_row() { let _ = Tuples::from_flat(2, 2, vec![1, 2, 3]); }

    /// Nullary rows cost no memory, so the row limit is reachable in a test.
    #[test]
    #[should_panic(expected = "at most RowId::MAX rows")]
    fn a_push_past_the_last_row_id_panics() {
        let mut tuples = Tuples::from_flat(0, RowId::MAX as usize, Vec::new());
        tuples.push(&[]);
    }

    /// `arity * len` wraps to 0 here, which would match an empty buffer.
    #[test]
    #[should_panic(expected = "0 values do not make 2 rows of arity")]
    fn from_flat_rejects_an_overflowing_row_size() {
        let _ = Tuples::from_flat(usize::MAX / 2 + 1, 2, Vec::new());
    }

    /// `arity * rows` wraps to 0 here, which would reserve nothing.
    #[test]
    #[should_panic(expected = "Tuples::with_capacity: arity × rows overflows usize")]
    fn with_capacity_rejects_an_overflowing_size() {
        let _ = Tuples::with_capacity(usize::MAX / 2 + 1, 2);
    }

    #[test]
    #[should_panic(expected = "exceed RowId::MAX")]
    fn from_flat_past_the_last_row_id_panics() {
        let _ = Tuples::from_flat(0, RowId::MAX as usize + 1, Vec::new());
    }

    #[test]
    fn capacity_is_counted_and_released() {
        let mut tuples = Tuples::with_capacity(3, 10);
        assert!(tuples.heap_size_bytes() >= 30 * std::mem::size_of::<usize>());
        tuples.push(&[1, 2, 3]);
        tuples.push(&[4, 5, 6]);
        tuples.shrink_to_fit();
        assert_eq!(tuples.heap_size_bytes(), 6 * std::mem::size_of::<usize>());
    }
}
