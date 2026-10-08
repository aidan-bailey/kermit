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
    kermit_iters::{JoinIterable, Tuples},
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
#[derive(Clone, Debug, PartialEq, Eq)]
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
    /// `tuples` is a [`Tuples`] batch, or anything that converts into one:
    /// test fixtures pass one `Vec<usize>` per tuple, whose conversion
    /// panics on mixed arity. An empty batch is accepted under any header.
    /// Implementations may sort or deduplicate during bulk construction;
    /// prefer this over `new` followed by repeated `insert` calls when all
    /// tuples are known up front.
    ///
    /// # Panics
    ///
    /// Panics if `tuples` is not empty and its arity does not equal
    /// `header.arity()`.
    fn from_tuples(header: RelationHeader, tuples: impl Into<Tuples>) -> Self;

    /// Inserts a tuple, given as anything that lends a slice of keys
    /// (`&[usize]`, `Vec<usize>`, an array). Duplicate tuples are silently
    /// absorbed (the relation behaves as a set).
    ///
    /// # Panics
    ///
    /// Panics if the tuple's length is not `self.header().arity()`.
    fn insert(&mut self, tuple: impl AsRef<[usize]>);

    /// Inserts every row of `tuples`. Equivalent to calling
    /// [`insert`](Self::insert) on each row in a loop; provided so
    /// implementations can specialise bulk insertion.
    ///
    /// # Panics
    ///
    /// Panics if `tuples` is not empty and its arity does not match the
    /// relation's arity.
    fn insert_all(&mut self, tuples: impl Into<Tuples>);
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
    /// Panics if `tuples` is not empty and its arity does not equal
    /// `header.arity()`.
    fn from_tuples_with_config(
        header: RelationHeader, config: Self::Config, tuples: impl Into<Tuples>,
    ) -> Self;

    /// The configuration this relation was built with.
    fn config(&self) -> &Self::Config;
}

/// A [`Relation`] with more than one way to build from a known set of
/// tuples — the BuildMode category of the optimization standard
/// (`docs/specs/optimization-standard.md`).
///
/// Every mode must build an *equivalent* relation: the same contents and the
/// same capacities, hence the same [`HeapSize`](crate::HeapSize). Placement
/// the structure leaves free, such as the slot a key takes in a hash table,
/// may differ between modes (the optimization standard's Amendment 2). Only
/// the construction process differs. [`Relation::from_tuples`] must equal
/// `from_tuples_with_build_mode(header, Self::BuildMode::default(), tuples)`.
/// Wrapper types that exist to inject a mode (see `BuiltWith` in
/// `built_with.rs`) deliberately do not implement this.
///
/// Only structures with a BuildMode axis implement this.
pub trait BuildModeRelation: Relation {
    /// The construction processes this structure offers.
    type BuildMode: kermit_iters::BuildMode + Copy + Default;

    /// Creates a relation populated with `tuples`, built by `mode`. Same
    /// contract as [`Relation::from_tuples`].
    ///
    /// # Panics
    ///
    /// Panics if `tuples` is not empty and its arity does not equal
    /// `header.arity()`, or if `mode` has a prerequisite the default config
    /// lacks (`HashTrie`'s `presized:N` requires `root-capacity=tuples`).
    fn from_tuples_with_build_mode(
        header: RelationHeader, mode: Self::BuildMode, tuples: impl Into<Tuples>,
    ) -> Self;
}

