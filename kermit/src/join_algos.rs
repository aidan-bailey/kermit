use kermit_iters::trie::{Iterable, TrieIterable};

pub trait JoinAlgo<'a, KT, ITB>
where
    KT: Ord + Clone,
    ITB: Iterable<'a, KT>,
{
    fn join(variables: Vec<usize>, rel_variables: Vec<Vec<usize>>, iterables: Vec<&ITB>);
}

pub struct LeapfrogTriejoin {}

impl<'a, KT, ITB> JoinAlgo<'a, KT, ITB> for LeapfrogTriejoin
where
    KT: Ord + Clone,
    ITB: TrieIterable<'a, KT>,
{
    fn join(variables: Vec<usize>, rel_variables: Vec<Vec<usize>>, iterables: Vec<&ITB>) {
        print!("Nice!")
    }
}
