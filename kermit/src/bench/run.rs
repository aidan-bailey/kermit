//! The generic runner behind `bench run`, `bench join` and `bench probe`:
//! one execution cell over one [`PreparedWorkload`], every requested metric,
//! one report per query, pushed as each query finishes.

use {
    super::{
        cell::{execution_segment, Cell, FamilyCell},
        criterion_directory_name,
        measure::{CriterionSettings, Described, MeasureRequest, Subject},
        workload::PreparedWorkload,
        Metric, CRITERION_MAX_DIRECTORY_NAME_BYTES,
    },
    crate::{
        bench_report::{write_metadata_block, BenchKind, BenchReport, MetadataLine, ReportSink},
        execution::{Execution, HashHtj, SortedTrie, Sweep, TrieLftj},
        options::{
            unreached_flag, with_hash_trie_layout, with_sorted_trie_layout, DsChoices, DsFlag,
            PlannerArgs,
        },
        IndexStructureSelector, JoinAlgorithmSelector,
    },
    kermit::db::{Catalog, Prepared},
    kermit_algos::{IndexSpec, INDEX_ID_PREFIX},
    kermit_bench::BenchmarkDefinition,
    std::{
        collections::{hash_map::Entry, BTreeMap, HashMap},
        io,
    },
};

/// Everything a measured join needs besides the cell and the workload:
/// what kind of report to stamp, how to name the Criterion group, which
/// optimiser the report names, and what to measure. One value is built
/// per subcommand invocation and shared by every cell it runs.
#[derive(Clone, Copy)]
pub(crate) struct RunSettings<'a> {
    /// The report's `kind` field (`Run` for `bench run`, `Join` for
    /// `bench join`); the group naming is the same for both.
    pub kind: BenchKind,
    /// First segment of the Criterion group
    /// (`{prefix}/{workload}/{query}/{ds}/{algo}`), from `--name` or the
    /// subcommand's default.
    pub prefix: &'a str,
    /// `--optimiser`, for the report's `optimiser` axis. The queries were
    /// planned when the workload was prepared, so nothing here plans.
    pub planner: PlannerArgs,
    pub metrics: &'a [Metric],
    /// K in the `end_to_end` metric's `T = build + K × query`.
    pub queries_per_build: u32,
    /// Compare each query's answer count with the workload's `expected`
    /// before timing it; a mismatch aborts.
    pub verify: bool,
    /// Count every query and record the describe step's build and
    /// execution times as single-run values (`bench probe`).
    pub single_run: bool,
    /// `--materialize-max-bytes`: the `materialize` metric is skipped, with a
    /// note, for a query whose full assignments would take more bytes than
    /// this (see [`materialize_bytes`]).
    pub materialize_max_bytes: u64,
    /// Criterion sample/measurement/warm-up settings.
    pub criterion: &'a CriterionSettings,
}

/// The value of one `index` metadata line, for the `position`th index of a
/// query: `{id} {relation} filter {filter} columns {columns:?}[ levels {n}]:
/// {rows} rows, {bytes} bytes`. ` levels {n}` appears only when the trie has
/// levels for fewer columns than it keeps (`--ds-layout-levels keys`, #116),
/// so every `all` line reads as before. `rows` is what the build copied, the
/// index view's count, and `bytes` the index's footprint as built.
fn describe_index(position: usize, spec: &IndexSpec, rows: usize, bytes: usize) -> String {
    let levels = if spec.levels < spec.columns.len() {
        format!(" levels {}", spec.levels)
    } else {
        String::new()
    };
    format!(
        "{INDEX_ID_PREFIX}{position} {} filter {} columns {:?}{levels}: {rows} rows, {bytes} bytes",
        spec.relation, spec.filter, spec.columns
    )
}

/// The rows the relations `prepared` reads hold in `catalog`, each relation
/// counted once however many atoms read it: the report's `tuples` axis, the
/// same for every structure.
fn tuples_read(catalog: &Catalog, prepared: &Prepared) -> usize {
    prepared
        .relations()
        .into_iter()
        .map(|name| {
            catalog
                .tuples(name)
                .expect("prepare checked that every relation a query reads is in the catalog")
                .len()
        })
        .sum()
}

/// The bytes `materialize` keeps for `rows` assignments of `width` variables:
/// `rows × width × 8`, saturating. The memory guard compares it with
/// `--materialize-max-bytes`.
pub(crate) fn materialize_bytes(rows: u64, width: usize) -> u64 {
    let width = u64::try_from(width).unwrap_or(u64::MAX);
    let value = u64::try_from(std::mem::size_of::<usize>()).unwrap_or(u64::MAX);
    rows.saturating_mul(width).saturating_mul(value)
}

