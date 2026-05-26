//! Kermit command-line interface.
//!
//! Two top-level subcommands: `join` (execute a Datalog query against
//! relation files) and `bench` (Criterion-based benchmarks, including the
//! YAML-defined benchmarks under `benchmarks/`).
//!
//! Run `kermit --help` for the full help text; each `clap` `#[arg(help =
//! …)]` string drives that help output. Benchmark YAML schema is documented
//! in the workspace `benchmarks/README.md`.

#![deny(missing_docs)]

use {
    anyhow::Context,
    clap::{Args, Parser, Subcommand},
    kermit::db::instantiate_database,
    kermit_algos::{JoinAlgorithm, JoinQuery},
    kermit_bench::BenchmarkDefinition,
    kermit_ds::{HeapSize, IndexStructure, Relation, RelationFileExt},
    kermit_iters::TrieIterable,
    kermit_parser::Term,
    std::{
        collections::BTreeMap,
        fs,
        io::{self, BufWriter, Write},
        path::{Path, PathBuf},
        time::Duration,
    },
};

mod bench_report;
mod materialize;
mod measurement;

use bench_report::{
    write_json_report, write_metadata_block, BenchKind, BenchReport, CriterionGroupRef,
    MetadataLine, ReportMetric,
};

/// Default Criterion group name when `--name` is omitted on `bench run`.
/// `bench run` treats `--name` as a *prefix* on the auto-generated
/// `{benchmark}/{query}/{ds}/{algo}` identity (see CLAUDE.md "bench `--name`
/// semantics").
const DEFAULT_RUN_GROUP: &str = "run";

/// Default Criterion group name for `bench join` (full group name, not a
/// prefix).
const DEFAULT_JOIN_GROUP: &str = "join";

/// Default Criterion group name for `bench ds` (full group name, not a
/// prefix).
const DEFAULT_DS_GROUP: &str = "ds";

#[derive(Parser)]
#[command(name = "kermit")]
#[command(version, about = "Relational data structures, iterators and algorithms", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Args)]
struct QueryArgs {
    /// Input relation data paths (files or directories)
    #[arg(short, long, value_name = "PATH", num_args = 1.., required = true)]
    relations: Vec<PathBuf>,

    /// Query file path
    #[arg(short, long, value_name = "PATH", required = true)]
    query: PathBuf,

    /// Join algorithm
    #[arg(short, long, value_name = "ALGORITHM", required = true, value_enum)]
    algorithm: JoinAlgorithm,

    /// Data structure
    #[arg(
        short,
        long,
        value_name = "INDEXSTRUCTURE",
        required = true,
        value_enum
    )]
    indexstructure: IndexStructure,
}

#[derive(Copy, Clone, Debug, PartialEq, clap::ValueEnum)]
enum Metric {
    Insertion,
    Iteration,
    Space,
}

/// CLI-side selector for `--indexstructure`. Wraps [`IndexStructure`] with
/// an `All` variant that expands to every concrete index structure, so
/// users can sweep without enumerating each one. The underlying
/// `IndexStructure` enum (in `kermit-ds`) stays free of CLI concerns.
#[derive(Copy, Clone, Debug, PartialEq, clap::ValueEnum)]
enum IndexStructureSelector {
    All,
    ColumnTrie,
    HashTrie,
    TreeTrie,
}

impl IndexStructureSelector {
    fn expand(self) -> Vec<IndexStructure> {
        use clap::ValueEnum;
        match self {
            | Self::All => IndexStructure::value_variants().to_vec(),
            | Self::ColumnTrie => vec![IndexStructure::ColumnTrie],
            | Self::HashTrie => vec![IndexStructure::HashTrie],
            | Self::TreeTrie => vec![IndexStructure::TreeTrie],
        }
    }

    /// Returns whether this index-structure selector is compatible with
    /// the given algorithm selector.
    ///
    /// Used to reject invalid CLI argument combinations (such as
    /// `--indexstructure hash-trie --algorithm leapfrog-triejoin`) at
    /// parse time, before launching any work. The `All` selector on
    /// either side is permissive — the cross-product caller in `bench
    /// run` filters individual `(IndexStructure, JoinAlgorithm)` pairs
    /// at expand time.
    fn supports_algorithm(self, algo: JoinAlgorithmSelector) -> bool {
        match (self, algo) {
            // `All` permits everything; the cross-product caller filters
            // at expand time.
            | (IndexStructureSelector::All, _) | (_, JoinAlgorithmSelector::All) => true,
            // Hash family: hash trie pairs only with hash triejoin.
            | (IndexStructureSelector::HashTrie, JoinAlgorithmSelector::HashTriejoin) => true,
            // Sorted family: sorted tries pair only with leapfrog triejoin.
            | (
                IndexStructureSelector::TreeTrie | IndexStructureSelector::ColumnTrie,
                JoinAlgorithmSelector::LeapfrogTriejoin,
            ) => true,
            // Anything else is incompatible.
            | _ => false,
        }
    }
}

/// CLI-side selector for `--algorithm`. Wraps [`JoinAlgorithm`] with an
/// `All` variant for sweeps. The selector exists so adding new algorithms
/// is purely additive — `All` resolves through
/// `clap::ValueEnum::value_variants()`, so a new variant on
/// `JoinAlgorithm` automatically joins the sweep without touching this
/// match.
#[derive(Copy, Clone, Debug, PartialEq, clap::ValueEnum)]
enum JoinAlgorithmSelector {
    All,
    HashTriejoin,
    LeapfrogTriejoin,
}

impl JoinAlgorithmSelector {
    fn expand(self) -> Vec<JoinAlgorithm> {
        use clap::ValueEnum;
        match self {
            | Self::All => JoinAlgorithm::value_variants().to_vec(),
            | Self::HashTriejoin => vec![JoinAlgorithm::HashTriejoin],
            | Self::LeapfrogTriejoin => vec![JoinAlgorithm::LeapfrogTriejoin],
        }
    }
}

#[derive(Args)]
struct BenchArgs {
    /// Name for the Criterion benchmark group
    #[arg(short, long, value_name = "NAME")]
    name: Option<String>,

    /// Number of samples to collect (min 10)
    #[arg(long, value_name = "N", default_value = "100")]
    sample_size: usize,

    /// Measurement time per sample in seconds
    #[arg(long, value_name = "SECS", default_value = "5")]
    measurement_time: u64,

    /// Warm-up time in seconds before sampling
    #[arg(long, value_name = "SECS", default_value = "3")]
    warm_up_time: u64,

    /// Override the path of the machine-readable JSON report. Default:
    /// `bench-runs/<kind>-<unix-millis>.json` (parent dir auto-created;
    /// `bench-runs/` is gitignored at the workspace root).
    #[arg(long, value_name = "PATH")]
    report_json: Option<PathBuf>,
}

#[derive(Subcommand)]
enum BenchSubcommand {
    /// Benchmark a join query.
    ///
    /// Note: `bench join` does not currently support `-i all` / `-a all`
    /// because it shares its argument struct with the one-shot top-level
    /// `kermit join` command. For sweeps, use `bench run` against a YAML
    /// benchmark instead.
    Join {
        #[command(flatten)]
        query_args: QueryArgs,

        /// Output file for one run's results (optional)
        #[arg(short, long, value_name = "PATH")]
        output: Option<PathBuf>,
    },

