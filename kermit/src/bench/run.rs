//! The generic runner behind `bench run` and `bench join`: one execution
//! cell over one [`Workload`], every requested metric, one report per
//! query.

use {
    super::{
        add_space_bench, build_space_criterion, build_time_criterion, criterion_directory_name,
        Metric, Workload, CRITERION_MAX_DIRECTORY_NAME_BYTES,
    },
    crate::{
        bench_report::{
            write_metadata_block, BenchKind, BenchReport, CriterionGroupRef, MetadataLine,
            ReportMetric,
        },
        execution::{Execution, ExecutionFamily, HashHtj, SortedTrie, Sweep, TrieLftj},
        options::{
            unreached_flag, with_hash_trie_layout, with_sorted_trie_layout, DsChoices, DsFlag,
            PlannerArgs,
        },
        BenchArgs, IndexStructureSelector, JoinAlgorithmSelector,
    },
    kermit::db::index_header,
    kermit_algos::{ColumnOrderPolicy, IndexSpec},
    kermit_bench::BenchmarkDefinition,
    kermit_ds::{HeapSize, Relation, RelationHeader},
    std::{
        collections::{hash_map::Entry, BTreeMap, HashMap},
        io,
    },
};

/// Why a timed join cannot fail: [`run_benchmark`] calls
/// [`Workload::validate`] on every query before it builds anything, and
/// the join rejects exactly the queries validation does.
const VALIDATED: &str = "query validated against the workload before timing";

/// Everything a measured join needs besides the cell and the workload:
/// what kind of report to stamp, how to name the Criterion group, how the
/// queries are planned (optimiser and column-order policy), and what to
/// measure. One value is built
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
    /// `--optimiser` and `--column-orders`.
    pub planner: PlannerArgs,
    pub metrics: &'a [Metric],
    /// K in the `end_to_end` metric's `T = build + K × query`.
    pub queries_per_build: u32,
    /// Run each query once before timing and compare its tuple count with
    /// the workload's `expected`; a mismatch aborts.
    pub verify: bool,
    /// Criterion sample/measurement/warm-up settings.
    pub bench_args: &'a BenchArgs,
}

