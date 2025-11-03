use {
    arrow::{
        array::{Int64Array, RecordBatch},
        datatypes::{DataType, Field, Schema},
    },
    kermit_ds::{ColumnTrie, Relation, RelationFileExt},
    kermit_iters::TrieIterable,
    parquet::{arrow::ArrowWriter, file::properties::WriterProperties},
    std::{path::PathBuf, sync::Arc},
};

/// Helper function to write test data to a parquet file
fn write_test_parquet(
    path: &PathBuf, attributes: &[String], data: &[Vec<usize>],
) -> Result<(), Box<dyn std::error::Error>> {
    let fields: Vec<Field> = attributes
        .iter()
        .map(|attr| Field::new(attr, DataType::Int64, true))
        .collect();

    let schema = Arc::new(Schema::new(fields));

    let mut columns = Vec::new();
    for attr_idx in 0..attributes.len() {
        let column_data: Vec<i64> = data
            .iter()
            .map(|row| row.get(attr_idx).copied().unwrap_or(0) as i64)
            .collect();
        columns.push(Arc::new(Int64Array::from(column_data)) as Arc<dyn arrow::array::Array>);
    }

    let batch = RecordBatch::try_new(schema.clone(), columns)?;

    let props = WriterProperties::builder().build();

    let file = std::fs::File::create(path)?;
    let mut writer = ArrowWriter::try_new(file, schema, Some(props))?;
    writer.write(&batch)?;
    writer.close()?;

    Ok(())
}

mod from_parquet_tests {
    use super::*;

    #[test]
    fn test_from_parquet_with_header_empty() {
        let temp_dir = std::env::temp_dir();
        let parquet_path = temp_dir.join("test_from_empty.parquet");

        // Write empty parquet file
        let attributes = vec!["a".to_string(), "b".to_string()];
        let data: Vec<Vec<usize>> = vec![];

        write_test_parquet(&parquet_path, &attributes, &data).unwrap();

        // Read using from_parquet
        let relation = ColumnTrie::from_parquet(&parquet_path).unwrap();

        let result: Vec<Vec<usize>> = relation.trie_iter().into_iter().collect();
        assert_eq!(result, data);

        // Verify header was extracted correctly
        assert_eq!(relation.header().name(), "test_from_empty");
        assert_eq!(relation.header().attrs(), &attributes);
        assert_eq!(relation.header().arity(), 2);

        // Cleanup
        std::fs::remove_file(parquet_path).ok();
    }

    #[test]
    fn test_from_parquet_with_header_single_tuple() {
        let temp_dir = std::env::temp_dir();
        let parquet_path = temp_dir.join("test_from_single.parquet");

        // Write parquet file with single tuple
        let attributes = vec!["x".to_string(), "y".to_string(), "z".to_string()];
        let data = vec![vec![1, 2, 3]];

        write_test_parquet(&parquet_path, &attributes, &data).unwrap();

        // Read using from_parquet
        let relation = ColumnTrie::from_parquet(&parquet_path).unwrap();

        let result: Vec<Vec<usize>> = relation.trie_iter().into_iter().collect();
        assert_eq!(result, data);

        // Verify header was extracted correctly
        assert_eq!(relation.header().name(), "test_from_single");
        assert_eq!(relation.header().attrs(), &attributes);
        assert_eq!(relation.header().arity(), 3);

        // Cleanup
        std::fs::remove_file(parquet_path).ok();
    }

    #[test]
    fn test_from_parquet_with_header_multiple_tuples() {
        let temp_dir = std::env::temp_dir();
        let parquet_path = temp_dir.join("test_from_multiple.parquet");

        // Write parquet file with multiple tuples
        let attributes = vec!["a".to_string(), "b".to_string()];
        let data = vec![vec![1, 2], vec![3, 4], vec![5, 6]];

        write_test_parquet(&parquet_path, &attributes, &data).unwrap();

        // Read using from_parquet
        let relation = ColumnTrie::from_parquet(&parquet_path).unwrap();

        let result: Vec<Vec<usize>> = relation.trie_iter().into_iter().collect();
        assert_eq!(result, data);

        // Verify header was extracted correctly
        assert_eq!(relation.header().name(), "test_from_multiple");
        assert_eq!(relation.header().attrs(), &attributes);
        assert_eq!(relation.header().arity(), 2);

        // Cleanup
        std::fs::remove_file(parquet_path).ok();
    }

