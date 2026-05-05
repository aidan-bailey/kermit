//! Materialization layer for declarative `generator:` benchmark YAMLs.
//!
//! When `bench run <name>` resolves a [`BenchmarkDefinition`] whose
//! `generator` field is `Some`, the binary is responsible for producing the
//! relation parquets and Datalog queries on demand. The static YAMLs already
//! work end-to-end; the generator-driven path threads through here.
//!
//! Cache layout (per benchmark `<name>`):
//!
//! ```text
//! ~/.cache/kermit/benchmarks/<name>/
//!   meta.json        # PipelineMeta or LubmMeta with spec_hash field
//!   benchmark.yml    # BenchmarkDefinition with file:// URLs
//!   <pred>.parquet × N
//!   ...
//! ```
//!
//! On a cache hit (matching `spec_hash`), [`materialize`] loads the
//! cache-side `benchmark.yml` and returns it. On drift it errors with
//! [`BenchError::SpecDrift`] unless `force` is set, in which case the
//! cache subdir is wiped and the pipeline re-runs.

use {
    kermit_bench::{BenchError, BenchmarkDefinition, GeneratorSpec, WatdivStressSpec},
    std::{
        fs,
        path::{Path, PathBuf},
    },
};

/// Resolves the workspace root by walking up from `CARGO_MANIFEST_DIR`.
pub(crate) fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("kermit crate must be inside workspace")
        .to_path_buf()
}

/// Path to the vendored watdiv source root
/// (`kermit-rdf/vendor/watdiv`). The binary lives at
/// `bin/Release/watdiv` underneath; vendor data files live in `files/`.
pub(crate) fn vendored_watdiv_root() -> PathBuf {
    workspace_root().join("kermit-rdf/vendor/watdiv")
}

/// Path to the vendored LUBM-UBA jar.
pub(crate) fn vendored_lubm_jar() -> PathBuf {
    workspace_root().join("kermit-rdf/vendor/lubm-uba/lubm-uba.jar")
}

/// Resolves the cache subdir for a generator-driven benchmark and either
/// re-uses it (cache hit), errors with [`BenchError::SpecDrift`] (drift,
/// `!force`), or wipes it before regenerating (`force`).
///
/// For a static benchmark (`def.generator.is_none()`), returns `def`
/// unchanged so the caller's existing fetch path takes over.
pub fn materialize(
    def: BenchmarkDefinition, cache_root: &Path, force: bool,
) -> Result<BenchmarkDefinition, BenchError> {
    let Some(spec) = def.generator.as_ref() else {
        return Ok(def);
    };
    let cache_subdir = cache_root.join(&def.name);
    let meta_path = cache_subdir.join("meta.json");
    let yml_path = cache_subdir.join("benchmark.yml");
    let expected_hash = spec.spec_hash();

    if meta_path.exists() {
        let actual_hash = read_meta_spec_hash(&meta_path)?;
        match actual_hash {
            | Some(actual) if actual == expected_hash => {
                return load_cached_yaml(&yml_path);
            },
            | other => {
                if !force {
                    return Err(BenchError::SpecDrift {
                        name: def.name.clone(),
                        expected_hash,
                        actual_hash: other.unwrap_or_else(|| "<missing>".to_string()),
                        hint: "re-run with `bench run --force <name>` to regenerate, or delete \
                               the cache subdir manually"
                            .to_string(),
                    });
                }
                fs::remove_dir_all(&cache_subdir)?;
            },
        }
    }

    fs::create_dir_all(&cache_subdir)?;
    dispatch(spec, &def.name, &cache_subdir, &expected_hash).map_err(|e| BenchError::Invalid {
        name: def.name.clone(),
        reason: format!("generator pipeline failed: {e}"),
    })?;
    load_cached_yaml(&yml_path)
}

/// Loads a `benchmark.yml` from a cache subdir and validates it.
fn load_cached_yaml(yml_path: &Path) -> Result<BenchmarkDefinition, BenchError> {
    let contents = fs::read_to_string(yml_path)?;
    let def: BenchmarkDefinition =
        serde_yaml::from_str(&contents).map_err(|source| BenchError::Yaml {
            path: yml_path.to_path_buf(),
            source,
        })?;
    def.validate()?;
    Ok(def)
}

