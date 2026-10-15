use {
    kermit_bench::{
        cache::sha256_hex, discovery::load_all_benchmarks_with_cache, BenchmarkDefinition,
        QueryDefinition,
    },
    kermit_ds::{read_parquet, RelationHeader, Tuples},
    kermit_generate::{run, Generated, GeneratedMeta, Generator, RelationSink},
    kermit_rdf::generator::{
        meta_json_path, read_meta_json, MetaHeader, Target, META_SCHEMA_VERSION,
    },
    serde::{Deserialize, Serialize},
    serde_json::{json, Value},
    std::{
        fs,
        path::{Path, PathBuf},
    },
};

const EDGES: [[usize; 2]; 5] = [[1, 2], [1, 3], [2, 3], [3, 1], [4, 1]];
const NODES: [usize; 6] = [1, 2, 3, 5, 8, 13];

/// Writes `edge` in one shot and `node` in three batches, with the one
/// query `Q(X, Y) :- edge(X, Y), node(X).` (4 answers).
struct ToyGenerator {
    out_dir: PathBuf,
    spec_hash: Option<String>,
    /// `generate` fails after writing `edge`.
    fail: bool,
    /// The query is not a well-formed rule.
    bad_query: bool,
    /// The query names a relation the generator never wrote.
    unknown_relation: bool,
    /// No query at all: every check passes and `validate()` fails at step
    /// 5, so a `meta.json` written before step 6 would be caught.
    no_queries: bool,
    extras: Value,
}

impl ToyGenerator {
    fn new(out_dir: PathBuf) -> Self {
        Self {
            out_dir,
            spec_hash: Some("hash-1".to_string()),
            fail: false,
            bad_query: false,
            unknown_relation: false,
            no_queries: false,
            extras: json!({ "seed": 7, "note": "toy" }),
        }
    }
}

impl Generator for ToyGenerator {
    type Extras = Value;

    const KIND: &'static str = "toy";

    fn target(&self) -> Target<'_> {
        Target {
            out_dir: &self.out_dir,
            bench_name: "toy-bench",
            tag: "t1",
            spec_hash: self.spec_hash.as_deref(),
        }
    }

    fn generate(&self, sink: &mut RelationSink) -> anyhow::Result<Generated<Value>> {
        let edge = RelationHeader::new("edge", vec!["src".to_string(), "dst".to_string()]);
        let edges: Vec<Vec<usize>> = EDGES.iter().map(|row| row.to_vec()).collect();
        sink.write(&edge, &Tuples::from(edges))?;
        if self.fail {
            anyhow::bail!("toy failure");
        }
        let node = RelationHeader::new("node", vec!["id".to_string()]);
        let mut writer = sink.open(&node)?;
        for chunk in NODES.chunks(2) {
            let mut batch = Tuples::new(1);
            for id in chunk {
                batch.push(&[*id]);
            }
            writer.write_batch(&batch)?;
        }
        writer.finish()?;
        let rule = if self.bad_query {
            "Q(X, Y) :- edge(X, Y"
        } else if self.unknown_relation {
            "Q(X, Y) :- edge(X, Y), ghost(X)."
        } else {
            "Q(X, Y) :- edge(X, Y), node(X)."
        };
        let queries = if self.no_queries {
            Vec::new()
        } else {
            vec![QueryDefinition {
                name: "q1".to_string(),
                description: "edges that leave a node".to_string(),
                query: rule.to_string(),
                expected: Some(4),
            }]
        };
        Ok(Generated {
            description: "A toy benchmark".to_string(),
            queries,
            extras: self.extras.clone(),
        })
    }
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct ToyExtras {
    seed: u64,
    note: String,
}

/// A `meta.json` a previous run might have left behind.
fn plant_stale_meta(out_dir: &Path) {
    fs::create_dir_all(out_dir).unwrap();
    fs::write(
        meta_json_path(out_dir),
        r#"{"schema_version":4,"kind":"toy"}"#,
    )
    .unwrap();
}

/// `run` must fail with a message containing `needle`, and leave neither a
/// `meta.json` (the stale one included) nor a `benchmark.yml`.
fn assert_fails_without_meta(generator: &ToyGenerator, needle: &str) {
    plant_stale_meta(&generator.out_dir);
    let err = run(generator).expect_err("run should have failed");
    let message = format!("{err:#}");
    assert!(
        message.contains(needle),
        "expected {needle:?} in: {message}"
    );
    assert!(
        !meta_json_path(&generator.out_dir).exists(),
        "the stale meta.json survived"
    );
    assert!(
        !generator.out_dir.join("benchmark.yml").exists(),
        "benchmark.yml was written"
    );
}

