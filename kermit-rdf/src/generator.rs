//! Shared post-driver orchestration for on-the-fly benchmark generators.
//!
//! Every generator (WatDiv stress, WatDiv basic, LUBM) drives a different
//! external tool, but once raw N-Triples and SPARQL exist the remaining work
//! is identical: partition the triples into per-predicate relations, write
//! them as Parquet, translate the queries to Datalog, write the dictionary,
//! emit `benchmark.yml`, write expected cardinalities, and record
//! `meta.json`. [`process_artifacts`] owns that sequence once; a generator
//! supplies only the parts that differ through the [`Generator`] trait.
//!
//! ```text
//!            driver (per generator)
//!                    │ Raw
//!                    ▼
//!  ┌──────────── process_artifacts ────────────┐
//!  │ stage_raw      → <out>/raw/*, nt to read  │  Generator::stage_raw
//!  │ partition      → relations + dict         │  partition::partition
//!  │ seed_relations → (optional) empty rels    │  Generator::seed_relations
//!  │ write_relation → <out>/<pred>.parquet     │  parquet::write_relation
//!  │ translate      → [(name, datalog)]        │  Generator::translate_queries
//!  │ write_dict     → <out>/dict.parquet       │  parquet::write_dict
//!  │ emit YAML      → <out>/benchmark.yml      │  yaml_emit::write_benchmark_yaml
//!  │ write_expected → <out>/expected/*.csv     │  Generator::write_expected
//!  │ build_meta     → <out>/meta.json          │  Generator::build_meta + write_meta_json
//!  └───────────────────────────────────────────┘
//! ```
//!
//! The `meta.json` files of different generators carry different
//! tool-specific provenance, so each keeps its own serde struct; the fields
//! they all share are exposed through [`GeneratorMeta`], and
//! [`MetaHeader`] reads just those fields back without knowing the kind.

use {
    crate::{
        dict::Dictionary,
        error::RdfError,
        parquet,
        partition::{self, Partitioned},
        timestamp::utc_iso8601_now,
        yaml_emit::{write_benchmark_yaml, YamlInputs},
    },
    serde::{de::DeserializeOwned, Deserialize, Serialize},
    std::{
        collections::HashMap,
        fs,
        path::{Path, PathBuf},
    },
};

/// Current `meta.json` schema version shared by every generator. Bump on
/// any breaking field-name or value-type change to a shared field; a
/// generator-specific field change bumps it too, since consumers key
/// compatibility off this single number.
pub const META_SCHEMA_VERSION: u32 = 2;

/// Where a generator writes its output and how the run is labelled. The
/// fields every pipeline's `*Inputs` struct carries, so the orchestrator
/// can read them without knowing the concrete inputs type.
#[derive(Debug, Clone, Copy)]
pub struct Target<'a> {
    /// Final output directory; created if missing.
    pub out_dir: &'a Path,
    /// Benchmark name (the YAML's `name` field, equal to the cache dir's
    /// basename in normal use).
    pub bench_name: &'a str,
    /// User-provided tag, recorded in `meta.json` for provenance.
    pub tag: &'a str,
    /// Hash of the declarative `GeneratorSpec` that produced this run, when
    /// driven by a YAML spec. `None` for imperative `bench gen` runs. Used
    /// by the materialization layer to detect param drift.
    pub spec_hash: Option<&'a str>,
}

/// The provenance fields the orchestrator computes for every generator and
/// hands to [`Generator::build_meta`], so no generator re-derives them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Provenance {
    /// Always [`META_SCHEMA_VERSION`].
    pub schema_version: u32,
    /// User-provided tag (CLI `--tag`).
    pub tag: String,
    /// UTC timestamp of generation.
    pub generated_at_utc: String,
    /// Number of `<pred>.parquet` relations written.
    pub relation_count: u32,
    /// Number of queries written to `benchmark.yml`.
    pub query_count: u32,
    /// See [`Target::spec_hash`].
    pub spec_hash: Option<String>,
}

/// Accessors for the fields every generator's `meta.json` shares. Each
/// generator keeps its own serde struct (the tool-specific fields differ);
/// this trait is what lets [`write_meta_json`] and the drift-detection
/// readers treat them uniformly.
pub trait GeneratorMeta: Serialize + DeserializeOwned {
    /// Schema version the file was written with.
    fn schema_version(&self) -> u32;
    /// Discriminator naming the generator (`"watdiv-onthefly"`,
    /// `"lubm-onthefly"`, …).
    fn kind(&self) -> &str;
    /// User-provided tag.
    fn tag(&self) -> &str;
    /// UTC timestamp of generation.
    fn generated_at_utc(&self) -> &str;
    /// Number of relations written.
    fn relation_count(&self) -> u32;
    /// Number of queries emitted.
    fn query_count(&self) -> u32;
    /// Declarative spec hash, if any.
    fn spec_hash(&self) -> Option<&str>;
}

