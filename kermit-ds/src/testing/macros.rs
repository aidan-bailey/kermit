#[macro_export]
macro_rules! define_initialisation_test {
    (
        $test_name:ident,
        $key_type:ty,
        $relation_type:ident,
        [ $( $input:expr ),* $(,)? ],
    ) => {
        #[test]
        fn $test_name() {
            let inputs: Vec<Vec<$key_type>> = vec![$($input.to_vec()),*];

            $crate::testing::utils::test_trie_relation_iteration::<$relation_type<$key_type>>(
                inputs,
            );
        }
    };
}

#[macro_export]
macro_rules! define_initialisation_tests {
    ($relation_type:ident) => {
        paste::paste! {

            $crate::define_initialisation_test!(
                [<empty_ $relation_type:lower _ key_type:lower>],
                u8,
                $relation_type,
                [],
            );

            $crate::define_initialisation_test!(
                [<unary_ $relation_type:lower _ key_type:lower>],
                u8,
                $relation_type,
                [vec![1], vec![2], vec![3]],
            );

        }
    };
}

#[macro_export]
macro_rules! define_trie_relation_test_suite {
    (
        $(
            $relation_type:ident
        ),+
    ) => {
        $(
                $crate::define_initialisation_tests!(
                    $relation_type
                );
        )+
    };
}