#[test]
fn run_writes_a_discoverable_cache() {
    let dir = tempfile::tempdir().unwrap();
    let cache_root = dir.path().join("cache");
    let out = cache_root.join("toy-bench");
    let meta = run(&ToyGenerator::new(out.clone())).unwrap();

    // benchmark.yml parses, validates, and carries each relation's digest.
    let yaml = fs::read_to_string(out.join("benchmark.yml")).unwrap();
    let definition: BenchmarkDefinition = serde_yaml::from_str(&yaml).unwrap();
    definition.validate().unwrap();
    assert_eq!(definition.name, "toy-bench");
    assert_eq!(definition.description, "A toy benchmark");
    assert!(definition.generator.is_none());
    let names: Vec<&str> = definition
        .relations
        .iter()
        .map(|r| r.name.as_str())
        .collect();
    assert_eq!(names, ["edge", "node"]);
    let canonical = out.canonicalize().unwrap();
    for relation in &definition.relations {
        let url = relation.url.as_deref().unwrap();
        let path = Path::new(url.strip_prefix("file://").unwrap());
        assert_eq!(path, canonical.join(format!("{}.parquet", relation.name)));
        assert!(path.exists(), "{url} does not exist");
        assert_eq!(
            relation.sha256.as_deref(),
            Some(sha256_hex(path).unwrap().as_str())
        );
        assert!(relation.path.is_none());
    }
    assert_eq!(definition.queries.len(), 1);
    assert_eq!(definition.queries[0].expected, Some(4));

    // meta.json carries the shared fields and the extras.
    let header = MetaHeader::read(&out).unwrap();
    assert_eq!(header.kind, "toy");
    assert_eq!(header.spec_hash.as_deref(), Some("hash-1"));
    assert_eq!(header.schema_version, META_SCHEMA_VERSION);
    let read: GeneratedMeta<ToyExtras> = read_meta_json(&out).unwrap();
    assert_eq!(read.extras, ToyExtras {
        seed: 7,
        note: "toy".to_string(),
    });
    assert_eq!(read.tag, "t1");
    assert_eq!(read.generated_at_utc.len(), "1970-01-01T00:00:00Z".len());
    assert_eq!((read.relation_count, read.query_count), (2, 1));
    assert_eq!(read.relations, meta.relations);
    assert_eq!(meta.extras, json!({ "seed": 7, "note": "toy" }));

    // Discovery finds it, in an otherwise empty workspace.
    let workspace = dir.path().join("workspace");
    let found = load_all_benchmarks_with_cache(&workspace, &cache_root).unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].name, "toy-bench");
}

#[test]
fn rows_and_relation_count_are_recorded() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("toy-bench");
    let meta = run(&ToyGenerator::new(out.clone())).unwrap();

    assert_eq!((meta.relation_count, meta.query_count), (2, 1));
    let summary: Vec<(&str, Vec<&str>, u64)> = meta
        .relations
        .iter()
        .map(|r| {
            (
                r.name.as_str(),
                r.columns.iter().map(String::as_str).collect(),
                r.rows,
            )
        })
        .collect();
    assert_eq!(summary, [
        ("edge", vec!["src", "dst"], 5),
        ("node", vec!["id"], 6)
    ]);

    // The streamed relation holds its rows in order, whatever the batching.
    let (header, tuples) = read_parquet(out.join("node.parquet")).unwrap();
    assert_eq!(header, RelationHeader::new("node", vec!["id".to_string()]));
    assert_eq!(tuples.to_vecs(), [[1], [2], [3], [5], [8], [13]]);
}

#[test]
fn failed_generate_writes_no_meta() {
    let dir = tempfile::tempdir().unwrap();
    let toy = ToyGenerator {
        fail: true,
        ..ToyGenerator::new(dir.path().join("toy-bench"))
    };
    assert_fails_without_meta(&toy, "toy failure");
}

#[test]
fn bad_query_writes_no_meta() {
    let dir = tempfile::tempdir().unwrap();
    let toy = ToyGenerator {
        bad_query: true,
        ..ToyGenerator::new(dir.path().join("toy-bench"))
    };
    assert_fails_without_meta(&toy, "\"q1\"");
}

#[test]
fn unknown_relation_writes_no_meta() {
    let dir = tempfile::tempdir().unwrap();
    let toy = ToyGenerator {
        unknown_relation: true,
        ..ToyGenerator::new(dir.path().join("toy-bench"))
    };
    assert_fails_without_meta(&toy, "ghost");
}

#[test]
fn colliding_extras_write_no_meta() {
    let dir = tempfile::tempdir().unwrap();
    let toy = ToyGenerator {
        extras: json!({ "relations": 1 }),
        ..ToyGenerator::new(dir.path().join("toy-bench"))
    };
    assert_fails_without_meta(&toy, "\"relations\"");
}

#[test]
fn non_object_extras_write_no_meta() {
    let dir = tempfile::tempdir().unwrap();
    let toy = ToyGenerator {
        extras: json!(42),
        ..ToyGenerator::new(dir.path().join("toy-bench"))
    };
    assert_fails_without_meta(&toy, "JSON object");
}

/// `validate()` runs at step 5, after every check passed; `meta.json`
/// (step 6) must still be absent, which pins that it is written last.
#[test]
fn no_queries_write_no_meta() {
    let dir = tempfile::tempdir().unwrap();
    let toy = ToyGenerator {
        no_queries: true,
        ..ToyGenerator::new(dir.path().join("toy-bench"))
    };
    assert_fails_without_meta(&toy, "queries must not be empty");
}
