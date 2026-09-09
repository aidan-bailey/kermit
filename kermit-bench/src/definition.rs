//! YAML-schema Serde types for benchmark definitions.
//!
//! Each benchmark lives in a single YAML file with one [`BenchmarkDefinition`]
//! at the top level. The schema is documented in the workspace
//! `benchmarks/README.md`.

use {
    crate::error::BenchError,
    std::{
        collections::HashSet,
        path::{Component, Path},
    },
};

/// Number of canonical LUBM queries (`q1`..`q14`, paper Appendix A). The domain
/// constant lives here so the accepted range and its error messages agree.
const LUBM_QUERY_COUNT: u32 = 14;

/// A benchmark definition loaded from a YAML file.
///
/// A benchmark is either *static* — `relations` and `queries` are populated
/// directly from the YAML — or *generated* — `generator` describes how to
/// materialise the data on demand via a `kermit-rdf` pipeline. The two are
/// mutually exclusive; [`BenchmarkDefinition::validate`] enforces the XOR.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct BenchmarkDefinition {
    /// Unique benchmark name. Must match the filename stem.
    pub name: String,
    /// Human-readable description, shown in `bench list` output.
    pub description: String,
    /// The relations referenced by this benchmark's queries. Empty for a
    /// generator-driven YAML; the relations are produced by the generator
    /// pipeline and recorded in the cache-side `benchmark.yml`.
    #[serde(default)]
    pub relations: Vec<RelationSource>,
    /// One or more named queries to run against the relations. Empty for a
    /// generator-driven YAML; the queries are produced by the generator
    /// pipeline.
    #[serde(default)]
    pub queries: Vec<QueryDefinition>,
    /// Optional declarative generator spec. When present, `bench run` runs
    /// the corresponding `kermit-rdf` pipeline on first invocation and
    /// caches the artefacts; subsequent runs short-circuit on the cached
    /// `meta.json` if the spec hash matches.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generator: Option<GeneratorSpec>,
}

/// A declarative generator spec. Tagged on the `kind` field
/// (`kind: watdiv` or `kind: lubm`).
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum GeneratorSpec {
    /// Drives the WatDiv pipeline (`kermit_rdf::pipeline::run_pipeline`).
    Watdiv {
        /// Scale factor passed to `watdiv -d` (>= 1).
        scale: u32,
        /// Stress-template parameters. Defaults to the same values as the
        /// `bench gen watdiv` CLI defaults.
        #[serde(default)]
        stress: WatdivStressSpec,
    },
    /// Drives the WatDiv Basic Testing workload — the 20 canonical L/S/F/C
    /// query templates (`kermit_rdf::pipeline::run_basic_pipeline`). Carries
    /// no stress parameters: the templates are fixed.
    WatdivBasic {
        /// Scale factor passed to `watdiv -d` (>= 1).
        scale: u32,
    },
    /// Drives the LUBM pipeline
    /// (`kermit_rdf::lubm::pipeline::run_lubm_pipeline`).
    Lubm {
        /// Universities to generate (`-u`); must be >= 1.
        scale: u32,
        /// RNG seed (`-s`). Default `0`.
        #[serde(default = "default_lubm_seed")]
        seed: u32,
        /// Worker thread count (`-t`). Default `1` for reproducibility.
        #[serde(default = "default_lubm_threads")]
        threads: u32,
        /// Starting university index (`-i`). Default `0`.
        #[serde(default)]
        start_index: u32,
        /// Ontology IRI (`--ontology`). Default
        /// [`DEFAULT_LUBM_ONTOLOGY`].
        #[serde(default = "default_lubm_ontology")]
        ontology: String,
        /// Optional subset of the 14 LUBM queries to run, by stem
        /// (`q1` … `q14`). `None` or omitted = all 14.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        queries: Option<Vec<String>>,
    },
}

/// WatDiv stress-template parameters. Field defaults match the `bench gen
/// watdiv` CLI defaults.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct WatdivStressSpec {
    /// `<max-query-size>` in stress templates. Default `5`.
    #[serde(default = "default_watdiv_max_query_size")]
    pub max_query_size: u32,
    /// `<query-count>` per template. Default `20`.
    #[serde(default = "default_watdiv_query_count")]
    pub query_count: u32,
    /// `<constants-per-query>`. Default `2`.
    #[serde(default = "default_watdiv_constants_per_query")]
    pub constants_per_query: u32,
    /// `<allow-join-vertex>`. Default `false`.
    #[serde(default)]
    pub allow_join_vertex: bool,
}

impl Default for WatdivStressSpec {
    fn default() -> Self {
        Self {
            max_query_size: default_watdiv_max_query_size(),
            query_count: default_watdiv_query_count(),
            constants_per_query: default_watdiv_constants_per_query(),
            allow_join_vertex: false,
        }
    }
}

