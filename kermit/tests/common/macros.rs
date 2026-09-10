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

/// `Q(V0) :- R0(V0, V0).` — a variable repeated inside one atom, the
/// PROBLEMS.md diagonal. `R0 = {(1,1),(1,2),(2,3),(3,3),(4,5)}` has exactly
/// two diagonal tuples. An executor that registers `R0` once "for `V0`"
/// opens only its first column and returns every subject (`{1,2,3,4}`),
/// or reaches the leaf one level early and panics; the selection rewrite
/// keeps both from ever seeing the repeat.
#[macro_export]
macro_rules! define_diagonal_multiway_join_test {
    ($relation_type:ident, $join_algorithm:ty, $optimiser:ty) => {
        paste::paste! {
        $crate::define_multiway_join_test!(
            [<diagonal_ $relation_type:lower _ $join_algorithm:lower _ $optimiser:lower>],
            $relation_type,
            $join_algorithm,
            $optimiser,
            [
                vec![vec![1, 1], vec![1, 2], vec![2, 3], vec![3, 3], vec![4, 5]]
            ],
            vec![0],
            vec![vec![0, 0]],
            vec![vec![1], vec![3]],
            {print!("");}
        );
        }
    };
}

/// `Q(V0, V1) :- R0(V0, V1, V0), R1(V1).` — the repeat is *not* adjacent
/// to its first occurrence, and another relation joins in between. Of
/// `R0 = {(1,2,1),(1,2,3),(2,5,2),(3,7,3)}`, `(1,2,3)` dies at the
/// constrained third column and `(2,5,2)` dies on `R1 = {2, 7}`, leaving
/// `(1,2)` and `(3,7)`. Guards the case where the admitted key comes from
/// a level other than the immediate parent.
#[macro_export]
macro_rules! define_repeated_nonadjacent_multiway_join_test {
    ($relation_type:ident, $join_algorithm:ty, $optimiser:ty) => {
        paste::paste! {
        $crate::define_multiway_join_test!(
            [<repeated_nonadjacent_ $relation_type:lower _ $join_algorithm:lower _ $optimiser:lower>],
            $relation_type,
            $join_algorithm,
            $optimiser,
            [
                vec![vec![1, 2, 1], vec![1, 2, 3], vec![2, 5, 2], vec![3, 7, 3]],
                vec![vec![2], vec![7]]
            ],
            vec![0, 1],
            vec![vec![0, 1, 0], vec![1]],
            vec![vec![1, 2], vec![3, 7]],
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
                $crate::define_diagonal_multiway_join_test!( $relation_type, $join_algorithm, $optimiser );
                $crate::define_repeated_nonadjacent_multiway_join_test!( $relation_type, $join_algorithm, $optimiser );
        )+
    };
}

/// The Config-axis counterpart of [`define_multiway_join_test_suite!`]
/// prescribed by `docs/specs/optimization-standard.md`.
///
/// Declares `type <Relation><Provider> = Configured<Relation, Provider>;`
/// inside a uniquely named module and runs the 14 standard join patterns
/// against it. `Provider` is a marker declared with
/// `kermit_ds::define_config_provider!`.
///
/// ```ignore
/// define_config_provider!(HalfFull, HashTrieConfig, HashTrieConfig { load_factor: LoadFactor::percent(50).unwrap() });
/// define_multiway_join_test_suite_with_config!(HashTrieSip, HashTriejoin, LexicographicOptimiser, HalfFull);
/// // → tests named e.g. `triangle_hashtriesiphalffull_hashtriejoin_lexicographicoptimiser`
/// ```
#[macro_export]
macro_rules! define_multiway_join_test_suite_with_config {
    (
        $(
            $relation_type:ident,
            $join_algorithm:ident,
            $optimiser:ident,
            $provider:ident
        ),+
    ) => {
        $(
            paste::paste! {
                // Each invocation gets its own module so the alias can be
                // declared once per (relation, algorithm, optimiser, provider)
                // without colliding with a sibling invocation's alias.
                mod [<with_config_ $relation_type:lower _ $join_algorithm:lower _ $optimiser:lower _ $provider:lower>] {
                    use super::*;

                    type [<$relation_type $provider>] =
                        kermit_ds::Configured<$relation_type, $provider>;

                    $crate::define_multiway_join_test_suite!(
                        [<$relation_type $provider>], $join_algorithm, $optimiser
                    );
                }
            }
        )+
    };
}
