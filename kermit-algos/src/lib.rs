//! Join algorithms for Kermit's relational algebra engine.
//!
//! Implements the [Leapfrog Triejoin](https://arxiv.org/abs/1210.0481) algorithm,
//! which performs worst-case optimal multi-way joins over trie-structured
//! relations. The algorithm is generic over any data structure that implements
//! [`TrieIterable`](kermit_iters::TrieIterable).
//!
//! # Layout
//!
//! The crate is organised in four parts. Two are family-agnostic and sit at
//! the root — `const_rewrite`, which turns constant atoms into synthetic
//! unary predicates, and `analysis`, the canonical variable numbering that
//! planners and executors must agree on. `optimiser` plans a
//! [`QueryPlan`]. The remaining two are the iterator families, `sorted`
//! (over [`TrieIterable`](kermit_iters::TrieIterable)) and `hash` (over
//! [`HashTrieIterable`](kermit_iters::HashTrieIterable)); they share no
//! code and never reference each other.
//!
//! Everything is re-exported flat from this root, so consumers name
//! `kermit_algos::HashTriejoin`, never the family path.
#![deny(missing_docs)]

mod analysis;
mod const_rewrite;
mod hash;
mod join_algo;
mod optimiser;
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
    analysis::{analyse, QueryAnalysis},
    const_rewrite::{
        is_const_predicate, rewrite_atoms, ConstSpec, RewriteError, CONST_PREDICATE_PREFIX,
    },
    hash::{HashTrieIterKind, HashTriejoin, SingletonHashTrieIter},
    join_algo::JoinAlgo,
    kermit_parser::JoinQuery,
    optimiser::{
        topological_order, CardinalityOptimiser, CatalogStats, LexicographicOptimiser, Optimiser,
        PlanError, QueryOptimiser, QueryPlan, RelationStats,
    },
    sorted::{LeapfrogTriejoin, SingletonTrieIter, TrieIterKind},
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
