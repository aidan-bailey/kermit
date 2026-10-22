//! The unit of work a measured join runs: a named set of relation files
//! plus named, already-parsed queries.

use {
    crate::execution::{read_relation, read_relation_header},
    kermit::db::{self, validate_query, Catalog, Prepared},
    kermit_algos::{JoinQuery, Planner},
    kermit_bench::BenchmarkDefinition,
    kermit_ds::RelationHeader,
    std::{
        collections::HashSet,
        path::{Path, PathBuf},
    },
};

/// One query of a workload, already parsed.
#[derive(Debug)]
pub struct NamedQuery {
    /// The name that becomes the report's `query` axis and the
    /// `{query}` segment of the Criterion group.
    pub name: String,
    /// The parsed query.
    pub query: JoinQuery,
    /// Expected result cardinality, when the definition declares one; read
    /// only under `--verify`.
    pub expected: Option<u64>,
}

/// What the generic runner measures: relation files (already cached or
/// resolved, read by [`Workload::prepare`]) and the queries to run over
/// them.
#[derive(Debug)]
pub struct Workload {
    /// The benchmark name (the report's `benchmark` axis), or
    /// [`Workload::ADHOC`] for a `bench join` invocation.
    pub name: String,
    /// Relation files, stored as given; nothing here checks they exist —
    /// [`Workload::prepare`] does when it reads their headers.
    pub relation_paths: Vec<PathBuf>,
    /// The queries to run, in report order.
    pub queries: Vec<NamedQuery>,
}

