//! Join algorithms for Kermit's relational algebra engine.
//!
//! Implements the [Leapfrog Triejoin](https://arxiv.org/abs/1210.0481) algorithm,
//! which performs worst-case optimal multi-way joins over trie-structured
//! relations. The algorithm is generic over any data structure that implements
//! [`TrieIterable`](kermit_iters::TrieIterable).
#![deny(missing_docs)]

mod join_algo;
mod leapfrog_join;
mod leapfrog_triejoin;
mod singleton;
mod trie_iter_kind;

use {clap::ValueEnum, std::str::FromStr};
pub use {
    join_algo::JoinAlgo, kermit_parser::JoinQuery, leapfrog_triejoin::LeapfrogTriejoin,
    singleton::SingletonTrieIter, trie_iter_kind::TrieIterKind,
};

/// The available join algorithm implementations.
///
/// Used as a CLI argument to select which algorithm to run.
#[derive(Copy, Clone, PartialEq, Eq, Debug, ValueEnum)]
pub enum JoinAlgorithm {
    /// The [Leapfrog Triejoin](https://arxiv.org/abs/1210.0481) algorithm;
    /// see [`LeapfrogTriejoin`].
    LeapfrogTriejoin,
}

impl FromStr for JoinAlgorithm {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            | "leapfrog_triejoin" => Ok(Self::LeapfrogTriejoin),
            | _ => Err(format!("Invalid join algorithm: {}", s)),
        }
    }
}
