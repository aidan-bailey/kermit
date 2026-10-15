//! Writing a generator's relations: [`RelationSink`] and its records.

use {
    anyhow::{bail, Context},
    kermit_bench::cache::sha256_hex,
    kermit_ds::{ParquetWriter, RelationHeader, Tuples},
    serde::{Deserialize, Serialize},
    std::path::{Path, PathBuf},
};

/// What the sink recorded about one finished relation file. `benchmark.yml`
/// and `meta.json` are both built from these.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelationRecord {
    /// The relation's name, which is also its file stem.
    pub name: String,
    /// The attribute names, in column order.
    pub columns: Vec<String>,
    /// The number of rows written.
    pub rows: u64,
    /// The SHA-256 of `<name>.parquet`, in the encoding `bench fetch`
    /// compares against: 64 lowercase hex characters.
    pub sha256: String,
}

/// Where a generator writes its relations: `<out_dir>/<name>.parquet`,
/// one file per relation.
///
/// Each file is written by `kermit_ds::ParquetWriter`, so it appears under
/// its real name only when complete, and its bytes do not depend on how the
/// generator batched its rows. The sink hashes every finished file and keeps
/// a [`RelationRecord`] of it, in write order.
#[derive(Debug)]
pub struct RelationSink {
    out_dir: PathBuf,
    records: Vec<RelationRecord>,
}

impl RelationSink {
    /// An empty sink over `out_dir`, which must exist.
    // Used by `run`, which Task 3.3 adds; Task 3.3 removes this allow.
    #[allow(dead_code)]
    pub(crate) fn new(out_dir: &Path) -> Self {
        Self {
            out_dir: out_dir.to_path_buf(),
            records: Vec::new(),
        }
    }

    /// Starts the relation `header` names, to be filled batch by batch.
    ///
    /// Only one relation can be open at a time: the writer borrows the sink.
    ///
    /// # Errors
    ///
    /// Fails, before creating any file, if the name does not match
    /// `[a-z][a-z0-9_]*` or a relation of that name was already written. The
    /// lowercase rule is stricter than the Datalog parser's identifiers, and
    /// so excludes the reserved prefixes (`Const_`, `Select_`, `Index_`)
    /// too. It also fails if `ParquetWriter::create` does, e.g. for a
    /// positional header.
    pub fn open(&mut self, header: &RelationHeader) -> anyhow::Result<RelationWriter<'_>> {
        let name = header.name();
        check_relation_name(name)?;
        if self.records.iter().any(|record| record.name == name) {
            bail!("the relation {name:?} was already written");
        }
        let path = self.out_dir.join(format!("{name}.parquet"));
        let writer = ParquetWriter::create(&path, header)?;
        Ok(RelationWriter {
            sink: self,
            writer,
            header: header.clone(),
            path,
        })
    }

    /// Writes a whole relation: [`open`](Self::open), one batch, and
    /// [`finish`](RelationWriter::finish).
    pub fn write(&mut self, header: &RelationHeader, tuples: &Tuples) -> anyhow::Result<()> {
        let mut writer = self.open(header)?;
        writer.write_batch(tuples)?;
        writer.finish()
    }

    /// `<out_dir>/raw/`, for a generator's raw artifacts (a downloaded
    /// file, say). Created on the first call.
    pub fn raw_dir(&self) -> anyhow::Result<PathBuf> {
        let dir = self.out_dir.join("raw");
        std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
        Ok(dir)
    }

    /// The relations finished so far, in write order.
    pub fn records(&self) -> &[RelationRecord] { &self.records }

    /// The records, once the generator is done.
    // Used by `run`, which Task 3.3 adds; Task 3.3 removes this allow.
    #[allow(dead_code)]
    pub(crate) fn into_records(self) -> Vec<RelationRecord> { self.records }
}

/// A relation being written batch by batch; see [`RelationSink::open`].
///
/// A writer dropped without [`finish`](Self::finish) records nothing and
/// leaves no file behind.
pub struct RelationWriter<'a> {
    sink: &'a mut RelationSink,
    writer: ParquetWriter,
    header: RelationHeader,
    path: PathBuf,
}

