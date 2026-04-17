//! YAML-schema Serde types for benchmark definitions.
//!
//! Each benchmark lives in a single YAML file with one [`BenchmarkDefinition`]
//! at the top level. The schema is documented in the workspace
//! `benchmarks/README.md`.

use {crate::error::BenchError, std::collections::HashSet};

/// A benchmark definition loaded from a YAML file.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct BenchmarkDefinition {
    /// Unique benchmark name. Must match the filename stem.
    pub name: String,
    /// Human-readable description, shown in `bench list` output.
    pub description: String,
    /// The relations referenced by this benchmark's queries.
    pub relations: Vec<RelationSource>,
    /// One or more named queries to run against the relations.
    pub queries: Vec<QueryDefinition>,
}

/// A relation source with a name and download URL.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct RelationSource {
    /// Relation identifier; matched against predicate names in Datalog
    /// queries.
    pub name: String,
    /// HTTP(S) URL of a Parquet file containing the relation's tuples.
    pub url: String,
}

/// A named query within a benchmark.
#[derive(Debug, Clone, serde::Deserialize)]
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
    fn missing_required_field() {
        let yaml = r#"
name: triangle
description: "Triangle query"
relations:
  - name: edge
    url: "https://example.com/edge.parquet"
"#;
        let result: Result<BenchmarkDefinition, _> = serde_yaml::from_str(yaml);
        assert!(result.is_err());
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
        };
        assert!(def.validate().is_err());
    }
}
