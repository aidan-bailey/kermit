//! This module defines the `Relation` trait and file reading extensions.
use {
    arrow::array::AsArray,
    kermit_iters::JoinIterable,
    parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder,
    std::{fmt, fs::File, path::Path},
};

/// Error type for relation file operations (CSV and Parquet).
#[derive(Debug)]
pub enum RelationError {
    /// A CSV library error.
    Csv(csv::Error),
    /// A filesystem I/O error.
    Io(std::io::Error),
    /// A Parquet library error.
    Parquet(parquet::errors::ParquetError),
    /// An Arrow conversion error.
    Arrow(arrow::error::ArrowError),
    /// A data value that could not be converted (e.g. non-integer in a CSV).
    InvalidData(String),
}

impl fmt::Display for RelationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            | RelationError::Csv(e) => write!(f, "CSV error: {e}"),
            | RelationError::Io(e) => write!(f, "I/O error: {e}"),
            | RelationError::Parquet(e) => write!(f, "Parquet error: {e}"),
            | RelationError::Arrow(e) => write!(f, "Arrow error: {e}"),
            | RelationError::InvalidData(msg) => write!(f, "Invalid data: {msg}"),
        }
    }
}

impl std::error::Error for RelationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            | RelationError::Csv(e) => Some(e),
            | RelationError::Io(e) => Some(e),
            | RelationError::Parquet(e) => Some(e),
            | RelationError::Arrow(e) => Some(e),
            | RelationError::InvalidData(_) => None,
        }
    }
}

impl From<csv::Error> for RelationError {
    fn from(e: csv::Error) -> Self { RelationError::Csv(e) }
}

impl From<std::io::Error> for RelationError {
    fn from(e: std::io::Error) -> Self { RelationError::Io(e) }
}

impl From<parquet::errors::ParquetError> for RelationError {
    fn from(e: parquet::errors::ParquetError) -> Self { RelationError::Parquet(e) }
}

impl From<arrow::error::ArrowError> for RelationError {
    fn from(e: arrow::error::ArrowError) -> Self { RelationError::Arrow(e) }
}

/// Whether a relation's attributes are identified by name or by position.
pub enum ModelType {
    /// Attributes are accessed by column index only.
    Positional,
    /// Attributes have explicit string names.
    Named,
}

/// Metadata for a relation: its name, attribute names, and arity.
///
/// A header can be "positional" (no attribute names, only arity) or "named"
/// (with explicit column names). A "nameless" header has an empty `name` field,
/// used for intermediate/projected relations.
#[derive(Clone, Debug)]
pub struct RelationHeader {
    name: String,
    attrs: Vec<String>,
    arity: usize,
}

impl RelationHeader {
    /// Creates a new `RelationHeader` with the specified name, attributes, and
    /// arity.
    pub fn new(name: impl Into<String>, attrs: Vec<String>) -> Self {
        let arity = attrs.len();
        RelationHeader {
            name: name.into(),
            attrs,
            arity,
        }
    }

    /// Creates a nameless header with the given attribute names. Arity is
    /// inferred from the length of `attrs`.
    pub fn new_nameless(attrs: Vec<String>) -> Self {
        let arity = attrs.len();
        RelationHeader {
            name: String::new(),
            attrs,
            arity,
        }
    }

    /// Creates a named header with positional (unnamed) attributes.
    pub fn new_positional(name: impl Into<String>, arity: usize) -> Self {
        RelationHeader {
            name: name.into(),
            attrs: vec![],
            arity,
        }
    }

    /// Creates a nameless header with positional attributes of the given arity.
    pub fn new_nameless_positional(arity: usize) -> Self {
        RelationHeader {
            name: String::new(),
            attrs: vec![],
            arity,
        }
    }

    pub fn is_nameless(&self) -> bool { self.name.is_empty() }

    pub fn name(&self) -> &str { &self.name }

    pub fn attrs(&self) -> &[String] { &self.attrs }

    pub fn arity(&self) -> usize { self.arity }

    pub fn model_type(&self) -> ModelType {
        if self.attrs.is_empty() {
            ModelType::Positional
        } else {
            ModelType::Named
        }
    }
}