/// A relation with both a Config and a BuildMode, and the one constructor
/// that takes both, so the two test markers can stack:
/// `BuiltWith<Configured<R, C>, M>` builds by `M`'s mode under `C`'s config.
pub trait ConfiguredBuildModeRelation: ConfigurableRelation + BuildModeRelation {
    /// Builds `tuples` by `mode` under `config`. Same contract as
    /// [`BuildModeRelation::from_tuples_with_build_mode`].
    fn from_tuples_with_config_and_build_mode(
        header: RelationHeader, config: Self::Config, mode: Self::BuildMode,
        tuples: impl Into<Tuples>,
    ) -> Self;
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

/// Opens `path` as a relation CSV and reads its header: attribute names
/// from the first non-comment row, relation name from the file stem. The
/// one place CSV header parsing happens — [`read_csv`] continues from the
/// returned reader, [`read_csv_header`] stops here.
fn open_csv(path: &Path) -> Result<(RelationHeader, csv::Reader<File>), RelationError> {
    let file = File::open(path)?;
    let mut rdr = csv::ReaderBuilder::new()
        .has_headers(true)
        .delimiter(b',')
        .double_quote(false)
        .escape(Some(b'\\'))
        .flexible(false)
        .comment(Some(b'#'))
        .from_reader(file);
    let attrs: Vec<String> = rdr.headers()?.iter().map(|s| s.to_string()).collect();
    Ok((RelationHeader::new(file_stem(path), attrs), rdr))
}

/// Reads only the header of a relation CSV, never its data rows: what a
/// caller needs to check a query against a relation's name and arity
/// before paying for the build.
///
/// # Errors
///
/// [`RelationError::Io`] if the file cannot be opened;
/// [`RelationError::Csv`] if the header row cannot be parsed.
pub fn read_csv_header<P: AsRef<Path>>(filepath: P) -> Result<RelationHeader, RelationError> {
    open_csv(filepath.as_ref()).map(|(header, _)| header)
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
    let (header, mut rdr) = open_csv(filepath.as_ref())?;

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

/// Opens `path` as a relation Parquet file and reads its header: column
/// names from the schema in the footer, relation name from the file stem.
/// The arity therefore holds even for a file with no rows. The one place
/// Parquet header parsing happens — [`read_parquet`] continues from the
/// returned builder, [`read_parquet_header`] stops here.
fn open_parquet(
    path: &Path,
) -> Result<(RelationHeader, ParquetRecordBatchReaderBuilder<File>), RelationError> {
    let builder = ParquetRecordBatchReaderBuilder::try_new(File::open(path)?)?;
    let attrs: Vec<String> = builder
        .schema()
        .fields()
        .iter()
        .map(|field| field.name().clone())
        .collect();
    Ok((RelationHeader::new(file_stem(path), attrs), builder))
}

/// Reads only the header of a relation Parquet file (its footer schema),
/// never its row groups. Counterpart of [`read_csv_header`].
///
/// # Errors
///
/// [`RelationError::Io`] if the file cannot be opened;
/// [`RelationError::Parquet`] if it is not a readable Parquet file.
pub fn read_parquet_header<P: AsRef<Path>>(filepath: P) -> Result<RelationHeader, RelationError> {
    open_parquet(filepath.as_ref()).map(|(header, _)| header)
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
    let (header, builder) = open_parquet(filepath.as_ref())?;

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

        let result: Result<TreeTrie, _> = TreeTrie::from_csv(&path);
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

        let result: Result<TreeTrie, _> =
            TreeTrie::from_csv("/tmp/nonexistent_kermit_test_file.csv");
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

        let result: Result<TreeTrie, _> =
            TreeTrie::from_parquet("/tmp/nonexistent_kermit_test_file.parquet");
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

        let result: Result<TreeTrie, _> = TreeTrie::from_parquet(&path);
        assert!(result.is_err());
        assert!(
            matches!(result.unwrap_err(), RelationError::Parquet(_)),
            "expected Parquet variant for corrupt file"
        );

        std::fs::remove_file(path).ok();
    }

    /// Writes `rows` as an `Int64` Parquet file with one column per name.
    fn write_parquet(path: &Path, attrs: &[&str], rows: &[Vec<i64>]) {
        use {
            arrow::{
                array::{ArrayRef, Int64Array, RecordBatch},
                datatypes::{DataType, Field, Schema},
            },
            parquet::arrow::ArrowWriter,
            std::sync::Arc,
        };
        let schema = Arc::new(Schema::new(
            attrs
                .iter()
                .map(|a| Field::new(*a, DataType::Int64, false))
                .collect::<Vec<_>>(),
        ));
        let columns: Vec<ArrayRef> = (0..attrs.len())
            .map(|c| {
                Arc::new(Int64Array::from(
                    rows.iter().map(|r| r[c]).collect::<Vec<_>>(),
                )) as ArrayRef
            })
            .collect();
        let batch = RecordBatch::try_new(schema.clone(), columns).unwrap();
        let mut writer = ArrowWriter::try_new(File::create(path).unwrap(), schema, None).unwrap();
        writer.write(&batch).unwrap();
        writer.close().unwrap();
    }

    /// The header comes from the first non-comment line alone: a malformed
    /// data row that `read_csv` rejects is never read.
    #[test]
    fn read_csv_header_reads_only_the_header() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("edge.csv");
        std::fs::write(&path, "# comment\na,b\n1,x\n").unwrap();
        assert!(read_csv(&path).is_err());
        let header = read_csv_header(&path).unwrap();
        assert_eq!(header.name(), "edge");
        assert_eq!(header.attrs(), ["a", "b"]);
    }

