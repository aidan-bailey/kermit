//! Core relation abstraction: the [`Relation`] trait that every storage
//! backend implements (see `TreeTrie`, `ColumnTrie`, and `HashTrie` in
//! [`crate::ds`]),
//! plus the blanket [`RelationFileExt`] for loading from CSV or Parquet.
//!
//! The trait exists so join algorithms in `kermit-algos` can be written
//! generically over different trie layouts without coupling to a specific
//! representation. All tuple values are `usize` keys — typically
//! dictionary-encoded IDs from a separate symbol table — so a relation never
//! stores raw strings or domain values directly.
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
///
/// Returned by [`RelationHeader::model_type`]. A header is [`Named`] when it
/// carries explicit attribute names (e.g. from a CSV/Parquet schema), and
/// [`Positional`] otherwise — typical for intermediate relations produced
/// during query evaluation where only arity matters.
///
/// [`Named`]: ModelType::Named
/// [`Positional`]: ModelType::Positional
pub enum ModelType {
    /// Attributes are accessed by column index only; attribute names are
    /// absent.
    Positional,
    /// Attributes have explicit string names, typically sourced from a file
    /// header or schema.
    Named,
}

/// Metadata for a relation: its name, attribute names, and arity.
///
/// A header is **named** when `attrs` is non-empty (then `arity ==
/// attrs.len()`) and **positional** when `attrs` is empty (then `arity` is
/// the only authoritative column count). Orthogonally, a header is
/// **nameless** when its `name` is empty — used for intermediate or
/// projected relations whose origin no longer matters.
#[derive(Clone, Debug)]
pub struct RelationHeader {
    name: String,
    /// Attribute names. Empty iff this is a positional header.
    attrs: Vec<String>,
    /// Number of columns. For named headers this equals `attrs.len()`; for
    /// positional headers (`attrs.is_empty()`) it is the only authoritative
    /// column count.
    arity: usize,
}

impl RelationHeader {
    /// Creates a named header with the given attribute names. Arity is
    /// derived from `attrs.len()`.
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

    /// Returns `true` if this header has an empty name (i.e. was created via
    /// one of the `new_nameless*` constructors).
    pub fn is_nameless(&self) -> bool { self.name.is_empty() }

    /// Returns the relation's name (empty string for nameless headers).
    pub fn name(&self) -> &str { &self.name }

    /// Returns the attribute names. Empty for positional headers.
    pub fn attrs(&self) -> &[String] { &self.attrs }

    /// Returns the arity (number of columns) of the relation.
    pub fn arity(&self) -> usize { self.arity }

    /// Returns [`ModelType::Named`] when attribute names are set, otherwise
    /// [`ModelType::Positional`].
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
///
/// Projection is the π operator from relational algebra: given column indices
/// `[c₀, c₁, …]` it yields a relation whose `i`-th column is the `cᵢ`-th
/// column of the source. Duplicate and reordered indices are permitted; the
/// resulting relation has arity `columns.len()`.
pub trait Projectable {
    /// Returns a new relation containing only the columns at the given
    /// indices, in the order supplied.
    ///
    /// # Panics
    ///
    /// Panics if any element of `columns` is `>= self.header().arity()`.
    fn project(&self, columns: Vec<usize>) -> Self;
}

/// Default trie-iter-based projection shared by every storage backend.
///
/// Materialises every tuple via the trie iterator, projects the requested
/// columns, then rebuilds via `from_tuples`. Backends that can do better
/// (e.g. a column-store that can keep the existing layer arrays for the
/// requested columns) may override [`Projectable::project`] with their own
/// implementation.
pub(crate) fn project_via_trie_iter<R>(rel: &R, columns: Vec<usize>) -> R
where
    R: Relation + kermit_iters::TrieIterable,
{
    let current_header = rel.header();
    let projected_attrs: Vec<String> = columns
        .iter()
        .filter_map(|&col_idx| current_header.attrs().get(col_idx).cloned())
        .collect();

    let new_header = if projected_attrs.is_empty() {
        RelationHeader::new_nameless_positional(columns.len())
    } else {
        RelationHeader::new_nameless(projected_attrs)
    };

    let projected_tuples: Vec<Vec<usize>> = rel
        .trie_iter()
        .into_iter()
        .map(|tuple| columns.iter().map(|&col_idx| tuple[col_idx]).collect())
        .collect();

    R::from_tuples(new_header, projected_tuples)
}

/// A relational data structure that stores tuples of `usize` keys and can
/// participate in joins.
///
/// Tuple values are `usize` keys (typically dictionary-encoded — see the
/// module-level docs). The supertraits expose:
///
/// - [`JoinIterable`] — produces iterators that the join algorithms in
///   `kermit-algos` consume. Implementors typically also implement
///   [`TrieIterable`](kermit_iters::TrieIterable) so the iterator can be driven
///   hierarchically.
/// - [`Projectable`] — the relational π operator (column projection).
pub trait Relation: JoinIterable + Projectable {
    /// Returns the header describing this relation's name, attributes, and
    /// arity.
    fn header(&self) -> &RelationHeader;

