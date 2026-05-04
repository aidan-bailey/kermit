//! Reads watdiv `.desc` cardinality sidecars and emits one CSV per query.
//!
//! The vendored watdiv binary used by this crate does not emit `.desc`
//! sidecars, so [`write_expected_csvs`] is effectively a no-op for it
//! (returns 0). The function is kept for forward compatibility with a
//! future binary or external sidecar source that does provide them.

use {
    crate::error::RdfError,
    std::{
        io::Write,
        path::{Path, PathBuf},
    },
};

/// Reads a `.desc` file (one integer per non-blank line) and returns the
/// list.
pub fn parse_desc(desc_path: &Path) -> Result<Vec<u64>, RdfError> {
    let text = std::fs::read_to_string(desc_path)?;
    let mut out = Vec::new();
    for (lineno, raw) in text.lines().enumerate() {
        let stripped = raw.trim();
        if stripped.is_empty() {
            continue;
        }
        let n: u64 = stripped.parse().map_err(|_| {
            RdfError::Expected(format!(
                "{}:{}: not an integer: {stripped:?}",
                desc_path.display(),
                lineno + 1,
            ))
        })?;
        out.push(n);
    }
    Ok(out)
}

/// For each `sparql_path`, looks at its `.desc` sibling, reads the i-th
/// cardinality, and writes `<out_dir>/<stem>_q<i>.csv` containing a one-line
/// header `cardinality\n<N>\n`.
///
/// Returns the number of `.csv` files written.
pub fn write_expected_csvs(sparql_files: &[PathBuf], out_dir: &Path) -> Result<usize, RdfError> {
    std::fs::create_dir_all(out_dir)?;
    let mut written = 0;
    for sparql in sparql_files {
        let desc = sparql.with_extension("desc");
        if !desc.exists() {
            continue;
        }
        let nums = parse_desc(&desc)?;
        let stem = sparql.file_stem().and_then(|s| s.to_str()).unwrap_or("q");
        let stem = stem.replace('.', "-");
        for (i, n) in nums.iter().enumerate() {
            let qname = format!("{stem}_q{i:04}");
            let csv_path = out_dir.join(format!("{qname}.csv"));
            let mut f = std::fs::File::create(&csv_path)?;
            writeln!(f, "cardinality")?;
            writeln!(f, "{n}")?;
            written += 1;
        }
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use {super::*, std::io::Write};

    #[test]
    fn parses_desc_numbers() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        writeln!(f, "12").unwrap();
        writeln!(f, "0").unwrap();
        writeln!(f, "987").unwrap();
        let nums = parse_desc(f.path()).unwrap();
        assert_eq!(nums, vec![12, 0, 987]);
    }

    #[test]
    fn writes_one_csv_per_query() {
        let dir = tempfile::tempdir().unwrap();
        let sparql = dir.path().join("test1.sparql");
        std::fs::write(&sparql, "SELECT * WHERE { }\nSELECT * WHERE { }\n").unwrap();
        let desc = dir.path().join("test1.desc");
        std::fs::write(&desc, "5\n7\n").unwrap();
        let out_dir = dir.path().join("expected");
        let n = write_expected_csvs(&[sparql], &out_dir).unwrap();
        assert_eq!(n, 2);
        assert!(out_dir.join("test1_q0000.csv").exists());
        let content = std::fs::read_to_string(out_dir.join("test1_q0001.csv")).unwrap();
        assert!(content.contains("7"));
    }

    #[test]
    fn missing_desc_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let sparql = dir.path().join("test1.sparql");
        std::fs::write(&sparql, "SELECT *").unwrap();
        let n = write_expected_csvs(&[sparql], &dir.path().join("expected")).unwrap();
        assert_eq!(n, 0);
    }
}
