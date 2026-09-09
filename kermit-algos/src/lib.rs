//! Join algorithms for Kermit's relational algebra engine.
//!
//! Implements the [Leapfrog Triejoin](https://arxiv.org/abs/1210.0481) algorithm,
//! which performs worst-case optimal multi-way joins over trie-structured
//! relations. The algorithm is generic over any data structure that implements
//! [`TrieIterable`](kermit_iters::TrieIterable).
#![deny(missing_docs)]

mod const_rewrite;
mod hash_singleton;
mod hash_trie_iter_kind;
mod hash_triejoin;
mod join_algo;
mod leapfrog_join;
mod leapfrog_triejoin;
mod optimiser;
mod singleton;
mod trie_iter_kind;

// `clap::ValueEnum` is derived here, in a library crate, on purpose: it keeps
// the registry enums beside the implementations they name, so adding an
// algorithm or optimiser touches one file for both the type and its CLI
// spelling. The cost is `clap` in this crate's dependency tree. Decided in
// aidan-bailey/kermit#60 (item 5).
use clap::ValueEnum;
pub use {
    const_rewrite::{
        is_const_predicate, rewrite_atoms, ConstSpec, RewriteError, CONST_PREDICATE_PREFIX,
    },
    hash_singleton::SingletonHashTrieIter,
    hash_trie_iter_kind::HashTrieIterKind,
    hash_triejoin::HashTriejoin,
    join_algo::JoinAlgo,
    kermit_parser::JoinQuery,
    leapfrog_triejoin::LeapfrogTriejoin,
    optimiser::{
        analyse, topological_order, CardinalityOptimiser, CatalogStats, LexicographicOptimiser,
        PlanError, QueryAnalysis, QueryOptimiser, QueryPlan, RelationStats,
    },
    singleton::SingletonTrieIter,
    trie_iter_kind::TrieIterKind,
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

/// The available query optimisers.
///
/// Used as a CLI argument to select which [`QueryOptimiser`] plans the
/// join's variable ordering. Distinct from the optimization *axes*
/// standard (`ds_layout_*` etc.) — the optimiser is a first-class
/// benchmark dimension with its own `optimiser` report axis.
#[derive(Copy, Clone, PartialEq, Eq, Debug, ValueEnum)]
pub enum Optimiser {
    /// Smallest canonical variable index first — reproduces the
    /// pre-optimiser hardcoded ordering. The default.
    Lexicographic,
    /// Smallest-relation-first; see [`CardinalityOptimiser`].
    Cardinality,
}

impl Optimiser {
    /// Boxes the corresponding [`QueryOptimiser`] implementation.
    pub fn instantiate(self) -> Box<dyn QueryOptimiser> {
        match self {
            | Self::Lexicographic => Box::new(LexicographicOptimiser),
            | Self::Cardinality => Box::new(CardinalityOptimiser),
        }
    }

    /// The bench-report axis value for this optimiser (the `optimiser`
    /// key).
    pub fn axis_value(self) -> &'static str {
        match self {
            | Self::Lexicographic => "lexicographic",
            | Self::Cardinality => "cardinality",
        }
    }
}

#[cfg(test)]
mod optimiser_enum_tests {
    use super::*;

    /// Pins `axis_value` to clap's derived kebab-case value name, so a
    /// variant rename cannot silently desync the CLI value from the
    /// bench-report `optimiser` axis.
    #[test]
    fn axis_values_match_clap_value_names() {
        for v in Optimiser::value_variants() {
            assert_eq!(v.axis_value(), v.to_possible_value().unwrap().get_name());
        }
    }
}