/// What the guard decides: `Some(bytes)`, the result's size, when it exceeds
/// `limit` and `materialize` must be skipped; `None` when it fits.
fn materialize_skip(rows: u64, width: usize, limit: u64) -> Option<u64> {
    let bytes = materialize_bytes(rows, width);
    (bytes > limit).then_some(bytes)
}

/// What the memory guard decides for one query's `materialize`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MaterializeVerdict {
    /// `materialize` is not asked for.
    NotAsked,
    /// The result fits the limit.
    Run,
    /// A streaming subject's result is over the limit, by this many bytes.
    /// The describe step may still count: its streamed count is cheap.
    Skip(u64),
    /// A subject that does not stream is over the limit, by this many bytes.
    /// Its count is a `materialize`, so nothing materialises: no describe-step
    /// count, no preflight count.
    SkipUncounted(u64),
    /// A streaming subject with no `expected`: ask again with the describe
    /// step's streamed count.
    AfterStreamedCount,
    /// A subject that does not stream, with no `expected`: the guard has no
    /// row count to read, so it warns and everything runs unguarded.
    Unguarded,
}

impl MaterializeVerdict {
    /// Whether the describe step and a cell's preflight may count: false only
    /// when a subject that does not stream is skipped (its count is the
    /// `materialize` the guard refused).
    pub(crate) fn counts(self) -> bool { !matches!(self, Self::SkipUncounted(_)) }

    /// The result's size in bytes, when the guard skips `materialize`.
    pub(crate) fn skipped(self) -> Option<u64> {
        match self {
            | Self::Skip(bytes) | Self::SkipUncounted(bytes) => Some(bytes),
            | _ => None,
        }
    }
}

/// The memory guard, in one place for every subject. `requested` is whether
/// anything would materialise, as [`guard_requested`] computes it; `streams` is
/// the subject type's [`Subject::STREAMS`]; `rows` is the full-assignment count
/// the guard reads: the query's `expected`, or, on a second call for a
/// streaming subject, the describe step's streamed count; `width` is
/// `Prepared::assignment_width`.
///
/// A streaming subject with no `rows` answers
/// [`AfterStreamedCount`](MaterializeVerdict::AfterStreamedCount). A subject
/// that does not stream is decided before its describe step, from `expected`
/// alone, so over the limit it answers
/// [`SkipUncounted`](MaterializeVerdict::SkipUncounted) and nothing
/// materialises; with no `expected` it answers
/// [`Unguarded`](MaterializeVerdict::Unguarded).
pub(crate) fn materialize_guard(
    requested: bool, streams: bool, rows: Option<u64>, width: usize, limit: u64,
) -> MaterializeVerdict {
    use MaterializeVerdict::*;
    if !requested {
        return NotAsked;
    }
    match (rows, streams) {
        | (Some(rows), _) => match materialize_skip(rows, width, limit) {
            | None => Run,
            | Some(bytes) if streams => Skip(bytes),
            | Some(bytes) => SkipUncounted(bytes),
        },
        | (None, true) => AfterStreamedCount,
        | (None, false) => Unguarded,
    }
}

/// The guard's `requested`. For a streaming subject it is whether the
/// `materialize` metric is asked for (`metric`): the describe step streams its
/// count, so only the metric materialises. A subject that does not stream
/// counts through `materialize`, so it is also whether `--verify` has an
/// `expected` to check (`verifies`) or the run is a probe's single run
/// (`single_run`): a `--verify`-only run over the limit then materialises
/// nothing either.
fn guard_requested(streams: bool, metric: bool, verifies: bool, single_run: bool) -> bool {
    metric || (!streams && (verifies || single_run))
}

