#[macro_export]
macro_rules! relation_construction_test {
    (
        $test_name:ident,
        $key_type:ty,
        $relation_type:ident,
        [ $( $input:expr ),* $(,)? ]
    ) => {
        #[test]
        fn $test_name() {
            use {kermit_ds::relation::Relation, kermit_iters::trie::TrieIterable};
            let tuples: Vec<Vec<$key_type>> = vec![$($input.to_vec()),*];
            let k = if tuples.is_empty() { 0 } else { tuples[0].len() };
            let relation = $relation_type::from_tuples(k.into(), tuples.clone());
            let res = relation.trie_iter().into_iter().collect::<Vec<_>>();
            assert_eq!(res, tuples);
        }
    };
}

#[macro_export]
macro_rules! relation_construction_tests {
    ($relation_type:ident) => {
        mod construction {

            use super::*;

            $crate::relation_construction_test!(empty, u8, $relation_type, []);

            $crate::relation_construction_test!(unary, u8, $relation_type, [
                vec![1],
                vec![2],
                vec![3]
            ]);

            $crate::relation_construction_test!(binary, u8, $relation_type, [vec![1, 2], vec![
                3, 4
            ]]);

            $crate::relation_construction_test!(ternary, u8, $relation_type, [
                vec![1, 2, 3],
                vec![4, 5, 6]
            ]);
        }
    };
}

#[macro_export]
macro_rules! trie_test {
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
            let k = if inputs.is_empty() { 0 } else { inputs[0].len() };
            let relation = $relation_type::<$key_type>::from_tuples(k.into(), inputs);
            $code(&mut relation.trie_iter());
        }
    };
}

#[macro_export]
macro_rules! trie_traversal_tests {
    ($relation_type:ident) => {
        mod trie_traversal {

            use super::*;

            $crate::trie_test!(
                empty,
                u8,
                $relation_type,
                [],
                |iter: &mut dyn TrieIterator<KT = u8>| {
                    assert!(iter.key().is_none());
                    assert!(!iter.open());
                    assert!(!iter.up());
                    assert!(iter.next().is_none());
                    assert!(iter.key().is_none());
                    assert!(iter.at_end());
                }
            );

            $crate::trie_test!(
                single,
                u8,
                $relation_type,
                [vec![1]],
                |iter: &mut dyn TrieIterator<KT = u8>| {
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

            $crate::trie_test!(
                siblings,
                u8,
                $relation_type,
                [vec![1], vec![2], vec![3]],
                |iter: &mut dyn TrieIterator<KT = u8>| {
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

            $crate::trie_test!(
                shared,
                u8,
                $relation_type,
                [vec![1, 2], vec![1, 3]],
                |iter: &mut dyn TrieIterator<KT = u8>| {
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

            $crate::trie_test!(
                deep,
                u8,
                $relation_type,
                [vec![1, 2, 3]],
                |iter: &mut dyn TrieIterator<KT = u8>| {
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

            $crate::trie_test!(
                linear,
                u8,
                $relation_type,
                [vec![1], vec![2], vec![3]],
                |iter: &mut dyn TrieIterator<KT = u8>| {
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

            $crate::trie_test!(
                open,
                u8,
                $relation_type,
                [vec![1, 2, 3]],
                |iter: &mut dyn TrieIterator<KT = u8>| {
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

            $crate::trie_test!(
                hard,
                u8,
                $relation_type,
                [vec![1, 2, 3], vec![1, 2, 4], vec![1, 5, 6], vec![7, 8, 9]],
                |iter: &mut dyn TrieIterator<KT = u8>| {
                    // Begin at root
                    assert!(iter.key().is_none());
                    assert!(iter.at_end());

                    // Open root → 1
                    assert!(iter.open());
                    assert_eq!(iter.key(), Some(1));
                    assert!(!iter.at_end());

                    // Open 1 → 2
                    assert!(iter.open());
                    assert_eq!(iter.key(), Some(2));
                    assert!(!iter.at_end());

                    // Open 2 → 3
                    assert!(iter.open());
                    assert_eq!(iter.key(), Some(3));
                    assert!(!iter.open()); // 3 is a leaf

                    // next() → 4 (sibling of 3 under [1,2])
                    assert_eq!(iter.next(), Some(4));
                    assert_eq!(iter.key(), Some(4));
                    assert!(!iter.open()); // 4 is a leaf

                    // up() → [1,2]
                    assert!(iter.up());
                    assert_eq!(iter.key(), Some(2));

                    // up() → [1]
                    assert!(iter.up());
                    assert_eq!(iter.key(), Some(1));

                    // next() → 5 (sibling of 2 under [1])
                    assert!(iter.open());
                    assert_eq!(iter.next(), Some(5));
                    assert_eq!(iter.key(), Some(5));

                    // open() → 6
                    assert!(iter.open());
                    assert_eq!(iter.key(), Some(6));
                    assert!(!iter.open()); // 6 is a leaf

                    // up() → 5
                    assert!(iter.up());
                    assert_eq!(iter.key(), Some(5));

                    // up() → 1
                    assert!(iter.up());
                    assert_eq!(iter.key(), Some(1));

                    // up() → root
                    assert!(iter.up());
                    assert_eq!(iter.key(), None);
                    assert!(iter.at_end());

                    // open() → 7
                    assert!(iter.open());
                    assert_eq!(iter.next(), Some(7));

                    // open() → 8
                    assert!(iter.open());
                    assert_eq!(iter.key(), Some(8));

                    // open() → 9
                    assert!(iter.open());
                    assert_eq!(iter.key(), Some(9));
                    assert!(!iter.open()); // 9 is a leaf

                    // up() → 8
                    assert!(iter.up());
                    assert_eq!(iter.key(), Some(8));

                    // up() → 7
                    assert!(iter.up());
                    assert_eq!(iter.key(), Some(7));

                    // up() → root
                    assert!(iter.up());
                    assert_eq!(iter.key(), None);

                    // Now should be at end
                    assert!(iter.at_end());
                    assert!(iter.next().is_none());
                }
            );
        }
    };
}

#[macro_export]
macro_rules! relation_trie_test_suite {
    (
        $(
            $relation_type:ident
        ),+
    ) => {
        $(
            paste::paste! {
                #[cfg(test)]
                mod [<$relation_type:lower>] {

                    use super::*;

                    $crate::relation_construction_tests!(
                        $relation_type
                    );

                    $crate::trie_traversal_tests!($relation_type);

                }
            }
        )+
    };
}
