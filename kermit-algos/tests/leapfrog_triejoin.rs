#[cfg(test)]
mod tests {
    use {
        kermit_algos::leapfrog_triejoin::{LeapfrogTriejoinIter, LeapfrogTriejoinIterator},
        kermit_ds::relation_trie::trie_builder::TrieBuilder,
        kermit_iters::trie::TrieIterable,
    };

    #[test]
    fn test_classic() {
        let t1 = TrieBuilder::<i32>::new(1)
            .add_tuples(vec![vec![1], vec![2], vec![3]])
            .build();
        let t2 = TrieBuilder::<i32>::new(1)
            .add_tuples(vec![vec![1], vec![2], vec![3]])
            .build();
        let t1_iter = t1.trie_iter();
        let t2_iter = t2.trie_iter();
        let mut triejoin_iter =
            LeapfrogTriejoinIter::new(vec![0], vec![vec![0], vec![0]], vec![t1_iter, t2_iter]);
        triejoin_iter.open();
        assert_eq!(triejoin_iter.key.unwrap(), 1);
        assert_eq!(triejoin_iter.next().unwrap(), &2);
        assert_eq!(triejoin_iter.next().unwrap(), &3);
    }

    #[test]
    fn more_complicated() {
        let r = TrieBuilder::<i32>::new(2)
            .add_tuples(vec![vec![7, 4]])
            .build();
        let s = TrieBuilder::<i32>::new(2)
            .add_tuples(vec![vec![4, 1], vec![4, 4], vec![4, 5], vec![4, 9]])
            .build();
        let t = TrieBuilder::<i32>::new(2)
            .add_tuples(vec![vec![7, 2], vec![7, 3], vec![7, 5]])
            .build();
        let r_iter = r.trie_iter();
        let s_iter = s.trie_iter();
        let t_iter = t.trie_iter();
        let mut triejoin_iter = LeapfrogTriejoinIter::new(
            vec![0, 1, 2],
            vec![vec![0, 1], vec![1, 2], vec![0, 2]],
            vec![r_iter, s_iter, t_iter],
        );
        triejoin_iter.open();
        assert_eq!(triejoin_iter.key.unwrap(), 7);
        assert!(triejoin_iter.next().is_none());
        triejoin_iter.open();
        assert_eq!(triejoin_iter.key.unwrap(), 4);
        assert!(triejoin_iter.next().is_none());
        triejoin_iter.open();
        assert_eq!(triejoin_iter.key.unwrap(), 5);
    }

    // #[test_case(
    // vec!["tests/data/a.csv", "tests/data/b.csv", "tests/data/c.csv"],
    // vec![vec![8]];
    // "a,b,c"
    // )]
    // #[test_case(
    // vec!["tests/data/onetoten.csv", "tests/data/onetoten.csv",
    // "tests/data/onetoten.csv"], vec![vec![1], vec![2], vec![3], vec![4],
    // vec![5], vec![6], vec![7], vec![8], vec![9], vec![10]]; "onetoten x
    // 3" )]
    // #[test_case(
    // vec!["tests/data/col_a.csv", "tests/data/col_b.csv",
    // "tests/data/col_c.csv"], vec![vec![7], vec![10], vec![20]];
    // "col_a, col_b, col_c"
    // )]
    // fn test_files(file_paths: Vec<&'static str>, expected: Vec<Vec<i32>>) {
    // let tries: Vec<_> = file_paths
    // .iter()
    // .map(|file_path| {
    // TrieBuilder::<i32>::new(1)
    // .from_file(file_path)
    // .unwrap()
    // .build()
    // })
    // .collect();
    // let res = leapfrog_triejoin(tries.iter().collect());
    // assert_eq!(res, expected);
    // }
    //
    // #[test_case(
    // 1,
    // vec![
    // vec![vec![1], vec![2], vec![3]],
    // vec![vec![1], vec![2], vec![3]]
    // ],
    // vec![vec![1], vec![2], vec![3]];
    // "1-ary"
    // )]
    // fn test_inputs_outputs(arity: usize, inputs: Vec<Vec<Vec<i32>>>,
    // expected: Vec<Vec<i32>>) { let tries: Vec<_> = inputs
    // .into_iter()
    // .map(|input| TrieBuilder::<i32>::new(arity).add_tuples(input).build())
    // .collect();
    // let res = leapfrog_triejoin(tries.iter().collect());
    // assert_eq!(res, expected);
    // }
}
