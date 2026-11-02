//! [`HashIndex`]: the representation, the build and the [`Relation`] impls.

use {
    crate::{
        cardinality::Cardinality,
        heap_size::HeapSize,
        morsel::row_id,
        relation::{check_view_arity, Projectable, Relation, RelationHeader},
    },
    kermit_iters::{
        HasOptimizationAxes, HashStrategy, JoinIterable, LayoutOption, RowId, SipHashStrategy,
        TrieLevels, TupleView, Tuples,
    },
    serde_json::Value,
    std::{collections::BTreeMap, marker::PhantomData},
};

/// No row: an empty bucket's head, and the link of a chain's last row. A
/// [`Tuples`] batch holds at most `RowId::MAX` rows, so `RowId::MAX` is never
/// a row id.
pub(super) const NIL: RowId = RowId::MAX;

/// `FxHash`'s multiplier: the combine step of the key hash.
const FX_SEED: u64 = 0x517c_c1b7_2722_0a95;

/// 2⁶⁴ over the golden ratio, rounded to an odd number: the multiplier of
/// Fibonacci hashing, which spreads the key hash over the directory by its top
/// bits.
const FIBONACCI: u64 = 0x9E37_79B9_7F4A_7C15;

/// A chained hash table over one relation's rows, keyed on their leading
/// columns: the build side of a binary hash join (Wang et al., "Free Join:
/// Unifying Worst-Case Optimal and Traditional Joins", SIGMOD 2023, §2.2: a
/// hash table keyed on the attributes a relation shares with the relations
/// before it).
///
/// # Representation
///
/// - `rows`: the view's matching rows, copied once, in source order.
/// - `directory`: `slots` bucket heads, each a [`RowId`] or `NIL`, with `slots
///   = max(2, rows.next_power_of_two())`, so the load factor is at most 1.
/// - `next`: per row, the next row of its bucket's chain, or `NIL`.
///
/// Both arrays are plain `Vec<RowId>`. `HashTrie`'s `ChainLinks` exist so
/// that a lazy trie can re-thread rows through `&self`; a keyed index never
/// does, so it needs none of that machinery. Both arrays are empty when the
/// index has no key columns.
///
/// # Keys and hashing
///
/// The key is the first [`key_len`](kermit_iters::KeyedIterable::key_len)
/// columns, the header's level count at build (`RelationHeader::levels()`;
/// every column unless the header says fewer, as `bench ds` builds it). The
/// key hash folds `H::hash` over the key values with `FxHash`'s combine step,
/// `h = (h.rotate_left(5) ^ H::hash(v)) * 0x517c_c1b7_2722_0a95`, from
/// `h = 0`. The bucket is the top `log2(slots)` bits of
/// `h * 0x9E37_79B9_7F4A_7C15` (Fibonacci hashing), all modulo 2⁶⁴. The
/// hasher is the Layout `H`, bench axis `ds_layout_hasher`.
///
/// # Invariants
///
/// - Rows are linked last to first, each prepended to its bucket's chain, so
///   every chain lists its rows in ascending row id: source order.
/// - `directory.len()` is a power of two, at least 2, and at least the row
///   count. `directory` and `next` are empty exactly when `key_len == 0`.
/// - `key_len` is `header.levels()`, at most the arity.
///
/// # Duplicates
///
/// A repeated row is stored and lent once per copy
/// ([`Relation::KEEPS_DUPLICATES`] is `true`), so a join's counts equal
/// `HashTrie`'s. It is planned under [`TrieLevels::ProbeKeys`].
///
/// # Example
///
/// ```
/// use {
///     kermit_ds::{HashIndex, Relation, RelationHeader},
///     kermit_iters::KeyedIterable,
/// };
///
/// // S(b, c), keyed on its first column b.
/// let s: HashIndex = HashIndex::from_tuples(
///     RelationHeader::new_nameless_positional(2).with_levels(1),
///     vec![vec![2, 3], vec![2, 4], vec![3, 4]],
/// );
/// assert_eq!(s.key_len(), 1);
/// let matches: Vec<Vec<usize>> = s.probe(&[2]).map(<[usize]>::to_vec).collect();
/// assert_eq!(matches, vec![vec![2, 3], vec![2, 4]]);
/// assert_eq!(s.probe(&[5]).count(), 0);
/// ```
#[derive(Clone, Debug)]
pub struct HashIndex<H: HashStrategy = SipHashStrategy> {
    /// The relation's header; its level count is the key length.
    pub(super) header: RelationHeader,
    /// The rows, in source order.
    pub(super) rows: Tuples,
    /// How many leading columns form the key (`header.levels()`).
    pub(super) key_len: usize,
    /// Per bucket, the first row of its chain or `NIL`.
    pub(super) directory: Vec<RowId>,
    /// Per row, the next row of its chain or `NIL`.
    pub(super) next: Vec<RowId>,
    /// The hasher: a type-level choice, zero-sized.
    _hasher: PhantomData<H>,
}

