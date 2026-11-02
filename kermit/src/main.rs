//! Kermit command-line interface.
//!
//! Two top-level subcommands: `join` (execute a Datalog query against
//! relation files) and `bench` (Criterion-based benchmarks, including the
//! YAML-defined benchmarks under `benchmarks/`, and `bench probe`, their
//! Criterion-free screen).
//!
//! Run `kermit --help` for the full help text; each `clap` `#[arg(help =
//! …)]` string drives that help output. Benchmark YAML schema is documented
//! in the workspace `benchmarks/README.md`.

#![deny(missing_docs)]

use {
    anyhow::Context,
    clap::{Args, Parser, Subcommand},
    kermit_algos::JoinAlgorithm,
    kermit_bench::BenchmarkDefinition,
    kermit_ds::IndexStructure,
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
mod options;

use {
    bench::{
        check_join_group_directories, check_sweep_group_directories, dispatch_ds_bench,
        gen::GenSubcommand, measure::CriterionSettings, resolve_sweep, workload::PreparedWorkload,
        Metric, RunSettings, Workload,
    },
    bench_report::{BenchKind, ReportSink},
    execution::{Execution, ExecutionFamily, HashHtj, SortedTrie, TrieLftj},
    options::{
        with_hash_trie_layout, with_sorted_trie_layout, BuildChoices, ConfigChoices, DsChoices,
        DsFlag, EngineChoices, LayoutChoices, PlannerArgs,
    },
};

/// The process-wide allocator: jemalloc, unless the default `jemalloc` feature
/// is off (#112). glibc returns a freed chunk to the arena that allocated it,
/// so the parallel builds, whose workers free input tuples the calling thread
/// allocated, serialise on that arena: `TreeTrie` `parallel:N` stops near 1.9x
/// on 10⁷ tuples under glibc. See `docs/data-structures/parallel-build.md`.
///
/// Not in this crate's own unit tests: they count allocations through
/// `allocation_counter`, which installs its own global allocator.
#[cfg(all(feature = "jemalloc", not(test)))]
#[global_allocator]
static GLOBAL: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;

/// The allocator this binary runs on, as every bench report's `allocator`
/// axis records it: `"jemalloc"`, or `"system"` (the platform's malloc, glibc
/// on Linux) when built without the `jemalloc` feature. The integration tests
/// check the real binary reports `"jemalloc"` under the feature.
const ALLOCATOR: &str = if cfg!(all(feature = "jemalloc", not(test))) {
    "jemalloc"
} else {
    "system"
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

/// `--materialize-max-bytes`'s default (4 GiB) and bounds (1 MiB, 1 TiB).
const DEFAULT_MATERIALIZE_MAX_BYTES: u64 = 4_294_967_296;
const MIN_MATERIALIZE_MAX_BYTES: u64 = 1_048_576;
const MAX_MATERIALIZE_MAX_BYTES: u64 = 1_099_511_627_776;

/// Parses `--materialize-max-bytes`: whole bytes in
/// `MIN_MATERIALIZE_MAX_BYTES..=MAX_MATERIALIZE_MAX_BYTES`. Outside it, the
/// error names the range.
fn parse_materialize_max_bytes(text: &str) -> Result<u64, String> {
    let range = format!(
        "--materialize-max-bytes must be between {MIN_MATERIALIZE_MAX_BYTES} (1 MiB) and \
         {MAX_MATERIALIZE_MAX_BYTES} (1 TiB)"
    );
    let bytes: u64 = text
        .parse()
        .map_err(|_| format!("`{text}` is not a whole number of bytes; {range}"))?;
    if (MIN_MATERIALIZE_MAX_BYTES..=MAX_MATERIALIZE_MAX_BYTES).contains(&bytes) {
        Ok(bytes)
    } else {
        Err(range)
    }
}

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

/// The memory guard of the opt-in `materialize` metric, on `bench run`,
/// `bench join` and `bench probe`.
#[derive(Args, Clone, Copy, Debug)]
struct MaterializeArgs {
    /// Skip `materialize` (with a note) for a query whose full assignments
    /// would take more than this many bytes (rows × variables × 8), between
    /// 1 MiB and 1 TiB
    #[arg(
        long,
        value_name = "BYTES",
        default_value_t = DEFAULT_MATERIALIZE_MAX_BYTES,
        value_parser = parse_materialize_max_bytes
    )]
    materialize_max_bytes: u64,
}