/// The kind-agnostic subset of any generator's `meta.json`. Deserializes
/// from every generator's file (unknown fields are ignored) and from legacy
/// schema-version-1 files that predate `spec_hash`, so the materialization
/// layer can detect spec drift without matching on `kind`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct MetaHeader {
    /// Schema version the file was written with.
    pub schema_version: u32,
    /// Generator discriminator.
    pub kind: String,
    /// Declarative spec hash; `None` for imperative runs and legacy files.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spec_hash: Option<String>,
}

impl MetaHeader {
    /// Reads the header fields of `<out_dir>/meta.json`.
    pub fn read(out_dir: &Path) -> Result<Self, RdfError> {
        let text = fs::read_to_string(meta_json_path(out_dir))?;
        serde_json::from_str(&text).map_err(|e| RdfError::Expected(e.to_string()))
    }
}

/// Path of a generator's `meta.json` inside its output directory.
pub fn meta_json_path(out_dir: &Path) -> PathBuf { out_dir.join("meta.json") }

/// Serialises `meta` as pretty JSON to `<out_dir>/meta.json`.
pub fn write_meta_json<M: GeneratorMeta>(out_dir: &Path, meta: &M) -> Result<(), RdfError> {
    let json = serde_json::to_string_pretty(meta).map_err(|e| RdfError::Expected(e.to_string()))?;
    fs::write(meta_json_path(out_dir), json)?;
    Ok(())
}

/// Reads `<out_dir>/meta.json` back as a concrete generator's meta type.
pub fn read_meta_json<M: GeneratorMeta>(out_dir: &Path) -> Result<M, RdfError> {
    let text = fs::read_to_string(meta_json_path(out_dir))?;
    serde_json::from_str(&text).map_err(|e| RdfError::Expected(e.to_string()))
}

/// The generator-specific parts of the post-driver pipeline. Implement this
/// for a `(inputs, workload)` view of a pipeline and hand it to
/// [`process_artifacts`]; see `crate::pipeline` (WatDiv) and
/// `crate::lubm::pipeline` (LUBM) for the two precedents.
pub trait Generator {
    /// What the driver produced (paths inside a staging dir plus echoed
    /// parameters).
    type Raw;
    /// What [`Generator::stage_raw`] produced and later hooks need
    /// (copied query paths, entailment statistics, …).
    type Staged;
    /// This generator's `meta.json` struct.
    type Meta: GeneratorMeta;

    /// Output location and run labels.
    fn target(&self) -> Target<'_>;

    /// Copies the raw driver artifacts under `raw_root` (`<out_dir>/raw`,
    /// already created) and runs any pre-partition transformation (LUBM's
    /// entailment). Returns the N-Triples file the orchestrator partitions,
    /// plus whatever later hooks need.
    fn stage_raw(
        &self, raw: &Self::Raw, raw_root: &Path,
    ) -> Result<(PathBuf, Self::Staged), RdfError>;

    /// Hook to add relations the data lacks before Parquet is written (the
    /// WatDiv basic workload seeds empty relations for query predicates
    /// absent from the generated data). Default: nothing.
    fn seed_relations(
        &self, _staged: &Self::Staged, _part: &mut Partitioned,
    ) -> Result<(), RdfError> {
        Ok(())
    }

    /// Produces `(query_name, datalog)` pairs for `benchmark.yml`, growing
    /// `dict` for constants the data never mentioned.
    fn translate_queries(
        &self, staged: &Self::Staged, dict: &mut Dictionary,
        predicate_map: &HashMap<String, String>,
    ) -> Result<Vec<(String, String)>, RdfError>;

    /// Human-readable `description` for `benchmark.yml`.
    fn description(&self, raw: &Self::Raw) -> String;

    /// Writes `expected/<query>.csv` cardinalities into `expected_dir`
    /// (already created). May write nothing.
    fn write_expected(&self, staged: &Self::Staged, expected_dir: &Path) -> Result<(), RdfError>;

    /// Assembles this generator's `meta.json` from the tool-specific inputs
    /// plus the shared `provenance` the orchestrator computed.
    fn build_meta(
        &self, raw: &Self::Raw, staged: &Self::Staged, part: &Partitioned, provenance: Provenance,
    ) -> Result<Self::Meta, RdfError>;
}

