//! [`Filter`] and `TupleView`: a borrowed, filtered and projected reading of
//! a [`Tuples`](crate::Tuples) batch.
//!
//! A query reads a relation through the view an index needs: only the rows
//! that satisfy the query's constants and repeated variables, and only the
//! columns the plan binds, in the plan's order. The view lends those rows
//! without copying them, and one `materialize` copies the matching ones once,
//! into a buffer of exactly their size. That copy is the only one an index
//! build makes (the build-mode table of
//! `docs/specs/2026-10-22-per-query-index-build-design.md`).

use {
    crate::tuples::{RowId, Tuples},
    std::fmt,
};

/// A closed row predicate: constants a row must hold and pairs of columns
/// that must agree. Nothing else can be expressed.
///
/// A query's constants (`edge(c1, X)`) and repeated variables
/// (`edge(X, X)`) are exactly these two conditions, so a `TupleView`
/// filtered by one reads the rows an atom can match, and no others. Both
/// lists are kept sorted and deduplicated, so two filters built from the
/// same lists, in any order, compare and hash equal.
///
/// ```
/// use kermit_iters::Filter;
///
/// // Column 2 holds 5, and columns 0 and 1 agree.
/// let filter = Filter::new(vec![(2, 5)], vec![(0, 1)]);
/// assert!(filter.matches(&[3, 3, 5]));
/// assert!(!filter.matches(&[3, 4, 5]));
/// assert_eq!(filter.to_string(), "col2=5, col0=col1");
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Filter {
    /// `(column, value)`: the row holds `value` in `column`. Sorted,
    /// deduplicated.
    constants: Vec<(usize, usize)>,
    /// `(earlier column, later column)`: the row holds one value in both.
    /// Sorted, deduplicated.
    equalities: Vec<(usize, usize)>,
}

impl Filter {
    /// The filter every row satisfies.
    pub fn none() -> Filter { Filter::default() }

    /// A filter of `constants` (`(column, value)`) and `equalities`
    /// (`(earlier column, later column)`). Both lists are sorted and
    /// deduplicated.
    ///
    /// # Panics
    ///
    /// Panics if an equality's first column is not below its second.
    pub fn new(mut constants: Vec<(usize, usize)>, mut equalities: Vec<(usize, usize)>) -> Filter {
        for &(earlier, later) in &equalities {
            assert!(
                earlier < later,
                "Filter::new: equality col{earlier}=col{later} must name the earlier column first"
            );
        }
        constants.sort_unstable();
        constants.dedup();
        equalities.sort_unstable();
        equalities.dedup();
        Filter {
            constants,
            equalities,
        }
    }

    /// Whether the filter holds no condition, so every row satisfies it.
    pub fn is_none(&self) -> bool { self.constants.is_empty() && self.equalities.is_empty() }

    /// The constants as `(column, value)`, sorted.
    pub fn constants(&self) -> &[(usize, usize)] { &self.constants }

    /// The equalities as `(earlier column, later column)`, sorted.
    pub fn equalities(&self) -> &[(usize, usize)] { &self.equalities }

    /// Whether `row` satisfies every constant and every equality.
    ///
    /// # Panics
    ///
    /// Panics if `row` is shorter than a column the filter names.
    pub fn matches(&self, row: &[usize]) -> bool {
        self.constants
            .iter()
            .all(|&(column, value)| row[column] == value)
            && self
                .equalities
                .iter()
                .all(|&(earlier, later)| row[earlier] == row[later])
    }
}

/// `-` for no filter; otherwise the constants as `col{c}={value}` and then
/// the equalities as `col{a}=col{b}`, joined by `, `. The report's `index`
/// lines print a filter this way.
impl fmt::Display for Filter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_none() {
            return f.write_str("-");
        }
        let constants = self
            .constants
            .iter()
            .map(|&(column, value)| format!("col{column}={value}"));
        let equalities = self
            .equalities
            .iter()
            .map(|&(earlier, later)| format!("col{earlier}=col{later}"));
        let parts: Vec<String> = constants.chain(equalities).collect();
        f.write_str(&parts.join(", "))
    }
}