/// A [`Workload`] ready to build and run: every query validated and planned,
/// and the relations the queries name read once into a catalog. No structure
/// is built from them here; each cell builds the indexes a query's plan
/// reads, from the catalog's flat tuples.
pub struct PreparedWorkload {
    /// The workload that was prepared: its name, files and parsed queries.
    pub workload: Workload,
    /// The relations the queries name, as flat tuples with their statistics.
    pub catalog: Catalog,
    /// One per entry of `workload.queries`, in the same order.
    pub prepared: Vec<Prepared>,
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
                    expected: q.expected,
                })
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        Ok(Self {
            name: def.name.clone(),
            relation_paths,
            queries,
        })
    }

    /// Validates every query from the relations' headers, reads only the
    /// relations the queries name into one catalog at the statistics level
    /// `planner` reads, and prepares each query once.
    ///
    /// The headers settle validation, so a query that cannot run fails in
    /// milliseconds, before any tuple is read, even for a workload whose
    /// relations take minutes to load. A relation no query names is never
    /// read. Planning happens here, once per query, so no timed region pays
    /// for it and every cell runs the same plan.
    ///
    /// # Errors
    ///
    /// A relation file whose header or tuples cannot be read, or the first
    /// query that validation, planning or index planning rejects, named with
    /// its benchmark.
    pub fn prepare(self, planner: &Planner) -> anyhow::Result<PreparedWorkload> {
        let headers = self
            .relation_paths
            .iter()
            .map(|path| read_relation_header(path))
            .collect::<anyhow::Result<Vec<RelationHeader>>>()?;
        for q in &self.queries {
            validate_query(&q.query, headers.as_slice()).map_err(|e| {
                anyhow::anyhow!("benchmark '{}' query '{}': {e}", self.name, q.name)
            })?;
        }
        let named: HashSet<&str> = self
            .queries
            .iter()
            .flat_map(|q| q.query.body.iter().map(|atom| atom.name.as_str()))
            .collect();
        let mut relations = Vec::new();
        for (path, header) in self.relation_paths.iter().zip(&headers) {
            if named.contains(header.name()) {
                relations.push(read_relation(path)?);
            }
        }
        let catalog = Catalog::new(relations, planner.required_statistics());
        let prepared = self
            .queries
            .iter()
            .map(|q| {
                db::prepare(&catalog, &q.query, planner).map_err(|e| {
                    anyhow::anyhow!("benchmark '{}' query '{}': {e}", self.name, q.name)
                })
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        Ok(PreparedWorkload {
            workload: self,
            catalog,
            prepared,
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
                expected: None,
            }],
        })
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        kermit_algos::{
            CostBasedOptimiser, LexicographicOptimiser, QueryOptimiser, StatisticsLevel,
        },
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
                sha256: None,
            }],
            queries: vec![
                QueryDefinition {
                    name: "pair".to_string(),
                    description: "pair".to_string(),
                    query: "Q(X, Y) :- edge(X, Y).".to_string(),
                    expected: Some(2),
                },
                QueryDefinition {
                    name: "path".to_string(),
                    description: "path".to_string(),
                    query: "Q(X, Z) :- edge(X, Y), edge(Y, Z).".to_string(),
                    expected: None,
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
        assert_eq!(w.queries[0].expected, Some(2));
        assert_eq!(w.queries[1].expected, None);
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
        assert!(w.queries[0].expected.is_none());
    }

    #[test]
    fn adhoc_rejects_query_path_without_stem() {
        let err = Workload::adhoc(vec![], Path::new("/")).unwrap_err();
        assert!(err.to_string().contains("query path"), "{err}");
    }

    /// A workspace holding `data/edge.csv` (three edges) and `data/unused.csv`,
    /// whose header parses and whose only row does not, so reading its tuples
    /// fails. The definition declares both and carries one query per
    /// `(name, text)` pair.
    fn def_with_an_unreadable_relation(
        queries: &[(&str, &str)],
    ) -> (tempfile::TempDir, BenchmarkDefinition) {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("data")).unwrap();
        fs::write(
            root.path().join("data/edge.csv"),
            "src,dst\n1,2\n2,3\n3,1\n",
        )
        .unwrap();
        fs::write(root.path().join("data/unused.csv"), "a,b\nnot,numbers\n").unwrap();
        let relation = |name: &str| RelationSource {
            name: name.to_string(),
            url: None,
            path: Some(format!("data/{name}.csv")),
            sha256: None,
        };
        let def = BenchmarkDefinition {
            name: "local".to_string(),
            description: "local fixture".to_string(),
            relations: vec![relation("edge"), relation("unused")],
            queries: queries
                .iter()
                .map(|&(name, query)| QueryDefinition {
                    name: name.to_string(),
                    description: name.to_string(),
                    query: query.to_string(),
                    expected: None,
                })
                .collect(),
            generator: None,
        };
        (root, def)
    }

    /// A planner for these tests: the lexicographic optimiser.
    fn free() -> Planner { Planner::new(Box::new(LexicographicOptimiser)) }

    #[test]
    fn prepare_reads_only_the_relations_the_queries_name() {
        let (root, def) = def_with_an_unreadable_relation(&[("pair", "Q(X, Y) :- edge(X, Y).")]);
        let workload = Workload::from_definition(&def, root.path(), None).unwrap();

        let prepared = workload.prepare(&free()).unwrap();

        assert_eq!(prepared.catalog.names().collect::<Vec<_>>(), ["edge"]);
        assert_eq!(prepared.catalog.tuples("edge").unwrap().len(), 3);
        assert_eq!(prepared.prepared.len(), 1);
        assert_eq!(prepared.prepared[0].relations(), ["edge"]);
    }

    /// The control for the test above: `unused.csv` really is unreadable, so a
    /// query that names it fails when its tuples are read.
    #[test]
    fn prepare_reads_a_relation_a_query_names() {
        let (root, def) =
            def_with_an_unreadable_relation(&[("both", "Q(X, Y) :- edge(X, Y), unused(X, Y).")]);
        let workload = Workload::from_definition(&def, root.path(), None).unwrap();

        let err = workload.prepare(&free()).err().unwrap().to_string();

        assert!(err.contains("unused.csv"), "{err}");
        assert!(err.contains("cannot parse"), "{err}");
    }

    /// A query that cannot run is refused from the headers alone: this one
    /// names the unreadable relation, yet the error is the arity mismatch,
    /// not the unparsable row, and it names the benchmark and the query.
    #[test]
    fn prepare_validates_from_headers_before_reading_any_tuples() {
        let (root, def) = def_with_an_unreadable_relation(&[("narrow", "Q(X) :- unused(X).")]);
        let workload = Workload::from_definition(&def, root.path(), None).unwrap();

        let err = workload.prepare(&free()).err().unwrap().to_string();

        assert!(err.contains("benchmark 'local' query 'narrow': "), "{err}");
        assert!(
            err.contains("has 1 term(s), but relation \"unused\" has arity 2"),
            "{err}"
        );
        assert!(!err.contains("cannot parse"), "{err}");
    }

    #[test]
    fn prepare_gathers_the_statistics_the_planner_reads() {
        let (root, def) = def_with_an_unreadable_relation(&[("pair", "Q(X, Y) :- edge(X, Y).")]);
        let level_under = |optimiser: Box<dyn QueryOptimiser>| {
            let workload = Workload::from_definition(&def, root.path(), None).unwrap();
            let planner = Planner::new(optimiser);
            workload.prepare(&planner).unwrap().catalog.level()
        };

        assert_eq!(
            level_under(Box::new(LexicographicOptimiser)),
            StatisticsLevel::TupleCounts
        );
        assert_eq!(
            level_under(Box::<CostBasedOptimiser>::default()),
            StatisticsLevel::ColumnDistinct
        );
    }

    #[test]
    fn prepare_prepares_each_query_in_definition_order() {
        let (root, def) = def_with_an_unreadable_relation(&[
            ("pair", "Q(X, Y) :- edge(X, Y)."),
            ("start", "Q(X) :- edge(X, Y)."),
        ]);
        let workload = Workload::from_definition(&def, root.path(), None).unwrap();

        let prepared = workload.prepare(&free()).unwrap();

        let names: Vec<&str> = prepared
            .workload
            .queries
            .iter()
            .map(|q| q.name.as_str())
            .collect();
        assert_eq!(names, ["pair", "start"]);
        let heads: Vec<usize> = prepared.prepared.iter().map(Prepared::head_len).collect();
        assert_eq!(heads, [2, 1]);
    }
}