/// Default LUBM ontology IRI. Mirrors
/// `kermit_rdf::lubm::driver::DEFAULT_ONTOLOGY_IRI`.
pub const DEFAULT_LUBM_ONTOLOGY: &str = "http://www.lehigh.edu/~zhp2/2004/0401/univ-bench.owl";

fn default_watdiv_max_query_size() -> u32 { 5 }
fn default_watdiv_query_count() -> u32 { 20 }
fn default_watdiv_constants_per_query() -> u32 { 2 }
fn default_lubm_seed() -> u32 { 0 }
fn default_lubm_threads() -> u32 { 1 }
fn default_lubm_ontology() -> String { DEFAULT_LUBM_ONTOLOGY.to_string() }

impl GeneratorSpec {
    /// Computes the canonical SHA-256 hash of this spec, used by the
    /// materialization layer to detect parameter drift against a cached
    /// `meta.json`. Hashes the YAML serialization of the spec; field
    /// ordering is fixed by the struct/enum definition so the output is
    /// deterministic across runs and platforms (no `HashMap` fields).
    ///
    /// # Panics
    ///
    /// Panics if `serde_yaml::to_string` fails for `Self`. The serializer
    /// is total over all `GeneratorSpec` values.
    pub fn spec_hash(&self) -> String {
        use sha2::{Digest, Sha256};
        let yaml = serde_yaml::to_string(self).expect("GeneratorSpec serializes to YAML");
        let mut h = Sha256::new();
        h.update(yaml.as_bytes());
        hex_digest(&h.finalize())
    }
}

/// Lowercase, zero-padded hex encoding of a digest.
///
/// sha2 0.11 returns a `hybrid_array::Array` from `finalize`, which no longer
/// implements `LowerHex`. This reproduces the exact string the previous
/// `format!("{:x}", ..)` produced — the digests are persisted (in `meta.json`,
/// and compared by `spec_hash` drift detection), so the encoding must not
/// shift under a dependency bump.
pub(crate) fn hex_digest(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        write!(out, "{b:02x}").expect("writing to a String never fails");
    }
    out
}

/// Where a relation's tuples come from: either a download URL or a file
/// committed alongside the benchmark.
///
/// Exactly one of `url` and `path` must be set;
/// [`BenchmarkDefinition::validate`] enforces the XOR. `url` suits large or
/// externally-hosted datasets, which are fetched once into the cache; `path`
/// suits small worked examples that should be readable in the repository and
/// runnable with no network (see `benchmarks/triangle.yml`).
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct RelationSource {
    /// Relation identifier; matched against predicate names in Datalog
    /// queries.
    pub name: String,
    /// HTTP(S) URL of a Parquet file containing the relation's tuples.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Workspace-relative path to a committed CSV or Parquet file. The file
    /// stem must equal `name`, because the loaders take the relation's name
    /// from the filename.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// Optional SHA-256 of the relation file, 64 lowercase hex characters.
    /// Checked after a download and by `bench fetch`; never on `bench run`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
}

impl RelationSource {
    /// Whether this relation is committed in the repository rather than
    /// fetched. Local relations need no cache entry and no network.
    #[must_use]
    pub fn is_local(&self) -> bool { self.path.is_some() }
}

/// A named query within a benchmark.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct QueryDefinition {
    /// Query identifier; used to select a specific query via the CLI.
    pub name: String,
    /// Human-readable description shown in `bench list` output.
    pub description: String,
    /// The Datalog query string (see `kermit-parser` for grammar).
    pub query: String,
    /// Expected number of result tuples, when known. Read by `bench run
    /// --verify`; absent means "unknown", not "zero".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected: Option<u64>,
}

