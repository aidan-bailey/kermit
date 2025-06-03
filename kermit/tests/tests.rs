use kermit::{
    algos::LeapfrogTriejoin,
    compute_db_join, compute_join,
    ds::TrieBuilder,
    kvs::{AnyValType, NaiveStore},
};

#[test]
fn test_simple_join() {
    type RB = TrieBuilder<u64>;
    type JA = LeapfrogTriejoin;

    let arity = 1;
    let inputa = vec![vec![1u64], vec![2u64], vec![3u64]];
    let inputb = vec![vec![1u64], vec![2u64], vec![3u64]];
    let inputs = vec![inputa, inputb];
    let variables = vec![0];
    let rel_variables = vec![vec![0], vec![0]];
    compute_join::<RB, JA>(arity, inputs, variables, rel_variables);
}

#[test]
fn test_db_creation() {
    compute_db_join::<AnyValType, NaiveStore<_, _>, TrieBuilder<u64>, LeapfrogTriejoin>(
        vec![vec![1_u64], vec![2], vec![3]],
        vec![vec![1_u64], vec![2], vec![3]],
    );
}
