use kermit_iters::trie::{Iterable, TrieIterable};

pub trait JoinAlgo<'a, KT, ITB>
where
    KT: Ord + Clone,
    ITB: Iterable<'a, KT>,
{
    fn join(variables: Vec<usize>, rel_variables: Vec<Vec<usize>>, iterables: Vec<&ITB>);
}
