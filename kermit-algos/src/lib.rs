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

use {clap::ValueEnum, std::str::FromStr};
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

impl FromStr for JoinAlgorithm {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            | "hash_triejoin" => Ok(Self::HashTriejoin),
            | "leapfrog_triejoin" => Ok(Self::LeapfrogTriejoin),
            | _ => Err(format!("Invalid join algorithm: {}", s)),
        }
    }
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
