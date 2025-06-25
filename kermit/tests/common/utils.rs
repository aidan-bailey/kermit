use {kermit_algos::join_algo::JoinAlgo, kermit_ds::relation::Relation};

pub fn test_join<R, JA>(
    input: Vec<Vec<Vec<R::KT>>>, variables: Vec<usize>, rel_variables: Vec<Vec<usize>>,
    result: Vec<Vec<R::KT>>,
) where
    R: Relation,
    JA: JoinAlgo<R>,
{
    let relations: Vec<_> = input
        .into_iter()
        .map(|tuples| R::from_tuples(tuples))
        .collect();
    let iterables = relations.iter().collect::<Vec<_>>();
    assert_eq!(
        JA::join_iter(variables, rel_variables, iterables).collect::<Vec<Vec<R::KT>>>(),
        result
    );
}
