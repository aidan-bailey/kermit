#[macro_export]
macro_rules! define_multiway_join_test {
    (
        $test_name:ident,
        $relation_type:ident,
        $join_algorithm:ty,
        $optimiser:ty,
        [ $( $input:expr ),+ $(,)? ],
        $join_vars:expr,
        $projection:expr,
        $expected:expr,
        $debugger:block
    ) => {
        #[test]
        fn $test_name() {
            let inputs: Vec<Vec<Vec<usize>>> = vec![$($input.to_vec()),+];

            $debugger

            $crate::common::utils::test_join::<$relation_type, $join_algorithm, $optimiser>(
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
    ($relation_type:ident, $join_algorithm:ty, $optimiser:ty) => {
        paste::paste! {
        $crate::define_multiway_join_test!(
            [<simple_multiwayjoin_ $relation_type:lower _ $join_algorithm:lower _ $optimiser:lower>],
            $relation_type,
            $join_algorithm,
            $optimiser,
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
    ($relation_type:ident, $join_algorithm:ty, $optimiser:ty) => {
        paste::paste! {
        $crate::define_multiway_join_test!(
            [<triangle_ $relation_type:lower _ $join_algorithm:lower _ $optimiser:lower>],
            $relation_type,
            $join_algorithm,
            $optimiser,
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
    ($relation_type:ident, $join_algorithm:ty, $optimiser:ty) => {
        paste::paste! {
        $crate::define_multiway_join_test!(
            [<chain_ $relation_type:lower _ $join_algorithm:lower _ $optimiser:lower>],
            $relation_type,
            $join_algorithm,
            $optimiser,
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
    ($relation_type:ident, $join_algorithm:ty, $optimiser:ty) => {
        paste::paste! {
        $crate::define_multiway_join_test!(
            [<star_ $relation_type:lower _ $join_algorithm:lower _ $optimiser:lower>],
            $relation_type,
            $join_algorithm,
            $optimiser,
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
    ($relation_type:ident, $join_algorithm:ty, $optimiser:ty) => {
        paste::paste! {
        $crate::define_multiway_join_test!(
            [<selfjoin_ $relation_type:lower _ $join_algorithm:lower _ $optimiser:lower>],
            $relation_type,
            $join_algorithm,
            $optimiser,
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
    ($relation_type:ident, $join_algorithm:ty, $optimiser:ty) => {
        paste::paste! {
        $crate::define_multiway_join_test!(
            [<existential_ $relation_type:lower _ $join_algorithm:lower _ $optimiser:lower>],
            $relation_type,
            $join_algorithm,
            $optimiser,
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
macro_rules! define_empty_result_multiway_join_test {
    ($relation_type:ident, $join_algorithm:ty, $optimiser:ty) => {
        paste::paste! {
        $crate::define_multiway_join_test!(
            [<empty_result_ $relation_type:lower _ $join_algorithm:lower _ $optimiser:lower>],
            $relation_type,
            $join_algorithm,
            $optimiser,
            [
                vec![vec![1, 2], vec![3, 4]],
                vec![vec![5, 6], vec![7, 8]]
            ],
            vec![0, 1, 2],
            vec![vec![0, 1], vec![1, 2]],
            Vec::<Vec<usize>>::new(),
            {print!("");}
        );
        }
    };
}

#[macro_export]
macro_rules! define_single_relation_multiway_join_test {
    ($relation_type:ident, $join_algorithm:ty, $optimiser:ty) => {
        paste::paste! {
        $crate::define_multiway_join_test!(
            [<single_relation_ $relation_type:lower _ $join_algorithm:lower _ $optimiser:lower>],
            $relation_type,
            $join_algorithm,
            $optimiser,
            [
                vec![vec![1, 2], vec![3, 4], vec![5, 6]]
            ],
            vec![0, 1],
            vec![vec![0, 1]],
            vec![vec![1, 2], vec![3, 4], vec![5, 6]],
            {print!("");}
        );
        }
    };
}

#[macro_export]
macro_rules! define_four_way_chain_multiway_join_test {
    ($relation_type:ident, $join_algorithm:ty, $optimiser:ty) => {
        paste::paste! {
        $crate::define_multiway_join_test!(
            [<four_way_chain_ $relation_type:lower _ $join_algorithm:lower _ $optimiser:lower>],
            $relation_type,
            $join_algorithm,
            $optimiser,
            [
                vec![vec![1, 2]],
                vec![vec![2, 3]],
                vec![vec![3, 4]],
                vec![vec![4, 5]]
            ],
            vec![0, 1, 2, 3, 4],
            vec![vec![0, 1], vec![1, 2], vec![2, 3], vec![3, 4]],
            vec![vec![1, 2, 3, 4, 5]],
            {print!("");}
        );
        }
    };
}

#[macro_export]
macro_rules! define_wide_fanout_multiway_join_test {
    ($relation_type:ident, $join_algorithm:ty, $optimiser:ty) => {
        paste::paste! {
        $crate::define_multiway_join_test!(
            [<wide_fanout_ $relation_type:lower _ $join_algorithm:lower _ $optimiser:lower>],
            $relation_type,
            $join_algorithm,
            $optimiser,
            [
                vec![vec![1, 2], vec![1, 3], vec![1, 4]],
                vec![vec![1, 10], vec![1, 20]]
            ],
            vec![0, 1, 2],
            vec![vec![0, 1], vec![0, 2]],
            vec![
                vec![1, 2, 10], vec![1, 2, 20],
                vec![1, 3, 10], vec![1, 3, 20],
                vec![1, 4, 10], vec![1, 4, 20]
            ],
            {print!("");}
        );
        }
    };
}

#[macro_export]
macro_rules! define_dead_end_multiway_join_test {
    ($relation_type:ident, $join_algorithm:ty, $optimiser:ty) => {
        paste::paste! {
        $crate::define_multiway_join_test!(
            [<dead_end_ $relation_type:lower _ $join_algorithm:lower _ $optimiser:lower>],
            $relation_type,
            $join_algorithm,
            $optimiser,
            [
                vec![vec![1, 2], vec![2, 3], vec![3, 4]],
                vec![vec![2, 3], vec![3, 4]]
            ],
            vec![0, 1, 2],
            vec![vec![0, 1], vec![1, 2]],
            vec![vec![1, 2, 3], vec![2, 3, 4]],
            {print!("");}
        );
        }
    };
}

/// A chain whose *first* branch dead-ends at descent depth 3, with a valid
/// branch after it.
///
/// The existing `dead_end` pattern fails at depth 2, where a desync between a
/// join's depth and the tuple stack in `TrieIteratorWrapper` self-heals
/// (popping the emptied stack is a no-op). Only a failure at depth 3 or
/// deeper, with a surviving prefix still to enumerate, loses answers — see
/// the "A failed descent is atomic" invariant in
/// `docs/algorithms/leapfrog-triejoin.md`.
///
/// `Q(V0, V1, V2, V3) :- A(V0, V1), B(V1, V2), C(V2, V3).`
/// `V1 = 2` reaches `V2 = 5`, which `C` cannot extend — a dead end two
/// variables deep. The answer lives on the sibling branch `V1 = 3`, so an
/// implementation that mishandles the failed descent drops it and returns
/// nothing.
#[macro_export]
macro_rules! define_deep_dead_end_multiway_join_test {
    ($relation_type:ident, $join_algorithm:ty, $optimiser:ty) => {
        paste::paste! {
        $crate::define_multiway_join_test!(
            [<deep_dead_end_ $relation_type:lower _ $join_algorithm:lower _ $optimiser:lower>],
            $relation_type,
            $join_algorithm,
            $optimiser,
            [
                vec![vec![1, 2], vec![1, 3]],
                vec![vec![2, 5], vec![3, 6]],
                vec![vec![6, 7]]
            ],
            vec![0, 1, 2, 3],
            vec![vec![0, 1], vec![1, 2], vec![2, 3]],
            vec![vec![1, 3, 6, 7]],
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
            $join_algorithm:ty,
            $optimiser:ty
        ),+
    ) => {
        $(
                $crate::define_unary_multiway_join_test!( $relation_type, $join_algorithm, $optimiser );
                $crate::define_triangle_multiway_join_test!( $relation_type, $join_algorithm, $optimiser );
                $crate::define_chain_multiway_join_test!( $relation_type, $join_algorithm, $optimiser );
                $crate::define_star_multiway_join_test!( $relation_type, $join_algorithm, $optimiser );
                $crate::define_self_multiway_join_test!( $relation_type, $join_algorithm, $optimiser );
                $crate::define_existential_multiway_join_test!( $relation_type, $join_algorithm, $optimiser );
                $crate::define_empty_result_multiway_join_test!( $relation_type, $join_algorithm, $optimiser );
                $crate::define_single_relation_multiway_join_test!( $relation_type, $join_algorithm, $optimiser );
                $crate::define_four_way_chain_multiway_join_test!( $relation_type, $join_algorithm, $optimiser );
                $crate::define_wide_fanout_multiway_join_test!( $relation_type, $join_algorithm, $optimiser );
                $crate::define_dead_end_multiway_join_test!( $relation_type, $join_algorithm, $optimiser );
                $crate::define_deep_dead_end_multiway_join_test!( $relation_type, $join_algorithm, $optimiser );
        )+
    };
}
