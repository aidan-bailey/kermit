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
        .map(|tuples| {
            let k = if tuples.is_empty() {
                0
            } else {
                tuples[0].len()
            };
            R::from_tuples(k.into(), tuples)
        })
        .collect();
    let iterables = relations.iter().collect::<Vec<_>>();
    assert_eq!(
        JA::join_iter(variables, rel_variables, iterables).collect::<Vec<Vec<R::KT>>>(),
        result
    );
}