    /// Creates an empty relation matching `header`.
    fn new(header: RelationHeader) -> Self;

    /// Creates a relation populated with `tuples`, matching `header`.
    /// Implementations may sort or deduplicate during bulk construction;
    /// prefer this over `new` followed by repeated `insert` calls when all
    /// tuples are known up front.
    ///
    /// # Panics
    ///
    /// Panics if any tuple's length does not equal `header.arity()`.
    fn from_tuples(header: RelationHeader, tuples: Vec<Vec<usize>>) -> Self;

    /// Inserts a tuple. Duplicate tuples are silently absorbed (the relation
    /// behaves as a set).
    ///
    /// # Panics
    ///
    /// Panics if `tuple.len() != self.header().arity()`.
    fn insert(&mut self, tuple: Vec<usize>);

    /// Inserts every tuple in `tuples`. Equivalent to calling
    /// [`insert`](Self::insert) in a loop; provided so implementations can
    /// specialise bulk insertion.
    ///
    /// # Panics
    ///
    /// Panics if any tuple's length does not match the relation's arity.
    fn insert_all(&mut self, tuples: Vec<Vec<usize>>);
}

/// A [`Relation`] with runtime configuration — the Config category of the
/// optimization standard (`docs/specs/optimization-standard.md`).
///
/// `Relation::new` / `Relation::from_tuples` have no parameter through which
/// a config value could travel, so this extension trait adds the
/// config-carrying constructors. A data structure implementing this should
/// make `Relation::new` equivalent to `with_config(header,
/// Self::Config::default())`, so plain-trait construction is unchanged from
/// pre-config behaviour. Wrapper types that exist to inject a configuration
/// (see `Configured` in `configured.rs`, added later) deliberately override
/// this.
///
/// Only structures with a Config axis implement this; structures without
/// one are not required to.
pub trait ConfigurableRelation: Relation {
    /// The runtime flags this structure reads.
    type Config: kermit_iters::ConfigOption;

    /// Creates an empty relation matching `header` that will honour
    /// `config` for every subsequent insert.
    fn with_config(header: RelationHeader, config: Self::Config) -> Self;

    /// Creates a relation populated with `tuples` under `config`. Same
    /// contract as [`Relation::from_tuples`].
    ///
    /// # Panics
    ///
    /// Panics if any tuple's length does not equal `header.arity()`.
    fn from_tuples_with_config(
        header: RelationHeader, config: Self::Config, tuples: Vec<Vec<usize>>,
    ) -> Self;

