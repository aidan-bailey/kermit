//! Hand-crafted N-Triples + SPARQL through the shared post-driver orchestrator
//! (`kermit_rdf::generator::process_artifacts`), with no external tool.
//!
//! Every test reads one artifact back from disk and pins its *contents*, not
//! just its existence: the per-predicate Parquet tuples, the dictionary, the
//! full translated Datalog rules, and the emitted `benchmark.yml`. All
//! expected values below are derived by hand from [`NT`] and the pipeline's
//! documented rules:
//!
//! - **Dictionary** (`dict.rs`): every term is interned in stream order —
//!   subject, predicate, object per triple — so ids are dense from 0. The
//!   translator interns query-only constants afterwards, and the orchestrator
//!   writes `dict.parquet` after translation, so those ids follow the data's.
//! - **Partition** (`partition.rs`): one relation per predicate IRI in order of
//!   first appearance, tuples in input order. A sanitisation collision is
//!   resolved by suffixing `_<dict-id of the predicate IRI>` onto every name
//!   but the first.
//!
//! | id | canonical value                                     | first seen |
//! |----|-----------------------------------------------------|------------|
//! |  0 | `<http://x/alice>`                                  | t1 s       |
//! |  1 | `<http://x/follows>`                                | t1 p       |
//! |  2 | `<http://x/bob>`                                    | t1 o       |
//! |  3 | `<http://x/carol>`                                  | t2 o       |
//! |  4 | `<http://ogp.me/ns#title>`                          | t3 p       |
//! |  5 | `"Dr"@en`                                           | t3 o       |
//! |  6 | `<http://purl.org/stuff/rev#title>`                 | t5 p       |
//! |  7 | `_:review1`                                         | t5 o       |
//! |  8 | `<http://x/has-age>`                                | t6 p       |
//! |  9 | `"42"^^<http://www.w3.org/2001/XMLSchema#integer>`  | t6 o       |
//! | 10 | `"Mx"`                                              | t7 o       |
//! | 11 | `<http://x/dave>`                                   | query only |
//! | 12 | `<http://x/erin>`                                   | query only |

use {
    arrow::array::{Int64Array, StringArray},
    kermit_bench::BenchmarkDefinition,
    kermit_rdf::{
        dict::Dictionary,
        error::RdfError,
        generator::{
            process_artifacts, Generator, GeneratorMeta, Provenance, Target, TranslatedQuery,
        },
        partition::Partitioned,
        sparql::translator::translate_query,
    },
    parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder,
    serde::{Deserialize, Serialize},
    std::{
        collections::HashMap,
        fs,
        path::{Path, PathBuf},
    },
};

/// Seven triples over four predicate IRIs. `ogp.me/ns#title` and
/// `purl.org/stuff/rev#title` both sanitise to `title`; `has-age` sanitises to
/// `has_age`. Objects cover IRIs, a language-tagged literal, a blank node, a
/// typed literal and a plain literal.
const NT: &str = r#"<http://x/alice> <http://x/follows> <http://x/bob> .
<http://x/bob> <http://x/follows> <http://x/carol> .
<http://x/alice> <http://ogp.me/ns#title> "Dr"@en .
<http://x/carol> <http://x/follows> <http://x/alice> .
<http://x/bob> <http://purl.org/stuff/rev#title> _:review1 .
<http://x/carol> <http://x/has-age> "42"^^<http://www.w3.org/2001/XMLSchema#integer> .
<http://x/bob> <http://ogp.me/ns#title> "Mx" .
"#;

