//! A streaming Parquet writer for relations: [`ParquetWriter`], and
//! [`write_parquet`] for a relation held whole.
//!
//! The counterpart of [`read_parquet`](crate::read_parquet), which it
//! inverts exactly.
use {
    crate::relation::{file_stem, RelationError, RelationHeader},
    arrow::{
        array::{ArrayRef, Int64Array, RecordBatch},
        datatypes::{DataType, Field, Schema, SchemaRef},
    },
    kermit_iters::{RowId, Tuples},
    parquet::{arrow::ArrowWriter, file::properties::WriterProperties},
    std::{
        fs::File,
        path::{Path, PathBuf},
        sync::Arc,
    },
};

/// Rows per chunk handed to `ArrowWriter`: the writer's own buffering
/// (`ArrowWriter` also holds the encoded row group in progress).
pub(crate) const WRITE_CHUNK_ROWS: usize = 1 << 16;
/// Rows per row group; a multiple of `WRITE_CHUNK_ROWS`.
pub(crate) const ROW_GROUP_ROWS: usize = 1 << 20;

/// Writes one relation to a Parquet file, batch by batch.
///
/// Four guarantees, the first three each pinned by a test:
///
/// - **The exact inverse of `read_parquet`.** The file has one non-nullable
///   `Int64` column per attribute, named as in the header, and `create`
///   requires the file stem to equal the header's name, so every successful
///   write satisfies `read_parquet(path) == (header, tuples)`.
/// - **Bytes independent of batching.** `ArrowWriter` fixes row-group
///   boundaries but cuts data pages at checkpoints counted from the start of
///   each input batch, so different batchings can give different bytes. The
///   writer therefore transposes caller rows into its own column buffers of
///   `WRITE_CHUNK_ROWS` rows and hands `ArrowWriter` only full chunks, plus
///   the remainder at `finish`. Rows per row group is pinned to
///   `ROW_GROUP_ROWS`, a multiple of the chunk size, so a chunk never
///   straddles a row group. `ArrowWriter`'s input then depends only on the
///   row sequence, and the writer's extra memory is one chunk whatever the
///   caller's batch size.
/// - **No partial files.** Rows go to `<path>.part`, renamed to `path` by
///   `finish`. A writer dropped before `finish`, or one whose `finish`
///   fails, removes `<path>.part` and never writes `path`.
/// - **Stable bytes for a fixed build only.** The footer records
///   `created_by`, which names the parquet crate version, so files differ
///   across versions of it.
///
/// After any error every later call returns an error too.
pub struct ParquetWriter {
    /// `None` once finished or dropped.
    writer: Option<ArrowWriter<File>>,
    schema: SchemaRef,
    /// One chunk buffer per column, each with capacity `WRITE_CHUNK_ROWS`.
    columns: Vec<Vec<i64>>,
    /// Rows currently in `columns`.
    buffered: usize,
    /// Rows accepted so far, across all batches.
    rows: u64,
    /// The final path.
    path: PathBuf,
    /// `<path>.part`: `path` with `.part` appended to its file name.
    part: PathBuf,
    /// Set by any error; later calls return `InvalidData`.
    failed: bool,
}

impl ParquetWriter {
    /// Opens `<path>.part` for writing, truncating a leftover one.
    ///
    /// # Errors
    ///
    /// [`RelationError::InvalidData`] if `header` has no named attributes
    /// (positional, or arity 0: a Parquet file needs one named column per
    /// attribute) or if `path`'s file stem differs from `header.name()`;
    /// [`RelationError::Io`] / [`RelationError::Parquet`] if the file cannot
    /// be created.
    pub fn create<P: AsRef<Path>>(
        path: P, header: &RelationHeader,
    ) -> Result<Self, RelationError> {
        let path = path.as_ref();
        if header.attrs().is_empty() {
            return Err(RelationError::InvalidData(format!(
                "relation `{}` has no named attributes; a Parquet file needs one named column \
                 per attribute",
                header.name()
            )));
        }
        let stem = file_stem(path);
        if stem != header.name() {
            return Err(RelationError::InvalidData(format!(
                "file stem `{stem}` of {} differs from the relation name `{}`",
                path.display(),
                header.name()
            )));
        }
        let schema: SchemaRef = Arc::new(Schema::new(
            header
                .attrs()
                .iter()
                .map(|attr| Field::new(attr, DataType::Int64, false))
                .collect::<Vec<_>>(),
        ));
        let properties = WriterProperties::builder()
            .set_max_row_group_row_count(Some(ROW_GROUP_ROWS))
            .build();
        let mut part_name = path.as_os_str().to_owned();
        part_name.push(".part");
        let part = PathBuf::from(part_name);
        let file = File::create(&part)?;
        let writer =
            ArrowWriter::try_new(file, schema.clone(), Some(properties)).inspect_err(|_| {
                let _ = std::fs::remove_file(&part);
            })?;
        Ok(ParquetWriter {
            writer: Some(writer),
            schema,
            columns: header
                .attrs()
                .iter()
                .map(|_| Vec::with_capacity(WRITE_CHUNK_ROWS))
                .collect(),
            buffered: 0,
            rows: 0,
            path: path.to_path_buf(),
            part,
            failed: false,
        })
    }