/// Runs every query of `workload` on one execution cell and returns one
/// report per query, each stamped with `settings.kind` and grouped under
/// `settings.prefix`.
///
/// Generic over the [`ExecutionFamily`] so the sorted family
/// (`TrieLftj<R>`) and the hash family (`HashHtj<H, P, E>`) share one body:
/// relation loading, metadata, Criterion group wiring and report assembly
/// are identical, and the report's `data_structure` / `algorithm` axes
/// come from `family.execution()` — the same value that picked the code
/// path — so a report can never name an algorithm it did not run (issue
/// #56). Every query is validated against the relations' headers before
/// anything is loaded, so a query that cannot run aborts the cell with an
/// error rather than a panic inside Criterion (issue #78). With
/// `settings.verify`, each query with an `expected` count is executed once
/// before timing and a mismatch aborts. Under `--column-orders any`, each
/// query's reordered copies are built before it is verified or timed and
/// dropped after it (see `ExecutionFamily::add_index`).
fn run_benchmark<F: ExecutionFamily>(
    family: &F, workload: &Workload, settings: RunSettings<'_>,
) -> anyhow::Result<Vec<BenchReport>> {
    let RunSettings {
        kind,
        prefix,
        planner,
        metrics,
        queries_per_build,
        verify,
        bench_args,
    } = settings;
    let column_orders = planner.column_orders;
    // Reject a query that cannot run before loading anything: the headers
    // alone settle it, and a failure inside a timed closure below could
    // only panic.
    workload.validate(column_orders)?;
    // Load each relation from disk exactly once; the family builds its
    // engine from these typed relations rather than re-reading the files.
    // Rebuilding metrics rebuild from each relation's tuples in file order,
    // kept here (see `RelationFamily::load_with_tuples`): `insertion`,
    // `end_to_end`, and, for a family whose joins mutate their relations,
    // `iteration` and `--verify`, which then never probe the loaded engine.
    // Under `--column-orders any` so does every query's reordered copy. A
    // run with no rebuilding metric keeps none: it would be a dead copy of
    // the whole workload.
    let rebuilds = metrics
        .iter()
        .any(|m| matches!(m, Metric::Insertion | Metric::EndToEnd))
        || (F::JOIN_MUTATES && (verify || metrics.contains(&Metric::Iteration)))
        || column_orders == ColumnOrderPolicy::Any;
    let mut relations: Vec<F::Rel> = Vec::with_capacity(workload.relation_paths.len());
    let mut build_inputs: Vec<(RelationHeader, Vec<Vec<usize>>)> = Vec::new();
    for path in &workload.relation_paths {
        if rebuilds {
            let (relation, tuples) = family.load_with_tuples(path)?;
            build_inputs.push((relation.header().clone(), tuples));
            relations.push(relation);
        } else {
            relations.push(family.load(path)?);
        }
    }
    // Each query's reordered copies (`--column-orders any`) are built from
    // the same file-order tuples `insertion` rebuilds from, through the
    // same `build_relation`, into whichever engine the query reads.
    let input_of = |base: &str| -> &(RelationHeader, Vec<Vec<usize>>) {
        build_inputs
            .iter()
            .find(|(header, _)| header.name() == base)
            .expect("validation checked that every base relation was loaded")
    };
    let add_copies = |engine: &mut F::Engine, specs: &[IndexSpec]| {
        for spec in specs {
            let (header, tuples) = input_of(&spec.base);
            family.add_index(engine, spec.clone(), header, tuples);
        }
    };
    let mut engine = family.build(relations);

    // `ds_name`/`algo_name` become the report's identity axes and the
    // on-disk `target/criterion/{group}` names (the group_name below embeds
    // them). The labels are pinned by `IndexStructure::axis_value` and
    // `JoinAlgorithm::axis_value`; see the tests there before changing one.
    let execution = family.execution();
    let ds_name = execution.index_structure().axis_value();
    let algo_name = execution.algorithm().axis_value();

    let has_time_metrics = metrics
        .iter()
        .any(|m| matches!(m, Metric::Insertion | Metric::Iteration | Metric::EndToEnd));

    // Everything read from the base relations is taken once, here, so the
    // per-query loop can add and drop copies in the engine.
    let (total_tuples, optimization_axes, relation_lines, as_built_bytes) = {
        let relations = F::relations(&engine);
        // Sum across relations: scaling plots key off this as the workload's
        // total input size. One walk per relation is cheap vs the bench
        // itself.
        let total_tuples: usize = relations.iter().map(|r| F::tuple_count(r)).sum();

        // Standard optimization axes: merge in dimensions emitted by the DS.
        // Every relation in this run shares the same type, so any one of
        // them produces the canonical `ds_layout_*` set. The `ds_*` prefix
        // convention (see `kermit_iters::HasOptimizationAxes`) guarantees no
        // collision with the base axes assembled per query below.
        let mut optimization_axes = relations
            .first()
            .map(|r| F::optimization_axes(r))
            .unwrap_or_default();
        // The build mode comes from the family, not a relation: it describes
        // the build, which leaves no trace in the structure, and it must be
        // present even for a workload with no relations.
        optimization_axes.extend(family.build_mode_axes());

        let relation_lines: Vec<String> = relations
            .iter()
            .map(|rel| {
                let h = rel.header();
                format!("{:?} (arity {})", h.name(), h.arity())
            })
            .collect();

        // A family whose joins mutate their relations (a lazy HashTrie
        // expands what a join reaches, #92) never probes this engine:
        // `--verify` and `iteration` run on fresh builds, so `space`
        // measures the relations as built, whatever ran before it. Their
        // footprint now is what each `space` block checks against.
        let as_built_bytes: Option<Vec<usize>> =
            F::JOIN_MUTATES.then(|| relations.iter().map(|r| r.heap_size_bytes()).collect());
        (
            total_tuples,
            optimization_axes,
            relation_lines,
            as_built_bytes,
        )
    };

    let mut reports: Vec<BenchReport> = Vec::with_capacity(workload.queries.len());

    for query_def in &workload.queries {
        // The copies this query reads under `any` (none under `stored`):
        // built into the loaded engine before anything is verified or
        // timed, and dropped after the query, so one query's copies are
        // held at a time. Planning reads only statistics, so this never
        // probes a relation.
        let specs = family.required_indexes(&engine, &query_def.query)?;
        add_copies(&mut engine, &specs);
        // As for the base relations: the copies' footprint as built.
        let copies_as_built: Option<Vec<usize>> = F::JOIN_MUTATES.then(|| {
            F::indexes(&engine)
                .iter()
                .map(|(_, copy)| copy.heap_size_bytes())
                .collect()
        });

        let mut metadata = vec![
            MetadataLine::new("benchmark", &workload.name),
            MetadataLine::new("query", &query_def.name),
            MetadataLine::new("data structure", ds_name),
            MetadataLine::new("algorithm", algo_name),
        ];
        for spec in &specs {
            metadata.push(MetadataLine::new("index", spec.describe()));
        }
        // Correctness gate: with `--verify`, run the query once (untimed)
        // and compare the answer count before spending any measurement time
        // on it. A mismatch aborts so a wrong answer can never yield a
        // plausible timing. The count comes through the same streaming path
        // the `iteration` metric times, so verifying a huge result never
        // materialises it.
        let verified = if verify {
            match query_def.expected {
                | Some(expected) => {
                    let actual = if F::JOIN_MUTATES {
                        let mut fresh = family.build_from_tuples(build_inputs.clone());
                        add_copies(&mut fresh, &specs);
                        family.count(&fresh, query_def.query.clone())?
                    } else {
                        family.count(&engine, query_def.query.clone())?
                    };
                    if actual != expected {
                        anyhow::bail!(
                            "verification failed: benchmark '{}' query '{}' on {}/{} returned {} \
                             tuples, expected {}",
                            workload.name,
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
                | None => {
                    eprintln!(
                        "bench run: no expected cardinality for query '{}' in benchmark '{}'; not \
                         verified",
                        query_def.name, workload.name
                    );
                    false
                },
            }
        } else {
            false
        };
        if metrics.contains(&Metric::EndToEnd) {
            metadata.push(MetadataLine::new("queries per build", queries_per_build));
        }
        for line in &relation_lines {
            metadata.push(MetadataLine::new("relation", line));
        }
        write_metadata_block(&mut io::stderr(), "bench run metadata", &metadata)?;

        let group_name = criterion_group_name(prefix, &workload.name, &query_def.name, execution);

        let mut criterion_groups: Vec<CriterionGroupRef> = Vec::new();

        if has_time_metrics {
            let mut criterion = build_time_criterion(bench_args);
            let mut group = criterion.benchmark_group(&group_name);

            if metrics.contains(&Metric::Insertion) {
                group.bench_function("insertion", |b| {
                    b.iter_batched(
                        || build_inputs.clone(),
                        |data| {
                            // Times the per-relation construction only —
                            // `family.build_relation` rather than the whole
                            // engine build, which `end_to_end` covers — and
                            // goes through the family so the build honours
                            // the same configuration and build mode the
                            // report's `ds_config_*` / `ds_build_mode` axes
                            // name.
                            for (header, tuples) in data {
                                std::hint::black_box(family.build_relation(header, tuples));
                            }
                        },
                        criterion::BatchSize::SmallInput,
                    );
                });
                criterion_groups.push(CriterionGroupRef {
                    group: group_name.clone(),
                    function: "insertion".to_string(),
                    metric: ReportMetric::Time,
                });

                // The price of `any`: permuting and building this query's
                // copies, timed as one function beside `insertion` and
                // through the same `build_relation`. The permutation is
                // inside the timed region, since a copy cannot be built
                // without it. Emitted only when the plan needs a copy, so a
                // `stored` report is unchanged.
                if !specs.is_empty() {
                    group.bench_function("copies", |b| {
                        b.iter_batched(
                            || specs.clone(),
                            |specs| {
                                for spec in specs {
                                    let (header, tuples) = input_of(&spec.base);
                                    let header = index_header(&spec, header);
                                    std::hint::black_box(
                                        family.build_relation(header, spec.permute_all(tuples)),
                                    );
                                }
                            },
                            criterion::BatchSize::SmallInput,
                        );
                    });
                    criterion_groups.push(CriterionGroupRef {
                        group: group_name.clone(),
                        function: "copies".to_string(),
                        metric: ReportMetric::Time,
                    });
                }
            }

            if metrics.contains(&Metric::Iteration) {
                // Times the join with its rows counted, never collected: the
                // timed region holds no per-row allocation, and a batch keeps
                // only `u64`s alive, so memory is independent of result size
                // (issue #65).
                if F::JOIN_MUTATES {
                    // Cold (#92): each timed join runs on an engine built in
                    // the untimed setup, so it pays the expansion its own
                    // probes cause instead of finding it done by an earlier
                    // sample. The setup builds the query's copies too, so
                    // they are as cold as the base relations. The routine
                    // hands the engine back, so Criterion drops it after
                    // timing, and `PerIteration` keeps one fresh engine
                    // alive at a time.
                    group.bench_function("iteration", |b| {
                        b.iter_batched(
                            || {
                                let mut fresh = family.build_from_tuples(build_inputs.clone());
                                add_copies(&mut fresh, &specs);
                                (fresh, query_def.query.clone())
                            },
                            |(fresh, q)| {
                                let rows = family.count(&fresh, q).expect(VALIDATED);
                                (fresh, rows)
                            },
                            criterion::BatchSize::PerIteration,
                        );
                    });
                } else {
                    group.bench_function("iteration", |b| {
                        b.iter_batched(
                            || query_def.query.clone(),
                            |q| family.count(&engine, q).expect(VALIDATED),
                            criterion::BatchSize::SmallInput,
                        );
                    });
                }
                criterion_groups.push(CriterionGroupRef {
                    group: group_name.clone(),
                    function: "iteration".to_string(),
                    metric: ReportMetric::Time,
                });
            }

            if metrics.contains(&Metric::EndToEnd) {
                // The timed body rebuilds the engine from the file-order
                // tuples kept above, through the same `build_relation` path
                // that loaded the untimed engine and that the `insertion`
                // metric times, so the build term is the one the
                // `iteration` metric's engine paid. It additionally pays for
                // assembling the relation store and, under `any`, for this
                // query's copies, which the engine `iteration` reads holds.
                //
                // PerIteration: a fresh build per sample is the point of this
                // metric — batching would amortise away the construction cost
                // whose interaction with query traversal is under measurement.
                group.bench_function("end_to_end", |b| {
                    b.iter_batched(
                        || {
                            (build_inputs.clone(), vec![
                                query_def.query.clone();
                                queries_per_build as usize
                            ])
                        },
                        |(inputs, queries)| {
                            let mut fresh = family.build_from_tuples(inputs);
                            add_copies(&mut fresh, &specs);
                            for q in queries {
                                std::hint::black_box(family.count(&fresh, q).expect(VALIDATED));
                            }
                        },
                        criterion::BatchSize::PerIteration,
                    );
                });
                criterion_groups.push(CriterionGroupRef {
                    group: group_name.clone(),
                    function: "end_to_end".to_string(),
                    metric: ReportMetric::Time,
                });
            }

            group.finish();
            criterion.final_summary();
        }

        if metrics.contains(&Metric::Space) {
            let relations = F::relations(&engine);
            let copies = F::indexes(&engine);
            if let (Some(as_built), Some(copies_as_built)) = (&as_built_bytes, &copies_as_built) {
                let now: Vec<usize> = relations.iter().map(|r| r.heap_size_bytes()).collect();
                let copies_now: Vec<usize> =
                    copies.iter().map(|(_, c)| c.heap_size_bytes()).collect();
                anyhow::ensure!(
                    now == *as_built && copies_now == *copies_as_built,
                    "internal error: a probe reached the loaded engine of a {ds_name} cell whose \
                     joins mutate their relations, so `space` would measure them partly expanded; \
                     see `ExecutionFamily::JOIN_MUTATES`"
                );
            }
            let mut criterion = build_space_criterion(bench_args);
            let mut group = criterion.benchmark_group(&group_name);
            for rel in relations {
                let function = format!("space/{}", rel.header().name());
                criterion_groups.push(add_space_bench(&mut group, &group_name, function, rel));
            }
            // One function per copy, beside the base relations'.
            for (spec, copy) in copies {
                let function = format!("space/{}", spec.name);
                criterion_groups.push(add_space_bench(&mut group, &group_name, function, copy));
            }
            group.finish();
            criterion.final_summary();
        }

        let mut axes = BTreeMap::from([
            ("benchmark".to_string(), serde_json::json!(workload.name)),
            ("query".to_string(), serde_json::json!(query_def.name)),
            ("data_structure".to_string(), serde_json::json!(ds_name)),
            ("algorithm".to_string(), serde_json::json!(algo_name)),
            (
                "optimiser".to_string(),
                serde_json::json!(planner.optimiser.axis_value()),
            ),
            (
                "column_orders".to_string(),
                serde_json::json!(column_orders.axis_value()),
            ),
            ("tuples".to_string(), serde_json::json!(total_tuples)),
        ]);
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
        reports.push(BenchReport::new(kind, &metadata, axes, criterion_groups));

        F::clear_indexes(&mut engine);
    }

    Ok(reports)
}

/// Runs one `bench run` cell by monomorphising [`run_benchmark`] over the
/// [`ExecutionFamily`] the cell names. There is no separate algorithm
/// parameter to ignore: the `Execution` fixes both halves of the pair.
pub(crate) fn dispatch_run_bench(
    cell: Execution, workload: &Workload, settings: RunSettings<'_>,
) -> anyhow::Result<Vec<BenchReport>> {
    // One planner per family: a `Planner` owns its optimiser.
    let planner = || settings.planner.instantiate();
    match cell {
        | Execution::TrieLftj(SortedTrie::TreeTrie {
            seek,
        }) => with_sorted_trie_layout!(seek, |S| run_benchmark(
            &TrieLftj::<kermit_ds::TreeTrie<S>>::new((), planner()),
            workload,
            settings,
        )),
        | Execution::TrieLftj(SortedTrie::ColumnTrie {
            seek,
            build,
        }) => with_sorted_trie_layout!(seek, |S| run_benchmark(
            &TrieLftj::<kermit_ds::ColumnTrie<S>>::new(build, planner()),
            workload,
            settings,
        )),
        | Execution::HashHtj {
            hasher,
            pruning,
            expansion,
            config,
        } => with_hash_trie_layout!(hasher, pruning, expansion, |H, P, E| run_benchmark(
            &HashHtj::<H, P, E>::new(config, planner()),
            workload,
            settings,
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
/// are announced on stderr and skipped, so `-i all -a all` runs exactly the
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
    given: &[DsFlag],
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
        eprintln!("bench run: skipping incompatible pair ({ds:?}, {algo:?})");
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
        let rows: &[(JoinAlgorithmSelector, &[DsFlag], &[DsFlag])] = &[
            (
                JoinAlgorithmSelector::LeapfrogTriejoin,
                &[LayoutSeek, Build],
                &[LayoutHasher, LayoutPruning, LayoutExpansion, Config],
            ),
            (
                JoinAlgorithmSelector::HashTriejoin,
                &[LayoutHasher, LayoutPruning, LayoutExpansion, Config],
                &[LayoutSeek, Build],
            ),
            (
                JoinAlgorithmSelector::All,
                &[
                    LayoutHasher,
                    LayoutPruning,
                    LayoutExpansion,
                    LayoutSeek,
                    Config,
                    Build,
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
}
