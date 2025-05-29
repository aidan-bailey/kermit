use kermit_iters::trie::Iterable;

pub trait JoinAlgo<KT, ITB>
where
    KT: Ord + Clone,
    ITB: Iterable<KT>,
{
    fn join(
        variables: Vec<usize>, rel_variables: Vec<Vec<usize>>, iterables: Vec<&ITB>,
    ) -> Vec<Vec<KT>>;
}
