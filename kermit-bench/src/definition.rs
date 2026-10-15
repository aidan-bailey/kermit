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
/// (`kind: watdiv`, `kind: watdiv-basic`, `kind: lubm` or `kind: snap`).
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
    /// Drives the SNAP generator (`kermit_generate::snap::SnapGenerator`):
    /// one symmetric `edge` relation from a SNAP community edge list, and the
    /// four SNAP queries with their exact counts. Names the graph with
    /// `graph`, or a `url` and the `sha256` of the `.gz` it serves; exactly
    /// one of the two ([`BenchmarkDefinition::validate`] enforces it).
    Snap {
        /// A named graph with a pinned download.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        graph: Option<SnapGraph>,
        /// A SNAP edge-list `.gz` to download instead of a named graph.
        /// Needs `sha256`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        url: Option<String>,
        /// SHA-256 of the `.gz` at `url`, 64 lowercase hex characters. The
        /// spec hash covers this text, not the file, so a `url` must pin
        /// its content here. Forbidden with `graph`, which carries its own.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sha256: Option<String>,
        /// Optional subset of the four queries, in the order to emit them.
        /// `None` or omitted = all four, in [`SnapQuery::ALL`] order.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        queries: Option<Vec<SnapQuery>>,
    },
}

/// A SNAP community graph with a pinned download (Yang & Leskovec, ICDM 2012).
///
/// The YAML and command-line spelling is kebab case (`graph: com-amazon`);
/// [`SnapGraph::name`] returns it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SnapGraph {
    /// `com-amazon`: the Amazon product co-purchasing network.
    ComAmazon,
    /// `com-dblp`: the DBLP collaboration network.
    ComDblp,
    /// `com-youtube`: the YouTube social network.
    ComYoutube,
}

impl SnapGraph {
    /// Every named graph, in this order.
    pub const ALL: [SnapGraph; 3] = [Self::ComAmazon, Self::ComDblp, Self::ComYoutube];

    /// The YAML and CLI spelling: `com-amazon`, `com-dblp`, `com-youtube`.
    pub fn name(self) -> &'static str {
        match self {
            | Self::ComAmazon => "com-amazon",
            | Self::ComDblp => "com-dblp",
            | Self::ComYoutube => "com-youtube",
        }
    }

    /// The pinned download: its URL and the SHA-256 of the `.gz` it serves
    /// (64 lowercase hex characters).
    pub fn source(self) -> (&'static str, &'static str) {
        match self {
            | Self::ComAmazon => (
                "https://snap.stanford.edu/data/bigdata/communities/com-amazon.ungraph.txt.gz",
                "bb4b9fcf549c9174049e885e9a0a4057c9724846e7297c4a1ea299b8e92eec34",
            ),
            | Self::ComDblp => (
                "https://snap.stanford.edu/data/bigdata/communities/com-dblp.ungraph.txt.gz",
                "9eb0bd30312ddd04e2624f7c36c0983a2e99b116f0385be5a7fce6d6170f4cb3",
            ),
            | Self::ComYoutube => (
                "https://snap.stanford.edu/data/bigdata/communities/com-youtube.ungraph.txt.gz",
                "dff1b97ba7d2fa9c59884b67dcd2275e717ff9501f86ed82ce6582ed4971f3e0",
            ),
        }
    }
}

/// One of the four SNAP queries (spec § Workload).
///
/// The YAML and command-line spelling is kebab case (`queries: [triangle,
/// clique4]`); [`SnapQuery::name`] returns it, and it is also the query's name
/// in the generated `benchmark.yml`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SnapQuery {
    /// Six times the triangle count.
    Triangle,
    /// Three-step walks, vertices may repeat.
    Path3,
    /// Closed four-step walks, `tr(A⁴)`.
    Cycle4,
    /// Twenty-four times the 4-clique count.
    Clique4,
}

impl SnapQuery {
    /// Every query, in the default order a spec without `queries` uses.
    pub const ALL: [SnapQuery; 4] = [Self::Triangle, Self::Path3, Self::Cycle4, Self::Clique4];

