#[macro_export]
macro_rules! define_multiway_join_test {
    (
        $test_name:ident,
        $key_type:ty,
        $relation_type:ident,
        $join_algorithm:ty,
        $arity:expr,
        [ $( $input:expr ),+ $(,)? ],
        $join_vars:expr,
        $projection:expr,
        $expected:expr
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
            1,
            [
                vec![vec![1], vec![2], vec![3]],
                vec![vec![1], vec![2], vec![3]]
            ],
            vec![0],
            vec![vec![0]],
            vec![vec![1], vec![2], vec![3]]
        );
        }
    };
}

#[macro_export]
macro_rules! define_triangle_multiway_join_test {
    ($key_type:ty, $relation_type:ident, $join_algorithm:ty) => {
        paste::paste! {
        $crate::define_multiway_join_test!(
            [<triangle_ $relation_type:lower _ $join_algorithm:lower _ $key_type:lower>],
            $key_type,
            $relation_type,
            $join_algorithm,
            2,
            [
                vec![vec![1, 2], vec![2, 3], vec![3, 1]],
                vec![vec![2, 3], vec![3, 1], vec![1, 2]],
                vec![vec![3, 1], vec![1, 2], vec![2, 3]]
            ],
            vec![0, 1, 2],
            vec![vec![0, 1], vec![1, 2], vec![2, 3]],
            vec![vec![1, 2, 3], vec![2, 3, 1], vec![3, 1, 2]]
        );
        }
    };
}

#[macro_export]
macro_rules! define_chainjoin_multiway_join_test {
    ($key_type:ty, $relation_type:ident, $join_algorithm:ty) => {
        paste::paste! {
        $crate::define_multiway_join_test!(
            [<chainjoin_ $relation_type:lower _ $join_algorithm:lower _ $key_type:lower>],
            $key_type,
            $relation_type,
            $join_algorithm,
            2,
            [
                vec![vec![1, 2], vec![2, 3]],
                vec![vec![2, 4], vec![3, 5]],
                vec![vec![4, 6], vec![5, 7]]
            ],
            vec![0, 1, 2, 3],
            vec![vec![0, 1], vec![1, 2], vec![2, 3]],
            vec![vec![1, 2, 4, 6], vec![2, 3, 5, 7]]
        );
        }
    };
}

#[macro_export]
macro_rules! define_multiway_join_test_suite {
    (
        $(
            $relation_type:ident,
            $join_algorithm:ty,
        ),+
    ) => {
        $(
                $crate::define_simple_multiway_join_test!(
                    u64,
                    $relation_type,
                    $join_algorithm
                );

            /*
                $crate::define_triangle_multiway_join_test!(
                    u64,
                    $relation_type,
                    $join_algorithm
                );
            */

                $crate::define_chainjoin_multiway_join_test!(
                    u64,
                    $relation_type,
                    $join_algorithm
                );
        )+
    };
}
