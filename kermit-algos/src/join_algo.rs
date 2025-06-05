use kermit_iters::trie::Iterable;

pub trait JoinAlgo<ITB>
where
    ITB: Iterable,
{
    fn join(
        variables: Vec<usize>, rel_variables: Vec<Vec<usize>>, iterables: Vec<&ITB>,
    ) -> Vec<Vec<ITB::KT>>;
}
