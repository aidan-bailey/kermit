pub mod algos {
    pub use kermit_algos::leapfrog_triejoin::LeapfrogTriejoin;
}

pub mod kvs {
    pub use kermit_kvs::{anyvaltype::AnyValType, naivestore::NaiveStore};
}

pub mod ds {
    pub use kermit_ds::{
        ds::relation_trie::RelationTrie,
        relation::{Builder, RelationBuilder},
    };
}

pub mod db;

use {kermit_algos::join_algo::JoinAlgo, kermit_ds::relation::Relation};

pub fn compute_join<R, JA>(
    input: Vec<Vec<Vec<R::KT>>>, variables: Vec<usize>, rel_variables: Vec<Vec<usize>>,
) -> Vec<Vec<R::KT>>
where
    R: Relation,
    JA: JoinAlgo<R>,
{
    let relations: Vec<_> = input
        .into_iter()
        .map(|tuples| {
            let k = if tuples.is_empty() { 0 } else { tuples[0].len() };
            R::from_tuples(k.into(), tuples)
        })
        .collect();
    let iterables = relations.iter().collect::<Vec<_>>();
    JA::join_iter(variables, rel_variables, iterables).collect()
}
