use {
    crate::keyvalstore::KeyValStore,
    nohash_hasher::BuildNoHashHasher,
    std::{
        collections::HashMap,
        hash::{BuildHasher, BuildHasherDefault, DefaultHasher, Hash},
    },
};

pub struct NaiveStore<VT, HB>
where
    VT: Hash + Clone,
    HB: BuildHasher,
{
    map: HashMap<u64, VT, BuildNoHashHasher<u64>>,
    hash_builder: HB,
}

impl<VT, HB> KeyValStore<u64, VT> for NaiveStore<VT, HB>
where
    VT: Hash + Clone,
    HB: BuildHasher,
{
    fn add(&mut self, val: VT) -> u64 {
        let hash = self.hash_builder.hash_one(&val);
        self.map.insert(hash, val);
        hash
    }

    fn add_all(&mut self, val: Vec<VT>) -> Vec<u64> {
        val.into_iter().map(|v| self.add(v)).collect()
    }

    fn get(&self, key: &u64) -> Option<&VT> {
        let hash = self.hash_builder.hash_one(&key);
        self.map.get(&hash)
    }

    fn get_all(&self, key: Vec<&u64>) -> Vec<Option<&VT>> {
        key.into_iter().map(|k| self.get(&k)).collect()
    }
}

impl<VT, HB> NaiveStore<VT, HB>
where
    VT: Hash + Clone,
    HB: BuildHasher,
{
    pub fn with_hasher(hasher: HB) -> Self {
        Self {
            map: HashMap::with_hasher(BuildNoHashHasher::<u64>::default()),
            hash_builder: hasher,
        }
    }
}

impl<VT> NaiveStore<VT, BuildHasherDefault<DefaultHasher>>
where
    VT: Hash + Clone,
{
    pub fn new() -> Self {
        Self {
            map: HashMap::with_hasher(BuildNoHashHasher::<u64>::default()),
            hash_builder: BuildHasherDefault::<DefaultHasher>::default(),
        }
    }
}
