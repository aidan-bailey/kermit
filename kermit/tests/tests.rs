use kermit::{algos::LeapfrogTriejoin, compute_join, ds::RelationTrie};

#[test]
fn test_simple_join() {
    type R = RelationTrie<u64>;
    type JA = LeapfrogTriejoin;

    let arity = 1;
    let inputa = vec![vec![1u64], vec![2u64], vec![3u64]];
    let inputb = vec![vec![1u64], vec![2u64], vec![3u64]];
    let inputs = vec![inputa, inputb];
    let variables = vec![0];
    let rel_variables = vec![vec![0]];
    let res = compute_join::<R, JA>(arity, inputs, variables, rel_variables);
    assert_eq!(res.len(), 3);
    assert_eq!(res[0], vec![1u64]);
    assert_eq!(res[1], vec![2u64]);
    assert_eq!(res[2], vec![3u64]);
}