    /// Benchmark an index structure (insertion, iteration, space)
    Ds {
        /// Input relation data path (single file)
        #[arg(short, long, value_name = "PATH", required = true)]
        relation: PathBuf,

        /// Data structure (`all` sweeps every available structure)
        #[arg(
            short,
            long,
            value_name = "INDEXSTRUCTURE",
            required = true,
            value_enum
        )]
        indexstructure: IndexStructureSelector,

        /// Metrics to benchmark
        #[arg(
            short,
            long,
            value_enum,
            num_args = 1..,
            default_values_t = vec![Metric::Insertion, Metric::Iteration, Metric::Space]
        )]
        metrics: Vec<Metric>,
    },

    /// Run a named benchmark from benchmarks/ YAML files
    Run {
        /// Benchmark name (omit for --all)
        #[arg(value_name = "NAME")]
        name: Option<String>,

        /// Run all available benchmarks
        #[arg(long, conflicts_with = "name")]
        all: bool,

        /// Run only the named query (omit to run all queries)
        #[arg(short, long, value_name = "QUERY")]
        query: Option<String>,

        /// Data structure (`all` sweeps every available structure)
        #[arg(
            short,
            long,
            value_name = "INDEXSTRUCTURE",
            required = true,
            value_enum
        )]
        indexstructure: IndexStructureSelector,

        /// Join algorithm (`all` sweeps every available algorithm)
        #[arg(short, long, value_name = "ALGORITHM", required = true, value_enum)]
        algorithm: JoinAlgorithmSelector,

        /// Metrics to benchmark
        #[arg(
            short,
            long,
            value_enum,
            num_args = 1..,
            default_values_t = vec![Metric::Insertion, Metric::Iteration, Metric::Space]
        )]
        metrics: Vec<Metric>,

        /// Regenerate generator-driven benchmarks if their cached
        /// `meta.json` spec_hash differs from the current YAML's params.
        /// Without this flag, `bench run` errors out on drift instead of
        /// silently re-running an expensive pipeline. No-op for static
        /// benchmarks.
        #[arg(long)]
        force: bool,
    },

    /// List available benchmarks
    List,

    /// Fetch (download) benchmark data files
    Fetch {
        /// Benchmark name (omit to fetch all)
        #[arg(value_name = "NAME")]
        name: Option<String>,
    },

    /// Clean cached benchmark data files
    Clean {
        /// Benchmark name (omit to clean all)
        #[arg(value_name = "NAME")]
        name: Option<String>,
    },

    /// Generate a fresh benchmark on the fly
    Gen {
        #[command(subcommand)]
        subcommand: GenSubcommand,
    },
}

#[derive(Subcommand)]
enum GenSubcommand {
    /// Generate a fresh watdiv benchmark on the fly
    Watdiv {
        /// Scale factor passed to watdiv -d (>= 1)
        #[arg(long, value_name = "N", required = true)]
        scale: u32,

        /// Tag appended to the benchmark name; must contain a non-numeric
        /// character so it cannot collide with committed snapshot names
        #[arg(long, value_name = "STRING", required = true)]
        tag: String,

        /// max-query-size for stress templates (default 5)
        #[arg(long, value_name = "N", default_value = "5")]
        max_query_size: u32,

        /// concrete queries per template (default 20)
        #[arg(long, value_name = "N", default_value = "20")]
        query_count: u32,

        /// constants per query (default 2)
        #[arg(long, value_name = "N", default_value = "2")]
        constants_per_query: u32,

        /// allow join-vertex (default false)
        #[arg(long)]
        allow_join_vertex: bool,

        /// Override the watdiv binary path (default: vendored)
        #[arg(long, value_name = "PATH", env = "KERMIT_WATDIV_BIN")]
        watdiv_bin: Option<PathBuf>,

        /// Override the cache dir parent (default: ~/.cache/kermit/benchmarks).
        /// NOTE: benchmarks generated outside the default cache are NOT
        /// auto-discovered by `bench list/fetch/run`; mainly useful for tests.
        #[arg(long, value_name = "PATH")]
        output_dir: Option<PathBuf>,

        /// Skip bwrap sandbox; require host /usr/share/dict/words
        #[arg(long)]
        no_bwrap: bool,
    },

    /// Generate a fresh LUBM benchmark on the fly
    Lubm {
        /// Number of universities to generate (`-u`); must be >= 1
        #[arg(long, value_name = "N", required = true)]
        scale: u32,

        /// Tag appended to the benchmark name; pick a value that won't
        /// collide with committed snapshot names
        #[arg(long, value_name = "STRING", required = true)]
        tag: String,

        /// RNG seed (`-s`). Default 0 matches LUBM-UBA's documented default
        #[arg(long, value_name = "N", default_value = "0")]
        seed: u32,

        /// Starting university index (`-i`)
        #[arg(long, value_name = "N", default_value = "0")]
        start_index: u32,

        /// Worker thread count (`-t`). Default 1 for reproducibility.
        #[arg(long, value_name = "N", default_value = "1")]
        threads: u32,

        /// Override the LUBM-UBA jar path (default: vendored)
        #[arg(long, value_name = "PATH", env = "KERMIT_LUBM_JAR")]
        lubm_jar: Option<PathBuf>,

        /// Ontology IRI used as the base URL for generated entity IRIs.
        /// Defaults to the canonical Univ-Bench TBox URL — override only if
        /// you've mirrored the ontology elsewhere.
        #[arg(
            long,
            value_name = "URL",
            default_value = "http://www.lehigh.edu/~zhp2/2004/0401/univ-bench.owl"
        )]
        ontology: String,

        /// Override the cache dir parent (default: ~/.cache/kermit/benchmarks).
        /// NOTE: benchmarks generated outside the default cache are NOT
        /// auto-discovered by `bench list/fetch/run`; mainly useful for tests.
        #[arg(long, value_name = "PATH")]
        output_dir: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
enum Commands {
    /// Run a join query
    Join {
        #[command(flatten)]
        query_args: QueryArgs,

        /// Output file (optional, defaults to stdout)
        #[arg(short, long, value_name = "PATH")]
        output: Option<PathBuf>,
    },

    /// Run a Criterion benchmark
    Bench {
        #[command(flatten)]
        bench_args: BenchArgs,

        #[command(subcommand)]
        subcommand: BenchSubcommand,
    },
}

use materialize::{vendored_lubm_jar, vendored_watdiv_root, workspace_root};

/// Column names derived from a query's head predicate. `Var(X)` becomes
/// `"X"`, `Atom(c)` becomes `"c"` (constants are pre-rewritten by
/// `rewrite_atoms` so they appear as `c<id>` in the head when present), and
/// `Placeholder` becomes `"_"`.
fn head_column_names(query: &JoinQuery) -> Vec<String> {
    query
        .head
        .terms
        .iter()
        .map(|t| match t {
            | Term::Var(name) | Term::Atom(name) => name.clone(),
            | Term::Placeholder => "_".to_string(),
        })
        .collect()
}

