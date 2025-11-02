mod join_algo;
mod leapfrog_join;
mod leapfrog_triejoin;
mod queries;

pub use {
    join_algo::JoinAlgo, leapfrog_triejoin::LeapfrogTriejoin, queries::join_query::JoinQuery,
};
use {clap::ValueEnum, std::str::FromStr};

#[derive(Copy, Clone, PartialEq, Eq, Debug, ValueEnum)]
pub enum JoinAlgorithm {
    LeapfrogTriejoin,
}

impl FromStr for JoinAlgorithm {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            | "leapfrog_triejoin" => Ok(Self::LeapfrogTriejoin),
            | _ => Err(format!("Invalid join algorithm: {}", s).into()),
        }
    }
}