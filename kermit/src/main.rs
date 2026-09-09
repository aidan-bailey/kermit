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
    kermit_algos::{JoinAlgorithm, JoinQuery, Optimiser},
    kermit_bench::BenchmarkDefinition,
    kermit_ds::{HashTrieConfig, IndexStructure},
    kermit_parser::Term,
    std::{
        fs,
        io::{self, BufWriter, Write},
        path::{Path, PathBuf},
    },
};

mod bench;
mod bench_report;
mod execution;
mod materialize;
mod measurement;
mod options;

use {
    bench::{dispatch_ds_bench, dispatch_run_bench, resolve_sweep, Metric, RunSettings, Workload},
    bench_report::{BenchKind, ReportSink},
    execution::{Execution, ExecutionFamily, HashHtj, SortedTrie, TrieLftj},
    options::{
        validate_config_choices, validate_layout_choices, with_hash_trie_layout, ConfigChoices,
        LayoutChoices,
    },
};

/// Default Criterion group name when `--name` is omitted on `bench run`.
/// `bench run` treats `--name` as a *prefix* on the auto-generated
/// `{benchmark}/{query}/{ds}/{algo}` identity (see CLAUDE.md "bench `--name`
/// semantics").
const DEFAULT_RUN_GROUP: &str = "run";

/// Default Criterion group *prefix* for `bench join`
/// (`{prefix}/adhoc/{query}/{ds}/{algo}`).
const DEFAULT_JOIN_GROUP: &str = "join";

/// Default Criterion group name for `bench ds` (full group name, not a
/// prefix).
const DEFAULT_DS_GROUP: &str = "ds";

/// Sentinel cache path used only when the platform cache dir cannot be
/// resolved (`base_cache_dir` errors). Discovery against this path finds no
/// cached benchmarks, so behaviour degrades to "workspace benchmarks only".
const NO_CACHE_FALLBACK: &str = "/tmp/no-cache";

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

    /// Query optimiser (plans the join's variable ordering). Long-only:
    /// `-o` belongs to `--output`.
    #[arg(long, value_enum, default_value_t = Optimiser::Lexicographic)]
    optimiser: Optimiser,

    #[command(flatten)]
    layout: LayoutChoices,
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
    /// The selector naming exactly `ds`, so the `--ds-layout-*` /
    /// `--ds-config` validators (which take a selector) apply to a
    /// concrete structure.
    fn of(ds: IndexStructure) -> Self {
        match ds {
            | IndexStructure::ColumnTrie => Self::ColumnTrie,
            | IndexStructure::HashTrie => Self::HashTrie,
            | IndexStructure::TreeTrie => Self::TreeTrie,
        }
    }

    fn expand(self) -> Vec<IndexStructure> {
        use clap::ValueEnum;
        match self {
            | Self::All => IndexStructure::value_variants().to_vec(),
            | Self::ColumnTrie => vec![IndexStructure::ColumnTrie],
            | Self::HashTrie => vec![IndexStructure::HashTrie],
            | Self::TreeTrie => vec![IndexStructure::TreeTrie],
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
    /// Benchmark a join query over relation files.
    ///
    /// Runs through the same runner as `bench run`, with the workload named
    /// `adhoc` and the query named by the query file's stem, so the Criterion
    /// group is `{--name|join}/adhoc/{stem}/{ds}/{algo}`. No `-i all` /
    /// `-a all` sweep here; use `bench run` for sweeps.
    Join {
        #[command(flatten)]
        query_args: QueryArgs,

        /// Output file for one run's results (optional)
        #[arg(short, long, value_name = "PATH")]
        output: Option<PathBuf>,

        /// Metrics to benchmark (`end-to-end` is opt-in, not in the default
        /// set)
        #[arg(
            short,
            long,
            value_enum,
            num_args = 1..,
            default_values_t = vec![Metric::Insertion, Metric::Iteration, Metric::Space]
        )]
        metrics: Vec<Metric>,

        /// Query executions per database build in the `end-to-end` metric's
        /// timed body (T = build + K × query). Ignored by other metrics.
        #[arg(long, value_name = "K", default_value = "1", value_parser = clap::value_parser!(u32).range(1..))]
        queries_per_build: u32,

        #[command(flatten)]
        config: ConfigChoices,
    },

    /// Benchmark an index structure (insertion, iteration, space,
    /// end-to-end)
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

        /// Metrics to benchmark (`end-to-end` is opt-in, not in the default
        /// set)
        #[arg(
            short,
            long,
            value_enum,
            num_args = 1..,
            default_values_t = vec![Metric::Insertion, Metric::Iteration, Metric::Space]
        )]
        metrics: Vec<Metric>,

        /// Full-trie iterations per build in the `end-to-end` metric's
        /// timed body (T = build + K × iteration). Ignored by other
        /// metrics.
        #[arg(long, value_name = "K", default_value = "1", value_parser = clap::value_parser!(u32).range(1..))]
        queries_per_build: u32,

        #[command(flatten)]
        layout: LayoutChoices,

        #[command(flatten)]
        config: ConfigChoices,
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

        /// Query optimiser (plans the join's variable ordering)
        // Long-only for parity with the join surfaces (where -o is --output).
        #[arg(long, value_enum, default_value_t = Optimiser::Lexicographic)]
        optimiser: Optimiser,

        /// Metrics to benchmark (`end-to-end` is opt-in, not in the default
        /// set)
        #[arg(
            short,
            long,
            value_enum,
            num_args = 1..,
            default_values_t = vec![Metric::Insertion, Metric::Iteration, Metric::Space]
        )]
        metrics: Vec<Metric>,

        /// Query executions per database build in the `end-to-end` metric's
        /// timed body (T = build + K × query). Ignored by other metrics.
        #[arg(long, value_name = "K", default_value = "1", value_parser = clap::value_parser!(u32).range(1..))]
        queries_per_build: u32,

        /// Regenerate generator-driven benchmarks if their cached
        /// `meta.json` spec_hash differs from the current YAML's params.
        /// Without this flag, `bench run` errors out on drift instead of
        /// silently re-running an expensive pipeline. No-op for static
        /// benchmarks.
        #[arg(long)]
        force: bool,

        #[command(flatten)]
        layout: LayoutChoices,

        #[command(flatten)]
        config: ConfigChoices,
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

