//! The generic runner behind `bench run`, `bench join` and `bench probe`:
//! one execution cell over one [`PreparedWorkload`], every requested metric,
//! one report per query, pushed as each query finishes.

use {
    super::{
        criterion_directory_name,
        measure::{CriterionSettings, Described, MeasureRequest, QuerySubject},
        workload::PreparedWorkload,
        Metric, CRITERION_MAX_DIRECTORY_NAME_BYTES,
    },
    crate::{
        bench_report::{write_metadata_block, BenchKind, BenchReport, MetadataLine, ReportSink},
        execution::{Execution, ExecutionFamily, HashHtj, SortedTrie, Sweep, TrieLftj},
        options::{
            unreached_flag, with_hash_trie_layout, with_sorted_trie_layout, DsChoices, DsFlag,
            PlannerArgs,
        },
        IndexStructureSelector, JoinAlgorithmSelector,
    },
    kermit::db::{Catalog, Prepared},
    kermit_algos::{IndexSpec, INDEX_ID_PREFIX},
    kermit_bench::BenchmarkDefinition,
    kermit_ds::{RelationHeader, TupleView, Tuples},
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
    /// Criterion sample/measurement/warm-up settings.
    pub criterion: &'a CriterionSettings,
}

/// The value of one `index` metadata line, for the `position`th index of a
/// query: `{id} {relation} filter {filter} columns {columns:?}: {rows} rows,
/// {bytes} bytes`. `rows` is what the build copied, the index view's count,
/// and `bytes` the index's footprint as built.
fn describe_index(position: usize, spec: &IndexSpec, rows: usize, bytes: usize) -> String {
    format!(
        "{INDEX_ID_PREFIX}{position} {} filter {} columns {:?}: {rows} rows, {bytes} bytes",
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

/// Runs every query of `workload` on one execution cell and pushes one
/// report per query to `sink` as soon as that query finishes, each stamped
/// with `settings.kind` and grouped under `settings.prefix`.
///
/// Generic over the [`ExecutionFamily`] so the sorted family
/// (`TrieLftj<R>`) and the hash family (`HashHtj<H, P, E>`) share one body:
/// metadata, Criterion group wiring and report assembly are identical, and
/// the report's `data_structure` / `algorithm` axes come from
/// `family.execution()` — the same value that picked the code path — so a
/// report can never name an algorithm it did not run (issue #56).
///
/// The workload arrives prepared (`Workload::prepare`): every query is
/// validated and planned, and the relations the queries name sit in the
/// catalog as flat tuples, so a query that cannot run never reaches this
/// function (issue #78). Per query, the cell measures a [`QuerySubject`]
/// through the measure layer. One describe build outside Criterion gives the
/// `index` lines, the exact `space` and the count `--verify` checks, which
/// `bench probe` also times; then
/// [`Described::measure`] runs `iteration` on that build, drops it, and times
/// `build` and `end_to_end`. With `settings.verify`, a query with an
/// `expected` count is checked before any timer starts, and a mismatch aborts
/// with every earlier query's report already in `sink`.
fn run_benchmark<F: ExecutionFamily>(
    family: &F, workload: &PreparedWorkload, settings: RunSettings<'_>, sink: &mut ReportSink,
) -> anyhow::Result<()> {
    let RunSettings {
        kind,
        prefix,
        planner,
        metrics,
        queries_per_build,
        verify,
        single_run,
        criterion,
    } = settings;
    let PreparedWorkload {
        workload: definition,
        catalog,
        prepared: prepared_queries,
    } = workload;

    // `ds_name`/`algo_name` become the report's identity axes and the
    // on-disk `target/criterion/{group}` names (the group_name below embeds
    // them). The labels are pinned by `IndexStructure::axis_value` and
    // `JoinAlgorithm::axis_value`; see the tests there before changing one.
    let execution = family.execution();
    let ds_name = execution.index_structure().axis_value();
    let algo_name = execution.algorithm().axis_value();

    // Standard optimization axes: the dimensions the structure emits (the
    // `ds_*` prefix convention, see `kermit_iters::HasOptimizationAxes`,
    // guarantees no collision with the base axes assembled per query below)
    // plus the build mode, which comes from the family because it leaves no
    // trace in the structure. They are read off one empty relation, since
    // they depend on the type, not on the data, and a query whose atoms are
    // all constant checks builds no index to read them from.
    let optimization_axes = {
        let empty = Tuples::new(1);
        let probe = family.build_index(
            RelationHeader::new_positional("axes", 1),
            TupleView::all(&empty),
        );
        let mut axes = F::optimization_axes(&probe);
        axes.extend(family.build_mode_axes());
        axes
    };

    for (query_def, prepared) in definition.queries.iter().zip(prepared_queries) {
        // The describe step: this query's one build outside Criterion. Every
        // untimed number in its report is read from it, and `measure`
        // consumes it before any build is timed.
        let subject = QuerySubject {
            family,
            catalog,
            prepared,
        };
        let count = (verify && query_def.expected.is_some()) || single_run;
        let described = Described::new(&subject, count);

        let mut metadata = vec![
            MetadataLine::new("benchmark", &definition.name),
            MetadataLine::new("query", &query_def.name),
            MetadataLine::new("data structure", ds_name),
            MetadataLine::new("algorithm", algo_name),
            MetadataLine::new("allocator", crate::ALLOCATOR),
        ];
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
        // Correctness gate: the describe step's count is compared before any
        // measurement time is spent on the query, so a wrong answer can never
        // yield a plausible timing. The count came through the same streaming
        // path `iteration` times, so verifying a huge result never
        // materialised it.
        let verified = match (verify, query_def.expected) {
            | (true, Some(expected)) => {
                let actual = described
                    .rows()
                    .expect("the describe step counts every query it verifies");
                if actual != expected {
                    anyhow::bail!(
                        "verification failed: benchmark '{}' query '{}' on {}/{} returned {} \
                         tuples, expected {}",
                        definition.name,
                        query_def.name,
                        ds_name,
                        algo_name,
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
        if single_run {
            let build = described.build_time();
            let iteration = described
                .exercise_time()
                .expect("a probe counts every query");
            metadata.push(MetadataLine::new(
                "single run",
                format!("build {build:?}, iteration {iteration:?}"),
            ));
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

        let group_name = criterion_group_name(prefix, &definition.name, &query_def.name, execution);
        let measured = described.measure(&MeasureRequest {
            metrics,
            single_run,
            queries_per_build,
            criterion,
            group: &group_name,
            function_prefix: None,
        });

        let mut axes = BTreeMap::from([
            ("benchmark".to_string(), serde_json::json!(definition.name)),
            ("query".to_string(), serde_json::json!(query_def.name)),
            ("data_structure".to_string(), serde_json::json!(ds_name)),
            ("algorithm".to_string(), serde_json::json!(algo_name)),
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
        axes.extend(optimization_axes.clone());
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
/// [`ExecutionFamily`] the cell names, pushing each query's report to
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
            &TrieLftj::<kermit_ds::TreeTrie<S>>::new(build),
            workload,
            settings,
            sink,
        )),
        | Execution::TrieLftj(SortedTrie::ColumnTrie {
            seek,
            build,
        }) => with_sorted_trie_layout!(seek, |S| run_benchmark(
            &TrieLftj::<kermit_ds::ColumnTrie<S>>::new(build),
            workload,
            settings,
            sink,
        )),
        | Execution::HashHtj {
            hasher,
            pruning,
            expansion,
            config,
            build,
        } => with_hash_trie_layout!(hasher, pruning, expansion, |H, P, E| run_benchmark(
            &HashHtj::<H, P, E>::new(config, build),
            workload,
            settings,
            sink,
        )),
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
/// `{prefix}/{workload}/{query}/{ds}/{algo}`.
fn criterion_group_name(prefix: &str, workload: &str, query: &str, cell: Execution) -> String {
    format!(
        "{prefix}/{workload}/{query}/{}/{}",
        cell.index_structure().axis_value(),
        cell.algorithm().axis_value()
    )
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
        };
        assert_eq!(
            describe_index(0, &pinned, 1, 96),
            "#0 edge filter col1=2 columns [0]: 1 rows, 96 bytes"
        );
        let reversed = IndexSpec {
            relation: "edge".to_string(),
            filter: Filter::none(),
            columns: vec![1, 0],
        };
        assert_eq!(
            describe_index(1, &reversed, 3, 128),
            "#1 edge filter - columns [1, 0]: 3 rows, 128 bytes"
        );
    }
}
