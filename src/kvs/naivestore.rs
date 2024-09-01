use {
    crate::kvs::{anyvaltype::AnyValType, keyvalstore::KeyValStore},
    csv::Error,
    nohash_hasher::BuildNoHashHasher,
    std::{
        collections::HashMap,
        fs::File,
        hash::{BuildHasher, BuildHasherDefault, DefaultHasher, Hash},
        path::Path,
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

    fn get(&self, key: &u64) -> Option<&VT> { self.map.get(key) }

    fn get_all(&self, key: Vec<&u64>) -> Vec<Option<&VT>> {
        key.into_iter().map(|k| self.get(k)).collect()
    }

    fn keys(&self) -> Vec<u64> { self.map.keys().cloned().collect() }

    fn size(&self) -> usize { self.map.len() }

    fn contains_key(&self, key: &u64) -> bool { self.map.contains_key(key) }

    fn contains_val(&self, val: &VT) -> bool { self.contains_key(&self.hash_builder.hash_one(val)) }
}

impl<VT, HB> NaiveStore<VT, HB>
where
    VT: Hash + Clone,
    HB: BuildHasher,
{
    pub fn with_hasher(hasher_builder: HB) -> Self {
        Self {
            map: HashMap::with_hasher(BuildNoHashHasher::<u64>::default()),
            hash_builder: hasher_builder,
        }
    }
}

impl<HB> NaiveStore<AnyValType, HB>
where
    HB: BuildHasher,
{
    pub fn add_file<P: AsRef<Path>>(
        &mut self, types: Vec<AnyValType>, filepath: P,
    ) -> Result<(), Error> {
        let file = File::open(filepath)?;
        let mut rdr = csv::ReaderBuilder::new()
            .has_headers(false)
            .delimiter(b',')
            .double_quote(false)
            .escape(Some(b'\\'))
            .flexible(false)
            .comment(Some(b'#'))
            .from_reader(file);
        for result in rdr.records() {
            let record = result?;
            for (i, x) in record.iter().enumerate() {
                let t = &types[i];
                let val = t.parse_into_self(x);
                self.add(val);
            }
        }
        Ok(())
    }
}

impl<VT> Default for NaiveStore<VT, BuildHasherDefault<DefaultHasher>>
where
    VT: Hash + Clone,
{
    fn default() -> Self {
        Self {
            map: HashMap::with_hasher(BuildNoHashHasher::<u64>::default()),
            hash_builder: BuildHasherDefault::<DefaultHasher>::default(),
        }
    }
}
