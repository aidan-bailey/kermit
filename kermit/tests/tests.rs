use kermit::{
    algos::LeapfrogTriejoin,
    compute_db_join, compute_join,
    ds::{RelationTrie, TrieBuilder},
    kvs::{AnyValType, NaiveStore},
};

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

#[test]
fn test_db_creation() {
    compute_db_join::<
        u64,
        AnyValType,
        NaiveStore<_, std::hash::BuildHasherDefault<std::hash::DefaultHasher>>,
        RelationTrie<_>,
        TrieBuilder<_>,
        LeapfrogTriejoin,
    >(vec![vec![1_u64], vec![2], vec![3]], vec![
        vec![1_u64],
        vec![2],
        vec![3],
    ]);
}