impl RelationWriter<'_> {
    /// Appends `batch`'s rows. Batch boundaries do not show in the file,
    /// and an empty batch is a no-op at any arity.
    ///
    /// # Errors
    ///
    /// Fails as `ParquetWriter::write_batch` does: on a non-empty batch
    /// whose arity is not the header's, a value above `i64::MAX`, or more
    /// than `RowId::MAX` rows in total. After an error the writer refuses
    /// further batches.
    pub fn write_batch(&mut self, batch: &Tuples) -> anyhow::Result<()> {
        Ok(self.writer.write_batch(batch)?)
    }

    /// Finishes the file, hashes it and records it in the sink.
    pub fn finish(self) -> anyhow::Result<()> {
        let rows = self.writer.finish()?;
        let sha256 = sha256_hex(&self.path)?;
        self.sink.records.push(RelationRecord {
            name: self.header.name().to_string(),
            columns: self.header.attrs().to_vec(),
            rows,
            sha256,
        });
        Ok(())
    }
}

/// Checks `name` against `[a-z][a-z0-9_]*`, by hand: the crate has no regex
/// dependency.
fn check_relation_name(name: &str) -> anyhow::Result<()> {
    let mut chars = name.chars();
    let well_formed = chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    if !well_formed {
        bail!(
            "the relation name {name:?} must match [a-z][a-z0-9_]*, which the Datalog parser \
             accepts and no reserved prefix (`Const_`, `Select_`, `Index_`) starts"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use {super::*, kermit_ds::read_parquet, std::fs};

    fn header(name: &str) -> RelationHeader {
        RelationHeader::new(name, vec!["a".to_string(), "b".to_string()])
    }

    fn pairs() -> Tuples { Tuples::from(vec![vec![1, 2], vec![3, 4]]) }

    #[test]
    fn rejects_uppercase_and_reserved_names() {
        let dir = tempfile::tempdir().unwrap();
        let mut sink = RelationSink::new(dir.path());
        for name in ["Edge", "Const_x", "1x", "x-y", ""] {
            let Err(err) = sink.open(&header(name)) else {
                panic!("the name {name:?} was accepted");
            };
            assert!(err.to_string().contains("[a-z][a-z0-9_]*"), "{name:?}: {err}");
        }
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0, "a rejected name left a file");
        sink.write(&header("r2_d2"), &pairs()).unwrap();
        assert_eq!(sink.records().len(), 1);
    }

    #[test]
    fn rejects_duplicate_names() {
        let dir = tempfile::tempdir().unwrap();
        let mut sink = RelationSink::new(dir.path());
        sink.write(&header("edge"), &pairs()).unwrap();
        let file = dir.path().join("edge.parquet");
        let before = fs::read(&file).unwrap();
        let Err(err) = sink.open(&header("edge")) else {
            panic!("a second `edge` was accepted");
        };
        assert!(err.to_string().contains("already"), "{err}");
        assert_eq!(sink.records().len(), 1);
        assert_eq!(fs::read(&file).unwrap(), before, "the first file was touched");
    }

    #[test]
    fn record_hash_matches_file() {
        let dir = tempfile::tempdir().unwrap();
        let mut sink = RelationSink::new(dir.path());
        let (edge, rows) = (header("edge"), pairs());
        sink.write(&edge, &rows).unwrap();
        let file = dir.path().join("edge.parquet");
        assert_eq!(
            sink.records(),
            [RelationRecord {
                name: "edge".to_string(),
                columns: vec!["a".to_string(), "b".to_string()],
                rows: 2,
                sha256: sha256_hex(&file).unwrap(),
            }]
        );
        assert_eq!(read_parquet(&file).unwrap(), (edge, rows));
    }

    #[test]
    fn raw_dir_is_created_on_demand() {
        let dir = tempfile::tempdir().unwrap();
        let sink = RelationSink::new(dir.path());
        let raw = dir.path().join("raw");
        assert!(!raw.exists());
        assert_eq!(sink.raw_dir().unwrap(), raw);
        assert!(raw.is_dir());
        assert_eq!(sink.raw_dir().unwrap(), raw, "a second call must also succeed");
    }

    #[test]
    fn dropped_relation_writer_records_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let mut sink = RelationSink::new(dir.path());
        let mut writer = sink.open(&header("edge")).unwrap();
        writer.write_batch(&pairs()).unwrap();
        drop(writer);
        assert!(sink.records().is_empty());
        let left: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert!(left.is_empty(), "a dropped writer left {left:?}");
        sink.write(&header("edge"), &pairs()).unwrap();
        assert_eq!(sink.records().len(), 1, "the name must be free again");
    }
}
