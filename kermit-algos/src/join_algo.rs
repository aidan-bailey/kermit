use kermit_iters::trie::TrieIterator;
use kermit_iters::JoinIterator;

pub trait JoinAlgo<KT, IT> where IT: JoinIterator<KT> {
    fn join(&self, variables: Vec<usize>, rel_variables: Vec<Vec<usize>>, iters: Vec<IT>) -> IT;
}
