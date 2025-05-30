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

fn compute_join<KT, R, RB, JA>(
    arity: usize, input: Vec<Vec<Vec<KT>>>, variables: Vec<usize>, rel_variables: Vec<Vec<usize>>,
) -> Vec<Vec<KT>>
where
    KT: PartialOrd + std::clone::Clone + Ord + Debug + FromStr,
    R: Relation<KT> + Iterable<KT>,
    RB: RelationBuilder<KT, R>,
    JA: JoinAlgo<KT, R>,
{
    let relations: Vec<_> = input
        .into_iter()
        .map(|tuples| RB::new(arity).add_tuples(tuples).build())
        .collect();
    let iterables = relations.iter().collect::<Vec<_>>();
    JA::join(variables, rel_variables, iterables)
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
    let inputs = vec![inputa, inputb];
    let variables = vec![0];
    let rel_variables = vec![vec![0], vec![0]];
    compute_join::<KT, R, RB, JA>(arity, inputs, variables, rel_variables);
}
