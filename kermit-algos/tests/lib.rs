#[cfg(test)]
mod tests {
    use kermit_algos::leapfrog_triejoin::{LeapfrogTriejoinIter, LeapfrogTriejoinIterator};
    use kermit_ds::{iterator::TrieIter, trie_builder::TrieBuilder};
    use kermit_iters::trie::TrieIterator;

    // Variable types
    #[test]
    fn figure1() {
        let trie_a = TrieBuilder::<i32>::new(1).from_file("tests/data/a.csv").unwrap().build();
        let trie_b = TrieBuilder::<i32>::new(1).from_file("tests/data/b.csv").unwrap().build();
        let trie_c = TrieBuilder::<i32>::new(1).from_file("tests/data/c.csv").unwrap().build();
        let mut iter_a = TrieIter::new(&trie_a);
        iter_a.open().unwrap();
        let mut iter_b = TrieIter::new(&trie_b);
        iter_b.open().unwrap();
        let mut iter_c = TrieIter::new(&trie_c);
        iter_c.open().unwrap();
        let mut triejoin = LeapfrogTriejoinIter::new(vec![iter_a, iter_b, iter_c]);
        assert_eq!(triejoin.key.unwrap(), 8);
    }

    #[test]
    fn full_join() {
        let trie_a = TrieBuilder::<i32>::new(1).from_file("tests/data/onetoten.csv").unwrap().build();
        let trie_b = TrieBuilder::<i32>::new(1).from_file("tests/data/onetoten.csv").unwrap().build();
        let trie_c = TrieBuilder::<i32>::new(1).from_file("tests/data/onetoten.csv").unwrap().build();
        let mut iter_a = TrieIter::new(&trie_a);
        iter_a.open().unwrap();
        let mut iter_b = TrieIter::new(&trie_b);
        iter_b.open().unwrap();
        let mut iter_c = TrieIter::new(&trie_c);
        iter_c.open().unwrap();
        let mut triejoin = LeapfrogTriejoinIter::new(vec![iter_a, iter_b, iter_c]);
        assert_eq!(triejoin.key.unwrap(), 1);
        for i in 2..11 {
            triejoin.next().expect("Hello");
            assert_eq!(triejoin.key.unwrap(), i);
        }
    }

    #[test]
    fn join3() {
        let trie_a = TrieBuilder::<i32>::new(1).from_file("tests/data/col_a.csv").unwrap().build();
        let trie_b = TrieBuilder::<i32>::new(1).from_file("tests/data/col_b.csv").unwrap().build();
        let trie_c = TrieBuilder::<i32>::new(1).from_file("tests/data/col_c.csv").unwrap().build();
        let mut iter_a = TrieIter::new(&trie_a);
        iter_a.open().unwrap();
        let mut iter_b = TrieIter::new(&trie_b);
        iter_b.open().unwrap();
        let mut iter_c = TrieIter::new(&trie_c);
        iter_c.open().unwrap();
        let mut triejoin = LeapfrogTriejoinIter::new(vec![iter_a, iter_b, iter_c]);
        assert_eq!(triejoin.key.unwrap(), 7);
        triejoin.next().expect("Hello");
        assert_eq!(triejoin.key.unwrap(), 10);
        triejoin.next().expect("Hello");
        assert_eq!(triejoin.key.unwrap(), 20);
        triejoin.next().expect("Hello");
        assert!(triejoin.at_end());
    }
}