    /// The YAML, CLI and query name: `triangle`, `path3`, `cycle4`, `clique4`.
    pub fn name(self) -> &'static str {
        match self {
            | Self::Triangle => "triangle",
            | Self::Path3 => "path3",
            | Self::Cycle4 => "cycle4",
            | Self::Clique4 => "clique4",
        }
    }
}

/// The smallest H2O scale: upstream's floor. Below N = 1e7, `small` (N / 1e6
/// keys) has no left-only or right-only key.
pub const H2O_MIN_SCALE: u32 = 10_000_000;

/// The default H2O seed: upstream's `set.seed(108)`.
pub const DEFAULT_H2O_SEED: u32 = 108;

/// The row order of every relation an H2O generator writes (#101's
/// input-order axis).
///
/// The YAML and command-line spelling is kebab case (`order: sorted`);
/// [`H2oOrder::name`] returns it.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Deserialize, serde::Serialize,
)]
#[serde(rename_all = "kebab-case")]
pub enum H2oOrder {
    /// The order the generator draws rows in, as upstream writes them.
    #[default]
    Shuffled,
    /// Each relation's rows sorted lexicographically (`Tuples::sort`), the
    /// order the sorted tries build in. The draws are the same as
    /// `Shuffled`'s.
    Sorted,
}

impl H2oOrder {
    /// Every order, in this order.
    pub const ALL: [H2oOrder; 2] = [Self::Shuffled, Self::Sorted];

    /// The YAML and CLI spelling: `shuffled`, `sorted`.
    pub fn name(self) -> &'static str {
        match self {
            | Self::Shuffled => "shuffled",
            | Self::Sorted => "sorted",
        }
    }
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
            if !is_sha256_hex(digest) {
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
        | GeneratorSpec::Snap {
            graph,
            url,
            sha256,
            queries,
        } => {
            let invalid = |reason: String| BenchError::Invalid {
                name: bench_name.to_string(),
                reason,
            };
            match (graph, url) {
                | (Some(_), Some(_)) => {
                    return Err(invalid(
                        "snap generator takes `graph` or `url`, not both".to_string(),
                    ));
                },
                | (None, None) => {
                    return Err(invalid("snap generator needs `graph` or `url`".to_string()));
                },
                | (Some(_), None) => {
                    if sha256.is_some() {
                        return Err(invalid(
                            "snap generator `sha256` is only for `url`; a named `graph` carries \
                             its own pinned digest"
                                .to_string(),
                        ));
                    }
                },
                | (None, Some(url)) => {
                    if !(url.starts_with("http://") || url.starts_with("https://")) {
                        return Err(invalid(format!(
                            "snap generator url '{url}' must start with http:// or https://"
                        )));
                    }
                    match sha256 {
                        | None => {
                            return Err(invalid(
                                "snap generator `url` requires `sha256`: the spec hash covers the \
                                 spec, not the file"
                                    .to_string(),
                            ));
                        },
                        | Some(digest) if !is_sha256_hex(digest) => {
                            return Err(invalid(format!(
                                "snap generator sha256 must be 64 lowercase hex characters, got \
                                 '{digest}'"
                            )));
                        },
                        | Some(_) => {},
                    }
                },
            }
            if let Some(qs) = queries {
                if qs.is_empty() {
                    return Err(invalid(
                        "snap generator queries list must be non-empty if provided (omit to run \
                         all four)"
                            .to_string(),
                    ));
                }
                let mut seen = HashSet::new();
                for q in qs {
                    if !seen.insert(*q) {
                        return Err(invalid(format!(
                            "snap generator query '{}' is listed twice",
                            q.name()
                        )));
                    }
                }
            }
        },
    }
    Ok(())
}

