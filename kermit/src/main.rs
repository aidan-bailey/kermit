use clap::{Parser, Subcommand};
use std::path::PathBuf;

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
        /// Input data paths (files or directories)
        #[arg(short, long, value_name = "PATH", num_args = 1.., required = true)]
        input: Vec<PathBuf>,

        /// Query file path
        #[arg(short, long, value_name = "PATH", required = true)]
        query: PathBuf,

        /// Join algorithm
        #[arg(short, long, value_name = "ALGORITHM", required = true)]
        algorithm: String,

        /// Data structure
        #[arg(short, long, value_name = "DATASTRUCTURE", required = true)]
        datastructure: String,

        /// Output file (optional, defaults to stdout)
        #[arg(short, long, value_name = "PATH")]
        output: Option<PathBuf>,
    },
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    if cli.verbose {
        println!("Verbose mode enabled");
    }

    match cli.command {
        Commands::Join {
            input,
            query,
            algorithm,
            datastructure,
            output,
        } => {
            println!("Running join query:");
            println!("  Input files: {:?}", input);
            println!("  Output: {:?}", output.unwrap_or_else(|| PathBuf::from("stdout")));
            println!("  Query: {:?}", query);
            println!("  Data structure: {}", datastructure);
            println!("  Algorithm: {}", algorithm);
            println!("\n[TODO: Implement join execution]");
        }
    }

    Ok(())
}

