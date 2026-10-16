//! `bench gen`: generate a benchmark on the fly into the cache, where `bench
//! list` and `bench run` discover it.
//!
//! [`GenSubcommand`] is the clap subtree and [`run`] hands it to one handler
//! per kind. `watdiv` and `lubm` drive `kermit-rdf`'s pipelines and take their
//! own output flags; `snap`, `h2o` and `job` are `kermit_generate::Generator`s
//! and share [`GenTarget`]'s. No handler records a `spec_hash`, which is what
//! marks a generation as imperative rather than a workspace YAML's.

use {
    crate::materialize::{vendored_lubm_jar, vendored_watdiv_root, workspace_root},
    clap::Subcommand,
    kermit_bench::BenchmarkDefinition,
    std::path::PathBuf,
};

/// Output flags shared by every non-RDF `bench gen <kind>` (D15).
#[derive(clap::Args)]
pub(crate) struct GenTarget {
    /// Tag appended to the benchmark name; pick a value that won't
    /// collide with committed snapshot names
    #[arg(long, value_name = "STRING", required = true)]
    tag: String,

    /// Override the cache dir parent (default: ~/.cache/kermit/benchmarks).
    /// NOTE: benchmarks generated outside the default cache are NOT
    /// auto-discovered by `bench list/fetch/run`; mainly useful for tests.
    #[arg(long, value_name = "PATH")]
    output_dir: Option<PathBuf>,
}

impl GenTarget {
    /// Resolves and creates `bench_name`'s output directory, as `bench gen
    /// lubm` does: refuses a name a workspace YAML already uses, and notes
    /// an undiscovered `--output-dir` and a non-empty directory on stderr.
    fn prepare(&self, kind: &str, bench_name: &str) -> anyhow::Result<PathBuf> {
        let workspace_names = kermit_bench::discovery::list_benchmarks(&workspace_root())
            .map_err(|e| anyhow::anyhow!("failed to enumerate workspace benchmarks: {e}"))?;
        if workspace_names.iter().any(|n| n == bench_name) {
            anyhow::bail!(
                "--tag {:?} produces bench name {bench_name:?} which already exists in the \
                 workspace; pick a different tag",
                self.tag
            );
        }
        let out = kermit_generate::output_dir(bench_name, self.output_dir.as_deref())?;
        if !out.discovered {
            eprintln!(
                "[gen {kind}] note: --output-dir is set; {} will NOT be auto-discovered by `bench \
                 list/fetch/run`",
                out.path.display()
            );
        }
        if std::fs::read_dir(&out.path)
            .map(|mut d| d.next().is_some())
            .unwrap_or(false)
        {
            eprintln!(
                "[gen {kind}] note: {} is non-empty; existing files will be overwritten",
                out.path.display()
            );
        }
        std::fs::create_dir_all(&out.path)?;
        Ok(out.path)
    }
}

/// The CLI spelling of `kermit_bench::SnapGraph`. kermit-bench has no clap
/// dependency, so the binary declares its own `ValueEnum` and maps it across;
/// `snap_clap_names_match_the_spec_names` pins that the names agree.
// The `Com` prefix is part of the SNAP graph names (`com-amazon`), which clap
// derives from the variant names and which must equal the spec's.
#[allow(clippy::enum_variant_names)]
#[derive(Clone, Copy, clap::ValueEnum)]
pub(crate) enum SnapGraphArg {
    /// The Amazon product co-purchasing communities (`com-amazon`)
    ComAmazon,
    /// The DBLP collaboration communities (`com-dblp`)
    ComDblp,
    /// The YouTube social communities (`com-youtube`)
    ComYoutube,
}

impl From<SnapGraphArg> for kermit_bench::SnapGraph {
    fn from(arg: SnapGraphArg) -> Self {
        match arg {
            | SnapGraphArg::ComAmazon => Self::ComAmazon,
            | SnapGraphArg::ComDblp => Self::ComDblp,
            | SnapGraphArg::ComYoutube => Self::ComYoutube,
        }
    }
}