impl BenchmarkDefinition {
    /// Validates structural invariants of the benchmark definition.
    ///
    /// Requires the benchmark to declare *either* `relations`+`queries`
    /// (static) *or* a `generator` (generated); the two are mutually
    /// exclusive. `name` must be non-empty and a portable filename (ASCII
    /// alphanumerics plus `.`, `_`, `-`, and never a path separator or `..`).
    /// For static benchmarks, `relations` and `queries` must be non-empty,
    /// every query must have a non-empty `name` and `query`, and relation
    /// names and query names must be unique within the benchmark. For
    /// generated benchmarks, the spec is checked by `validate_generator`
    /// (scale must be >= 1, etc.).
    ///
    /// # Errors
    ///
    /// Returns [`BenchError::Invalid`] describing the first failing
    /// constraint.
    ///
    /// # Example
    ///
    /// ```
    /// use kermit_bench::{BenchmarkDefinition, QueryDefinition, RelationSource};
    ///
    /// let def = BenchmarkDefinition {
    ///     name: "triangle".into(),
    ///     description: "Triangle query".into(),
    ///     relations: vec![RelationSource {
    ///         name: "edge".into(),
    ///         url: Some("https://example.com/edge.parquet".into()),
    ///         path: None,
    ///         sha256: None,
    ///     }],
    ///     queries: vec![QueryDefinition {
    ///         name: "triangle".into(),
    ///         description: "triangle".into(),
    ///         query: "T(X, Y, Z) :- edge(X, Y), edge(Y, Z), edge(X, Z).".into(),
    ///         expected: None,
    ///     }],
    ///     generator: None,
    /// };
    /// assert!(def.validate().is_ok());
    /// ```
    pub fn validate(&self) -> Result<(), BenchError> {
        if self.name.is_empty() {
            return Err(BenchError::Invalid {
                name: self.name.clone(),
                reason: "name must not be empty".to_string(),
            });
        }

        // The name doubles as a cache subdir component (and, for static
        // benchmarks, a downloaded relation's parent dir). The
        // materialization layer also calls `fs::remove_dir_all` on the
        // cache subdir during `--force` regeneration, so the name must not
        // be able to escape it. Restrict to portable filename characters.
        if !is_portable_filename(&self.name) {
            return Err(BenchError::Invalid {
                name: self.name.clone(),
                reason: "name may only contain ASCII alphanumerics, '.', '_', or '-' (no path \
                         separators or '..')"
                    .to_string(),
            });
        }

        let has_static = !self.relations.is_empty() || !self.queries.is_empty();
        match (&self.generator, has_static) {
            | (Some(_), true) => {
                return Err(BenchError::Invalid {
                    name: self.name.clone(),
                    reason: "benchmark cannot mix `generator` with `relations`/`queries`; pick one"
                        .to_string(),
                });
            },
            | (None, false) => {
                return Err(BenchError::Invalid {
                    name: self.name.clone(),
                    reason: "benchmark must declare either `relations`+`queries` or `generator`"
                        .to_string(),
                });
            },
            | (Some(spec), false) => return validate_generator(&self.name, spec),
            | (None, true) => {},
        }

        if self.relations.is_empty() {
            return Err(BenchError::Invalid {
                name: self.name.clone(),
                reason: "relations must not be empty".to_string(),
            });
        }

        if self.queries.is_empty() {
            return Err(BenchError::Invalid {
                name: self.name.clone(),
                reason: "queries must not be empty".to_string(),
            });
        }

        for q in &self.queries {
            if q.name.is_empty() {
                return Err(BenchError::Invalid {
                    name: self.name.clone(),
                    reason: "query name must not be empty".to_string(),
                });
            }
            if q.query.is_empty() {
                return Err(BenchError::Invalid {
                    name: self.name.clone(),
                    reason: format!("query '{}' has empty query string", q.name),
                });
            }
        }

        let mut seen = HashSet::new();
        for rel in &self.relations {
            if !seen.insert(&rel.name) {
                return Err(BenchError::Invalid {
                    name: self.name.clone(),
                    reason: format!("duplicate relation name: {}", rel.name),
                });
            }
            self.validate_relation_source(rel)?;
        }

        seen.clear();
        for q in &self.queries {
            if !seen.insert(&q.name) {
                return Err(BenchError::Invalid {
                    name: self.name.clone(),
                    reason: format!("duplicate query name: {}", q.name),
                });
            }
        }

        Ok(())
    }

    /// Enforces the `url` XOR `path` rule and the constraints a committed
    /// `path` must satisfy.
    ///
    /// A local path is resolved against the workspace root, so it must stay
    /// inside the workspace: absolute paths and `..` components are rejected.
    /// The file stem must equal the relation's `name`, because `from_csv` /
    /// `from_parquet` derive the relation name from the filename — a mismatch
    /// would load a relation the benchmark's queries cannot refer to.
    fn validate_relation_source(&self, rel: &RelationSource) -> Result<(), BenchError> {
        let invalid = |reason: String| BenchError::Invalid {
            name: self.name.clone(),
            reason,
        };

        if let Some(digest) = &rel.sha256 {
            let well_formed = digest.len() == 64
                && digest
                    .bytes()
                    .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
            if !well_formed {
                return Err(invalid(format!(
                    "relation '{}' sha256 must be 64 lowercase hex characters",
                    rel.name
                )));
            }
        }

        let path = match (&rel.url, &rel.path) {
            | (Some(_), Some(_)) => {
                return Err(invalid(format!(
                    "relation '{}' sets both `url` and `path`; pick one",
                    rel.name
                )));
            },
            | (None, None) => {
                return Err(invalid(format!(
                    "relation '{}' must set either `url` (fetched) or `path` (committed)",
                    rel.name
                )));
            },
            | (Some(_), None) => return Ok(()),
            | (None, Some(path)) => Path::new(path),
        };

        if path.is_absolute() || path.components().any(|c| c == Component::ParentDir) {
            return Err(invalid(format!(
                "relation '{}' path must be workspace-relative and must not contain '..'",
                rel.name
            )));
        }

        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default();
        if stem != rel.name {
            return Err(invalid(format!(
                "relation '{}' path has file stem '{stem}'; the loaders take the relation name \
                 from the filename, so the stem must match the relation name",
                rel.name
            )));
        }

        Ok(())
    }
}

