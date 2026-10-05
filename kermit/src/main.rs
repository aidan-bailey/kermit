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
    kermit::db::{validate_query, JoinError},
    kermit_algos::{ColumnOrderPolicy, JoinAlgorithm, JoinQuery},
    kermit_bench::BenchmarkDefinition,
    kermit_ds::{IndexStructure, Relation, RelationHeader},
    kermit_parser::Term,
    std::{
        collections::BTreeMap,
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
    bench::{
        check_sweep_group_directories, dispatch_ds_bench, dispatch_run_bench, resolve_sweep,
        Metric, RunSettings, Workload,
    },
    bench_report::{BenchKind, ReportSink},
    execution::{read_relation_header, Execution, ExecutionFamily, HashHtj, SortedTrie, TrieLftj},
    options::{
        with_hash_trie_layout, with_sorted_trie_layout, BuildChoices, ConfigChoices, DsChoices,
        DsFlag, LayoutChoices, PlannerArgs,
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

    #[command(flatten)]
    planner: PlannerArgs,

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
    /// `--ds-config` / `--ds-build` validators (which take a selector) apply
    /// to a concrete structure.
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

        #[command(flatten)]
        build: BuildChoices,
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

        #[command(flatten)]
        build: BuildChoices,
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

        #[command(flatten)]
        planner: PlannerArgs,

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

        /// Run each query once before timing and check its result count
        /// against the benchmark's `expected` value; a mismatch aborts.
        /// Queries without `expected` are noted and skipped.
        #[arg(long)]
        verify: bool,

        #[command(flatten)]
        layout: LayoutChoices,

        #[command(flatten)]
        config: ConfigChoices,

        #[command(flatten)]
        build: BuildChoices,
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
        #[arg(
            long,
            value_name = "N",
            required = true,
            value_parser = clap::value_parser!(u32).range(1..)
        )]
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
        #[arg(
            long,
            value_name = "N",
            required = true,
            value_parser = clap::value_parser!(u32).range(1..)
        )]
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

/// Column names derived from a query's head predicate: one per head term,
/// matching the columns the join emits (it projects to the head). A query
/// reaching the join has a head of distinct variables — validation rejects
/// head constants and placeholders — so the `Atom` (`"c<id>"`) and
/// `Placeholder` (`"_"`) arms only keep this function total.
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

/// Streams a join result as CSV: the header row, then one line per tuple as
/// the join produces it, so writing a result never materialises it (the
/// memory half of issue #65: a result larger than RAM is written, not
/// killed). A join's sink cannot fail, so the first I/O error is kept,
/// later rows are dropped, and [`finish`](Self::finish) returns it.
struct CsvSink<W: Write> {
    writer: W,
    error: Option<io::Error>,
}

impl<W: Write> CsvSink<W> {
    /// Writes the header row (if any) and returns a sink for the rows.
    fn new(mut writer: W, header: &[String]) -> io::Result<Self> {
        if !header.is_empty() {
            writeln!(writer, "{}", header.join(","))?;
        }
        Ok(Self {
            writer,
            error: None,
        })
    }

    /// Writes one row, unless an earlier write failed.
    fn write_tuple(&mut self, tuple: &[usize]) {
        if self.error.is_none() {
            if let Err(e) = write_row(&mut self.writer, tuple) {
                self.error = Some(e);
            }
        }
    }

    /// Flushes the writer, or returns the first write error.
    fn finish(mut self) -> io::Result<()> {
        match self.error.take() {
            | Some(e) => Err(e),
            | None => self.writer.flush(),
        }
    }
}

/// Writes `tuple` as one comma-separated line, without allocating.
fn write_row(writer: &mut impl Write, tuple: &[usize]) -> io::Result<()> {
    for (i, value) in tuple.iter().enumerate() {
        if i > 0 {
            writer.write_all(b",")?;
        }
        write!(writer, "{value}")?;
    }
    writer.write_all(b"\n")
}