fn write_tuples(
    mut writer: impl Write, header: &[String], tuples: &[Vec<usize>],
) -> io::Result<()> {
    if !header.is_empty() {
        writeln!(writer, "{}", header.join(","))?;
    }
    for tuple in tuples {
        let line: String = tuple
            .iter()
            .map(|v| v.to_string())
            .collect::<Vec<_>>()
            .join(",");
        writeln!(writer, "{}", line)?;
    }
    writer.flush()
}

fn load_query(args: &QueryArgs) -> anyhow::Result<(Box<dyn kermit::db::DB>, JoinQuery)> {
    let query_str = fs::read_to_string(&args.query)
        .map_err(|e| anyhow::anyhow!("Failed to read query file {:?}: {}", args.query, e))?;
    let join_query: JoinQuery = query_str
        .trim()
        .parse()
        .map_err(|e| anyhow::anyhow!("Failed to parse query from {:?}: {}", args.query, e))?;

    let mut db = instantiate_database(args.indexstructure, args.algorithm, "join".to_string());
    for path in &args.relations {
        db.add_file(path)
            .map_err(|e| anyhow::anyhow!("Failed to load relation {:?}: {}", path, e))?;
    }

    Ok((db, join_query))
}

fn build_time_criterion(args: &BenchArgs) -> criterion::Criterion {
    criterion::Criterion::default()
        .sample_size(args.sample_size)
        .measurement_time(Duration::from_secs(args.measurement_time))
        .warm_up_time(Duration::from_secs(args.warm_up_time))
}

fn default_report_path(kind: BenchKind) -> PathBuf {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let kind_str = match kind {
        | BenchKind::Join => "join",
        | BenchKind::Ds => "ds",
        | BenchKind::Run => "run",
    };
    PathBuf::from(format!("bench-runs/{kind_str}-{now_ms}.json"))
}

fn write_bench_report(
    override_path: Option<&Path>, kind: BenchKind, reports: &[BenchReport],
) -> anyhow::Result<()> {
    let path = match override_path {
        | Some(p) => p.to_path_buf(),
        | None => default_report_path(kind),
    };
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    let mut writer = BufWriter::new(fs::File::create(&path)?);
    write_json_report(&mut writer, reports)?;
    eprintln!("Report written: {}", path.display());
    Ok(())
}

fn build_space_criterion(args: &BenchArgs) -> criterion::Criterion<measurement::SpaceMeasurement> {
    criterion::Criterion::default()
        .with_measurement(measurement::SpaceMeasurement)
        .sample_size(args.sample_size)
        .measurement_time(Duration::from_secs(args.measurement_time))
        .warm_up_time(Duration::from_secs(args.warm_up_time))
}

/// Adds a single space-metric `bench_function` to `group` measuring
/// `relation`'s heap size in bytes. Centralises the `iter_custom`
/// calibration-trap workaround (see [`measurement::SpaceMeasurement`]
/// docs for the full explanation): an O(N) `heap_size_bytes()` call
/// inside the loop is required, and `black_box` is required to defeat
/// LICM. Without these, Criterion's wall-clock warm-up makes `iters`
/// ramp toward `u64::MAX` and the `usize` math saturates.
///
/// Returns the [`CriterionGroupRef`] the caller should append to its
/// report's `criterion_groups`.
fn add_space_bench<R>(
    group: &mut criterion::BenchmarkGroup<'_, measurement::SpaceMeasurement>, group_name: &str,
    function: String, relation: &R,
) -> CriterionGroupRef
where
    R: HeapSize,
{
    debug_assert!(
        std::hint::black_box(relation).heap_size_bytes() > 0,
        "SpaceMeasurement requires O(N) work inside iter_custom; passing a zero-cost HeapSize \
         stub re-triggers Criterion's iters-toward-u64::MAX calibration trap. See CLAUDE.md → \
         Space benchmarks gotcha."
    );
    group.bench_function(&function, |b| {
        b.iter_custom(|iters| {
            let mut total = 0usize;
            for _ in 0..iters {
                total = total.saturating_add(std::hint::black_box(relation).heap_size_bytes());
            }
            total
        });
    });
    CriterionGroupRef {
        group: group_name.to_string(),
        function,
        metric: ReportMetric::Space,
    }
}

fn run_ds_bench<R>(
    relation_path: &Path, indexstructure: IndexStructure, metrics: &[Metric], group_name: &str,
    bench_args: &BenchArgs,
) -> anyhow::Result<BenchReport>
where
    R: Relation + TrieIterable + HeapSize + 'static,
{
    let extension = relation_path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    let relation: R = match extension.to_lowercase().as_str() {
        | "csv" => R::from_csv(relation_path)
            .map_err(|e| anyhow::anyhow!("Failed to load relation: {e}"))?,
        | "parquet" => R::from_parquet(relation_path)
            .map_err(|e| anyhow::anyhow!("Failed to load relation: {e}"))?,
        | _ => anyhow::bail!("Unsupported file extension: {extension}"),
    };

    let tuples: Vec<Vec<usize>> = relation.trie_iter().into_iter().collect();
    let header = relation.header().clone();

    let ds_name = format!("{:?}", indexstructure);
    let relation_bytes = fs::metadata(relation_path).map(|m| m.len()).unwrap_or(0);

    let metadata = vec![
        MetadataLine::new("data structure", &ds_name),
        MetadataLine::new("relation", relation_path.display()),
        MetadataLine::new("relation size", measurement::format_bytes(relation_bytes)),
        MetadataLine::new("tuples", tuples.len()),
        MetadataLine::new("arity", header.arity()),
    ];
    write_metadata_block(&mut io::stderr(), "bench ds metadata", &metadata)?;

    let mut criterion_groups = Vec::new();

    let has_time_metrics = metrics
        .iter()
        .any(|m| matches!(m, Metric::Insertion | Metric::Iteration));

    if has_time_metrics {
        let mut criterion = build_time_criterion(bench_args);
        let mut group = criterion.benchmark_group(group_name);

        if metrics.contains(&Metric::Insertion) {
            let insertion_tuples = tuples.clone();
            let insertion_header = header.clone();
            let function = format!("{ds_name}/insertion");
            group.bench_function(&function, |b| {
                b.iter_batched(
                    || (insertion_header.clone(), insertion_tuples.clone()),
                    |(h, t)| R::from_tuples(h, t),
                    criterion::BatchSize::SmallInput,
                );
            });
            criterion_groups.push(CriterionGroupRef {
                group: group_name.to_string(),
                function,
                metric: ReportMetric::Time,
            });
        }

        if metrics.contains(&Metric::Iteration) {
            let function = format!("{ds_name}/iteration");
            group.bench_function(&function, |b| {
                b.iter(|| relation.trie_iter().into_iter().collect::<Vec<_>>());
            });
            criterion_groups.push(CriterionGroupRef {
                group: group_name.to_string(),
                function,
                metric: ReportMetric::Time,
            });
        }

        group.finish();
        criterion.final_summary();
    }

    if metrics.contains(&Metric::Space) {
        let n = tuples.len();
        let mut criterion = build_space_criterion(bench_args);
        let mut group = criterion.benchmark_group(group_name);
        group.throughput(criterion::Throughput::Elements(n as u64));
        let function = format!("{ds_name}/space");
        criterion_groups.push(add_space_bench(&mut group, group_name, function, &relation));
        group.finish();
        criterion.final_summary();
    }

    let axes = BTreeMap::from([
        ("data_structure".to_string(), serde_json::json!(ds_name)),
        (
            "relation_path".to_string(),
            serde_json::json!(relation_path.display().to_string()),
        ),
        (
            "relation_bytes".to_string(),
            serde_json::json!(relation_bytes),
        ),
        ("tuples".to_string(), serde_json::json!(tuples.len())),
        ("arity".to_string(), serde_json::json!(header.arity())),
    ]);
    Ok(BenchReport::new(
        BenchKind::Ds,
        &metadata,
        axes,
        criterion_groups,
    ))
}