/// Runs the shared post-driver sequence for `generator` over `raw`, writing
/// the complete benchmark cache directory at `target().out_dir` and
/// returning the `meta.json` contents it recorded.
pub fn process_artifacts<G: Generator>(generator: &G, raw: &G::Raw) -> Result<G::Meta, RdfError> {
    let target = generator.target();
    let out_dir = target.out_dir;

    // Stage A: copy the raw driver artifacts into `<out_dir>/raw/` and
    // apply any pre-partition transformation.
    fs::create_dir_all(out_dir)?;
    let raw_root = out_dir.join("raw");
    fs::create_dir_all(&raw_root)?;
    let (nt_path, staged) = generator.stage_raw(raw, &raw_root)?;

    // Stage B: partition the N-Triples into per-predicate relations and
    // write each as Parquet.
    let mut part = partition::partition(&nt_path)?;
    generator.seed_relations(&staged, &mut part)?;
    for rel in &part.relations {
        parquet::write_relation(rel, &out_dir.join(format!("{}.parquet", rel.name)))?;
    }
    let all_predicates: Vec<String> = part.relations.iter().map(|r| r.name.clone()).collect();

    // Stage C: translate the SPARQL workload to Datalog.
    let queries = generator.translate_queries(&staged, &mut part.dict, &part.predicate_map)?;

    // Stage D: write the dictionary (after the translator may have grown it).
    parquet::write_dict(&part.dict, &out_dir.join("dict.parquet"))?;

    // Stage E: emit benchmark.yml.
    let base_url = format!("file://{}", out_dir.canonicalize()?.display());
    let description = generator.description(raw);
    let yaml = YamlInputs {
        name: target.bench_name,
        description: &description,
        queries: queries.clone(),
        all_predicates: &all_predicates,
        base_url: &base_url,
    };
    write_benchmark_yaml(&yaml, out_dir)?;

    // Stage F: expected cardinalities.
    let expected_dir = out_dir.join("expected");
    fs::create_dir_all(&expected_dir)?;
    generator.write_expected(&staged, &expected_dir)?;

    // Stage G: meta.json.
    let provenance = Provenance {
        schema_version: META_SCHEMA_VERSION,
        tag: target.tag.to_string(),
        generated_at_utc: utc_iso8601_now(),
        relation_count: part.relations.len() as u32,
        query_count: queries.len() as u32,
        spec_hash: target.spec_hash.map(|s| s.to_string()),
    };
    let meta = generator.build_meta(raw, &staged, &part, provenance)?;
    write_meta_json(out_dir, &meta)?;
    Ok(meta)
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::{expected::write_cardinality_csv, sparql::translator::translate_query},
    };

    /// A minimal generator over hand-written N-Triples and one SPARQL query,
    /// exercising every hook without an external tool.
    struct ToyGenerator<'a> {
        target: Target<'a>,
    }

    struct ToyRaw {
        nt_text: &'static str,
    }

    #[derive(Debug, Deserialize, Serialize, PartialEq)]
    struct ToyMeta {
        schema_version: u32,
        kind: String,
        tag: String,
        generated_at_utc: String,
        relation_count: u32,
        query_count: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        spec_hash: Option<String>,
        triple_count: u64,
    }

    impl GeneratorMeta for ToyMeta {
        fn schema_version(&self) -> u32 { self.schema_version }

        fn kind(&self) -> &str { &self.kind }

        fn tag(&self) -> &str { &self.tag }

        fn generated_at_utc(&self) -> &str { &self.generated_at_utc }

        fn relation_count(&self) -> u32 { self.relation_count }

        fn query_count(&self) -> u32 { self.query_count }

        fn spec_hash(&self) -> Option<&str> { self.spec_hash.as_deref() }
    }

    const QUERY: &str = "SELECT * WHERE { ?x <http://x/follows> ?y . ?y <http://x/follows> ?z . }";

    impl Generator for ToyGenerator<'_> {
        type Meta = ToyMeta;
        type Raw = ToyRaw;
        type Staged = ();

        fn target(&self) -> Target<'_> { self.target }

        fn stage_raw(
            &self, raw: &ToyRaw, raw_root: &Path,
        ) -> Result<(PathBuf, Self::Staged), RdfError> {
            let nt = raw_root.join("data.nt");
            fs::write(&nt, raw.nt_text)?;
            Ok((nt, ()))
        }

        fn translate_queries(
            &self, _staged: &(), dict: &mut Dictionary, predicate_map: &HashMap<String, String>,
        ) -> Result<Vec<(String, String)>, RdfError> {
            let dl = translate_query(QUERY, dict, predicate_map, "Q_path")?;
            Ok(vec![("path".to_string(), dl)])
        }

        fn description(&self, _raw: &ToyRaw) -> String { "toy".to_string() }

        fn write_expected(&self, _staged: &(), expected_dir: &Path) -> Result<(), RdfError> {
            write_cardinality_csv(&expected_dir.join("path.csv"), 3)
        }

        fn build_meta(
            &self, _raw: &ToyRaw, _staged: &(), part: &Partitioned, provenance: Provenance,
        ) -> Result<ToyMeta, RdfError> {
            Ok(ToyMeta {
                schema_version: provenance.schema_version,
                kind: "toy".to_string(),
                tag: provenance.tag,
                generated_at_utc: provenance.generated_at_utc,
                relation_count: provenance.relation_count,
                query_count: provenance.query_count,
                spec_hash: provenance.spec_hash,
                triple_count: part.relations.iter().map(|r| r.tuples.len() as u64).sum(),
            })
        }
    }

    const NT: &str = "\
<http://x/a> <http://x/follows> <http://x/b> .
<http://x/b> <http://x/follows> \
                      <http://x/c> .
