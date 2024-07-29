#[cfg(test)]
mod tests {
    use kermit_algos::leapfrog_triejoin::leapfrog_triejoin;
    use kermit_ds::tuple_trie::trie_builder::TrieBuilder;
    use test_case::test_case;

    #[test_case(
        vec!["tests/data/a.csv", "tests/data/b.csv", "tests/data/c.csv"],
        vec![vec![8]];
        "a,b,c"
    )]
    #[test_case(
        vec!["tests/data/onetoten.csv", "tests/data/onetoten.csv", "tests/data/onetoten.csv"],
        vec![vec![1], vec![2], vec![3], vec![4], vec![5], vec![6], vec![7], vec![8], vec![9], vec![10]];
        "onetoten x 3"
    )]
    #[test_case(
        vec!["tests/data/col_a.csv", "tests/data/col_b.csv", "tests/data/col_c.csv"],
        vec![vec![7], vec![10], vec![20]];
        "col_a, col_b, col_c"
    )]
    fn test_files(file_paths: Vec<&'static str>, expected: Vec<Vec<i32>>) {
        let tries: Vec<_> = file_paths
            .iter()
            .map(|file_path| {
                TrieBuilder::<i32>::new(1)
                    .from_file(file_path)
                    .unwrap()
                    .build()
            })
            .collect();
        let res = leapfrog_triejoin(tries.iter().collect());
        assert_eq!(res, expected);
    }
}