/// A borrowed reading of a [`Tuples`] batch: the rows a [`Filter`] accepts,
/// each cut down to `columns`, in source order.
///
/// Output column `i` reads source column `columns[i]`, so `columns` both
/// projects and reorders (a column may repeat). A view with no output
/// columns is legal: its [`count`](Self::count) is the multiplicity of an
/// all-constant atom. Rows are lent, never copied, until
/// [`materialize`](Self::materialize) (or one of the sorted helpers) copies
/// the matching ones once.
///
/// ```
/// use kermit_iters::{Filter, TupleView, Tuples};
///
/// let source = Tuples::from(vec![vec![1, 1, 5], vec![2, 3, 5], vec![3, 3, 6]]);
/// // Rows whose last column is 5, reading columns 2 then 0.
/// let view = TupleView::new(&source, Filter::new(vec![(2, 5)], vec![]), vec![2, 0]);
/// assert_eq!(view.count(), 2);
/// assert_eq!(view.materialize().to_vecs(), vec![vec![5, 1], vec![5, 2]]);
/// ```
#[derive(Clone, Debug)]
pub struct TupleView<'a> {
    source: &'a Tuples,
    filter: Filter,
    columns: Vec<usize>,
}

impl<'a> TupleView<'a> {
    /// A view of `source` through `filter`, reading `columns`.
    ///
    /// # Panics
    ///
    /// Panics if a column, or a column the filter names, is not below
    /// `source.arity()`.
    pub fn new(source: &'a Tuples, filter: Filter, columns: Vec<usize>) -> TupleView<'a> {
        let arity = source.arity();
        for &column in &columns {
            assert!(
                column < arity,
                "TupleView::new: column {column} is not below the source arity {arity}"
            );
        }
        let named = filter
            .constants()
            .iter()
            .map(|&(column, _)| column)
            .chain(filter.equalities().iter().flat_map(|&(a, b)| [a, b]));
        for column in named {
            assert!(
                column < arity,
                "TupleView::new: filter column {column} is not below the source arity {arity}"
            );
        }
        TupleView {
            source,
            filter,
            columns,
        }
    }

    /// Every row and every column of `source`, unchanged.
    pub fn all(source: &'a Tuples) -> TupleView<'a> {
        TupleView::new(source, Filter::none(), (0..source.arity()).collect())
    }

    /// The batch the view reads.
    pub fn source(&self) -> &'a Tuples { self.source }

    /// The conditions a row must satisfy.
    pub fn filter(&self) -> &Filter { &self.filter }

    /// The source column each output column reads.
    pub fn columns(&self) -> &[usize] { &self.columns }

    /// The number of output columns.
    pub fn arity(&self) -> usize { self.columns.len() }

    /// The source's row count, matching or not: what morsels split by row id.
    pub fn source_rows(&self) -> usize { self.source.len() }

    /// Whether the view is its source: no filter, and every column in place.
    pub fn is_identity(&self) -> bool {
        self.filter.is_none() && self.columns.iter().copied().eq(0..self.source.arity())
    }

    /// Whether source row `row` satisfies the filter.
    ///
    /// # Panics
    ///
    /// Panics if `row` is not below [`source_rows`](Self::source_rows).
    pub fn matches(&self, row: RowId) -> bool { self.filter.matches(self.source.row(row)) }

    /// Writes source row `row`'s output columns into `out`. The filter is not
    /// consulted.
    ///
    /// # Panics
    ///
    /// Panics if `out.len() != self.arity()`, or if `row` is not below
    /// [`source_rows`](Self::source_rows).
    pub fn write(&self, row: RowId, out: &mut [usize]) {
        assert_eq!(
            out.len(),
            self.columns.len(),
            "TupleView::write: out has {} values for a view of arity {}",
            out.len(),
            self.columns.len()
        );
        let source = self.source.row(row);
        for (slot, &column) in out.iter_mut().zip(&self.columns) {
            *slot = source[column];
        }
    }

    /// Calls `visit` with each matching row's output columns, in source
    /// order, through one reused scratch row.
    pub fn for_each(&self, mut visit: impl FnMut(&[usize])) {
        let mut scratch = vec![0; self.columns.len()];
        for row in self.row_ids() {
            if self.matches(row) {
                self.write(row, &mut scratch);
                visit(&scratch);
            }
        }
    }

    /// The number of matching rows; [`source_rows`](Self::source_rows), with
    /// no scan, when the view is unfiltered.
    pub fn count(&self) -> usize {
        if self.filter.is_none() {
            self.source.len()
        } else {
            self.row_ids().filter(|&row| self.matches(row)).count()
        }
    }

    /// The matching rows' output columns, in source order, as one batch whose
    /// buffer is exactly [`count`](Self::count) × [`arity`](Self::arity)
    /// values: one allocation and one copy. An identity view is a single
    /// `memcpy` of the source.
    ///
    /// A view with no columns gives `Tuples::from_flat(0, count, Vec::new())`.
    pub fn materialize(&self) -> Tuples {
        if self.is_identity() {
            // `to_vec` allocates exactly the source's length.
            return Tuples::from_flat(
                self.arity(),
                self.source.len(),
                self.source.as_flat().to_vec(),
            );
        }
        self.gather(
            self.row_ids().filter(|&row| self.matches(row)),
            self.count(),
        )
    }

    /// The matching rows' output columns sorted lexicographically, as one
    /// batch of exactly [`count`](Self::count) × [`arity`](Self::arity)
    /// values, with one copy of the data at every arity.
    ///
    /// Arities 1-4 [`materialize`](Self::materialize) and then sort in place
    /// with [`Tuples::sort`], which sorts `[usize; N]` arrays there. From
    /// arity 5, `Tuples::sort` would gather into a second buffer, so this
    /// sorts the matching row ids against the view and gathers once
    /// ([`gather_sorted`](Self::gather_sorted)). Arity 0 has nothing to order.
    pub fn materialize_sorted(&self) -> Tuples {
        match self.arity() {
            | 0 => self.materialize(),
            | 1..=4 => {
                let mut sorted = self.materialize();
                sorted.sort();
                sorted
            },
            | _ => {
                let mut rows: Vec<RowId> =
                    self.row_ids().filter(|&row| self.matches(row)).collect();
                self.gather_sorted(&mut rows)
            },
        }
    }

    /// `rows` are matching source row ids: sorts them by their output rows
    /// (reordering the slice), then gathers the output rows once, into a
    /// buffer of exactly their size. TreeTrie's `parallel:N` workers call
    /// this on their partitions of row ids.
    ///
    /// An id may appear more than once, and ids that give equal output rows
    /// may end in either order, which the result cannot show.
    ///
    /// # Panics
    ///
    /// Panics if an id is not below [`source_rows`](Self::source_rows); in a
    /// debug build, also if a row does not match the filter.
    pub fn gather_sorted(&self, rows: &mut [RowId]) -> Tuples {
        for &row in rows.iter() {
            debug_assert!(
                self.matches(row),
                "TupleView::gather_sorted: row {row} does not match the filter"
            );
        }
        if self.columns.iter().copied().eq(0..self.source.arity()) {
            // The output rows are the source rows: compare them as slices.
            let source = self.source;
            rows.sort_unstable_by(|&a, &b| source.row(a).cmp(source.row(b)));
        } else {
            rows.sort_unstable_by(|&a, &b| self.output(a).cmp(self.output(b)));
        }
        self.gather(rows.iter().copied(), rows.len())
    }

    /// Source row `row`'s output columns, lent as an iterator for comparing.
    fn output(&self, row: RowId) -> impl Iterator<Item = usize> + '_ {
        let source = self.source.row(row);
        self.columns.iter().map(move |&column| source[column])
    }

    /// Every source row id, in order. A batch holds at most `RowId::MAX` rows,
    /// so the cast is exact.
    fn row_ids(&self) -> std::ops::Range<RowId> { 0..self.source.len() as RowId }

    /// The output columns of `rows`, in the order given, copied into a
    /// buffer reserved for `count` rows: the one copy of a build.
    ///
    /// # Panics
    ///
    /// Panics if `count × arity` overflows `usize`, or if `rows` yields a
    /// number of rows other than `count`.
    fn gather(&self, rows: impl Iterator<Item = RowId>, count: usize) -> Tuples {
        let values = count
            .checked_mul(self.arity())
            .expect("TupleView: arity × rows overflows usize");
        let mut data = Vec::with_capacity(values);
        for row in rows {
            let source = self.source.row(row);
            data.extend(self.columns.iter().map(|&column| source[column]));
        }
        Tuples::from_flat(self.arity(), count, data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn none_matches_every_row() {
        let none = Filter::none();
        assert!(none.is_none());
        assert!(none.constants().is_empty());
        assert!(none.equalities().is_empty());
        assert!(none.matches(&[1, 2, 3]));
        assert!(none.matches(&[]));
        assert_eq!(none, Filter::default());
        assert_eq!(none, Filter::new(vec![], vec![]));
    }

    #[test]
    fn new_sorts_and_dedups_both_lists() {
        let filter = Filter::new(vec![(2, 7), (0, 5), (2, 7)], vec![(1, 3), (0, 2), (1, 3)]);
        assert_eq!(filter.constants(), [(0, 5), (2, 7)]);
        assert_eq!(filter.equalities(), [(0, 2), (1, 3)]);
        assert!(!filter.is_none());
        // Filters written in another order are equal, and hash alike.
        let reordered = Filter::new(vec![(0, 5), (2, 7)], vec![(0, 2), (1, 3)]);
        assert_eq!(filter, reordered);
        let mut seen = std::collections::HashSet::new();
        assert!(seen.insert(filter));
        assert!(!seen.insert(reordered));
    }

    #[test]
    fn matches_checks_every_constant_and_every_equality() {
        let filter = Filter::new(vec![(2, 5)], vec![(0, 1)]);
        assert!(filter.matches(&[1, 1, 5]));
        assert!(!filter.matches(&[1, 2, 5]), "the equality fails");
        assert!(!filter.matches(&[1, 1, 6]), "the constant fails");
        assert!(!filter.matches(&[1, 2, 6]), "both fail");

        let two_constants = Filter::new(vec![(0, 1), (1, 2)], vec![]);
        assert!(two_constants.matches(&[1, 2]));
        assert!(!two_constants.matches(&[1, 3]), "the second constant fails");
        assert!(!two_constants.matches(&[2, 2]), "the first constant fails");

        // Two constants on one column can never both hold.
        let clash = Filter::new(vec![(0, 1), (0, 2)], vec![]);
        assert!(!clash.matches(&[1]));
        assert!(!clash.matches(&[2]));

        let chain = Filter::new(vec![], vec![(0, 1), (1, 2)]);
        assert!(chain.matches(&[4, 4, 4]));
        assert!(!chain.matches(&[4, 4, 5]));
        assert!(!chain.matches(&[4, 5, 5]));
    }

    #[test]
    fn display_lists_constants_then_equalities() {
        assert_eq!(Filter::none().to_string(), "-");
        assert_eq!(
            Filter::new(vec![(2, 7), (0, 5)], vec![(1, 3)]).to_string(),
            "col0=5, col2=7, col1=col3"
        );
        assert_eq!(Filter::new(vec![], vec![(0, 2)]).to_string(), "col0=col2");
        assert_eq!(Filter::new(vec![(1, 0)], vec![]).to_string(), "col1=0");
    }

    #[test]
    #[should_panic(expected = "Filter::new: equality col2=col2 must name the earlier column first")]
    fn an_equality_of_a_column_with_itself_panics() { let _ = Filter::new(vec![], vec![(2, 2)]); }

    #[test]
    #[should_panic(expected = "Filter::new: equality col3=col1 must name the earlier column first")]
    fn an_equality_written_later_column_first_panics() {
        let _ = Filter::new(vec![], vec![(3, 1)]);
    }

    /// Five rows of arity 3: `[1,1,5] [2,3,5] [2,2,6] [1,1,6] [3,3,5]`.
    fn source() -> Tuples {
        Tuples::from(vec![
            vec![1, 1, 5],
            vec![2, 3, 5],
            vec![2, 2, 6],
            vec![1, 1, 6],
            vec![3, 3, 5],
        ])
    }

    /// Rows whose column 2 holds 5 and whose columns 0 and 1 agree (rows 0
    /// and 4), reading columns 2 then 0: the output rows `[5, 1]`, `[5, 3]`.
    fn filtered(source: &Tuples) -> TupleView<'_> {
        TupleView::new(source, Filter::new(vec![(2, 5)], vec![(0, 1)]), vec![2, 0])
    }

    #[test]
    fn all_is_the_identity_view() {
        let source = source();
        let view = TupleView::all(&source);
        assert!(view.is_identity());
        assert_eq!((view.arity(), view.source_rows()), (3, 5));
        assert_eq!(view.columns(), [0, 1, 2]);
        assert!(view.filter().is_none());
        assert!(std::ptr::eq(view.source(), &source));
        assert_eq!(view.count(), 5);
        assert!(view.matches(3));
        let copy = view.materialize();
        assert_eq!(copy, source);
        assert_eq!(
            copy.value_capacity(),
            15,
            "the copy is exactly the rows' size"
        );
    }

    #[test]
    fn a_projection_or_a_filter_is_not_the_identity() {
        let source = source();
        let prefix = TupleView::new(&source, Filter::none(), vec![0, 1]);
        assert!(!prefix.is_identity());
        let permuted = TupleView::new(&source, Filter::none(), vec![2, 0]);
        assert!(!permuted.is_identity());
        let narrowed = TupleView::new(&source, Filter::new(vec![(0, 1)], vec![]), vec![0, 1, 2]);
        assert!(!narrowed.is_identity());
    }

    #[test]
    fn an_unfiltered_projection_keeps_every_row_in_source_order() {
        let source = source();
        let view = TupleView::new(&source, Filter::none(), vec![2, 0]);
        assert_eq!(view.count(), 5);
        let copy = view.materialize();
        assert_eq!(copy.to_vecs(), vec![
            vec![5, 1],
            vec![5, 2],
            vec![6, 2],
            vec![6, 1],
            vec![5, 3]
        ]);
        assert_eq!(copy.value_capacity(), 10);
    }

    #[test]
    fn a_filtered_view_lends_and_copies_only_the_matching_rows() {
        let source = source();
        let view = filtered(&source);
        let matching: Vec<bool> = (0..5).map(|row| view.matches(row)).collect();
        assert_eq!(matching, [true, false, false, false, true]);
        assert_eq!(view.count(), 2);

        let copy = view.materialize();
        assert_eq!(copy, Tuples::from_flat(2, 2, vec![5, 1, 5, 3]));
        assert_eq!(
            copy.value_capacity(),
            4,
            "sized by the count, not the source"
        );

        let mut visited = Vec::new();
        view.for_each(|row| visited.push(row.to_vec()));
        assert_eq!(visited, vec![vec![5, 1], vec![5, 3]]);
    }

    #[test]
    fn write_fills_the_output_columns_of_a_row() {
        let source = source();
        let view = filtered(&source);
        let mut out = [0; 2];
        view.write(4, &mut out);
        assert_eq!(out, [5, 3]);
        // `write` reads the columns and ignores the filter.
        view.write(1, &mut out);
        assert_eq!(out, [5, 2]);
    }

    #[test]
    fn for_each_reuses_one_scratch_row() {
        let source = source();
        let view = filtered(&source);
        let mut addresses = Vec::new();
        view.for_each(|row| addresses.push(row.as_ptr()));
        assert_eq!(addresses.len(), 2);
        assert_eq!(addresses[0], addresses[1]);
    }

    #[test]
    fn a_filter_nothing_satisfies_counts_and_copies_nothing() {
        let source = source();
        let view = TupleView::new(&source, Filter::new(vec![(0, 9)], vec![]), vec![1]);
        assert_eq!(view.count(), 0);
        let copy = view.materialize();
        assert_eq!(copy, Tuples::new(1));
        assert_eq!(copy.value_capacity(), 0);
        view.for_each(|_| panic!("no row matches"));
    }

    #[test]
    fn a_view_with_no_columns_counts_the_matching_rows() {
        let source = source();
        let counted = TupleView::new(&source, Filter::new(vec![(2, 5)], vec![]), vec![]);
        assert_eq!(counted.arity(), 0);
        assert_eq!(counted.count(), 3);
        assert_eq!(counted.materialize(), Tuples::from_flat(0, 3, Vec::new()));
        let mut calls = 0;
        counted.for_each(|row| {
            assert!(row.is_empty());
            calls += 1;
        });
        assert_eq!(calls, 3);

        let unfiltered = TupleView::new(&source, Filter::none(), vec![]);
        assert!(!unfiltered.is_identity());
        assert_eq!(unfiltered.count(), 5);
        assert_eq!(
            unfiltered.materialize(),
            Tuples::from_flat(0, 5, Vec::new())
        );
    }

    #[test]
    fn nullary_and_empty_sources() {
        let nullary = Tuples::from_flat(0, 4, Vec::new());
        let view = TupleView::all(&nullary);
        assert!(view.is_identity());
        assert_eq!(view.count(), 4);
        assert_eq!(view.materialize(), nullary);

        let empty = Tuples::new(3);
        let view = TupleView::all(&empty);
        assert_eq!((view.count(), view.source_rows()), (0, 0));
        assert_eq!(view.materialize(), empty);
    }

    #[test]
    #[should_panic(expected = "TupleView::new: column 3 is not below the source arity 3")]
    fn an_output_column_past_the_arity_panics() {
        let _ = TupleView::new(&source(), Filter::none(), vec![3]);
    }

    #[test]
    #[should_panic(expected = "TupleView::new: filter column 3 is not below the source arity 3")]
    fn a_filter_constant_past_the_arity_panics() {
        let _ = TupleView::new(&source(), Filter::new(vec![(3, 1)], vec![]), vec![0]);
    }

    #[test]
    #[should_panic(expected = "TupleView::new: filter column 3 is not below the source arity 3")]
    fn a_filter_equality_past_the_arity_panics() {
        let _ = TupleView::new(&source(), Filter::new(vec![], vec![(0, 3)]), vec![0]);
    }

    #[test]
    #[should_panic(expected = "TupleView::write: out has 3 values for a view of arity 2")]
    fn write_into_a_buffer_of_the_wrong_length_panics() {
        let source = source();
        filtered(&source).write(0, &mut [0; 3]);
    }

    #[test]
    #[should_panic(expected = "row 5 out of range for 5 rows")]
    fn matches_past_the_last_row_panics() {
        let source = source();
        let _ = filtered(&source).matches(5);
    }

    /// A 64-bit LCG (Knuth's MMIX constants), as `tuples.rs`'s tests use, so
    /// the sort tests are seeded without a `rand` dependency.
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

    /// The oracle: the view's matching rows through `for_each`, sorted by
    /// `Vec`'s own ordering.
    fn sorted_reference(view: &TupleView<'_>) -> Vec<Vec<usize>> {
        let mut rows = Vec::new();
        view.for_each(|row| rows.push(row.to_vec()));
        rows.sort();
        rows
    }

    /// At every output arity from 0 to 6, with and without a filter, the
    /// sorted copy holds the matching rows in lexicographic order at exactly
    /// their size. Arities 1-4 take the in-place array sort and 5 and above
    /// the row-id gather, and a small domain repeats prefixes and whole rows.
    #[test]
    fn materialize_sorted_sorts_the_matching_rows_at_every_arity() {
        let mut lcg = Lcg(0x1CE);
        let data: Vec<usize> = (0..300 * 6).map(|_| lcg.below(4)).collect();
        let source = Tuples::from_flat(6, 300, data);
        let column_sets: [&[usize]; 8] = [
            &[],
            &[3],
            &[5, 1],
            &[2, 0, 4],
            &[1, 3, 5, 0],
            &[4, 2, 0, 1, 3],
            &[5, 4, 3, 2, 1, 0],
            // The identity: `gather_sorted` compares whole source rows.
            &[0, 1, 2, 3, 4, 5],
        ];
        let filters = [
            Filter::none(),
            Filter::new(vec![(0, 1)], vec![]),
            Filter::new(vec![(2, 3)], vec![(1, 4)]),
            Filter::new(vec![(0, 9)], vec![]),
        ];
        for columns in column_sets {
            for filter in &filters {
                let view = TupleView::new(&source, filter.clone(), columns.to_vec());
                let sorted = view.materialize_sorted();
                let context = format!("columns {columns:?}, filter {filter}");
                assert_eq!(
                    (sorted.arity(), sorted.len()),
                    (columns.len(), view.count()),
                    "{context}"
                );
                assert_eq!(sorted.to_vecs(), sorted_reference(&view), "{context}");
                assert_eq!(
                    sorted.value_capacity(),
                    view.count() * columns.len(),
                    "{context}: exactly the rows' size"
                );
            }
        }
    }

    #[test]
    fn gather_sorted_sorts_the_ids_by_output_row_and_copies_once() {
        let source = source();
        let view = TupleView::new(&source, Filter::none(), vec![2, 0]);
        // Output rows: id 3 is [6, 1], id 1 is [5, 2], id 0 is [5, 1].
        let mut rows: [RowId; 3] = [3, 1, 0];
        let gathered = view.gather_sorted(&mut rows);
        assert_eq!(rows, [0, 1, 3], "the slice is reordered");
        assert_eq!(gathered, Tuples::from_flat(2, 3, vec![5, 1, 5, 2, 6, 1]));
        assert_eq!(gathered.value_capacity(), 6);
    }

    #[test]
    fn gather_sorted_on_an_identity_view_compares_whole_source_rows() {
        let source = source();
        let view = TupleView::new(&source, Filter::new(vec![(2, 5)], vec![]), vec![0, 1, 2]);
        assert!(
            !view.is_identity(),
            "filtered, but its columns are in place"
        );
        // Rows 4, 1, 0 are `[3,3,5]`, `[2,3,5]`, `[1,1,5]`.
        let mut rows: [RowId; 3] = [4, 1, 0];
        let gathered = view.gather_sorted(&mut rows);
        assert_eq!(rows, [0, 1, 4]);
        assert_eq!(
            gathered,
            Tuples::from_flat(3, 3, vec![1, 1, 5, 2, 3, 5, 3, 3, 5])
        );
    }

    #[test]
    fn gather_sorted_keeps_equal_output_rows() {
        let source = source();
        let view = TupleView::new(&source, Filter::none(), vec![2]);
        // Column 2 holds 5, 5, 6, 6, 5 in rows 0..5, so ids 4, 0, 1 tie.
        let mut rows: [RowId; 4] = [4, 2, 0, 1];
        let gathered = view.gather_sorted(&mut rows);
        assert_eq!(gathered, Tuples::from_flat(1, 4, vec![5, 5, 5, 6]));
        assert_eq!(rows[3], 2);
    }

    #[test]
    fn gather_sorted_reads_only_the_ids_it_is_given() {
        let source = source();
        let view = filtered(&source);
        let mut rows: [RowId; 2] = [4, 0];
        let gathered = view.gather_sorted(&mut rows);
        assert_eq!(rows, [0, 4]);
        assert_eq!(gathered, Tuples::from_flat(2, 2, vec![5, 1, 5, 3]));
        assert_eq!(view.gather_sorted(&mut []), Tuples::new(2));
    }

    /// A row id the filter rejects is a caller's bug, which debug builds
    /// catch.
    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "TupleView::gather_sorted: row 1 does not match the filter")]
    fn gather_sorted_debug_asserts_that_every_id_matches() {
        let source = source();
        let _ = filtered(&source).gather_sorted(&mut [0, 1]);
    }
}
