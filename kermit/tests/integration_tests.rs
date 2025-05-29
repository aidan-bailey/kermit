use {
    kermit_algos::{join_algo::JoinAlgo, leapfrog_triejoin::LeapfrogTriejoin},
    kermit_ds::{
        relation::Relation,
        relation_builder::RelationBuilder,
        relation_trie::{trie::RelationTrie, trie_builder::TrieBuilder},
    },
    kermit_iters::trie::Iterable,
    std::{cmp::PartialOrd, fmt::Debug, str::FromStr},
};

use kermit_iters::trie::TrieIterable;

// Takes in a data structure
fn compute_join<'a, KT, R, RB, JA>(arity: usize, inputa: Vec<Vec<KT>>, inputb: Vec<Vec<KT>>, variables: Vec<usize>, rel_variables: Vec<Vec<usize>>)
where
    KT: PartialOrd + std::clone::Clone + Ord + Debug + FromStr + 'a,
    R: Relation<KT> + TrieIterable<'a, KT> + 'a,
    RB: RelationBuilder<KT, R>,
    JA: JoinAlgo<'a, KT, R>,
{
    let rb1 = RB::new(arity).add_tuples(inputa).build();
    let rb2 = RB::new(arity).add_tuples(inputb).build();
    let _res = JA::join(variables, rel_variables, vec![&rb1, &rb2]);
}

#[test]
fn test_simple_join() {
    type KT = u64; // Key Type
    type R = RelationTrie<KT>;
    type RB = TrieBuilder<KT>;
    type JA = LeapfrogTriejoin;

    let arity = 1;
    let inputa = vec![vec![1u64], vec![2u64], vec![3u64]];
    let inputb = vec![vec![1u64], vec![2u64], vec![3u64]];
    //compute_join::<KT, R, RB, JA>(arity, inputa, inputb);
}

