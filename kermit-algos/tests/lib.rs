#[cfg(test)]
mod tests {
    use kermit_algos::leapfrog_triejoin::{
        leapfrog_triejoin, LeapfrogTriejoinIter, LeapfrogTriejoinIterator,
    };
    use kermit_ds::tuple_trie::{trie_iter::TrieIter, trie_builder::TrieBuilder};
    use kermit_iters::trie::TrieIterator;

    // Variable types
    #[test]
    fn figure1() {
        let trie_a = TrieBuilder::<i32>::new(1)
            .from_file("tests/data/a.csv")
            .unwrap()
            .build();
        let trie_b = TrieBuilder::<i32>::new(1)
            .from_file("tests/data/b.csv")
            .unwrap()
            .build();
        let trie_c = TrieBuilder::<i32>::new(1)
            .from_file("tests/data/c.csv")
            .unwrap()
            .build();
        let mut iter_a = TrieIter::new(&trie_a);
        iter_a.open().unwrap();
        let mut iter_b = TrieIter::new(&trie_b);
        iter_b.open().unwrap();
        let mut iter_c = TrieIter::new(&trie_c);
        iter_c.open().unwrap();
        let triejoin = LeapfrogTriejoinIter::new(vec![iter_a, iter_b, iter_c]);
        assert_eq!(triejoin.key.unwrap(), 8);
    }

    #[test]
    fn full_join() {
        let trie_a = TrieBuilder::<i32>::new(1)
            .from_file("tests/data/onetoten.csv")
            .unwrap()
            .build();
        let trie_b = TrieBuilder::<i32>::new(1)
            .from_file("tests/data/onetoten.csv")
            .unwrap()
            .build();
        let trie_c = TrieBuilder::<i32>::new(1)
            .from_file("tests/data/onetoten.csv")
            .unwrap()
            .build();
        let mut iter_a = TrieIter::new(&trie_a);
        iter_a.open().unwrap();
        let mut iter_b = TrieIter::new(&trie_b);
        iter_b.open().unwrap();
        let mut iter_c = TrieIter::new(&trie_c);
        iter_c.open().unwrap();
        let mut triejoin = LeapfrogTriejoinIter::new(vec![iter_a, iter_b, iter_c]);
        assert_eq!(triejoin.key.unwrap(), 1);
        for i in 2..11 {
            assert_eq!(triejoin.next().unwrap(), &i);
        }
    }

    #[test]
    fn join3() {
        let trie_a = TrieBuilder::<i32>::new(1)
            .from_file("tests/data/col_a.csv")
            .unwrap()
            .build();
        let trie_b = TrieBuilder::<i32>::new(1)
            .from_file("tests/data/col_b.csv")
            .unwrap()
            .build();
        let trie_c = TrieBuilder::<i32>::new(1)
            .from_file("tests/data/col_c.csv")
            .unwrap()
            .build();
        let mut iter_a = TrieIter::new(&trie_a);
        iter_a.open().unwrap();
        let mut iter_b = TrieIter::new(&trie_b);
        iter_b.open().unwrap();
        let mut iter_c = TrieIter::new(&trie_c);
        iter_c.open().unwrap();
        let mut triejoin = LeapfrogTriejoinIter::new(vec![iter_a, iter_b, iter_c]);
        assert_eq!(triejoin.key.unwrap(), 7);
        assert_eq!(triejoin.next().unwrap(), &10);
        assert_eq!(triejoin.next().unwrap(), &20);
        assert!(triejoin.next().is_none());
        assert!(triejoin.at_end());
    }

    // Variable types
    #[test]
    fn algo() {
        let trie_a = TrieBuilder::<i32>::new(1)
            .from_file("tests/data/onetoten.csv")
            .unwrap()
            .build();
        let trie_b = TrieBuilder::<i32>::new(1)
            .from_file("tests/data/onetoten.csv")
            .unwrap()
            .build();
        let trie_c = TrieBuilder::<i32>::new(1)
            .from_file("tests/data/onetoten.csv")
            .unwrap()
            .build();
        let res = leapfrog_triejoin(vec![&trie_a, &trie_b, &trie_c]);
        assert_eq!(
            res,
            vec![
                vec![1],
                vec![2],
                vec![3],
                vec![4],
                vec![5],
                vec![6],
                vec![7],
                vec![8],
                vec![9],
                vec![10]
            ]
        );
    }
}
