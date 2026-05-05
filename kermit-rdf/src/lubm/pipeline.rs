//! End-to-end LUBM benchmark generation pipeline.
//!
//! Mirrors `crate::pipeline::run_pipeline` (the WatDiv path) at the module
//! level, but with a different driver, an entailment pre-step, and
//! hand-written queries instead of templates from the binary.
//!
//! ## Output layout
//!
//! ```text
//! <out_dir>/
//!   meta.json                  (LubmMeta — kind = "lubm-onthefly")
//!   benchmark.yml
//!   dict.parquet
//!   <predicate>.parquet × N    (one per predicate seen in entailed data)
//!   raw/data.nt                (gunzipped jar output, document-self stripped)
//!   raw/data.entailed.nt       (post-Univ-Bench-TBox closure; what partition reads)
//!   raw/queries/qN.sparql      (the 14 LUBM queries verbatim)
//!   expected/<query>.csv       (one cardinality per query, optional input)
//! ```

use {
    crate::{
        error::RdfError,
        lubm::{
            driver::{drive, LubmDriverInputs, LubmRawArtifacts},
            entailment::{entail, EntailmentStats},
        },
        parquet, partition,
        sparql::translator::translate_query,
        yaml_emit::{write_benchmark_yaml, YamlInputs},
    },
    serde::Serialize,
    sha2::{Digest, Sha256},
    std::{
        fs,
        io::{Read, Write},
        path::{Path, PathBuf},
    },
};

/// One hand-written LUBM query.
#[derive(Debug, Clone)]
pub struct LubmQuerySpec {
    /// Filename stem (`q1`, `q2`, …) used for the SPARQL filename and the
    /// Datalog head-predicate name.
    pub name: String,
    /// SPARQL source. Must be a BGP-only `SELECT` — the translator rejects
    /// FILTER/OPTIONAL/UNION.
    pub sparql: String,
    /// Optional expected cardinality for this query at this scale (from
    /// the LUBM paper Table 3 or recomputed). Written to
    /// `expected/<name>.csv` if provided.
    pub expected_cardinality: Option<u64>,
}

/// Inputs for [`run_lubm_pipeline`].
pub struct LubmPipelineInputs<'a> {
    /// Driver inputs — passes through to `lubm::driver::drive`.
    pub driver: LubmDriverInputs<'a>,
    /// Final output directory; created if missing.
    pub out_dir: &'a Path,
    /// Benchmark name (used for the YAML's `name` field, equal to the cache
    /// dir's basename in normal use).
    pub bench_name: &'a str,
    /// User-provided tag (recorded in meta.json for provenance).
    pub tag: &'a str,
    /// The 14 LUBM queries (or any other BGP-only SPARQL workload).
    pub queries: &'a [LubmQuerySpec],
    /// Optional `spec_hash` recorded in meta.json so the materialization
    /// layer can detect param drift on subsequent `bench run` invocations.
    /// `None` for the imperative `bench gen` path; populated by the
    /// declarative-YAML path.
    pub spec_hash: Option<&'a str>,
}

/// Stats and provenance recorded into `meta.json`.
#[derive(Debug, serde::Deserialize, Serialize)]
pub struct LubmMeta {
    /// Schema version. Bump on any breaking field-name or value-type change.
    pub schema_version: u32,
    /// Discriminator for distinguishing on-the-fly meta variants. Always
    /// `"lubm-onthefly"` here.
    pub kind: String,
    /// Universities count (`-u`).
    pub scale: u32,
    /// RNG seed (`-s`).
    pub seed: u32,
    /// Starting university index (`-i`).
    pub start_index: u32,
    /// Worker thread count (`-t`).
    pub threads: u32,
    /// User-provided tag (CLI `--tag`).
    pub tag: String,
    /// SHA-256 of the LUBM-UBA jar that produced this snapshot.
    pub lubm_jar_sha256: String,
    /// Ontology IRI passed to the jar (`--ontology`).
    pub ontology_iri: String,
    /// UTC timestamp.
    pub generated_at_utc: String,
    /// Triples in the gunzipped, document-self-stripped jar output.
    pub triple_count_pre_entailment: u64,
    /// Triples after Univ-Bench TBox closure (input ∪ derived).
    pub triple_count_post_entailment: u64,
    /// Distinct triples produced by entailment (post − pre).
    pub derived_triple_count: u64,
    /// Number of fixed-point iterations the entailment loop ran for.
    pub entailment_iterations: u32,
    /// Distinct predicates after entailment (= number of `<pred>.parquet`
    /// files written).
    pub relation_count: u32,
    /// Number of queries written to `benchmark.yml`.
    pub query_count: u32,
    /// Hash of the declarative `GeneratorSpec` that produced this run, when
    /// the run was driven by a YAML spec. `None` for imperative `bench gen`
    /// runs. Used by the materialization layer to detect param drift.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spec_hash: Option<String>,
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

// Hand-rolled ISO-8601 instead of pulling in `chrono` or `time` — mirrors
// the existing watdiv pipeline's helper at `crate::pipeline::utc_iso8601_now`
// (which is private to that module so we can't reuse). Calendar correctness
// around leap seconds doesn't matter for a sortable provenance string;
// breaks at year 4801 (Gregorian leap-year edge case in the simple loop).
fn utc_iso8601_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = secs / 86400;
    let rem = secs % 86400;
    let h = rem / 3600;
    let m = (rem % 3600) / 60;
    let s = rem % 60;
    let (y, mo, d) = days_since_epoch_to_ymd(days as i64);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{m:02}:{s:02}Z")
}