fn run_benchmark<R>(
    benchmark: &BenchmarkDefinition, indexstructure: IndexStructure, algorithm: JoinAlgorithm,
    metrics: &[Metric], query_filter: Option<&str>, bench_args: &BenchArgs,
) -> anyhow::Result<Vec<BenchReport>>
where
    R: Relation + TrieIterable + HeapSize + 'static,
{
    let queries: Vec<&kermit_bench::QueryDefinition> = match query_filter {
        | Some(name) => {
            let q = benchmark
                .queries
                .iter()
                .find(|q| q.name == name)
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "query '{}' not found in benchmark '{}' (available: {})",
                        name,
                        benchmark.name,
                        benchmark
                            .queries
                            .iter()
                            .map(|q| q.name.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                })?;
            vec![q]
        },
        | None => benchmark.queries.iter().collect(),
    };

    let cached_paths = kermit_bench::cache::ensure_cached(benchmark)
        .map_err(|e| anyhow::anyhow!("Failed to fetch benchmark data: {e}"))?;

    // Load each relation from disk exactly once. Populating `db` from these
    // typed `R`s (rather than via `db.add_file` from disk) avoids a second
    // parquet read per relation, which is the dominant cost on large
    // workloads like WatDiv-scale-1000.
    let relations: Vec<R> = cached_paths
        .iter()
        .map(|p| R::from_parquet(p).map_err(|e| anyhow::anyhow!("Failed to load {p:?}: {e}")))
        .collect::<Result<_, _>>()?;

    let mut db = instantiate_database(indexstructure, algorithm, benchmark.name.clone());
    for rel in &relations {
        let header = rel.header();
        let name = header.name();
        let tuples: Vec<Vec<usize>> = rel.trie_iter().into_iter().collect();
        db.add_relation(name, header.arity());
        db.add_keys_batch(name, tuples);
    }

    let ds_name = format!("{:?}", indexstructure);
    let algo_name = format!("{:?}", algorithm);

    let has_time_metrics = metrics
        .iter()
        .any(|m| matches!(m, Metric::Insertion | Metric::Iteration));

    // Sum across relations: scaling plots key off this as the workload's
    // total input size. One trie walk per relation is cheap vs the bench itself.
    let total_tuples: usize = relations
        .iter()
        .map(|r| r.trie_iter().into_iter().count())
        .sum();

    let mut reports: Vec<BenchReport> = Vec::with_capacity(queries.len());

    for query_def in &queries {
        let join_query: JoinQuery =
            query_def.query.trim().parse().map_err(|e| {
                anyhow::anyhow!("Failed to parse query '{}': {:?}", query_def.query, e)
            })?;

        let mut lines = vec![
            MetadataLine::new("benchmark", &benchmark.name),
            MetadataLine::new("query", &query_def.name),
            MetadataLine::new("data structure", &ds_name),
            MetadataLine::new("algorithm", &algo_name),
        ];
        for rel in &relations {
            let h = rel.header();
            lines.push(MetadataLine::new(
                "relation",
                format!("{:?} (arity {})", h.name(), h.arity()),
            ));
        }
        write_metadata_block(&mut io::stderr(), "bench run metadata", &lines)?;

        let prefix = bench_args.name.as_deref().unwrap_or(DEFAULT_RUN_GROUP);
        let group_name = format!(
            "{}/{}/{}/{}/{}",
            prefix, benchmark.name, query_def.name, ds_name, algo_name
        );

        let mut criterion_groups: Vec<CriterionGroupRef> = Vec::new();

        if has_time_metrics {
            let mut criterion = build_time_criterion(bench_args);
            let mut group = criterion.benchmark_group(&group_name);

            if metrics.contains(&Metric::Insertion) {
                let tuples_and_headers: Vec<_> = relations
                    .iter()
                    .map(|r| {
                        (
                            r.header().clone(),
                            r.trie_iter().into_iter().collect::<Vec<_>>(),
                        )
                    })
                    .collect();

                group.bench_function("insertion", |b| {
                    b.iter_batched(
                        || tuples_and_headers.clone(),
                        |data| {
                            for (header, tuples) in data {
                                std::hint::black_box(R::from_tuples(header, tuples));
                            }
                        },
                        criterion::BatchSize::SmallInput,
                    );
                });
                criterion_groups.push(CriterionGroupRef {
                    group: group_name.clone(),
                    function: "insertion".to_string(),
                    metric: ReportMetric::Time,
                });
            }

            if metrics.contains(&Metric::Iteration) {
                group.bench_function("iteration", |b| {
                    b.iter_batched(
                        || join_query.clone(),
                        |q| db.join(q),
                        criterion::BatchSize::SmallInput,
                    );
                });
                criterion_groups.push(CriterionGroupRef {
                    group: group_name.clone(),
                    function: "iteration".to_string(),
                    metric: ReportMetric::Time,
                });
            }

            group.finish();
            criterion.final_summary();
        }

        if metrics.contains(&Metric::Space) {
            let mut criterion = build_space_criterion(bench_args);
            let mut group = criterion.benchmark_group(&group_name);
            for rel in &relations {
                let rel_name = rel.header().name().to_string();
                let function = format!("space/{}", rel_name);
                criterion_groups.push(add_space_bench(&mut group, &group_name, function, rel));
            }
            group.finish();
            criterion.final_summary();
        }

        let axes = BTreeMap::from([
            ("benchmark".to_string(), serde_json::json!(benchmark.name)),
            ("query".to_string(), serde_json::json!(query_def.name)),
            ("data_structure".to_string(), serde_json::json!(ds_name)),
            ("algorithm".to_string(), serde_json::json!(algo_name)),
            ("tuples".to_string(), serde_json::json!(total_tuples)),
        ]);
        reports.push(BenchReport::new(
            BenchKind::Run,
            &lines,
            axes,
            criterion_groups,
        ));
    }

    Ok(reports)
}

