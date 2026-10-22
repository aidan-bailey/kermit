//! SNAP community graphs (Yang & Leskovec, ICDM 2012) as a benchmark.
//!
//! [`parse_edge_list`] and [`parse_gz`] turn a SNAP edge list into the one
//! symmetric `edge(src, dst)` relation every SNAP benchmark has. Comments are
//! skipped; a self-loop and a repeat of an unordered pair are dropped and
//! counted; each kept line `s d` contributes the rows `(s, d)` and `(d, s)`,
//! with the raw ids. A malformed line is an error that names where it is
//! (#78), never a skipped line.

mod counts;
pub use counts::expected_counts;
use {
    crate::{
        generator::{Generated, Generator},
        sink::RelationSink,
    },
    anyhow::{bail, Context},
    flate2::read::MultiGzDecoder,
    kermit_bench::{
        cache::{download_verified, sha256_hex},
        QueryDefinition, SnapGraph, SnapQuery,
    },
    kermit_ds::{RelationHeader, Tuples},
    kermit_rdf::generator::Target,
    serde::{Deserialize, Serialize},
    std::{
        collections::HashSet,
        fs::{self, File},
        io::{BufRead, BufReader},
        path::{Path, PathBuf},
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

/// The Datalog rule for `query` (the spec's § Workload, verbatim).
///
/// `cycle4` closes with `edge(A, D)`, not `edge(D, A)`. Both run, and on a
/// symmetric relation the two hold the same tuples, but when the plan puts
/// `A` before `D`, `edge(D, A)` needs a second, reordered index of `edge`.
pub fn rule(query: SnapQuery) -> &'static str {
    match query {
        | SnapQuery::Triangle => "T(A, B, C) :- edge(A, B), edge(B, C), edge(A, C).",
        | SnapQuery::Path3 => "P(A, B, C, D) :- edge(A, B), edge(B, C), edge(C, D).",
        | SnapQuery::Cycle4 => "C(A, B, C, D) :- edge(A, B), edge(B, C), edge(C, D), edge(A, D).",
        | SnapQuery::Clique4 => {
            "K(A, B, C, D) :- edge(A, B), edge(A, C), edge(A, D), edge(B, C), edge(B, D), edge(C, \
             D)."
        },
    }
}

/// What a query's `expected` counts, for `benchmark.yml`.
fn query_description(query: SnapQuery) -> &'static str {
    match query {
        | SnapQuery::Triangle => "6 × triangles",
        | SnapQuery::Path3 => "3-step walks (vertices may repeat)",
        | SnapQuery::Cycle4 => "closed 4-step walks, tr(A⁴): not 8 × 4-cycles",
        | SnapQuery::Clique4 => "24 × 4-cliques",
    }
}

/// Where a graph's `.gz` comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SnapSource {
    /// A local `.gz` (tests).
    Local(PathBuf),
    /// A URL and its pinned SHA-256, fetched with
    /// `kermit_bench::cache::download_verified`.
    Remote {
        /// Where to download the `.gz` from.
        url: String,
        /// The `.gz`'s SHA-256, 64 lowercase hex characters.
        sha256: String,
    },
}

impl SnapSource {
    /// A named graph's pinned download.
    pub fn named(graph: SnapGraph) -> Self {
        let (url, sha256) = graph.source();
        Self::Remote {
            url: url.to_string(),
            sha256: sha256.to_string(),
        }
    }
}

/// SNAP's own `meta.json` fields, flattened beside the common ones.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapExtras {
    /// The named graph, or `None` for a URL or a local file.
    pub graph: Option<SnapGraph>,
    /// The URL the `.gz` came from; `None` for a local source.
    pub source_url: Option<String>,
    /// The SHA-256 of the `.gz` read, whichever its source.
    pub gz_sha256: String,
    /// Data lines read (comments excluded).
    pub input_edges: u64,
    /// Lines with the same source and destination, dropped.
    pub dropped_self_loops: u64,
    /// Lines repeating an unordered pair seen before, dropped.
    pub dropped_duplicates: u64,
}