fn validate_generator(bench_name: &str, spec: &GeneratorSpec) -> Result<(), BenchError> {
    match spec {
        | GeneratorSpec::Watdiv {
            scale, ..
        } => {
            if *scale == 0 {
                return Err(BenchError::Invalid {
                    name: bench_name.to_string(),
                    reason: "watdiv generator scale must be >= 1".to_string(),
                });
            }
        },
        | GeneratorSpec::WatdivBasic {
            scale,
        } => {
            if *scale == 0 {
                return Err(BenchError::Invalid {
                    name: bench_name.to_string(),
                    reason: "watdiv-basic generator scale must be >= 1".to_string(),
                });
            }
        },
        | GeneratorSpec::Lubm {
            scale,
            queries,
            ontology,
            ..
        } => {
            if *scale == 0 {
                return Err(BenchError::Invalid {
                    name: bench_name.to_string(),
                    reason: "lubm generator scale must be >= 1".to_string(),
                });
            }
            if ontology.is_empty() {
                return Err(BenchError::Invalid {
                    name: bench_name.to_string(),
                    reason: "lubm generator ontology must not be empty".to_string(),
                });
            }
            if let Some(qs) = queries {
                if qs.is_empty() {
                    return Err(BenchError::Invalid {
                        name: bench_name.to_string(),
                        reason: format!(
                            "lubm generator queries list must be non-empty if provided (omit to \
                             run all {LUBM_QUERY_COUNT})"
                        ),
                    });
                }
                let mut seen = HashSet::new();
                for q in qs {
                    if !is_valid_lubm_query_name(q) {
                        return Err(BenchError::Invalid {
                            name: bench_name.to_string(),
                            reason: format!(
                                "lubm generator query '{q}' is not one of q1..q{LUBM_QUERY_COUNT}"
                            ),
                        });
                    }
                    if !seen.insert(q) {
                        return Err(BenchError::Invalid {
                            name: bench_name.to_string(),
                            reason: format!("duplicate lubm query name: {q}"),
                        });
                    }
                }
            }
        },
    }
    Ok(())
}

fn is_valid_lubm_query_name(name: &str) -> bool {
    let Some(rest) = name.strip_prefix('q') else {
        return false;
    };
    // Reject leading zeros (`q01`) so canonical-form `q1`..`q14` is the only
    // accepted spelling. Otherwise `u32::parse` would happily accept padded
    // forms here that fail at materialize time when matched exactly against
    // `lubm_query_specs()`'s canonical names.
    if rest.len() > 1 && rest.starts_with('0') {
        return false;
    }
    matches!(rest.parse::<u32>(), Ok(n) if (1..=LUBM_QUERY_COUNT).contains(&n))
}