/// Reads the optional `spec_hash` field from a `meta.json` produced by
/// either the watdiv or lubm pipeline. Returns `Ok(None)` for legacy
/// (schema_version=1) meta files that pre-date the field.
fn read_meta_spec_hash(meta_path: &Path) -> Result<Option<String>, BenchError> {
    let contents = fs::read_to_string(meta_path)?;
    let value: serde_json::Value =
        serde_json::from_str(&contents).map_err(|e| BenchError::Invalid {
            name: meta_path.display().to_string(),
            reason: format!("failed to parse meta.json: {e}"),
        })?;
    Ok(value
        .get("spec_hash")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string()))
}

/// Routes a `GeneratorSpec` to the appropriate `kermit-rdf` pipeline,
/// passing `spec_hash` through so it lands in the produced `meta.json`.
fn dispatch(
    spec: &GeneratorSpec, bench_name: &str, out_dir: &Path, spec_hash: &str,
) -> anyhow::Result<()> {
    match spec {
        | GeneratorSpec::Watdiv {
            scale,
            stress,
        } => run_watdiv(*scale, stress, bench_name, out_dir, spec_hash),
        | GeneratorSpec::Lubm {
            scale,
            seed,
            threads,
            start_index,
            ontology,
            queries,
        } => run_lubm(
            *scale,
            *seed,
            *threads,
            *start_index,
            ontology,
            queries.as_deref(),
            bench_name,
            out_dir,
            spec_hash,
        ),
    }
}

