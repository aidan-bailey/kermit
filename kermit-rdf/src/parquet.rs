//! Parquet writers for the dictionary and per-predicate relation tables.

use {
    crate::{dict::Dictionary, error::RdfError, partition::PartitionedRelation},
    arrow::{
        array::{ArrayRef, Int64Array, StringArray},
        datatypes::{DataType, Field, Schema},
        record_batch::RecordBatch,
    },
    parquet::{arrow::ArrowWriter, file::properties::WriterProperties},
    std::{path::Path, sync::Arc},
};

/// Writes the dictionary as a 2-column Parquet file: `id: i64`, `value:
/// string`. `value` is the canonical string form (`<iri>`, `_:bN`, `"lit"`).
pub fn write_dict(dict: &Dictionary, out_path: &Path) -> Result<(), RdfError> {
    let ids: Vec<i64> = (0..dict.len() as i64).collect();
    let values: Vec<String> = dict.iter().map(|(_, v)| v.to_canonical()).collect();
    let schema = Arc::new(Schema::new(vec![
        Field::new("id", DataType::Int64, false),
        Field::new("value", DataType::Utf8, false),
    ]));
    let id_arr = Arc::new(Int64Array::from(ids)) as ArrayRef;
    let val_arr = Arc::new(StringArray::from(values)) as ArrayRef;
    let batch = RecordBatch::try_new(schema.clone(), vec![id_arr, val_arr])?;
    let file = std::fs::File::create(out_path)?;
    let mut writer = ArrowWriter::try_new(file, schema, Some(WriterProperties::default()))?;
    writer.write(&batch)?;
    writer.close()?;
    Ok(())
}

/// Writes one predicate's tuples as a 2-column Parquet file: `s: i64`, `o:
/// i64`.
pub fn write_relation(rel: &PartitionedRelation, out_path: &Path) -> Result<(), RdfError> {
    let ss: Vec<i64> = rel.tuples.iter().map(|(s, _)| *s as i64).collect();
    let oo: Vec<i64> = rel.tuples.iter().map(|(_, o)| *o as i64).collect();
    let schema = Arc::new(Schema::new(vec![
        Field::new("s", DataType::Int64, false),
        Field::new("o", DataType::Int64, false),
    ]));
    let s_arr = Arc::new(Int64Array::from(ss)) as ArrayRef;
    let o_arr = Arc::new(Int64Array::from(oo)) as ArrayRef;
    let batch = RecordBatch::try_new(schema.clone(), vec![s_arr, o_arr])?;
    let file = std::fs::File::create(out_path)?;
    let mut writer = ArrowWriter::try_new(file, schema, Some(WriterProperties::default()))?;
    writer.write(&batch)?;
    writer.close()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::value::RdfValue,
        parquet::file::reader::{FileReader, SerializedFileReader},
    };

    #[test]
    fn dict_roundtrip() {
        let mut d = Dictionary::new();
        d.intern(RdfValue::Iri("http://x/a".into()));
        d.intern(RdfValue::Literal("\"hello\"".into()));

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("dict.parquet");
        write_dict(&d, &path).unwrap();

        let f = std::fs::File::open(&path).unwrap();
        let reader = SerializedFileReader::new(f).unwrap();
        let meta = reader.metadata();
        assert_eq!(meta.file_metadata().num_rows(), 2);
    }

    #[test]
    fn relation_roundtrip() {
        let rel = PartitionedRelation {
            name: "follows".into(),
            tuples: vec![(0, 1), (1, 2), (2, 0)],
        };
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("follows.parquet");
        write_relation(&rel, &path).unwrap();

        let f = std::fs::File::open(&path).unwrap();
        let reader = SerializedFileReader::new(f).unwrap();
        assert_eq!(reader.metadata().file_metadata().num_rows(), 3);
    }

    #[test]
    fn empty_dict_writes_zero_row_file() {
        let d = Dictionary::new();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("dict.parquet");
        write_dict(&d, &path).unwrap();
        assert!(path.exists());
    }
}