/// `(name, sparql, hand-counted answers over NT)`.
///
/// - `path`: two-hop `follows` cycle — (alice, bob, carol), (bob, carol,
///   alice), (carol, alice, bob).
/// - `titles`: explicit projection in a non-first-appearance order, both
///   colliding predicates, and a data constant (`carol`, id 3). Only `bob`
///   follows `carol`, with review `_:review1` and title `"Mx"`.
/// - `dave`: a constant absent from the data, in subject position (id 11).
/// - `strangers`: a second absent constant (`erin`, id 12) before a repeat of
///   `dave`, which must reuse id 11.
const QUERIES: &[(&str, &str, u64)] = &[
    (
        "path",
        "SELECT * WHERE { ?x <http://x/follows> ?y . ?y <http://x/follows> ?z . }",
        3,
    ),
    (
        "titles",
        "SELECT ?t ?n ?x WHERE { ?x <http://x/follows> <http://x/carol> . \
         ?x <http://purl.org/stuff/rev#title> ?t . ?x <http://ogp.me/ns#title> ?n . }",
        1,
    ),
    (
        "dave",
        "SELECT ?y WHERE { <http://x/dave> <http://x/follows> ?y . }",
        0,
    ),
    (
        "strangers",
        "SELECT ?x WHERE { ?x <http://x/follows> <http://x/erin> . \
         <http://x/dave> <http://x/follows> ?x . }",
        0,
    ),
];

const BENCH_NAME: &str = "handcrafted-bench";
const DESCRIPTION: &str = "hand-crafted pipeline fixture";

/// A generator whose "driver output" is the in-memory [`NT`] text.
struct HandcraftedGenerator<'a> {
    out_dir: &'a Path,
}

#[derive(Debug, Serialize, Deserialize)]
struct HandcraftedMeta {
    schema_version: u32,
    kind: String,
    tag: String,
    generated_at_utc: String,
    relation_count: u32,
    query_count: u32,
}

impl GeneratorMeta for HandcraftedMeta {
    fn schema_version(&self) -> u32 { self.schema_version }

    fn kind(&self) -> &str { &self.kind }

    fn tag(&self) -> &str { &self.tag }

    fn generated_at_utc(&self) -> &str { &self.generated_at_utc }

    fn relation_count(&self) -> u32 { self.relation_count }

    fn query_count(&self) -> u32 { self.query_count }

    fn spec_hash(&self) -> Option<&str> { None }
}

impl Generator for HandcraftedGenerator<'_> {
    type Meta = HandcraftedMeta;
    type Raw = ();
    type Staged = ();

    fn target(&self) -> Target<'_> {
        Target {
            out_dir: self.out_dir,
            bench_name: BENCH_NAME,
            tag: "handcrafted",
            spec_hash: None,
        }
    }

    fn stage_raw(&self, _raw: &(), raw_root: &Path) -> Result<(PathBuf, ()), RdfError> {
        let nt = raw_root.join("data.nt");
        fs::write(&nt, NT)?;
        Ok((nt, ()))
    }

    fn translate_queries(
        &self, _staged: &(), dict: &mut Dictionary, predicate_map: &HashMap<String, String>,
    ) -> Result<Vec<TranslatedQuery>, RdfError> {
        QUERIES
            .iter()
            .map(|(name, sparql, expected)| {
                Ok(TranslatedQuery {
                    name: (*name).to_string(),
                    datalog: translate_query(sparql, dict, predicate_map, &format!("Q_{name}"))?,
                    expected: Some(*expected),
                })
            })
            .collect()
    }

    fn description(&self, _raw: &()) -> String { DESCRIPTION.to_string() }

    fn build_meta(
        &self, _raw: &(), _staged: &(), _part: &Partitioned, provenance: Provenance,
    ) -> Result<HandcraftedMeta, RdfError> {
        Ok(HandcraftedMeta {
            schema_version: provenance.schema_version,
            kind: "handcrafted".to_string(),
            tag: provenance.tag,
            generated_at_utc: provenance.generated_at_utc,
            relation_count: provenance.relation_count,
            query_count: provenance.query_count,
        })
    }
}

/// Runs the full post-driver sequence into a fresh temp dir.
fn generate() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    process_artifacts(
        &HandcraftedGenerator {
            out_dir: dir.path(),
        },
        &(),
    )
    .expect("hand-crafted pipeline succeeds");
    dir
}

