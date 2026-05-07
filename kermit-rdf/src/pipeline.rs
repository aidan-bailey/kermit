//! End-to-end pipeline orchestrator.
//!
//! Runs the driver to produce raw watdiv artifacts, then runs stages 4–6
//! in pure Rust to produce the final benchmark cache directory:
//!
//! ```text
//! <out_dir>/
//!   meta.json
//!   benchmark.yml
//!   dict.parquet
//!   <predicate>.parquet × N
//!   raw/data.nt
//!   raw/templates/*.txt
//!   raw/queries/*.sparql + *.desc
//!   expected/<query>.csv
//! ```

use {
    crate::{
        driver::{self, invoke::split_queries, DriverInputs, RawArtifacts, StressParams},
        error::RdfError,
        expected, parquet, partition,
        sparql::translator::translate_query,
        timestamp::utc_iso8601_now,
        yaml_emit::{write_benchmark_yaml, YamlInputs},
    },
    serde::Serialize,
    sha2::{Digest, Sha256},
    std::{
        collections::HashMap,
        fs,
        io::Read,
        path::{Path, PathBuf},
    },
};

/// Inputs for `run_pipeline`.
pub struct PipelineInputs<'a> {
    /// Driver inputs (binary path, model file, vendor files, scale, stress).
    pub driver: DriverInputs<'a>,
    /// Final output directory; created if missing.
    pub out_dir: &'a Path,
    /// Benchmark name (used for the YAML's `name` field, equal to the cache
    /// dir's basename in normal use).
    pub bench_name: &'a str,
    /// Tag (recorded in meta.json for provenance).
    pub tag: &'a str,
    /// Optional `spec_hash` recorded in meta.json so the materialization
    /// layer can detect param drift on subsequent `bench run` invocations.
    /// `None` for the imperative `bench gen` path; populated by the
    /// declarative-YAML path.
    pub spec_hash: Option<&'a str>,
}

/// Returns the byte counts and IDs surfaced in `meta.json`.
#[derive(Debug, serde::Deserialize, Serialize)]
pub struct PipelineMeta {
    /// Schema version (bump on breaking field-name or value-type change).
    pub schema_version: u32,
    /// "watdiv-onthefly" for this pipeline.
    pub kind: String,
    /// Scale factor passed to watdiv -d.
    pub scale: u32,
    /// User-provided tag (CLI `--tag`).
    pub tag: String,
    /// SHA-256 of the watdiv binary file.
    pub watdiv_binary_sha256: String,
    /// SHA-256s of vendor files used.
    pub names_files_sha256: HashMap<String, String>,
    /// SHA-256 of the model file.
    pub model_file_sha256: String,
    /// Stress params copied from CLI / defaults.
    pub stress_params: StressParamsMeta,
    /// UTC timestamp of generation.
    pub generated_at_utc: String,
    /// Number of triples generated.
    pub triple_count: u64,
    /// Number of distinct predicates (relations).
    pub relation_count: u32,
    /// Number of queries produced.
    pub query_count: u32,
    /// Hash of the declarative `GeneratorSpec` that produced this run, when
    /// the run was driven by a YAML spec. `None` for imperative `bench gen`
    /// runs. Used by the materialization layer to detect param drift.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spec_hash: Option<String>,
}

/// Stress params surfaced into meta.json.
#[derive(Debug, serde::Deserialize, Serialize)]
pub struct StressParamsMeta {
    /// `--max-query-size`.
    pub max_query_size: u32,
    /// `--query-count`.
    pub query_count: u32,
    /// `--constants-per-query`.
    pub constants_per_query: u32,
    /// `--allow-join-vertex`.
    pub allow_join_vertex: bool,
}

impl From<&StressParams> for StressParamsMeta {
    fn from(s: &StressParams) -> Self {
        Self {
            max_query_size: s.max_query_size,
            query_count: s.query_count,
            constants_per_query: s.constants_per_query,
            allow_join_vertex: s.allow_join_vertex,
        }
    }
}

fn sha256_file(path: &Path) -> Result<String, RdfError> {
    let mut h = Sha256::new();
    let mut f = fs::File::open(path)?;
    let mut buf = [0u8; 8192];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(format!("{:x}", h.finalize()))
}

