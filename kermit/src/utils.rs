use {kermit_algos::join_algo::JoinAlgo, kermit_ds::relation::Relation};

pub fn compute_join<R, JA>(
    arity: usize, input: Vec<Vec<Vec<R::KT>>>, variables: Vec<usize>,
    rel_variables: Vec<Vec<usize>>,
) -> Vec<Vec<R::KT>>
where
    R: Relation,
    JA: JoinAlgo<R>,
{
    let relations: Vec<_> = input
        .into_iter()
        .map(|tuples| R::from_tuples(arity, tuples))
        .collect();
    let iterables = relations.iter().collect::<Vec<_>>();
    JA::join(variables, rel_variables, iterables)
}
