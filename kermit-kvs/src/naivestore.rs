use {
    crate::{anyvaltype::AnyValType, keyvalstore::KeyValStore}, csv::Error, nohash_hasher::BuildNoHashHasher, std::{
        collections::HashMap, fs::File, hash::{BuildHasher, BuildHasherDefault, DefaultHasher, Hash, Hasher}, path::Path
    }
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
        let mut hasher = self.hash_builder.build_hasher();
        val.hash(&mut hasher);
        let hash = hasher.finish();
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

    fn keys(&self) -> Vec<u64> {
        self.map.keys().cloned().collect()
    }
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

impl<HB> NaiveStore<AnyValType, HB> where HB: BuildHasher {
    pub fn add_file<P: AsRef<Path>>(&mut self, types: Vec<AnyValType>, filepath: P) -> Result<(), Error> {
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

#[cfg(test)]
mod tests {
    use {super::*, crate::anyvaltype::*};

    #[test]
    fn test_default() {
        let mut store = NaiveStore::<String, _>::default();
        let key1 = store.add("hello".to_string());
        let key2 = store.add("world".to_string());
        assert_eq!(store.get(&key1), Some(&"hello".to_string()));
        assert_eq!(store.get(&key2), Some(&"world".to_string()));
        assert_eq!(store.get(&0), None);
        assert_eq!(store.get_all(vec![&key1, &key2, &0]), vec![
            Some(&"hello".to_string()),
            Some(&"world".to_string()),
            None
        ]);
    }

    #[test]
    fn test_anyvaltype() {
        let mut store = NaiveStore::<AnyValType, _>::default();
        let str_key1 = store.add(AnyValType::from("hello"));
        let str_key2 = store.add(AnyValType::from("world"));
        assert_eq!(store.get(&str_key1), Some(&AnyValType::from("hello")));
        assert_eq!(store.get(&str_key2), Some(&AnyValType::from("world")));
        let float_key1 = store.add(AnyValType::F64(0.5));
        assert_eq!(store.get(&float_key1), Some(&AnyValType::F64(0.5)));
    }
}
