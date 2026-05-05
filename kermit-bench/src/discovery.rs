//! Discovery helpers for benchmark definitions.
//!
//! Reads the workspace's `benchmarks/` directory, parsing every `*.yml` /
//! `*.yaml` file as a [`BenchmarkDefinition`]. The filename stem must match
//! the `name:` field inside the YAML.

use {
    crate::{definition::BenchmarkDefinition, error::BenchError},
    std::path::Path,
};

/// Loads a single benchmark definition by name from the `benchmarks/`
/// directory.
///
/// # Errors
///
/// Returns a [`BenchError`] if:
/// - [`BenchError::NotFound`] — no `<name>.yml` exists under
///   `workspace_root/benchmarks`.
/// - [`BenchError::Io`] — the file exists but cannot be read.
/// - [`BenchError::Yaml`] — the file is not valid YAML or is missing required
///   fields.
/// - [`BenchError::Invalid`] — the YAML parses but the `name:` field does not
///   match the filename or the definition fails
///   [`BenchmarkDefinition::validate`](crate::BenchmarkDefinition::validate).
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
///
/// Returns an empty vector if the directory does not exist. Entries are
/// returned sorted by filename.
///
/// # Errors
///
/// Returns any [`BenchError`] produced by
/// [`load_benchmark`] while reading an individual YAML file. If iteration of
/// the directory itself fails, returns [`BenchError::Io`].
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
///
/// # Errors
///
/// Returns any [`BenchError`] produced by [`load_all_benchmarks`].
///
/// # Example
///
/// ```no_run
/// use {kermit_bench::discovery::list_benchmarks, std::path::Path};
///
/// let names = list_benchmarks(Path::new(".")).unwrap();
/// for name in names {
///     println!("{name}");
/// }
/// ```
pub fn list_benchmarks(workspace_root: &Path) -> Result<Vec<String>, BenchError> {
    let benchmarks = load_all_benchmarks(workspace_root)?;
    Ok(benchmarks.into_iter().map(|b| b.name).collect())
}

/// Loads all benchmarks from the workspace AND the cache root.
///
/// Workspace benchmarks live at `<workspace>/benchmarks/*.yml` (existing
/// behavior). Cache benchmarks live at `<cache_root>/<name>/benchmark.yml`
/// (one self-contained directory per generated benchmark). The two lists
/// are concatenated; cache benchmarks override workspace benchmarks of the
/// same name.
///
/// A cache subdirectory is only consumed if it contains both
/// `benchmark.yml` AND `meta.json`. The `meta.json` marker proves the dir
/// was produced by a kermit generator (e.g. `bench gen watdiv`), so unrelated
/// directories the user may have under `~/.cache/kermit/benchmarks/` are
/// silently ignored.
///
/// # Errors
///
/// Returns any [`BenchError`] produced while reading either root.
pub fn load_all_benchmarks_with_cache(
    workspace_root: &Path, cache_root: &Path,
) -> Result<Vec<BenchmarkDefinition>, BenchError> {
    let mut out = load_all_benchmarks(workspace_root)?;
    let mut existing: std::collections::HashMap<String, usize> = out
        .iter()
        .enumerate()
        .map(|(i, b)| (b.name.clone(), i))
        .collect();
    if !cache_root.exists() {
        return Ok(out);
    }
    let mut entries: Vec<_> = std::fs::read_dir(cache_root)?
        .filter_map(|e| e.ok())
        .collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let yml = path.join("benchmark.yml");
        let meta = path.join("meta.json");
        if !yml.exists() || !meta.exists() {
            continue;
        }
        let contents = std::fs::read_to_string(&yml)?;
        let def: BenchmarkDefinition =
            serde_yaml::from_str(&contents).map_err(|source| BenchError::Yaml {
                path: yml.clone(),
                source,
            })?;
        def.validate()?;
        if let Some(&idx) = existing.get(&def.name) {
            out[idx] = def;
        } else {
            existing.insert(def.name.clone(), out.len());
            out.push(def);
        }
    }
    Ok(out)
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

    #[test]
    fn load_all_with_cache_walks_both_roots() {
        let workspace = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();
        let workspace_benchmarks = workspace.path().join("benchmarks");
        std::fs::create_dir(&workspace_benchmarks).unwrap();

        let yaml_in_workspace = r#"
name: alpha
description: "Workspace bench"
relations:
  - name: r
    url: "https://example.com/r.parquet"
queries:
  - name: q
    description: "default"
    query: "Q(X) :- r(X)."
"#;
        std::fs::write(workspace_benchmarks.join("alpha.yml"), yaml_in_workspace).unwrap();

        let cache_subdir = cache.path().join("beta");
        std::fs::create_dir(&cache_subdir).unwrap();
        let yaml_in_cache = r#"
name: beta
description: "Cached bench"
relations:
  - name: r
    url: "file:///tmp/r.parquet"
queries:
  - name: q
    description: "default"
    query: "Q(X) :- r(X)."
"#;
        std::fs::write(cache_subdir.join("benchmark.yml"), yaml_in_cache).unwrap();
        std::fs::write(cache_subdir.join("meta.json"), "{}").unwrap();

        let names: Vec<String> = load_all_benchmarks_with_cache(workspace.path(), cache.path())
            .unwrap()
            .into_iter()
            .map(|b| b.name)
            .collect();
        assert!(names.contains(&"alpha".to_string()));
        assert!(names.contains(&"beta".to_string()));
    }

    #[test]
    fn cache_dir_without_meta_json_is_ignored() {
        let workspace = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();

        let cache_subdir = cache.path().join("orphan");
        std::fs::create_dir(&cache_subdir).unwrap();
        let yaml_in_cache = r#"
name: orphan
description: "Bench with no marker"
relations:
  - name: r
    url: "file:///tmp/r.parquet"
queries:
  - name: q
    description: "default"
    query: "Q(X) :- r(X)."
"#;
        std::fs::write(cache_subdir.join("benchmark.yml"), yaml_in_cache).unwrap();

        let names: Vec<String> = load_all_benchmarks_with_cache(workspace.path(), cache.path())
            .unwrap()
            .into_iter()
            .map(|b| b.name)
            .collect();
        assert!(
            !names.contains(&"orphan".to_string()),
            "cache dir without meta.json should be ignored, got names = {names:?}"
        );
    }
}
