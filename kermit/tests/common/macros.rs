#[macro_export]
macro_rules! define_multiway_join_test {
    (
        $test_name:ident,
        $key_type:ty,
        $relation_type:ident,
        $join_algorithm:ty,
        [ $( $input:expr ),+ $(,)? ],
        $join_vars:expr,
        $projection:expr,
        $expected:expr,
        $debugger:block
    ) => {
        #[test]
        fn $test_name() {
            let inputs: Vec<Vec<Vec<$key_type>>> = vec![$($input.to_vec()),+];

            $debugger

            $crate::common::utils::test_join::<$relation_type<$key_type>, $join_algorithm>(
                inputs,
                $join_vars.to_vec(),
                $projection.to_vec(),
                $expected.to_vec(),
            );
        }
    };
}

#[macro_export]
macro_rules! define_unary_multiway_join_test {
    ($relation_type:ident, $join_algorithm:ty) => {
        paste::paste! {
        $crate::define_multiway_join_test!(
            [<simple_multiwayjoin_ $relation_type:lower _ $join_algorithm:lower>],
            u8,
            $relation_type,
            $join_algorithm,
            [
                vec![vec![1], vec![2], vec![3]],
                vec![vec![1], vec![2], vec![3]]
            ],
            vec![0],
            vec![vec![0]],
            vec![vec![1], vec![2], vec![3]],
            {print!("");}
        );
        }
    };
}

#[macro_export]
macro_rules! define_triangle_multiway_join_test {
    ($relation_type:ident, $join_algorithm:ty) => {
        paste::paste! {
        $crate::define_multiway_join_test!(
            [<triangle_ $relation_type:lower _ $join_algorithm:lower>],
            u8,
            $relation_type,
            $join_algorithm,
            [
                vec![vec![1, 2], vec![2, 3], vec![3, 1]],
                vec![vec![2, 3], vec![3, 1], vec![1, 2]],
                vec![vec![1, 3], vec![2, 1], vec![3, 2]]
            ],
            vec![0, 1, 2],
            vec![vec![0, 1], vec![1, 2], vec![0, 2]],
            vec![vec![1, 2, 3], vec![2, 3, 1], vec![3, 1, 2]],
            {print!("");}
        );
        }
    };
}

#[macro_export]
macro_rules! define_chain_multiway_join_test {
    ($relation_type:ident, $join_algorithm:ty) => {
        paste::paste! {
        $crate::define_multiway_join_test!(
            [<chain_ $relation_type:lower _ $join_algorithm:lower>],
            u8,
            $relation_type,
            $join_algorithm,
            [
                vec![vec![1, 2], vec![2, 3]],
                vec![vec![2, 4], vec![3, 5]],
                vec![vec![4, 6], vec![5, 7]]
            ],
            vec![0, 1, 2, 3],
            vec![vec![0, 1], vec![1, 2], vec![2, 3]],
            vec![vec![1, 2, 4, 6], vec![2, 3, 5, 7]],
            {print!("");}
        );
        }
    };
}

#[macro_export]
macro_rules! define_star_multiway_join_test {
    ($relation_type:ident, $join_algorithm:ty) => {
        paste::paste! {
        $crate::define_multiway_join_test!(
            [<star_ $relation_type:lower _ $join_algorithm:lower>],
            u8,
            $relation_type,
            $join_algorithm,
            [
                vec![vec![1, 10], vec![2, 20]],
                vec![vec![1, 100], vec![2, 200]]
            ],
            vec![0, 1, 2],
            vec![vec![0, 1], vec![0, 2]],
            vec![vec![1, 10, 100], vec![2, 20, 200]],
            {print!("");}
        );
        }
    };
}

#[macro_export]
macro_rules! define_self_multiway_join_test {
    ($relation_type:ident, $join_algorithm:ty) => {
        paste::paste! {
        $crate::define_multiway_join_test!(
            [<selfjoin_ $relation_type:lower _ $join_algorithm:lower>],
            u8,
            $relation_type,
            $join_algorithm,
            [
                vec![vec![1, 2], vec![2, 3], vec![3, 4]],
                vec![vec![2, 3], vec![3, 4], vec![4, 5]]
            ],
            vec![0, 1, 2],
            vec![vec![0, 1], vec![1, 2]],
            vec![vec![1, 2, 3], vec![2, 3, 4], vec![3, 4, 5]],
            {print!("");}
        );
        }
    };
}

#[macro_export]
macro_rules! define_existential_multiway_join_test {
    ($relation_type:ident, $join_algorithm:ty) => {
        paste::paste! {
        $crate::define_multiway_join_test!(
            [<existential_ $relation_type:lower _ $join_algorithm:lower>],
            u8,
            $relation_type,
            $join_algorithm,
            [
                vec![vec![1], vec![2], vec![3]],
                vec![vec![2], vec![3], vec![4]]
            ],
            vec![0],
            vec![vec![0], vec![0]],
            vec![vec![2], vec![3]],
            {print!("");}
        );
        }
    };
}

#[macro_export]
macro_rules! define_multiway_join_test_suite {
    (
        $(
            $relation_type:ident,
            $join_algorithm:ty
        ),+
    ) => {
        $(
                $crate::define_unary_multiway_join_test!(
                    $relation_type,
                    $join_algorithm
                );

                $crate::define_triangle_multiway_join_test!(
                    $relation_type,
                    $join_algorithm
                );

                $crate::define_chain_multiway_join_test!(
                    $relation_type,
                    $join_algorithm
                );

                $crate::define_star_multiway_join_test!(
                    $relation_type,
                    $join_algorithm
                );

                $crate::define_self_multiway_join_test!(
                    $relation_type,
                    $join_algorithm
                );

                $crate::define_existential_multiway_join_test!(
                    $relation_type,
                    $join_algorithm
                );
        )+
    };
}