impl From<usize> for RelationHeader {
    fn from(value: usize) -> RelationHeader { RelationHeader::new_nameless_positional(value) }
}

/// A relation that can produce a new relation containing only the specified
/// columns.
pub trait Projectable {
    /// Returns a new relation containing only the columns at the given indices.
    fn project(&self, columns: Vec<usize>) -> Self;
}

/// The `Relation` trait defines a relational data structure that can store and
/// retrieve tuples of `usize` keys, and participate in join operations.
pub trait Relation: JoinIterable + Projectable {
    /// Returns the header (name, attributes, arity) of this relation.
    fn header(&self) -> &RelationHeader;

    /// Creates a new relation with the specified arity.
    fn new(header: RelationHeader) -> Self;

    /// Creates a new relation with the specified arity and given tuples.
    fn from_tuples(header: RelationHeader, tuples: Vec<Vec<usize>>) -> Self;

    /// Inserts a tuple into the relation, returning `true` if successful and
    /// `false` if otherwise.
    fn insert(&mut self, tuple: Vec<usize>) -> bool;

    /// Inserts multiple tuples into the relation, returning `true` if
    /// successful and `false` if otherwise.
    fn insert_all(&mut self, tuples: Vec<Vec<usize>>) -> bool;
}

/// Extension trait for `Relation` to add file reading capabilities.
pub trait RelationFileExt: Relation {
    /// Creates a new relation from a Parquet file with header.
    ///
    /// This method extracts column names from the Parquet schema and the
    /// relation name from the filename.
    fn from_parquet<P: AsRef<Path>>(filepath: P) -> Result<Self, RelationError>
    where
        Self: Sized;

    /// Creates a new relation from a CSV file.
    ///
    /// # Note
    /// * Each line represents a tuple, and each value in the line should be
    ///   parsable into `Relation::KT`.
    fn from_csv<P: AsRef<Path>>(filepath: P) -> Result<Self, RelationError>
    where
        Self: Sized;
}

