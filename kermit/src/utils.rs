use {
    crate::db::Database,
    kermit_algos::join_algo::JoinAlgo,
    kermit_ds::{
        relation::Relation,
        relation_builder::{Builder, RelationBuilder},
    },
    kermit_kvs::keyvalstore::KeyValStore,
    std::hash::Hash,
};

pub fn compute_join<R, JA>(
    arity: usize, input: Vec<Vec<Vec<R::KT>>>, variables: Vec<usize>,
    rel_variables: Vec<Vec<usize>>,
) -> Vec<Vec<R::KT>>
where
    R: Relation,
    JA: JoinAlgo<R>,
{
    let relations: Vec<_> = input
        .into_iter()
        .map(|tuples| Builder::<R>::new(arity).add_tuples(tuples).build())
        .collect();
    let iterables = relations.iter().collect::<Vec<_>>();
    JA::join(variables, rel_variables, iterables)
}
pub fn compute_db_join<VT, KVST, R, JA>(
    input1: Vec<Vec<R::KT>>, input2: Vec<Vec<R::KT>>,
) -> Database<VT, KVST, R>
where
    KVST: KeyValStore<R::KT, VT> + Default,
    VT: Hash,
    R: Relation,
    JA: JoinAlgo<R>,
{
    let mut db = Database::<VT, KVST, R>::new("test_db".to_string(), KVST::default());

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