/// Runs every query of `workload` on one execution cell and pushes one
/// report per query to `sink` as soon as that query finishes, each stamped
/// with `settings.kind` and grouped under `settings.prefix`.
///
/// Generic over the [`Cell`] so every kind of cell shares one body: kermit's
/// sorted family (`TrieLftj<R>`) and hash family (`HashHtj<H, P, E>`), each
/// wrapped in a [`FamilyCell`], and any other cell. Metadata, Criterion group
/// wiring and report assembly are identical; what differs is asked of the
/// cell. For a `FamilyCell` the report's `data_structure` / `algorithm` axes
/// come from `family.execution()` — the same value that picked the code path
/// — so a report can never name an algorithm it did not run (issue #56).
///
/// The workload arrives prepared (`Workload::prepare`): every query is
/// validated and planned, and the relations the queries name sit in the
/// catalog as flat tuples, so a query that cannot run never reaches this
/// function (issue #78). Per query, the cell measures its subject
/// through the measure layer. One describe build outside Criterion gives the
/// `index` lines, the exact `space` and the count `--verify` checks, which
/// `bench probe` also times; then
/// [`Described::measure`] runs `iteration` on that build, drops it, and times
/// `build` and `end_to_end`. With `settings.verify`, a query with an
/// `expected` count is checked before any timer starts, and a mismatch aborts
/// with every earlier query's report already in `sink`.
///
/// `materialize`, when asked for, is guarded first (see
/// [`materialize_guard`]): its result is every full assignment, so it is
/// skipped, with a stderr note and a `materialize` metadata line, when
/// `expected` (for a streaming subject, else the describe step's streamed
/// count) times the variable count times 8 bytes exceeds
/// `settings.materialize_max_bytes`. Otherwise `--verify` also checks its
/// distinct count against `expected`, once.
fn run_benchmark<C: Cell>(
    cell: &C, workload: &PreparedWorkload, settings: RunSettings<'_>, sink: &mut ReportSink,
) -> anyhow::Result<()> {
    let RunSettings {
        kind,
        prefix,
        planner,
        metrics,
        queries_per_build,
        verify,
        single_run,
        materialize_max_bytes,
        criterion,
    } = settings;
    let PreparedWorkload {
        workload: definition,
        catalog,
        ..
    } = workload;
    // The queries prepared under this cell's trie levels (#116).
    let prepared_queries = workload.prepared(cell.trie_levels());

    // The cell's label names it in the messages below; its axes (`system`,
    // and for a kermit cell `data_structure`, `algorithm` and the `ds_*`
    // dimensions) are the base of every query's report axes, and its group
    // segment completes the Criterion group (the on-disk
    // `target/criterion/{group}` names embed it).
    let label = cell.label();
    let cell_axes = cell.axes();
    let group_segment = cell.group_segment();

    // A metric the cell cannot measure is not requested of it.
    let supported: Vec<Metric> = metrics
        .iter()
        .copied()
        .filter(|&metric| cell.supports(metric))
        .collect();
    let metrics = supported.as_slice();

    for (query_def, prepared) in definition.queries.iter().zip(prepared_queries) {
        // The memory guard of `materialize`, first from `expected` alone; a
        // streaming subject with none is decided again after its describe
        // step has counted.
        let streams = <C::Subject<'_> as Subject>::STREAMS;
        let materialize_requested = metrics.contains(&Metric::Materialize);
        let width = prepared.assignment_width();
        let mut guard = materialize_guard(
            guard_requested(
                streams,
                materialize_requested,
                verify && query_def.expected.is_some(),
                single_run,
            ),
            streams,
            query_def.expected,
            width,
            materialize_max_bytes,
        );
        if guard == MaterializeVerdict::Unguarded {
            eprintln!(
                "warning: {label} does not stream and {}/{} has no expected count, so materialize \
                 runs without the --materialize-max-bytes guard",
                definition.name, query_def.name
            );
        }

        // The cell's preflight counts unless the materialize guard (bound at
        // the top of this loop body) has skipped an uncounted `materialize`: it
        // is that guard's decision, not a second copy of it.
        cell.preflight(catalog, prepared, guard.counts())?;
        let subject = cell.subject(catalog, prepared)?;
        let subject_axes = cell.subject_axes(&subject);
        let count = guard.counts()
            && ((verify && query_def.expected.is_some()) || single_run || materialize_requested);
        // The describe step: this query's one build outside Criterion. Every
        // untimed number in its report is read from it, and `measure`
        // consumes it before any build is timed.
        let mut described = Described::new(&subject, count);

        let mut metadata = vec![
            MetadataLine::new("benchmark", &definition.name),
            MetadataLine::new("query", &query_def.name),
        ];
        metadata.extend(cell.metadata());
        metadata.push(MetadataLine::new("allocator", crate::ALLOCATOR));
        if C::PER_INDEX_BYTES {
            for (position, ((spec, &rows), &bytes)) in prepared
                .indexes()
                .iter()
                .zip(prepared.index_rows())
                .zip(described.bytes())
                .enumerate()
            {
                metadata.push(MetadataLine::new(
                    "index",
                    describe_index(position, spec, rows, bytes),
                ));
            }
        }
        // Correctness gate: the describe step's count is compared before any
        // measurement time is spent on the query, so a wrong answer can never
        // yield a plausible timing. The count came through the same streaming
        // path `iteration` times, so verifying a huge result never
        // materialised it.
        let verified = match (verify, query_def.expected) {
            | (true, Some(_)) if !guard.counts() => {
                eprintln!(
                    "bench {kind}: query '{}' in benchmark '{}' on {label} is not verified: its \
                     count would materialize the result, which the guard skips",
                    query_def.name, definition.name
                );
                false
            },
            | (true, Some(expected)) => {
                let actual = described
                    .rows()
                    .expect("the describe step counts every query it verifies");
                if actual != expected {
                    anyhow::bail!(
                        "verification failed: benchmark '{}' query '{}' on {} returned {} tuples, \
                         expected {}",
                        definition.name,
                        query_def.name,
                        label,
                        actual,
                        expected
                    );
                }
                metadata.push(MetadataLine::new("verified", "yes"));
                true
            },
            | (true, None) => {
                eprintln!(
                    "bench {kind}: no expected cardinality for query '{}' in benchmark '{}'; not \
                     verified",
                    query_def.name, definition.name
                );
                false
            },
            | (false, _) => false,
        };
        let answer_rows = described.rows();
        if let Some(rows) = answer_rows {
            metadata.push(MetadataLine::new("answer rows", rows));
        }
        // A streaming subject with no `expected` is decided now, from the
        // streamed count the describe step just took.
        if guard == MaterializeVerdict::AfterStreamedCount {
            guard = materialize_guard(true, streams, answer_rows, width, materialize_max_bytes);
            assert_ne!(
                guard,
                MaterializeVerdict::AfterStreamedCount,
                "the describe step counts when materialize is requested"
            );
        }
        let mut run_materialize = materialize_requested;
        if let Some(bytes) = guard.skipped() {
            eprintln!(
                "note: skipping materialize for {}/{} on {label}: {bytes} bytes over \
                 --materialize-max-bytes {materialize_max_bytes}",
                definition.name, query_def.name
            );
            metadata.push(MetadataLine::new(
                "materialize",
                format!("skipped: {bytes} bytes over {materialize_max_bytes}"),
            ));
            run_materialize = false;
        }
        // `--verify`'s check of the materialized count, once; a probe's
        // single-run `materialize` value comes from the same run. A subject
        // that does not stream returns the describe step's count, so
        // `materialize` has still run once on that build.
        if run_materialize && (single_run || (verify && query_def.expected.is_some())) {
            let distinct = described
                .materialize_once()
                .expect("a query subject materializes");
            if let (true, Some(expected)) = (verify, query_def.expected) {
                if distinct != expected {
                    anyhow::bail!(
                        "verification failed: benchmark '{}' query '{}' on {label} materialized \
                         {distinct} distinct full assignments, expected {expected}",
                        definition.name,
                        query_def.name,
                    );
                }
                metadata.push(MetadataLine::new("verified materialize", "yes"));
            }
        }
        // A skipped `materialize` is not measured.
        let measured_metrics: Vec<Metric> = metrics
            .iter()
            .copied()
            .filter(|&metric| metric != Metric::Materialize || run_materialize)
            .collect();
        if single_run {
            let build = described.build_time();
            let mut single = format!("build {build:?}");
            if streams {
                let iteration = described
                    .exercise_time()
                    .expect("a probe counts every query it streams");
                single = format!("{single}, iteration {iteration:?}");
            }
            if run_materialize {
                if let Some(materialize) = described.materialize_time() {
                    single = format!("{single}, materialize {materialize:?}");
                }
            }
            metadata.push(MetadataLine::new("single run", single));
        }
        if metrics.contains(&Metric::EndToEnd) {
            metadata.push(MetadataLine::new("queries per build", queries_per_build));
        }
        for name in prepared.relations() {
            if let Some(header) = catalog.header(name) {
                metadata.push(MetadataLine::new(
                    "relation",
                    format!("{:?} (arity {})", header.name(), header.arity()),
                ));
            }
        }
        let title = if kind == BenchKind::Probe {
            "bench probe metadata"
        } else {
            "bench run metadata"
        };
        write_metadata_block(&mut io::stderr(), title, &metadata)?;

        let group_name =
            group_with_segment(prefix, &definition.name, &query_def.name, &group_segment);
        let measured = described.measure(&MeasureRequest {
            metrics: &measured_metrics,
            single_run,
            queries_per_build,
            criterion,
            group: &group_name,
            function_prefix: None,
        });

        let mut axes = BTreeMap::from([
            ("benchmark".to_string(), serde_json::json!(definition.name)),
            ("query".to_string(), serde_json::json!(query_def.name)),
            (
                "optimiser".to_string(),
                serde_json::json!(planner.optimiser.axis_value()),
            ),
            (
                "tuples".to_string(),
                serde_json::json!(tuples_read(catalog, prepared)),
            ),
            (
                "index_rows".to_string(),
                serde_json::json!(prepared.index_rows().iter().sum::<usize>()),
            ),
            (
                "plan".to_string(),
                serde_json::json!(prepared.variable_order().join(", ")),
            ),
        ]);
        if let Some(rows) = answer_rows {
            axes.insert("answer_rows".to_string(), serde_json::json!(rows));
        }
        // Only meaningful when the end-to-end metric ran; omitting it
        // otherwise keeps historical invocations' reports unchanged.
        if metrics.contains(&Metric::EndToEnd) {
            axes.insert(
                "queries_per_build".to_string(),
                serde_json::json!(queries_per_build),
            );
        }
        axes.extend(cell_axes.clone());
        axes.extend(subject_axes);
        if verified {
            axes.insert("verified".to_string(), serde_json::json!(true));
        }
        sink.push(vec![BenchReport::new(
            kind,
            &metadata,
            axes,
            measured.criterion_groups,
            measured.values,
        )])?;
    }

    Ok(())
}

