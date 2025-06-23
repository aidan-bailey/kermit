
use kermit_iters::trie::TrieIterable;

use crate::relation::Relation;

pub fn test_trie_relation_iteration<DS>(input: Vec<Vec<DS::KT>>)
where
    DS: Relation + TrieIterable,
{
    let relation = DS::from_tuples(input.clone());
    let res = relation.trie_iter().into_iter().map(|x| x.into_iter().cloned().collect::<Vec<_>>()).collect::<Vec<_>>();
    assert_eq!(res.len(), input.len());
}   