/// Generates a SNAP benchmark: one symmetric `edge` relation and up to four
/// queries, each with the exact count the oracle computed.
#[derive(Debug)]
pub struct SnapGenerator<'a> {
    target: Target<'a>,
    /// `Some` for a named graph, recorded in `meta.json`.
    graph: Option<SnapGraph>,
    source: SnapSource,
    /// Already defaulted to all four when the caller gave none.
    queries: Vec<SnapQuery>,
}

impl<'a> SnapGenerator<'a> {
    /// `queries: None` means [`SnapQuery::ALL`].
    pub fn new(
        target: Target<'a>, graph: Option<SnapGraph>, source: SnapSource,
        queries: Option<&[SnapQuery]>,
    ) -> Self {
        Self {
            target,
            graph,
            source,
            queries: queries.map_or_else(|| SnapQuery::ALL.to_vec(), <[SnapQuery]>::to_vec),
        }
    }

    /// What the benchmark's description calls the graph: its name, else the
    /// stem of its URL or file name.
    fn label(&self) -> &str {
        if let Some(graph) = self.graph {
            return graph.name();
        }
        let stem = match &self.source {
            | SnapSource::Local(path) => path.to_str().and_then(url_stem),
            | SnapSource::Remote {
                url, ..
            } => url_stem(url),
        };
        stem.unwrap_or("edge list")
    }
}

