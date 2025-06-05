use kermit_iters::join_iterable::JoinIterable;

pub trait JoinAlgo<ITB>
where
    ITB: JoinIterable,
{
    fn join(
        variables: Vec<usize>, rel_variables: Vec<Vec<usize>>, iterables: Vec<&ITB>,
    ) -> Vec<Vec<ITB::KT>>;
}
