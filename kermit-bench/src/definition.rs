//! YAML-schema Serde types for benchmark definitions.
//!
//! Each benchmark lives in a single YAML file with one [`BenchmarkDefinition`]
//! at the top level. The schema is documented in the workspace
//! `benchmarks/README.md`.

use {crate::error::BenchError, std::collections::HashSet};

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
        format!("{:x}", h.finalize())
    }
}

/// A relation source with a name and download URL.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct RelationSource {
    /// Relation identifier; matched against predicate names in Datalog
    /// queries.
    pub name: String,
    /// HTTP(S) URL of a Parquet file containing the relation's tuples.
    pub url: String,
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
}

impl BenchmarkDefinition {
    /// Validates structural invariants of the benchmark definition.
    ///
    /// Checks that `name`, `relations`, and `queries` are non-empty, that
    /// every query has a non-empty `name` and `query`, and that relation
    /// names and query names are unique within the benchmark.
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
    ///         url: "https://example.com/edge.parquet".into(),
    ///     }],
    ///     queries: vec![QueryDefinition {
    ///         name: "triangle".into(),
    ///         description: "triangle".into(),
    ///         query: "T(X, Y, Z) :- edge(X, Y), edge(Y, Z), edge(X, Z).".into(),
    ///     }],
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
                        reason: "lubm generator queries list must be non-empty if provided (omit \
                                 to run all 14)"
                            .to_string(),
                    });
                }
                let mut seen = HashSet::new();
                for q in qs {
                    if !is_valid_lubm_query_name(q) {
                        return Err(BenchError::Invalid {
                            name: bench_name.to_string(),
                            reason: format!("lubm generator query '{q}' is not one of q1..q14"),
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
    matches!(rest.parse::<u32>(), Ok(n) if (1..=14).contains(&n))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_query(name: &str, query: &str) -> QueryDefinition {
        QueryDefinition {
            name: name.to_string(),
            description: format!("{name} query"),
            query: query.to_string(),
        }
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
                url: "http://x".to_string(),
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
                url: "http://x".to_string(),
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
                url: "http://x".to_string(),
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
                url: "http://x".to_string(),
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
                    url: "http://x".to_string(),
                },
                RelationSource {
                    name: "edge".to_string(),
                    url: "http://y".to_string(),
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
                url: "http://x".to_string(),
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
}
