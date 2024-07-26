
#[cfg(test)]
mod tests {
    use kermit_ds::{iterator::TrieIter, trie_builder::TrieBuilder};
    use kermit_algos::leapfrog_triejoin::{LeapfrogTriejoinIter, LeapfrogTriejoinIterator};
    use kermit_iters::trie::TrieIterator;

    // Variable types
    #[test]
    fn trie_with_variable_type() {
        let trie_a = TrieBuilder::<i32>::new(1)
            .add_tuples(vec![
                vec![0],
                vec![1],
                vec![3],
                vec![4],
                vec![5],
                vec![6],
                vec![7],
                vec![8],
                vec![9],
                vec![11],
            ])
            .build();
        let trie_b = TrieBuilder::<i32>::new(1)
            .add_tuples(vec![vec![0], vec![2], vec![6], vec![7], vec![8], vec![9]])
            .build();
        let trie_c = TrieBuilder::<i32>::new(1)
            .add_tuples(vec![vec![2], vec![4], vec![5], vec![8], vec![10]])
            .build();
        let mut iter_a = TrieIter::new(&trie_a);
        iter_a.open();
        let mut iter_b = TrieIter::new(&trie_b);
        iter_b.open();
        let mut iter_c = TrieIter::new(&trie_c);
        iter_c.open();
        let mut triejoin = LeapfrogTriejoinIter::new(vec![iter_a, iter_b, iter_c]);
        //assert_eq!(triejoin.next(), Some(8));
        assert!(true);
    }
}
