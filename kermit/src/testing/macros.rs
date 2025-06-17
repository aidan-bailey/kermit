#[macro_export]
macro_rules! define_join_test {
    (
        $test_name:ident,                // fn name
        $relation_type:ty,              // RelationTrie<u64>
        $join_algorithm:ty,             // LeapfrogTriejoin
        $arity:expr,                    // arity (e.g. 1 or 2)
        [ $( $input:expr ),+ $(,)? ],   // input relations
        $join_vars:expr,                // join variable indices
        $projection:expr,               // projected columns
        $expected:expr                  // expected result
    ) => {
        #[test]
        fn $test_name() {
            let inputs: Vec<Vec<Vec<u64>>> = vec![$($input.to_vec()),+];

            $crate::testing::utils::test_join::<$relation_type, $join_algorithm>(
                $arity,
                inputs,
                $join_vars.to_vec(),
                $projection.to_vec(),
                $expected.to_vec(),
            );
        }
    };
}

#[macro_export]
macro_rules! define_simple_join_test {
    ($test_name:ident, $relation_type:ty, $join_algorithm:ty) => {
        $crate::define_join_test!(
            $test_name,
            $relation_type,
            $join_algorithm,
            1, // Arity (unary relations)
            [
                // Input relations
                vec![vec![1u64], vec![2u64], vec![3u64]],
                vec![vec![1u64], vec![2u64], vec![3u64]]
            ],
            vec![0],                                  // Join variables
            vec![vec![0]],                            // Projection
            vec![vec![1u64], vec![2u64], vec![3u64]]  // Expected output
        );
    };
}

#[macro_export]
macro_rules! define_join_test_suite {
    (
        $(
            $test_name:ident,                // fn name
            $relation_type:ty,              // RelationTrie<u64>
            $join_algorithm:ty,             // LeapfrogTriejoin
        ),+
    ) => {
        $(
            paste::paste! {
                $crate::define_simple_join_test!(
                    [<$test_name _simple_join>],
                    $relation_type,
                    $join_algorithm
                );
            }
        )+
    };
}
