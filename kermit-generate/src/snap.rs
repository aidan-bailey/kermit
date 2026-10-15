//! SNAP community graphs (Yang & Leskovec, ICDM 2012) as a benchmark.
//!
//! [`parse_edge_list`] and [`parse_gz`] turn a SNAP edge list into the one
//! symmetric `edge(src, dst)` relation every SNAP benchmark has. Comments are
//! skipped; a self-loop and a repeat of an unordered pair are dropped and
//! counted; each kept line `s d` contributes the rows `(s, d)` and `(d, s)`,
//! with the raw ids. A malformed line is an error that names where it is
//! (#78), never a skipped line.

mod counts;

use {
    anyhow::{bail, Context},
    flate2::read::MultiGzDecoder,
    kermit_ds::{RelationHeader, Tuples},
    std::{
        collections::HashSet,
        fs::File,
        io::{BufRead, BufReader},
        path::Path,
    },
};

/// The one relation every SNAP benchmark has.
pub const EDGE: &str = "edge";

/// `edge(src, dst)`.
pub fn edge_header() -> RelationHeader {
    RelationHeader::new(EDGE, vec!["src".to_string(), "dst".to_string()])
}

/// A parsed, cleaned and symmetrised SNAP edge list.
#[derive(Debug)]
pub struct ParsedEdges {
    /// Arity 2: each kept line's (s, d) then (d, s), in file order.
    pub tuples: Tuples,
    /// Data lines read (comments excluded).
    pub input_edges: u64,
    /// Lines with s == d, dropped.
    pub self_loops: u64,
    /// Lines whose unordered pair had appeared before, dropped (a reversed
    /// repeat included).
    pub duplicates: u64,
}

/// Parses a SNAP edge list. `source` names the input in errors (a path or
/// URL).
///
/// A line starting with `#` is a comment. Any other line must be exactly two
/// whitespace-separated unsigned integers; a self-loop (s == d) and a repeat
/// of an unordered pair are dropped and counted, and each kept line pushes
/// (s, d) then (d, s).
///
/// # Errors
///
/// A line that is not a comment and not two unsigned integers (a blank line
/// included) is an error naming its 1-based line number, comments counted,
/// and its text. So is a list with no line left after cleaning, and an
/// unreadable input.
pub fn parse_edge_list(reader: impl BufRead, source: &str) -> anyhow::Result<ParsedEdges> {
    let mut tuples = Tuples::new(2);
    let mut seen: HashSet<(usize, usize)> = HashSet::new();
    let (mut input_edges, mut self_loops, mut duplicates) = (0u64, 0u64, 0u64);
    for (index, line) in reader.lines().enumerate() {
        let number = index + 1;
        let line = line.with_context(|| format!("{source}:{number}: unreadable line"))?;
        if line.starts_with('#') {
            continue;
        }
        let Some((s, d)) = parse_edge_line(&line) else {
            bail!(
                "{source}:{number}: malformed edge line {line:?}: expected two unsigned integers"
            );
        };
        input_edges += 1;
        if s == d {
            self_loops += 1;
        } else if !seen.insert((s.min(d), s.max(d))) {
            duplicates += 1;
        } else if tuples.len() + 2 > kermit_ds::RowId::MAX as usize {
            // `Tuples::push` panics past `RowId::MAX` rows; a `--url` graph
            // can be that large, so report it instead.
            bail!(
                "{source}:{number}: more edges than one `Tuples` batch holds ({} rows)",
                kermit_ds::RowId::MAX
            );
        } else {
            tuples.push(&[s, d]);
            tuples.push(&[d, s]);
        }
    }
    if tuples.is_empty() {
        bail!(
            "{source} has no edges after dropping {self_loops} self-loops and {duplicates} \
             duplicates"
        );
    }
    tuples.shrink_to_fit();
    Ok(ParsedEdges {
        tuples,
        input_edges,
        self_loops,
        duplicates,
    })
}

/// The two ids on a data line, or `None` unless it is exactly two
/// whitespace-separated unsigned integers.
fn parse_edge_line(line: &str) -> Option<(usize, usize)> {
    let mut fields = line.split_whitespace();
    let (Some(s), Some(d), None) = (fields.next(), fields.next(), fields.next()) else {
        return None;
    };
    Some((parse_id(s)?, parse_id(d)?))
}

/// An unsigned decimal integer that fits `usize`. `str::parse` would also
/// accept a leading `+`, which SNAP files never contain.
fn parse_id(field: &str) -> Option<usize> {
    if field.bytes().all(|b| b.is_ascii_digit()) {
        field.parse().ok()
    } else {
        None
    }
}

/// [`parse_edge_list`] over a gzipped file (`flate2::read::MultiGzDecoder`,
/// so a file of several gzip members is read whole). Errors name `path`.
pub fn parse_gz(path: &Path) -> anyhow::Result<ParsedEdges> {
    let file = File::open(path).with_context(|| format!("opening {}", path.display()))?;
    parse_edge_list(
        BufReader::new(MultiGzDecoder::new(file)),
        &path.display().to_string(),
    )
}

/// The text after the last `/` of `url`; all of `url` if it has none.
fn last_segment(url: &str) -> &str { url.rsplit('/').next().unwrap_or(url) }

