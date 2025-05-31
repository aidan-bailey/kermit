use {
    kermit::db::Database,
    kermit_algos::{join_algo::JoinAlgo, leapfrog_triejoin::LeapfrogTriejoin},
    kermit_ds::{
        relation::Relation,
        relation_builder::RelationBuilder,
        relation_trie::{trie::RelationTrie, trie_builder::TrieBuilder},
    },
    kermit_iters::trie::Iterable,
    kermit_kvs::{anyvaltype::AnyValType, keyvalstore::KeyValStore, naivestore::NaiveStore},
    std::{cmp::PartialOrd, fmt::Debug, hash::Hash, str::FromStr},
};

fn compute_join<KT, R, RB, JA>(
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

#[test]
fn test_simple_join() {
    type KT = u64; // Key Type
    type R = RelationTrie<KT>;
    type RB = TrieBuilder<KT>;
    type JA = LeapfrogTriejoin;

    let arity = 1;
    let inputa = vec![vec![1u64], vec![2u64], vec![3u64]];
    let inputb = vec![vec![1u64], vec![2u64], vec![3u64]];
    let inputs = vec![inputa, inputb];
    let variables = vec![0];
    let rel_variables = vec![vec![0], vec![0]];
    compute_join::<KT, R, RB, JA>(arity, inputs, variables, rel_variables);
}

fn compute_db_join<KT, VT, KVST, R, RB, JA>(
    input1: Vec<Vec<KT>>, input2: Vec<Vec<KT>>,
) -> kermit::db::Database<KT, VT, KVST, R, RB>
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

#[test]
fn test_db_creation() {
    // Assuming you have a concrete implementation of KeyValStore, Relation, and
    // RelationBuilder Replace `ConcreteKVStore`, `ConcreteRelation`, and
    // `ConcreteRelationBuilder` with actual types
    compute_db_join::<
        u64,
        AnyValType,
        NaiveStore<_, std::hash::BuildHasherDefault<std::hash::DefaultHasher>>,
        RelationTrie<_>,
        TrieBuilder<_>,
        LeapfrogTriejoin,
    >(vec![vec![1_u64], vec![2], vec![3]], vec![
        vec![1_u64],
        vec![2],
        vec![3],
    ]);
}