/// Reads a whole Parquet file, asserting its column names (in order) first.
fn read_batches(path: &Path, columns: &[&str]) -> Vec<arrow::record_batch::RecordBatch> {
    let file = fs::File::open(path).unwrap_or_else(|e| panic!("open {path:?}: {e}"));
    let builder = ParquetRecordBatchReaderBuilder::try_new(file).unwrap();
    let names: Vec<&str> = builder
        .schema()
        .fields()
        .iter()
        .map(|f| f.name().as_str())
        .collect();
    assert_eq!(names, columns, "column layout of {path:?}");
    builder.build().unwrap().map(|b| b.unwrap()).collect()
}

/// The `(s, o)` rows of a relation file, in file order.
fn read_relation(path: &Path) -> Vec<(i64, i64)> {
    let mut rows = Vec::new();
    for batch in read_batches(path, &["s", "o"]) {
        let s = batch
            .column(0)
            .as_any()
            .downcast_ref::<Int64Array>()
            .unwrap();
        let o = batch
            .column(1)
            .as_any()
            .downcast_ref::<Int64Array>()
            .unwrap();
        rows.extend(
            s.iter()
                .zip(o.iter())
                .map(|(s, o)| (s.unwrap(), o.unwrap())),
        );
    }
    rows
}

/// The `(id, value)` rows of `dict.parquet`, in file order.
fn read_dict(path: &Path) -> Vec<(i64, String)> {
    let mut rows = Vec::new();
    for batch in read_batches(path, &["id", "value"]) {
        let ids = batch
            .column(0)
            .as_any()
            .downcast_ref::<Int64Array>()
            .unwrap();
        let values = batch
            .column(1)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        rows.extend(
            ids.iter()
                .zip(values.iter())
                .map(|(id, v)| (id.unwrap(), v.unwrap().to_string())),
        );
    }
    rows
}

/// Every `<name>.parquet` in `dir` other than the dictionary, sorted by name.
fn relation_files(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .filter_map(|f| f.strip_suffix(".parquet").map(str::to_string))
        .filter(|stem| stem != "dict")
        .collect();
    names.sort();
    names
}

#[test]
fn relation_parquet_files_hold_each_predicates_tuples_in_input_order() {
    let dir = generate();
    let out = dir.path();

    // One file per distinct predicate IRI, including the one no query uses.
    assert_eq!(relation_files(out), [
        "follows", "has_age", "title", "title_6"
    ]);

    // t1, t2, t4: alice→bob, bob→carol, carol→alice.
    assert_eq!(read_relation(&out.join("follows.parquet")), [
        (0, 2),
        (2, 3),
        (3, 0)
    ]);
    // t3, t7: the first `title` IRI (ogp.me) keeps the bare name.
    assert_eq!(read_relation(&out.join("title.parquet")), [(0, 5), (2, 10)]);
    // t5: bob → _:review1.
    assert_eq!(read_relation(&out.join("title_6.parquet")), [(2, 7)]);
    // t6: carol → "42"^^xsd:integer.
    assert_eq!(read_relation(&out.join("has_age.parquet")), [(3, 9)]);
}

#[test]
fn sanitisation_collision_suffixes_the_second_predicate_with_its_dict_id() {
    let dir = generate();
    let out = dir.path();

    // Both IRIs sanitise to `title`. The ogp.me IRI appears first (t3) and
    // keeps the bare name; the purl.org IRI (t5) is dictionary id 6.
    let dict = read_dict(&out.join("dict.parquet"));
    assert_eq!(dict[4], (4, "<http://ogp.me/ns#title>".to_string()));
    assert_eq!(
        dict[6],
        (6, "<http://purl.org/stuff/rev#title>".to_string())
    );
    assert!(out.join("title.parquet").exists());
    assert!(out.join("title_6.parquet").exists());

    // The translator resolves each IRI to its own name, not by sanitising.
    let bench: BenchmarkDefinition =
        serde_yaml::from_str(&fs::read_to_string(out.join("benchmark.yml")).unwrap()).unwrap();
    let titles = bench.queries.iter().find(|q| q.name == "titles").unwrap();
    assert!(
        titles.query.contains("title_6(X, T), title(X, N)"),
        "{}",
        titles.query
    );
}