/// Blanket implementation of `RelationFileExt` for any type that
/// implements `Relation`.
impl<R> RelationFileExt for R
where
    R: Relation,
{
    fn from_csv<P: AsRef<Path>>(filepath: P) -> Result<Self, RelationError> {
        let path = filepath.as_ref();
        let file = File::open(path)?;

        let mut rdr = csv::ReaderBuilder::new()
            .has_headers(true)
            .delimiter(b',')
            .double_quote(false)
            .escape(Some(b'\\'))
            .flexible(false)
            .comment(Some(b'#'))
            .from_reader(file);

        // Extract column names from CSV header
        let attrs: Vec<String> = rdr.headers()?.iter().map(|s| s.to_string()).collect();

        // Extract relation name from filename (without extension)
        let relation_name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();

        // Create header from the CSV header with the extracted name
        let header = RelationHeader::new(relation_name, attrs);

        let mut tuples = Vec::new();
        for (row_idx, result) in rdr.records().enumerate() {
            let record = result?;
            let mut tuple: Vec<usize> = Vec::with_capacity(record.len());
            for (col_idx, field) in record.iter().enumerate() {
                let value = field.parse::<usize>().map_err(|_| {
                    RelationError::InvalidData(format!(
                        "row {row_idx}, column {col_idx}: cannot parse {:?} as usize",
                        field,
                    ))
                })?;
                tuple.push(value);
            }
            tuples.push(tuple);
        }
        Ok(R::from_tuples(header, tuples))
    }

    fn from_parquet<P: AsRef<Path>>(filepath: P) -> Result<Self, RelationError> {
        let path = filepath.as_ref();
        let file = File::open(path)?;

        let builder = ParquetRecordBatchReaderBuilder::try_new(file)?;

        // Extract schema to get column names
        let schema = builder.schema();
        let attrs: Vec<String> = schema
            .fields()
            .iter()
            .map(|field| field.name().clone())
            .collect();

        // Extract relation name from filename (without extension)
        let relation_name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();

        // Create header from the parquet schema with the extracted name
        let header = RelationHeader::new(relation_name, attrs);

        // Build the reader
        let reader = builder.build()?;

        // Collect all tuples first for efficient construction
        let mut tuples = Vec::new();

        // Read all record batches and collect tuples
        for batch_result in reader {
            let batch = batch_result?;

            let num_rows = batch.num_rows();
            let num_cols = batch.num_columns();

            // Convert columnar data to row format (tuples)
            for row_idx in 0..num_rows {
                let mut tuple: Vec<usize> = Vec::with_capacity(num_cols);

                for col_idx in 0..num_cols {
                    let column = batch.column(col_idx);
                    let int_array = column.as_primitive::<arrow::datatypes::Int64Type>();

                    if let Ok(value) = usize::try_from(int_array.value(row_idx)) {
                        tuple.push(value);
                    } else {
                        return Err(RelationError::InvalidData(
                            "failed to convert Parquet value to usize".into(),
                        ));
                    }
                }

                tuples.push(tuple);
            }
        }

        // Use from_tuples for efficient construction (sorts before insertion)
        Ok(R::from_tuples(header, tuples))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── RelationError Display ──────────────────────────────────────────

    #[test]
    fn relation_error_display_csv() {
        let csv_err = csv::Error::from(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "file not found",
        ));
        let err = RelationError::from(csv_err);
        let msg = err.to_string();
        assert!(msg.starts_with("CSV error:"), "got: {msg}");
    }

    #[test]
    fn relation_error_display_io() {
        let err = RelationError::from(std::io::Error::new(std::io::ErrorKind::NotFound, "gone"));
        assert!(err.to_string().starts_with("I/O error:"));
    }

    #[test]
    fn relation_error_display_invalid_data() {
        let err = RelationError::InvalidData("bad value".into());
        assert_eq!(err.to_string(), "Invalid data: bad value");
    }

    #[test]
    fn relation_error_source_delegates() {
        use std::error::Error;

        let io_err = std::io::Error::other("inner");
        let err = RelationError::Io(io_err);
        assert!(err.source().is_some());

        let err = RelationError::InvalidData("no source".into());
        assert!(err.source().is_none());
    }

    // ── from_csv error on invalid data ─────────────────────────────────

    #[test]
    fn from_csv_rejects_non_integer_values() {
        use crate::ds::TreeTrie;

        let dir = std::env::temp_dir();
        let path = dir.join("test_csv_bad_value.csv");
        std::fs::write(&path, "a,b\n1,2\n3,hello\n").unwrap();

        let result = TreeTrie::from_csv(&path);
        assert!(result.is_err(), "expected error for non-integer CSV value");

        let err = result.unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("hello"),
            "error should mention the bad value, got: {msg}"
        );
        assert!(
            msg.contains("row 1"),
            "error should mention the row, got: {msg}"
        );
        assert!(
            msg.contains("column 1"),
            "error should mention the column, got: {msg}"
        );

        std::fs::remove_file(path).ok();
    }

    #[test]
    fn from_csv_missing_file_returns_error() {
        use crate::ds::TreeTrie;

        let result = TreeTrie::from_csv("/tmp/nonexistent_kermit_test_file.csv");
        assert!(result.is_err());
        assert!(
            matches!(result.unwrap_err(), RelationError::Io(_)),
            "expected Io variant for missing file"
        );
    }

    // ── from_parquet error paths ───────────────────────────────────────

    #[test]
    fn from_parquet_missing_file_returns_error() {
        use crate::ds::TreeTrie;

        let result = TreeTrie::from_parquet("/tmp/nonexistent_kermit_test_file.parquet");
        assert!(result.is_err());
        assert!(
            matches!(result.unwrap_err(), RelationError::Io(_)),
            "expected Io variant for missing file"
        );
    }

    #[test]
    fn from_parquet_invalid_file_returns_error() {
        use crate::ds::TreeTrie;

        let dir = std::env::temp_dir();
        let path = dir.join("test_bad_parquet.parquet");
        std::fs::write(&path, b"this is not a parquet file").unwrap();

        let result = TreeTrie::from_parquet(&path);
        assert!(result.is_err());
        assert!(
            matches!(result.unwrap_err(), RelationError::Parquet(_)),
            "expected Parquet variant for corrupt file"
        );

        std::fs::remove_file(path).ok();
    }
}
