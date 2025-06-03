use {
    crate::db::Database,
    kermit_algos::join_algo::JoinAlgo,
    kermit_ds::{relation::Relation, relation_builder::RelationBuilder},
    kermit_iters::trie::Iterable,
    kermit_kvs::keyvalstore::KeyValStore,
    std::hash::Hash,
};

pub fn compute_join<R, RB, JA>(
    arity: usize, input: Vec<Vec<Vec<R::KT>>>, variables: Vec<usize>,
    rel_variables: Vec<Vec<usize>>,
) -> Vec<Vec<R::KT>>
where
    R: Relation + Iterable<R::KT>,
    RB: RelationBuilder<R>,
    JA: JoinAlgo<R::KT, R>,
{
    let relations: Vec<_> = input
        .into_iter()
        .map(|tuples| RB::new(arity).add_tuples(tuples).build())
        .collect();
    let iterables = relations.iter().collect::<Vec<_>>();
    JA::join(variables, rel_variables, iterables)
}

pub fn compute_db_join<VT, KVST, R, RB, JA>(
    input1: Vec<Vec<R::KT>>, input2: Vec<Vec<R::KT>>,
) -> Database<VT, KVST, R, RB>
where
    KVST: KeyValStore<R::KT, VT> + Default,
    VT: Hash,
    R: Relation + Iterable<R::KT>,
    RB: RelationBuilder<R>,
    JA: JoinAlgo<R::KT, R>,
{
    let mut db = Database::<VT, KVST, R, RB>::new("test_db".to_string(), KVST::default());

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