/// Returns true if `digest` is a SHA-256 as the specs spell it: 64 lowercase
/// hex characters.
fn is_sha256_hex(digest: &str) -> bool {
    digest.len() == 64
        && digest
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
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

    #[test]
    fn snap_names_match_their_serde_spelling() {
        for graph in SnapGraph::ALL {
            assert_eq!(
                serde_yaml::to_string(&graph).unwrap().trim_end(),
                graph.name()
            );
            let parsed: SnapGraph = serde_yaml::from_str(graph.name()).unwrap();
            assert_eq!(parsed, graph);
        }
        for query in SnapQuery::ALL {
            assert_eq!(
                serde_yaml::to_string(&query).unwrap().trim_end(),
                query.name()
            );
            let parsed: SnapQuery = serde_yaml::from_str(query.name()).unwrap();
            assert_eq!(parsed, query);
        }
        // The documented spellings, in the documented order.
        assert_eq!(SnapGraph::ALL.map(SnapGraph::name), [
            "com-amazon",
            "com-dblp",
            "com-youtube"
        ]);
        assert_eq!(SnapQuery::ALL.map(SnapQuery::name), [
            "triangle", "path3", "cycle4", "clique4"
        ]);
    }

    #[test]
    fn snap_graph_sources_are_pinned() {
        let mut urls = HashSet::new();
        let mut digests = HashSet::new();
        for graph in SnapGraph::ALL {
            let (url, sha256) = graph.source();
            assert!(url.starts_with("https://snap.stanford.edu/"), "{url}");
            assert!(
                url.ends_with(&format!("/{}.ungraph.txt.gz", graph.name())),
                "{url} is not the .gz of {}",
                graph.name()
            );
            assert!(
                sha256.len() == 64
                    && sha256
                        .bytes()
                        .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')),
                "{sha256} is not 64 lowercase hex characters"
            );
            assert!(urls.insert(url), "{url} is pinned twice");
            assert!(digests.insert(sha256), "{sha256} is pinned twice");
        }
        // The digests of spec § Measured facts, spelled out so that an edit to
        // one is a failing test rather than a silent re-pin.
        assert_eq!(
            SnapGraph::ComAmazon.source().1,
            "bb4b9fcf549c9174049e885e9a0a4057c9724846e7297c4a1ea299b8e92eec34"
        );
        assert_eq!(
            SnapGraph::ComDblp.source().1,
            "9eb0bd30312ddd04e2624f7c36c0983a2e99b116f0385be5a7fce6d6170f4cb3"
        );
        assert_eq!(
            SnapGraph::ComYoutube.source().1,
            "dff1b97ba7d2fa9c59884b67dcd2275e717ff9501f86ed82ce6582ed4971f3e0"
        );
    }

    #[test]
    fn an_unknown_snap_name_fails_to_parse() {
        for bad in ["com-orkut", "ComAmazon", "com_amazon", ""] {
            let parsed = serde_yaml::from_str::<SnapGraph>(&format!("'{bad}'"));
            assert!(parsed.is_err(), "graph {bad:?} parsed as {parsed:?}");
        }
        for bad in ["path4", "Triangle", "cycle-4", ""] {
            let parsed = serde_yaml::from_str::<SnapQuery>(&format!("'{bad}'"));
            assert!(parsed.is_err(), "query {bad:?} parsed as {parsed:?}");
        }
        // The error names the valid spellings (spec § Testing).
        let err = serde_yaml::from_str::<SnapGraph>("'com-orkut'")
            .unwrap_err()
            .to_string();
        assert!(err.contains("com-amazon"), "{err}");
        let err = serde_yaml::from_str::<SnapQuery>("'path4'")
            .unwrap_err()
            .to_string();
        assert!(err.contains("clique4"), "{err}");
        // One unknown name spoils a whole list; the same list without it
        // parses.
        assert!(serde_yaml::from_str::<Vec<SnapQuery>>("[triangle, nope]").is_err());
        assert_eq!(
            serde_yaml::from_str::<Vec<SnapQuery>>("[triangle, clique4]").unwrap(),
            [SnapQuery::Triangle, SnapQuery::Clique4]
        );
    }

    #[test]
    fn h2o_order_names_match_their_serde_spelling() {
        for order in H2oOrder::ALL {
            assert_eq!(
                serde_yaml::to_string(&order).unwrap().trim_end(),
                order.name()
            );
            let parsed: H2oOrder = serde_yaml::from_str(order.name()).unwrap();
            assert_eq!(parsed, order);
        }
        // The documented spellings, in the documented order.
        assert_eq!(H2oOrder::ALL.map(H2oOrder::name), ["shuffled", "sorted"]);
    }

    #[test]
    fn h2o_order_defaults_to_shuffled() {
        assert_eq!(H2oOrder::default(), H2oOrder::Shuffled);
        assert_eq!(H2oOrder::ALL[0], H2oOrder::default());
        // The other two defaults of an H2O spec: upstream's floor and seed.
        assert_eq!(H2O_MIN_SCALE, 10_000_000);
        assert_eq!(DEFAULT_H2O_SEED, 108);
    }

    #[test]
    fn an_unknown_h2o_order_fails_to_parse() {
        for bad in ["random", "Sorted", ""] {
            let parsed = serde_yaml::from_str::<H2oOrder>(&format!("'{bad}'"));
            assert!(parsed.is_err(), "order {bad:?} parsed as {parsed:?}");
        }
        // The error names the valid spellings.
        let err = serde_yaml::from_str::<H2oOrder>("'random'")
            .unwrap_err()
            .to_string();
        assert!(err.contains("shuffled"), "{err}");
        assert!(err.contains("sorted"), "{err}");
    }

    /// A `GeneratorSpec::Snap` with exactly the fields given.
    fn snap(
        graph: Option<SnapGraph>, url: Option<&str>, sha256: Option<&str>,
        queries: Option<Vec<SnapQuery>>,
    ) -> GeneratorSpec {
        GeneratorSpec::Snap {
            graph,
            url: url.map(str::to_string),
            sha256: sha256.map(str::to_string),
            queries,
        }
    }

    /// The reason `validate_generator` rejects `spec` with; `spec` must be
    /// invalid.
    fn snap_reason(spec: &GeneratorSpec) -> String {
        match validate_generator("s", spec).unwrap_err() {
            | BenchError::Invalid {
                name,
                reason,
            } => {
                assert_eq!(name, "s");
                reason
            },
            | other => panic!("expected Invalid, got {other:?}"),
        }
    }

    #[test]
    fn snap_named_and_url_forms_parse() {
        let named = r#"
name: snap-amazon
description: "amazon"
generator:
  kind: snap
  graph: com-amazon
"#;
        let def: BenchmarkDefinition = serde_yaml::from_str(named).unwrap();
        def.validate().unwrap();
        let named_spec = snap(Some(SnapGraph::ComAmazon), None, None, None);
        assert_eq!(def.generator, Some(named_spec.clone()));
        // Absent fields stay out of the YAML that `spec_hash` covers.
        assert_eq!(
            serde_yaml::to_string(&named_spec).unwrap(),
            "kind: snap\ngraph: com-amazon\n"
        );

        let digest = "ab".repeat(32);
        let by_url = format!(
            r#"
name: snap-mini
description: "mini"
generator:
  kind: snap
  url: https://example.org/data/mini.ungraph.txt.gz
  sha256: '{digest}'
  queries: [triangle, clique4]
"#
        );
        let def: BenchmarkDefinition = serde_yaml::from_str(&by_url).unwrap();
        def.validate().unwrap();
        assert_eq!(
            def.generator,
            Some(snap(
                None,
                Some("https://example.org/data/mini.ungraph.txt.gz"),
                Some(&digest),
                Some(vec![SnapQuery::Triangle, SnapQuery::Clique4]),
            ))
        );
    }

    #[test]
    fn snap_spec_rejects_bad_combinations() {
        let url = "https://example.org/mini.ungraph.txt.gz";
        let digest = "ab".repeat(32);
        let upper = "AB".repeat(32);
        let non_hex = "g".repeat(64);
        let graph = SnapGraph::ComAmazon;

        // The accepted forms, so that each rejection below is one field away
        // from a spec that passes.
        for accepted in [
            snap(Some(graph), None, None, None),
            snap(None, Some(url), Some(&digest), None),
            snap(
                None,
                Some("http://127.0.0.1:8000/m.gz"),
                Some(&digest),
                None,
            ),
            snap(
                Some(graph),
                None,
                None,
                Some(vec![SnapQuery::Clique4, SnapQuery::Triangle]),
            ),
        ] {
            validate_generator("s", &accepted).unwrap();
        }

        let cases = [
            (snap(Some(graph), Some(url), None, None), "not both"),
            (
                snap(Some(graph), Some(url), Some(&digest), None),
                "not both",
            ),
            (snap(None, None, None, None), "needs `graph` or `url`"),
            (
                snap(None, None, Some(&digest), None),
                "needs `graph` or `url`",
            ),
            (snap(None, Some(url), None, None), "requires `sha256`"),
            (
                snap(Some(graph), None, Some(&digest), None),
                "only for `url`",
            ),
            (
                snap(None, Some("ftp://h/m.gz"), Some(&digest), None),
                "http:// or https://",
            ),
            (
                snap(None, Some("/data/m.gz"), Some(&digest), None),
                "http:// or https://",
            ),
            (
                snap(None, Some(url), Some(&upper), None),
                "64 lowercase hex",
            ),
            (
                snap(None, Some(url), Some(&non_hex), None),
                "64 lowercase hex",
            ),
            (
                snap(None, Some(url), Some(&digest[..63]), None),
                "64 lowercase hex",
            ),
            (snap(Some(graph), None, None, Some(vec![])), "non-empty"),
            (
                snap(
                    Some(graph),
                    None,
                    None,
                    Some(vec![
                        SnapQuery::Triangle,
                        SnapQuery::Path3,
                        SnapQuery::Triangle,
                    ]),
                ),
                "listed twice",
            ),
        ];
        for (spec, fragment) in &cases {
            let reason = snap_reason(spec);
            assert!(reason.starts_with("snap generator"), "{reason}");
            assert!(
                reason.contains(fragment),
                "{reason:?} lacks {fragment:?} for {spec:?}"
            );
        }
    }

    #[test]
    fn snap_spec_hash_changes_with_each_field() {
        let url_a = "https://example.org/a.ungraph.txt.gz";
        let url_b = "https://example.org/b.ungraph.txt.gz";
        let digest_a = "ab".repeat(32);
        let digest_b = "cd".repeat(32);
        let amazon = SnapGraph::ComAmazon;
        let specs = [
            snap(Some(amazon), None, None, None),
            snap(Some(SnapGraph::ComDblp), None, None, None),
            snap(Some(amazon), None, None, Some(vec![SnapQuery::Triangle])),
            snap(
                Some(amazon),
                None,
                None,
                Some(vec![SnapQuery::Triangle, SnapQuery::Clique4]),
            ),
            snap(None, Some(url_a), Some(&digest_a), None),
            snap(None, Some(url_b), Some(&digest_a), None),
            snap(None, Some(url_a), Some(&digest_b), None),
            snap(
                None,
                Some(url_a),
                Some(&digest_a),
                Some(vec![SnapQuery::Triangle]),
            ),
        ];
        let hashes: HashSet<String> = specs.iter().map(GeneratorSpec::spec_hash).collect();
        assert_eq!(hashes.len(), specs.len(), "two specs share a spec_hash");
        // A spec hashes the same twice, and a snap spec never hashes as a
        // spec of another kind.
        assert_eq!(specs[0].spec_hash(), specs[0].clone().spec_hash());
        let lubm = GeneratorSpec::Lubm {
            scale: 1,
            seed: 0,
            threads: 1,
            start_index: 0,
            ontology: DEFAULT_LUBM_ONTOLOGY.to_string(),
            queries: None,
        };
        assert!(!hashes.contains(&lubm.spec_hash()));
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
