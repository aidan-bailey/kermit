//! The column-order policy: how free the planner is to bind an atom's
//! columns.
//!
//! Tries are built once, in each relation's stored column order, so a
//! trie-descending join binds an atom's columns left to right. That is a
//! constraint on the plan, not on the query: Leapfrog Triejoin's own
//! setting (Veldhuizen 2014) chooses the variable order first and uses
//! tries whose column order matches it. [`ColumnOrderPolicy`] says which
//! of the two the planner does. It is a planner setting, a peer of the
//! optimiser, carried by [`Planner`](super::Planner); it is neither a
//! Layout, a Config nor a BuildMode, and it applies to both iterator
//! families.

use clap::ValueEnum;

/// Which column orders the planner may bind an atom's columns in.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default, ValueEnum)]
pub enum ColumnOrderPolicy {
    /// Each relation is read in its stored column order, so a plan binds
    /// every atom's columns left to right. Today's behaviour, and the
    /// default: the zero-copy point every `any` measurement is compared
    /// against.
    #[default]
    Stored,
    /// The planner binds an atom's columns in any order. An atom whose
    /// plan disagrees with its stored order runs over a per-query copy of
    /// its relation with the columns permuted (see `crate::orient`).
    Any,
}

impl ColumnOrderPolicy {
    /// The bench-report axis value for this policy (the `column_orders`
    /// key). Pinned to clap's value names by a test, as
    /// [`Optimiser::axis_value`](super::Optimiser::axis_value) is.
    pub fn axis_value(self) -> &'static str {
        match self {
            | Self::Stored => "stored",
            | Self::Any => "any",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A variant rename cannot desync the CLI value from the report axis.
    #[test]
    fn axis_values_match_clap_value_names() {
        for v in ColumnOrderPolicy::value_variants() {
            assert_eq!(v.axis_value(), v.to_possible_value().unwrap().get_name());
        }
    }

    /// `stored` is today's behaviour and stays the default until the next
    /// sweep has data on `any` (issue #93).
    #[test]
    fn stored_is_the_default() {
        assert_eq!(ColumnOrderPolicy::default(), ColumnOrderPolicy::Stored);
        assert_eq!(ColumnOrderPolicy::Stored.axis_value(), "stored");
        assert_eq!(ColumnOrderPolicy::Any.axis_value(), "any");
    }
}
