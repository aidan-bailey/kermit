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
//! When `--report-json <path>` is set on the `bench` subcommand, the same
//! metadata plus pointers into Criterion's output directory are serialised
//! to `path` as a [`BenchReport`] (see [`write_json_report`]). External
//! tooling can then parse the JSON to correlate stderr metadata with
//! `target/criterion/{group}/{function}/` artefacts produced by Criterion.

use {
    serde::Serialize,
    std::io::{self, Write},
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
pub const REPORT_SCHEMA_VERSION: u32 = 1;

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
/// benchmark function. Together with `target/criterion/` this resolves to
/// `target/criterion/{group}/{function}/estimates.json` and friends.
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct CriterionGroupRef {
    /// Criterion `benchmark_group` name (first path segment under
    /// `target/criterion/`).
    pub group: String,
    /// Criterion `bench_function` id (second path segment).
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
    /// One entry per Criterion `bench_function` that ran. Multiple entries
    /// occur when a single subcommand records both time and space metrics or
    /// iterates several queries / relations.
    pub criterion_groups: Vec<CriterionGroupRef>,
}

impl BenchReport {
    /// Construct a report from the same `&[MetadataLine]` slice that
    /// [`write_metadata_block`] consumes, plus the list of Criterion
    /// functions that were run.
    pub fn new(
        kind: BenchKind, metadata: &[MetadataLine], criterion_groups: Vec<CriterionGroupRef>,
    ) -> Self {
        Self {
            schema_version: REPORT_SCHEMA_VERSION,
            kind,
            metadata: metadata.iter().map(ReportField::from).collect(),
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

#[cfg(test)]
mod tests {
    use super::*;

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
        let groups = vec![CriterionGroupRef {
            group: "ds".into(),
            function: "TreeTrie/space".into(),
            metric: ReportMetric::Space,
        }];
        let report = BenchReport::new(BenchKind::Ds, &metadata, groups);

        let mut buf = Vec::new();
        write_json_report(&mut buf, std::slice::from_ref(&report)).unwrap();
        let json: serde_json::Value = serde_json::from_slice(&buf).unwrap();
        assert!(json.is_array());
        assert_eq!(json[0]["schema_version"], 1);
        assert_eq!(json[0]["kind"], "ds");
        assert_eq!(json[0]["metadata"][0]["label"], "data structure");
        assert_eq!(json[0]["metadata"][0]["value"], "TreeTrie");
        assert_eq!(json[0]["criterion_groups"][0]["group"], "ds");
        assert_eq!(json[0]["criterion_groups"][0]["function"], "TreeTrie/space");
        assert_eq!(json[0]["criterion_groups"][0]["metric"], "space");
    }
}