<http://x/c> <http://x/follows> <http://x/a> .
";

    fn run_toy(out_dir: &Path, spec_hash: Option<&str>) -> ToyMeta {
        let generator = ToyGenerator {
            target: Target {
                out_dir,
                bench_name: "toy-bench",
                tag: "toy-tag",
                spec_hash,
            },
        };
        process_artifacts(&generator, &ToyRaw {
            nt_text: NT,
        })
        .expect("toy pipeline succeeds")
    }

    #[test]
    fn orchestrator_writes_full_cache_layout() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path();
        let meta = run_toy(out, Some("abc"));

        assert!(out.join("raw/data.nt").exists());
        assert!(out.join("follows.parquet").exists());
        assert!(out.join("dict.parquet").exists());
        assert!(out.join("benchmark.yml").exists());
        assert_eq!(
            fs::read_to_string(out.join("expected/path.csv")).unwrap(),
            "cardinality\n3\n"
        );
        assert!(meta_json_path(out).exists());

        let yaml = fs::read_to_string(out.join("benchmark.yml")).unwrap();
        assert!(yaml.contains("Q_path(X, Y, Z) :- follows(X, Y), follows(Y, Z)."));
        assert!(yaml.contains("toy-bench"));
        assert!(yaml.contains("description: toy"));

        assert_eq!(meta.schema_version, META_SCHEMA_VERSION);
        assert_eq!(meta.tag, "toy-tag");
        assert_eq!(meta.relation_count, 1);
        assert_eq!(meta.query_count, 1);
        assert_eq!(meta.triple_count, 3);
        assert_eq!(meta.spec_hash.as_deref(), Some("abc"));
        assert!(!meta.generated_at_utc.is_empty());
    }

    #[test]
    fn meta_json_round_trips_through_typed_and_header_readers() {
        let dir = tempfile::tempdir().unwrap();
        let meta = run_toy(dir.path(), Some("abc"));

        let typed: ToyMeta = read_meta_json(dir.path()).unwrap();
        assert_eq!(typed, meta);

        let header = MetaHeader::read(dir.path()).unwrap();
        assert_eq!(header, MetaHeader {
            schema_version: META_SCHEMA_VERSION,
            kind: "toy".to_string(),
            spec_hash: Some("abc".to_string()),
        });
    }

    #[test]
    fn imperative_runs_omit_spec_hash_from_meta_json() {
        let dir = tempfile::tempdir().unwrap();
        run_toy(dir.path(), None);
        let text = fs::read_to_string(meta_json_path(dir.path())).unwrap();
        assert!(!text.contains("spec_hash"));
        assert_eq!(MetaHeader::read(dir.path()).unwrap().spec_hash, None);
    }

    #[test]
    fn header_reads_legacy_meta_without_spec_hash() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            meta_json_path(dir.path()),
            r#"{"schema_version": 1, "kind": "watdiv-onthefly", "scale": 100}"#,
        )
        .unwrap();
        let header = MetaHeader::read(dir.path()).unwrap();
        assert_eq!(header.schema_version, 1);
        assert_eq!(header.kind, "watdiv-onthefly");
        assert_eq!(header.spec_hash, None);
    }
}
