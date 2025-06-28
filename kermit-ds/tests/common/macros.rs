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
            use {kermit_ds::relation::Relation, kermit_iters::trie::TrieIterable};
            let tuples: Vec<Vec<$key_type>> = vec![$($input.to_vec()),*];
            let relation = $relation_type::from_tuples(tuples.clone());
            let res = relation.trie_iter().into_iter().collect::<Vec<_>>();
            assert_eq!(res, tuples);
        }
    };
}

#[macro_export]
macro_rules! define_initialisation_tests {
    ($relation_type:ident) => {
        paste::paste! {

            $crate::define_initialisation_test!(
                [<$relation_type:lower _ init  _ empty>],
                u8,
                $relation_type,
                [],
            );

            $crate::define_initialisation_test!(
                [<$relation_type:lower _ init  _ unary>],
                u8,
                $relation_type,
                [vec![1], vec![2], vec![3]],
            );

            $crate::define_initialisation_test!(
                [<$relation_type:lower _ init  _ binary>],
                u8,
                $relation_type,
                [vec![1, 2], vec![3, 4]],
            );

            $crate::define_initialisation_test!(
                [<$relation_type:lower _ init _ ternary>],
                u8,
                $relation_type,
                [vec![1, 2, 3], vec![4, 5, 6]],
            );

        }
    };
}

#[macro_export]
macro_rules! generate_iter_test {
    (
        $test_name:ident,
        $key_type:ty,
        $relation_type:ident,
        [ $( $input:expr ),* $(,)? ],
        $code: expr
    ) => {
        #[test]
       fn $test_name() {
            use kermit_ds::relation::Relation;
            use kermit_iters::trie::TrieIterable;
            use kermit_iters::trie::TrieIterator;
            let inputs: Vec<Vec<$key_type>> = vec![$($input.to_vec()),*];
            let relation = $relation_type::<$key_type>::from_tuples(inputs);
            $code(&mut relation.trie_iter());
        }
    };
}

#[macro_export]
macro_rules! define_trie_traversal_tests {
    ($relation_type:ident) => {
        paste::paste! {

            $crate::generate_iter_test!(
                [<$relation_type:lower _ trieiter _ empty>],
                usize,
                $relation_type,
                [],
                |iter: &mut dyn TrieIterator<KT = usize>| {
                    assert!(iter.key().is_none());
                    assert!(!iter.open());
                    assert!(!iter.up());
                    assert!(iter.next().is_none());
                    assert!(iter.key().is_none());
                    assert!(iter.at_end());
                }
            );

            $crate::generate_iter_test!(
                [<$relation_type:lower _ trieiter _ single>],
                usize,
                $relation_type,
                [vec![1]],
                |iter: &mut dyn TrieIterator<KT = usize>| {
                    assert!(iter.key().is_none());
                    assert!(iter.at_end());
                    assert!(iter.open());
                    assert_eq!(iter.key(), Some(1));
                    assert!(!iter.at_end());
                    assert!(!iter.open());
                    assert!(iter.next().is_none());
                    assert!(iter.at_end());
                    assert!(iter.up());
                    assert!(!iter.up());
                }
            );

            $crate::generate_iter_test!(
                [<$relation_type:lower _ trieiter _ siblings>],
                usize,
                $relation_type,
                [vec![1], vec![2], vec![3]],
                |iter: &mut dyn TrieIterator<KT = usize>| {
                    assert!(iter.key().is_none());
                    assert!(iter.at_end());
                    assert!(iter.open());
                    assert_eq!(iter.key(), Some(1));
                    assert!(!iter.at_end());
                    assert!(!iter.open());
                    assert_eq!(iter.next(), Some(2));
                    assert_eq!(iter.key(), Some(2));
                    assert_eq!(iter.next(), Some(3));
                    assert_eq!(iter.key(), Some(3));
                    assert!(iter.next().is_none());
                    assert!(iter.key().is_none());
                    assert!(iter.at_end());
                    assert!(iter.up());
                    assert!(!iter.up());
                }
            );

            $crate::generate_iter_test!(
                [<$relation_type:lower _ trieiter _ shared>],
                usize,
                $relation_type,
                [vec![1, 2], vec![1, 3]],
                |iter: &mut dyn TrieIterator<KT = usize>| {
                    assert!(iter.open());
                    assert_eq!(iter.key(), Some(1));
                    assert!(iter.open());
                    assert_eq!(iter.key(), Some(2));
                    assert_eq!(iter.next(), Some(3));
                    assert!(iter.next().is_none());
                    assert!(iter.up());
                    assert!(iter.up());
                }
            );

            $crate::generate_iter_test!(
                [<$relation_type:lower _ trieiter _ deep>],
                usize,
                $relation_type,
                [vec![1, 2, 3]],
                |iter: &mut dyn TrieIterator<KT = usize>| {
                    assert!(iter.open());
                    assert_eq!(iter.key(), Some(1));
                    assert!(iter.open());
                    assert_eq!(iter.key(), Some(2));
                    assert!(iter.open());
                    assert_eq!(iter.key(), Some(3));
                    assert!(!iter.open());
                    assert!(iter.up());
                    assert_eq!(iter.key(), Some(2));
                    assert!(iter.up());
                    assert_eq!(iter.key(), Some(1));
                    assert!(iter.up());
                    assert!(iter.at_end());
                }
            );

            $crate::generate_iter_test!(
                [<$relation_type:lower _ trieiter _ linear>],
                usize,
                $relation_type,
                [vec![1], vec![2], vec![3]],
                |iter: &mut dyn TrieIterator<KT = usize>| {
                    assert!(iter.key().is_none());
                    assert!(iter.at_end());
                    assert!(iter.open());
                    assert!(!iter.open());
                    assert_eq!(iter.key(), Some(1));
                    assert!(!iter.at_end());
                    assert!(!iter.open());
                    assert_eq!(iter.next(), Some(2));
                    assert_eq!(iter.key(), Some(2));
                    assert!(!iter.at_end());
                    assert!(!iter.open());
                    assert_eq!(iter.next(), Some(3));
                    assert_eq!(iter.key(), Some(3));
                    assert!(!iter.at_end());
                    assert!(!iter.open());
                    assert!(iter.next().is_none());
                    assert!(iter.key().is_none());
                    assert!(iter.at_end());
                    assert!(!iter.open());
                    assert!(iter.up());
                    assert!(!iter.up());
                    assert!(iter.key().is_none());
                    assert!(iter.next().is_none());
                    assert!(iter.open());
                    assert_eq!(iter.key(), Some(1));
                    assert!(!iter.at_end());
                    assert!(!iter.open());
                }
            );

            $crate::generate_iter_test!(
                [<$relation_type:lower _ trieiter _ open>],
                usize,
                $relation_type,
                [vec![1, 2, 3]],
                |iter: &mut dyn TrieIterator<KT = usize>| {
                    assert!(iter.key().is_none());
                    assert!(iter.at_end());
                    assert!(iter.open());
                    assert_eq!(iter.key(), Some(1));
                    assert!(iter.open());
                    assert_eq!(iter.key(), Some(2));
                    assert!(iter.open());
                    assert_eq!(iter.key(), Some(3));
                    assert!(!iter.open());
                }
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
                $crate::define_trie_traversal_tests!($relation_type);
        )+
    };
}
