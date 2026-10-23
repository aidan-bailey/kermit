//! Join algorithms for Kermit's relational algebra engine.
//!
//! Implements the [Leapfrog Triejoin](https://arxiv.org/abs/1210.0481) algorithm,
//! which performs worst-case optimal multi-way joins over trie-structured
//! relations. The algorithm is generic over any data structure that implements
//! [`TrieIterable`](kermit_iters::TrieIterable).
//!
//! # Layout
//!
//! The crate is organised in seven parts. Four are family-agnostic and sit
//! at the root — `placeholder_rewrite`, which turns each `_` into a fresh
//! variable; `access`, which reads each body atom as a selection and a
//! projection of its relation, so that constants and repeated variables
//! become a filter applied when an index is built; `analysis`, the
//! canonical variable numbering that planners and executors must agree on;
//! and `index_plan`, which runs after planning, orders each access's columns
//! by the plan and merges identical accesses into the indexes a query
//! builds. `optimiser` plans a [`QueryPlan`]. The remaining two are the
//! iterator families, `sorted` (over
//! [`TrieIterable`](kermit_iters::TrieIterable)) and `hash` (over
//! [`HashTrieIterable`](kermit_iters::HashTrieIterable)); they share no
//! code and never reference each other.
//!
//! Everything is re-exported flat from this root, so consumers name
//! `kermit_algos::HashTriejoin`, never the family path.
#![deny(missing_docs)]

mod access;
mod analysis;
mod hash;
mod index_plan;
mod join_algo;
mod optimiser;
mod placeholder_rewrite;
mod sorted;

// `clap::ValueEnum` is derived in this library crate on purpose: it keeps
// the registry enums beside the implementations they name, so adding an
// algorithm or optimiser touches one file for both the type and its CLI
// spelling. The cost is `clap` in this crate's dependency tree. Decided in
// aidan-bailey/kermit#60 (item 5). `JoinAlgorithm` lives here rather than in
// a family module because it spans both families; `Optimiser` lives beside
// its implementations in `optimiser`.
use clap::ValueEnum;
pub use {
    access::{extract_accesses, Access, AccessQuery, RewriteError, ACCESS_PREFIX},
    analysis::{analyse, canonical_names, QueryAnalysis},
    hash::HashTriejoin,
    index_plan::{plan_indexes, IndexPlan, IndexSpec, INDEX_ID_PREFIX},
    join_algo::JoinAlgo,
    kermit_parser::JoinQuery,
    optimiser::{
        distinct_per_column, order_by_rank, CardinalityOptimiser, CatalogStats, CostBasedOptimiser,
        LexicographicOptimiser, Optimiser, PlanError, Planner, QueryOptimiser, QueryPlan,
        RelationStats, StatisticsLevel,
    },
    placeholder_rewrite::rewrite_placeholders,
    sorted::LeapfrogTriejoin,
};

/// The available join algorithm implementations.
///
/// Used as a CLI argument to select which algorithm to run.
#[derive(Copy, Clone, PartialEq, Eq, Debug, ValueEnum)]
pub enum JoinAlgorithm {
    /// The Hash Trie Join algorithm (SIGMOD 2020); see [`HashTriejoin`].
    HashTriejoin,
    /// The [Leapfrog Triejoin](https://arxiv.org/abs/1210.0481) algorithm;
    /// see [`LeapfrogTriejoin`].
    LeapfrogTriejoin,
}

impl JoinAlgorithm {
    /// The bench-report axis value for this algorithm (the `algorithm`
    /// key) and the label embedded in Criterion group names. This is a
    /// stable external contract: it names on-disk `target/criterion/`
    /// directories, so existing measurements are repartitioned if it
    /// changes. Equal to the `Debug` representation for historical
    /// continuity.
    pub fn axis_value(self) -> &'static str {
        match self {
            | Self::HashTriejoin => "HashTriejoin",
            | Self::LeapfrogTriejoin => "LeapfrogTriejoin",
        }
    }
}

#[cfg(test)]
mod join_algorithm_tests {
    use super::*;

    /// Pins every label to a literal. These strings name
    /// `target/criterion/{group}` directories and the report's `algorithm`
    /// axis, so a change here repartitions every measurement taken so far.
    #[test]
    fn axis_values_are_pinned() {
        assert_eq!(JoinAlgorithm::HashTriejoin.axis_value(), "HashTriejoin");
        assert_eq!(
            JoinAlgorithm::LeapfrogTriejoin.axis_value(),
            "LeapfrogTriejoin"
        );
    }

    /// The label used to be `format!("{:?}")`; keeping them equal preserves
    /// historical Criterion directory names. Diverge only with a deliberate
    /// decision about old measurements.
    #[test]
    fn axis_values_match_debug_repr() {
        for v in JoinAlgorithm::value_variants() {
            assert_eq!(v.axis_value(), format!("{v:?}"));
        }
    }
}
