use kermit_iters::trie::Iterable;

pub trait JoinAlgo<'a, KT, ITB>
where
    KT: Ord + Clone + 'a,
    ITB: Iterable<'a, KT>,
{
    fn join(variables: Vec<usize>, rel_variables: Vec<Vec<usize>>, iterables: Vec<&'a ITB>) -> Vec<Vec<KT>>;
}