/// Runs `query` through `join`, streaming its header and rows to `writer`.
fn write_join(
    writer: impl Write, header: &[String], join: &mut JoinRunner, query: JoinQuery,
) -> anyhow::Result<()> {
    let mut sink = CsvSink::new(writer, header)?;
    join(query, &mut |tuple| sink.write_tuple(tuple))?;
    sink.finish()?;
    Ok(())
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

/// A built engine behind a closure: runs one query, passing each result
/// tuple to the sink as the join produces it.
type JoinRunner = Box<dyn FnMut(JoinQuery, &mut dyn FnMut(&[usize])) -> Result<(), JoinError>>;

/// Loads `args.relations` into `family`'s engine and returns a runner over it.
/// `cell` is the cell the caller resolved, which `family` must implement.
/// Under `--column-orders any` the file-order tuples are kept, so a query's
/// reordered copies can be built from them
/// (`RelationFamily::load_with_tuples`) before its join and dropped after;
/// under `stored` nothing is kept.
fn build_join_runner<F: ExecutionFamily + 'static>(
    family: F, cell: Execution, paths: &[PathBuf], column_orders: ColumnOrderPolicy,
) -> anyhow::Result<JoinRunner> {
    // The joins this runner serves (`kermit join`, `bench join --output`)
    // write CSV, not a report axis, and every Layout gives the same answers,
    // so this is the only check that a dispatch arm monomorphised the cell it
    // was asked for: `execution` re-derives the Layout labels from the
    // family's type parameters.
    debug_assert_eq!(
        family.execution(),
        cell,
        "dispatch built a different cell than the one resolved"
    );
    let mut relations = Vec::with_capacity(paths.len());
    let mut inputs: BTreeMap<String, (RelationHeader, Vec<Vec<usize>>)> = BTreeMap::new();
    for path in paths {
        if column_orders == ColumnOrderPolicy::Any {
            let (relation, tuples) = family.load_with_tuples(path)?;
            let header = relation.header().clone();
            inputs.insert(header.name().to_string(), (header, tuples));
            relations.push(relation);
        } else {
            relations.push(family.load(path)?);
        }
    }
    let mut engine = family.build(relations);
    Ok(Box::new(move |q, sink| {
        for spec in family.required_indexes(&engine, &q)? {
            let (header, tuples) = inputs
                .get(&spec.base)
                .expect("validation checked that every base relation was loaded");
            family.add_index(&mut engine, spec, header, tuples);
        }
        let result = family.join_for_each(&engine, q, sink);
        F::clear_indexes(&mut engine);
        result
    }))
}

/// Checks `query` against the headers of `args.relations`, read without
/// their tuples, so a query that cannot run fails before any relation is
/// built. The error names the query file and is the same for every
/// structure.
fn validate_query_files(query: &JoinQuery, args: &QueryArgs) -> anyhow::Result<()> {
    let headers = args
        .relations
        .iter()
        .map(|path| read_relation_header(path))
        .collect::<anyhow::Result<Vec<_>>>()?;
    validate_query(query, headers.as_slice(), args.planner.column_orders)
        .map_err(|e| anyhow::anyhow!("query {:?}: {e}", args.query))
}

/// Resolves the `(structure, algorithm)` pair in `args` to its execution
/// cell, carrying `choices`, which both callers obtain from
/// `DsChoices::resolve` (so every `--ds-*` flag is already validated).
/// Incompatible pairs are a usage error.
///
/// `kermit join` deliberately carries neither `--ds-config` nor `--ds-build`:
/// it resolves their defaults, because neither can change a query's answers
/// (the one Config value trades space against probe length, and every build
/// mode builds the same structure).
fn query_cell(args: &QueryArgs, choices: DsChoices) -> anyhow::Result<Execution> {
    Execution::for_pair(args.indexstructure, args.algorithm, choices).ok_or_else(|| {
        anyhow::anyhow!(
            "incompatible selection: {:?} cannot run under {:?}",
            args.indexstructure,
            args.algorithm
        )
    })
}

