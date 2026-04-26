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
    std::{
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
    /// Verbose output
    #[arg(short, long)]
    verbose: bool,

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

    /// Write a machine-readable JSON report of metadata and Criterion
    /// group/function ids to this path (in addition to the human-readable
    /// stderr metadata block and Criterion's own target/criterion/ output)
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

fn write_tuples(mut writer: impl Write, tuples: &[Vec<usize>]) -> io::Result<()> {
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

fn maybe_write_report(path: Option<&Path>, reports: &[BenchReport]) -> anyhow::Result<()> {
    let Some(p) = path else {
        return Ok(());
    };
    let mut writer = BufWriter::new(fs::File::create(p)?);
    write_json_report(&mut writer, reports)?;
    Ok(())
}

fn build_space_criterion(args: &BenchArgs) -> criterion::Criterion<measurement::SpaceMeasurement> {
    // Plotters panics on zero-variance data (heap_size_bytes() is
    // deterministic), so plots must be disabled for the space measurement.
    criterion::Criterion::default()
        .with_measurement(measurement::SpaceMeasurement)
        .without_plots()
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
        // Reconstruct per iter so Criterion's calibration sees real work; the
        // returned total = heap_size_bytes() * iters, so per-iter = bytes
        // exactly (see docs/specs/space-benchmarks.md).
        let function = format!("{ds_name}/space");
        group.bench_function(&function, |b| {
            b.iter_custom(|iters| {
                let mut total = 0usize;
                for _ in 0..iters {
                    let r = R::from_tuples(header.clone(), tuples.clone());
                    total = total.saturating_add(r.heap_size_bytes());
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

    let report = BenchReport::new(BenchKind::Ds, &metadata, criterion_groups);
    maybe_write_report(bench_args.report_json.as_deref(), std::slice::from_ref(&report))?;

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

        let group_name = format!(
            "{}/{}/{}/{}",
            benchmark.name, query_def.name, ds_name, algo_name
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
                group.bench_function("join", |b| {
                    b.iter_batched(
                        || join_query.clone(),
                        |q| db.join(q),
                        criterion::BatchSize::SmallInput,
                    );
                });
                criterion_groups.push(CriterionGroupRef {
                    group: group_name.clone(),
                    function: "join".to_string(),
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
                let h = rel.header().clone();
                let rel_name = h.name().to_string();
                let rel_tuples: Vec<Vec<usize>> = rel.trie_iter().into_iter().collect();
                // Reconstruct per iter so Criterion's calibration sees real
                // work and total = heap_size_bytes() * iters (per spec at
                // docs/specs/space-benchmarks.md).
                let function = format!("space/{}", rel_name);
                group.bench_function(&function, |b| {
                    b.iter_custom(|iters| {
                        let mut total = 0usize;
                        for _ in 0..iters {
                            let r = R::from_tuples(h.clone(), rel_tuples.clone());
                            total = total.saturating_add(r.heap_size_bytes());
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

        reports.push(BenchReport::new(BenchKind::Run, &lines, criterion_groups));
    }

    maybe_write_report(bench_args.report_json.as_deref(), &reports)?;

    Ok(())
}

fn resolve_benchmarks(
    name: &Option<String>, all: bool,
) -> anyhow::Result<Vec<BenchmarkDefinition>> {
    let root = workspace_root();
    if all {
        kermit_bench::discovery::load_all_benchmarks(&root)
            .map_err(|e| anyhow::anyhow!("Failed to load benchmarks: {e}"))
    } else if let Some(name) = name {
        Ok(vec![kermit_bench::discovery::load_benchmark(&root, name)
            .map_err(|e| anyhow::anyhow!("{e}"))?])
    } else {
        anyhow::bail!("Specify a benchmark name or --all")
    }
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    if cli.verbose {
        println!("Verbose mode enabled");
    }

    match cli.command {
        | Commands::Join { query_args, output } => {
            let (db, join_query) = load_query(&query_args)?;
            let tuples = db.join(join_query);
            let writer: Box<dyn Write> = match &output {
                | Some(path) => Box::new(BufWriter::new(fs::File::create(path)?)),
                | None => Box::new(BufWriter::new(io::stdout().lock())),
            };
            write_tuples(writer, &tuples)?;
        },

        | Commands::Bench {
            bench_args,
            subcommand,
        } => match subcommand {
            | BenchSubcommand::List => {
                let root = workspace_root();
                let benchmarks = kermit_bench::discovery::load_all_benchmarks(&root)
                    .map_err(|e| anyhow::anyhow!("{e}"))?;
                if benchmarks.is_empty() {
                    eprintln!("No benchmarks found in benchmarks/");
                } else {
                    for b in &benchmarks {
                        let cached = kermit_bench::cache::is_cached(b).unwrap_or(false);
                        let status = if cached {
                            "cached"
                        } else {
                            "not cached"
                        };
                        println!("{} ({}) [{}]", b.name, b.description, status);
                        for q in &b.queries {
                            println!("  query: {} - {}", q.name, q.description);
                        }
                    }
                }
            },

            | BenchSubcommand::Fetch { name } => {
                let root = workspace_root();
                let benchmarks = match &name {
                    | Some(n) => {
                        vec![kermit_bench::discovery::load_benchmark(&root, n)
                            .map_err(|e| anyhow::anyhow!("{e}"))?]
                    },
                    | None => kermit_bench::discovery::load_all_benchmarks(&root)
                        .map_err(|e| anyhow::anyhow!("{e}"))?,
                };
                for benchmark in &benchmarks {
                    eprintln!("Fetching {}...", benchmark.name);
                    kermit_bench::cache::ensure_cached(benchmark)
                        .map_err(|e| anyhow::anyhow!("Failed to fetch {}: {e}", benchmark.name))?;
                    eprintln!("  Done.");
                }
            },

            | BenchSubcommand::Clean { name } => match &name {
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

            | BenchSubcommand::Join { query_args, output } => {
                let (db, join_query) = load_query(&query_args)?;

                if let Some(path) = &output {
                    let tuples = db.join(join_query.clone());
                    let writer = BufWriter::new(fs::File::create(path)?);
                    write_tuples(writer, &tuples)?;
                }

                let group_name = bench_args.name.as_deref().unwrap_or("join").to_string();
                let bench_id =
                    format!("{:?}/{:?}", query_args.indexstructure, query_args.algorithm);

                let metadata = vec![
                    MetadataLine::new(
                        "data structure",
                        format!("{:?}", query_args.indexstructure),
                    ),
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

                let report = BenchReport::new(
                    BenchKind::Join,
                    &metadata,
                    vec![CriterionGroupRef {
                        group: group_name,
                        function: bench_id,
                        metric: ReportMetric::Time,
                    }],
                );
                maybe_write_report(
                    bench_args.report_json.as_deref(),
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
        },
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_tuples_formats_csv_lines() {
        let tuples = vec![vec![1, 2, 3], vec![4, 5, 6]];
        let mut buf = Vec::new();
        write_tuples(&mut buf, &tuples).unwrap();
        assert_eq!(String::from_utf8(buf).unwrap(), "1,2,3\n4,5,6\n");
    }

    #[test]
    fn write_tuples_single_column() {
        let tuples = vec![vec![10], vec![20]];
        let mut buf = Vec::new();
        write_tuples(&mut buf, &tuples).unwrap();
        assert_eq!(String::from_utf8(buf).unwrap(), "10\n20\n");
    }

    #[test]
    fn write_tuples_empty() {
        let tuples: Vec<Vec<usize>> = vec![];
        let mut buf = Vec::new();
        write_tuples(&mut buf, &tuples).unwrap();
        assert_eq!(String::from_utf8(buf).unwrap(), "");
    }
}