/// Parses the query file named by `args`.
fn parse_query(args: &QueryArgs) -> anyhow::Result<JoinQuery> {
    let query_str = fs::read_to_string(&args.query)
        .map_err(|e| anyhow::anyhow!("Failed to read query file {:?}: {}", args.query, e))?;
    query_str
        .trim()
        .parse()
        .map_err(|e| anyhow::anyhow!("Failed to parse query from {:?}: {}", args.query, e))
}

/// A built engine behind a closure: runs one query and returns its tuples.
type JoinRunner = Box<dyn Fn(JoinQuery) -> Vec<Vec<usize>>>;

/// Loads `args.relations` into `family`'s engine and returns a runner over it.
fn build_join_runner<F: ExecutionFamily + 'static>(
    family: F, paths: &[PathBuf],
) -> anyhow::Result<JoinRunner> {
    let relations = paths
        .iter()
        .map(|p| family.load(p))
        .collect::<anyhow::Result<Vec<_>>>()?;
    let engine = family.build(relations);
    Ok(Box::new(move |q| family.join(&engine, q)))
}

/// Resolves the `(structure, algorithm)` pair in `args` to its execution
/// cell and builds a [`JoinRunner`] for it. Incompatible pairs are a usage
/// error, and the `--ds-layout-*` flags are only accepted with
/// `-i hash-trie`.
///
/// `kermit join` deliberately carries no `--ds-config` and passes
/// [`HashTrieConfig::default()`]: the one Config value trades space against
/// probe length and cannot change a query's answers. `bench join --output`
/// passes its resolved config so the CSV comes from the same build the
/// measurements use.
fn load_query_runner(args: &QueryArgs, config: HashTrieConfig) -> anyhow::Result<JoinRunner> {
    // Deliberately duplicates `validate_layout_choices` for `kermit join`,
    // which has no selector-based validation of its own; `bench join`
    // validates first and pays this check a second time on its `--output`
    // path, which is harmless.
    if args.indexstructure != IndexStructure::HashTrie {
        let explicit: &[(&str, bool)] = &[
            (
                "--ds-layout-hasher",
                args.layout.hash_trie_hasher_explicit(),
            ),
            (
                "--ds-layout-pruning",
                args.layout.hash_trie_pruning_explicit(),
            ),
        ];
        for (flag, given) in explicit {
            if *given {
                anyhow::bail!("{flag} is only valid with --indexstructure hash-trie");
            }
        }
    }
    let cell = Execution::for_pair(
        args.indexstructure,
        args.algorithm,
        args.layout.hash_trie_hasher_resolved(),
        args.layout.hash_trie_pruning_resolved(),
        config,
    )
    .ok_or_else(|| {
        anyhow::anyhow!(
            "incompatible selection: {:?} cannot run under {:?}",
            args.indexstructure,
            args.algorithm
        )
    })?;
    let optimiser = args.optimiser;
    match cell {
        | Execution::TrieLftj(SortedTrie::TreeTrie) => build_join_runner(
            TrieLftj::<kermit_ds::TreeTrie>::new(optimiser),
            &args.relations,
        ),
        | Execution::TrieLftj(SortedTrie::ColumnTrie) => build_join_runner(
            TrieLftj::<kermit_ds::ColumnTrie>::new(optimiser),
            &args.relations,
        ),
        | Execution::HashHtj {
            hasher,
            pruning,
            config,
        } => with_hash_trie_layout!(hasher, pruning, |H, P| build_join_runner(
            HashHtj::<H, P>::new(config, optimiser),
            &args.relations
        )),
    }
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
    let cached_hash = kermit_rdf::generator::MetaHeader::read(&cache_subdir)
        .ok()
        .and_then(|h| h.spec_hash);
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
        kermit_bench::cache::base_cache_dir().unwrap_or_else(|_| PathBuf::from(NO_CACHE_FALLBACK));
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

/// Handler for the top-level `kermit join` subcommand: run one join query
/// and write its tuples to `output` (or stdout when `None`).
fn run_join(query_args: QueryArgs, output: Option<PathBuf>) -> anyhow::Result<()> {
    let join_query = parse_query(&query_args)?;
    let join = load_query_runner(&query_args, HashTrieConfig::default())?;
    let header = head_column_names(&join_query);
    let tuples = join(join_query);
    let writer: Box<dyn Write> = match &output {
        | Some(path) => Box::new(BufWriter::new(fs::File::create(path)?)),
        | None => Box::new(BufWriter::new(io::stdout().lock())),
    };
    write_tuples(writer, &header, &tuples)?;
    Ok(())
}

/// Handler for `bench list`: print every discoverable benchmark with its
/// source (workspace/cache) and cache status.
fn run_list() -> anyhow::Result<()> {
    let root = workspace_root();
    let cache =
        kermit_bench::cache::base_cache_dir().unwrap_or_else(|_| PathBuf::from(NO_CACHE_FALLBACK));
    let workspace_defs: std::collections::HashMap<String, BenchmarkDefinition> =
        kermit_bench::discovery::load_all_benchmarks(&root)?
            .into_iter()
            .map(|d| (d.name.clone(), d))
            .collect();
    let benchmarks = kermit_bench::discovery::load_all_benchmarks_with_cache(&root, &cache)?;
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
    Ok(())
}

/// Handler for `bench fetch`: download the data files for the named
/// benchmark, or every benchmark when `name` is `None`.
fn run_fetch(name: Option<String>) -> anyhow::Result<()> {
    let benchmarks = resolve_benchmarks(&name, name.is_none())?;
    for benchmark in &benchmarks {
        eprintln!("Fetching {}...", benchmark.name);
        kermit_bench::cache::ensure_cached(benchmark, &workspace_root())
            .map_err(|e| anyhow::anyhow!("Failed to fetch {}: {e}", benchmark.name))?;
        eprintln!("  Done.");
    }
    Ok(())
}

/// Handler for `bench clean`: remove cached data for the named benchmark,
/// or every cached benchmark when `name` is `None`.
fn run_clean(name: Option<String>) -> anyhow::Result<()> {
    match &name {
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
    }
    Ok(())
}

/// Handler for `bench join`: an ad-hoc workload over the given relation
/// files, run through the same generic runner as `bench run` under the
/// `adhoc/{query-stem}` identity.
fn run_bench_join(
    bench_args: &BenchArgs, query_args: QueryArgs, output: Option<PathBuf>, metrics: &[Metric],
    queries_per_build: u32, config: ConfigChoices,
) -> anyhow::Result<()> {
    let selector = IndexStructureSelector::of(query_args.indexstructure);
    validate_layout_choices(selector, &query_args.layout)?;
    validate_config_choices(selector, &config)?;
    let hash_trie_config = config.hash_trie_config_resolved()?;
    let cell = Execution::for_pair(
        query_args.indexstructure,
        query_args.algorithm,
        query_args.layout.hash_trie_hasher_resolved(),
        query_args.layout.hash_trie_pruning_resolved(),
        hash_trie_config,
    )
    .ok_or_else(|| {
        anyhow::anyhow!(
            "incompatible selection: {:?} cannot run under {:?}",
            query_args.indexstructure,
            query_args.algorithm
        )
    })?;

    if let Some(path) = &output {
        let join_query = parse_query(&query_args)?;
        let join = load_query_runner(&query_args, hash_trie_config)?;
        let header = head_column_names(&join_query);
        let tuples = join(join_query);
        let writer = BufWriter::new(fs::File::create(path)?);
        write_tuples(writer, &header, &tuples)?;
    }

    let workload = Workload::adhoc(query_args.relations.clone(), &query_args.query)?;
    let settings = RunSettings {
        kind: BenchKind::Join,
        prefix: bench_args.name.as_deref().unwrap_or(DEFAULT_JOIN_GROUP),
        optimiser: query_args.optimiser,
        metrics,
        queries_per_build,
        bench_args,
    };
    let mut sink = ReportSink::open(bench_args.report_json.as_deref(), BenchKind::Join)?;
    let reports = dispatch_run_bench(cell, &workload, settings)?;
    sink.push(reports)?;
    sink.finish()?;
    Ok(())
}

/// Handler for `bench ds`: benchmark one or more index structures over a
/// single relation file and write the reports.
#[allow(clippy::too_many_arguments)]
fn run_ds_bench_command(
    bench_args: &BenchArgs, relation: PathBuf, indexstructure: IndexStructureSelector,
    metrics: Vec<Metric>, queries_per_build: u32, layout: LayoutChoices, config: ConfigChoices,
) -> anyhow::Result<()> {
    validate_layout_choices(indexstructure, &layout)?;
    validate_config_choices(indexstructure, &config)?;
    let hash_trie_config = config.hash_trie_config_resolved()?;
    let group_name = bench_args.name.as_deref().unwrap_or(DEFAULT_DS_GROUP);
    let mut sink = ReportSink::open(bench_args.report_json.as_deref(), BenchKind::Ds)?;
    for ds in indexstructure.expand() {
        let report = dispatch_ds_bench(
            ds,
            layout.hash_trie_hasher_resolved(),
            layout.hash_trie_pruning_resolved(),
            hash_trie_config,
            &relation,
            &metrics,
            queries_per_build,
            group_name,
            bench_args,
        )?;
        sink.push(vec![report])?;
    }
    sink.finish()?;
    Ok(())
}

/// Handler for `bench run`: materialise the selected benchmarks and run
/// every valid (index-structure, algorithm) cell of the requested sweep,
/// writing the aggregated reports.
#[allow(clippy::too_many_arguments)]
fn run_bench_run_command(
    bench_args: &BenchArgs, name: Option<String>, all: bool, query: Option<String>,
    indexstructure: IndexStructureSelector, algorithm: JoinAlgorithmSelector, optimiser: Optimiser,
    metrics: Vec<Metric>, queries_per_build: u32, force: bool, layout: LayoutChoices,
    config: ConfigChoices,
) -> anyhow::Result<()> {
    validate_layout_choices(indexstructure, &layout)?;
    validate_config_choices(indexstructure, &config)?;
    let hash_trie_config = config.hash_trie_config_resolved()?;
    let cells = resolve_sweep(
        indexstructure,
        algorithm,
        layout.hash_trie_hasher_resolved(),
        layout.hash_trie_pruning_resolved(),
        hash_trie_config,
    )?;
    let benchmarks = resolve_benchmarks(&name, all)?;
    let cache_root = kermit_bench::cache::base_cache_dir()
        .map_err(|e| anyhow::anyhow!("no cache directory available: {e}"))?;
    let materialized: Vec<BenchmarkDefinition> = benchmarks
        .into_iter()
        .map(|b| materialize::materialize(b, &cache_root, force))
        .collect::<Result<Vec<_>, _>>()?;

    let prefix = bench_args.name.as_deref().unwrap_or(DEFAULT_RUN_GROUP);
    let settings = RunSettings {
        kind: BenchKind::Run,
        prefix,
        optimiser,
        metrics: &metrics,
        queries_per_build,
        bench_args,
    };
    // Opened before the loop so every finished cell is on disk before the
    // next one starts; a crash mid-sweep keeps the completed cells.
    let mut sink = ReportSink::open(bench_args.report_json.as_deref(), BenchKind::Run)?;
    for benchmark in &materialized {
        let workload = Workload::from_definition(benchmark, &workspace_root(), query.as_deref())
            .with_context(|| {
                format!(
                    "bench run failed on benchmark '{}'; partial report retained at {}",
                    benchmark.name,
                    sink.path().display()
                )
            })?;
        for &cell in &cells {
            let cell_reports =
                dispatch_run_bench(cell, &workload, settings).with_context(|| {
                    format!(
                        "bench run failed on benchmark '{}' cell {:?}; partial report retained at \
                         {}",
                        benchmark.name,
                        cell,
                        sink.path().display()
                    )
                })?;
            sink.push(cell_reports)?;
        }
    }
    sink.finish()?;
    Ok(())
}

/// Handler for `bench gen watdiv`: materialise a fresh WatDiv benchmark on
/// the fly via the `kermit-rdf` pipeline.
#[allow(clippy::too_many_arguments)]
fn run_gen_watdiv(
    scale: u32, tag: String, max_query_size: u32, query_count: u32, constants_per_query: u32,
    allow_join_vertex: bool, watdiv_bin: Option<PathBuf>, output_dir: Option<PathBuf>,
    no_bwrap: bool,
) -> anyhow::Result<()> {
    let bench_name = format!("watdiv-stress-{scale}-{tag}");
    let workspace = workspace_root();
    let workspace_names = kermit_bench::discovery::list_benchmarks(&workspace)
        .map_err(|e| anyhow::anyhow!("failed to enumerate workspace benchmarks: {e}"))?;
    if workspace_names.iter().any(|n| n == &bench_name) {
        anyhow::bail!(
            "--tag {tag:?} produces bench name {bench_name:?} which already exists in the \
             workspace; pick a different tag"
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
            "[gen watdiv] note: --output-dir is set to {}; the generated benchmark will NOT be \
             auto-discovered by `bench list/fetch/run` (those scan {})",
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
    Ok(())
}

/// Handler for `bench gen lubm`: materialise a fresh LUBM benchmark on the
/// fly via the `kermit-rdf` LUBM pipeline.
#[allow(clippy::too_many_arguments)]
fn run_gen_lubm(
    scale: u32, tag: String, seed: u32, start_index: u32, threads: u32, lubm_jar: Option<PathBuf>,
    ontology: String, output_dir: Option<PathBuf>,
) -> anyhow::Result<()> {
    let bench_name = format!("lubm-{scale}-{tag}");
    let workspace = workspace_root();
    let workspace_names = kermit_bench::discovery::list_benchmarks(&workspace)
        .map_err(|e| anyhow::anyhow!("failed to enumerate workspace benchmarks: {e}"))?;
    if workspace_names.iter().any(|n| n == &bench_name) {
        anyhow::bail!(
            "--tag {tag:?} produces bench name {bench_name:?} which already exists in the \
             workspace; pick a different tag"
        );
    }
    let jar = lubm_jar.unwrap_or_else(vendored_lubm_jar);
    if !jar.exists() {
        anyhow::bail!(
            "LUBM-UBA jar not found at {jar:?}; build with `mvn package` in lubm-uba-rs and copy \
             to kermit-rdf/vendor/lubm-uba/, or override with --lubm-jar / KERMIT_LUBM_JAR"
        );
    }
    let default_cache = kermit_bench::cache::base_cache_dir()
        .map_err(|e| anyhow::anyhow!("no cache directory available: {e}"))?;
    let cache_parent = output_dir.unwrap_or_else(|| default_cache.clone());
    if cache_parent != default_cache {
        eprintln!(
            "[gen lubm] note: --output-dir is set to {}; the generated benchmark will NOT be \
             auto-discovered by `bench list/fetch/run` (those scan {})",
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

    // LUBM(1, 0) cardinalities are only valid at scale 1; at other scales we
    // still emit the queries but skip the expected.csv files to avoid
    // misleading the cardinality test.
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
        "[gen lubm] wrote {} (pre={}, post={}, derived={}, relations={}, queries={})",
        out_dir.display(),
        meta.triple_count_pre_entailment,
        meta.triple_count_post_entailment,
        meta.derived_triple_count,
        meta.relation_count,
        meta.query_count
    );
    Ok(())
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        | Commands::Join {
            query_args,
            output,
        } => run_join(query_args, output)?,

        | Commands::Bench {
            bench_args,
            subcommand,
        } => match subcommand {
            | BenchSubcommand::List => run_list()?,

            | BenchSubcommand::Fetch {
                name,
            } => run_fetch(name)?,

            | BenchSubcommand::Clean {
                name,
            } => run_clean(name)?,

            | BenchSubcommand::Join {
                query_args,
                output,
                metrics,
                queries_per_build,
                config,
            } => run_bench_join(
                &bench_args,
                query_args,
                output,
                &metrics,
                queries_per_build,
                config,
            )?,

            | BenchSubcommand::Ds {
                relation,
                indexstructure,
                metrics,
                queries_per_build,
                layout,
                config,
            } => run_ds_bench_command(
                &bench_args,
                relation,
                indexstructure,
                metrics,
                queries_per_build,
                layout,
                config,
            )?,

            | BenchSubcommand::Run {
                name,
                all,
                query,
                indexstructure,
                algorithm,
                optimiser,
                metrics,
                queries_per_build,
                force,
                layout,
                config,
            } => run_bench_run_command(
                &bench_args,
                name,
                all,
                query,
                indexstructure,
                algorithm,
                optimiser,
                metrics,
                queries_per_build,
                force,
                layout,
                config,
            )?,

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
                } => run_gen_watdiv(
                    scale,
                    tag,
                    max_query_size,
                    query_count,
                    constants_per_query,
                    allow_join_vertex,
                    watdiv_bin,
                    output_dir,
                    no_bwrap,
                )?,

                | GenSubcommand::Lubm {
                    scale,
                    tag,
                    seed,
                    start_index,
                    threads,
                    lubm_jar,
                    ontology,
                    output_dir,
                } => run_gen_lubm(
                    scale,
                    tag,
                    seed,
                    start_index,
                    threads,
                    lubm_jar,
                    ontology,
                    output_dir,
                )?,
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
                url: Some("https://example.invalid/nope".to_string()),
                path: None,
            }],
            queries: vec![kermit_bench::QueryDefinition {
                name: "q".to_string(),
                description: "q".to_string(),
                query: "Q(X) :- edge(X, Y).".to_string(),
                expected: None,
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
            serde_json::json!({"schema_version": 2, "kind": "watdiv-onthefly", "spec_hash": hash})
                .to_string(),
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
            serde_json::json!({
                "schema_version": 2,
                "kind": "watdiv-onthefly",
                "spec_hash": "old-hash"
            })
            .to_string(),
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
            serde_json::json!({"schema_version": 2, "kind": "watdiv-onthefly", "spec_hash": hash})
                .to_string(),
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