/// Runs one `bench run` cell by monomorphising [`run_benchmark`] over the
/// [`FamilyCell`] of the family the cell names, pushing each query's report to
/// `sink`. There is no separate algorithm parameter to ignore: the
/// `Execution` fixes both halves of the pair.
pub(crate) fn dispatch_run_bench(
    cell: Execution, workload: &PreparedWorkload, settings: RunSettings<'_>, sink: &mut ReportSink,
) -> anyhow::Result<()> {
    match cell {
        | Execution::TrieLftj(SortedTrie::TreeTrie {
            seek,
            build,
        }) => with_sorted_trie_layout!(seek, |S| run_benchmark(
            &FamilyCell(&TrieLftj::<kermit_ds::TreeTrie<S>>::new(build)),
            workload,
            settings,
            sink,
        )),
        | Execution::TrieLftj(SortedTrie::ColumnTrie {
            seek,
            build,
        }) => with_sorted_trie_layout!(seek, |S| run_benchmark(
            &FamilyCell(&TrieLftj::<kermit_ds::ColumnTrie<S>>::new(build)),
            workload,
            settings,
            sink,
        )),
        | Execution::HashHtj {
            hasher,
            pruning,
            expansion,
            levels,
            config,
            build,
        } => with_hash_trie_layout!(hasher, pruning, expansion, levels, |H, P, E, L| {
            run_benchmark(
                &FamilyCell(&HashHtj::<H, P, E, L>::new(config, build)),
                workload,
                settings,
                sink,
            )
        }),
    }
}

