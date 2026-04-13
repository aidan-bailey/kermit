use {crate::error::BenchError, std::collections::HashSet};

/// A benchmark definition loaded from a YAML file.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct BenchmarkDefinition {
    pub name: String,
    pub description: String,
    pub relations: Vec<RelationSource>,
    pub query: String,
}

/// A relation source with a name and download URL.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct RelationSource {
    pub name: String,
    pub url: String,
}

impl BenchmarkDefinition {
    /// Validates structural invariants of the benchmark definition.
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

        if self.query.is_empty() {
            return Err(BenchError::Invalid {
                name: self.name.clone(),
                reason: "query must not be empty".to_string(),
            });
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

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserialize_valid_yaml() {
        let yaml = r#"
name: triangle
description: "Triangle query"
relations:
  - name: edge
    url: "https://example.com/edge.parquet"
query: "T(X, Y, Z) :- edge(X, Y), edge(Y, Z), edge(X, Z)."
"#;
        let def: BenchmarkDefinition = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(def.name, "triangle");
        assert_eq!(def.relations.len(), 1);
        assert_eq!(def.relations[0].name, "edge");
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
query: "P(X, Z) :- edge(X, Y), node(Y), edge(Y, Z)."
"#;
        let def: BenchmarkDefinition = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(def.relations.len(), 2);
        assert!(def.validate().is_ok());
    }

    #[test]
    fn missing_required_field() {
        let yaml = r#"
name: triangle
description: "Triangle query"
query: "T(X, Y, Z) :- edge(X, Y), edge(Y, Z), edge(X, Z)."
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
            query: "Q(X) :- r(X).".to_string(),
        };
        assert!(def.validate().is_err());
    }

    #[test]
    fn validate_empty_relations() {
        let def = BenchmarkDefinition {
            name: "test".to_string(),
            description: "test".to_string(),
            relations: vec![],
            query: "Q(X) :- r(X).".to_string(),
        };
        assert!(def.validate().is_err());
    }

    #[test]
    fn validate_empty_query() {
        let def = BenchmarkDefinition {
            name: "test".to_string(),
            description: "test".to_string(),
            relations: vec![RelationSource {
                name: "r".to_string(),
                url: "http://x".to_string(),
            }],
            query: String::new(),
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
            query: "Q(X) :- edge(X).".to_string(),
        };
        assert!(def.validate().is_err());
    }
}
