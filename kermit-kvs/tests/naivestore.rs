#[cfg(test)]
mod tests {
    use kermit_kvs::{anyvaltype::*, keyvalstore::*, naivestore::*};

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

    #[test]
    fn read_file() {
        let mut store = NaiveStore::<AnyValType, _>::default();
        store
            .add_file(
                vec![
                    AnyValType::default_str(),
                    AnyValType::default_str(),
                    AnyValType::default_str(),
                ],
                "test1.csv",
            )
            .unwrap();
        store
            .add_file(
                vec![
                    AnyValType::default_str(),
                    AnyValType::default_str(),
                    AnyValType::default_i32(),
                    AnyValType::default_i32(),
                ],
                "test2.csv",
            )
            .unwrap();
    }
}
