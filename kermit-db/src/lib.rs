use {
    kermit_ds::relation::Relation, kermit_kvs::keyvalstore::KeyValStore, std::{collections::HashMap, hash::Hash},
};

pub struct Database<KT, VT, KVST>
where
    KT: PartialOrd + PartialEq + Clone + Hash + std::cmp::Eq,
    KVST: KeyValStore<KT, VT>, VT: Hash
{
    name: String,
    relations: HashMap<String, Box<dyn Relation<KT>>>,
    store: KVST,
    phantom_vt: std::marker::PhantomData<VT>,
}

impl<KT, VT, KVST> Database<KT, VT, KVST>
where
    KT: PartialOrd + PartialEq + Clone + Hash + std::cmp::Eq,
    KVST: KeyValStore<KT, VT>, VT: Hash
{
    pub fn new(name: String, store: KVST) -> Self {
        Database {
            name,
            relations: HashMap::new(),
            store,
            phantom_vt: std::marker::PhantomData,
        }
    }

    pub fn add_relation(&mut self, name: String, relation: Box<dyn Relation<KT>>) {
        self.relations.insert(name, relation);
    }

    pub fn get_relation(&self, name: &str) -> Option<&Box<dyn Relation<KT>>>
    {
        self.relations.get(name)
    }

    pub fn get_store(&self) -> &KVST {
        &self.store
    }

    pub fn get_name(&self) -> &str {
        &self.name
    }
}
