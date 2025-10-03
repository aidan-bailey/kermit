use {
    arrow::{
        array::{Int64Array, RecordBatch},
        datatypes::{DataType, Field, Schema},
    },
    parquet::{arrow::ArrowWriter, file::properties::WriterProperties},
    std::{path::Path, sync::Arc},
};

pub fn write_relation_to_parquet(
    path: &Path, attributes: &[String], data: &[Vec<usize>],
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
