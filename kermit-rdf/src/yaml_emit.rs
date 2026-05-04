//! Emits a kermit `BenchmarkDefinition` YAML for a generated artifact set.

use {
    crate::error::RdfError,
    kermit_bench::{BenchmarkDefinition, QueryDefinition, RelationSource},
    std::{collections::HashSet, path::Path},
};

/// Inputs for emitting a benchmark YAML.
pub struct YamlInputs<'a> {
    /// Benchmark name (matches the cache directory and YAML filename).
    pub name: &'a str,
    /// Human-readable description.
    pub description: &'a str,
    /// All translated queries as `(query_name, datalog)` pairs.
    pub queries: Vec<(String, String)>,
    /// All known predicate names (canonical Datalog names).
    pub all_predicates: &'a [String],
    /// Base URL for relation parquet files (use `file:///abs/path/to/dir`
    /// for on-the-fly generation; the cache layer skips download when files
    /// already exist on disk).
    pub base_url: &'a str,
}

/// Returns the predicate names referenced in any query body.
fn collect_used_predicates(queries: &[(String, String)]) -> HashSet<String> {
    let pat = regex_lite::Regex::new(r"([a-z][a-z0-9_]*)\(").unwrap();
    let mut used = HashSet::new();
    for (_, dl) in queries {
        if let Some((_, body)) = dl.split_once(":-") {
            for cap in pat.captures_iter(body) {
                used.insert(cap[1].to_string());
            }
        }
    }
    used
}

/// Builds a `BenchmarkDefinition`, then writes it to `<dir>/benchmark.yml`.
pub fn write_benchmark_yaml(
    inputs: &YamlInputs, out_dir: &Path,
) -> Result<BenchmarkDefinition, RdfError> {
    let used = collect_used_predicates(&inputs.queries);
    let known: HashSet<&str> = inputs.all_predicates.iter().map(|s| s.as_str()).collect();
    for name in &used {
        if !known.contains(name.as_str()) {
            return Err(RdfError::UnsupportedSparql(format!(
                "query body references unknown predicate: {name}"
            )));
        }
    }
    let mut sorted_used: Vec<&String> = used.iter().collect();
    sorted_used.sort();
    let relations: Vec<RelationSource> = sorted_used
        .iter()
        .map(|name| RelationSource {
            name: (*name).clone(),
            url: format!("{}/{}.parquet", inputs.base_url.trim_end_matches('/'), name),
        })
        .collect();
    let queries: Vec<QueryDefinition> = inputs
        .queries
        .iter()
        .map(|(qname, dl)| QueryDefinition {
            name: qname.clone(),
            description: format!("query {qname}"),
            query: dl.clone(),
        })
        .collect();
    let def = BenchmarkDefinition {
        name: inputs.name.to_string(),
        description: inputs.description.to_string(),
        relations,
        queries,
    };
    def.validate()
        .map_err(|e| RdfError::Expected(e.to_string()))?;
    let yaml = serde_yaml::to_string(&def).map_err(|e| RdfError::Expected(e.to_string()))?;
    std::fs::write(out_dir.join("benchmark.yml"), yaml)?;
    Ok(def)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emits_yaml_with_only_used_predicates() {
        let dir = tempfile::tempdir().unwrap();
        let inputs = YamlInputs {
            name: "test",
            description: "test bench",
            queries: vec![("q0000".into(), "Q_q0000(X) :- follows(X, Y).".into())],
            all_predicates: &["follows".to_string(), "likes".to_string()],
            base_url: "file:///tmp/x",
        };
        let def = write_benchmark_yaml(&inputs, dir.path()).unwrap();
        assert_eq!(def.relations.len(), 1);
        assert_eq!(def.relations[0].name, "follows");
        assert!(dir.path().join("benchmark.yml").exists());
    }

    #[test]
    fn unknown_predicate_in_body_errors() {
        let dir = tempfile::tempdir().unwrap();
        let inputs = YamlInputs {
            name: "test",
            description: "test bench",
            queries: vec![("q0000".into(), "Q_q0000(X) :- ghost(X, Y).".into())],
            all_predicates: &["follows".to_string()],
            base_url: "file:///tmp/x",
        };
        assert!(write_benchmark_yaml(&inputs, dir.path()).is_err());
    }
}