    /// The configuration this relation was built with.
    fn config(&self) -> &Self::Config;
}

/// Loads a [`Relation`] from a CSV or Parquet file.
///
/// Defined as an extension trait (with a blanket impl over every
/// [`Relation`]) so file-loading is added without bloating the core trait
/// or requiring each concrete data structure to reimplement it. Anything
/// that implements [`Relation`] automatically gains
/// [`from_csv`](Self::from_csv) and [`from_parquet`](Self::from_parquet).
pub trait RelationFileExt: Relation {
    /// Creates a new relation from a Parquet file.
    ///
    /// Column names are extracted from the Parquet schema and the relation
    /// name is taken from the file stem. All columns must be `Int64` and
    /// every value must be non-negative so it fits in `usize`.
    ///
    /// # Errors
    ///
    /// Returns a [`RelationError`] if any of the following occur:
    /// - [`RelationError::Io`] — the file cannot be opened.
    /// - [`RelationError::Parquet`] — the file is not a valid Parquet file or
    ///   the reader cannot be constructed.
    /// - [`RelationError::Arrow`] — a record batch fails to decode.
    /// - [`RelationError::InvalidData`] — an `Int64` value cannot be converted
    ///   to `usize` (e.g. it is negative).
    fn from_parquet<P: AsRef<Path>>(filepath: P) -> Result<Self, RelationError>
    where
        Self: Sized;

    /// Creates a new relation from a CSV file.
    ///
    /// The first row is treated as a header providing attribute names; each
    /// subsequent row is one tuple. Every field must parse as a `usize`. The
    /// relation name is taken from the file stem.
    ///
    /// # Errors
    ///
    /// Returns a [`RelationError`] if any of the following occur:
    /// - [`RelationError::Io`] — the file cannot be opened.
    /// - [`RelationError::Csv`] — the CSV reader cannot parse the header or a
    ///   row (e.g. inconsistent column count).
    /// - [`RelationError::InvalidData`] — a field cannot be parsed as a
    ///   `usize`; the message identifies the offending row and column.
    fn from_csv<P: AsRef<Path>>(filepath: P) -> Result<Self, RelationError>
    where
        Self: Sized;
}

/// Extracts the relation name from a file path: the file stem (filename
/// without extension), or an empty string if it cannot be determined.
fn file_stem(path: &Path) -> String {
    path.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string()
}

/// Reads a CSV file into a header (attribute names from the header row,
/// relation name from the file stem) and its tuples. Shared by
/// [`RelationFileExt::from_csv`] and by callers that need to build with a
/// non-default configuration
/// ([`ConfigurableRelation::from_tuples_with_config`]).
///
/// # Errors
///
/// Same conditions as [`RelationFileExt::from_csv`].
pub fn read_csv<P: AsRef<Path>>(
    filepath: P,
) -> Result<(RelationHeader, Vec<Vec<usize>>), RelationError> {
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

    // Create header from the CSV header with the extracted name
    let header = RelationHeader::new(file_stem(path), attrs);

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
    Ok((header, tuples))
}

/// Reads a Parquet file into a header (column names from the schema,
/// relation name from the file stem) and its tuples. Counterpart of
/// [`read_csv`]. Shared by [`RelationFileExt::from_parquet`] and by callers
/// that need to build with a non-default configuration
/// ([`ConfigurableRelation::from_tuples_with_config`]).
///
/// # Errors
///
/// Same conditions as [`RelationFileExt::from_parquet`].
pub fn read_parquet<P: AsRef<Path>>(
    filepath: P,
) -> Result<(RelationHeader, Vec<Vec<usize>>), RelationError> {
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

    // Create header from the parquet schema with the extracted name
    let header = RelationHeader::new(file_stem(path), attrs);

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

    Ok((header, tuples))
}

/// Blanket implementation of `RelationFileExt` for any type that
/// implements `Relation`.
impl<R> RelationFileExt for R
where
    R: Relation,
{
    fn from_csv<P: AsRef<Path>>(filepath: P) -> Result<Self, RelationError> {
        let (header, tuples) = read_csv(filepath)?;
        // Use from_tuples for efficient construction (sorts before insertion)
        Ok(R::from_tuples(header, tuples))
    }

    fn from_parquet<P: AsRef<Path>>(filepath: P) -> Result<Self, RelationError> {
        let (header, tuples) = read_parquet(filepath)?;
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

    #[test]
    fn read_csv_returns_header_and_tuples() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("edge.csv");
        std::fs::write(&path, "a,b\n1,2\n3,4\n").unwrap();
        let (header, tuples) = read_csv(&path).unwrap();
        assert_eq!(header.name(), "edge");
        assert_eq!(header.arity(), 2);
        assert_eq!(tuples, vec![vec![1, 2], vec![3, 4]]);
    }
}