    #[test]
    fn test_from_parquet_with_header_large_dataset() {
        let temp_dir = std::env::temp_dir();
        let parquet_path = temp_dir.join("test_from_large.parquet");

        // Write parquet file with larger dataset
        let attributes = vec!["col1".to_string(), "col2".to_string(), "col3".to_string()];
        let data: Vec<Vec<usize>> = (0..100).map(|i| vec![i, i * 2, i * 3]).collect();

        write_test_parquet(&parquet_path, &attributes, &data).unwrap();

        // Read using from_parquet
        let relation = ColumnTrie::from_parquet(&parquet_path).unwrap();

        let result: Vec<Vec<usize>> = relation.trie_iter().into_iter().collect();
        assert_eq!(result, data);

        // Verify header was extracted correctly
        assert_eq!(relation.header().name(), "test_from_large");
        assert_eq!(relation.header().attrs(), &attributes);
        assert_eq!(relation.header().arity(), 3);

        // Cleanup
        std::fs::remove_file(parquet_path).ok();
    }

    #[test]
    fn test_from_parquet_with_header_oxford_format() {
        let temp_dir = std::env::temp_dir();
        let parquet_path = temp_dir.join("test_from_oxford.parquet");

        // Simulate Oxford dataset format
        let attributes = vec![
            "attr1".to_string(),
            "attr2".to_string(),
            "attr3".to_string(),
        ];
        let data = vec![vec![10, 20, 30], vec![15, 25, 35], vec![20, 30, 40], vec![
            25, 35, 45,
        ]];

        write_test_parquet(&parquet_path, &attributes, &data).unwrap();

        // Read using from_parquet
        let relation = ColumnTrie::from_parquet(&parquet_path).unwrap();

        let result: Vec<Vec<usize>> = relation.trie_iter().into_iter().collect();
        assert_eq!(result.len(), data.len());
        assert_eq!(result, data);

        // Verify header was extracted correctly
        assert_eq!(relation.header().name(), "test_from_oxford");
        assert_eq!(relation.header().attrs(), &attributes);
        assert_eq!(relation.header().arity(), 3);

        // Cleanup
        std::fs::remove_file(parquet_path).ok();
    }

    #[test]
    fn test_from_parquet_with_header_unary_relation() {
        let temp_dir = std::env::temp_dir();
        let parquet_path = temp_dir.join("test_from_unary.parquet");

        // Write unary relation (single column)
        let attributes = vec!["x".to_string()];
        let data = vec![vec![1], vec![2], vec![3], vec![4], vec![5]];

        write_test_parquet(&parquet_path, &attributes, &data).unwrap();

        // Read using from_parquet
        let relation = ColumnTrie::from_parquet(&parquet_path).unwrap();

        let result: Vec<Vec<usize>> = relation.trie_iter().into_iter().collect();
        assert_eq!(result, data);

        // Verify header was extracted correctly
        assert_eq!(relation.header().name(), "test_from_unary");
        assert_eq!(relation.header().attrs(), &attributes);
        assert_eq!(relation.header().arity(), 1);

        // Cleanup
        std::fs::remove_file(parquet_path).ok();
    }

    #[test]
    fn test_from_parquet_with_header_extracts_column_names() {
        let temp_dir = std::env::temp_dir();
        let parquet_path = temp_dir.join("test_from_column_names.parquet");

        // Write parquet file with specific column names
        let attributes = vec![
            "employee_id".to_string(),
            "department".to_string(),
            "salary".to_string(),
        ];
        let data = vec![vec![101, 5, 50000], vec![102, 3, 60000]];

        write_test_parquet(&parquet_path, &attributes, &data).unwrap();

        // Read using from_parquet
        let relation = ColumnTrie::from_parquet(&parquet_path).unwrap();

        // Verify the relation name and column names were extracted correctly
        assert_eq!(relation.header().name(), "test_from_column_names");
        assert_eq!(relation.header().attrs(), &attributes);
        assert_eq!(relation.header().attrs()[0], "employee_id");
        assert_eq!(relation.header().attrs()[1], "department");
        assert_eq!(relation.header().attrs()[2], "salary");

        // Verify data
        let result: Vec<Vec<usize>> = relation.trie_iter().into_iter().collect();
        assert_eq!(result, data);

        // Cleanup
        std::fs::remove_file(parquet_path).ok();
    }
}