/// Returns the `bench list` status string for a benchmark.
///
/// For static benchmarks the values are "cached" / "not cached" (matching
/// the historical behaviour). For generator-driven benchmarks the values
/// distinguish "not generated" (no cache subdir), "cached" (subdir exists
/// and `meta.json` spec_hash matches the spec), and "stale" (subdir
/// exists but the spec has drifted).
///
/// `workspace_def`, when supplied, is the workspace YAML for this
/// benchmark name. It must be passed for generator-driven benchmarks
/// because the cache-side `benchmark.yml` (which `b` may have been loaded
/// from after the discovery merge) intentionally drops the `generator`
/// field — provenance lives in `meta.json` on the cache side. Without
/// `workspace_def`, the function would fall into the static branch for
/// any previously-generated benchmark and lose the `cached`/`stale`
/// distinction.
fn describe_benchmark_status(
    b: &BenchmarkDefinition, workspace_def: Option<&BenchmarkDefinition>, cache_root: &Path,
) -> &'static str {
    let spec = workspace_def
        .and_then(|w| w.generator.as_ref())
        .or(b.generator.as_ref());
    let Some(spec) = spec else {
        return if kermit_bench::cache::is_cached(b).unwrap_or(false) {
            "cached"
        } else {
            "not cached"
        };
    };
    let cache_subdir = cache_root.join(&b.name);
    let meta_path = cache_subdir.join("meta.json");
    if !meta_path.exists() {
        return "not generated";
    }
    let Ok(contents) = fs::read_to_string(&meta_path) else {
        return "stale";
    };
    let parsed: Option<serde_json::Value> = serde_json::from_str(&contents).ok();
    let cached_hash = parsed
        .as_ref()
        .and_then(|v| v.get("spec_hash"))
        .and_then(|v| v.as_str());
    match cached_hash {
        | Some(h) if h == spec.spec_hash() => "cached",
        | _ => "stale",
    }
}

