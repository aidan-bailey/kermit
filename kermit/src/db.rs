use {
    kermit_algos::join_algo::JoinAlgo,
    kermit_ds::{relation::Relation, relation_builder::RelationBuilder},
    kermit_iters::trie::Iterable,
    kermit_kvs::keyvalstore::KeyValStore,
    std::{collections::HashMap, fmt::Debug, hash::Hash, str::FromStr},
};

pub struct Database<KT, VT, KVST, R, RB>
where
    KT: Debug + FromStr + PartialOrd + PartialEq + Clone + Hash + std::cmp::Eq,
    KVST: KeyValStore<KT, VT>,
    VT: Hash,
    R: Relation<KT>,
    RB: RelationBuilder<KT, R>,
{
    name: String,
    relations: HashMap<String, R>,
    store: KVST,
    phantom_vt: std::marker::PhantomData<VT>,
    phantom_kt: std::marker::PhantomData<KT>,
    phantom_rb: std::marker::PhantomData<RB>,
}

impl<KT, VT, KVST, R, RB> Database<KT, VT, KVST, R, RB>
where
    KT: Debug + FromStr + PartialOrd + PartialEq + Clone + Hash + std::cmp::Eq + Ord,
    KVST: KeyValStore<KT, VT>,
    VT: Hash,
    R: Relation<KT> + Iterable<KT>,
    RB: RelationBuilder<KT, R>,
{
    pub fn new(name: String, store: KVST) -> Self {
        Database {
            name,
            relations: HashMap::new(),
            store,
            phantom_vt: std::marker::PhantomData,
            phantom_kt: std::marker::PhantomData,
            phantom_rb: std::marker::PhantomData,
        }
    }

    pub fn name(&self) -> &String { &self.name }

    pub fn add_relation(&mut self, name: &str, cardinality: usize) {
        let relation = RB::new(cardinality).build();
        self.relations.insert(name.to_owned(), relation);
    }

    pub fn add_tuple(&mut self, relation_name: &str, tuple: Vec<VT>) {
        let keys = self.store.add_all(tuple);
        self.relations.get_mut(relation_name).unwrap().insert(keys);
    }

    pub fn add_keys(&mut self, relation_name: &str, keys: Vec<KT>) {
        self.relations.get_mut(relation_name).unwrap().insert(keys);
    }

    pub fn add_keys_batch(&mut self, relation_name: &str, keys: Vec<Vec<KT>>) {
        self.relations
            .get_mut(relation_name)
            .unwrap()
            .insert_all(keys);
    }

    pub fn join<JA>(
        &self, relations: Vec<String>, variables: Vec<usize>, rel_variables: Vec<Vec<usize>>,
    ) -> R
    where
        JA: JoinAlgo<KT, R>,
    {
        let iterables = relations
            .iter()
            .map(|name| self.relations.get(name).unwrap())
            .collect::<Vec<&R>>();
        let cardinality = variables.len();
        let tuples = JA::join(variables, rel_variables, iterables);
        RB::new(cardinality).add_tuples(tuples).build()
    }
}

#[cfg(test)]
mod tests {

    use {
        super::*,
        kermit_algos::leapfrog_triejoin::LeapfrogTriejoin,
        kermit_ds::relation_trie::{trie::RelationTrie, trie_builder::TrieBuilder},
        kermit_kvs::{anyvaltype::AnyValType, naivestore::NaiveStore},
    };

    #[test]
    fn test_relation() {
        let mut db: Database<
            u64,
            AnyValType,
            NaiveStore<_, std::hash::BuildHasherDefault<std::hash::DefaultHasher>>,
            RelationTrie<_>,
            TrieBuilder<_>,
        > = Database::new("test".to_string(), NaiveStore::<AnyValType, _>::default());
        let relation_name = "apple".to_string();
        db.add_relation(&relation_name, 3);
        let tuple = vec![
            AnyValType::from("Apple"),
            AnyValType::from(1),
            AnyValType::from(2),
        ];
        db.add_tuple(&relation_name, tuple)
    }

    #[test]
    fn test_join() {
        let mut db: Database<
            u64,
            AnyValType,
            NaiveStore<_, std::hash::BuildHasherDefault<std::hash::DefaultHasher>>,
            RelationTrie<_>,
            TrieBuilder<_>,
        > = Database::new("test".to_string(), NaiveStore::<AnyValType, _>::default());

        db.add_relation("first", 1);
        db.add_keys_batch("first", vec![vec![1_u64], vec![2], vec![3]]);

        db.add_relation("second", 1);
        db.add_keys_batch("second", vec![vec![1_u64], vec![2], vec![3]]);

        let _res = db.join::<LeapfrogTriejoin>(
            vec!["first".to_string(), "second".to_string()],
            vec![0],
            vec![vec![0], vec![0]],
        );
    }
}