/// Fails if two of `groups` would share a Criterion directory.
///
/// Criterion truncates directory names to 64 bytes, and each group here gets
/// a fresh `Criterion`, whose within-instance de-duplication therefore never
/// sees the other groups: two groups sharing their first 64 bytes write into
/// one `target/criterion/<dir>/`, the later silently replacing the earlier
/// (issue #69). Checked before any cell runs, so a doomed sweep costs nothing.
fn check_group_directories_distinct(
    groups: impl IntoIterator<Item = String>,
) -> anyhow::Result<()> {
    let mut claimed: HashMap<String, String> = HashMap::new();
    for group in groups {
        match claimed.entry(criterion_directory_name(&group)) {
            | Entry::Occupied(first) => anyhow::bail!(
                "Criterion groups '{}' and '{group}' would share the Criterion directory '{}': \
                 Criterion truncates directory names to {CRITERION_MAX_DIRECTORY_NAME_BYTES} \
                 bytes, so the later would overwrite the earlier's results. Shorten --name (or \
                 the benchmark or query name) so every group differs within its first \
                 {CRITERION_MAX_DIRECTORY_NAME_BYTES} bytes.",
                first.get(),
                first.key()
            ),
            | Entry::Vacant(slot) => {
                slot.insert(group);
            },
        }
    }
    Ok(())
}

/// The Criterion group under which one query of one cell is measured:
/// `{prefix}/{workload}/{query}/{segment}`, where the segment is the cell's
/// own (`{ds}/{algo}` for a kermit cell).
fn group_with_segment(prefix: &str, workload: &str, query: &str, segment: &str) -> String {
    format!("{prefix}/{workload}/{query}/{segment}")
}

