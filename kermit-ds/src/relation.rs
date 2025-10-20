//! This module defines the `Relation` trait and file reading extensions.
use {
    arrow::array::AsArray,
    csv::Error,
    kermit_iters::Joinable,
    parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder,
    std::{fs::File, path::Path, str::FromStr},
};

pub enum ModelType {
    Positional,
    Named,
}

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

    pub fn new_nameless(attrs: Vec<String>) -> Self {
        let arity = attrs.len();
        RelationHeader {
            name: String::new(),
            attrs,
            arity,
        }
    }

    pub fn new_positional(name: impl Into<String>, arity: usize) -> Self {
        RelationHeader {
            name: name.into(),
            attrs: vec![],
            arity,
        }
    }

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

pub trait Projectable {
    fn project(&self, columns: Vec<usize>) -> Self;
}

/// The `Relation` trait defines a relational data structure.
pub trait Relation: Joinable + Projectable {
    fn header(&self) -> &RelationHeader;

    /// Creates a new relation with the specified arity.
    fn new(header: RelationHeader) -> Self;

    /// Creates a new relation with the specified arity and given tuples.
    fn from_tuples(header: RelationHeader, tuples: Vec<Vec<Self::KT>>) -> Self;

    /// Inserts a tuple into the relation, returning `true` if successful and
    /// `false` if otherwise.
    fn insert(&mut self, tuple: Vec<Self::KT>) -> bool;

    /// Inserts multiple tuples into the relation, returning `true` if
    /// successful and `false` if otherwise.
    fn insert_all(&mut self, tuples: Vec<Vec<Self::KT>>) -> bool;
}

/// Extension trait for `Relation` to add file reading capabilities.
pub trait RelationFileExt: Relation {
    /// Creates a new relation from a Parquet file with header.
    ///
    /// This method extracts column names from the Parquet schema and the
    /// relation name from the filename.
    fn from_parquet<P: AsRef<Path>>(filepath: P) -> Result<Self, Error>
    where
        Self: Sized;

    /// Creates a new relation from a CSV file.
    ///
    /// # Note
    /// * The CSV file should not have headers, and the delimiter can be
    ///   specified.
    /// * Each line represents a tuple, and each value in the line should be
    ///   parsable into `Relation::KT`.
    fn from_csv<P: AsRef<Path>>(filepath: P, header: RelationHeader, delimiter: u8) -> Result<Self, Error>
    where
        Self: Sized;
}

/// Blanket implementation of `RelationFileExt` for any type that
/// implements `Relation`.
impl<R> RelationFileExt for R
where
    R: Relation,
    R::KT: FromStr + TryFrom<i64>,
{
    fn from_csv<P: AsRef<Path>>(filepath: P, header: RelationHeader, delimiter: u8) -> Result<Self, Error> {
        let file = File::open(filepath)?;
        let mut rdr = csv::ReaderBuilder::new()
            .has_headers(false)
            .delimiter(delimiter)
            .double_quote(false)
            .escape(Some(b'\\'))
            .flexible(false)
            .comment(Some(b'#'))
            .from_reader(file);
        
        let mut tuples = Vec::new();
        for result in rdr.records() {
            let record = result?;
            let mut tuple: Vec<R::KT> = vec![];
            for x in record.iter() {
                if let Ok(y) = x.to_string().parse::<R::KT>() {
                    tuple.push(y);
                }
            }
            tuples.push(tuple);
        }
        Ok(R::from_tuples(header, tuples))
    }
    
    fn from_parquet<P: AsRef<Path>>(filepath: P) -> Result<Self, Error> {
        let path = filepath.as_ref();
        let file = File::open(path)
            .map_err(|e| Error::from(std::io::Error::new(std::io::ErrorKind::Other, e)))?;
        
        let builder = ParquetRecordBatchReaderBuilder::try_new(file)
            .map_err(|e| Error::from(std::io::Error::new(std::io::ErrorKind::Other, e)))?;
        
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
        let mut reader = builder.build()
            .map_err(|e| Error::from(std::io::Error::new(std::io::ErrorKind::Other, e)))?;
        
        // Collect all tuples first for efficient construction
        let mut tuples = Vec::new();
        
        // Read all record batches and collect tuples
        while let Some(batch_result) = reader.next() {
            let batch = batch_result
                .map_err(|e| Error::from(std::io::Error::new(std::io::ErrorKind::Other, e)))?;
            
            let num_rows = batch.num_rows();
            let num_cols = batch.num_columns();
            
            // Convert columnar data to row format (tuples)
            for row_idx in 0..num_rows {
                let mut tuple = Vec::with_capacity(num_cols);
                
                for col_idx in 0..num_cols {
                    let column = batch.column(col_idx);
                    let int_array = column.as_primitive::<arrow::datatypes::Int64Type>();
                    
                    if let Some(value) = int_array.value(row_idx).try_into().ok() {
                        tuple.push(value);
                    } else {
                        return Err(Error::from(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            "Failed to convert parquet value",
                        )));
                    }
                }
                
                tuples.push(tuple);
            }
        }
        
        // Use from_tuples for efficient construction (sorts before insertion)
        Ok(R::from_tuples(header, tuples))
    }
}