/// Returns true if `name` is safe to use as a cache subdir component.
/// Restricts to ASCII alphanumerics plus `.`, `_`, `-`. Rejects anything
/// containing path separators (`/`, `\`) or relative-path tokens (`..`).
fn is_portable_filename(name: &str) -> bool {
    if name == "." || name == ".." || name.is_empty() {
        return false;
    }
    name.chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_query(name: &str, query: &str) -> QueryDefinition {
        QueryDefinition {
            name: name.to_string(),
            description: format!("{name} query"),
            query: query.to_string(),
            expected: None,
        }
    }

    /// A minimal valid static definition: one fetched relation, one query.
    fn valid_static_definition() -> BenchmarkDefinition {
        BenchmarkDefinition {
            name: "static".to_string(),
            description: "static".to_string(),
            relations: vec![RelationSource {
                name: "r".to_string(),
                url: Some("http://x".to_string()),
                path: None,
                sha256: None,
            }],
            queries: vec![make_query("q", "Q(X) :- r(X).")],
            generator: None,
        }
    }

    #[test]
    fn relation_sha256_must_be_64_lowercase_hex() {
        let good = "a".repeat(64);
        let upper = "A".repeat(64);
        let short = "a".repeat(63);
        let non_hex = "g".repeat(64);
        let cases: [(&str, bool); 4] = [
            (&good, true),
            (&upper, false),
            (&short, false),
            (&non_hex, false),
        ];
        for (digest, ok) in cases {
            let mut def = valid_static_definition();
            def.relations[0].sha256 = Some(digest.to_string());
            assert_eq!(def.validate().is_ok(), ok, "{digest}");
        }
    }

    #[test]
    fn query_expected_round_trips_through_yaml() {
        let mut q = make_query("t", "Q(X) :- r(X).");
        q.expected = Some(7);
        let yaml = serde_yaml::to_string(&q).unwrap();
        assert!(yaml.contains("expected: 7"), "{yaml}");
        let back: QueryDefinition = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(back.expected, Some(7));
    }

    #[test]
    fn query_without_expected_serialises_without_the_key() {
        let q = make_query("t", "Q(X) :- r(X).");
        let yaml = serde_yaml::to_string(&q).unwrap();
        assert!(!yaml.contains("expected"), "{yaml}");
    }

    #[test]
    fn legacy_query_yaml_without_expected_deserialises_to_none() {
        let back: QueryDefinition =
            serde_yaml::from_str("name: t\ndescription: d\nquery: 'Q(X) :- r(X).'\n").unwrap();
        assert_eq!(back.expected, None);
    }

    #[test]
    fn deserialize_valid_yaml() {
        let yaml = r#"
name: triangle
description: "Triangle query"
relations:
  - name: edge
    url: "https://example.com/edge.parquet"
queries:
  - name: triangle
    description: "Triangle query"
    query: "T(X, Y, Z) :- edge(X, Y), edge(Y, Z), edge(X, Z)."
"#;
        let def: BenchmarkDefinition = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(def.name, "triangle");
        assert_eq!(def.relations.len(), 1);
        assert_eq!(def.relations[0].name, "edge");
        assert_eq!(def.queries.len(), 1);
        assert_eq!(def.queries[0].name, "triangle");
        assert!(def.validate().is_ok());
        assert!(!def.relations[0].is_local());
    }

    /// The committed-relation form used by `benchmarks/triangle.yml`: a
    /// workspace-relative `path` in place of `url`, needing no network.
    #[test]
    fn deserialize_committed_path_relation() {
        let yaml = r#"
name: triangle
description: "Triangle query over a committed edge relation"
relations:
  - name: edge
    path: "benchmarks/data/triangle/edge.csv"
queries:
  - name: triangle
    description: "Triangle query"
    query: "T(X, Y, Z) :- edge(X, Y), edge(Y, Z), edge(X, Z)."
"#;
        let def: BenchmarkDefinition = serde_yaml::from_str(yaml).unwrap();
        assert!(def.validate().is_ok());
        assert!(def.relations[0].is_local());
        assert!(def.relations[0].url.is_none());
    }

    fn relation_yaml(relation_fields: &str) -> String {
        format!(
            r#"
name: triangle
description: "Triangle query"
relations:
  - name: edge
{relation_fields}
queries:
  - name: triangle
    description: "Triangle query"
    query: "T(X, Y, Z) :- edge(X, Y), edge(Y, Z), edge(X, Z)."
"#
        )
    }

    #[test]
    fn relation_must_set_exactly_one_source() {
        // Neither.
        let def: BenchmarkDefinition = serde_yaml::from_str(&relation_yaml("")).unwrap();
        assert!(def.validate().is_err());

        // Both.
        let yaml = relation_yaml(
            "    url: \"https://example.com/edge.parquet\"\n    path: \
             \"benchmarks/data/triangle/edge.csv\"",
        );
        let def: BenchmarkDefinition = serde_yaml::from_str(&yaml).unwrap();
        assert!(def.validate().is_err());
    }

    /// A local path is joined onto the workspace root, so it must not be able
    /// to address anything outside the workspace.
    #[test]
    fn relation_path_must_stay_inside_the_workspace() {
        for bad in ["/etc/edge.csv", "../../elsewhere/edge.csv"] {
            let def: BenchmarkDefinition =
                serde_yaml::from_str(&relation_yaml(&format!("    path: \"{bad}\""))).unwrap();
            assert!(def.validate().is_err(), "should reject path {bad}");
        }
    }

    /// The CSV/Parquet loaders take the relation name from the filename, so a
    /// stem that disagrees with `name` would load a relation the queries
    /// cannot reference.
    #[test]
    fn relation_path_stem_must_match_relation_name() {
        let def: BenchmarkDefinition = serde_yaml::from_str(&relation_yaml(
            "    path: \"benchmarks/data/triangle/edges.csv\"",
        ))
        .unwrap();
        assert!(def.validate().is_err());
    }

    #[test]
    fn deserialize_multiple_relations() {
        let yaml = r#"
name: path
description: "Path query"
relations:
  - name: edge
    url: "https://example.com/edge.parquet"
  - name: node
    url: "https://example.com/node.parquet"
queries:
  - name: path
    description: "Path query"
    query: "P(X, Z) :- edge(X, Y), node(Y), edge(Y, Z)."
"#;
        let def: BenchmarkDefinition = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(def.relations.len(), 2);
        assert!(def.validate().is_ok());
    }

    #[test]
    fn deserialize_multiple_queries() {
        let yaml = r#"
name: graph
description: "Graph queries"
relations:
  - name: edge
    url: "https://example.com/edge.parquet"
queries:
  - name: triangle
    description: "Triangle query"
    query: "T(X, Y, Z) :- edge(X, Y), edge(Y, Z), edge(X, Z)."
  - name: two-hop
    description: "Two-hop path"
    query: "P(X, Z) :- edge(X, Y), edge(Y, Z)."
"#;
        let def: BenchmarkDefinition = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(def.name, "graph");
        assert_eq!(def.queries.len(), 2);
        assert_eq!(def.queries[0].name, "triangle");
        assert_eq!(def.queries[1].name, "two-hop");
        assert!(def.validate().is_ok());
    }

    #[test]
    fn validate_empty_name() {
        let def = BenchmarkDefinition {
            name: String::new(),
            description: "test".to_string(),
            relations: vec![RelationSource {
                name: "r".to_string(),
                url: Some("http://x".to_string()),
                path: None,
                sha256: None,
            }],
            queries: vec![make_query("q", "Q(X) :- r(X).")],
            generator: None,
        };
        assert!(def.validate().is_err());
    }

    #[test]
    fn validate_empty_relations() {
        let def = BenchmarkDefinition {
            name: "test".to_string(),
            description: "test".to_string(),
            relations: vec![],
            queries: vec![make_query("q", "Q(X) :- r(X).")],
            generator: None,
        };
        assert!(def.validate().is_err());
    }

    #[test]
    fn validate_empty_queries() {
        let def = BenchmarkDefinition {
            name: "test".to_string(),
            description: "test".to_string(),
            relations: vec![RelationSource {
                name: "r".to_string(),
                url: Some("http://x".to_string()),
                path: None,
                sha256: None,
            }],
            queries: vec![],
            generator: None,
        };
        assert!(def.validate().is_err());
    }

    #[test]
    fn validate_empty_query_name() {
        let def = BenchmarkDefinition {
            name: "test".to_string(),
            description: "test".to_string(),
            relations: vec![RelationSource {
                name: "r".to_string(),
                url: Some("http://x".to_string()),
                path: None,
                sha256: None,
            }],
            queries: vec![make_query("", "Q(X) :- r(X).")],
            generator: None,
        };
        assert!(def.validate().is_err());
    }

    #[test]
    fn validate_empty_query_string() {
        let def = BenchmarkDefinition {
            name: "test".to_string(),
            description: "test".to_string(),
            relations: vec![RelationSource {
                name: "r".to_string(),
                url: Some("http://x".to_string()),
                path: None,
                sha256: None,
            }],
            queries: vec![make_query("q", "")],
            generator: None,
        };
        assert!(def.validate().is_err());
    }

    #[test]
    fn validate_duplicate_relation_names() {
        let def = BenchmarkDefinition {
            name: "test".to_string(),
            description: "test".to_string(),
            relations: vec![
                RelationSource {
                    name: "edge".to_string(),
                    url: Some("http://x".to_string()),
                    path: None,
                    sha256: None,
                },
                RelationSource {
                    name: "edge".to_string(),
                    url: Some("http://y".to_string()),
                    path: None,
                    sha256: None,
                },
            ],
            queries: vec![make_query("q", "Q(X) :- edge(X).")],
            generator: None,
        };
        assert!(def.validate().is_err());
    }

    #[test]
    fn validate_duplicate_query_names() {
        let def = BenchmarkDefinition {
            name: "test".to_string(),
            description: "test".to_string(),
            relations: vec![RelationSource {
                name: "r".to_string(),
                url: Some("http://x".to_string()),
                path: None,
                sha256: None,
            }],
            queries: vec![
                make_query("q", "Q(X) :- r(X)."),
                make_query("q", "Q(Y) :- r(Y)."),
            ],
            generator: None,
        };
        assert!(def.validate().is_err());
    }

    #[test]
    fn missing_required_fields_fails_validation() {
        let yaml = r#"
name: triangle
description: "Triangle query"
relations:
  - name: edge
    url: "https://example.com/edge.parquet"
"#;
        let def: BenchmarkDefinition = serde_yaml::from_str(yaml).unwrap();
        assert!(
            def.validate().is_err(),
            "queries-less static benchmark must fail validation"
        );
    }

    #[test]
    fn deserialize_watdiv_generator() {
        let yaml = r#"
name: watdiv-100
description: "watdiv at scale 100"
generator:
  kind: watdiv
  scale: 100
  stress:
    max_query_size: 5
    query_count: 20
    constants_per_query: 2
    allow_join_vertex: false
"#;
        let def: BenchmarkDefinition = serde_yaml::from_str(yaml).unwrap();
        assert!(def.relations.is_empty());
        assert!(def.queries.is_empty());
        match def.generator.as_ref().unwrap() {
            | GeneratorSpec::Watdiv {
                scale,
                stress,
            } => {
                assert_eq!(*scale, 100);
                assert_eq!(stress.query_count, 20);
            },
            | other => panic!("expected watdiv, got {other:?}"),
        }
        assert!(def.validate().is_ok());
    }

    #[test]
    fn deserialize_watdiv_generator_with_default_stress() {
        let yaml = r#"
name: watdiv-1
description: "watdiv default stress"
generator:
  kind: watdiv
  scale: 1
"#;
        let def: BenchmarkDefinition = serde_yaml::from_str(yaml).unwrap();
        match def.generator.as_ref().unwrap() {
            | GeneratorSpec::Watdiv {
                stress, ..
            } => {
                assert_eq!(stress, &WatdivStressSpec::default());
            },
            | _ => panic!("expected watdiv"),
        }
        assert!(def.validate().is_ok());
    }

    #[test]
    fn deserialize_lubm_generator_full() {
        let yaml = r#"
name: lubm-2
description: "lubm scale 2"
generator:
  kind: lubm
  scale: 2
  seed: 7
  threads: 4
  start_index: 1
  ontology: "http://example.com/onto"
  queries: [q1, q3, q14]
"#;
        let def: BenchmarkDefinition = serde_yaml::from_str(yaml).unwrap();
        match def.generator.as_ref().unwrap() {
            | GeneratorSpec::Lubm {
                scale,
                seed,
                threads,
                start_index,
                ontology,
                queries,
            } => {
                assert_eq!(*scale, 2);
                assert_eq!(*seed, 7);
                assert_eq!(*threads, 4);
                assert_eq!(*start_index, 1);
                assert_eq!(ontology, "http://example.com/onto");
                assert_eq!(queries.as_ref().unwrap(), &vec!["q1", "q3", "q14"]);
            },
            | _ => panic!("expected lubm"),
        }
        assert!(def.validate().is_ok());
    }

    #[test]
    fn deserialize_lubm_generator_minimal() {
        let yaml = r#"
name: lubm-1
description: "lubm minimal"
generator:
  kind: lubm
  scale: 1
"#;
        let def: BenchmarkDefinition = serde_yaml::from_str(yaml).unwrap();
        match def.generator.as_ref().unwrap() {
            | GeneratorSpec::Lubm {
                scale,
                seed,
                threads,
                start_index,
                ontology,
                queries,
            } => {
                assert_eq!(*scale, 1);
                assert_eq!(*seed, 0);
                assert_eq!(*threads, 1);
                assert_eq!(*start_index, 0);
                assert_eq!(ontology, DEFAULT_LUBM_ONTOLOGY);
                assert!(queries.is_none());
            },
            | _ => panic!("expected lubm"),
        }
        assert!(def.validate().is_ok());
    }

    #[test]
    fn xor_rejects_generator_with_relations_and_queries() {
        let yaml = r#"
name: hybrid
description: "both"
relations:
  - name: r
    url: "http://x"
queries:
  - name: q
    description: "default"
    query: "Q(X) :- r(X)."
generator:
  kind: watdiv
  scale: 1
"#;
        let def: BenchmarkDefinition = serde_yaml::from_str(yaml).unwrap();
        let err = def.validate().unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("cannot mix"), "expected XOR error, got: {msg}");
    }

    #[test]
    fn xor_rejects_neither_generator_nor_relations() {
        let yaml = r#"
name: empty
description: "nothing"
"#;
        let def: BenchmarkDefinition = serde_yaml::from_str(yaml).unwrap();
        let err = def.validate().unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("must declare"),
            "expected XOR error, got: {msg}"
        );
    }

    #[test]
    fn lubm_invalid_query_name_rejected() {
        let def = BenchmarkDefinition {
            name: "lubm-bad".to_string(),
            description: "bad query".to_string(),
            relations: vec![],
            queries: vec![],
            generator: Some(GeneratorSpec::Lubm {
                scale: 1,
                seed: 0,
                threads: 1,
                start_index: 0,
                ontology: DEFAULT_LUBM_ONTOLOGY.to_string(),
                queries: Some(vec!["q15".to_string()]),
            }),
        };
        assert!(def.validate().is_err());
    }

    #[test]
    fn lubm_zero_scale_rejected() {
        let def = BenchmarkDefinition {
            name: "lubm-zero".to_string(),
            description: "zero scale".to_string(),
            relations: vec![],
            queries: vec![],
            generator: Some(GeneratorSpec::Lubm {
                scale: 0,
                seed: 0,
                threads: 1,
                start_index: 0,
                ontology: DEFAULT_LUBM_ONTOLOGY.to_string(),
                queries: None,
            }),
        };
        assert!(def.validate().is_err());
    }

    #[test]
    fn watdiv_zero_scale_rejected() {
        let def = BenchmarkDefinition {
            name: "watdiv-zero".to_string(),
            description: "zero scale".to_string(),
            relations: vec![],
            queries: vec![],
            generator: Some(GeneratorSpec::Watdiv {
                scale: 0,
                stress: WatdivStressSpec::default(),
            }),
        };
        assert!(def.validate().is_err());
    }

    #[test]
    fn spec_hash_is_deterministic() {
        let a = GeneratorSpec::Watdiv {
            scale: 10,
            stress: WatdivStressSpec::default(),
        };
        let b = GeneratorSpec::Watdiv {
            scale: 10,
            stress: WatdivStressSpec::default(),
        };
        assert_eq!(a.spec_hash(), b.spec_hash());
    }

    #[test]
    fn spec_hash_differs_on_param_change() {
        let a = GeneratorSpec::Watdiv {
            scale: 10,
            stress: WatdivStressSpec::default(),
        };
        let b = GeneratorSpec::Watdiv {
            scale: 20,
            stress: WatdivStressSpec::default(),
        };
        assert_ne!(a.spec_hash(), b.spec_hash());
    }

    #[test]
    fn name_with_path_traversal_rejected() {
        for evil in [
            "../escape",
            "..",
            ".",
            "with/slash",
            "with\\backslash",
            "has space",
            "has:colon",
        ] {
            let def = BenchmarkDefinition {
                name: evil.to_string(),
                description: "x".to_string(),
                relations: vec![RelationSource {
                    name: "r".to_string(),
                    url: Some("http://x".to_string()),
                    path: None,
                    sha256: None,
                }],
                queries: vec![make_query("q", "Q(X) :- r(X).")],
                generator: None,
            };
            assert!(
                def.validate().is_err(),
                "expected rejection for name = {evil:?}"
            );
        }
    }

    #[test]
    fn lubm_zero_padded_query_name_rejected() {
        for bad in ["q01", "q014", "q00", "q001"] {
            let def = BenchmarkDefinition {
                name: "lubm-zero-pad".to_string(),
                description: "x".to_string(),
                relations: vec![],
                queries: vec![],
                generator: Some(GeneratorSpec::Lubm {
                    scale: 1,
                    seed: 0,
                    threads: 1,
                    start_index: 0,
                    ontology: DEFAULT_LUBM_ONTOLOGY.to_string(),
                    queries: Some(vec![bad.to_string()]),
                }),
            };
            assert!(
                def.validate().is_err(),
                "expected rejection for query name = {bad:?}"
            );
        }
    }

    #[test]
    fn deserialize_watdiv_basic_generator() {
        let yaml = r#"
name: watdiv-basic
description: "watdiv basic"
generator:
  kind: watdiv-basic
  scale: 10
"#;
        let def: BenchmarkDefinition = serde_yaml::from_str(yaml).unwrap();
        def.validate().unwrap();
        match def.generator.as_ref().unwrap() {
            | GeneratorSpec::WatdivBasic {
                scale,
            } => assert_eq!(*scale, 10),
            | other => panic!("expected WatdivBasic, got {other:?}"),
        }
    }

    #[test]
    fn watdiv_basic_scale_zero_is_invalid() {
        let spec = GeneratorSpec::WatdivBasic {
            scale: 0,
        };
        assert!(validate_generator("b", &spec).is_err());
    }

    #[test]
    fn watdiv_basic_spec_hash_differs_from_watdiv() {
        let basic = GeneratorSpec::WatdivBasic {
            scale: 10,
        };
        let stress = GeneratorSpec::Watdiv {
            scale: 10,
            stress: WatdivStressSpec::default(),
        };
        assert_ne!(basic.spec_hash(), stress.spec_hash());
    }

    #[test]
    fn spec_hash_distinguishes_lubm_query_subset() {
        let a = GeneratorSpec::Lubm {
            scale: 1,
            seed: 0,
            threads: 1,
            start_index: 0,
            ontology: DEFAULT_LUBM_ONTOLOGY.to_string(),
            queries: None,
        };
        let b = GeneratorSpec::Lubm {
            scale: 1,
            seed: 0,
            threads: 1,
            start_index: 0,
            ontology: DEFAULT_LUBM_ONTOLOGY.to_string(),
            queries: Some(vec!["q1".to_string(), "q2".to_string()]),
        };
        assert_ne!(a.spec_hash(), b.spec_hash());
    }

    /// Pins the encoding against the NIST SHA-256 vector for `"abc"`. The
    /// digests this crate emits are written into `meta.json` and compared on
    /// later runs, so a formatting change here would read as spec drift on
    /// every already-cached benchmark.
    #[test]
    fn hex_digest_matches_the_nist_abc_vector() {
        use sha2::{Digest, Sha256};
        let mut h = Sha256::new();
        h.update(b"abc");
        assert_eq!(
            hex_digest(&h.finalize()),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
