use {kermit_ds::relation::Relation, kermit_iters::trie::TrieIterable};

pub fn test_trie_relation_iteration<DS>(input: Vec<Vec<DS::KT>>)
where
    DS: Relation + TrieIterable,
{
    let relation = DS::from_tuples(input.clone());
    let res = relation.trie_iter().into_iter().collect::<Vec<_>>();
    assert_eq!(res, input);
}