fn days_since_epoch_to_ymd(mut days: i64) -> (i32, u32, u32) {
    let mut y: i32 = 1970;
    loop {
        let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
        let year_days = if leap {
            366
        } else {
            365
        };
        if days < year_days as i64 {
            break;
        }
        days -= year_days as i64;
        y += 1;
    }
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let months = [
        31,
        if leap {
            29
        } else {
            28
        },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut mo: u32 = 1;
    for &mlen in &months {
        if days < mlen as i64 {
            break;
        }
        days -= mlen as i64;
        mo += 1;
    }
    (y, mo, days as u32 + 1)
}

fn write_expected_cardinality(path: &Path, n: u64) -> Result<(), RdfError> {
    let mut f = fs::File::create(path)?;
    writeln!(f, "cardinality")?;
    writeln!(f, "{n}")?;
    Ok(())
}

/// Stages 4–6 of the pipeline (entailment, partition, translate, emit).
/// Public so tests can drive these without invoking the jar.
pub fn process_artifacts(
    inputs: &LubmPipelineInputs, raw: &LubmRawArtifacts,
) -> Result<LubmMeta, RdfError> {
    fs::create_dir_all(inputs.out_dir)?;
    let raw_root = inputs.out_dir.join("raw");
    fs::create_dir_all(raw_root.join("queries"))?;

    let raw_data_nt = raw_root.join("data.nt");
    fs::copy(&raw.data_nt, &raw_data_nt)?;

    // Stage A: entail.
    let entailed_nt = raw_root.join("data.entailed.nt");
    let entailment_stats: EntailmentStats = entail(&raw_data_nt, &entailed_nt)?;

    // Stage B: partition the entailed file.
    let part = partition::partition(&entailed_nt)?;
    let mut dict = part.dict;
    for rel in &part.relations {
        let path = inputs.out_dir.join(format!("{}.parquet", rel.name));
        parquet::write_relation(rel, &path)?;
    }
    let all_predicates: Vec<String> = part.relations.iter().map(|r| r.name.clone()).collect();

    // Stage C: copy queries + translate to Datalog.
    let mut translated: Vec<(String, String)> = Vec::new();
    let mut sparql_paths: Vec<PathBuf> = Vec::new();
    for spec in inputs.queries {
        let sparql_path = raw_root
            .join("queries")
            .join(format!("{}.sparql", spec.name));
        fs::write(&sparql_path, &spec.sparql)?;
        sparql_paths.push(sparql_path);
        let head = format!("Q_{}", spec.name);
        let dl = translate_query(&spec.sparql, &mut dict, &part.predicate_map, &head)?;
        translated.push((spec.name.clone(), dl));
    }

    // Stage D: write dict (after translator may have grown it for unseen URIs).
    let dict_path = inputs.out_dir.join("dict.parquet");
    parquet::write_dict(&dict, &dict_path)?;

    // Stage E: emit benchmark.yml.
    let base_url = format!("file://{}", inputs.out_dir.canonicalize()?.display());
    let description = format!(
        "Lehigh University Benchmark, on-the-fly: scale={}, seed={}, tag={}",
        raw.scale, raw.seed, inputs.tag
    );
    let yaml = YamlInputs {
        name: inputs.bench_name,
        description: &description,
        queries: translated.clone(),
        all_predicates: &all_predicates,
        base_url: &base_url,
    };
    write_benchmark_yaml(&yaml, inputs.out_dir)?;

    // Stage F: expected cardinalities (if provided in spec).
    let expected_dir = inputs.out_dir.join("expected");
    fs::create_dir_all(&expected_dir)?;
    for spec in inputs.queries {
        if let Some(n) = spec.expected_cardinality {
            write_expected_cardinality(&expected_dir.join(format!("{}.csv", spec.name)), n)?;
        }
    }

    // Stage G: meta.json.
    let triple_count_pre_entailment = entailment_stats.input_triples as u64;
    let triple_count_post_entailment = entailment_stats.output_triples as u64;
    let derived_triple_count = entailment_stats.derived_triples as u64;
    let meta = LubmMeta {
        schema_version: 2,
        kind: "lubm-onthefly".to_string(),
        scale: raw.scale,
        seed: raw.seed,
        start_index: raw.start_index,
        threads: inputs.driver.threads,
        tag: inputs.tag.to_string(),
        lubm_jar_sha256: sha256_file(inputs.driver.jar_path)?,
        ontology_iri: raw.ontology_iri.clone(),
        generated_at_utc: utc_iso8601_now(),
        triple_count_pre_entailment,
        triple_count_post_entailment,
        derived_triple_count,
        entailment_iterations: entailment_stats.iterations,
        relation_count: part.relations.len() as u32,
        query_count: translated.len() as u32,
        spec_hash: inputs.spec_hash.map(|s| s.to_string()),
    };
    let meta_json =
        serde_json::to_string_pretty(&meta).map_err(|e| RdfError::Expected(e.to_string()))?;
    fs::write(inputs.out_dir.join("meta.json"), meta_json)?;
    Ok(meta)
}

/// Top-level entry point: drives the LUBM-UBA jar, then runs stages A–G.
pub fn run_lubm_pipeline(inputs: &LubmPipelineInputs) -> Result<LubmMeta, RdfError> {
    let raw = drive(&inputs.driver)?;
    process_artifacts(inputs, &raw)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ymd_epoch_zero_is_jan_1_1970() {
        assert_eq!(days_since_epoch_to_ymd(0), (1970, 1, 1));
    }

    #[test]
    fn iso_timestamp_well_formed() {
        let s = utc_iso8601_now();
        assert_eq!(s.len(), 20);
        assert!(s.ends_with('Z'));
    }

    #[test]
    fn write_expected_cardinality_two_lines() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("q.csv");
        write_expected_cardinality(&p, 42).unwrap();
        let text = fs::read_to_string(&p).unwrap();
        assert_eq!(text, "cardinality\n42\n");
    }
}
