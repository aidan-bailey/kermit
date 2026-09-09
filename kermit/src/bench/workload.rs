//! The unit of work a measured join runs: a named set of relation files
//! plus named, already-parsed queries.

use {
    kermit_algos::JoinQuery,
    kermit_bench::BenchmarkDefinition,
    std::path::{Path, PathBuf},
};

/// One query of a workload, already parsed.
#[derive(Debug)]
pub struct NamedQuery {
    /// The name that becomes the report's `query` axis and the
    /// `{query}` segment of the Criterion group.
    pub name: String,
    /// The parsed query.
    pub query: JoinQuery,
}

/// What the generic runner measures: relation files (already cached or
/// resolved, loaded by the execution family) and the queries to run over
/// them.
#[derive(Debug)]
pub struct Workload {
    /// The benchmark name (the report's `benchmark` axis), or
    /// [`Workload::ADHOC`] for a `bench join` invocation.
    pub name: String,
    /// Relation files, stored as given; nothing here checks they exist —
    /// the family's `load` does.
    pub relation_paths: Vec<PathBuf>,
    /// The queries to run, in report order.
    pub queries: Vec<NamedQuery>,
}

impl Workload {
    /// The `name` of a workload built by [`Workload::adhoc`].
    pub const ADHOC: &'static str = "adhoc";

    /// Builds the workload for `bench run`: caches the definition's
    /// relations (downloading `url:` ones on first use), parses every
    /// query string, and keeps only `query_filter` when given.
    ///
    /// # Errors
    ///
    /// Unknown filter name (the message lists the available names), a
    /// relation that cannot be cached, or a query string that fails to
    /// parse.
    pub fn from_definition(
        def: &BenchmarkDefinition, workspace_root: &Path, query_filter: Option<&str>,
    ) -> anyhow::Result<Self> {
        let selected: Vec<&kermit_bench::QueryDefinition> = match query_filter {
            | Some(name) => {
                let q = def.queries.iter().find(|q| q.name == name).ok_or_else(|| {
                    anyhow::anyhow!(
                        "query '{}' not found in benchmark '{}' (available: {})",
                        name,
                        def.name,
                        def.queries
                            .iter()
                            .map(|q| q.name.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                })?;
                vec![q]
            },
            | None => def.queries.iter().collect(),
        };
        let relation_paths = kermit_bench::cache::ensure_cached(def, workspace_root)
            .map_err(|e| anyhow::anyhow!("Failed to fetch benchmark data: {e}"))?;
        let queries = selected
            .into_iter()
            .map(|q| {
                let query: JoinQuery = q
                    .query
                    .trim()
                    .parse()
                    .map_err(|e| anyhow::anyhow!("Failed to parse query '{}': {e}", q.query))?;
                Ok(NamedQuery {
                    name: q.name.clone(),
                    query,
                })
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        Ok(Self {
            name: def.name.clone(),
            relation_paths,
            queries,
        })
    }

    /// Builds the workload for `bench join`: the given relation paths
    /// verbatim and one query read from `query_path`, named by its file
    /// stem (`triangle.dl` → `triangle`).
    ///
    /// # Errors
    ///
    /// `query_path` has no file stem, cannot be read, or does not parse.
    pub fn adhoc(relation_paths: Vec<PathBuf>, query_path: &Path) -> anyhow::Result<Self> {
        let name = query_path
            .file_stem()
            .and_then(|s| s.to_str())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| anyhow::anyhow!("query path {query_path:?} has no file name"))?
            .to_string();
        let text = std::fs::read_to_string(query_path)
            .map_err(|e| anyhow::anyhow!("Failed to read query file {query_path:?}: {e}"))?;
        let query: JoinQuery = text
            .trim()
            .parse()
            .map_err(|e| anyhow::anyhow!("Failed to parse query from {query_path:?}: {e}"))?;
        Ok(Self {
            name: Self::ADHOC.to_string(),
            relation_paths,
            queries: vec![NamedQuery {
                name,
                query,
            }],
        })
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        kermit_bench::{QueryDefinition, RelationSource},
        std::fs,
    };

    /// A workspace root holding `data/edge.csv`, plus a definition whose
    /// single relation points at it via `path:` so `ensure_cached` never
    /// touches the network.
    fn local_def() -> (tempfile::TempDir, BenchmarkDefinition) {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("data")).unwrap();
        fs::write(root.path().join("data/edge.csv"), "1,2\n2,3\n").unwrap();
        let def = BenchmarkDefinition {
            name: "local".to_string(),
            description: "local fixture".to_string(),
            relations: vec![RelationSource {
                name: "edge".to_string(),
                url: None,
                path: Some("data/edge.csv".to_string()),
            }],
            queries: vec![
                QueryDefinition {
                    name: "pair".to_string(),
                    description: "pair".to_string(),
                    query: "Q(X, Y) :- edge(X, Y).".to_string(),
                },
                QueryDefinition {
                    name: "path".to_string(),
                    description: "path".to_string(),
                    query: "Q(X, Z) :- edge(X, Y), edge(Y, Z).".to_string(),
                },
            ],
            generator: None,
        };
        (root, def)
    }

    #[test]
    fn from_definition_keeps_every_query_in_order() {
        let (root, def) = local_def();
        let w = Workload::from_definition(&def, root.path(), None).unwrap();
        assert_eq!(w.name, "local");
        assert_eq!(w.relation_paths, vec![root.path().join("data/edge.csv")]);
        let names: Vec<&str> = w.queries.iter().map(|q| q.name.as_str()).collect();
        assert_eq!(names, ["pair", "path"]);
    }

    #[test]
    fn from_definition_filter_keeps_exactly_one() {
        let (root, def) = local_def();
        let w = Workload::from_definition(&def, root.path(), Some("path")).unwrap();
        assert_eq!(w.queries.len(), 1);
        assert_eq!(w.queries[0].name, "path");
    }

    #[test]
    fn from_definition_unknown_filter_lists_available_queries() {
        let (root, def) = local_def();
        let err = Workload::from_definition(&def, root.path(), Some("nope")).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("query 'nope' not found in benchmark 'local'"),
            "{msg}"
        );
        assert!(msg.contains("pair, path"), "{msg}");
    }

    #[test]
    fn adhoc_names_query_by_file_stem() {
        let dir = tempfile::tempdir().unwrap();
        let q = dir.path().join("triangle.dl");
        fs::write(&q, "Q(X, Y, Z) :- r(X, Y), s(Y, Z), t(Z, X).").unwrap();
        let rels = vec![PathBuf::from("/x/r.csv"), PathBuf::from("/x/s.csv")];
        let w = Workload::adhoc(rels.clone(), &q).unwrap();
        assert_eq!(w.name, "adhoc");
        assert_eq!(w.relation_paths, rels);
        assert_eq!(w.queries.len(), 1);
        assert_eq!(w.queries[0].name, "triangle");
    }

    #[test]
    fn adhoc_rejects_query_path_without_stem() {
        let err = Workload::adhoc(vec![], Path::new("/")).unwrap_err();
        assert!(err.to_string().contains("query path"), "{err}");
    }
}
