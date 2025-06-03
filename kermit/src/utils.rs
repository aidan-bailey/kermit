use {
    crate::db::Database,
    kermit_algos::join_algo::JoinAlgo,
    kermit_ds::{relation::Relation, relation_builder::RelationBuilder},
    kermit_iters::trie::Iterable,
    kermit_kvs::keyvalstore::KeyValStore,
    std::hash::Hash,
};

pub fn compute_join<RB, JA>(
    arity: usize, input: Vec<Vec<Vec<<RB::Output as Relation>::KT>>>, variables: Vec<usize>,
    rel_variables: Vec<Vec<usize>>,
) -> Vec<Vec<<RB::Output as Relation>::KT>>
where
    RB::Output: Relation + Iterable<<RB::Output as Relation>::KT>,
    RB: RelationBuilder,
    JA: JoinAlgo<<RB::Output as Relation>::KT, RB::Output>,
{
    let relations: Vec<_> = input
        .into_iter()
        .map(|tuples| RB::new(arity).add_tuples(tuples).build())
        .collect();
    let iterables = relations.iter().collect::<Vec<_>>();
    JA::join(variables, rel_variables, iterables)
}

pub fn compute_db_join<VT, KVST, RB, JA>(
    input1: Vec<Vec<<RB::Output as Relation>::KT>>, input2: Vec<Vec<<RB::Output as Relation>::KT>>,
) -> Database<VT, KVST, RB>
where
    KVST: KeyValStore<<RB::Output as Relation>::KT, VT> + Default,
    VT: Hash,
    RB::Output: Relation + Iterable<<RB::Output as Relation>::KT>,
    RB: RelationBuilder,
    JA: JoinAlgo<<RB::Output as Relation>::KT, RB::Output>,
{
    let mut db = Database::<VT, KVST, RB>::new("test_db".to_string(), KVST::default());

    db.add_relation("first", 1);
    db.add_keys_batch("first", input1);

    db.add_relation("second", 1);
    db.add_keys_batch("second", input2);

    let _result = db.join::<JA>(
        vec!["first".to_string(), "second".to_string()],
        vec![0],
        vec![vec![0], vec![0]],
    );

    db
}