impl<H: HashStrategy> HashIndex<H> {
    /// An index over `rows` under `header`, sized for exactly those rows.
    fn with_rows(header: RelationHeader, rows: Tuples) -> Self {
        let key_len = header.levels();
        let mut index = Self {
            header,
            rows,
            key_len,
            directory: Vec::new(),
            next: Vec::new(),
            _hasher: PhantomData,
        };
        index.link_rows();
        index
    }

    /// The bucket of `key`: the top `log2(slots)` bits of the Fibonacci
    /// product of the `FxHash` fold of `H::hash` over `key`.
    pub(super) fn bucket(&self, key: &[usize]) -> usize {
        debug_assert!(
            !self.directory.is_empty(),
            "an index with no key has no buckets"
        );
        let mut hash = 0_u64;
        for &value in key {
            hash = (hash.rotate_left(5) ^ H::hash(value)).wrapping_mul(FX_SEED);
        }
        // `slots >= 2`, so the shift is at most 63.
        let shift = u64::BITS - self.directory.len().trailing_zeros();
        (hash.wrapping_mul(FIBONACCI) >> shift) as usize
    }

    /// Sizes the directory for the rows held and links every row, last to
    /// first, each prepended to its bucket's chain: three allocations in all
    /// (`rows` is the caller's), none per row. An index with no key columns
    /// has no chains to link.
    fn link_rows(&mut self) {
        if self.key_len == 0 {
            return;
        }
        let rows = self.rows.len();
        self.directory = vec![NIL; rows.next_power_of_two().max(2)];
        self.next = vec![NIL; rows];
        for position in (0..rows).rev() {
            let row = row_id(position);
            let bucket = self.bucket(&self.rows.row(row)[..self.key_len]);
            self.next[position] = self.directory[bucket];
            self.directory[bucket] = row;
        }
    }

    /// Appends `row`, the newest and so the highest id, to the tail of its
    /// chain, which keeps the chain in source order. Linear in the chain.
    fn append_to_chain(&mut self, row: RowId) {
        let bucket = self.bucket(&self.rows.row(row)[..self.key_len]);
        let mut at = self.directory[bucket];
        if at == NIL {
            self.directory[bucket] = row;
            return;
        }
        while self.next[at as usize] != NIL {
            at = self.next[at as usize];
        }
        self.next[at as usize] = row;
    }
}

impl<H: HashStrategy> JoinIterable for HashIndex<H> {}

impl<H: HashStrategy> Relation for HashIndex<H> {
    /// An index is a multiset: a repeated row is stored and lent once per
    /// copy (see [`Cardinality`]).
    const KEEPS_DUPLICATES: bool = true;
    /// Planned under [`TrieLevels::ProbeKeys`]: each index's level count is
    /// the length of the key it is probed on.
    const TRIE_LEVELS: TrieLevels = TrieLevels::ProbeKeys;

    fn header(&self) -> &RelationHeader { &self.header }

    /// An empty index under `header`: two empty slots when the header has key
    /// columns, none when it has not.
    fn new(header: RelationHeader) -> Self {
        let rows = Tuples::new(header.arity());
        Self::with_rows(header, rows)
    }

