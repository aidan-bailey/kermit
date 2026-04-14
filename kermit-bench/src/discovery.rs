use {
    crate::{definition::BenchmarkDefinition, error::BenchError},
    std::path::Path,
};

/// Loads a single benchmark definition by name from the `benchmarks/`
/// directory.
pub fn load_benchmark(
    workspace_root: &Path, name: &str,
) -> Result<BenchmarkDefinition, BenchError> {
    let path = workspace_root
        .join("benchmarks")
        .join(format!("{name}.yml"));
    if !path.exists() {
        return Err(BenchError::NotFound(name.to_string()));
    }

    let contents = std::fs::read_to_string(&path)?;
    let def: BenchmarkDefinition =
        serde_yaml::from_str(&contents).map_err(|source| BenchError::Yaml {
            path: path.clone(),
            source,
        })?;

    if def.name != name {
        return Err(BenchError::Invalid {
            name: def.name.clone(),
            reason: format!(
                "name field '{}' does not match filename '{}'",
                def.name, name
            ),
        });
    }

    def.validate()?;
    Ok(def)
}

/// Loads all benchmark definitions from the `benchmarks/` directory.
pub fn load_all_benchmarks(workspace_root: &Path) -> Result<Vec<BenchmarkDefinition>, BenchError> {
    let dir = workspace_root.join("benchmarks");
    if !dir.exists() {
        return Ok(vec![]);
    }

    let mut benchmarks = Vec::new();
    let mut entries: Vec<_> = std::fs::read_dir(&dir)?.filter_map(|e| e.ok()).collect();
    entries.sort_by_key(|e| e.file_name());

    for entry in entries {
        let path = entry.path();
        let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
        if ext != "yml" && ext != "yaml" {
            continue;
        }

        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        if name.is_empty() {
            continue;
        }

        let benchmark = load_benchmark(workspace_root, &name)?;
        benchmarks.push(benchmark);
    }

    Ok(benchmarks)
}

/// Returns the names of all available benchmarks, sorted alphabetically.
pub fn list_benchmarks(workspace_root: &Path) -> Result<Vec<String>, BenchError> {
    let benchmarks = load_all_benchmarks(workspace_root)?;
    Ok(benchmarks.into_iter().map(|b| b.name).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_benchmark_from_tempdir() {
        let dir = tempfile::tempdir().unwrap();
        let benchmarks_dir = dir.path().join("benchmarks");
        std::fs::create_dir(&benchmarks_dir).unwrap();

        let yaml = r#"
name: test
description: "A test benchmark"
relations:
  - name: r
    url: "https://example.com/r.parquet"
queries:
  - name: default
    description: "Default query"
    query: "Q(X) :- r(X)."
"#;
        std::fs::write(benchmarks_dir.join("test.yml"), yaml).unwrap();

        let def = load_benchmark(dir.path(), "test").unwrap();
        assert_eq!(def.name, "test");
        assert_eq!(def.relations.len(), 1);
        assert_eq!(def.queries.len(), 1);
    }

    #[test]
    fn load_benchmark_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let benchmarks_dir = dir.path().join("benchmarks");
        std::fs::create_dir(&benchmarks_dir).unwrap();

        let result = load_benchmark(dir.path(), "nonexistent");
        assert!(matches!(result, Err(BenchError::NotFound(_))));
    }

    #[test]
    fn load_benchmark_name_mismatch() {
        let dir = tempfile::tempdir().unwrap();
        let benchmarks_dir = dir.path().join("benchmarks");
        std::fs::create_dir(&benchmarks_dir).unwrap();

        let yaml = r#"
name: wrong
description: "Mismatched name"
relations:
  - name: r
    url: "https://example.com/r.parquet"
queries:
  - name: default
    description: "Default query"
    query: "Q(X) :- r(X)."
"#;
        std::fs::write(benchmarks_dir.join("test.yml"), yaml).unwrap();

        let result = load_benchmark(dir.path(), "test");
        assert!(matches!(result, Err(BenchError::Invalid { .. })));
    }

    #[test]
    fn load_all_benchmarks_empty_dir() {
        let dir = tempfile::tempdir().unwrap();
        let benchmarks_dir = dir.path().join("benchmarks");
        std::fs::create_dir(&benchmarks_dir).unwrap();

        let benchmarks = load_all_benchmarks(dir.path()).unwrap();
        assert!(benchmarks.is_empty());
    }

    #[test]
    fn load_all_benchmarks_multiple() {
        let dir = tempfile::tempdir().unwrap();
        let benchmarks_dir = dir.path().join("benchmarks");
        std::fs::create_dir(&benchmarks_dir).unwrap();

        for name in &["alpha", "beta"] {
            let yaml = format!(
                r#"
name: {name}
description: "Benchmark {name}"
relations:
  - name: r
    url: "https://example.com/r.parquet"
queries:
  - name: default
    description: "Default query"
    query: "Q(X) :- r(X)."
"#
            );
            std::fs::write(benchmarks_dir.join(format!("{name}.yml")), yaml).unwrap();
        }

        let benchmarks = load_all_benchmarks(dir.path()).unwrap();
        assert_eq!(benchmarks.len(), 2);
        assert_eq!(benchmarks[0].name, "alpha");
        assert_eq!(benchmarks[1].name, "beta");
    }

    #[test]
    fn load_all_skips_non_yaml() {
        let dir = tempfile::tempdir().unwrap();
        let benchmarks_dir = dir.path().join("benchmarks");
        std::fs::create_dir(&benchmarks_dir).unwrap();

        std::fs::write(benchmarks_dir.join("readme.txt"), "not yaml").unwrap();

        let yaml = r#"
name: real
description: "A real benchmark"
relations:
  - name: r
    url: "https://example.com/r.parquet"
queries:
  - name: default
    description: "Default query"
    query: "Q(X) :- r(X)."
"#;
        std::fs::write(benchmarks_dir.join("real.yml"), yaml).unwrap();

        let benchmarks = load_all_benchmarks(dir.path()).unwrap();
        assert_eq!(benchmarks.len(), 1);
        assert_eq!(benchmarks[0].name, "real");
    }

    #[test]
    fn load_all_no_benchmarks_dir() {
        let dir = tempfile::tempdir().unwrap();
        let benchmarks = load_all_benchmarks(dir.path()).unwrap();
        assert!(benchmarks.is_empty());
    }

    #[test]
    fn list_benchmarks_returns_names() {
        let dir = tempfile::tempdir().unwrap();
        let benchmarks_dir = dir.path().join("benchmarks");
        std::fs::create_dir(&benchmarks_dir).unwrap();

        let yaml = r#"
name: test
description: "test"
relations:
  - name: r
    url: "https://example.com/r.parquet"
queries:
  - name: default
    description: "Default query"
    query: "Q(X) :- r(X)."
"#;
        std::fs::write(benchmarks_dir.join("test.yml"), yaml).unwrap();

        let names = list_benchmarks(dir.path()).unwrap();
        assert_eq!(names, vec!["test"]);
    }
}