impl Generator for SnapGenerator<'_> {
    type Extras = SnapExtras;

    const KIND: &'static str = "snap";

    fn target(&self) -> Target<'_> { self.target }

    fn generate(&self, sink: &mut RelationSink) -> anyhow::Result<Generated<SnapExtras>> {
        let raw = sink.raw_dir()?;
        let (gz, source_url) = match &self.source {
            | SnapSource::Local(path) => {
                let Some(name) = path.file_name() else {
                    bail!("{} has no file name", path.display());
                };
                let dest = raw.join(name);
                fs::copy(path, &dest)
                    .with_context(|| format!("copying {} to {}", path.display(), dest.display()))?;
                (dest, None)
            },
            | SnapSource::Remote {
                url,
                sha256,
            } => {
                let name = last_segment(url);
                if matches!(name, "" | "." | "..") {
                    bail!("{url} does not end in a file name");
                }
                let dest = raw.join(name);
                download_verified(url, &dest, sha256)
                    .with_context(|| format!("downloading {url}"))?;
                (dest, Some(url.clone()))
            },
        };
        let gz_sha256 = sha256_hex(&gz)?;

        let parsed = parse_gz(&gz)?;
        sink.write(&edge_header(), &parsed.tuples)?;

        let counts = expected_counts(&parsed.tuples, &self.queries)?;
        let queries = self
            .queries
            .iter()
            .zip(counts)
            .map(|(&query, count)| QueryDefinition {
                name: query.name().to_string(),
                description: query_description(query).to_string(),
                query: rule(query).to_string(),
                expected: Some(count),
            })
            .collect();

        Ok(Generated {
            description: format!(
                "SNAP {}: one symmetric `edge` relation; queries count walks, not simple paths or \
                 cycles",
                self.label()
            ),
            queries,
            extras: SnapExtras {
                graph: self.graph,
                source_url,
                gz_sha256,
                input_edges: parsed.input_edges,
                dropped_self_loops: parsed.self_loops,
                dropped_duplicates: parsed.duplicates,
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::run::run,
        flate2::{write::GzEncoder, Compression},
        kermit_bench::{BenchError, BenchmarkDefinition},
        kermit_ds::read_parquet,
        kermit_parser::{JoinQuery, Term},
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

    /// A header, a triangle with a tail (1-2-3, 3-4, 4-5), a 4-clique (6-9), a
    /// self-loop (`5 5`), a reversed repeat (`4 3`) and an exact repeat
    /// (`6 7`). Data lines: 14; kept edges: 11, so 22 rows.
    ///
    /// By hand, as the sum over its two components (the triangle with a tail,
    /// then K4): triangle 6 + 24 = 30; path3 48 + 108 = 156; cycle4
    /// 34 + 84 = 118 (8 * C4 + 2 * sum d^2 - 2m, with no 4-cycle in the first
    /// component and three in K4); clique4 0 + 24 = 24.
    const MINI: &str = "\
# mini SNAP graph
# Nodes: 9
1\t2
2\t3
1\t3
3\t4
4\t3
5\t5
4\t5
6\t7
6\t8
6\t9
7\t8
7\t9
8\t9
6\t7
";

    const MINI_COUNTS: [u64; 4] = [30, 156, 118, 24];

    /// `<root>/snap-mini` is the output directory the tests give `run`.
    fn target(out: &Path) -> Target<'_> {
        Target {
            out_dir: out,
            bench_name: "snap-mini",
            tag: "t",
            spec_hash: None,
        }
    }

    /// Gzips `text` into `<dir>/mini.ungraph.txt.gz`.
    fn gz_file(dir: &Path, text: &str) -> PathBuf {
        let path = dir.join("mini.ungraph.txt.gz");
        fs::write(&path, gzip(text)).unwrap();
        path
    }

    fn read_benchmark(out: &Path) -> BenchmarkDefinition {
        serde_yaml::from_str(&fs::read_to_string(out.join("benchmark.yml")).unwrap()).unwrap()
    }

    /// Serves `body` as `200 OK` to every connection on a loopback port and
    /// returns the URL of `file` there.
    fn serve(body: Vec<u8>, file: &str) -> String {
        use std::{io::Read, net::TcpListener};

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/{file}", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else {
                    return;
                };
                let mut request = Vec::new();
                let mut buf = [0u8; 1024];
                while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                    match stream.read(&mut buf) {
                        | Ok(0) | Err(_) => break,
                        | Ok(n) => request.extend_from_slice(&buf[..n]),
                    }
                }
                let head = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream
                    .write_all(head.as_bytes())
                    .and_then(|()| stream.write_all(&body));
            }
        });
        url
    }

    #[test]
    fn rules_are_the_spec_queries() {
        let expected = [
            (
                SnapQuery::Triangle,
                "T(A, B, C) :- edge(A, B), edge(B, C), edge(A, C).",
                (3, 3),
            ),
            (
                SnapQuery::Path3,
                "P(A, B, C, D) :- edge(A, B), edge(B, C), edge(C, D).",
                (4, 3),
            ),
            (
                SnapQuery::Cycle4,
                "C(A, B, C, D) :- edge(A, B), edge(B, C), edge(C, D), edge(A, D).",
                (4, 4),
            ),
            (
                SnapQuery::Clique4,
                "K(A, B, C, D) :- edge(A, B), edge(A, C), edge(A, D), edge(B, C), edge(B, D), \
                 edge(C, D).",
                (4, 6),
            ),
        ];
        for (query, text, (head_terms, atoms)) in expected {
            assert_eq!(rule(query), text, "{}", query.name());
            let parsed: JoinQuery = rule(query).parse().unwrap();
            assert_eq!(parsed.head.terms.len(), head_terms, "{}", query.name());
            assert_eq!(parsed.body.len(), atoms, "{}", query.name());
            assert!(
                parsed
                    .body
                    .iter()
                    .all(|p| p.name == EDGE && p.terms.len() == 2),
                "{} must name only the binary relation `edge`",
                query.name()
            );
        }
        // Decision 6: the cycle closes with `edge(A, D)`, not `edge(D, A)`.
        let cycle: JoinQuery = rule(SnapQuery::Cycle4).parse().unwrap();
        assert_eq!(cycle.body[3].terms, [
            Term::Var("A".to_string()),
            Term::Var("D".to_string())
        ]);
    }

    #[test]
    fn a_named_source_is_the_graphs_pinned_pair() {
        for graph in SnapGraph::ALL {
            let (url, sha256) = graph.source();
            assert_eq!(SnapSource::named(graph), SnapSource::Remote {
                url: url.to_string(),
                sha256: sha256.to_string(),
            });
        }
    }

    #[test]
    fn a_local_fixture_generates_a_cache() {
        let root = tempfile::tempdir().unwrap();
        let gz = gz_file(root.path(), MINI);
        let out = root.path().join("snap-mini");

        let generator = SnapGenerator::new(target(&out), None, SnapSource::Local(gz.clone()), None);
        let meta = run(&generator).unwrap();

        assert_eq!(meta.kind, "snap");
        assert_eq!(meta.extras, SnapExtras {
            graph: None,
            source_url: None,
            gz_sha256: sha256_hex(&gz).unwrap(),
            input_edges: 14,
            dropped_self_loops: 1,
            dropped_duplicates: 2,
        });
        assert_eq!(
            sha256_hex(&out.join("raw").join("mini.ungraph.txt.gz")).unwrap(),
            meta.extras.gz_sha256,
            "the raw copy must be the input, byte for byte"
        );

        assert_eq!(meta.relations.len(), 1);
        assert_eq!(meta.relations[0].name, "edge");
        assert_eq!(meta.relations[0].columns, ["src", "dst"]);
        assert_eq!(meta.relations[0].rows, 22);
        let (header, tuples) = read_parquet(out.join("edge.parquet")).unwrap();
        assert_eq!(header, edge_header());
        assert_eq!(tuples, parse_gz(&gz).unwrap().tuples);
        assert_eq!(tuples.to_vecs()[..4], [
            vec![1, 2],
            vec![2, 1],
            vec![2, 3],
            vec![3, 2]
        ]);

        let bench = read_benchmark(&out);
        assert_eq!(bench.name, "snap-mini");
        assert!(
            bench.description.starts_with("SNAP mini: "),
            "{}",
            bench.description
        );
        let got: Vec<_> = bench
            .queries
            .iter()
            .map(|q| (q.name.as_str(), q.query.as_str(), q.expected))
            .collect();
        let want: Vec<_> = SnapQuery::ALL
            .iter()
            .zip(MINI_COUNTS)
            .map(|(q, count)| (q.name(), rule(*q), Some(count)))
            .collect();
        assert_eq!(got, want);
        let descriptions: Vec<_> = bench
            .queries
            .iter()
            .map(|q| q.description.as_str())
            .collect();
        assert_eq!(descriptions, [
            "6 × triangles",
            "3-step walks (vertices may repeat)",
            "closed 4-step walks, tr(A⁴): not 8 × 4-cycles",
            "24 × 4-cliques"
        ]);
    }

    #[test]
    fn a_named_graph_is_recorded_and_names_the_benchmark() {
        let root = tempfile::tempdir().unwrap();
        let gz = gz_file(root.path(), MINI);
        let out = root.path().join("snap-mini");
        let generator = SnapGenerator::new(
            target(&out),
            Some(SnapGraph::ComDblp),
            SnapSource::Local(gz),
            None,
        );
        let meta = run(&generator).unwrap();
        assert_eq!(meta.extras.graph, Some(SnapGraph::ComDblp));
        let description = read_benchmark(&out).description;
        assert!(description.starts_with("SNAP com-dblp: "), "{description}");
    }

    #[test]
    fn a_query_subset_keeps_its_order() {
        let root = tempfile::tempdir().unwrap();
        let gz = gz_file(root.path(), MINI);
        let out = root.path().join("snap-mini");
        let generator = SnapGenerator::new(
            target(&out),
            None,
            SnapSource::Local(gz),
            Some(&[SnapQuery::Clique4, SnapQuery::Triangle]),
        );
        let meta = run(&generator).unwrap();
        assert_eq!(meta.query_count, 2);
        let got: Vec<_> = read_benchmark(&out)
            .queries
            .into_iter()
            .map(|q| (q.name, q.expected))
            .collect();
        assert_eq!(got, [
            ("clique4".to_string(), Some(24)),
            ("triangle".to_string(), Some(30))
        ]);
    }

    #[test]
    fn an_edgeless_input_fails_without_a_commit_marker() {
        let root = tempfile::tempdir().unwrap();
        let gz = gz_file(root.path(), "# only a header\n3\t3\n");
        let out = root.path().join("snap-mini");
        let generator = SnapGenerator::new(target(&out), None, SnapSource::Local(gz), None);
        let err = format!("{:#}", run(&generator).unwrap_err());
        assert!(
            err.contains("has no edges after dropping 1 self-loops and 0 duplicates"),
            "{err}"
        );
        assert!(!out.join("meta.json").exists());
        assert!(!out.join("edge.parquet").exists());
    }

    #[test]
    fn a_remote_source_is_fetched_into_the_raw_directory() {
        let root = tempfile::tempdir().unwrap();
        let gz = gz_file(root.path(), MINI);
        let sha256 = sha256_hex(&gz).unwrap();
        let url = serve(fs::read(&gz).unwrap(), "mini.ungraph.txt.gz");
        let out = root.path().join("snap-mini");
        let source = SnapSource::Remote {
            url: url.clone(),
            sha256: sha256.clone(),
        };
        let meta = run(&SnapGenerator::new(target(&out), None, source, None)).unwrap();
        assert_eq!(meta.extras.source_url, Some(url));
        assert_eq!(meta.extras.gz_sha256, sha256);
        assert_eq!(meta.extras.input_edges, 14);
        assert_eq!(
            sha256_hex(&out.join("raw").join("mini.ungraph.txt.gz")).unwrap(),
            sha256
        );
    }

    #[test]
    fn a_wrong_digest_fails_before_parsing_and_commits_nothing() {
        let root = tempfile::tempdir().unwrap();
        let gz = gz_file(root.path(), MINI);
        let url = serve(fs::read(&gz).unwrap(), "mini.ungraph.txt.gz");
        let out = root.path().join("snap-mini");
        let source = SnapSource::Remote {
            url,
            sha256: "0".repeat(64),
        };
        let err = run(&SnapGenerator::new(target(&out), None, source, None)).unwrap_err();
        let message = format!("{err:#}");
        assert!(message.contains("integrity check failed"), "{message}");
        assert!(!out.join("raw").join("mini.ungraph.txt.gz").exists());
        assert!(!out.join("edge.parquet").exists());
        assert!(!out.join("meta.json").exists());
    }

    /// Downloads com-amazon and com-dblp (8.6 MB) and checks the eight counts
    /// verified independently with scipy and a direct 4-clique count
    /// (2026-10-05; spec § Workload). Needs the network: a download that fails
    /// without an HTTP status (connection, DNS, timeout) skips with a note on
    /// stderr. Anything else fails, a dead SNAP URL (a 4xx or 5xx status) and
    /// a digest mismatch included.
    #[test]
    fn reference_counts_match_the_verified_values() {
        let cases = [
            (SnapGraph::ComAmazon, 925_872, [
                4_002_774,
                206_831_062,
                65_863_072,
                6_623_064,
            ]),
            (SnapGraph::ComDblp, 1_049_866, [
                13_346_310,
                1_453_845_122,
                530_084_528,
                401_116_608,
            ]),
        ];
        let dir = tempfile::tempdir().unwrap();
        for (graph, input_edges, expected) in cases {
            let SnapSource::Remote {
                url,
                sha256,
            } = SnapSource::named(graph)
            else {
                panic!("{} has no pinned download", graph.name());
            };
            let dest = dir.path().join(format!("{}.ungraph.txt.gz", graph.name()));
            match download_verified(&url, &dest, &sha256) {
                | Ok(()) => {},
                | Err(BenchError::Download {
                    source, ..
                }) if source.status().is_none() => {
                    eprintln!("skipping the SNAP reference counts: {url} is unreachable: {source}");
                    return;
                },
                | Err(other) => panic!("downloading {url}: {other}"),
            }
            let parsed = parse_gz(&dest).unwrap();
            assert_eq!(parsed.input_edges, input_edges, "{}", graph.name());
            assert_eq!(
                (parsed.self_loops, parsed.duplicates),
                (0, 0),
                "{}",
                graph.name()
            );
            let counts = expected_counts(&parsed.tuples, &SnapQuery::ALL).unwrap();
            assert_eq!(counts, expected, "{}", graph.name());
        }
    }
}