/// Builds a [`JoinRunner`] for `cell` over `args.relations`. `bench join
/// --output` passes the very cell its measurements run, so the CSV comes from
/// the same build.
fn load_query_runner(args: &QueryArgs, cell: Execution) -> anyhow::Result<JoinRunner> {
    // One planner per family: a `Planner` owns its optimiser.
    let planner = || args.planner.instantiate();
    let column_orders = args.planner.column_orders;
    match cell {
        | Execution::TrieLftj(SortedTrie::TreeTrie {
            seek,
        }) => with_sorted_trie_layout!(seek, |S| build_join_runner(
            TrieLftj::<kermit_ds::TreeTrie<S>>::new((), planner()),
            cell,
            &args.relations,
            column_orders,
        )),
        | Execution::TrieLftj(SortedTrie::ColumnTrie {
            seek,
            build,
        }) => with_sorted_trie_layout!(seek, |S| build_join_runner(
            TrieLftj::<kermit_ds::ColumnTrie<S>>::new(build, planner()),
            cell,
            &args.relations,
            column_orders,
        )),
        | Execution::HashHtj {
            hasher,
            pruning,
            expansion,
            config,
            build,
        } => with_hash_trie_layout!(hasher, pruning, expansion, |H, P, E| build_join_runner(
            HashHtj::<H, P, E>::new(config, build, planner()),
            cell,
            &args.relations,
            column_orders,
        )),
    }
}