/// The benchmark-name stem of a URL: its last path segment up to the first `.`
/// (`…/com-orkut.ungraph.txt.gz` gives `com-orkut`). `None` if that is empty.
pub fn url_stem(url: &str) -> Option<&str> {
    let stem = last_segment(url).split('.').next()?;
    (!stem.is_empty()).then_some(stem)
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        flate2::{write::GzEncoder, Compression},
        std::{fs, io::Write},
    };

    fn parse(text: &str) -> anyhow::Result<ParsedEdges> {
        parse_edge_list(text.as_bytes(), "t.txt")
    }

    fn gzip(text: &str) -> Vec<u8> {
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(text.as_bytes()).unwrap();
        encoder.finish().unwrap()
    }

    #[test]
    fn edge_header_names_the_relation() {
        let header = edge_header();
        assert_eq!(header.name(), "edge");
        assert_eq!(header.attrs(), ["src", "dst"]);
        assert_eq!(header.arity(), 2);
    }

    #[test]
    fn comments_are_skipped() {
        let parsed = parse("# a header\n# Nodes: 3\n1\t2\n# in the middle\n2 3\n").unwrap();
        assert_eq!(parsed.input_edges, 2);
        assert_eq!((parsed.self_loops, parsed.duplicates), (0, 0));
        assert_eq!(parsed.tuples.to_vecs(), vec![
            vec![1, 2],
            vec![2, 1],
            vec![2, 3],
            vec![3, 2]
        ]);
    }

    #[test]
    fn a_malformed_line_names_its_number_and_text() {
        let cases: [(&str, usize, &str); 9] = [
            ("1\n", 1, "1"),
            ("# c\n1 2\n1 2 3\n", 3, "1 2 3"),
            ("a b\n", 1, "a b"),
            ("1 -2\n", 1, "1 -2"),
            ("1 2\n\n3 4\n", 2, ""),
            ("   \n", 1, "   "),
            ("+1 2\n", 1, "+1 2"),
            (
                "1 99999999999999999999999\n",
                1,
                "1 99999999999999999999999",
            ),
            ("1\t2\ttrailing\n", 1, "1\t2\ttrailing"),
        ];
        for (input, line, text) in cases {
            let err = parse(input).unwrap_err();
            assert_eq!(
                err.to_string(),
                format!(
                    "t.txt:{line}: malformed edge line {text:?}: expected two unsigned integers"
                ),
                "{input:?}"
            );
        }
    }

    #[test]
    fn self_loops_and_duplicates_are_dropped_and_counted() {
        // Three self-loops; one exact repeat (1 2) and one reversed repeat
        // (4 3). The counts differ so that swapping them is visible.
        let parsed = parse("5 5\n1 2\n1 2\n3 4\n4 3\n6 6\n7 7\n").unwrap();
        assert_eq!(parsed.input_edges, 7);
        assert_eq!(parsed.self_loops, 3);
        assert_eq!(parsed.duplicates, 2);
        assert_eq!(parsed.tuples.to_vecs(), vec![
            vec![1, 2],
            vec![2, 1],
            vec![3, 4],
            vec![4, 3]
        ]);
    }

    #[test]
    fn rows_are_symmetrised_in_file_order() {
        let parsed = parse("3 1\n1 2\n").unwrap();
        assert_eq!((parsed.tuples.arity(), parsed.tuples.len()), (2, 4));
        assert_eq!(parsed.tuples.to_vecs(), vec![
            vec![3, 1],
            vec![1, 3],
            vec![1, 2],
            vec![2, 1]
        ]);
    }

    #[test]
    fn an_edgeless_file_is_rejected_with_its_counts() {
        let err = parse("# h\n3 3\n4 4\n").unwrap_err();
        assert_eq!(
            err.to_string(),
            "t.txt has no edges after dropping 2 self-loops and 0 duplicates"
        );
        for empty in ["# only a header\n", ""] {
            let err = parse(empty).unwrap_err();
            assert_eq!(
                err.to_string(),
                "t.txt has no edges after dropping 0 self-loops and 0 duplicates",
                "{empty:?}"
            );
        }
    }

    #[test]
    fn parse_gz_reads_a_gzipped_list() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("g.ungraph.txt.gz");
        // Two gzip members back to back: only a multi-member decoder reads
        // the second.
        let mut bytes = gzip("# header\n1\t2\n2\t3\n");
        bytes.extend(gzip("3\t4\n"));
        fs::write(&path, bytes).unwrap();
        let parsed = parse_gz(&path).unwrap();
        assert_eq!(parsed.input_edges, 3);
        assert_eq!(parsed.tuples.to_vecs(), vec![
            vec![1, 2],
            vec![2, 1],
            vec![2, 3],
            vec![3, 2],
            vec![3, 4],
            vec![4, 3]
        ]);
    }

    #[test]
    fn parse_gz_errors_name_the_path() {
        let dir = tempfile::tempdir().unwrap();
        let plain = dir.path().join("plain.gz");
        fs::write(&plain, "1 2\n").unwrap();
        let err = format!("{:#}", parse_gz(&plain).unwrap_err());
        assert!(err.contains(&plain.display().to_string()), "{err}");
        // The decoder failed, not a silently empty stream ("has no edges").
        assert!(err.contains("unreadable"), "{err}");

        let missing = dir.path().join("missing.gz");
        let err = format!("{:#}", parse_gz(&missing).unwrap_err());
        assert!(err.contains("opening"), "{err}");
        assert!(err.contains(&missing.display().to_string()), "{err}");
    }

    #[test]
    fn url_stem_takes_the_last_segment_up_to_its_first_dot() {
        let orkut = "https://snap.stanford.edu/data/bigdata/communities/com-orkut.ungraph.txt.gz";
        assert_eq!(url_stem(orkut), Some("com-orkut"));
        assert_eq!(url_stem("http://h/a.b/c"), Some("c"));
        assert_eq!(url_stem("http://h/x"), Some("x"));
        assert_eq!(url_stem("com-orkut"), Some("com-orkut"));
        assert_eq!(url_stem("http://h/"), None);
        assert_eq!(url_stem("http://h/.gz"), None);
        assert_eq!(url_stem(""), None);
    }
}
