//! WatDiv end-to-end pipeline.
//!
//! Runs the driver to produce raw watdiv artifacts, then hands them to the
//! shared [`generator::process_artifacts`] orchestrator, which produces the
//! final benchmark cache directory:
//!
//! ```text
//! <out_dir>/
//!   meta.json
//!   benchmark.yml
//!   dict.parquet
//!   <predicate>.parquet × N
//!   raw/data.nt
//!   raw/templates/*.txt
//!   raw/queries/*.sparql (+ *.desc only if the binary emits them — the vendored one does not)
//! ```
//!
//! This module supplies only the WatDiv-specific hooks of the
//! [`Generator`] trait: staging the binary's artifacts, seeding empty
//! relations for query predicates the generated data lacks, splitting the
//! `-q` output on `#end` markers, and the `watdiv-*` provenance fields.

use {
    crate::{
        dict::Dictionary,
        driver::{self, invoke::split_on_end_markers, DriverInputs, RawArtifacts, StressParams},
        error::RdfError,
        generator::{self, Generator, GeneratorMeta, Provenance, Target, TranslatedQuery},
        partition::{self, Partitioned},
        sha256_file,
        sparql::translator::{bgp_predicate_iris, translate_query},
        value::RdfValue,
    },
    serde::Serialize,
    std::{
        collections::{HashMap, HashSet},
        fs,
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

impl GeneratorMeta for PipelineMeta {
    fn schema_version(&self) -> u32 { self.schema_version }

    fn kind(&self) -> &str { &self.kind }

    fn tag(&self) -> &str { &self.tag }

    fn generated_at_utc(&self) -> &str { &self.generated_at_utc }

    fn relation_count(&self) -> u32 { self.relation_count }

    fn query_count(&self) -> u32 { self.query_count }

    fn spec_hash(&self) -> Option<&str> { self.spec_hash.as_deref() }
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

/// Which WatDiv workload `process_artifacts` is finishing. Selects the
/// `meta.json` `kind` discriminator.
pub enum Workload {
    /// Stress workload (`run_pipeline`): watdiv `-s` templates.
    Stress,
    /// Basic Testing workload (`run_basic_pipeline`): static templates.
    Basic,
}

impl Workload {
    /// The `meta.json` `kind` string recorded for this workload.
    fn meta_kind(&self) -> &str {
        match self {
            | Workload::Stress => "watdiv-onthefly",
            | Workload::Basic => "watdiv-basic-onthefly",
        }
    }
}

/// WatDiv draws data (`-d`) and queries (`-s`/`-q`) as independent samples
/// of the model, so any workload can reference a predicate that drew zero
/// triples (rare predicates sit in `<pgroup>` blocks with p < 1 over small
/// entity populations; see issue #63). Seed an empty relation into `part`
/// for each such predicate so translation yields an empty-result join
/// instead of erroring. Mirrors the naming convention used by
/// `partition::partition` for collision-free relation names.
fn seed_missing_predicates(
    part: &mut Partitioned, sparql_paths: &[PathBuf],
) -> Result<(), RdfError> {
    let mut needed: Vec<String> = Vec::new();
    for sparql_path in sparql_paths {
        let text = fs::read_to_string(sparql_path)?;
        for q in split_on_end_markers(&text) {
            for iri in bgp_predicate_iris(&q)? {
                if !needed.contains(&iri) {
                    needed.push(iri);
                }
            }
        }
    }
    let mut used: HashSet<String> = part.relations.iter().map(|r| r.name.clone()).collect();
    for p_iri in needed {
        if part.predicate_map.contains_key(&p_iri) {
            continue;
        }
        let base = partition::sanitize_predicate(&p_iri);
        let pred_id = part.dict.intern(RdfValue::Iri(p_iri.clone()));
        let name = if used.contains(&base) {
            format!("{base}_{pred_id}")
        } else {
            base.clone()
        };
        used.insert(name.clone());
        part.predicate_map.insert(p_iri.clone(), name.clone());
        part.relations.push(partition::PartitionedRelation {
            name,
            tuples: Vec::new(),
        });
    }
    Ok(())
}

/// The WatDiv view of the shared [`Generator`] contract: a set of pipeline
/// inputs plus the workload that decides `kind`.
struct WatdivGenerator<'a> {
    inputs: &'a PipelineInputs<'a>,
    workload: Workload,
}

/// What [`WatdivGenerator::stage_raw`] leaves behind for later hooks: the
/// `.sparql` files as copied under `<out_dir>/raw/queries/`.
struct WatdivStaged {
    copied_sparql_paths: Vec<PathBuf>,
}

impl Generator for WatdivGenerator<'_> {
    type Meta = PipelineMeta;
    type Raw = RawArtifacts;
    type Staged = WatdivStaged;

    fn target(&self) -> Target<'_> {
        Target {
            out_dir: self.inputs.out_dir,
            bench_name: self.inputs.bench_name,
            tag: self.inputs.tag,
            spec_hash: self.inputs.spec_hash,
        }
    }

    fn stage_raw(
        &self, raw: &RawArtifacts, raw_root: &Path,
    ) -> Result<(PathBuf, WatdivStaged), RdfError> {
        fs::create_dir_all(raw_root.join("templates"))?;
        fs::create_dir_all(raw_root.join("queries"))?;

        let data_nt = raw_root.join("data.nt");
        fs::copy(&raw.data_nt, &data_nt)?;
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
        Ok((data_nt, WatdivStaged {
            copied_sparql_paths,
        }))
    }

    fn seed_relations(
        &self, staged: &WatdivStaged, part: &mut Partitioned,
    ) -> Result<(), RdfError> {
        seed_missing_predicates(part, &staged.copied_sparql_paths)
    }

    fn translate_queries(
        &self, staged: &WatdivStaged, dict: &mut Dictionary,
        predicate_map: &HashMap<String, String>,
    ) -> Result<Vec<TranslatedQuery>, RdfError> {
        let mut all_queries: Vec<TranslatedQuery> = Vec::new();
        for sparql_path in &staged.copied_sparql_paths {
            let stem = sparql_path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("q")
                .replace('.', "-");
            let text = fs::read_to_string(sparql_path)?;
            let stem_underscores = stem.replace('-', "_");
            // Watdiv `-q` emits multi-line SPARQL queries separated by `#end`
            // markers; one logical query is a multi-line block, not one line.
            for (i, q) in split_on_end_markers(&text).iter().enumerate() {
                let qname = format!("{stem}_q{i:04}");
                let head = format!("Q_{stem_underscores}_q{i:04}");
                let dl = translate_query(q, dict, predicate_map, &head)?;
                all_queries.push(TranslatedQuery {
                    name: qname,
                    datalog: dl,
                    expected: None,
                });
            }
        }
        Ok(all_queries)
    }

    fn description(&self, _raw: &RawArtifacts) -> String {
        format!(
            "WatDiv on-the-fly generation, scale {}, tag {}",
            self.inputs.driver.scale, self.inputs.tag
        )
    }

    fn build_meta(
        &self, _raw: &RawArtifacts, _staged: &WatdivStaged, part: &Partitioned,
        provenance: Provenance,
    ) -> Result<PipelineMeta, RdfError> {
        let driver = &self.inputs.driver;
        let mut names_hashes = HashMap::new();
        for n in ["firstnames.txt", "lastnames.txt"] {
            let p = driver.vendor_files.join(n);
            names_hashes.insert(n.to_string(), sha256_file(&p)?);
        }
        let triple_count: u64 = part.relations.iter().map(|r| r.tuples.len() as u64).sum();

        Ok(PipelineMeta {
            schema_version: provenance.schema_version,
            kind: self.workload.meta_kind().to_string(),
            scale: driver.scale,
            tag: provenance.tag,
            watdiv_binary_sha256: sha256_file(driver.watdiv_bin)?,
            names_files_sha256: names_hashes,
            model_file_sha256: sha256_file(driver.model_file)?,
            stress_params: StressParamsMeta::from(&driver.stress),
            generated_at_utc: provenance.generated_at_utc,
            triple_count,
            relation_count: provenance.relation_count,
            query_count: provenance.query_count,
            spec_hash: provenance.spec_hash,
        })
    }
}