/// Returns the `bench list` status string for a benchmark.
///
/// For static benchmarks the values are "cached" / "not cached" (matching
/// the historical behaviour), or "stale" for a generated cache with no
/// workspace spec (an imperative `bench gen` output) whose encoding is
/// outdated. For generator-driven benchmarks the values
/// distinguish "not generated" (no cache subdir), "cached" (subdir exists
/// and `meta.json` spec_hash matches the spec), and "stale" (subdir
/// exists but the spec has drifted, or the cache predates a pipeline change
/// to this spec's output — see `MetaHeader::outdated_reason`).
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
    let cache_subdir = cache_root.join(&b.name);
    let header = kermit_rdf::generator::MetaHeader::read(&cache_subdir).ok();
    let Some(spec) = spec else {
        if header
            .as_ref()
            .is_some_and(|h| h.outdated_reason().is_some())
        {
            return "stale";
        }
        return if kermit_bench::cache::is_cached(b).unwrap_or(false) {
            "cached"
        } else {
            "not cached"
        };
    };
    if !cache_subdir.join("meta.json").exists() {
        return "not generated";
    }
    match header {
        | Some(h)
            if h.spec_hash.as_deref() == Some(spec.spec_hash().as_str())
                && h.outdated_reason().is_none() =>
        {
            "cached"
        },
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
    validate_query_files(&join_query, &query_args)?;
    // `kermit join` has no `--ds-config` / `--ds-build` (neither can change
    // an answer), so it resolves its layout flags against their defaults,
    // through the same validator as every bench command.
    let choices = DsChoices::resolve(
        IndexStructureSelector::of(query_args.indexstructure),
        &query_args.layout,
        &ConfigChoices::default(),
        &BuildChoices::default(),
    )?;
    let mut join = load_query_runner(&query_args, query_cell(&query_args, choices)?)?;
    let header = head_column_names(&join_query);
    let writer: Box<dyn Write> = match &output {
        | Some(path) => Box::new(BufWriter::new(fs::File::create(path)?)),
        | None => Box::new(BufWriter::new(io::stdout().lock())),
    };
    write_join(writer, &header, &mut join, join_query)
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

/// Handler for `bench fetch`: cache every relation of the named benchmark
/// (or all of them) and re-hash the ones that declare a `sha256`.
fn run_fetch(name: Option<String>) -> anyhow::Result<()> {
    let benchmarks = resolve_benchmarks(&name, name.is_none())?;
    let root = workspace_root();
    for benchmark in &benchmarks {
        eprintln!("Fetching {}...", benchmark.name);
        kermit_bench::cache::ensure_cached(benchmark, &root)
            .map_err(|e| anyhow::anyhow!("Failed to fetch {}: {e}", benchmark.name))?;
        // A cold fetch hashes a fresh download twice (once in ensure_cached,
        // once here); the re-read is cheap and keeps the report's count honest
        // for cached files too.
        let checked = kermit_bench::cache::verify_integrity(benchmark, &root)
            .map_err(|e| anyhow::anyhow!("Failed to verify {}: {e}", benchmark.name))?;
        if checked == 0 {
            eprintln!("  No integrity hashes declared.");
        } else {
            eprintln!("  Verified {checked} relation(s).");
        }
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
    queries_per_build: u32, config: ConfigChoices, build: BuildChoices,
) -> anyhow::Result<()> {
    let selector = IndexStructureSelector::of(query_args.indexstructure);
    let choices = DsChoices::resolve(selector, &query_args.layout, &config, &build)?;
    let cell = query_cell(&query_args, choices)?;

    if let Some(path) = &output {
        let join_query = parse_query(&query_args)?;
        validate_query_files(&join_query, &query_args)?;
        let mut join = load_query_runner(&query_args, cell)?;
        let header = head_column_names(&join_query);
        let writer = BufWriter::new(fs::File::create(path)?);
        write_join(writer, &header, &mut join, join_query)?;
    }

    let workload = Workload::adhoc(query_args.relations.clone(), &query_args.query)?;
    let settings = RunSettings {
        kind: BenchKind::Join,
        prefix: bench_args.name.as_deref().unwrap_or(DEFAULT_JOIN_GROUP),
        planner: query_args.planner,
        metrics,
        queries_per_build,
        verify: false,
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
    build: BuildChoices,
) -> anyhow::Result<()> {
    // None of `bench ds`'s metrics calls `seek`: insertion builds the
    // relation, iteration and end_to_end scan it with open/next/up
    // (`TrieIteratorWrapper::advance`), and space measures it. Accepting
    // the flag would label identical measurements as a seek ablation.
    if layout.sorted_trie_seek_explicit() {
        anyhow::bail!(
            "--ds-layout-seek has no effect on bench ds: none of its metrics calls seek \
             (insertion builds the relation; iteration and end-to-end scan it with open/next/up; \
             space measures it). Time a join with bench run or bench join."
        );
    }
    let choices = DsChoices::resolve(indexstructure, &layout, &config, &build)?;
    let group_name = bench_args.name.as_deref().unwrap_or(DEFAULT_DS_GROUP);
    let mut sink = ReportSink::open(bench_args.report_json.as_deref(), BenchKind::Ds)?;
    for ds in indexstructure.expand() {
        let report = dispatch_ds_bench(
            ds,
            choices,
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
    indexstructure: IndexStructureSelector, algorithm: JoinAlgorithmSelector, planner: PlannerArgs,
    metrics: Vec<Metric>, queries_per_build: u32, force: bool, verify: bool, layout: LayoutChoices,
    config: ConfigChoices, build: BuildChoices,
) -> anyhow::Result<()> {
    let choices = DsChoices::resolve(indexstructure, &layout, &config, &build)?;
    let given = DsFlag::given(&layout, &config, &build);
    let cells = resolve_sweep(indexstructure, algorithm, choices, &given)?;
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
        planner,
        metrics: &metrics,
        queries_per_build,
        verify,
        bench_args,
    };
    // Names only — nothing is fetched — so a sweep whose cells would
    // overwrite each other's Criterion results fails before any is timed.
    check_sweep_group_directories(prefix, &materialized, query.as_deref(), &cells)?;

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

    // Reference cardinalities are only attached for LUBM(1, 0).
    let queries = kermit_rdf::lubm::queries::lubm_query_specs(
        kermit_rdf::lubm::queries::lubm_reference_applies(scale, seed, start_index),
    );

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
                build,
            } => run_bench_join(
                &bench_args,
                query_args,
                output,
                &metrics,
                queries_per_build,
                config,
                build,
            )?,

            | BenchSubcommand::Ds {
                relation,
                indexstructure,
                metrics,
                queries_per_build,
                layout,
                config,
                build,
            } => run_ds_bench_command(
                &bench_args,
                relation,
                indexstructure,
                metrics,
                queries_per_build,
                layout,
                config,
                build,
            )?,

            | BenchSubcommand::Run {
                name,
                all,
                query,
                indexstructure,
                algorithm,
                planner,
                metrics,
                queries_per_build,
                force,
                verify,
                layout,
                config,
                build,
            } => run_bench_run_command(
                &bench_args,
                name,
                all,
                query,
                indexstructure,
                algorithm,
                planner,
                metrics,
                queries_per_build,
                force,
                verify,
                layout,
                config,
                build,
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

    /// Streams `tuples` through a [`CsvSink`] over `header` and returns the
    /// text written.
    fn csv(header: &[&str], tuples: &[&[usize]]) -> String {
        let header: Vec<String> = header.iter().map(|h| h.to_string()).collect();
        let mut buf = Vec::new();
        let mut sink = CsvSink::new(&mut buf, &header).unwrap();
        for tuple in tuples {
            sink.write_tuple(tuple);
        }
        sink.finish().unwrap();
        String::from_utf8(buf).unwrap()
    }

    #[test]
    fn csv_sink_without_header_emits_only_rows() {
        assert_eq!(csv(&[], &[&[1, 2, 3], &[4, 5, 6]]), "1,2,3\n4,5,6\n");
    }

    #[test]
    fn csv_sink_emits_the_header_then_rows() {
        assert_eq!(
            csv(&["X", "Y", "Z"], &[&[1, 2, 3], &[4, 5, 6]]),
            "X,Y,Z\n1,2,3\n4,5,6\n"
        );
    }

    #[test]
    fn csv_sink_single_column() {
        assert_eq!(csv(&[], &[&[10], &[20]]), "10\n20\n");
    }

    #[test]
    fn csv_sink_with_no_rows_writes_only_the_header() {
        assert_eq!(csv(&[], &[]), "");
        assert_eq!(csv(&["X"], &[]), "X\n");
    }

    /// Writing a result allocates nothing per row, so `kermit join`'s memory
    /// does not grow with the result (#65): the counts at two result sizes
    /// 1000x apart must be equal.
    #[test]
    fn csv_sink_allocates_nothing_per_row() {
        let allocations = |rows: usize| {
            allocation_counter::measure(|| {
                let header = ["X".to_string(), "Y".to_string()];
                let mut sink = CsvSink::new(io::sink(), &header).unwrap();
                for i in 0..rows {
                    sink.write_tuple(&[i, i + 1]);
                }
                sink.finish().unwrap();
            })
            .count_total
        };
        assert_eq!(allocations(10), allocations(10_000));
    }

    /// A writer that accepts `capacity` bytes, then fails every write.
    struct FailAfter {
        capacity: usize,
        writes: usize,
    }

    impl Write for FailAfter {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.writes += 1;
            if buf.len() > self.capacity {
                return Err(io::Error::other("disk full"));
            }
            self.capacity -= buf.len();
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> { Ok(()) }
    }

    /// A join sink cannot fail, so the first write error is kept, later
    /// rows are not attempted, and `finish` returns the error.
    #[test]
    fn csv_sink_reports_the_first_write_error_and_stops_writing() {
        let mut writer = FailAfter {
            capacity: 4,
            writes: 0,
        };
        let mut sink = CsvSink::new(&mut writer, &[]).unwrap();
        sink.write_tuple(&[1, 2]);
        sink.write_tuple(&[3, 4]);
        let writes_at_error = sink.writer.writes;
        sink.write_tuple(&[5, 6]);
        assert_eq!(
            sink.writer.writes, writes_at_error,
            "kept writing after an error"
        );
        let err = sink.finish().unwrap_err();
        assert_eq!(err.to_string(), "disk full");
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
                sha256: None,
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
            serde_json::json!({
                "schema_version": kermit_rdf::generator::META_SCHEMA_VERSION,
                "kind": "watdiv-onthefly",
                "spec_hash": hash
            })
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

    fn lubm_status(schema_version: u32) -> &'static str {
        let dir = tempfile::tempdir().unwrap();
        let spec = kermit_bench::GeneratorSpec::Lubm {
            scale: 1,
            seed: 0,
            threads: 1,
            start_index: 0,
            ontology: kermit_rdf::lubm::driver::DEFAULT_ONTOLOGY_IRI.to_string(),
            queries: None,
        };
        let subdir = dir.path().join("lubm-x");
        fs::create_dir_all(&subdir).unwrap();
        fs::write(
            subdir.join("meta.json"),
            serde_json::json!({
                "schema_version": schema_version,
                "kind": "lubm-onthefly",
                "spec_hash": spec.spec_hash()
            })
            .to_string(),
        )
        .unwrap();
        let def = make_generator_def("lubm-x", spec);
        describe_benchmark_status(&def, Some(&def), dir.path())
    }

    /// Issue #75: a WatDiv cache written before the injective variable
    /// mapping matches its spec, but its queries use the old uppercased
    /// variable names, so it is stale like any other outdated encoding.
    #[test]
    fn status_stale_for_watdiv_cache_before_injective_variable_mapping() {
        let dir = tempfile::tempdir().unwrap();
        let spec = kermit_bench::GeneratorSpec::Watdiv {
            scale: 1,
            stress: kermit_bench::WatdivStressSpec::default(),
        };
        let subdir = dir.path().join("watdiv-old");
        fs::create_dir_all(&subdir).unwrap();
        fs::write(
            subdir.join("meta.json"),
            serde_json::json!({
                "schema_version": 3,
                "kind": "watdiv-onthefly",
                "spec_hash": spec.spec_hash()
            })
            .to_string(),
        )
        .unwrap();
        let def = make_generator_def("watdiv-old", spec);
        assert_eq!(
            describe_benchmark_status(&def, Some(&def), dir.path()),
            "stale"
        );
    }

    /// Issue #74: a LUBM cache written before entailment became
    /// reproducible matches its spec but not today's encoding of it.
    #[test]
    fn status_stale_for_lubm_cache_before_reproducible_entailment() {
        assert_eq!(lubm_status(2), "stale");
        assert_eq!(
            lubm_status(kermit_rdf::generator::META_SCHEMA_VERSION),
            "cached"
        );
    }

    /// A generated cache with no workspace spec (imperative `bench gen`) is
    /// listed through the static branch, but must still show an outdated
    /// encoding.
    #[test]
    fn status_stale_for_cache_only_lubm_cache_before_reproducible_entailment() {
        let dir = tempfile::tempdir().unwrap();
        let subdir = dir.path().join("lubm-1-old");
        fs::create_dir_all(&subdir).unwrap();
        fs::write(
            subdir.join("meta.json"),
            serde_json::json!({"schema_version": 2, "kind": "lubm-onthefly"}).to_string(),
        )
        .unwrap();
        let def = make_static_def("lubm-1-old");
        assert_eq!(describe_benchmark_status(&def, None, dir.path()), "stale");
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
            serde_json::json!({
                "schema_version": kermit_rdf::generator::META_SCHEMA_VERSION,
                "kind": "watdiv-onthefly",
                "spec_hash": hash
            })
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