fn run_watdiv(
    scale: u32, stress: &WatdivStressSpec, bench_name: &str, out_dir: &Path, spec_hash: &str,
) -> anyhow::Result<()> {
    let vendor = vendored_watdiv_root();
    let bin = std::env::var_os("KERMIT_WATDIV_BIN")
        .map(PathBuf::from)
        .unwrap_or_else(|| vendor.join("bin/Release/watdiv"));
    if !bin.exists() {
        anyhow::bail!("watdiv binary not found at {bin:?}");
    }
    let driver_stress = kermit_rdf::driver::StressParams {
        max_query_size: stress.max_query_size,
        query_count: stress.query_count,
        constants_per_query: stress.constants_per_query,
        allow_join_vertex: stress.allow_join_vertex,
    };
    let inputs = kermit_rdf::pipeline::PipelineInputs {
        driver: kermit_rdf::driver::DriverInputs {
            watdiv_bin: &bin,
            vendor_files: &vendor.join("files"),
            model_file: &vendor.join("MODEL.txt"),
            scale,
            stress: driver_stress,
            query_count_per_template: stress.query_count,
            use_bwrap: std::env::var_os("KERMIT_NO_BWRAP").is_none(),
        },
        out_dir,
        bench_name,
        tag: bench_name,
        spec_hash: Some(spec_hash),
    };
    kermit_rdf::pipeline::run_pipeline(&inputs)
        .map_err(|e| anyhow::anyhow!("watdiv pipeline failed: {e}"))?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn run_lubm(
    scale: u32, seed: u32, threads: u32, start_index: u32, ontology: &str,
    query_subset: Option<&[String]>, bench_name: &str, out_dir: &Path, spec_hash: &str,
) -> anyhow::Result<()> {
    let jar = std::env::var_os("KERMIT_LUBM_JAR")
        .map(PathBuf::from)
        .unwrap_or_else(vendored_lubm_jar);
    if !jar.exists() {
        anyhow::bail!(
            "LUBM-UBA jar not found at {jar:?}; build with `mvn package` in lubm-uba-rs and copy \
             to kermit-rdf/vendor/lubm-uba/, or override with KERMIT_LUBM_JAR"
        );
    }
    let all_queries = kermit_rdf::lubm::queries::lubm_query_specs(scale == 1);
    let queries: Vec<kermit_rdf::lubm::pipeline::LubmQuerySpec> = match query_subset {
        | Some(names) => {
            let mut selected = Vec::with_capacity(names.len());
            for name in names {
                let q = all_queries
                    .iter()
                    .find(|q| q.name == *name)
                    .ok_or_else(|| anyhow::anyhow!("unknown LUBM query: {name}"))?;
                selected.push(q.clone());
            }
            selected
        },
        | None => all_queries,
    };
    let inputs = kermit_rdf::lubm::pipeline::LubmPipelineInputs {
        driver: kermit_rdf::lubm::driver::LubmDriverInputs {
            jar_path: &jar,
            scale,
            seed,
            start_index,
            threads,
            ontology_iri: ontology,
        },
        out_dir,
        bench_name,
        tag: bench_name,
        queries: &queries,
        spec_hash: Some(spec_hash),
    };
    kermit_rdf::lubm::pipeline::run_lubm_pipeline(&inputs)
        .map_err(|e| anyhow::anyhow!("lubm pipeline failed: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        kermit_bench::{QueryDefinition, RelationSource},
    };

    fn static_def() -> BenchmarkDefinition {
        BenchmarkDefinition {
            name: "static-bench".to_string(),
            description: "static".to_string(),
            relations: vec![RelationSource {
                name: "edge".to_string(),
                url: "file:///tmp/x.parquet".to_string(),
            }],
            queries: vec![QueryDefinition {
                name: "q".to_string(),
                description: "q".to_string(),
                query: "Q(X) :- edge(X, Y).".to_string(),
            }],
            generator: None,
        }
    }

    fn write_meta_and_yaml(subdir: &Path, spec_hash_in_meta: Option<&str>) {
        fs::create_dir_all(subdir).unwrap();
        let meta = match spec_hash_in_meta {
            | Some(h) => serde_json::json!({
                "schema_version": 2,
                "kind": "watdiv-onthefly",
                "spec_hash": h,
            }),
            | None => serde_json::json!({
                "schema_version": 1,
                "kind": "watdiv-onthefly",
            }),
        };
        fs::write(
            subdir.join("meta.json"),
            serde_json::to_string_pretty(&meta).unwrap(),
        )
        .unwrap();
        let yaml = r#"
name: cached-bench
description: "cached benchmark"
relations:
  - name: edge
    url: "file:///tmp/edge.parquet"
queries:
  - name: q
    description: "default"
    query: "Q(X) :- edge(X, Y)."
"#;
        fs::write(subdir.join("benchmark.yml"), yaml).unwrap();
    }

    #[test]
    fn static_benchmarks_pass_through_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let def = static_def();
        let original_name = def.name.clone();
        let out = materialize(def, dir.path(), false).unwrap();
        assert_eq!(out.name, original_name);
        assert!(out.generator.is_none());
        // No cache subdir created for static benchmarks
        assert!(!dir.path().join(original_name).exists());
    }

    #[test]
    fn cache_hit_loads_cached_yaml() {
        let dir = tempfile::tempdir().unwrap();
        let spec = GeneratorSpec::Watdiv {
            scale: 5,
            stress: WatdivStressSpec::default(),
        };
        let hash = spec.spec_hash();
        let subdir = dir.path().join("cached-bench");
        write_meta_and_yaml(&subdir, Some(&hash));

        let def = BenchmarkDefinition {
            name: "cached-bench".to_string(),
            description: "x".to_string(),
            relations: vec![],
            queries: vec![],
            generator: Some(spec),
        };
        let out = materialize(def, dir.path(), false).unwrap();
        assert_eq!(out.name, "cached-bench");
        assert!(
            out.generator.is_none(),
            "cached YAML is post-materialization"
        );
        assert_eq!(out.relations.len(), 1);
        assert_eq!(out.queries.len(), 1);
    }

    #[test]
    fn drift_without_force_errors_with_specdrift() {
        let dir = tempfile::tempdir().unwrap();
        let subdir = dir.path().join("drifted-bench");
        write_meta_and_yaml(&subdir, Some("stale-hash"));

        let def = BenchmarkDefinition {
            name: "drifted-bench".to_string(),
            description: "x".to_string(),
            relations: vec![],
            queries: vec![],
            generator: Some(GeneratorSpec::Watdiv {
                scale: 5,
                stress: WatdivStressSpec::default(),
            }),
        };
        let err = materialize(def, dir.path(), false).unwrap_err();
        match err {
            | BenchError::SpecDrift {
                name,
                actual_hash,
                ..
            } => {
                assert_eq!(name, "drifted-bench");
                assert_eq!(actual_hash, "stale-hash");
            },
            | other => panic!("expected SpecDrift, got {other:?}"),
        }
        // Cache subdir must still exist — drift must NOT delete data without
        // explicit consent.
        assert!(subdir.exists());
    }

    #[test]
    fn legacy_meta_without_spec_hash_treated_as_drift() {
        let dir = tempfile::tempdir().unwrap();
        let subdir = dir.path().join("legacy-bench");
        write_meta_and_yaml(&subdir, None);

        let def = BenchmarkDefinition {
            name: "legacy-bench".to_string(),
            description: "x".to_string(),
            relations: vec![],
            queries: vec![],
            generator: Some(GeneratorSpec::Watdiv {
                scale: 5,
                stress: WatdivStressSpec::default(),
            }),
        };
        let err = materialize(def, dir.path(), false).unwrap_err();
        assert!(matches!(err, BenchError::SpecDrift { .. }));
    }
}