    #[test]
    fn read_csv_header_agrees_with_read_csv() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("edge.csv");
        std::fs::write(&path, "# comment\nsrc,dst\n1,2\n3,4\n").unwrap();
        assert_eq!(read_csv_header(&path).unwrap(), read_csv(&path).unwrap().0);
    }

    #[test]
    fn read_parquet_header_agrees_with_read_parquet() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("triple.parquet");
        write_parquet(&path, &["a", "b", "c"], &[vec![1, 2, 3], vec![4, 5, 6]]);
        let header = read_parquet_header(&path).unwrap();
        assert_eq!(header.arity(), 3);
        assert_eq!(header, read_parquet(&path).unwrap().0);
    }

    /// An empty relation's arity comes from the schema, not from a first
    /// row — the case of a WatDiv relation seeded for an absent predicate.
    #[test]
    fn empty_parquet_keeps_its_arity_from_the_schema() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("empty.parquet");
        write_parquet(&path, &["s", "o"], &[]);
        let header = read_parquet_header(&path).unwrap();
        assert_eq!(header.name(), "empty");
        assert_eq!(header.arity(), 2);
        let (full, tuples) = read_parquet(&path).unwrap();
        assert!(tuples.is_empty());
        assert_eq!(header, full);
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

    // ── constructors take a batch (#111) ──────────────────────────────

    crate::define_config_provider!(
        DefaultConfig,
        crate::HashTrieConfig,
        crate::HashTrieConfig::default()
    );
    crate::define_build_mode_provider!(
        ColumnIncremental,
        crate::ColumnTrieBuildMode,
        crate::ColumnTrieBuildMode::Incremental
    );

    /// A sorted trie's tuples, in its (sorted) iteration order.
    fn trie_rows(relation: &impl kermit_iters::TrieIterable) -> Vec<Vec<usize>> {
        relation.trie_iter().into_iter().collect()
    }

    /// A hash trie's tuples, sorted (a hash trie lends them in hash order).
    fn hash_rows(relation: &crate::ds::HashTrie) -> Vec<Vec<usize>> {
        let mut rows = relation.collect_tuples();
        rows.sort();
        rows
    }

    /// Every constructor takes a [`Tuples`] batch as well as one `Vec` per
    /// tuple, and `insert` takes a slice or an array as well as a `Vec`;
    /// every form builds the same relation.
    #[test]
    fn every_constructor_takes_a_batch() {
        use crate::{
            ds::{
                ColumnTrie, ColumnTrieBuildMode, HashTrie, HashTrieBuildMode, HashTrieConfig,
                TreeTrie, TreeTrieBuildMode,
            },
            BuiltWith, Cardinality, Configured, HeapSize, Threads,
        };
        let vecs = vec![vec![3, 4], vec![1, 2], vec![1, 2], vec![1, 5]];
        let batch = Tuples::from(vecs.clone());
        let two = || Threads::new(2).unwrap();

        // Sorted tries: a set.
        let tree: TreeTrie = TreeTrie::from_tuples(2.into(), vecs);
        let set = trie_rows(&tree);
        assert_eq!(set, vec![vec![1, 2], vec![1, 5], vec![3, 4]]);
        let tree_batch: TreeTrie = TreeTrie::from_tuples(2.into(), batch.clone());
        assert_eq!(trie_rows(&tree_batch), set);
        assert_eq!(tree_batch.heap_size_bytes(), tree.heap_size_bytes());
        let tree_parallel: TreeTrie = TreeTrie::from_tuples_with_build_mode(
            2.into(),
            TreeTrieBuildMode::Parallel(two()),
            batch.clone(),
        );
        assert_eq!(trie_rows(&tree_parallel), set);
        let mut tree_inserted: TreeTrie = TreeTrie::new(2.into());
        for row in batch.rows() {
            tree_inserted.insert(row);
        }
        tree_inserted.insert([1, 5]);
        assert_eq!(trie_rows(&tree_inserted), set);
        let mut tree_all: TreeTrie = TreeTrie::new(2.into());
        tree_all.insert_all(batch.clone());
        assert_eq!(trie_rows(&tree_all), set);

        let column: ColumnTrie = ColumnTrie::from_tuples(2.into(), batch.clone());
        assert_eq!(trie_rows(&column), set);
        let column_incremental: ColumnTrie = ColumnTrie::from_tuples_with_build_mode(
            2.into(),
            ColumnTrieBuildMode::Incremental,
            batch.clone(),
        );
        assert_eq!(trie_rows(&column_incremental), set);
        let built_with =
            BuiltWith::<ColumnTrie, ColumnIncremental>::from_tuples(2.into(), batch.clone());
        assert_eq!(trie_rows(&built_with), set);
        let mut column_all: ColumnTrie = ColumnTrie::new(2.into());
        column_all.insert_all(batch.clone());
        column_all.insert(&[1, 5][..]);
        assert_eq!(trie_rows(&column_all), set);

        // The hash trie: a multiset.
        let multiset = vec![vec![1, 2], vec![1, 2], vec![1, 5], vec![3, 4]];
        let hash: HashTrie = HashTrie::from_tuples(2.into(), batch.clone());
        assert_eq!(hash_rows(&hash), multiset);
        let hash_config: HashTrie =
            HashTrie::from_tuples_with_config(2.into(), HashTrieConfig::default(), batch.clone());
        assert_eq!(hash_rows(&hash_config), multiset);
        let hash_incremental: HashTrie = HashTrie::from_tuples_with_build_mode(
            2.into(),
            HashTrieBuildMode::Incremental,
            batch.clone(),
        );
        assert_eq!(hash_rows(&hash_incremental), multiset);
        let hash_both: HashTrie = HashTrie::from_tuples_with_config_and_build_mode(
            2.into(),
            HashTrieConfig::default(),
            HashTrieBuildMode::Parallel(two()),
            batch.clone(),
        );
        assert_eq!(hash_rows(&hash_both), multiset);
        let configured =
            Configured::<HashTrie, DefaultConfig>::from_tuples(2.into(), batch.clone());
        assert_eq!(hash_rows(&configured), multiset);
        let mut hash_inserted: HashTrie = HashTrie::new(2.into());
        for row in batch.rows() {
            hash_inserted.insert(row);
        }
        assert_eq!(hash_rows(&hash_inserted), multiset);
        hash_inserted.insert_all(batch);
        assert_eq!(hash_inserted.tuple_count(), 8);
    }

    /// A batch carries one arity, so a mixed-arity input is rejected while
    /// it converts, before any build sees it.
    #[test]
    #[should_panic(expected = "Tuples::from: row 1 has arity 1")]
    fn mixed_arity_vectors_are_rejected_before_the_build() {
        use crate::ds::TreeTrie;
        let _: TreeTrie = TreeTrie::from_tuples(2.into(), vec![vec![1, 2], vec![3]]);
    }
}