/// The CLI spelling of `kermit_bench::SnapQuery`; see [`SnapGraphArg`].
#[derive(Clone, Copy, clap::ValueEnum)]
pub(crate) enum SnapQueryArg {
    /// `edge(A, B), edge(B, C), edge(A, C)`: 6 x the triangles
    Triangle,
    /// The 3-step walks (vertices may repeat)
    Path3,
    /// The closed 4-step walks, tr(A^4)
    Cycle4,
    /// 24 x the 4-cliques
    Clique4,
}

impl From<SnapQueryArg> for kermit_bench::SnapQuery {
    fn from(arg: SnapQueryArg) -> Self {
        match arg {
            | SnapQueryArg::Triangle => Self::Triangle,
            | SnapQueryArg::Path3 => Self::Path3,
            | SnapQueryArg::Cycle4 => Self::Cycle4,
            | SnapQueryArg::Clique4 => Self::Clique4,
        }
    }
}

/// The CLI spelling of `kermit_bench::H2oOrder`; see [`SnapGraphArg`].
/// `h2o_clap_names_match_the_spec_names` pins that the names agree.
#[derive(Clone, Copy, clap::ValueEnum)]
pub(crate) enum H2oOrderArg {
    /// Each relation's rows in the order they are drawn (upstream's)
    Shuffled,
    /// Each relation's rows sorted, the order the sorted tries build in
    Sorted,
}

impl From<H2oOrderArg> for kermit_bench::H2oOrder {
    fn from(arg: H2oOrderArg) -> Self {
        match arg {
            | H2oOrderArg::Shuffled => Self::Shuffled,
            | H2oOrderArg::Sorted => Self::Sorted,
        }
    }
}

#[derive(Subcommand)]
pub(crate) enum GenSubcommand {
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

    /// Generate a SNAP community-graph benchmark (#96)
    Snap {
        /// A named graph with a pinned download
        #[arg(
            long,
            value_enum,
            required_unless_present = "url",
            conflicts_with_all = ["url", "sha256"]
        )]
        graph: Option<SnapGraphArg>,

        /// A SNAP edge-list `.gz` URL; requires --sha256
        #[arg(long, value_name = "URL", requires = "sha256")]
        url: Option<String>,

        /// SHA-256 of the `.gz` at --url (64 lowercase hex)
        #[arg(long, value_name = "HEX", requires = "url")]
        sha256: Option<String>,

        /// Queries to emit, comma-separated (default: all four)
        #[arg(long, value_enum, value_delimiter = ',')]
        queries: Vec<SnapQueryArg>,

        #[command(flatten)]
        target: GenTarget,
    },

    /// Generate an H2O db-benchmark join benchmark (#95)
    H2o {
        /// N, the rows of the fact table `x` (at least 10000000, upstream's
        /// floor)
        #[arg(
            long,
            value_name = "N",
            value_parser = clap::value_parser!(u32).range(i64::from(kermit_bench::H2O_MIN_SCALE)..)
        )]
        scale: u32,

        /// Random seed (default: upstream's 108)
        #[arg(long, value_name = "SEED", default_value_t = kermit_bench::DEFAULT_H2O_SEED)]
        seed: u32,

        /// Row order of every relation
        #[arg(long, value_enum, default_value_t = H2oOrderArg::Shuffled)]
        order: H2oOrderArg,

        #[command(flatten)]
        target: GenTarget,
    },

    /// Generate one Join Order Benchmark template on the fly (needs DuckDB)
    Job {
        /// The JOB template to generate, 1-33: every variant of it (`6` is
        /// `6a`-`6f`) becomes one query of the benchmark
        #[arg(long, value_name = "N", required = true)]
        template: u8,

        #[command(flatten)]
        target: GenTarget,
    },
}