    /// Appends `batch`'s rows.
    ///
    /// # Errors
    ///
    /// [`RelationError::InvalidData`] if the writer already failed, if a
    /// non-empty `batch`'s arity differs from the header's (an empty batch is
    /// a no-op at any arity), if the running total would
    /// exceed [`RowId::MAX`] rows, or if a value exceeds `i64::MAX` (the
    /// message gives its row, counted across all batches, and its column);
    /// [`RelationError::Parquet`] / [`RelationError::Arrow`] if a chunk
    /// cannot be written. The writer is unusable afterwards.
    pub fn write_batch(&mut self, batch: &Tuples) -> Result<(), RelationError> {
        let result = self.append(batch);
        if result.is_err() {
            self.failed = true;
        }
        result
    }

    /// Flushes the last chunk, closes the file, renames `<path>.part` to
    /// `path`, and returns the number of rows written.
    ///
    /// # Errors
    ///
    /// [`RelationError::InvalidData`] if the writer already failed;
    /// [`RelationError::Parquet`] / [`RelationError::Arrow`] /
    /// [`RelationError::Io`] if the flush, the close or the rename fails.
    /// On any error `<path>.part` is removed and `path` is not written.
    pub fn finish(mut self) -> Result<u64, RelationError> {
        let result = self.commit();
        if result.is_err() {
            drop(self.writer.take());
            let _ = std::fs::remove_file(&self.part);
        }
        result
    }

    fn append(&mut self, batch: &Tuples) -> Result<(), RelationError> {
        if self.failed {
            return Err(RelationError::InvalidData(format!(
                "the writer for {} failed earlier and accepts no more batches",
                self.path.display()
            )));
        }
        // An empty batch is a no-op whatever its arity, as an empty batch is
        // accepted under any header elsewhere (`Relation::from_tuples`).
        if batch.is_empty() {
            return Ok(());
        }
        if batch.arity() != self.columns.len() {
            return Err(RelationError::InvalidData(format!(
                "a batch of arity {} for a relation of arity {}",
                batch.arity(),
                self.columns.len()
            )));
        }
        if self.rows + batch.len() as u64 > u64::from(RowId::MAX) {
            return Err(RelationError::InvalidData(format!(
                "{} rows exceed the {} rows a relation holds",
                self.rows + batch.len() as u64,
                RowId::MAX
            )));
        }
        for row in batch.rows() {
            let row_number = self.rows;
            for (column_index, (column, &value)) in self.columns.iter_mut().zip(row).enumerate() {
                let value = i64::try_from(value).map_err(|_| {
                    RelationError::InvalidData(format!(
                        "row {row_number}, column {column_index}: {value} exceeds i64::MAX"
                    ))
                })?;
                column.push(value);
            }
            self.rows += 1;
            self.buffered += 1;
            if self.buffered == WRITE_CHUNK_ROWS {
                self.flush_chunk()?;
            }
        }
        Ok(())
    }

    /// Hands the buffered rows, if any, to `ArrowWriter` as one record batch
    /// and starts empty chunk buffers.
    fn flush_chunk(&mut self) -> Result<(), RelationError> {
        if self.buffered == 0 {
            return Ok(());
        }
        let arrays: Vec<ArrayRef> = self
            .columns
            .iter_mut()
            .map(|column| {
                let values = std::mem::replace(column, Vec::with_capacity(WRITE_CHUNK_ROWS));
                Arc::new(Int64Array::from(values)) as ArrayRef
            })
            .collect();
        self.buffered = 0;
        let batch = RecordBatch::try_new(self.schema.clone(), arrays)?;
        self.writer
            .as_mut()
            .expect("a ParquetWriter holds its writer until finish or drop")
            .write(&batch)?;
        Ok(())
    }

    fn commit(&mut self) -> Result<u64, RelationError> {
        if self.failed {
            return Err(RelationError::InvalidData(format!(
                "the writer for {} failed earlier and cannot finish",
                self.path.display()
            )));
        }
        self.flush_chunk()?;
        let writer = self
            .writer
            .take()
            .expect("a ParquetWriter holds its writer until finish or drop");
        writer.close()?;
        std::fs::rename(&self.part, &self.path)?;
        Ok(self.rows)
    }
}

impl Drop for ParquetWriter {
    /// A writer dropped before `finish` removes its `.part` file, ignoring
    /// errors.
    fn drop(&mut self) {
        if self.writer.take().is_some() {
            let _ = std::fs::remove_file(&self.part);
        }
    }
}

