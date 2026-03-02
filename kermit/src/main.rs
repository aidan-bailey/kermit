use {
    clap::{Parser, Subcommand},
    kermit::db::instantiate_database,
    kermit_algos::{JoinAlgorithm, JoinQuery},
    kermit_bench::benchmarks::Benchmark,
    kermit_ds::IndexStructure,
    std::{
        fs,
        io::{self, BufWriter, Write},
        path::PathBuf,
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

#[derive(Subcommand)]
enum Commands {
    /// Run a join query
    Join {
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

        /// Output file (optional, defaults to stdout)
        #[arg(short, long, value_name = "PATH")]
        output: Option<PathBuf>,
    },

    /// Run a benchmark
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

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    if cli.verbose {
        println!("Verbose mode enabled");
    }

    match cli.command {
        | Commands::Join {
            relations,
            query,
            algorithm,
            indexstructure,
            output,
        } => {
            let query_str = fs::read_to_string(&query)
                .map_err(|e| anyhow::anyhow!("Failed to read query file {:?}: {}", query, e))?;
            let join_query: JoinQuery = query_str
                .trim()
                .parse()
                .map_err(|e| anyhow::anyhow!("Failed to parse query from {:?}: {}", query, e))?;

            let mut db = instantiate_database(indexstructure, algorithm);
            for path in &relations {
                db.add_file(path)
                    .map_err(|e| anyhow::anyhow!("Failed to load relation {:?}: {}", path, e))?;
            }

            let tuples = db.join(join_query);

            let writer: Box<dyn Write> = match &output {
                | Some(path) => Box::new(BufWriter::new(fs::File::create(path)?)),
                | None => Box::new(BufWriter::new(io::stdout().lock())),
            };
            write_tuples(writer, &tuples)?;
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
