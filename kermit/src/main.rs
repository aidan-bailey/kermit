use {
    clap::{Parser, Subcommand},
    std::path::PathBuf,
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
        #[arg(short, long, value_name = "ALGORITHM", required = true)]
        algorithm: String,

        /// Data structure
        #[arg(short, long, value_name = "INDEXSTRUCTURE", required = true)]
        indexstructure: String,

        /// Output file (optional, defaults to stdout)
        #[arg(short, long, value_name = "PATH")]
        output: Option<PathBuf>,
    },

    /// Run a benchmark
    Benchmark {
        /// Benchmark name
        #[arg(short, long, value_name = "NAME", required = true)]
        name: String,

        /// Join algorithm
        #[arg(short, long, value_name = "ALGORITHM", required = true)]
        algorithm: String,

        /// Index structure
        #[arg(short, long, value_name = "INDEXSTRUCTURE", required = true)]
        indexstructure: String,

        /// Dataset directory (generated)
        #[arg(short, long, value_name = "PATH", default_value = "benchmarks")]
        dataset_dir: PathBuf,

        /// Output directory for results (generated)
        #[arg(short, long, value_name = "PATH", default_value = "results")]
        output_dir: PathBuf,
    },
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    if cli.verbose {
        println!("Verbose mode enabled");
    }

    match cli.command {
        | Commands::Join {
            relations: input,
            query,
            algorithm,
            indexstructure,
            output,
        } => {
            println!("Running join query:");
            println!("  Input files: {:?}", input);
            println!(
                "  Output: {:?}",
                output.unwrap_or_else(|| PathBuf::from("stdout"))
            );
            println!("  Query: {:?}", query);
            println!("  Index structure: {}", indexstructure);
            println!("  Algorithm: {}", algorithm);
            todo!("Implement join execution");
        },

        | Commands::Benchmark {
            name,
            dataset_dir,
            output_dir,
            algorithm,
            indexstructure,
        } => {
            println!("Running benchmarks:");
            println!("  Benchmark name: {}", name);
            println!("  Index structure: {}", indexstructure);
            println!("  Algorithm: {}", algorithm);
            println!("  Dataset directory: {:?}", dataset_dir);
            println!("  Output directory: {:?}", output_dir);
            todo!("Implement benchmark execution");
        },
    }

    Ok(())
}