/// Runs a parsed `bench gen <kind>` through its handler.
pub(crate) fn run(subcommand: GenSubcommand) -> anyhow::Result<()> {
    match subcommand {
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
        ),

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
        ),

        | GenSubcommand::Snap {
            graph,
            url,
            sha256,
            queries,
            target,
        } => run_gen_snap(graph, url, sha256, queries, target),

        | GenSubcommand::H2o {
            scale,
            seed,
            order,
            target,
        } => run_gen_h2o(scale, seed, order, target),

        | GenSubcommand::Job {
            template,
            target,
        } => run_gen_job(template, target),
    }
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

/// The name of a `bench gen snap` benchmark: `snap-<name>-<tag>`, where
/// `<name>` is the graph's name or the URL's file name up to its first `.`
/// (`.../com-orkut.ungraph.txt.gz` gives `com-orkut`).
///
/// A URL with no such name (one ending in `/`, or in `/.gz`) is an error that
/// names it. Whether the result is a portable filename is not checked here:
/// `BenchmarkDefinition::validate` does that for the finished name.
fn snap_bench_name(
    graph: Option<kermit_bench::SnapGraph>, url: Option<&str>, tag: &str,
) -> anyhow::Result<String> {
    let name = match (graph, url) {
        | (Some(g), _) => g.name(),
        | (None, Some(u)) => {
            let Some(stem) = kermit_generate::snap::url_stem(u) else {
                anyhow::bail!("--url {u:?} has no file name to name the benchmark by");
            };
            stem
        },
        | (None, None) => anyhow::bail!("pass --graph, or --url together with --sha256"),
    };
    Ok(format!("snap-{name}-{tag}"))
}

/// Handler for `bench gen snap`: download (or take) a SNAP edge list and write
/// a benchmark from it through [`kermit_generate::snap::SnapGenerator`].
///
/// The benchmark is named by [`snap_bench_name`]. The name, the digest and
/// the URL are checked once, by validating the equivalent declarative spec,
/// before anything is downloaded. The run has no `spec_hash`, like every
/// imperative `bench gen`.
fn run_gen_snap(
    graph: Option<SnapGraphArg>, url: Option<String>, sha256: Option<String>,
    queries: Vec<SnapQueryArg>, target: GenTarget,
) -> anyhow::Result<()> {
    use {
        kermit_bench::{GeneratorSpec, SnapGraph, SnapQuery},
        kermit_generate::snap::{SnapGenerator, SnapSource},
    };

    let graph = graph.map(SnapGraph::from);
    let queries: Vec<SnapQuery> = queries.into_iter().map(SnapQuery::from).collect();
    let queries = (!queries.is_empty()).then_some(queries);
    let spec = GeneratorSpec::Snap {
        graph,
        url: url.clone(),
        sha256: sha256.clone(),
        queries: queries.clone(),
    };

    let bench_name = snap_bench_name(graph, url.as_deref(), &target.tag)?;

    BenchmarkDefinition {
        name: bench_name.clone(),
        description: String::new(),
        relations: vec![],
        queries: vec![],
        generator: Some(spec),
    }
    .validate()?;

    let source = match (graph, url, sha256) {
        | (Some(g), None, None) => SnapSource::named(g),
        | (None, Some(url), Some(sha256)) => SnapSource::Remote {
            url,
            sha256,
        },
        | _ => anyhow::bail!("pass --graph, or --url together with --sha256"),
    };
    let out = target.prepare("snap", &bench_name)?;
    let generator = SnapGenerator::new(
        kermit_rdf::generator::Target {
            out_dir: &out,
            bench_name: &bench_name,
            tag: &target.tag,
            spec_hash: None,
        },
        graph,
        source,
        queries.as_deref(),
    );
    kermit_generate::run(&generator)?;
    eprintln!("snap benchmark written to {}", out.display());
    Ok(())
}