fn resolve_benchmarks(
    name: &Option<String>, all: bool,
) -> anyhow::Result<Vec<BenchmarkDefinition>> {
    let root = workspace_root();
    let cache =
        kermit_bench::cache::base_cache_dir().unwrap_or_else(|_| PathBuf::from("/tmp/no-cache"));
    if all {
        kermit_bench::discovery::load_all_benchmarks_with_cache(&root, &cache)
            .context("Failed to load benchmarks")
    } else if let Some(name) = name {
        match kermit_bench::discovery::load_benchmark(&root, name) {
            | Ok(b) => Ok(vec![b]),
            | Err(_) => {
                let def = kermit_bench::discovery::load_cached_benchmark(&cache, name)
                    .map_err(|_| anyhow::anyhow!("benchmark not found: {name}"))?;
                Ok(vec![def])
            },
        }
    } else {
        anyhow::bail!("Specify a benchmark name or --all")
    }
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        | Commands::Join {
            query_args,
            output,
        } => {
            let (db, join_query) = load_query(&query_args)?;
            let header = head_column_names(&join_query);
            let tuples = db.join(join_query);
            let writer: Box<dyn Write> = match &output {
                | Some(path) => Box::new(BufWriter::new(fs::File::create(path)?)),
                | None => Box::new(BufWriter::new(io::stdout().lock())),
            };
            write_tuples(writer, &header, &tuples)?;
        },

        | Commands::Bench {
            bench_args,
            subcommand,
        } => match subcommand {
            | BenchSubcommand::List => {
                let root = workspace_root();
                let cache = kermit_bench::cache::base_cache_dir()
                    .unwrap_or_else(|_| PathBuf::from("/tmp/no-cache"));
                let workspace_defs: std::collections::HashMap<String, BenchmarkDefinition> =
                    kermit_bench::discovery::load_all_benchmarks(&root)?
                        .into_iter()
                        .map(|d| (d.name.clone(), d))
                        .collect();
                let benchmarks =
                    kermit_bench::discovery::load_all_benchmarks_with_cache(&root, &cache)?;
                if benchmarks.is_empty() {
                    eprintln!("No benchmarks found in benchmarks/ or cache");
                } else {
                    for b in &benchmarks {
                        let workspace_def = workspace_defs.get(&b.name);
                        let status = describe_benchmark_status(b, workspace_def, &cache);
                        let source = if workspace_def.is_some() {
                            "workspace"
                        } else {
                            "cache"
                        };
                        println!("{} ({}) [{source}, {status}]", b.name, b.description);
                        for q in &b.queries {
                            println!("  query: {} - {}", q.name, q.description);
                        }
                    }
                }
            },

            | BenchSubcommand::Fetch {
                name,
            } => {
                let benchmarks = resolve_benchmarks(&name, name.is_none())?;
                for benchmark in &benchmarks {
                    eprintln!("Fetching {}...", benchmark.name);
                    kermit_bench::cache::ensure_cached(benchmark)
                        .map_err(|e| anyhow::anyhow!("Failed to fetch {}: {e}", benchmark.name))?;
                    eprintln!("  Done.");
                }
            },

            | BenchSubcommand::Clean {
                name,
            } => match &name {
                | Some(n) => {
                    kermit_bench::cache::clean_benchmark(n)
                        .map_err(|e| anyhow::anyhow!("Failed to clean {}: {e}", n))?;
                    eprintln!("Cleaned cache for benchmark '{}'", n);
                },
                | None => {
                    kermit_bench::cache::clean_all()
                        .map_err(|e| anyhow::anyhow!("Failed to clean cache: {e}"))?;
                    eprintln!("Cleaned all benchmark caches");
                },
            },

            | BenchSubcommand::Join {
                query_args,
                output,
            } => {
                let (db, join_query) = load_query(&query_args)?;

                if let Some(path) = &output {
                    let header = head_column_names(&join_query);
                    let tuples = db.join(join_query.clone());
                    let writer = BufWriter::new(fs::File::create(path)?);
                    write_tuples(writer, &header, &tuples)?;
                }

                let group_name = bench_args
                    .name
                    .as_deref()
                    .unwrap_or(DEFAULT_JOIN_GROUP)
                    .to_string();
                let bench_id =
                    format!("{:?}/{:?}", query_args.indexstructure, query_args.algorithm);

                let metadata = vec![
                    MetadataLine::new("data structure", format!("{:?}", query_args.indexstructure)),
                    MetadataLine::new("algorithm", format!("{:?}", query_args.algorithm)),
                    MetadataLine::new("relations", query_args.relations.len()),
                ];
                write_metadata_block(&mut io::stderr(), "bench metadata", &metadata)?;

                let mut criterion = build_time_criterion(&bench_args);
                let mut group = criterion.benchmark_group(&group_name);
                group.bench_function(&bench_id, |b| {
                    b.iter_batched(
                        || join_query.clone(),
                        |q| db.join(q),
                        criterion::BatchSize::SmallInput,
                    );
                });
                group.finish();
                criterion.final_summary();

                let axes = BTreeMap::from([
                    (
                        "data_structure".to_string(),
                        serde_json::json!(format!("{:?}", query_args.indexstructure)),
                    ),
                    (
                        "algorithm".to_string(),
                        serde_json::json!(format!("{:?}", query_args.algorithm)),
                    ),
                    (
                        "relations".to_string(),
                        serde_json::json!(query_args.relations.len()),
                    ),
                ]);
                let report =
                    BenchReport::new(BenchKind::Join, &metadata, axes, vec![CriterionGroupRef {
                        group: group_name,
                        function: bench_id,
                        metric: ReportMetric::Time,
                    }]);
                write_bench_report(
                    bench_args.report_json.as_deref(),
                    BenchKind::Join,
                    std::slice::from_ref(&report),
                )?;
            },

            | BenchSubcommand::Ds {
                relation,
                indexstructure,
                metrics,
            } => {
                let group_name = bench_args.name.as_deref().unwrap_or(DEFAULT_DS_GROUP);
                let mut reports: Vec<BenchReport> = Vec::new();
                for ds in indexstructure.expand() {
                    let report = match ds {
                        | IndexStructure::TreeTrie => run_ds_bench::<kermit_ds::TreeTrie>(
                            &relation,
                            ds,
                            &metrics,
                            group_name,
                            &bench_args,
                        )?,
                        | IndexStructure::ColumnTrie => run_ds_bench::<kermit_ds::ColumnTrie>(
                            &relation,
                            ds,
                            &metrics,
                            group_name,
                            &bench_args,
                        )?,
                        // Phase 5 added `IndexStructure::HashTrie` to the
                        // enum; Phase 9 will route it to `run_ds_bench`.
                        // Until then, selecting it from the CLI bails — the
                        // arm exists only to keep the match exhaustive.
                        | IndexStructure::HashTrie => anyhow::bail!(
                            "bench ds: HashTrie is not yet runnable from the CLI (phase 9 will \
                             wire it)"
                        ),
                    };
                    reports.push(report);
                }
                write_bench_report(bench_args.report_json.as_deref(), BenchKind::Ds, &reports)?;
            },

            | BenchSubcommand::Run {
                name,
                all,
                query,
                indexstructure,
                algorithm,
                metrics,
                force,
            } => {
                // Reject incompatible (index-structure, algorithm) pairs
                // up front. `All` on either side is permissive — the
                // cross-product loop below already filters individual
                // concrete pairs at dispatch time.
                if !indexstructure.supports_algorithm(algorithm) {
                    anyhow::bail!(
                        "incompatible CLI selection: --indexstructure {indexstructure:?} cannot be \
                         joined with --algorithm {algorithm:?} (hash-trie pairs with \
                         hash-triejoin; sorted tries pair with leapfrog-triejoin)"
                    );
                }
                let benchmarks = resolve_benchmarks(&name, all)?;
                let cache_root = kermit_bench::cache::base_cache_dir()
                    .map_err(|e| anyhow::anyhow!("no cache directory available: {e}"))?;
                let materialized: Vec<BenchmarkDefinition> = benchmarks
                    .into_iter()
                    .map(|b| materialize::materialize(b, &cache_root, force))
                    .collect::<Result<Vec<_>, _>>()?;

                let indexstructures = indexstructure.expand();
                let algorithms = algorithm.expand();
                let mut reports: Vec<BenchReport> = Vec::new();
                for benchmark in &materialized {
                    for &ds in &indexstructures {
                        for &algo in &algorithms {
                            let mut chunk = match ds {
                                | IndexStructure::TreeTrie => run_benchmark::<kermit_ds::TreeTrie>(
                                    benchmark,
                                    ds,
                                    algo,
                                    &metrics,
                                    query.as_deref(),
                                    &bench_args,
                                )?,
                                | IndexStructure::ColumnTrie => {
                                    run_benchmark::<kermit_ds::ColumnTrie>(
                                        benchmark,
                                        ds,
                                        algo,
                                        &metrics,
                                        query.as_deref(),
                                        &bench_args,
                                    )?
                                },
                                // Phase 5 added `IndexStructure::HashTrie` to
                                // the enum; Phase 9 will route it to
                                // `run_benchmark`. Until then, selecting it
                                // from the CLI bails — the arm exists only
                                // to keep the match exhaustive.
                                | IndexStructure::HashTrie => anyhow::bail!(
                                    "bench run: HashTrie is not yet runnable from the CLI (phase \
                                     9 will wire it)"
                                ),
                            };
                            reports.append(&mut chunk);
                        }
                    }
                }
                write_bench_report(bench_args.report_json.as_deref(), BenchKind::Run, &reports)?;
            },

            | BenchSubcommand::Gen {
                subcommand,
            } => match subcommand {
                | GenSubcommand::Watdiv {
                    scale,
                    tag,
                    max_query_size,
                    query_count,
                    constants_per_query,
                    allow_join_vertex,
                    watdiv_bin,
                    output_dir,
                    no_bwrap,
                } => {
                    let bench_name = format!("watdiv-stress-{scale}-{tag}");
                    let workspace = workspace_root();
                    let workspace_names = kermit_bench::discovery::list_benchmarks(&workspace)
                        .map_err(|e| {
                            anyhow::anyhow!("failed to enumerate workspace benchmarks: {e}")
                        })?;
                    if workspace_names.iter().any(|n| n == &bench_name) {
                        anyhow::bail!(
                            "--tag {tag:?} produces bench name {bench_name:?} which already \
                             exists in the workspace; pick a different tag"
                        );
                    }
                    let vendor = vendored_watdiv_root();
                    let bin = watdiv_bin.unwrap_or_else(|| vendor.join("bin/Release/watdiv"));
                    if !bin.exists() {
                        anyhow::bail!("watdiv binary not found at {bin:?}");
                    }
                    let default_cache = kermit_bench::cache::base_cache_dir()
                        .map_err(|e| anyhow::anyhow!("no cache directory available: {e}"))?;
                    let cache_parent = output_dir.unwrap_or_else(|| default_cache.clone());
                    if cache_parent != default_cache {
                        eprintln!(
                            "[gen watdiv] note: --output-dir is set to {}; the generated \
                             benchmark will NOT be auto-discovered by `bench list/fetch/run` \
                             (those scan {})",
                            cache_parent.display(),
                            default_cache.display()
                        );
                    }
                    let out_dir = cache_parent.join(&bench_name);
                    std::fs::create_dir_all(&out_dir)?;

                    let stress = kermit_rdf::driver::StressParams {
                        max_query_size,
                        query_count,
                        constants_per_query,
                        allow_join_vertex,
                    };
                    let inputs = kermit_rdf::pipeline::PipelineInputs {
                        driver: kermit_rdf::driver::DriverInputs {
                            watdiv_bin: &bin,
                            vendor_files: &vendor.join("files"),
                            model_file: &vendor.join("MODEL.txt"),
                            scale,
                            stress,
                            query_count_per_template: query_count,
                            use_bwrap: !no_bwrap,
                        },
                        out_dir: &out_dir,
                        bench_name: &bench_name,
                        tag: &tag,
                        spec_hash: None,
                    };
                    let meta = kermit_rdf::pipeline::run_pipeline(&inputs)
                        .map_err(|e| anyhow::anyhow!("gen watdiv pipeline failed: {e}"))?;
                    eprintln!(
                        "[gen watdiv] wrote {} (triples={}, relations={}, queries={})",
                        out_dir.display(),
                        meta.triple_count,
                        meta.relation_count,
                        meta.query_count
                    );
                },

                | GenSubcommand::Lubm {
                    scale,
                    tag,
                    seed,
                    start_index,
                    threads,
                    lubm_jar,
                    ontology,
                    output_dir,
                } => {
                    let bench_name = format!("lubm-{scale}-{tag}");
                    let workspace = workspace_root();
                    let workspace_names = kermit_bench::discovery::list_benchmarks(&workspace)
                        .map_err(|e| {
                            anyhow::anyhow!("failed to enumerate workspace benchmarks: {e}")
                        })?;
                    if workspace_names.iter().any(|n| n == &bench_name) {
                        anyhow::bail!(
                            "--tag {tag:?} produces bench name {bench_name:?} which already \
                             exists in the workspace; pick a different tag"
                        );
                    }
                    let jar = lubm_jar.unwrap_or_else(vendored_lubm_jar);
                    if !jar.exists() {
                        anyhow::bail!(
                            "LUBM-UBA jar not found at {jar:?}; build with `mvn package` in \
                             lubm-uba-rs and copy to kermit-rdf/vendor/lubm-uba/, or override \
                             with --lubm-jar / KERMIT_LUBM_JAR"
                        );
                    }
                    let default_cache = kermit_bench::cache::base_cache_dir()
                        .map_err(|e| anyhow::anyhow!("no cache directory available: {e}"))?;
                    let cache_parent = output_dir.unwrap_or_else(|| default_cache.clone());
                    if cache_parent != default_cache {
                        eprintln!(
                            "[gen lubm] note: --output-dir is set to {}; the generated benchmark \
                             will NOT be auto-discovered by `bench list/fetch/run` (those scan {})",
                            cache_parent.display(),
                            default_cache.display()
                        );
                    }
                    let out_dir = cache_parent.join(&bench_name);
                    if out_dir.exists()
                        && std::fs::read_dir(&out_dir)
                            .map(|mut d| d.next().is_some())
                            .unwrap_or(false)
                    {
                        eprintln!(
                            "[gen lubm] note: {} is non-empty; existing files will be overwritten",
                            out_dir.display()
                        );
                    }
                    std::fs::create_dir_all(&out_dir)?;

                    // LUBM(1, 0) cardinalities are only valid at scale 1; at
                    // other scales we still emit the queries but skip the
                    // expected.csv files to avoid misleading the cardinality
                    // test.
                    let queries = kermit_rdf::lubm::queries::lubm_query_specs(scale == 1);

                    let inputs = kermit_rdf::lubm::pipeline::LubmPipelineInputs {
                        driver: kermit_rdf::lubm::driver::LubmDriverInputs {
                            jar_path: &jar,
                            scale,
                            seed,
                            start_index,
                            threads,
                            ontology_iri: &ontology,
                        },
                        out_dir: &out_dir,
                        bench_name: &bench_name,
                        tag: &tag,
                        queries: &queries,
                        spec_hash: None,
                    };
                    let meta = kermit_rdf::lubm::pipeline::run_lubm_pipeline(&inputs)
                        .map_err(|e| anyhow::anyhow!("gen lubm pipeline failed: {e}"))?;
                    eprintln!(
                        "[gen lubm] wrote {} (pre={}, post={}, derived={}, relations={}, \
                         queries={})",
                        out_dir.display(),
                        meta.triple_count_pre_entailment,
                        meta.triple_count_post_entailment,
                        meta.derived_triple_count,
                        meta.relation_count,
                        meta.query_count
                    );
                },
            },
        },
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_tuples_no_header_emits_only_rows() {
        let tuples = vec![vec![1, 2, 3], vec![4, 5, 6]];
        let mut buf = Vec::new();
        write_tuples(&mut buf, &[], &tuples).unwrap();
        assert_eq!(String::from_utf8(buf).unwrap(), "1,2,3\n4,5,6\n");
    }

    #[test]
    fn write_tuples_with_header_emits_header_then_rows() {
        let tuples = vec![vec![1, 2, 3], vec![4, 5, 6]];
        let header = vec!["X".to_string(), "Y".to_string(), "Z".to_string()];
        let mut buf = Vec::new();
        write_tuples(&mut buf, &header, &tuples).unwrap();
        assert_eq!(String::from_utf8(buf).unwrap(), "X,Y,Z\n1,2,3\n4,5,6\n");
    }

    #[test]
    fn write_tuples_single_column() {
        let tuples = vec![vec![10], vec![20]];
        let mut buf = Vec::new();
        write_tuples(&mut buf, &[], &tuples).unwrap();
        assert_eq!(String::from_utf8(buf).unwrap(), "10\n20\n");
    }

    #[test]
    fn write_tuples_empty() {
        let tuples: Vec<Vec<usize>> = vec![];
        let mut buf = Vec::new();
        write_tuples(&mut buf, &[], &tuples).unwrap();
        assert_eq!(String::from_utf8(buf).unwrap(), "");
    }

    #[test]
    fn head_column_names_extracts_variables_atoms_and_placeholders() {
        let q: JoinQuery = "Q(X, Y, _) :- R(X, Y, Z).".parse().unwrap();
        assert_eq!(head_column_names(&q), vec!["X", "Y", "_"]);
    }

    fn make_generator_def(name: &str, spec: kermit_bench::GeneratorSpec) -> BenchmarkDefinition {
        BenchmarkDefinition {
            name: name.to_string(),
            description: "test".to_string(),
            relations: vec![],
            queries: vec![],
            generator: Some(spec),
        }
    }

    fn make_static_def(name: &str) -> BenchmarkDefinition {
        BenchmarkDefinition {
            name: name.to_string(),
            description: "test".to_string(),
            relations: vec![kermit_bench::RelationSource {
                name: "edge".to_string(),
                url: "file:///nope".to_string(),
            }],
            queries: vec![kermit_bench::QueryDefinition {
                name: "q".to_string(),
                description: "q".to_string(),
                query: "Q(X) :- edge(X, Y).".to_string(),
            }],
            generator: None,
        }
    }

    #[test]
    fn status_for_static_benchmark_uses_cached_or_not_cached() {
        let dir = tempfile::tempdir().unwrap();
        let def = make_static_def("static");
        // is_cached is keyed off the platform cache dir; on a tempdir we
        // expect "not cached" since we haven't downloaded anything.
        let status = describe_benchmark_status(&def, None, dir.path());
        assert!(matches!(status, "cached" | "not cached"));
    }

    #[test]
    fn status_not_generated_when_no_meta() {
        let dir = tempfile::tempdir().unwrap();
        let def = make_generator_def("watdiv-x", kermit_bench::GeneratorSpec::Watdiv {
            scale: 1,
            stress: kermit_bench::WatdivStressSpec::default(),
        });
        assert_eq!(
            describe_benchmark_status(&def, Some(&def), dir.path()),
            "not generated"
        );
    }

    #[test]
    fn status_cached_when_meta_hash_matches() {
        let dir = tempfile::tempdir().unwrap();
        let spec = kermit_bench::GeneratorSpec::Watdiv {
            scale: 1,
            stress: kermit_bench::WatdivStressSpec::default(),
        };
        let hash = spec.spec_hash();
        let subdir = dir.path().join("watdiv-cached");
        fs::create_dir_all(&subdir).unwrap();
        fs::write(
            subdir.join("meta.json"),
            serde_json::json!({"schema_version": 2, "spec_hash": hash}).to_string(),
        )
        .unwrap();
        let def = make_generator_def("watdiv-cached", spec);
        assert_eq!(
            describe_benchmark_status(&def, Some(&def), dir.path()),
            "cached"
        );
    }

    #[test]
    fn status_stale_when_meta_hash_differs() {
        let dir = tempfile::tempdir().unwrap();
        let subdir = dir.path().join("watdiv-stale");
        fs::create_dir_all(&subdir).unwrap();
        fs::write(
            subdir.join("meta.json"),
            serde_json::json!({"schema_version": 2, "spec_hash": "old-hash"}).to_string(),
        )
        .unwrap();
        let def = make_generator_def("watdiv-stale", kermit_bench::GeneratorSpec::Watdiv {
            scale: 7,
            stress: kermit_bench::WatdivStressSpec::default(),
        });
        assert_eq!(
            describe_benchmark_status(&def, Some(&def), dir.path()),
            "stale"
        );
    }

    #[test]
    fn status_stale_for_legacy_meta_without_spec_hash() {
        let dir = tempfile::tempdir().unwrap();
        let subdir = dir.path().join("watdiv-legacy");
        fs::create_dir_all(&subdir).unwrap();
        fs::write(
            subdir.join("meta.json"),
            serde_json::json!({"schema_version": 1, "kind": "watdiv-onthefly"}).to_string(),
        )
        .unwrap();
        let def = make_generator_def("watdiv-legacy", kermit_bench::GeneratorSpec::Watdiv {
            scale: 1,
            stress: kermit_bench::WatdivStressSpec::default(),
        });
        assert_eq!(
            describe_benchmark_status(&def, Some(&def), dir.path()),
            "stale"
        );
    }

    /// Pins the `IndexStructureSelector::All` expansion to every concrete
    /// `IndexStructure` variant. Adding a new `IndexStructure` variant
    /// (and not adding it to clap's `ValueEnum`-derived list) would
    /// silently drop it from sweeps; this test fails as soon as the
    /// upstream variant set changes, prompting the maintainer to verify
    /// the new variant is reachable from the binary's match arms in
    /// `bench run` / `bench ds`.
    #[test]
    fn index_structure_selector_all_covers_every_value_enum_variant() {
        use clap::ValueEnum;
        assert_eq!(
            IndexStructureSelector::All.expand(),
            IndexStructure::value_variants().to_vec()
        );
    }

    #[test]
    fn index_structure_selector_concrete_returns_singleton() {
        assert_eq!(IndexStructureSelector::TreeTrie.expand(), vec![
            IndexStructure::TreeTrie
        ]);
        assert_eq!(IndexStructureSelector::ColumnTrie.expand(), vec![
            IndexStructure::ColumnTrie
        ]);
        assert_eq!(IndexStructureSelector::HashTrie.expand(), vec![
            IndexStructure::HashTrie
        ]);
    }

    #[test]
    fn join_algorithm_selector_all_covers_every_value_enum_variant() {
        use clap::ValueEnum;
        assert_eq!(
            JoinAlgorithmSelector::All.expand(),
            JoinAlgorithm::value_variants().to_vec()
        );
    }

    #[test]
    fn join_algorithm_selector_concrete_returns_singleton() {
        assert_eq!(JoinAlgorithmSelector::LeapfrogTriejoin.expand(), vec![
            JoinAlgorithm::LeapfrogTriejoin
        ]);
        assert_eq!(JoinAlgorithmSelector::HashTriejoin.expand(), vec![
            JoinAlgorithm::HashTriejoin
        ]);
    }

    /// Pins the CLI's compatibility matrix. Hash trie pairs only with
    /// hash triejoin; sorted tries pair only with leapfrog triejoin;
    /// `All` on either side permits anything (the cross-product caller
    /// filters at expand time).
    #[test]
    fn supports_algorithm_filters_incompatible_pairs() {
        // `All` is unqualified-ambiguous between the two enums; use
        // explicit aliases for clarity (and to satisfy E0659).
        type Is = IndexStructureSelector;
        type Ja = JoinAlgorithmSelector;
        // Hash trie pairs only with hash triejoin.
        assert!(Is::HashTrie.supports_algorithm(Ja::HashTriejoin));
        assert!(!Is::HashTrie.supports_algorithm(Ja::LeapfrogTriejoin));
        // Sorted tries pair only with LFTJ.
        assert!(Is::TreeTrie.supports_algorithm(Ja::LeapfrogTriejoin));
        assert!(!Is::TreeTrie.supports_algorithm(Ja::HashTriejoin));
        assert!(Is::ColumnTrie.supports_algorithm(Ja::LeapfrogTriejoin));
        assert!(!Is::ColumnTrie.supports_algorithm(Ja::HashTriejoin));
        // `All` selectors are permissive on either side.
        assert!(Is::All.supports_algorithm(Ja::All));
        assert!(Is::All.supports_algorithm(Ja::HashTriejoin));
        assert!(Is::All.supports_algorithm(Ja::LeapfrogTriejoin));
        assert!(Is::HashTrie.supports_algorithm(Ja::All));
        assert!(Is::TreeTrie.supports_algorithm(Ja::All));
        assert!(Is::ColumnTrie.supports_algorithm(Ja::All));
    }

    /// Regression test: when discovery merges a workspace generator YAML
    /// with its cache-side artefact, the merged def has `generator: None`
    /// (because `kermit_rdf::yaml_emit::write_benchmark_yaml` always writes
    /// a static-shaped YAML in the cache). Without the workspace_def hint,
    /// `describe_benchmark_status` would fall into the static branch for
    /// any previously-generated benchmark and lose the `cached`/`stale`
    /// signal. The `workspace_def` argument is the fix.
    #[test]
    fn status_uses_workspace_generator_when_merged_def_drops_it() {
        let dir = tempfile::tempdir().unwrap();
        let spec = kermit_bench::GeneratorSpec::Watdiv {
            scale: 3,
            stress: kermit_bench::WatdivStressSpec::default(),
        };
        let hash = spec.spec_hash();

        // Simulate the post-merge state: the cache-side YAML is loaded
        // (static-shaped, generator: None) but the workspace YAML has the
        // generator block. This is exactly what `bench list` sees after
        // calling `load_all_benchmarks_with_cache`.
        let merged_def = make_static_def("watdiv-collision");
        let workspace_def = make_generator_def("watdiv-collision", spec);

        let subdir = dir.path().join("watdiv-collision");
        fs::create_dir_all(&subdir).unwrap();
        fs::write(
            subdir.join("meta.json"),
            serde_json::json!({"schema_version": 2, "spec_hash": hash}).to_string(),
        )
        .unwrap();

        // With workspace_def threaded in, the function correctly reports
        // `cached` for the generator-driven YAML.
        assert_eq!(
            describe_benchmark_status(&merged_def, Some(&workspace_def), dir.path()),
            "cached"
        );

        // Without it, the function would fall into the static branch and
        // emit "not cached" — proves the workspace_def path is load-bearing.
        let static_status = describe_benchmark_status(&merged_def, None, dir.path());
        assert_ne!(
            static_status, "cached",
            "without workspace_def the static path takes over and the generator status is lost"
        );
    }
}
