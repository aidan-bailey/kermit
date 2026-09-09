//! Unified formatting for the metadata blocks the `bench` subcommands emit
//! to stderr before invoking Criterion, plus the optional machine-readable
//! JSON report channel.
//!
//! Each subcommand builds a `&[MetadataLine]` of label/value pairs and hands
//! it to [`write_metadata_block`], which produces a header banner plus
//! left-aligned, column-padded entries. Centralising this here keeps the
//! three call sites (`bench join`, `bench ds`, `bench run`) consistent and
//! gives us a single place to integrate [`crate::measurement::format_bytes`]
//! for byte-valued fields.
//!
//! The report is written through [`ReportSink`], which rewrites it
//! atomically after every cell so an interrupted `bench run` sweep keeps
//! its finished cells.
//!
//! When `--report-json <path>` is set on the `bench` subcommand, the same
//! metadata plus pointers into Criterion's output directory are serialised
//! to `path` as a [`BenchReport`] (see [`write_json_report`]). External
//! tooling can then parse the JSON to correlate stderr metadata with the
//! Criterion artefacts under `target/criterion/`. Both `group` and `function`
//! are logical identities that may contain `/`; on disk Criterion flattens
//! `/`→`_` and nests the estimate files under a `new/` (or `base/`) subdir, so
//! resolve via each candidate subdir's `benchmark.json:directory_name` rather
//! than concatenating the strings — see
//! `docs/specs/bench-report-schema.md` §"Resolving a `CriterionGroupRef` to
//! filesystem paths".

use {
    serde::Serialize,
    std::{
        collections::BTreeMap,
        fs,
        io::{self, BufWriter, Write},
        path::{Path, PathBuf},
    },
};

/// One labelled line in a metadata block.
pub struct MetadataLine {
    /// Static label rendered before the colon (e.g. `"data structure"`).
    pub label: &'static str,
    /// Pre-formatted value rendered after the column padding.
    pub value: String,
}

impl MetadataLine {
    /// Build a `MetadataLine` whose value is produced by `Display`. For byte
    /// counts, compose with [`crate::measurement::format_bytes`]:
    /// `MetadataLine::new("heap size", format_bytes(n))`.
    pub fn new(label: &'static str, value: impl std::fmt::Display) -> Self {
        Self {
            label,
            value: value.to_string(),
        }
    }
}

/// Write a metadata block to `w`. The header is wrapped in `--- … ---` to
/// match the existing CLI layout, and label columns are padded so values
/// align in a single rectangle.
pub fn write_metadata_block<W: Write>(
    w: &mut W, header: &str, lines: &[MetadataLine],
) -> io::Result<()> {
    writeln!(w, "--- {header} ---")?;
    let label_width = lines.iter().map(|l| l.label.len()).max().unwrap_or(0);
    for line in lines {
        writeln!(
            w,
            "  {:<width$}  {}",
            format!("{}:", line.label),
            line.value,
            width = label_width + 1
        )?;
    }
    Ok(())
}

/// Schema version for the JSON report. Bump on any breaking change to
/// [`BenchReport`] field names or value types.
pub const REPORT_SCHEMA_VERSION: u32 = 2;

/// Which `bench` subcommand produced the report. Serialised as a lower-case
/// string (`"join"`, `"ds"`, `"run"`).
#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum BenchKind {
    /// `bench join`
    Join,
    /// `bench ds`
    Ds,
    /// `bench run`
    Run,
}

/// Which Criterion measurement axis a benchmark function records.
#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ReportMetric {
    /// Wall-clock time (Criterion's default `WallTime`).
    Time,
    /// Heap bytes via [`crate::measurement::SpaceMeasurement`].
    Space,
}

/// A single label/value pair as it appears in stderr metadata.
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct ReportField {
    /// Same string as [`MetadataLine::label`].
    pub label: String,
    /// Same string as [`MetadataLine::value`].
    pub value: String,
}

impl From<&MetadataLine> for ReportField {
    fn from(m: &MetadataLine) -> Self {
        Self {
            label: m.label.to_string(),
            value: m.value.clone(),
        }
    }
}

