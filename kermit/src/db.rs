use {
    crate::{
        ds::{relation::Relation, relation_builder::RelationBuilder},
        kvs::keyvalstore::KeyValStore,
    }, kermit_algos::join_algo::JoinAlgo, kermit_iters::trie::Iterable, std::{collections::HashMap, fmt::Debug, hash::Hash, str::FromStr}
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

impl<'a, KT, VT, KVST, R, RB> Database<KT, VT, KVST, R, RB>
where
    KT: Debug + FromStr + PartialOrd + PartialEq + Clone + Hash + std::cmp::Eq + Ord,
    KVST: KeyValStore<KT, VT>,
    VT: Hash,
    R: Relation<KT> + Iterable<'a, KT>,
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

    pub fn add_relation(&mut self, name: String, cardinality: usize) {
        let relation = RB::new(cardinality).build();
        self.relations.insert(name, relation);
    }

    pub fn add_tuple(&mut self, relation_name: &String, tuple: Vec<VT>) {
        let keys = self.store.add_all(tuple);
        self.relations.get_mut(relation_name).unwrap().insert(keys);
    }

    pub fn join<JA>(
        &self, relations: Vec<String>, variables: Vec<usize>, rel_variables: Vec<Vec<usize>>,
    )// -> R
    where
        JA: JoinAlgo<'a, KT, R>,
    {
        let iterables = relations
            .iter()
            .map(|name| self.relations.get(name).unwrap())
            .collect::<Vec<&R>>();
        JA::join(variables, rel_variables, iterables)
    }
}

#[cfg(test)]
mod tests {

    use {
        super::*,
        crate::{
            ds::relation_trie::{trie::RelationTrie, trie_builder::TrieBuilder},
            kvs::{anyvaltype::AnyValType, naivestore::NaiveStore},
        }, kermit_algos::leapfrog_triejoin::LeapfrogTriejoin
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
        db.add_relation(relation_name.clone(), 3);
        let tuple = vec![
            AnyValType::from("Apple"),
            AnyValType::from(1),
            AnyValType::from(2),
        ];
        db.join::<LeapfrogTriejoin>(vec!["apple".to_string()], vec![], vec![]);
        db.add_tuple(&relation_name, tuple)
    }
}
