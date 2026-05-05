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
mod measurement;

use bench_report::{
    write_json_report, write_metadata_block, BenchKind, BenchReport, CriterionGroupRef,
    MetadataLine, ReportMetric,
};

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
    /// Benchmark a join query
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

        /// Data structure
        #[arg(
            short,
            long,
            value_name = "INDEXSTRUCTURE",
            required = true,
            value_enum
        )]
        indexstructure: IndexStructure,

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

        /// Data structure
        #[arg(
            short,
            long,
            value_name = "INDEXSTRUCTURE",
            required = true,
            value_enum
        )]
        indexstructure: IndexStructure,

        /// Join algorithm
        #[arg(short, long, value_name = "ALGORITHM", required = true, value_enum)]
        algorithm: JoinAlgorithm,

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

    /// Generate a fresh watdiv benchmark on the fly
    WatdivGen {
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
    LubmGen {
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

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("kermit crate must be inside workspace")
        .to_path_buf()
}

fn vendored_watdiv_root() -> PathBuf { workspace_root().join("kermit-rdf/vendor/watdiv") }

fn vendored_lubm_jar() -> PathBuf {
    workspace_root().join("kermit-rdf/vendor/lubm-uba/lubm-uba.jar")
}

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

    let mut db = instantiate_database(args.indexstructure, args.algorithm);
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

fn run_ds_bench<R>(
    relation_path: &Path, indexstructure: IndexStructure, metrics: &[Metric], group_name: &str,
    bench_args: &BenchArgs,
) -> anyhow::Result<()>
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
        // Skip the expensive per-iter from_tuples rebuild but keep one O(N)
        // heap_size_bytes() traversal in the loop. Criterion uses wall-clock
        // during warm-up to pick `iters`, so a sub-microsecond closure makes
        // it ramp `iters` toward u64::MAX and the math saturates usize.
        // black_box prevents LICM from hoisting the call out of the loop.
        let function = format!("{ds_name}/space");
        group.bench_function(&function, |b| {
            b.iter_custom(|iters| {
                let mut total = 0usize;
                for _ in 0..iters {
                    total = total.saturating_add(std::hint::black_box(&relation).heap_size_bytes());
                }
                total
            });
        });
        criterion_groups.push(CriterionGroupRef {
            group: group_name.to_string(),
            function,
            metric: ReportMetric::Space,
        });
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
    let report = BenchReport::new(BenchKind::Ds, &metadata, axes, criterion_groups);
    write_bench_report(
        bench_args.report_json.as_deref(),
        BenchKind::Ds,
        std::slice::from_ref(&report),
    )?;

    Ok(())
}

fn run_benchmark<R>(
    benchmark: &BenchmarkDefinition, indexstructure: IndexStructure, algorithm: JoinAlgorithm,
    metrics: &[Metric], query_filter: Option<&str>, bench_args: &BenchArgs,
) -> anyhow::Result<()>
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

    let mut db = instantiate_database(indexstructure, algorithm);
    for path in &cached_paths {
        db.add_file(path)
            .map_err(|e| anyhow::anyhow!("Failed to load relation {:?}: {}", path, e))?;
    }

    let relations: Vec<R> = cached_paths
        .iter()
        .map(|p| R::from_parquet(p).map_err(|e| anyhow::anyhow!("Failed to load {p:?}: {e}")))
        .collect::<Result<_, _>>()?;

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

        let prefix = bench_args.name.as_deref().unwrap_or("run");
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
                // Skip the expensive per-iter from_tuples rebuild but keep one
                // O(N) heap_size_bytes() traversal in the loop. Criterion uses
                // wall-clock during warm-up to pick `iters`, so a sub-microsecond
                // closure makes it ramp `iters` toward u64::MAX and the math
                // saturates usize. black_box prevents LICM from hoisting the
                // call out of the loop.
                let function = format!("space/{}", rel_name);
                group.bench_function(&function, |b| {
                    b.iter_custom(|iters| {
                        let mut total = 0usize;
                        for _ in 0..iters {
                            total =
                                total.saturating_add(std::hint::black_box(rel).heap_size_bytes());
                        }
                        total
                    });
                });
                criterion_groups.push(CriterionGroupRef {
                    group: group_name.clone(),
                    function,
                    metric: ReportMetric::Space,
                });
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

    write_bench_report(bench_args.report_json.as_deref(), BenchKind::Run, &reports)?;

    Ok(())
}