/// The name of a `bench gen h2o` benchmark: `h2o-<scale>-<tag>`, as `bench
/// gen lubm` names `lubm-<scale>-<tag>`. The seed and the order go to
/// `meta.json`, not the name.
fn h2o_bench_name(scale: u32, tag: &str) -> String { format!("h2o-{scale}-{tag}") }

/// Handler for `bench gen h2o`: generate the H2O db-benchmark join relations
/// and queries through [`kermit_generate::h2o::H2oGenerator`].
///
/// The benchmark is named by [`h2o_bench_name`]. The equivalent declarative
/// spec is validated first (the floor and the name), before anything is
/// written. The run has no `spec_hash`, like every imperative `bench gen`.
fn run_gen_h2o(scale: u32, seed: u32, order: H2oOrderArg, target: GenTarget) -> anyhow::Result<()> {
    use {
        kermit_bench::{GeneratorSpec, H2oOrder},
        kermit_generate::h2o::{H2oGenerator, KeyPlan},
    };

    let order = H2oOrder::from(order);
    let bench_name = h2o_bench_name(scale, &target.tag);

    BenchmarkDefinition {
        name: bench_name.clone(),
        description: String::new(),
        relations: vec![],
        queries: vec![],
        generator: Some(GeneratorSpec::H2o {
            scale,
            seed,
            order,
        }),
    }
    .validate()?;

    let plan = KeyPlan::for_scale(scale)?;
    let out = target.prepare("h2o", &bench_name)?;
    let generator = H2oGenerator::new(
        kermit_rdf::generator::Target {
            out_dir: &out,
            bench_name: &bench_name,
            tag: &target.tag,
            spec_hash: None,
        },
        plan,
        seed,
        order,
    );
    kermit_generate::run(&generator)?;
    eprintln!("h2o benchmark written to {}", out.display());
    Ok(())
}

