//! The generic runner behind `bench run` and `bench join`: one execution
//! cell over one [`Workload`], every requested metric, one report per
//! query.

use {
    super::{add_space_bench, build_space_criterion, build_time_criterion, Metric, Workload},
    crate::{
        bench_report::{
            write_metadata_block, BenchKind, BenchReport, CriterionGroupRef, MetadataLine,
            ReportMetric,
        },
        execution::{Execution, ExecutionFamily, HashHtj, SortedTrie, Sweep, TrieLftj},
        options::{with_hash_trie_layout, HasherChoice, PruningChoice},
        BenchArgs, IndexStructureSelector, JoinAlgorithmSelector,
    },
    kermit_algos::Optimiser,
    kermit_ds::{HashTrieConfig, Relation},
    std::{collections::BTreeMap, io},
};

/// Everything a measured join needs besides the cell and the workload:
/// what kind of report to stamp, how to name the Criterion group, which
/// optimiser plans the queries, and what to measure. One value is built
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
    pub optimiser: Optimiser,
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
/// (`TrieLftj<R>`) and the hash family (`HashHtj<H, P>`) share one body:
/// relation loading, metadata, Criterion group wiring and report assembly
/// are identical, and the report's `data_structure` / `algorithm` axes
/// come from `family.execution()` — the same value that picked the code
/// path — so a report can never name an algorithm it did not run (issue
/// #56). With `settings.verify`, each query with an `expected` count is
/// executed once before timing and a mismatch aborts.
fn run_benchmark<F: ExecutionFamily>(
    family: &F, workload: &Workload, settings: RunSettings<'_>,
) -> anyhow::Result<Vec<BenchReport>> {
    let RunSettings {
        kind,
        prefix,
        optimiser,
        metrics,
        queries_per_build,
        verify,
        bench_args,
    } = settings;
    // Load each relation from disk exactly once; the family builds its
    // engine from these typed relations rather than re-reading the files.
    let relations: Vec<F::Rel> = workload
        .relation_paths
        .iter()
        .map(|p| family.load(p))
        .collect::<Result<_, _>>()?;
    let engine = family.build(relations);
    let relations = F::relations(&engine);

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

    // Sum across relations: scaling plots key off this as the workload's
    // total input size. One walk per relation is cheap vs the bench itself.
    let total_tuples: usize = relations.iter().map(|r| F::tuple_count(r)).sum();

    // Standard optimization axes: merge in dimensions emitted by the DS.
    // Every relation in this run shares the same type, so any one of them
    // produces the canonical `ds_layout_*` set. The `ds_*` prefix convention
    // (see `kermit_iters::HasOptimizationAxes`) guarantees no collision with
    // the base axes assembled per query below.
    let optimization_axes = relations
        .first()
        .map(|r| F::optimization_axes(r))
        .unwrap_or_default();

    let mut reports: Vec<BenchReport> = Vec::with_capacity(workload.queries.len());

    for query_def in &workload.queries {
        let mut metadata = vec![
            MetadataLine::new("benchmark", &workload.name),
            MetadataLine::new("query", &query_def.name),
            MetadataLine::new("data structure", ds_name),
            MetadataLine::new("algorithm", algo_name),
        ];
        // Correctness gate: with `--verify`, run the query once (untimed)
        // and compare the answer count before spending any measurement time
        // on it. A mismatch aborts so a wrong answer can never yield a
        // plausible timing.
        let verified = if verify {
            match query_def.expected {
                | Some(expected) => {
                    let actual = family.join(&engine, query_def.query.clone()).len() as u64;
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
        for rel in &relations {
            let h = rel.header();
            metadata.push(MetadataLine::new(
                "relation",
                format!("{:?} (arity {})", h.name(), h.arity()),
            ));
        }
        write_metadata_block(&mut io::stderr(), "bench run metadata", &metadata)?;

        let group_name = format!(
            "{}/{}/{}/{}/{}",
            prefix, workload.name, query_def.name, ds_name, algo_name
        );

        let mut criterion_groups: Vec<CriterionGroupRef> = Vec::new();

        if has_time_metrics {
            let mut criterion = build_time_criterion(bench_args);
            let mut group = criterion.benchmark_group(&group_name);

            // Snapshot each relation's header + tuples once; both the
            // `insertion` and `end_to_end` bodies rebuild from these. Skipped
            // for an `iteration`-only run, where it would be a dead copy of
            // the whole workload.
            let build_inputs: Vec<(kermit_ds::RelationHeader, Vec<Vec<usize>>)> = if metrics
                .iter()
                .any(|m| matches!(m, Metric::Insertion | Metric::EndToEnd))
            {
                relations
                    .iter()
                    .map(|r| (r.header().clone(), F::tuples(r)))
                    .collect()
            } else {
                Vec::new()
            };

            if metrics.contains(&Metric::Insertion) {
                group.bench_function("insertion", |b| {
                    b.iter_batched(
                        || build_inputs.clone(),
                        |data| {
                            // Times the per-relation construction only —
                            // `family.build_relation` rather than the whole
                            // engine build, which `end_to_end` covers — and
                            // goes through the family so the build honours
                            // the same configuration the report's
                            // `ds_config_*` axes name.
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
            }

            if metrics.contains(&Metric::Iteration) {
                group.bench_function("iteration", |b| {
                    b.iter_batched(
                        || query_def.query.clone(),
                        |q| family.join(&engine, q),
                        criterion::BatchSize::SmallInput,
                    );
                });
                criterion_groups.push(CriterionGroupRef {
                    group: group_name.clone(),
                    function: "iteration".to_string(),
                    metric: ReportMetric::Time,
                });
            }

            if metrics.contains(&Metric::EndToEnd) {
                // The timed body rebuilds the engine through the same path
                // the untimed `family.build` above used, so the build term
                // is the one the `iteration` metric's engine actually paid —
                // NOT the presorting `from_tuples` path the `insertion`
                // metric times.
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
                            let fresh = family.build_from_tuples(inputs);
                            for q in queries {
                                std::hint::black_box(family.join(&fresh, q));
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
            let mut criterion = build_space_criterion(bench_args);
            let mut group = criterion.benchmark_group(&group_name);
            for rel in &relations {
                let rel_name = rel.header().name().to_string();
                let function = format!("space/{}", rel_name);
                criterion_groups.push(add_space_bench(&mut group, &group_name, function, *rel));
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
                serde_json::json!(optimiser.axis_value()),
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
    }

    Ok(reports)
}

/// Runs one `bench run` cell by monomorphising [`run_benchmark`] over the
/// [`ExecutionFamily`] the cell names. There is no separate algorithm
/// parameter to ignore: the `Execution` fixes both halves of the pair.
pub(crate) fn dispatch_run_bench(
    cell: Execution, workload: &Workload, settings: RunSettings<'_>,
) -> anyhow::Result<Vec<BenchReport>> {
    let optimiser = settings.optimiser;
    match cell {
        | Execution::TrieLftj(SortedTrie::TreeTrie) => run_benchmark(
            &TrieLftj::<kermit_ds::TreeTrie>::new(optimiser),
            workload,
            settings,
        ),
        | Execution::TrieLftj(SortedTrie::ColumnTrie) => run_benchmark(
            &TrieLftj::<kermit_ds::ColumnTrie>::new(optimiser),
            workload,
            settings,
        ),
        | Execution::HashHtj {
            hasher,
            pruning,
            config,
        } => with_hash_trie_layout!(hasher, pruning, |H, P| run_benchmark(
            &HashHtj::<H, P>::new(config, optimiser),
            workload,
            settings,
        )),
    }
}

/// Expands the `-i` / `-a` selectors into the valid execution cells.
///
/// Incompatible concrete pairs are dropped: with `all` on either side they
/// are announced on stderr and skipped, so `-i all -a all` runs exactly the
/// three valid cells; when the user named a single incompatible pair there
/// is nothing left to run and that is a usage error.
pub(crate) fn resolve_sweep(
    indexstructure: IndexStructureSelector, algorithm: JoinAlgorithmSelector, hasher: HasherChoice,
    pruning: PruningChoice, config: HashTrieConfig,
) -> anyhow::Result<Vec<Execution>> {
    let sweep = Sweep::expand(
        &indexstructure.expand(),
        &algorithm.expand(),
        hasher,
        pruning,
        config,
    );
    if sweep.cells.is_empty() {
        anyhow::bail!(
            "incompatible CLI selection: --indexstructure {indexstructure:?} cannot be joined \
             with --algorithm {algorithm:?} (hash-trie pairs with hash-triejoin; sorted tries \
             pair with leapfrog-triejoin)"
        );
    }
    for (ds, algo) in &sweep.skipped {
        eprintln!("bench run: skipping incompatible pair ({ds:?}, {algo:?})");
    }
    Ok(sweep.cells)
}