/// [`group_with_segment`] for an [`Execution`] cell, which is how the sweep's
/// clash check names the groups before any cell runs.
fn criterion_group_name(prefix: &str, workload: &str, query: &str, cell: Execution) -> String {
    group_with_segment(prefix, workload, query, &execution_segment(cell))
}

/// Refuses a `bench run` sweep in which two (benchmark, query, cell) groups
/// would share a Criterion directory; see [`check_group_directories_distinct`].
///
/// Works from names alone, so it fetches nothing and runs before the first
/// cell. A `query_filter` naming no query of a benchmark contributes no
/// groups for it: that benchmark still fails in its own turn, after the
/// benchmarks before it have run and been reported.
pub(crate) fn check_sweep_group_directories(
    prefix: &str, benchmarks: &[BenchmarkDefinition], query_filter: Option<&str>,
    cells: &[Execution],
) -> anyhow::Result<()> {
    check_group_directories_distinct(benchmarks.iter().flat_map(|benchmark| {
        benchmark
            .queries
            .iter()
            .filter(move |query| query_filter.is_none_or(|name| query.name == name))
            .flat_map(move |query| {
                cells.iter().map(move |&cell| {
                    criterion_group_name(prefix, &benchmark.name, &query.name, cell)
                })
            })
    }))
}

