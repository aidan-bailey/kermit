use {
    crate::db::Database,
    kermit_algos::join_algo::JoinAlgo,
    kermit_ds::{
        relation::Relation,
        relation_builder::RelationBuilder,
    },
    kermit_iters::trie::Iterable,
    kermit_kvs::keyvalstore::KeyValStore,
    std::{cmp::PartialOrd, fmt::Debug, hash::Hash, str::FromStr},
};


pub fn compute_join<KT, R, RB, JA>(
    arity: usize, input: Vec<Vec<Vec<KT>>>, variables: Vec<usize>, rel_variables: Vec<Vec<usize>>,
) -> Vec<Vec<KT>>
where
    KT: PartialOrd + std::clone::Clone + Ord + Debug + FromStr,
    R: Relation<KT> + Iterable<KT>,
    RB: RelationBuilder<KT, R>,
    JA: JoinAlgo<KT, R>,
{
    let relations: Vec<_> = input
        .into_iter()
        .map(|tuples| RB::new(arity).add_tuples(tuples).build())
        .collect();
    let iterables = relations.iter().collect::<Vec<_>>();
    JA::join(variables, rel_variables, iterables)
}

pub fn compute_db_join<KT, VT, KVST, R, RB, JA>(
    input1: Vec<Vec<KT>>, input2: Vec<Vec<KT>>,
) -> Database<KT, VT, KVST, R, RB>
where
    KT: Debug + FromStr + PartialOrd + PartialEq + Clone + Hash + std::cmp::Eq + Ord,
    KVST: KeyValStore<KT, VT> + Default,
    VT: Hash,
    R: Relation<KT> + Iterable<KT>,
    RB: RelationBuilder<KT, R>,
    JA: JoinAlgo<KT, R>,
{
    let mut db = Database::<KT, VT, KVST, R, RB>::new("test_db".to_string(), KVST::default());

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