/// Handler for `bench gen job`: generate one JOB template into
/// `job-{template}-{tag}` through `kermit-generate`, with no spec hash (like
/// `bench gen lubm`). The range and the benchmark's name are checked first and
/// DuckDB is located before the output directory is made, so a bad template,
/// a bad tag or a host without DuckDB leaves nothing behind. A raw cache that
/// is then refused or fails to build still leaves the empty `job-N-<tag>`
/// output directory `GenTarget::prepare` made, as SNAP's download inside
/// `generate` does; discovery ignores a directory without `meta.json`.
fn run_gen_job(template: u8, target: GenTarget) -> anyhow::Result<()> {
    use kermit_generate::job::{duckdb::Duckdb, raw, JobGenerator, TEMPLATES};

    anyhow::ensure!(
        TEMPLATES.contains(&template),
        "--template {template} is not a JOB template: the benchmark has templates {}..={}",
        TEMPLATES.start(),
        TEMPLATES.end()
    );
    let bench_name = format!("job-{template}-{}", target.tag);
    // The name is checked here, as `run_gen_snap` does, because `run` would
    // check it only after the raw cache and every filter had been built.
    BenchmarkDefinition {
        name: bench_name.clone(),
        description: String::new(),
        relations: vec![],
        queries: vec![],
        generator: Some(kermit_bench::GeneratorSpec::Job {
            template,
        }),
    }
    .validate()?;
    let duckdb = Duckdb::locate()?;
    let out_dir = target.prepare("job", &bench_name)?;
    let cache = raw::ensure(&duckdb, &raw::default_raw_dir()?)?;
    let generator = JobGenerator {
        template,
        raw: &cache,
        duckdb: &duckdb,
        target: kermit_rdf::generator::Target {
            out_dir: &out_dir,
            bench_name: &bench_name,
            tag: &target.tag,
            spec_hash: None,
        },
    };
    let meta = kermit_generate::run(&generator)?;
    eprintln!(
        "[gen job] wrote {} (relations={}, queries={})",
        out_dir.display(),
        meta.relation_count,
        meta.query_count
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::{BenchSubcommand, Cli, Commands},
        clap::Parser,
    };

    /// The names clap accepts for `--graph` and `--queries` are the names the
    /// YAML spec and the generated queries use: kermit-bench has no clap
    /// dependency, so the binary spells them twice and this pins that the two
    /// agree, variant for variant, in order.
    #[test]
    fn snap_clap_names_match_the_spec_names() {
        use clap::ValueEnum;

        let mut graphs = Vec::new();
        for arg in SnapGraphArg::value_variants() {
            let clap_name = arg.to_possible_value().unwrap().get_name().to_string();
            assert_eq!(clap_name, kermit_bench::SnapGraph::from(*arg).name());
            graphs.push(clap_name);
        }
        let spec_graphs: Vec<&str> = kermit_bench::SnapGraph::ALL
            .iter()
            .map(|g| g.name())
            .collect();
        assert_eq!(graphs, spec_graphs);

        let mut queries = Vec::new();
        for arg in SnapQueryArg::value_variants() {
            let clap_name = arg.to_possible_value().unwrap().get_name().to_string();
            assert_eq!(clap_name, kermit_bench::SnapQuery::from(*arg).name());
            queries.push(clap_name);
        }
        let spec_queries: Vec<&str> = kermit_bench::SnapQuery::ALL
            .iter()
            .map(|q| q.name())
            .collect();
        assert_eq!(queries, spec_queries);
    }

    /// A graph names the benchmark by its name, a URL by its file name up to
    /// the first `.`, and a URL with no file name is refused naming the URL.
    #[test]
    fn snap_bench_name_follows_the_graph_or_the_url_stem() {
        use kermit_bench::SnapGraph;

        assert_eq!(
            snap_bench_name(Some(SnapGraph::ComAmazon), None, "dev").unwrap(),
            "snap-com-amazon-dev"
        );
        assert_eq!(
            snap_bench_name(
                None,
                Some("https://h.invalid/d/com-orkut.ungraph.txt.gz"),
                "dev"
            )
            .unwrap(),
            "snap-com-orkut-dev"
        );
        for url in ["https://h.invalid/d/", "https://h.invalid/d/.gz"] {
            let err = snap_bench_name(None, Some(url), "dev")
                .unwrap_err()
                .to_string();
            assert!(err.contains(url), "{err}");
        }
    }

    /// `--graph` xor (`--url` with `--sha256`) is required, the pairs require
    /// each other, `--queries` splits on commas, and a name outside the enums
    /// is refused, all at parse time, before the handler runs.
    #[test]
    fn snap_flags_conflict_and_require_as_documented() {
        let parse = |extra: &[&str]| {
            let mut argv = vec!["kermit", "bench", "gen", "snap", "--tag", "t"];
            argv.extend_from_slice(extra);
            Cli::try_parse_from(argv)
        };
        let digest = "0".repeat(64);
        let d = digest.as_str();
        let url = "https://example.invalid/g.ungraph.txt.gz";

        assert!(parse(&["--graph", "com-amazon"]).is_ok());
        assert!(parse(&["--url", url, "--sha256", d]).is_ok());

        let rejected: &[&[&str]] = &[
            // neither a graph nor a URL
            &[],
            // a URL without its digest, and a digest without its URL
            &["--url", url],
            &["--sha256", d],
            // a graph with a URL, and a graph with a digest
            &["--graph", "com-amazon", "--url", url, "--sha256", d],
            &["--graph", "com-amazon", "--sha256", d],
            // names outside the enums
            &["--graph", "com-orkut"],
            &["--graph", "com-amazon", "--queries", "triangle,pentagon"],
        ];
        for args in rejected {
            assert!(parse(args).is_err(), "{args:?} should be rejected");
        }

        let cli = parse(&["--graph", "com-dblp", "--queries", "triangle,clique4"])
            .expect("a graph with two queries parses");
        let Commands::Bench {
            subcommand:
                BenchSubcommand::Gen {
                    subcommand:
                        GenSubcommand::Snap {
                            queries, ..
                        },
                },
            ..
        } = cli.command
        else {
            panic!("not a `bench gen snap` command");
        };
        let names: Vec<&str> = queries
            .iter()
            .map(|q| kermit_bench::SnapQuery::from(*q).name())
            .collect();
        assert_eq!(names, ["triangle", "clique4"]);
    }

    /// The names clap accepts for `--order` are the names the YAML spec
    /// uses: kermit-bench has no clap dependency, so the binary spells them
    /// twice and this pins that the two agree, variant for variant, in
    /// order.
    #[test]
    fn h2o_clap_names_match_the_spec_names() {
        use clap::ValueEnum;

        let mut orders = Vec::new();
        for arg in H2oOrderArg::value_variants() {
            let clap_name = arg.to_possible_value().unwrap().get_name().to_string();
            assert_eq!(clap_name, kermit_bench::H2oOrder::from(*arg).name());
            orders.push(clap_name);
        }
        let spec_orders: Vec<&str> = kermit_bench::H2oOrder::ALL
            .iter()
            .map(|o| o.name())
            .collect();
        assert_eq!(orders, spec_orders);
    }

    /// `--scale` is required and floored at `H2O_MIN_SCALE`, `--seed` and
    /// `--order` default to upstream's 108 and `shuffled`, and a value
    /// outside any of them is refused at parse time, before the handler
    /// runs.
    #[test]
    fn h2o_flags_default_and_enforce_the_floor() {
        use kermit_bench::H2oOrder;

        let parse = |extra: &[&str]| {
            let mut argv = vec!["kermit", "bench", "gen", "h2o", "--tag", "t"];
            argv.extend_from_slice(extra);
            Cli::try_parse_from(argv)
        };
        let fields = |cli: Cli| {
            let Commands::Bench {
                subcommand:
                    BenchSubcommand::Gen {
                        subcommand:
                            GenSubcommand::H2o {
                                scale,
                                seed,
                                order,
                                ..
                            },
                    },
                ..
            } = cli.command
            else {
                panic!("not a `bench gen h2o` command");
            };
            (scale, seed, H2oOrder::from(order))
        };

        let cli = parse(&["--scale", "10000000"]).expect("the floor itself parses");
        assert_eq!(fields(cli), (10_000_000, 108, H2oOrder::Shuffled));
        let cli = parse(&["--scale", "12000000", "--order", "sorted", "--seed", "7"])
            .expect("a seed and an order parse");
        assert_eq!(fields(cli), (12_000_000, 7, H2oOrder::Sorted));
        let cli = parse(&["--scale", "4294967295"]).expect("the largest u32 parses");
        assert_eq!(fields(cli).0, u32::MAX);

        // One below the floor is refused, and the error names the flag and
        // the floor.
        let Err(below) = parse(&["--scale", "9999999"]) else {
            panic!("--scale 9999999 was accepted");
        };
        let message = below.to_string();
        assert!(
            message.contains("--scale") && message.contains("10000000"),
            "{message}"
        );

        let rejected: &[&[&str]] = &[
            // zero, and no scale at all
            &["--scale", "0"],
            &[],
            // an order outside the enum, or spelled in another case
            &["--scale", "10000000", "--order", "random"],
            &["--scale", "10000000", "--order", "Sorted"],
        ];
        for args in rejected {
            assert!(parse(args).is_err(), "{args:?} should be rejected");
        }
    }

    /// `h2o-<scale>-<tag>`, as `bench gen lubm` names `lubm-<scale>-<tag>`.
    #[test]
    fn h2o_bench_name_is_scale_and_tag() {
        assert_eq!(h2o_bench_name(10_000_000, "dev"), "h2o-10000000-dev");
        assert_eq!(h2o_bench_name(12_000_000, "a-b"), "h2o-12000000-a-b");
    }
}