/// Post-driver stages of the pipeline. Public so the no-binary pipeline
/// integration test can drive them with a hand-crafted
/// `RawArtifacts`-equivalent.
pub fn process_artifacts(
    inputs: &PipelineInputs, raw: &RawArtifacts, workload: Workload,
) -> Result<PipelineMeta, RdfError> {
    generator::process_artifacts(
        &WatdivGenerator {
            inputs,
            workload,
        },
        raw,
    )
}

/// Top-level entry point: runs the stress driver and processes artifacts.
pub fn run_pipeline(inputs: &PipelineInputs) -> Result<PipelineMeta, RdfError> {
    let raw = driver::drive(&inputs.driver)?;
    process_artifacts(inputs, &raw, Workload::Stress)
}

/// Top-level entry point for the **Basic Testing** workload: runs the basic
/// driver (static templates in `template_src_dir`) and processes artifacts.
pub fn run_basic_pipeline(
    inputs: &PipelineInputs, template_src_dir: &Path,
) -> Result<PipelineMeta, RdfError> {
    let raw = driver::drive_basic(&inputs.driver, template_src_dir)?;
    process_artifacts(inputs, &raw, Workload::Basic)
}

#[cfg(test)]
mod tests {
    use {super::*, crate::partition::PartitionedRelation};

