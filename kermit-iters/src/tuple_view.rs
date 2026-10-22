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

use std::fmt;

/// A closed row predicate: constants a row must hold and pairs of columns
/// that must agree. Nothing else can be expressed.
///
/// A query's constants (`edge(c1, X)`) and repeated variables
/// (`edge(X, X)`) are exactly these two conditions, so a `TupleView`
/// filtered by one reads the rows an atom can match, and no others. Both
/// lists are kept sorted and deduplicated, so two filters that express the
/// same conditions compare, and hash, equal.
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
}
