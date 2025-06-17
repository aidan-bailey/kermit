#[macro_export]
macro_rules! define_multiway_join_test {
    (
        $test_name:ident,                // fn name
        $key_type:ty,
        $relation_type:ident,              // RelationTrie<u64>
        $join_algorithm:ty,             // LeapfrogTriejoin
        $arity:expr,                    // arity (e.g. 1 or 2)
        [ $( $input:expr ),+ $(,)? ],   // input relations
        $join_vars:expr,                // join variable indices
        $projection:expr,               // projected columns
        $expected:expr                  // expected result
    ) => {
        #[test]
        fn $test_name() {
            let inputs: Vec<Vec<Vec<$key_type>>> = vec![$($input.to_vec()),+];

            $crate::testing::utils::test_join::<$relation_type<$key_type>, $join_algorithm>(
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
macro_rules! define_simple_multiway_join_test {
    ($key_type:ty, $relation_type:ident, $join_algorithm:ty) => {
        paste::paste! {
        $crate::define_multiway_join_test!(
            [<simple_multiwayjoin_ $relation_type:lower _ $join_algorithm:lower _ $key_type:lower>],
            $key_type,
            $relation_type,
            $join_algorithm,
            1, // Arity (unary relations)
            [
                vec![vec![1 as $key_type], vec![2], vec![3]],
                vec![vec![1], vec![2], vec![3]]
            ],
            vec![0],                                  // Join variables
            vec![vec![0]],                            // Projection
            vec![vec![1 as $key_type], vec![2], vec![3]]  // Expected output
        );
        }
    };
}

#[macro_export]
macro_rules! define_multiway_join_test_suite {
    (
        $(
            $relation_type:ident,              // RelationTrie
            $join_algorithm:ty,             // LeapfrogTriejoin
        ),+
    ) => {
        $(
                $crate::define_simple_multiway_join_test!(
                    u64,
                    $relation_type,
                    $join_algorithm
                );
        )+
    };
}