/// Expands the `-i` / `-a` selectors into the valid execution cells.
///
/// Incompatible concrete pairs are dropped: with `all` on either side they
/// are announced on stderr, naming the `kind` of `bench` subcommand, and
/// skipped, so `-i all -a all` runs exactly the
/// three valid cells; when the user named a single incompatible pair there
/// is nothing left to run and that is a usage error.
///
/// `given` are the `--ds-*` flags the user passed ([`DsFlag::given`]).
/// [`DsChoices::resolve`] has checked them against `-i` alone, which under
/// `-i all` accepts every flag; but `-a` can then drop every cell with a
/// flag's axis (`-i all -a leapfrog-triejoin --ds-config …` runs no
/// hash-trie cell). Such a flag would be silently ignored, so it is a usage
/// error too (#86).
pub(crate) fn resolve_sweep(
    indexstructure: IndexStructureSelector, algorithm: JoinAlgorithmSelector, choices: DsChoices,
    given: &[DsFlag], kind: BenchKind,
) -> anyhow::Result<Vec<Execution>> {
    let sweep = Sweep::expand(&indexstructure.expand(), &algorithm.expand(), choices);
    if sweep.cells.is_empty() {
        anyhow::bail!(
            "incompatible CLI selection: --indexstructure {indexstructure:?} cannot be joined \
             with --algorithm {algorithm:?} (hash-trie pairs with hash-triejoin; sorted tries \
             pair with leapfrog-triejoin)"
        );
    }
    let ran: Vec<_> = sweep
        .cells
        .iter()
        .map(|cell| cell.index_structure())
        .collect();
    if let Some(flag) = unreached_flag(given, &ran) {
        let cells: Vec<_> = sweep
            .cells
            .iter()
            .map(|cell| format!("({:?}, {:?})", cell.index_structure(), cell.algorithm()))
            .collect();
        anyhow::bail!(
            "{flag} applies only to {structures}, but --algorithm {algorithm:?} leaves this sweep \
             no {structures} cell; it runs only {}. The flag would be silently ignored.",
            cells.join(", "),
            structures = flag.structures_label(),
        );
    }
    for (ds, algo) in &sweep.skipped {
        eprintln!("bench {kind}: skipping incompatible pair ({ds:?}, {algo:?})");
    }
    Ok(sweep.cells)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// #86: under `-i all`, `-a` decides which cells run, and a flag must
    /// reach one of them. Each row is one `-a` with the flags that keep a
    /// cell under it and the flags that lose every cell.
    #[test]
    fn resolve_sweep_rejects_a_flag_the_algorithm_leaves_without_a_cell() {
        use DsFlag::*;
        let tree = Build(kermit_ds::IndexStructure::TreeTrie);
        let column = Build(kermit_ds::IndexStructure::ColumnTrie);
        let hash = Build(kermit_ds::IndexStructure::HashTrie);
        let rows: &[(JoinAlgorithmSelector, &[DsFlag], &[DsFlag])] = &[
            (
                JoinAlgorithmSelector::LeapfrogTriejoin,
                &[LayoutSeek, tree, column],
                &[LayoutHasher, LayoutPruning, LayoutExpansion, Config, hash],
            ),
            (
                JoinAlgorithmSelector::HashTriejoin,
                &[LayoutHasher, LayoutPruning, LayoutExpansion, Config, hash],
                &[LayoutSeek, tree, column],
            ),
            (
                JoinAlgorithmSelector::All,
                &[
                    LayoutHasher,
                    LayoutPruning,
                    LayoutExpansion,
                    LayoutSeek,
                    Config,
                    tree,
                    column,
                    hash,
                ],
                &[],
            ),
        ];
        let sweep = |algorithm, given: &[DsFlag]| {
            resolve_sweep(
                IndexStructureSelector::All,
                algorithm,
                DsChoices::default(),
                given,
                BenchKind::Run,
            )
        };
        for &(algorithm, reached, unreached) in rows {
            if let Err(e) = sweep(algorithm, reached) {
                panic!("{algorithm:?} rejected {reached:?}: {e}");
            }
            for &flag in unreached {
                let msg = sweep(algorithm, &[flag]).unwrap_err().to_string();
                assert!(msg.contains(&flag.to_string()), "{msg}");
                assert!(msg.contains(&format!("{algorithm:?}")), "{msg}");
            }
        }
    }

    /// The group name `bench run` has always used, pinned before it moves.
    #[test]
    fn a_kermit_cells_group_is_prefix_workload_query_structure_algorithm() {
        let cell =
            Execution::for_structure(kermit_ds::IndexStructure::ColumnTrie, DsChoices::default());
        assert_eq!(
            criterion_group_name("run", "triangle", "q0000", cell),
            "run/triangle/q0000/ColumnTrie/LeapfrogTriejoin"
        );
        let cell =
            Execution::for_structure(kermit_ds::IndexStructure::HashTrie, DsChoices::default());
        assert_eq!(
            criterion_group_name("join", "adhoc", "q", cell),
            "join/adhoc/q/HashTrie/HashTriejoin"
        );
    }

    #[test]
    fn groups_sharing_a_truncated_directory_are_rejected() {
        let prefix = format!("run/{}", "long-benchmark-name-".repeat(3));
        let tree = format!("{prefix}/q0000/TreeTrie/LeapfrogTriejoin");
        let column = format!("{prefix}/q0000/ColumnTrie/LeapfrogTriejoin");

        let err = check_group_directories_distinct([tree.clone(), column.clone()])
            .unwrap_err()
            .to_string();

        assert!(err.contains(&tree), "{err}");
        assert!(err.contains(&column), "{err}");
        assert!(err.contains("64"), "{err}");
    }

    #[test]
    fn long_groups_that_differ_within_the_first_64_bytes_are_accepted() {
        // The WatDiv prelim sweep's names: 72 bytes, truncated mid-algorithm,
        // but the structure segment already tells them apart.
        let groups = ["TreeTrie", "ColumnTrie"]
            .map(|ds| format!("run/watdiv-stress-100-test-1-prelim/q0000/{ds}/LeapfrogTriejoin"));
        assert!(groups.iter().all(|g| g.len() > 64));

        check_group_directories_distinct(groups).unwrap();
    }

    #[test]
    fn an_index_line_names_its_id_relation_filter_columns_rows_and_bytes() {
        use kermit_ds::Filter;
        let pinned = IndexSpec {
            relation: "edge".to_string(),
            filter: Filter::new(vec![(1, 2)], vec![]),
            columns: vec![0],
            levels: 1,
        };
        assert_eq!(
            describe_index(0, &pinned, 1, 96),
            "#0 edge filter col1=2 columns [0]: 1 rows, 96 bytes"
        );
        let reversed = IndexSpec {
            relation: "edge".to_string(),
            filter: Filter::none(),
            columns: vec![1, 0],
            levels: 2,
        };
        assert_eq!(
            describe_index(1, &reversed, 3, 128),
            "#1 edge filter - columns [1, 0]: 3 rows, 128 bytes"
        );
        // A trie with levels for fewer columns than it keeps names its count
        // (#116); a full one does not, as above.
        let one_level = IndexSpec {
            levels: 1,
            ..reversed
        };
        assert_eq!(
            describe_index(1, &one_level, 3, 128),
            "#1 edge filter - columns [1, 0] levels 1: 3 rows, 128 bytes"
        );
    }

    #[test]
    fn materialize_bytes_is_rows_times_width_times_eight() {
        assert_eq!(materialize_bytes(0, 3), 0);
        assert_eq!(materialize_bytes(10, 3), 240);
        assert_eq!(
            materialize_bytes(5, 0),
            0,
            "a query of checks has no variable"
        );
        assert_eq!(materialize_bytes(u64::MAX, 2), u64::MAX, "it saturates");
        assert_eq!(
            materialize_bytes(1_453_845_122, 4),
            46_523_043_904,
            "SNAP dblp path3: about 46.5 GB"
        );
    }

    #[test]
    fn the_guard_skips_only_what_exceeds_the_limit() {
        // 131072 rows of one variable are exactly 1 MiB.
        assert_eq!(materialize_skip(131_072, 1, 1_048_576), None);
        assert_eq!(materialize_skip(131_073, 1, 1_048_576), Some(1_048_584));
    }

    /// 131072 rows of one variable are exactly the 1 MiB limit; one more row
    /// is over it.
    const LIMIT: u64 = 1_048_576;

    #[test]
    fn the_guard_decides_a_streaming_subject_from_expected_else_the_streamed_count() {
        use MaterializeVerdict::*;
        // With `expected`: decided before the describe step.
        assert_eq!(materialize_guard(true, true, Some(131_072), 1, LIMIT), Run);
        assert_eq!(
            materialize_guard(true, true, Some(131_073), 1, LIMIT),
            Skip(1_048_584)
        );
        // Without it: decided again once the describe step has counted.
        assert_eq!(
            materialize_guard(true, true, None, 1, LIMIT),
            AfterStreamedCount
        );
        assert_eq!(materialize_guard(true, true, Some(10), 1, LIMIT), Run);
        assert_eq!(
            materialize_guard(true, true, Some(1_000_000), 1, LIMIT),
            Skip(8_000_000)
        );
    }

    #[test]
    fn the_guard_decides_a_subject_that_does_not_stream_before_anything_materialises() {
        use MaterializeVerdict::*;
        assert_eq!(materialize_guard(true, false, Some(131_072), 1, LIMIT), Run);
        // Over the limit nothing materialises, not even the describe step's
        // count.
        let over = materialize_guard(true, false, Some(131_073), 1, LIMIT);
        assert_eq!(over, SkipUncounted(1_048_584));
        assert!(!over.counts());
        // No `expected`: the warning, then everything runs unguarded.
        let unguarded = materialize_guard(true, false, None, 1, LIMIT);
        assert_eq!(unguarded, Unguarded);
        assert!(unguarded.counts());
    }

    #[test]
    fn the_guard_counts_unless_a_non_streaming_subject_is_skipped() {
        use MaterializeVerdict::*;
        for streams in [true, false] {
            assert_eq!(
                materialize_guard(false, streams, Some(u64::MAX), 4, LIMIT),
                NotAsked,
                "nothing is decided when nothing would materialise"
            );
            assert!(NotAsked.counts());
        }
        assert!(Run.counts());
        assert!(Skip(9).counts(), "a streamed count is cheap");
        assert!(AfterStreamedCount.counts());
        assert!(!SkipUncounted(9).counts());
        assert_eq!(Skip(9).skipped(), Some(9));
        assert_eq!(SkipUncounted(9).skipped(), Some(9));
        assert_eq!(Run.skipped(), None);
    }

    #[test]
    fn a_non_streaming_subject_is_guarded_when_only_verify_or_a_probe_would_materialise() {
        use MaterializeVerdict::*;
        // (streams, metric, verifies, single_run)
        assert!(
            guard_requested(false, false, true, false),
            "--verify with expected"
        );
        assert!(
            guard_requested(false, false, false, true),
            "a probe's single run"
        );
        assert!(guard_requested(false, true, false, false), "the metric");
        assert!(
            !guard_requested(false, false, false, false),
            "nothing would materialise"
        );
        // A streaming subject counts by streaming, so only the metric asks.
        assert!(!guard_requested(true, false, true, true));
        assert!(guard_requested(true, true, false, false));
        // `--verify` alone, `expected` over the limit: skipped, and uncounted.
        let verdict = materialize_guard(
            guard_requested(false, false, true, false),
            false,
            Some(131_073),
            1,
            LIMIT,
        );
        assert_eq!(verdict, SkipUncounted(1_048_584));
        assert!(
            !verdict.counts(),
            "so there is no describe-step count to verify"
        );
        // The same run on a streaming subject is not guarded at all.
        assert_eq!(
            materialize_guard(
                guard_requested(true, false, true, false),
                true,
                Some(131_073),
                1,
                LIMIT
            ),
            NotAsked
        );
    }
}
