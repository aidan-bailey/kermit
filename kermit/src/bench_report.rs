//! Unified formatting for the metadata blocks the `bench` subcommands emit
//! to stderr before invoking Criterion.
//!
//! Each subcommand builds a `&[MetadataLine]` of label/value pairs and hands
//! it to [`write_metadata_block`], which produces a header banner plus
//! left-aligned, column-padded entries. Centralising this here keeps the
//! three call sites (`bench join`, `bench ds`, `bench run`) consistent and
//! gives us a single place to integrate [`crate::measurement::format_bytes`]
//! for byte-valued fields.

use std::io::{self, Write};

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
}
