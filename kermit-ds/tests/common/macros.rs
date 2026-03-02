#[macro_export]
macro_rules! relation_construction_test {
    (
        $test_name:ident,
        $relation_type:ident,
        [ $( $input:expr ),* $(,)? ]
    ) => {
        #[test]
        fn $test_name() {
            use {kermit_ds::Relation, kermit_iters::TrieIterable};
            let tuples: Vec<Vec<usize>> = vec![$($input.to_vec()),*];
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

            $crate::relation_construction_test!(empty, $relation_type, []);

            $crate::relation_construction_test!(unary, $relation_type, [vec![1], vec![2], vec![3]]);

            $crate::relation_construction_test!(binary, $relation_type, [vec![1, 2], vec![3, 4]]);

            $crate::relation_construction_test!(ternary, $relation_type, [vec![1, 2, 3], vec![
                4, 5, 6
            ]]);
        }
    };
}

#[macro_export]
macro_rules! trie_test {
    (
        $test_name:ident,
        $relation_type:ident,
        [ $( $input:expr ),* $(,)? ],
        $code: expr
    ) => {
        #[test]
       fn $test_name() {
            use kermit_ds::Relation;
            use kermit_iters::{TrieIterable, TrieIterator};
            let inputs: Vec<Vec<usize>> = vec![$($input.to_vec()),*];
            let k = if inputs.is_empty() { 0 } else { inputs[0].len() };
            let relation = $relation_type::from_tuples(k.into(), inputs);
            $code(&mut relation.trie_iter());
        }
    };
}