/// A pointer into the Criterion artefacts directory, identifying one
/// benchmark function. `group` and `function` are the logical Criterion
/// identities; both may contain `/`. On disk Criterion flattens `/`→`_` and
/// nests the estimate files under a `new/` (or `base/`) subdir, so resolve via
/// each candidate subdir's `benchmark.json:directory_name` rather than
/// concatenating these strings — see
/// `docs/specs/bench-report-schema.md` §"Resolving a `CriterionGroupRef` to
/// filesystem paths".
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct CriterionGroupRef {
    /// Criterion `benchmark_group` name.
    pub group: String,
    /// Criterion `bench_function` id.
    pub function: String,
    /// Which measurement axis this function records.
    pub metric: ReportMetric,
}

/// A complete report describing one bench-subcommand invocation.
#[derive(Serialize, Clone, Debug)]
pub struct BenchReport {
    /// Schema version; consumers should refuse to parse unknown majors.
    pub schema_version: u32,
    /// Which `bench` subcommand produced this report.
    pub kind: BenchKind,
    /// The metadata that was also written to stderr via
    /// [`write_metadata_block`].
    pub metadata: Vec<ReportField>,
    /// Structured axis values for downstream tooling (e.g. plot grouping).
    /// Conventional keys: `data_structure`, `algorithm`, `query`,
    /// `benchmark`, `relation_path`, `relation_bytes`, `tuples`, `arity`.
    /// `BTreeMap` so JSON output is deterministically ordered.
    pub axes: BTreeMap<String, serde_json::Value>,
    /// One entry per Criterion `bench_function` that ran. Multiple entries
    /// occur when a single subcommand records both time and space metrics or
    /// iterates several queries / relations.
    pub criterion_groups: Vec<CriterionGroupRef>,
}

impl BenchReport {
    /// Construct a report from the same `&[MetadataLine]` slice that
    /// [`write_metadata_block`] consumes, the structured `axes` map, and the
    /// list of Criterion functions that were run.
    pub fn new(
        kind: BenchKind, metadata: &[MetadataLine], axes: BTreeMap<String, serde_json::Value>,
        criterion_groups: Vec<CriterionGroupRef>,
    ) -> Self {
        Self {
            schema_version: REPORT_SCHEMA_VERSION,
            kind,
            metadata: metadata.iter().map(ReportField::from).collect(),
            axes,
            criterion_groups,
        }
    }
}

/// Serialise `reports` as a pretty JSON array to `w`. The output is always
/// an array, even for single-report subcommands (`bench join`, `bench ds`),
/// so consumers can use one parser shape across all subcommands.
pub fn write_json_report<W: Write>(w: &mut W, reports: &[BenchReport]) -> io::Result<()> {
    serde_json::to_writer_pretty(&mut *w, reports)?;
    writeln!(w)
}

/// Crash-safe writer for the JSON report of one `bench` invocation.
///
/// `bench run` produces reports one cell at a time, and a sweep can run
/// for hours. The sink rewrites the complete array after every
/// [`push`](Self::push) — to a `.part` sibling, then renamed into place —
/// so at any instant the file on disk is a valid JSON array holding every
/// cell that has finished. An interrupted sweep therefore keeps its
/// completed cells; only the cell in flight is lost.
///
/// `bench join` and `bench ds` produce a single report and use the same
/// sink so all three subcommands share one output path.
pub struct ReportSink {
    path: PathBuf,
    reports: Vec<BenchReport>,
}