#[test]
fn dictionary_is_dense_canonical_and_appends_query_only_constants() {
    let dir = generate();
    let dict = read_dict(&dir.path().join("dict.parquet"));

    let expected: Vec<(i64, String)> = [
        "<http://x/alice>",
        "<http://x/follows>",
        "<http://x/bob>",
        "<http://x/carol>",
        "<http://ogp.me/ns#title>",
        "\"Dr\"@en",
        "<http://purl.org/stuff/rev#title>",
        "_:review1",
        "<http://x/has-age>",
        "\"42\"^^<http://www.w3.org/2001/XMLSchema#integer>",
        "\"Mx\"",
        // Query-only constants, after every data term: `dave` (first seen in
        // query `dave`), then `erin` (query `strangers`). `strangers` also
        // repeats `dave`, which must not be interned twice.
        "<http://x/dave>",
        "<http://x/erin>",
    ]
    .iter()
    .enumerate()
    .map(|(id, v)| (id as i64, (*v).to_string()))
    .collect();

    assert_eq!(dict, expected);
}

#[test]
fn translated_rules_match_exactly() {
    let dir = generate();
    let bench: BenchmarkDefinition =
        serde_yaml::from_str(&fs::read_to_string(dir.path().join("benchmark.yml")).unwrap())
            .unwrap();

    let rules: Vec<(&str, &str)> = bench
        .queries
        .iter()
        .map(|q| (q.name.as_str(), q.query.as_str()))
        .collect();
    assert_eq!(rules, [
        // SELECT *: head is every variable in first-appearance order.
        ("path", "Q_path(X, Y, Z) :- follows(X, Y), follows(Y, Z)."),
        // Explicit projection keeps the SELECT order; `carol` is data id 3.
        (
            "titles",
            "Q_titles(T, N, X) :- follows(X, c3), title_6(X, T), title(X, N)."
        ),
        // `dave` is absent from the data: first fresh id after the 11 data
        // terms.
        ("dave", "Q_dave(Y) :- follows(c11, Y)."),
        // `erin` is fresh (12); `dave` reuses 11.
        (
            "strangers",
            "Q_strangers(X) :- follows(X, c12), follows(c11, X)."
        ),
    ]);
}

#[test]
fn benchmark_yaml_parses_validates_and_resolves_every_relation() {
    let dir = generate();
    let out = dir.path();

    let yaml = fs::read_to_string(out.join("benchmark.yml")).unwrap();
    let bench: BenchmarkDefinition = serde_yaml::from_str(&yaml).expect("benchmark.yml parses");
    bench.validate().expect("benchmark.yml validates");

    assert_eq!(bench.name, BENCH_NAME);
    assert_eq!(bench.description, DESCRIPTION);
    assert!(bench.generator.is_none());

    let queries: Vec<(&str, Option<u64>)> = bench
        .queries
        .iter()
        .map(|q| (q.name.as_str(), q.expected))
        .collect();
    assert_eq!(queries, [
        ("path", Some(3)),
        ("titles", Some(1)),
        ("dave", Some(0)),
        ("strangers", Some(0))
    ]);

    // Only predicates some query body uses are declared, sorted by name, so
    // `has_age` is left out even though its Parquet file exists.
    let names: Vec<&str> = bench.relations.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(names, ["follows", "title", "title_6"]);

    let canonical_out = out.canonicalize().unwrap();
    for rel in &bench.relations {
        assert_eq!(
            rel.path, None,
            "generated relation {} sets `path`",
            rel.name
        );
        let url = rel.url.as_deref().expect("generated relations carry a url");
        let file = Path::new(
            url.strip_prefix("file://")
                .unwrap_or_else(|| panic!("{url} is not a file:// url")),
        );
        assert_eq!(file, canonical_out.join(format!("{}.parquet", rel.name)));
        assert!(file.is_file(), "{url} does not resolve to a file");
    }
}