#[macro_export]
macro_rules! trie_traversal_tests {
    ($relation_type:ident) => {
        mod trie_traversal {

            use super::*;

            $crate::trie_test!(empty, $relation_type, [], |iter: &mut dyn TrieIterator| {
                // All operations should indicate an empty/exhausted iterator
                assert!(iter.key().is_none());
                assert!(!iter.open());
                assert!(!iter.up());
                assert!(iter.next().is_none());
                assert!(iter.key().is_none());
                assert!(iter.at_end());
            });

            $crate::trie_test!(
                single,
                $relation_type,
                [vec![1]],
                |iter: &mut dyn TrieIterator| {
                    // Before open: no key, at end
                    assert!(iter.key().is_none());
                    assert!(iter.at_end());

                    // Open root → 1 (leaf)
                    assert!(iter.open());
                    assert_eq!(iter.key(), Some(1));
                    assert!(!iter.at_end());
                    assert!(!iter.open()); // leaf, no children

                    // Exhaust siblings → at end
                    assert!(iter.next().is_none());
                    assert!(iter.at_end());

                    // Navigate back up
                    assert!(iter.up());
                    assert!(!iter.up()); // already at root
                }
            );

            $crate::trie_test!(
                siblings,
                $relation_type,
                [vec![1], vec![2], vec![3]],
                |iter: &mut dyn TrieIterator| {
                    // Before open: no key, at end
                    assert!(iter.key().is_none());
                    assert!(iter.at_end());

                    // Open root → 1
                    assert!(iter.open());
                    assert_eq!(iter.key(), Some(1));
                    assert!(!iter.at_end());
                    assert!(!iter.open()); // leaf

                    // Walk siblings: 1 → 2 → 3
                    assert_eq!(iter.next(), Some(2));
                    assert_eq!(iter.key(), Some(2));
                    assert_eq!(iter.next(), Some(3));
                    assert_eq!(iter.key(), Some(3));

                    // Exhaust siblings → at end
                    assert!(iter.next().is_none());
                    assert!(iter.key().is_none());
                    assert!(iter.at_end());

                    // Navigate back up
                    assert!(iter.up());
                    assert!(!iter.up());
                }
            );

            $crate::trie_test!(
                shared,
                $relation_type,
                [vec![1, 2], vec![1, 3]],
                |iter: &mut dyn TrieIterator| {
                    // Descend: root → 1 → 2
                    assert!(iter.open());
                    assert_eq!(iter.key(), Some(1));
                    assert!(iter.open());
                    assert_eq!(iter.key(), Some(2));

                    // Sibling under shared prefix: 2 → 3
                    assert_eq!(iter.next(), Some(3));
                    assert!(iter.next().is_none());

                    // Navigate back up through both levels
                    assert!(iter.up());
                    assert!(iter.up());
                }
            );

            $crate::trie_test!(
                deep,
                $relation_type,
                [vec![1, 2, 3]],
                |iter: &mut dyn TrieIterator| {
                    // Descend: root → 1 → 2 → 3
                    assert!(iter.open());
                    assert_eq!(iter.key(), Some(1));
                    assert!(iter.open());
                    assert_eq!(iter.key(), Some(2));
                    assert!(iter.open());
                    assert_eq!(iter.key(), Some(3));
                    assert!(!iter.open()); // leaf

                    // Ascend: 3 → 2 → 1 → root
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
                $relation_type,
                [vec![1], vec![2], vec![3]],
                |iter: &mut dyn TrieIterator| {
                    // Before open: no key, at end
                    assert!(iter.key().is_none());
                    assert!(iter.at_end());

                    // Open root → 1 (leaf, so open fails)
                    assert!(iter.open());
                    assert!(!iter.open());
                    assert_eq!(iter.key(), Some(1));
                    assert!(!iter.at_end());

                    // Walk all leaves: 1 → 2 → 3, each is a leaf
                    assert!(!iter.open());
                    assert_eq!(iter.next(), Some(2));
                    assert_eq!(iter.key(), Some(2));
                    assert!(!iter.at_end());
                    assert!(!iter.open());
                    assert_eq!(iter.next(), Some(3));
                    assert_eq!(iter.key(), Some(3));
                    assert!(!iter.at_end());
                    assert!(!iter.open());

                    // Exhaust siblings → at end
                    assert!(iter.next().is_none());
                    assert!(iter.key().is_none());
                    assert!(iter.at_end());
                    assert!(!iter.open());

                    // Navigate back up to root
                    assert!(iter.up());
                    assert!(!iter.up());
                    assert!(iter.key().is_none());
                    assert!(iter.next().is_none());

                    // Re-open: should restart at first key
                    assert!(iter.open());
                    assert_eq!(iter.key(), Some(1));
                    assert!(!iter.at_end());
                    assert!(!iter.open());
                }
            );

            $crate::trie_test!(
                open,
                $relation_type,
                [vec![1, 2, 3]],
                |iter: &mut dyn TrieIterator| {
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
                $relation_type,
                [vec![1, 2, 3], vec![1, 2, 4], vec![1, 5, 6], vec![7, 8, 9]],
                |iter: &mut dyn TrieIterator| {
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
macro_rules! trie_seek_tests {
    ($relation_type:ident) => {
        mod trie_seek {

            use super::*;

            $crate::trie_test!(
                seek_exact,
                $relation_type,
                [vec![1], vec![3], vec![5], vec![7], vec![9]],
                |iter: &mut dyn TrieIterator| {
                    assert!(iter.open());
                    assert_eq!(iter.key(), Some(1));
                    assert!(iter.seek(5));
                    assert_eq!(iter.key(), Some(5));
                }
            );

            $crate::trie_test!(
                seek_upper_bound,
                $relation_type,
                [vec![1], vec![3], vec![5], vec![7], vec![9]],
                |iter: &mut dyn TrieIterator| {
                    assert!(iter.open());
                    assert_eq!(iter.key(), Some(1));
                    assert!(iter.seek(4));
                    assert_eq!(iter.key(), Some(5));
                }
            );

            $crate::trie_test!(
                seek_past_all,
                $relation_type,
                [vec![1], vec![3], vec![5]],
                |iter: &mut dyn TrieIterator| {
                    assert!(iter.open());
                    assert_eq!(iter.key(), Some(1));
                    assert!(!iter.seek(100));
                    assert!(iter.at_end());
                }
            );

            $crate::trie_test!(
                seek_to_current,
                $relation_type,
                [vec![1], vec![3], vec![5]],
                |iter: &mut dyn TrieIterator| {
                    assert!(iter.open());
                    assert_eq!(iter.key(), Some(1));
                    assert!(iter.seek(1));
                    assert_eq!(iter.key(), Some(1));
                }
            );

            $crate::trie_test!(
                seek_then_next,
                $relation_type,
                [vec![1], vec![3], vec![5], vec![7], vec![9]],
                |iter: &mut dyn TrieIterator| {
                    assert!(iter.open());
                    assert!(iter.seek(5));
                    assert_eq!(iter.key(), Some(5));
                    assert_eq!(iter.next(), Some(7));
                    assert_eq!(iter.next(), Some(9));
                    assert_eq!(iter.next(), None);
                    assert!(iter.at_end());
                }
            );

            $crate::trie_test!(
                seek_at_depth,
                $relation_type,
                [vec![1, 2], vec![1, 5], vec![1, 8]],
                |iter: &mut dyn TrieIterator| {
                    assert!(iter.open());
                    assert_eq!(iter.key(), Some(1));
                    assert!(iter.open());
                    assert_eq!(iter.key(), Some(2));
                    assert!(iter.seek(5));
                    assert_eq!(iter.key(), Some(5));
                    assert_eq!(iter.next(), Some(8));
                }
            );

            $crate::trie_test!(
                seek_upper_bound_at_depth,
                $relation_type,
                [vec![1, 2], vec![1, 5], vec![1, 8]],
                |iter: &mut dyn TrieIterator| {
                    assert!(iter.open());
                    assert!(iter.open());
                    assert_eq!(iter.key(), Some(2));
                    assert!(iter.seek(3));
                    assert_eq!(iter.key(), Some(5));
                }
            );

            $crate::trie_test!(
                seek_multiple,
                $relation_type,
                [vec![1], vec![3], vec![5], vec![7], vec![9]],
                |iter: &mut dyn TrieIterator| {
                    assert!(iter.open());
                    assert!(iter.seek(3));
                    assert_eq!(iter.key(), Some(3));
                    assert!(iter.seek(7));
                    assert_eq!(iter.key(), Some(7));
                    assert!(iter.seek(9));
                    assert_eq!(iter.key(), Some(9));
                    assert!(!iter.seek(10));
                    assert!(iter.at_end());
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

                    $crate::trie_seek_tests!($relation_type);

                }
            }
        )+
    };
}

#[macro_export]
macro_rules! parquet_test_suite {
    ($relation_type:ident) => {
        paste::paste! {
            #[cfg(test)]
            mod [<parquet_ $relation_type:lower>] {

                use {
                    kermit_ds::{$relation_type, Relation, RelationFileExt},
                    kermit_iters::TrieIterable,
                };

                fn write_parquet(
                    path: &std::path::PathBuf, attributes: &[String],
                    data: &[Vec<usize>],
                ) {
                    use {
                        arrow::{
                            array::{Array, Int64Array, RecordBatch},
                            datatypes::{DataType, Field, Schema},
                        },
                        parquet::{
                            arrow::ArrowWriter,
                            file::properties::WriterProperties,
                        },
                        std::sync::Arc,
                    };

                    let fields: Vec<Field> = attributes
                        .iter()
                        .map(|attr| Field::new(attr, DataType::Int64, true))
                        .collect();

                    let schema = Arc::new(Schema::new(fields));

                    let mut columns = Vec::new();
                    for attr_idx in 0..attributes.len() {
                        let column_data: Vec<i64> = data
                            .iter()
                            .map(|row| {
                                row.get(attr_idx).copied().unwrap_or(0) as i64
                            })
                            .collect();
                        columns.push(
                            Arc::new(Int64Array::from(column_data)) as Arc<dyn Array>
                        );
                    }

                    let batch =
                        RecordBatch::try_new(schema.clone(), columns).unwrap();

                    let props = WriterProperties::builder().build();

                    let file = std::fs::File::create(path).unwrap();
                    let mut writer =
                        ArrowWriter::try_new(file, schema, Some(props)).unwrap();
                    writer.write(&batch).unwrap();
                    writer.close().unwrap();
                }

                #[test]
                fn empty() {
                    let temp_dir = std::env::temp_dir();
                    let path = temp_dir.join(concat!(
                        "parquet_empty_",
                        stringify!($relation_type),
                        ".parquet"
                    ));

                    let attributes = vec!["a".to_string(), "b".to_string()];
                    let data: Vec<Vec<usize>> = vec![];

                    write_parquet(&path, &attributes, &data);

                    let relation =
                        $relation_type::from_parquet(&path).unwrap();
                    let result: Vec<Vec<usize>> =
                        relation.trie_iter().into_iter().collect();
                    assert_eq!(result, data);

                    assert_eq!(
                        relation.header().name(),
                        concat!(
                            "parquet_empty_",
                            stringify!($relation_type)
                        )
                    );
                    assert_eq!(relation.header().attrs(), &attributes);
                    assert_eq!(relation.header().arity(), 2);

                    std::fs::remove_file(path).ok();
                }

                #[test]
                fn single_tuple() {
                    let temp_dir = std::env::temp_dir();
                    let path = temp_dir.join(concat!(
                        "parquet_single_",
                        stringify!($relation_type),
                        ".parquet"
                    ));

                    let attributes = vec![
                        "x".to_string(),
                        "y".to_string(),
                        "z".to_string(),
                    ];
                    let data = vec![vec![1, 2, 3]];

                    write_parquet(&path, &attributes, &data);

                    let relation =
                        $relation_type::from_parquet(&path).unwrap();
                    let result: Vec<Vec<usize>> =
                        relation.trie_iter().into_iter().collect();
                    assert_eq!(result, data);

                    assert_eq!(relation.header().attrs(), &attributes);
                    assert_eq!(relation.header().arity(), 3);

                    std::fs::remove_file(path).ok();
                }

                #[test]
                fn multiple_tuples() {
                    let temp_dir = std::env::temp_dir();
                    let path = temp_dir.join(concat!(
                        "parquet_multi_",
                        stringify!($relation_type),
                        ".parquet"
                    ));

                    let attributes = vec!["a".to_string(), "b".to_string()];
                    let data = vec![vec![1, 2], vec![3, 4], vec![5, 6]];

                    write_parquet(&path, &attributes, &data);

                    let relation =
                        $relation_type::from_parquet(&path).unwrap();
                    let result: Vec<Vec<usize>> =
                        relation.trie_iter().into_iter().collect();
                    assert_eq!(result, data);

                    assert_eq!(relation.header().attrs(), &attributes);
                    assert_eq!(relation.header().arity(), 2);

                    std::fs::remove_file(path).ok();
                }

                #[test]
                fn large_dataset() {
                    let temp_dir = std::env::temp_dir();
                    let path = temp_dir.join(concat!(
                        "parquet_large_",
                        stringify!($relation_type),
                        ".parquet"
                    ));

                    let attributes = vec![
                        "col1".to_string(),
                        "col2".to_string(),
                        "col3".to_string(),
                    ];
                    let data: Vec<Vec<usize>> =
                        (0..100).map(|i| vec![i, i * 2, i * 3]).collect();

                    write_parquet(&path, &attributes, &data);

                    let relation =
                        $relation_type::from_parquet(&path).unwrap();
                    let result: Vec<Vec<usize>> =
                        relation.trie_iter().into_iter().collect();
                    assert_eq!(result, data);

                    assert_eq!(relation.header().attrs(), &attributes);
                    assert_eq!(relation.header().arity(), 3);

                    std::fs::remove_file(path).ok();
                }

                #[test]
                fn unary_relation() {
                    let temp_dir = std::env::temp_dir();
                    let path = temp_dir.join(concat!(
                        "parquet_unary_",
                        stringify!($relation_type),
                        ".parquet"
                    ));

                    let attributes = vec!["x".to_string()];
                    let data =
                        vec![vec![1], vec![2], vec![3], vec![4], vec![5]];

                    write_parquet(&path, &attributes, &data);

                    let relation =
                        $relation_type::from_parquet(&path).unwrap();
                    let result: Vec<Vec<usize>> =
                        relation.trie_iter().into_iter().collect();
                    assert_eq!(result, data);

                    assert_eq!(relation.header().attrs(), &attributes);
                    assert_eq!(relation.header().arity(), 1);

                    std::fs::remove_file(path).ok();
                }

                #[test]
                fn column_names() {
                    let temp_dir = std::env::temp_dir();
                    let path = temp_dir.join(concat!(
                        "parquet_cols_",
                        stringify!($relation_type),
                        ".parquet"
                    ));

                    let attributes = vec![
                        "employee_id".to_string(),
                        "department".to_string(),
                        "salary".to_string(),
                    ];
                    let data =
                        vec![vec![101, 5, 50000], vec![102, 3, 60000]];

                    write_parquet(&path, &attributes, &data);

                    let relation =
                        $relation_type::from_parquet(&path).unwrap();

                    assert_eq!(relation.header().attrs(), &attributes);
                    assert_eq!(
                        relation.header().attrs()[0],
                        "employee_id"
                    );
                    assert_eq!(
                        relation.header().attrs()[1],
                        "department"
                    );
                    assert_eq!(relation.header().attrs()[2], "salary");

                    let result: Vec<Vec<usize>> =
                        relation.trie_iter().into_iter().collect();
                    assert_eq!(result, data);

                    std::fs::remove_file(path).ok();
                }

                #[test]
                fn oxford_format() {
                    let temp_dir = std::env::temp_dir();
                    let path = temp_dir.join(concat!(
                        "parquet_oxford_",
                        stringify!($relation_type),
                        ".parquet"
                    ));

                    let attributes = vec![
                        "attr1".to_string(),
                        "attr2".to_string(),
                        "attr3".to_string(),
                    ];
                    let data = vec![
                        vec![10, 20, 30],
                        vec![15, 25, 35],
                        vec![20, 30, 40],
                        vec![25, 35, 45],
                    ];

                    write_parquet(&path, &attributes, &data);

                    let relation =
                        $relation_type::from_parquet(&path).unwrap();
                    let result: Vec<Vec<usize>> =
                        relation.trie_iter().into_iter().collect();
                    assert_eq!(result.len(), data.len());
                    assert_eq!(result, data);

                    assert_eq!(relation.header().attrs(), &attributes);
                    assert_eq!(relation.header().arity(), 3);

                    std::fs::remove_file(path).ok();
                }
            }
        }
    };
}