fn resolve_benchmarks(
    name: &Option<String>, all: bool,
) -> anyhow::Result<Vec<BenchmarkDefinition>> {
    let root = workspace_root();
    let cache = dirs::cache_dir()
        .map(|p| p.join("kermit").join("benchmarks"))
        .unwrap_or_else(|| PathBuf::from("/tmp/no-cache"));
    if all {
        kermit_bench::discovery::load_all_benchmarks_with_cache(&root, &cache)
            .map_err(|e| anyhow::anyhow!("Failed to load benchmarks: {e}"))
    } else if let Some(name) = name {
        match kermit_bench::discovery::load_benchmark(&root, name) {
            | Ok(b) => Ok(vec![b]),
            | Err(_) => {
                let dir = cache.join(name);
                let yml = dir.join("benchmark.yml");
                let meta = dir.join("meta.json");
                if !yml.exists() || !meta.exists() {
                    anyhow::bail!("benchmark not found: {name}");
                }
                let contents = std::fs::read_to_string(&yml)?;
                let def: BenchmarkDefinition = serde_yaml::from_str(&contents)?;
                def.validate()?;
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
                let cache = dirs::cache_dir()
                    .map(|p| p.join("kermit").join("benchmarks"))
                    .unwrap_or_else(|| PathBuf::from("/tmp/no-cache"));
                let workspace_names: std::collections::HashSet<String> =
                    kermit_bench::discovery::list_benchmarks(&root)
                        .map_err(|e| anyhow::anyhow!("{e}"))?
                        .into_iter()
                        .collect();
                let benchmarks =
                    kermit_bench::discovery::load_all_benchmarks_with_cache(&root, &cache)
                        .map_err(|e| anyhow::anyhow!("{e}"))?;
                if benchmarks.is_empty() {
                    eprintln!("No benchmarks found in benchmarks/ or cache");
                } else {
                    for b in &benchmarks {
                        let cached = kermit_bench::cache::is_cached(b).unwrap_or(false);
                        let status = if cached {
                            "cached"
                        } else {
                            "not cached"
                        };
                        let source = if workspace_names.contains(&b.name) {
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

                let group_name = bench_args.name.as_deref().unwrap_or("join").to_string();
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
                let group_name = bench_args.name.as_deref().unwrap_or("ds");

                match indexstructure {
                    | IndexStructure::TreeTrie => {
                        run_ds_bench::<kermit_ds::TreeTrie>(
                            &relation,
                            indexstructure,
                            &metrics,
                            group_name,
                            &bench_args,
                        )?;
                    },
                    | IndexStructure::ColumnTrie => {
                        run_ds_bench::<kermit_ds::ColumnTrie>(
                            &relation,
                            indexstructure,
                            &metrics,
                            group_name,
                            &bench_args,
                        )?;
                    },
                }
            },

            | BenchSubcommand::Run {
                name,
                all,
                query,
                indexstructure,
                algorithm,
                metrics,
            } => {
                let benchmarks = resolve_benchmarks(&name, all)?;

                for benchmark in &benchmarks {
                    match indexstructure {
                        | IndexStructure::TreeTrie => {
                            run_benchmark::<kermit_ds::TreeTrie>(
                                benchmark,
                                indexstructure,
                                algorithm,
                                &metrics,
                                query.as_deref(),
                                &bench_args,
                            )?;
                        },
                        | IndexStructure::ColumnTrie => {
                            run_benchmark::<kermit_ds::ColumnTrie>(
                                benchmark,
                                indexstructure,
                                algorithm,
                                &metrics,
                                query.as_deref(),
                                &bench_args,
                            )?;
                        },
                    }
                }
            },

            | BenchSubcommand::WatdivGen {
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
                        "--tag {tag:?} produces bench name {bench_name:?} which already exists in \
                         the workspace; pick a different tag"
                    );
                }
                let vendor = vendored_watdiv_root();
                let bin = watdiv_bin.unwrap_or_else(|| vendor.join("bin/Release/watdiv"));
                if !bin.exists() {
                    anyhow::bail!("watdiv binary not found at {bin:?}");
                }
                let default_cache = dirs::cache_dir()
                    .map(|p| p.join("kermit").join("benchmarks"))
                    .expect("no cache dir on this platform");
                let cache_parent = output_dir.unwrap_or_else(|| default_cache.clone());
                if cache_parent != default_cache {
                    eprintln!(
                        "[watdiv-gen] note: --output-dir is set to {}; the generated benchmark \
                         will NOT be auto-discovered by `bench list/fetch/run` (those scan {})",
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
                };
                let meta = kermit_rdf::pipeline::run_pipeline(&inputs)
                    .map_err(|e| anyhow::anyhow!("watdiv-gen pipeline failed: {e}"))?;
                eprintln!(
                    "[watdiv-gen] wrote {} (triples={}, relations={}, queries={})",
                    out_dir.display(),
                    meta.triple_count,
                    meta.relation_count,
                    meta.query_count
                );
            },

            | BenchSubcommand::LubmGen {
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
                        "--tag {tag:?} produces bench name {bench_name:?} which already exists in \
                         the workspace; pick a different tag"
                    );
                }
                let jar = lubm_jar.unwrap_or_else(vendored_lubm_jar);
                if !jar.exists() {
                    anyhow::bail!(
                        "LUBM-UBA jar not found at {jar:?}; build with `mvn package` in \
                         lubm-uba-rs and copy to kermit-rdf/vendor/lubm-uba/, or override with \
                         --lubm-jar / KERMIT_LUBM_JAR"
                    );
                }
                let default_cache = dirs::cache_dir()
                    .map(|p| p.join("kermit").join("benchmarks"))
                    .expect("no cache dir on this platform");
                let cache_parent = output_dir.unwrap_or_else(|| default_cache.clone());
                if cache_parent != default_cache {
                    eprintln!(
                        "[lubm-gen] note: --output-dir is set to {}; the generated benchmark will \
                         NOT be auto-discovered by `bench list/fetch/run` (those scan {})",
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
                        "[lubm-gen] note: {} is non-empty; existing files will be overwritten",
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
                };
                let meta = kermit_rdf::lubm::pipeline::run_lubm_pipeline(&inputs)
                    .map_err(|e| anyhow::anyhow!("lubm-gen pipeline failed: {e}"))?;
                eprintln!(
                    "[lubm-gen] wrote {} (pre={}, post={}, derived={}, relations={}, queries={})",
                    out_dir.display(),
                    meta.triple_count_pre_entailment,
                    meta.triple_count_post_entailment,
                    meta.derived_triple_count,
                    meta.relation_count,
                    meta.query_count
                );
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
}