impl ReportSink {
    /// Resolves the target path — `override_path` if given, otherwise
    /// [`default_path`](Self::default_path) — and creates its parent
    /// directory. Nothing is written until the first `push` or `finish`.
    pub fn open(override_path: Option<&Path>, kind: BenchKind) -> io::Result<Self> {
        let path = match override_path {
            | Some(p) => p.to_path_buf(),
            | None => Self::default_path(kind),
        };
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)?;
            }
        }
        Ok(Self {
            path,
            reports: Vec::new(),
        })
    }

    /// The path the report is written to.
    pub fn path(&self) -> &Path { &self.path }

    /// The default report path: `bench-runs/{kind}-{unix-millis}.json`
    /// relative to the current directory (`bench-runs/` is gitignored at
    /// the workspace root).
    pub fn default_path(kind: BenchKind) -> PathBuf {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let kind_str = match kind {
            | BenchKind::Join => "join",
            | BenchKind::Ds => "ds",
            | BenchKind::Run => "run",
        };
        PathBuf::from(format!("bench-runs/{kind_str}-{now_ms}.json"))
    }

    /// Appends `reports` and rewrites the whole file atomically.
    pub fn push(&mut self, reports: Vec<BenchReport>) -> io::Result<()> {
        self.reports.extend(reports);
        self.write_atomically()
    }

    /// Writes the file (even if nothing was pushed, so an empty sweep
    /// still yields `[]`), announces the path on stderr, and returns it.
    pub fn finish(self) -> io::Result<PathBuf> {
        self.write_atomically()?;
        eprintln!("Report written: {}", self.path.display());
        Ok(self.path)
    }

    /// Serialises to `<path>.part` and renames over `<path>`, the same
    /// staging discipline `kermit-bench`'s downloader uses, so a crash
    /// mid-write can never leave a truncated report.
    fn write_atomically(&self) -> io::Result<()> {
        let mut part = self.path.clone().into_os_string();
        part.push(".part");
        let part = PathBuf::from(part);
        {
            let mut writer = BufWriter::new(fs::File::create(&part)?);
            write_json_report(&mut writer, &self.reports)?;
            writer.flush()?;
        }
        fs::rename(&part, &self.path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The Python analysis layer keeps its own copy of the schema version.
    /// Embedding its module at compile time makes a mismatch a `cargo test`
    /// failure rather than a notebook surprise.
    const KERMIT_LAB_INIT: &str = include_str!("../../python/kermit-lab/kermit_lab/__init__.py");

    fn python_schema_version(source: &str) -> Option<u32> {
        source
            .lines()
            .find_map(|l| l.strip_prefix("SCHEMA_VERSION = "))
            .and_then(|v| v.trim().parse().ok())
    }

    #[test]
    fn python_schema_version_matches_rust() {
        assert_eq!(
            python_schema_version(KERMIT_LAB_INIT),
            Some(REPORT_SCHEMA_VERSION),
            "python/kermit-lab/kermit_lab/__init__.py SCHEMA_VERSION must equal \
             kermit/src/bench_report.rs REPORT_SCHEMA_VERSION; bump both together"
        );
    }

    #[test]
    fn python_schema_version_parser_rejects_a_file_without_the_line() {
        assert_eq!(python_schema_version("# nothing here\nX = 2\n"), None);
    }

    #[test]
    fn metadata_block_aligns_labels() {
        let lines = vec![
            MetadataLine::new("data structure", "TreeTrie"),
            MetadataLine::new("algorithm", "Leapfrog"),
            MetadataLine::new("relations", 2),
        ];
        let mut buf = Vec::new();
        write_metadata_block(&mut buf, "bench metadata", &lines).unwrap();
        let out = String::from_utf8(buf).unwrap();
        assert!(out.contains("--- bench metadata ---"));
        assert!(out.contains("data structure:"));
        assert!(out.contains("algorithm:"));
        assert!(out.contains("relations:"));

        // The longest label ("data structure") drives column width; shorter
        // labels are padded so the *value* column is identical across rows.
        // Each value row is `"  <padded-label>  <value>"`. Since all labels
        // pad to the same width and the indent + separator are constant,
        // verifying value placement is equivalent to verifying that each
        // line ends with two-space-then-value at the same byte offset.
        let body: Vec<&str> = out.lines().skip(1).collect();
        let positions: Vec<usize> = body
            .iter()
            .map(|l| l.rfind("  ").expect("expected indent before value"))
            .collect();
        assert!(
            positions.iter().all(|&p| p == positions[0]),
            "value columns should align: {body:?}"
        );
    }

    #[test]
    fn metadata_line_value_uses_display() {
        let line = MetadataLine::new("relations", 2);
        assert_eq!(line.label, "relations");
        assert_eq!(line.value, "2");
    }

    #[test]
    fn json_report_round_trips() {
        let metadata = vec![
            MetadataLine::new("data structure", "TreeTrie"),
            MetadataLine::new("relations", 2),
        ];
        let axes = std::collections::BTreeMap::from([
            ("data_structure".to_string(), serde_json::json!("TreeTrie")),
            ("tuples".to_string(), serde_json::json!(2)),
        ]);
        let groups = vec![CriterionGroupRef {
            group: "ds".into(),
            function: "TreeTrie/space".into(),
            metric: ReportMetric::Space,
        }];
        let report = BenchReport::new(BenchKind::Ds, &metadata, axes, groups);

        let mut buf = Vec::new();
        write_json_report(&mut buf, std::slice::from_ref(&report)).unwrap();
        let json: serde_json::Value = serde_json::from_slice(&buf).unwrap();
        assert!(json.is_array());
        assert_eq!(json[0]["schema_version"], 2);
        assert_eq!(json[0]["kind"], "ds");
        assert_eq!(json[0]["metadata"][0]["label"], "data structure");
        assert_eq!(json[0]["metadata"][0]["value"], "TreeTrie");
        assert_eq!(json[0]["axes"]["data_structure"], "TreeTrie");
        assert_eq!(json[0]["axes"]["tuples"], 2);
        assert_eq!(json[0]["criterion_groups"][0]["group"], "ds");
        assert_eq!(json[0]["criterion_groups"][0]["function"], "TreeTrie/space");
        assert_eq!(json[0]["criterion_groups"][0]["metric"], "space");
    }

    #[test]
    fn axes_preserve_value_types_and_order() {
        let axes = std::collections::BTreeMap::from([
            ("zeta".to_string(), serde_json::json!(true)),
            ("alpha".to_string(), serde_json::json!("hello")),
            ("middle".to_string(), serde_json::json!(42)),
            ("nested".to_string(), serde_json::json!({"k": [1, 2]})),
        ]);
        let report = BenchReport::new(BenchKind::Join, &[], axes, vec![]);

        let mut buf = Vec::new();
        write_json_report(&mut buf, std::slice::from_ref(&report)).unwrap();
        let s = String::from_utf8(buf).unwrap();

        // BTreeMap ordering => alphabetical key order in JSON for diff-friendly
        // output.
        let alpha_at = s.find("\"alpha\"").unwrap();
        let middle_at = s.find("\"middle\"").unwrap();
        let nested_at = s.find("\"nested\"").unwrap();
        let zeta_at = s.find("\"zeta\"").unwrap();
        assert!(alpha_at < middle_at);
        assert!(middle_at < nested_at);
        assert!(nested_at < zeta_at);

        // Value types are preserved (string/int/bool/object), not stringified.
        let json: serde_json::Value = serde_json::from_str(&s).unwrap();
        assert_eq!(json[0]["axes"]["alpha"], "hello");
        assert_eq!(json[0]["axes"]["middle"], 42);
        assert_eq!(json[0]["axes"]["zeta"], true);
        assert_eq!(json[0]["axes"]["nested"]["k"][1], 2);
    }

    fn report(kind: BenchKind, tag: &str) -> BenchReport {
        let axes = std::collections::BTreeMap::from([("tag".to_string(), serde_json::json!(tag))]);
        BenchReport::new(kind, &[], axes, vec![])
    }

    fn read_tags(path: &std::path::Path) -> Vec<String> {
        let text = std::fs::read_to_string(path).unwrap();
        let json: serde_json::Value = serde_json::from_str(&text).unwrap();
        json.as_array()
            .unwrap()
            .iter()
            .map(|r| r["axes"]["tag"].as_str().unwrap().to_string())
            .collect()
    }

    #[test]
    fn sink_finish_without_push_writes_empty_array() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested/run.json");
        let sink = ReportSink::open(Some(&path), BenchKind::Run).unwrap();
        let written = sink.finish().unwrap();
        assert_eq!(written, path);
        assert_eq!(read_tags(&path), Vec::<String>::new());
    }

    #[test]
    fn sink_push_is_visible_on_disk_before_finish() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("run.json");
        let mut sink = ReportSink::open(Some(&path), BenchKind::Run).unwrap();
        sink.push(vec![report(BenchKind::Run, "cell-1")]).unwrap();
        // The file already holds the first cell and no staging file remains.
        assert_eq!(read_tags(&path), vec!["cell-1"]);
        assert!(!dir.path().join("run.json.part").exists());
        sink.push(vec![
            report(BenchKind::Run, "cell-2a"),
            report(BenchKind::Run, "cell-2b"),
        ])
        .unwrap();
        assert_eq!(read_tags(&path), vec!["cell-1", "cell-2a", "cell-2b"]);
        sink.finish().unwrap();
        assert_eq!(read_tags(&path), vec!["cell-1", "cell-2a", "cell-2b"]);
    }

    #[test]
    fn sink_default_path_uses_kind_and_bench_runs_dir() {
        let path = ReportSink::default_path(BenchKind::Ds);
        assert_eq!(path.parent().unwrap(), std::path::Path::new("bench-runs"));
        let file = path.file_name().unwrap().to_str().unwrap();
        assert!(file.starts_with("ds-"), "{file}");
        assert!(file.ends_with(".json"), "{file}");
    }
}