    /// Builds an index from the rows a view keeps.
    ///
    /// Copies them once with [`TupleView::materialize`] (an exact-size
    /// buffer, in source order, the index's own `rows`), then sizes the
    /// directory and links every row: no allocation per input row.
    ///
    /// # Panics
    ///
    /// Panics if the view's source has rows and the view's arity does not
    /// match `header.arity()`, even when the filter keeps none
    /// (`relation::check_view_arity`, which owns the message). An empty
    /// source builds the empty index under any header.
    fn from_view(header: RelationHeader, view: TupleView<'_>) -> Self {
        check_view_arity(&header, &view);
        if view.source_rows() == 0 {
            return Self::new(header);
        }
        Self::with_rows(header, view.materialize())
    }

    /// Appends `tuple` and links it. Once the rows outnumber the slots the
    /// directory doubles and every row is relinked, so the contents
    /// (`directory`, `next` and `rows`) equal those of a bulk build of the
    /// same rows. Capacities may differ, since `next` and `rows` grow by
    /// doubling; `from_view` is the only build that is benchmarked.
    ///
    /// # Panics
    ///
    /// Panics if `tuple`'s length is not the relation's arity, or if the
    /// index already holds `RowId::MAX` rows ([`Tuples::push`]).
    fn insert(&mut self, tuple: impl AsRef<[usize]>) {
        let tuple = tuple.as_ref();
        let arity = self.header.arity();
        assert_eq!(
            tuple.len(),
            arity,
            "tuple arity {} does not match relation arity {}",
            tuple.len(),
            arity
        );
        self.rows.push(tuple);
        if self.key_len == 0 {
            return;
        }
        if self.rows.len() > self.directory.len() {
            self.link_rows();
        } else {
            self.next.push(NIL);
            let row = row_id(self.rows.len() - 1);
            self.append_to_chain(row);
        }
    }

    fn insert_all(&mut self, tuples: impl Into<Tuples>) {
        let tuples: Tuples = tuples.into();
        for tuple in tuples.rows() {
            self.insert(tuple);
        }
    }
}

impl<H: HashStrategy> Projectable for HashIndex<H> {
    /// Rebuilds from the stored rows (the rows `scan` lends) with only
    /// `columns`, in the order given, under a nameless header; the new index
    /// is keyed on every projected column.
    ///
    /// # Panics
    ///
    /// Panics if any element of `columns` is `>= self.header().arity()`.
    fn project(&self, columns: Vec<usize>) -> Self {
        let arity = self.header.arity();
        assert!(
            columns.iter().all(|&column| column < arity),
            "project: a column is out of range for arity {arity}"
        );
        let attrs: Vec<String> = columns
            .iter()
            .filter_map(|&column| self.header.attrs().get(column).cloned())
            .collect();
        let header = if attrs.is_empty() {
            RelationHeader::new_nameless_positional(columns.len())
        } else {
            RelationHeader::new_nameless(attrs)
        };

        let mut projected = Tuples::with_capacity(columns.len(), self.rows.len());
        let mut row = Vec::with_capacity(columns.len());
        for tuple in self.rows.rows() {
            row.clear();
            row.extend(columns.iter().map(|&column| tuple[column]));
            projected.push(&row);
        }
        Self::from_tuples(header, projected)
    }
}

impl<H: HashStrategy> HeapSize for HashIndex<H> {
    /// The rows, the directory and the links, by capacity.
    fn heap_size_bytes(&self) -> usize {
        self.rows.heap_size_bytes()
            + (self.directory.capacity() + self.next.capacity()) * std::mem::size_of::<RowId>()
    }
}

impl<H: HashStrategy> Cardinality for HashIndex<H> {
    /// The row count, duplicates included.
    fn tuple_count(&self) -> usize { self.rows.len() }
}

impl<H: HashStrategy> HasOptimizationAxes for HashIndex<H> {
    /// The one layout axis `ds_layout_hasher`: the strategy's
    /// `LayoutOption::NAME`.
    fn optimization_axes(&self) -> BTreeMap<String, Value> {
        BTreeMap::from([(
            "ds_layout_hasher".to_string(),
            Value::String(<H as LayoutOption>::NAME.to_string()),
        )])
    }
}