/// Which benchmarks, queries and cells a `bench run` or `bench probe`
/// covers, and how each query is planned.
#[derive(Args)]
struct RunSelection {
    /// Benchmark name (omit for --all)
    #[arg(value_name = "NAME")]
    name: Option<String>,

    /// Run all available benchmarks
    #[arg(long, conflicts_with = "name")]
    all: bool,

    /// Run only the named query (omit to run all queries)
    #[arg(short, long, value_name = "QUERY")]
    query: Option<String>,

    /// Data structure (`all` sweeps every available structure). Required
    /// unless `--engine` names the cells, and then only together with `-a`.
    #[arg(
        short,
        long,
        value_name = "INDEXSTRUCTURE",
        required_unless_present = "engine",
        requires = "algorithm",
        value_enum
    )]
    indexstructure: Option<IndexStructureSelector>,

    /// Join algorithm (`all` sweeps every available algorithm). Required
    /// unless `--engine` names the cells, and then only together with `-i`.
    #[arg(
        short,
        long,
        value_name = "ALGORITHM",
        required_unless_present = "engine",
        requires = "indexstructure",
        value_enum
    )]
    algorithm: Option<JoinAlgorithmSelector>,

    #[command(flatten)]
    planner: PlannerArgs,

    /// Regenerate generator-driven benchmarks if their cached
    /// `meta.json` spec_hash differs from the current YAML's params.
    /// Without this flag, a drifted benchmark is an error instead of
    /// silently re-running an expensive pipeline. No-op for static
    /// benchmarks.
    #[arg(long)]
    force: bool,

    #[command(flatten)]
    engines: EngineChoices,

    #[command(flatten)]
    layout: LayoutChoices,

    #[command(flatten)]
    config: ConfigChoices,

    #[command(flatten)]
    build: BuildChoices,
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

        /// External engines to run beside the `-i` / `-a` cell (additive:
        /// `-i` and `-a` stay required, since `QueryArgs` is shared with
        /// `kermit join`).
        #[command(flatten)]
        engines: EngineChoices,

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
            default_values_t = vec![Metric::Build, Metric::Iteration, Metric::Space]
        )]
        metrics: Vec<Metric>,

        /// Query executions per index build in the `end-to-end` metric's
        /// timed body (T = build + K × query). Ignored by other metrics.
        #[arg(long, value_name = "K", default_value = "1", value_parser = clap::value_parser!(u32).range(1..))]
        queries_per_build: u32,

        #[command(flatten)]
        config: ConfigChoices,

        #[command(flatten)]
        build: BuildChoices,

        #[command(flatten)]
        materialize: MaterializeArgs,
    },

    /// Benchmark an index structure (build, iteration, space,
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
            default_values_t = vec![Metric::Build, Metric::Iteration, Metric::Space]
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
        #[command(flatten)]
        selection: RunSelection,

        /// Metrics to benchmark (`end-to-end` is opt-in, not in the default
        /// set)
        #[arg(
            short,
            long,
            value_enum,
            num_args = 1..,
            default_values_t = vec![Metric::Build, Metric::Iteration, Metric::Space]
        )]
        metrics: Vec<Metric>,

        /// Query executions per index build in the `end-to-end` metric's
        /// timed body (T = build + K × query). Ignored by other metrics.
        #[arg(long, value_name = "K", default_value = "1", value_parser = clap::value_parser!(u32).range(1..))]
        queries_per_build: u32,

        /// Run each query once before timing and check its result count
        /// against the benchmark's `expected` value; a mismatch aborts.
        /// Queries without `expected` are noted and skipped.
        #[arg(long)]
        verify: bool,

        #[command(flatten)]
        materialize: MaterializeArgs,
    },

    /// Screen a named benchmark without Criterion: verify each query, time
    /// one build and one run, record the exact space
    ///
    /// Per query and cell, the query's indexes are built once and the query
    /// is run once, both timed (single runs, recorded as `values` with
    /// estimator `single`), beside the exact `space`, the answer count and
    /// the plan. Every query whose YAML declares `expected` is verified, and
    /// a mismatch aborts. Takes `bench run`'s selection flags. `bench`'s
    /// Criterion settings (`--name`, `--sample-size`, `--measurement-time`,
    /// `--warm-up-time`) do not apply; `--report-json` does. `--materialize`
    /// adds a single-run `materialize` value (the join with every full
    /// assignment kept, sorted and counted distinct), skipped when over
    /// `--materialize-max-bytes`, as in `bench run`. A single run is cold:
    /// compare it with other single runs, never with a Criterion estimate.
    Probe {
        #[command(flatten)]
        selection: RunSelection,

        /// Also record a single-run `materialize` value
        #[arg(long)]
        materialize: bool,

        #[command(flatten)]
        limits: MaterializeArgs,
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

use materialize::workspace_root;

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

/// Reads and plans the one query of `args`: the query file is parsed,
/// validated against every header in `args.relations`, and prepared over a
/// catalog of the relations it names (`Workload::prepare`). `kermit join`
/// and `bench join` (its CSV and its benchmark alike) share the result, so
/// the relations are read once.
///
/// # Errors
///
/// The query file cannot be read or parsed, a relation file cannot be
/// read, or validation rejects the query, before any index is built.
fn prepare_query(args: &QueryArgs, cell: Execution) -> anyhow::Result<PreparedWorkload> {
    prepare_query_under(args, &[cell.trie_levels()])
}

/// [`prepare_query`] for every trie-level policy in `levels`: `bench join`
/// adds `TrieLevels::All` for its engine cells.
fn prepare_query_under(
    args: &QueryArgs, levels: &[kermit_ds::TrieLevels],
) -> anyhow::Result<PreparedWorkload> {
    Workload::adhoc(args.relations.clone(), &args.query)?
        .prepare(&args.planner.instantiate(), levels)
}

/// Builds the indexes of `workload`'s one query on `family` and streams the
/// join to `writer` as CSV: the header row (the answer's attributes,
/// `Prepared::answer_header`), then one line per tuple as the join produces
/// it, so writing a result never materialises it. `workload` comes from
/// [`prepare_query`], so it holds exactly one query.
fn write_prepared_join<F: ExecutionFamily>(
    family: F, cell: Execution, workload: &PreparedWorkload, writer: impl Write,
) -> anyhow::Result<()> {
    // The joins this writes (`kermit join`, `bench join --output`) produce
    // CSV, not a report axis, and every Layout gives the same answers, so
    // this is the only check that a dispatch arm monomorphised the cell it
    // was asked for: `execution` re-derives the Layout labels from the
    // family's type parameters.
    debug_assert_eq!(
        family.execution(),
        cell,
        "dispatch built a different cell than the one resolved"
    );
    let prepared = &workload.prepared(<F::Rel as kermit_ds::Relation>::TRIE_LEVELS)[0];
    let indexes = family.build_indexes(&workload.catalog, prepared);
    let mut sink = CsvSink::new(writer, prepared.answer_header().attrs())?;
    family.execute(prepared, &indexes, |tuple| sink.write_tuple(tuple));
    sink.finish()?;
    Ok(())
}

/// Resolves the `(structure, algorithm)` pair in `args` to its execution
/// cell, carrying `choices`, which both callers obtain from
/// `DsChoices::resolve` (so every `--ds-*` flag is already validated).
/// Incompatible pairs are a usage error.
///
/// `kermit join` deliberately carries neither `--ds-config` nor `--ds-build`:
/// it resolves their defaults, because neither can change a query's answers
/// (the one Config value trades space against probe length, and every build
/// mode builds an equivalent structure: the same contents and capacities).
fn query_cell(args: &QueryArgs, choices: DsChoices) -> anyhow::Result<Execution> {
    Execution::for_pair(args.indexstructure, args.algorithm, choices).ok_or_else(|| {
        anyhow::anyhow!(
            "incompatible selection: {:?} cannot run under {:?}",
            args.indexstructure,
            args.algorithm
        )
    })
}

/// Writes `workload`'s one query, run on `cell`'s family, to `writer` as
/// CSV. `bench join --output` passes the very cell its measurements run, so
/// the CSV comes from the same kind of build.
fn write_query_csv(
    cell: Execution, workload: &PreparedWorkload, writer: impl Write,
) -> anyhow::Result<()> {
    match cell {
        | Execution::TrieLftj(SortedTrie::TreeTrie {
            seek,
            build,
        }) => with_sorted_trie_layout!(seek, |S| write_prepared_join(
            TrieLftj::<kermit_ds::TreeTrie<S>>::new(build),
            cell,
            workload,
            writer,
        )),
        | Execution::TrieLftj(SortedTrie::ColumnTrie {
            seek,
            build,
        }) => with_sorted_trie_layout!(seek, |S| write_prepared_join(
            TrieLftj::<kermit_ds::ColumnTrie<S>>::new(build),
            cell,
            workload,
            writer,
        )),
        | Execution::HashHtj {
            hasher,
            pruning,
            expansion,
            levels,
            config,
            build,
        } => with_hash_trie_layout!(hasher, pruning, expansion, levels, |H, P, E, L| {
            write_prepared_join(
                HashHtj::<H, P, E, L>::new(config, build),
                cell,
                workload,
                writer,
            )
        }),
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
    // `kermit join` has no `--ds-config` / `--ds-build` (neither can change
    // an answer), so it resolves its layout flags against their defaults,
    // through the same validator as every bench command.
    let choices = DsChoices::resolve(
        IndexStructureSelector::of(query_args.indexstructure),
        &query_args.layout,
        &ConfigChoices::default(),
        &BuildChoices::default(),
    )?;
    let cell = query_cell(&query_args, choices)?;
    let workload = prepare_query(&query_args, cell)?;
    let writer: Box<dyn Write> = match &output {
        | Some(path) => Box::new(BufWriter::new(fs::File::create(path)?)),
        | None => Box::new(BufWriter::new(io::stdout().lock())),
    };
    write_query_csv(cell, &workload, writer)
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
/// `adhoc/{query-stem}` identity. The CSV (`--output`) and the benchmark
/// share one prepared workload, so the relations are read once.
#[allow(clippy::too_many_arguments)]
fn run_bench_join(
    bench_args: &BenchArgs, query_args: QueryArgs, output: Option<PathBuf>, metrics: &[Metric],
    queries_per_build: u32, config: ConfigChoices, build: BuildChoices, materialize_max_bytes: u64,
    engines: EngineChoices,
) -> anyhow::Result<()> {
    // Resolved first, so a bad `--engine` fails before anything is read.
    let engine_cells = engines.resolve()?;
    let selector = IndexStructureSelector::of(query_args.indexstructure);
    let choices = DsChoices::resolve(selector, &query_args.layout, &config, &build)?;
    let cell = query_cell(&query_args, choices)?;
    // Engines prepare under `TrieLevels::All`; the kermit cell under its own
    // policy.
    let levels: Vec<kermit_ds::TrieLevels> = std::iter::once(cell.trie_levels())
        .chain(engine_cells.iter().map(|_| kermit_ds::TrieLevels::All))
        .collect();
    let workload = prepare_query_under(&query_args, &levels)?;

    // The kermit cell, then the engines: every cell goes through
    // `dispatch_sweep_cell`, and the clash check names their groups before
    // any is written.
    let cells: Vec<bench::run::SweepCell> = std::iter::once(bench::run::SweepCell::Kermit(cell))
        .chain(engine_cells.into_iter().map(bench::run::SweepCell::Engine))
        .collect();
    let prefix = bench_args.name.as_deref().unwrap_or(DEFAULT_JOIN_GROUP);
    if metrics.iter().any(|metric| metric.is_timed()) {
        check_join_group_directories(prefix, &workload.workload, &cells, metrics)?;
    }

    if let Some(path) = &output {
        let writer = BufWriter::new(fs::File::create(path)?);
        write_query_csv(cell, &workload, writer)?;
    }

    let criterion = CriterionSettings::from_args(bench_args);
    let settings = RunSettings {
        kind: BenchKind::Join,
        prefix,
        planner: query_args.planner,
        metrics,
        queries_per_build,
        verify: false,
        single_run: false,
        materialize_max_bytes,
        criterion: &criterion,
    };
    let mut sink = ReportSink::open(bench_args.report_json.as_deref(), BenchKind::Join)?;
    for cell in cells {
        bench::run::dispatch_sweep_cell(cell, &workload, settings, &mut sink)?;
    }
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
    // A relation has no join to materialize.
    bench::ds::refuse_materialize(&metrics)?;
    // None of `bench ds`'s metrics calls `seek`: build constructs the
    // relation, iteration and end_to_end scan it with open/next/up
    // (`TrieIteratorWrapper::advance`), and space measures it. Accepting
    // the flag would label identical measurements as a seek ablation.
    if layout.sorted_trie_seek_explicit() {
        anyhow::bail!(
            "--ds-layout-seek has no effect on bench ds: none of its metrics calls seek (build \
             constructs the relation; iteration and end-to-end scan it with open/next/up; space \
             measures it). Time a join with bench run or bench join."
        );
    }
    // `bench ds` builds one relation from a whole file, with no query, so no
    // column is a join key: `keys` would label an `all` build (#116).
    if layout.hash_trie_levels_explicit() {
        anyhow::bail!(
            "--ds-layout-levels has no effect on bench ds: it builds one relation from a whole \
             file, with no query, so no column is a join key and every level is kept. Time a \
             query with bench run or bench join."
        );
    }
    let choices = DsChoices::resolve(indexstructure, &layout, &config, &build)?;
    let group_name = bench_args.name.as_deref().unwrap_or(DEFAULT_DS_GROUP);
    let criterion = CriterionSettings::from_args(bench_args);
    let mut sink = ReportSink::open(bench_args.report_json.as_deref(), BenchKind::Ds)?;
    for ds in indexstructure.expand() {
        let report = dispatch_ds_bench(
            ds,
            choices,
            &relation,
            &metrics,
            queries_per_build,
            group_name,
            &criterion,
        )?;
        sink.push(vec![report])?;
    }
    sink.finish()?;
    Ok(())
}

/// Handler for `bench run` and `bench probe`: materialise the selected
/// benchmarks and run every valid (index-structure, algorithm) cell of the
/// requested sweep, writing each query's report as it finishes.
#[allow(clippy::too_many_arguments)]
fn run_bench_run_command(
    bench_args: &BenchArgs, selection: RunSelection, kind: BenchKind, metrics: Vec<Metric>,
    queries_per_build: u32, verify: bool, single_run: bool, materialize_max_bytes: u64,
) -> anyhow::Result<()> {
    let RunSelection {
        name,
        all,
        query,
        indexstructure,
        algorithm,
        planner,
        force,
        engines,
        layout,
        config,
        build,
    } = selection;
    // Without `-i` / `-a` (engines only) every `--ds-*` flag is checked against
    // all structures here, and `sweep_cells` then refuses one that has no
    // kermit cell to reach.
    let choices = DsChoices::resolve(
        indexstructure.unwrap_or(IndexStructureSelector::All),
        &layout,
        &config,
        &build,
    )?;
    let given = DsFlag::given(&layout, &config, &build);
    let kermit_cells = match (indexstructure, algorithm) {
        | (Some(indexstructure), Some(algorithm)) => {
            resolve_sweep(indexstructure, algorithm, choices, &given, kind)?
        },
        // clap's `requires` rules out one selector without the other.
        | _ => Vec::new(),
    };
    let cells = bench::run::sweep_cells(kermit_cells, &given, engines.resolve()?)?;
    // Each query is prepared once per trie-level policy its cells build
    // under: at most two, since each Layout flag takes one value (#116).
    let policies: Vec<kermit_ds::TrieLevels> =
        cells.iter().map(|cell| cell.trie_levels()).collect();
    let benchmarks = resolve_benchmarks(&name, all)?;
    let cache_root = kermit_bench::cache::base_cache_dir()
        .map_err(|e| anyhow::anyhow!("no cache directory available: {e}"))?;
    let materialized: Vec<BenchmarkDefinition> = benchmarks
        .into_iter()
        .map(|b| materialize::materialize(b, &cache_root, force))
        .collect::<Result<Vec<_>, _>>()?;

    let prefix = bench_args.name.as_deref().unwrap_or(DEFAULT_RUN_GROUP);
    let prepare_with = planner.instantiate();
    let criterion = CriterionSettings::from_args(bench_args);
    let settings = RunSettings {
        kind,
        prefix,
        planner,
        metrics: &metrics,
        queries_per_build,
        verify,
        single_run,
        materialize_max_bytes,
        criterion: &criterion,
    };
    // Names only — nothing is fetched — so a sweep whose cells would
    // overwrite each other's Criterion results fails before any is timed.
    // A run with no time metric (`-m space`) and a probe (its single runs go
    // through no Criterion function, `materialize` included) write no
    // Criterion group, so nothing can collide.
    if !single_run && metrics.iter().any(|metric| metric.is_timed()) {
        check_sweep_group_directories(prefix, &materialized, query.as_deref(), &cells, &metrics)?;
    }

    // Opened before the loop so every finished query is on disk before the
    // next one starts; a crash mid-sweep keeps the completed queries.
    let mut sink = ReportSink::open(bench_args.report_json.as_deref(), kind)?;
    for benchmark in &materialized {
        // Validated, read and planned once per benchmark, before its loop
        // over cells: every cell builds from this catalog and runs these
        // prepared queries.
        let workload = Workload::from_definition(benchmark, &workspace_root(), query.as_deref())
            .and_then(|workload| workload.prepare(&prepare_with, &policies))
            .with_context(|| {
                format!(
                    "bench {kind} failed on benchmark '{}'; partial report retained at {}",
                    benchmark.name,
                    sink.path().display()
                )
            })?;
        for &cell in &cells {
            bench::run::dispatch_sweep_cell(cell, &workload, settings, &mut sink).with_context(
                || {
                    format!(
                        "bench {kind} failed on benchmark '{}' cell {}; partial report retained \
                         at {}",
                        benchmark.name,
                        cell.group_segment(),
                        sink.path().display()
                    )
                },
            )?;
        }
    }
    sink.finish()?;
    Ok(())
}

/// Handler for `bench probe`: `bench run`'s runner with the probe's fixed
/// settings. Every query is counted and, where it declares `expected`,
/// verified; its describe step's build and execution times are recorded as
/// single-run values beside its exact `space`; nothing goes through
/// Criterion. With `materialize`, the query is also materialized once and
/// that time recorded as a single-run `materialize` value.
fn run_bench_probe_command(
    bench_args: &BenchArgs, selection: RunSelection, materialize: bool, materialize_max_bytes: u64,
) -> anyhow::Result<()> {
    let mut metrics = vec![Metric::Space];
    if materialize {
        metrics.push(Metric::Materialize);
    }
    run_bench_run_command(
        bench_args,
        selection,
        BenchKind::Probe,
        metrics,
        1,
        true,
        true,
        materialize_max_bytes,
    )
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
                engines,
                output,
                metrics,
                queries_per_build,
                config,
                build,
                materialize,
            } => run_bench_join(
                &bench_args,
                query_args,
                output,
                &metrics,
                queries_per_build,
                config,
                build,
                materialize.materialize_max_bytes,
                engines,
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
                selection,
                metrics,
                queries_per_build,
                verify,
                materialize,
            } => run_bench_run_command(
                &bench_args,
                selection,
                BenchKind::Run,
                metrics,
                queries_per_build,
                verify,
                false,
                materialize.materialize_max_bytes,
            )?,

            | BenchSubcommand::Probe {
                selection,
                materialize,
                limits,
            } => run_bench_probe_command(
                &bench_args,
                selection,
                materialize,
                limits.materialize_max_bytes,
            )?,

            | BenchSubcommand::Gen {
                subcommand,
            } => bench::gen::run(subcommand)?,
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