/// Stages 4 + 5 + 6 of the pipeline. Public so the no-binary pipeline
/// integration test (Task 17) can drive stages 4–6 with a hand-crafted
/// `RawArtifacts`-equivalent.
pub fn process_artifacts(
    inputs: &PipelineInputs, raw: &RawArtifacts,
) -> Result<PipelineMeta, RdfError> {
    fs::create_dir_all(inputs.out_dir)?;
    let raw_root = inputs.out_dir.join("raw");
    fs::create_dir_all(raw_root.join("templates"))?;
    fs::create_dir_all(raw_root.join("queries"))?;

    fs::copy(&raw.data_nt, raw_root.join("data.nt"))?;
    for tpl in &raw.templates {
        let dst = raw_root.join("templates").join(tpl.file_name().unwrap());
        fs::copy(tpl, dst)?;
    }
    let mut copied_sparql_paths: Vec<PathBuf> = Vec::new();
    for (sparql, desc) in &raw.queries {
        let s_dst = raw_root.join("queries").join(sparql.file_name().unwrap());
        fs::copy(sparql, &s_dst)?;
        if desc.exists() {
            let d_dst = raw_root.join("queries").join(desc.file_name().unwrap());
            fs::copy(desc, d_dst)?;
        }
        copied_sparql_paths.push(s_dst);
    }

    let part = partition::partition(raw_root.join("data.nt"))?;
    let mut dict = part.dict;

    for rel in &part.relations {
        let path = inputs.out_dir.join(format!("{}.parquet", rel.name));
        parquet::write_relation(rel, &path)?;
    }

    let all_predicates: Vec<String> = part.relations.iter().map(|r| r.name.clone()).collect();

    let mut all_queries: Vec<(String, String)> = Vec::new();
    for sparql_path in &copied_sparql_paths {
        let stem = sparql_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("q")
            .replace('.', "-");
        let text = fs::read_to_string(sparql_path)?;
        let stem_underscores = stem.replace('-', "_");
        // Watdiv `-q` emits multi-line SPARQL queries separated by `#end`
        // markers; one logical query is a multi-line block, not one line.
        for (i, q) in split_queries(&text).iter().enumerate() {
            let qname = format!("{stem}_q{i:04}");
            let head = format!("Q_{stem_underscores}_q{i:04}");
            let dl = translate_query(q, &mut dict, &part.predicate_map, &head)?;
            all_queries.push((qname, dl));
        }
    }

    let dict_path = inputs.out_dir.join("dict.parquet");
    parquet::write_dict(&dict, &dict_path)?;

    let base_url = format!("file://{}", inputs.out_dir.canonicalize()?.display());
    let description = format!(
        "WatDiv on-the-fly generation, scale {}, tag {}",
        inputs.driver.scale, inputs.tag
    );
    let yaml = YamlInputs {
        name: inputs.bench_name,
        description: &description,
        queries: all_queries.clone(),
        all_predicates: &all_predicates,
        base_url: &base_url,
    };
    write_benchmark_yaml(&yaml, inputs.out_dir)?;

    let expected_dir = inputs.out_dir.join("expected");
    expected::write_expected_csvs(&copied_sparql_paths, &expected_dir)?;

    let mut names_hashes = HashMap::new();
    for n in ["firstnames.txt", "lastnames.txt"] {
        let p = inputs.driver.vendor_files.join(n);
        names_hashes.insert(n.to_string(), sha256_file(&p)?);
    }

    let triple_count: u64 = part.relations.iter().map(|r| r.tuples.len() as u64).sum();

    let meta = PipelineMeta {
        schema_version: 2,
        kind: "watdiv-onthefly".to_string(),
        scale: inputs.driver.scale,
        tag: inputs.tag.to_string(),
        watdiv_binary_sha256: sha256_file(inputs.driver.watdiv_bin)?,
        names_files_sha256: names_hashes,
        model_file_sha256: sha256_file(inputs.driver.model_file)?,
        stress_params: StressParamsMeta::from(&inputs.driver.stress),
        generated_at_utc: utc_iso8601_now(),
        triple_count,
        relation_count: part.relations.len() as u32,
        query_count: all_queries.len() as u32,
        spec_hash: inputs.spec_hash.map(|s| s.to_string()),
    };
    let meta_json =
        serde_json::to_string_pretty(&meta).map_err(|e| RdfError::Expected(e.to_string()))?;
    fs::write(inputs.out_dir.join("meta.json"), meta_json)?;
    Ok(meta)
}

/// Top-level entry point: runs the driver and processes artifacts.
pub fn run_pipeline(inputs: &PipelineInputs) -> Result<PipelineMeta, RdfError> {
    let raw = driver::drive(&inputs.driver)?;
    process_artifacts(inputs, &raw)
}