    /// A `Partitioned` holding one relation, `title`, for `http://a/title`,
    /// as `partition::partition` would build it from data containing that
    /// predicate.
    fn partitioned_with_title() -> Partitioned {
        let mut part = Partitioned::default();
        let s = part.dict.intern(RdfValue::Iri("http://a/s1".into()));
        part.dict.intern(RdfValue::Iri("http://a/title".into()));
        let o = part.dict.intern(RdfValue::Iri("http://a/o1".into()));
        part.relations.push(PartitionedRelation {
            name: "title".into(),
            tuples: vec![(s, o)],
        });
        part.predicate_map
            .insert("http://a/title".into(), "title".into());
        part
    }

    /// Writes `text` to `<dir>/<name>` and returns its path.
    fn write_sparql(dir: &Path, name: &str, text: &str) -> PathBuf {
        let path = dir.join(name);
        fs::write(&path, text).unwrap();
        path
    }

    #[test]
    fn seed_missing_predicates_adds_empty_relations_for_absent_query_predicates() {
        let dir = tempfile::tempdir().unwrap();
        // One present predicate, one absent predicate, and one absent
        // predicate whose sanitised base (`title`) collides with the
        // present relation's name.
        let sparql = write_sparql(
            dir.path(),
            "q.sparql",
            "SELECT ?s ?o WHERE {\n\
             \t?s <http://a/title> ?o .\n\
             \t?s <http://x/rare> ?r .\n\
             \t?s <http://b/title> ?t .\n\
             }\n#end\n",
        );
        let mut part = partitioned_with_title();
        let dict_len_before = part.dict.len();

        seed_missing_predicates(&mut part, &[sparql]).unwrap();

        // The present relation is untouched.
        assert_eq!(part.relations[0].name, "title");
        assert_eq!(part.relations[0].tuples.len(), 1);
        assert_eq!(part.predicate_map["http://a/title"], "title");

        // Both absent predicates now have an empty relation and a map entry.
        assert_eq!(part.relations.len(), 3, "two relations seeded");
        assert_eq!(part.predicate_map.len(), 3);
        let rare = &part.predicate_map["http://x/rare"];
        assert_eq!(rare, "rare");
        let collided = &part.predicate_map["http://b/title"];
        let collided_id = part
            .dict
            .lookup(&RdfValue::Iri("http://b/title".into()))
            .expect("absent predicate interned into the dictionary");
        assert_eq!(collided, &format!("title_{collided_id}"));
        for name in [rare, collided] {
            let rel = part
                .relations
                .iter()
                .find(|r| &r.name == name)
                .unwrap_or_else(|| panic!("no relation named {name}"));
            assert!(rel.tuples.is_empty(), "{name} must be seeded empty");
        }

        // Exactly the two absent IRIs were interned.
        assert_eq!(part.dict.len(), dict_len_before + 2);
    }

    /// Builds a `WatdivGenerator` for `workload` whose driver paths are never
    /// touched; `seed_relations` reads only `staged`.
    fn generator_for<'a>(
        inputs: &'a PipelineInputs<'a>, workload: Workload,
    ) -> WatdivGenerator<'a> {
        WatdivGenerator {
            inputs,
            workload,
        }
    }

    #[test]
    fn every_workload_seeds_absent_query_predicates() {
        let dir = tempfile::tempdir().unwrap();
        let sparql = write_sparql(
            dir.path(),
            "q.sparql",
            "SELECT ?s ?r WHERE { ?s <http://x/rare> ?r . }\n#end\n",
        );
        let unused = Path::new("unused");
        let inputs = PipelineInputs {
            driver: DriverInputs {
                watdiv_bin: unused,
                vendor_files: unused,
                model_file: unused,
                scale: 1,
                stress: StressParams::default(),
                query_count_per_template: 1,
                use_bwrap: false,
            },
            out_dir: dir.path(),
            bench_name: "unit",
            tag: "unit",
            spec_hash: None,
        };
        let staged = WatdivStaged {
            copied_sparql_paths: vec![sparql],
        };

        for workload in [Workload::Stress, Workload::Basic] {
            let label = match workload {
                | Workload::Stress => "stress",
                | Workload::Basic => "basic",
            };
            let mut part = partitioned_with_title();

            generator_for(&inputs, workload)
                .seed_relations(&staged, &mut part)
                .unwrap();

            let name = part.predicate_map.get("http://x/rare").unwrap_or_else(|| {
                panic!("{label} workload must seed absent query predicates (issue #63)")
            });
            assert_eq!(part.relations.len(), 2, "{label}: one relation seeded");
            let rel = part
                .relations
                .iter()
                .find(|r| &r.name == name)
                .unwrap_or_else(|| panic!("{label}: no relation named {name}"));
            assert!(
                rel.tuples.is_empty(),
                "{label}: {name} must be seeded empty"
            );
        }
    }
}
