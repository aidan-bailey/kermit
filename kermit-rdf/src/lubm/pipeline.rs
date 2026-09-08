//! End-to-end LUBM benchmark generation pipeline.
//!
//! Shares the post-driver orchestration with the WatDiv path through
//! [`crate::generator::process_artifacts`]; this module supplies only what
//! differs: the LUBM-UBA jar driver, an entailment pre-step, hand-written
//! queries instead of templates from a binary, and the `lubm-*`
//! provenance fields.
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
        dict::Dictionary,
        error::RdfError,
        expected::write_cardinality_csv,
        generator::{self, Generator, GeneratorMeta, Provenance, Target},
        lubm::{
            driver::{drive, LubmDriverInputs, LubmRawArtifacts},
            entailment::{entail, EntailmentStats},
        },
        partition::Partitioned,
        sha256_file,
        sparql::translator::translate_query,
    },
    serde::Serialize,
    std::{
        collections::HashMap,
        fs,
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

impl GeneratorMeta for LubmMeta {
    fn schema_version(&self) -> u32 { self.schema_version }

    fn kind(&self) -> &str { &self.kind }

    fn tag(&self) -> &str { &self.tag }

    fn generated_at_utc(&self) -> &str { &self.generated_at_utc }

    fn relation_count(&self) -> u32 { self.relation_count }

    fn query_count(&self) -> u32 { self.query_count }

    fn spec_hash(&self) -> Option<&str> { self.spec_hash.as_deref() }
}

/// The LUBM view of the shared [`Generator`] contract.
struct LubmGenerator<'a> {
    inputs: &'a LubmPipelineInputs<'a>,
}

/// What [`LubmGenerator::stage_raw`] leaves behind: the entailment
/// statistics that `meta.json` records.
struct LubmStaged {
    entailment_stats: EntailmentStats,
}

impl Generator for LubmGenerator<'_> {
    type Meta = LubmMeta;
    type Raw = LubmRawArtifacts;
    type Staged = LubmStaged;

    fn target(&self) -> Target<'_> {
        Target {
            out_dir: self.inputs.out_dir,
            bench_name: self.inputs.bench_name,
            tag: self.inputs.tag,
            spec_hash: self.inputs.spec_hash,
        }
    }

    /// Copies the jar output and the hand-written queries verbatim, then
    /// entails the data; the orchestrator partitions the entailed file.
    fn stage_raw(
        &self, raw: &LubmRawArtifacts, raw_root: &Path,
    ) -> Result<(PathBuf, LubmStaged), RdfError> {
        let queries_dir = raw_root.join("queries");
        fs::create_dir_all(&queries_dir)?;
        for spec in self.inputs.queries {
            fs::write(
                queries_dir.join(format!("{}.sparql", spec.name)),
                &spec.sparql,
            )?;
        }
        let raw_data_nt = raw_root.join("data.nt");
        fs::copy(&raw.data_nt, &raw_data_nt)?;
        let entailed_nt = raw_root.join("data.entailed.nt");
        let entailment_stats = entail(&raw_data_nt, &entailed_nt)?;
        Ok((entailed_nt, LubmStaged {
            entailment_stats,
        }))
    }

    fn translate_queries(
        &self, _staged: &LubmStaged, dict: &mut Dictionary, predicate_map: &HashMap<String, String>,
    ) -> Result<Vec<(String, String)>, RdfError> {
        let mut translated: Vec<(String, String)> = Vec::new();
        for spec in self.inputs.queries {
            let head = format!("Q_{}", spec.name);
            let dl = translate_query(&spec.sparql, dict, predicate_map, &head)?;
            translated.push((spec.name.clone(), dl));
        }
        Ok(translated)
    }

    fn description(&self, raw: &LubmRawArtifacts) -> String {
        format!(
            "Lehigh University Benchmark, on-the-fly: scale={}, seed={}, tag={}",
            raw.scale, raw.seed, self.inputs.tag
        )
    }

    /// One CSV per query that carries an expected cardinality.
    fn write_expected(&self, _staged: &LubmStaged, expected_dir: &Path) -> Result<(), RdfError> {
        for spec in self.inputs.queries {
            if let Some(n) = spec.expected_cardinality {
                write_cardinality_csv(&expected_dir.join(format!("{}.csv", spec.name)), n)?;
            }
        }
        Ok(())
    }

    fn build_meta(
        &self, raw: &LubmRawArtifacts, staged: &LubmStaged, _part: &Partitioned,
        provenance: Provenance,
    ) -> Result<LubmMeta, RdfError> {
        let stats = &staged.entailment_stats;
        Ok(LubmMeta {
            schema_version: provenance.schema_version,
            kind: "lubm-onthefly".to_string(),
            scale: raw.scale,
            seed: raw.seed,
            start_index: raw.start_index,
            threads: self.inputs.driver.threads,
            tag: provenance.tag,
            lubm_jar_sha256: sha256_file(self.inputs.driver.jar_path)?,
            ontology_iri: raw.ontology_iri.clone(),
            generated_at_utc: provenance.generated_at_utc,
            triple_count_pre_entailment: stats.input_triples as u64,
            triple_count_post_entailment: stats.output_triples as u64,
            derived_triple_count: stats.derived_triples as u64,
            entailment_iterations: stats.iterations,
            relation_count: provenance.relation_count,
            query_count: provenance.query_count,
            spec_hash: provenance.spec_hash,
        })
    }
}

/// Post-driver stages of the pipeline (entailment, partition, translate,
/// emit). Public so tests can drive these without invoking the jar.
pub fn process_artifacts(
    inputs: &LubmPipelineInputs, raw: &LubmRawArtifacts,
) -> Result<LubmMeta, RdfError> {
    generator::process_artifacts(
        &LubmGenerator {
            inputs,
        },
        raw,
    )
}

/// Top-level entry point: drives the LUBM-UBA jar, then processes artifacts.
pub fn run_lubm_pipeline(inputs: &LubmPipelineInputs) -> Result<LubmMeta, RdfError> {
    let raw = drive(&inputs.driver)?;
    process_artifacts(inputs, &raw)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_expected_cardinality_two_lines() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("q.csv");
        write_cardinality_csv(&p, 42).unwrap();
        let text = fs::read_to_string(&p).unwrap();
        assert_eq!(text, "cardinality\n42\n");
    }
}