/// Writes `tuples` to `path` as a Parquet file: [`ParquetWriter::create`],
/// one [`ParquetWriter::write_batch`], then [`ParquetWriter::finish`]. See
/// [`ParquetWriter`] for the guarantees.
///
/// # Errors
///
/// Those of the three calls.
pub fn write_parquet<P: AsRef<Path>>(
    path: P, header: &RelationHeader, tuples: &Tuples,
) -> Result<(), RelationError> {
    let mut writer = ParquetWriter::create(path, header)?;
    writer.write_batch(tuples)?;
    writer.finish().map(|_| ())
}

#[cfg(test)]
mod tests {
    use {super::*, crate::read_parquet};

    /// A header named `name` with attributes `c0`, `c1`, ...
    fn header_of(name: &str, arity: usize) -> RelationHeader {
        RelationHeader::new(name, (0..arity).map(|c| format!("c{c}")).collect())
    }

    /// `rows` deterministic rows of `arity` values, varied enough that
    /// neighbouring rows and columns differ.
    fn rows_of(arity: usize, rows: usize) -> Tuples {
        let mut tuples = Tuples::with_capacity(arity, rows);
        for r in 0..rows {
            let row: Vec<usize> = (0..arity)
                .map(|c| (r * 2_654_435_761 + c * 40_503) % 1_000_003)
                .collect();
            tuples.push(&row);
        }
        tuples
    }

    /// The message of an `InvalidData` error; any other outcome panics.
    /// (`ParquetWriter` is not `Debug`, so `unwrap_err` is unavailable.)
    fn invalid_message<T>(result: Result<T, RelationError>) -> String {
        match result.err() {
            | Some(RelationError::InvalidData(message)) => message,
            | other => panic!("expected InvalidData, got {other:?}"),
        }
    }

    #[test]
    fn round_trips_arities_1_2_3() {
        let dir = tempfile::tempdir().unwrap();
        for arity in 1..=3 {
            let name = format!("rel{arity}");
            let header = header_of(&name, arity);
            let tuples = rows_of(arity, 2_500);
            let path = dir.path().join(format!("{name}.parquet"));
            write_parquet(&path, &header, &tuples).unwrap();
            assert!(
                !dir.path().join(format!("{name}.parquet.part")).exists(),
                "arity {arity}: finish leaves no .part file"
            );
            assert_eq!(read_parquet(&path).unwrap(), (header, tuples), "arity {arity}");
        }
    }

    #[test]
    fn round_trips_zero_rows() {
        let dir = tempfile::tempdir().unwrap();
        let header = header_of("empty", 2);
        let tuples = Tuples::new(2);
        let path = dir.path().join("empty.parquet");
        write_parquet(&path, &header, &tuples).unwrap();
        let (read_header, read_tuples) = read_parquet(&path).unwrap();
        assert_eq!(read_header, header);
        assert_eq!(read_tuples.arity(), 2);
        assert!(read_tuples.is_empty());
    }

    #[test]
    fn rejects_positional_header() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("p.parquet");
        let message =
            invalid_message(ParquetWriter::create(&path, &RelationHeader::new_positional("p", 2)));
        assert!(message.contains("named"), "{message}");
        let message =
            invalid_message(ParquetWriter::create(&path, &RelationHeader::new_positional("p", 0)));
        assert!(message.contains("named"), "{message}");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[test]
    fn rejects_stem_mismatch() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("other.parquet");
        let message = invalid_message(ParquetWriter::create(&path, &header_of("edge", 2)));
        assert!(message.contains("other"), "{message}");
        assert!(message.contains("edge"), "{message}");
        assert_eq!(
            std::fs::read_dir(dir.path()).unwrap().count(),
            0,
            "a rejected create makes no file"
        );
    }

    #[test]
    fn rejects_batch_arity_mismatch() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("edge.parquet");
        let mut writer = ParquetWriter::create(&path, &header_of("edge", 2)).unwrap();
        let message = invalid_message(writer.write_batch(&rows_of(3, 4)));
        assert!(message.contains('3'), "{message}");
        assert!(message.contains('2'), "{message}");
    }

    #[test]
    fn empty_batch_of_any_arity_is_a_no_op() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("edge.parquet");
        let header = header_of("edge", 2);
        let mut writer = ParquetWriter::create(&path, &header).unwrap();
        writer.write_batch(&Tuples::new(3)).unwrap();
        writer.write_batch(&rows_of(2, 4)).unwrap();
        writer.write_batch(&Tuples::new(0)).unwrap();
        assert_eq!(writer.finish().unwrap(), 4);
        assert_eq!(read_parquet(&path).unwrap(), (header, rows_of(2, 4)));
    }

    #[test]
    fn rejects_value_above_i64_max() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("edge.parquet");
        let mut writer = ParquetWriter::create(&path, &header_of("edge", 2)).unwrap();
        writer.write_batch(&rows_of(2, 2)).unwrap();
        // Row 2 overall (row 0 of this batch), column 1.
        let message = invalid_message(writer.write_batch(&Tuples::from(vec![vec![1, usize::MAX]])));
        assert!(message.starts_with("row 2, column 1:"), "{message}");
        assert!(message.contains("exceeds i64::MAX"), "{message}");
    }
}
