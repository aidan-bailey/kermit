use {
    clap::{Args, Parser, Subcommand},
    kermit::db::instantiate_database,
    kermit_algos::{JoinAlgorithm, JoinQuery},
    kermit_bench::benchmarks::Benchmark,
    kermit_ds::{HeapSize, IndexStructure, Relation, RelationFileExt},
    kermit_iters::TrieIterable,
    std::{
        fs,
        io::{self, BufWriter, Write},
        path::{Path, PathBuf},
        time::Duration,
    },
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

    /// Run a benchmark suite
    Benchmark {
        /// Benchmark to run
        #[arg(short, long, value_name = "NAME", required = true, value_enum)]
        benchmark: Benchmark,

        /// Join algorithm
        #[arg(short, long, value_name = "ALGORITHM", required = true, value_enum)]
        algorithm: JoinAlgorithm,

        /// Index structure
        #[arg(
            short,
            long,
            value_name = "INDEXSTRUCTURE",
            required = true,
            value_enum
        )]
        indexstructure: IndexStructure,

        /// Dataset directory (generated)
        #[arg(short, long, value_name = "PATH", default_value = "datasets")]
        dataset_dir: PathBuf,

        /// Results directory for benchmarks (generated)
        #[arg(short, long, value_name = "PATH", default_value = "results")]
        results_dir: PathBuf,
    },
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

fn run_ds_bench<R>(
    relation_path: &Path,
    indexstructure: IndexStructure,
    metrics: &[Metric],
    group_name: &str,
    criterion: &mut criterion::Criterion,
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

    // Extract tuples for the insertion benchmark setup closure
    let tuples: Vec<Vec<usize>> = relation.trie_iter().into_iter().collect();
    let header = relation.header().clone();

    let ds_name = format!("{:?}", indexstructure);

    eprintln!("--- bench ds metadata ---");
    eprintln!("  data structure:  {}", ds_name);
    eprintln!("  relation:        {}", relation_path.display());
    eprintln!("  tuples:          {}", tuples.len());
    eprintln!("  arity:           {}", header.arity());

    if metrics.contains(&Metric::Space) {
        eprintln!("  heap bytes:      {}", relation.heap_size_bytes());
    }

    let has_criterion_metrics = metrics
        .iter()
        .any(|m| matches!(m, Metric::Insertion | Metric::Iteration));

    if has_criterion_metrics {
        let mut group = criterion.benchmark_group(group_name);

        if metrics.contains(&Metric::Insertion) {
            let insertion_tuples = tuples.clone();
            let insertion_header = header.clone();
            group.bench_function(format!("{ds_name}/insertion"), |b| {
                b.iter_batched(
                    || (insertion_header.clone(), insertion_tuples.clone()),
                    |(h, t)| R::from_tuples(h, t),
                    criterion::BatchSize::SmallInput,
                );
            });
        }

        if metrics.contains(&Metric::Iteration) {
            group.bench_function(format!("{ds_name}/iteration"), |b| {
                b.iter(|| relation.trie_iter().into_iter().collect::<Vec<_>>());
            });
        }

        group.finish();
        criterion.final_summary();
    }

    Ok(())
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    if cli.verbose {
        println!("Verbose mode enabled");
    }

    match cli.command {
        | Commands::Join {
            query_args,
            output,
        } => {
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
        } => {
            let mut criterion = criterion::Criterion::default()
                .sample_size(bench_args.sample_size)
                .measurement_time(Duration::from_secs(bench_args.measurement_time))
                .warm_up_time(Duration::from_secs(bench_args.warm_up_time));

            match subcommand {
                | BenchSubcommand::Join {
                    query_args,
                    output,
                } => {
                    let (db, join_query) = load_query(&query_args)?;

                    if let Some(path) = &output {
                        let tuples = db.join(join_query.clone());
                        let writer = BufWriter::new(fs::File::create(path)?);
                        write_tuples(writer, &tuples)?;
                    }

                    let group_name = bench_args.name.as_deref().unwrap_or("join");
                    let bench_id =
                        format!("{:?}/{:?}", query_args.indexstructure, query_args.algorithm);

                    eprintln!("--- bench metadata ---");
                    eprintln!("  data structure:  {:?}", query_args.indexstructure);
                    eprintln!("  algorithm:       {:?}", query_args.algorithm);
                    eprintln!("  relations:       {}", query_args.relations.len());

                    let mut group = criterion.benchmark_group(group_name);
                    group.bench_function(&bench_id, |b| {
                        b.iter_batched(
                            || join_query.clone(),
                            |q| db.join(q),
                            criterion::BatchSize::SmallInput,
                        );
                    });
                    group.finish();
                    criterion.final_summary();
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
                                &mut criterion,
                            )?;
                        },
                        | IndexStructure::ColumnTrie => {
                            run_ds_bench::<kermit_ds::ColumnTrie>(
                                &relation,
                                indexstructure,
                                &metrics,
                                group_name,
                                &mut criterion,
                            )?;
                        },
                    }
                },
            }
        },

        | Commands::Benchmark {
            benchmark,
            dataset_dir,
            results_dir,
            algorithm,
            indexstructure,
        } => {
            println!("Running benchmarks:");
            println!("  Benchmark: {:?}", benchmark.name());
            println!("  Index structure: {:?}", indexstructure);
            println!("  Algorithm: {:?}", algorithm);
            println!("  Dataset directory: {:?}", dataset_dir);
            println!("  Results directory: {:?}", results_dir);
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
