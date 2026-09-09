//! The `bench ds` runner: one index structure over one relation file,
//! generic over the [`RelationFamily`] so both trait families share it.

use {
    super::{add_space_bench, build_space_criterion, build_time_criterion, Metric},
    crate::{
        bench_report::{
            write_metadata_block, BenchKind, BenchReport, CriterionGroupRef, MetadataLine,
            ReportMetric,
        },
        execution::{Execution, HashTrieFamily, RelationFamily, SortedTrie, SortedTrieFamily},
        measurement,
        options::{with_hash_trie_layout, HasherChoice, PruningChoice},
        BenchArgs,
    },
    kermit_ds::{HashTrieConfig, IndexStructure, Relation},
    std::{collections::BTreeMap, fs, io, path::Path},
};

/// Benchmarks one index structure over one relation file.
///
/// Generic over the [`RelationFamily`] so the sorted family
/// (`SortedTrieFamily<R>`) and the hash family (`HashTrieFamily<H, P>`)
/// share one body, as `run_benchmark` does for `bench run` (issue #61
/// closed the last hand-mirrored pair). The family supplies the four
/// points where the bodies used to diverge: the relation type to load
/// (`F::Rel`), how to *build* one from a `(header, tuples)` snapshot
/// honouring the family's `--ds-config` values
/// ([`RelationFamily::build_relation`], and [`RelationFamily::load`] on
/// top of it), how to recover its tuples (`F::tuples` — `trie_iter()` for
/// sorted tries, `collect_tuples()` for the hash trie, whose iterator
/// yields hashes), and its optimization axes. `bench ds` involves no join,
/// which the bound states: a `RelationFamily` has no engine to build or
/// query.
fn run_ds_bench<F: RelationFamily>(
    family: &F, relation_path: &Path, metrics: &[Metric], queries_per_build: u32, group_name: &str,
    bench_args: &BenchArgs,
) -> anyhow::Result<BenchReport> {
    let relation: F::Rel = family.load(relation_path)?;

    // Read the tuples back off the built relation rather than off the
    // reader: singleton pruning does not change iteration order (a
    // one-tuple subtrie yields the same sequence either way), so the
    // insertion / end-to-end closures below are fed identical input
    // whichever Layout ran.
    let tuples: Vec<Vec<usize>> = F::tuples(&relation);
    let header = relation.header().clone();

    // This string becomes the report's `data_structure` axis and the
    // `{ds_name}/<metric>` Criterion function ids under
    // `target/criterion/{group}/`. It is derived from the family's own
    // `Execution`, the value that picked the code path; the label itself is
    // pinned by `IndexStructure::axis_value`.
    let ds_name = family.execution().index_structure().axis_value();
    let relation_bytes = fs::metadata(relation_path).map(|m| m.len()).unwrap_or(0);

    let mut metadata = vec![
        MetadataLine::new("data structure", ds_name),
        MetadataLine::new("relation", relation_path.display()),
        MetadataLine::new("relation size", measurement::format_bytes(relation_bytes)),
        MetadataLine::new("tuples", tuples.len()),
        MetadataLine::new("arity", header.arity()),
    ];
    if metrics.contains(&Metric::EndToEnd) {
        metadata.push(MetadataLine::new("queries per build", queries_per_build));
    }
    write_metadata_block(&mut io::stderr(), "bench ds metadata", &metadata)?;

    let mut criterion_groups = Vec::new();

    let has_time_metrics = metrics
        .iter()
        .any(|m| matches!(m, Metric::Insertion | Metric::Iteration | Metric::EndToEnd));

    if has_time_metrics {
        let mut criterion = build_time_criterion(bench_args);
        let mut group = criterion.benchmark_group(group_name);

        if metrics.contains(&Metric::Insertion) {
            let insertion_tuples = tuples.clone();
            let insertion_header = header.clone();
            let function = format!("{ds_name}/insertion");
            group.bench_function(&function, |b| {
                b.iter_batched(
                    || (insertion_header.clone(), insertion_tuples.clone()),
                    |(h, t)| family.build_relation(h, t),
                    criterion::BatchSize::SmallInput,
                );
            });
            criterion_groups.push(CriterionGroupRef {
                group: group_name.to_string(),
                function,
                metric: ReportMetric::Time,
            });
        }

        if metrics.contains(&Metric::Iteration) {
            let function = format!("{ds_name}/iteration");
            group.bench_function(&function, |b| {
                b.iter(|| F::tuples(&relation));
            });
            criterion_groups.push(CriterionGroupRef {
                group: group_name.to_string(),
                function,
                metric: ReportMetric::Time,
            });
        }

        if metrics.contains(&Metric::EndToEnd) {
            let e2e_tuples = tuples.clone();
            let e2e_header = header.clone();
            let function = format!("{ds_name}/end_to_end");
            // PerIteration: a fresh build per sample is the point of this
            // metric — batching would amortise away the construction cost
            // whose interaction with traversal is under measurement.
            group.bench_function(&function, |b| {
                b.iter_batched(
                    || (e2e_header.clone(), e2e_tuples.clone()),
                    |(h, t)| {
                        let built = family.build_relation(h, t);
                        for _ in 0..queries_per_build {
                            std::hint::black_box(F::tuples(&built));
                        }
                    },
                    criterion::BatchSize::PerIteration,
                );
            });
            criterion_groups.push(CriterionGroupRef {
                group: group_name.to_string(),
                function,
                metric: ReportMetric::Time,
            });
        }

        group.finish();
        criterion.final_summary();
    }

    if metrics.contains(&Metric::Space) {
        let n = tuples.len();
        let mut criterion = build_space_criterion(bench_args);
        let mut group = criterion.benchmark_group(group_name);
        group.throughput(criterion::Throughput::Elements(n as u64));
        let function = format!("{ds_name}/space");
        criterion_groups.push(add_space_bench(&mut group, group_name, function, &relation));
        group.finish();
        criterion.final_summary();
    }

    let mut axes = BTreeMap::from([
        ("data_structure".to_string(), serde_json::json!(ds_name)),
        (
            "relation_path".to_string(),
            serde_json::json!(relation_path.display().to_string()),
        ),
        (
            "relation_bytes".to_string(),
            serde_json::json!(relation_bytes),
        ),
        ("tuples".to_string(), serde_json::json!(tuples.len())),
        ("arity".to_string(), serde_json::json!(header.arity())),
    ]);
    // Only meaningful when the end-to-end metric ran; omitting it otherwise
    // keeps historical invocations' reports unchanged.
    if metrics.contains(&Metric::EndToEnd) {
        axes.insert(
            "queries_per_build".to_string(),
            serde_json::json!(queries_per_build),
        );
    }
    // Standard optimization axes: merge in dimensions emitted by the DS.
    // The `ds_layout_*` / `ds_config_*` / `ds_build_mode` naming convention
    // (see `kermit_iters::HasOptimizationAxes`) guarantees no collision
    // with the base axes assembled above. Empty for the sorted tries.
    axes.extend(F::optimization_axes(&relation));
    Ok(BenchReport::new(
        BenchKind::Ds,
        &metadata,
        axes,
        criterion_groups,
    ))
}

/// Runs one `bench ds` measurement by monomorphising [`run_ds_bench`]
/// over the [`RelationFamily`] that [`Execution::for_structure`] names
/// for `ds`. The hash cell's `H` / `P` Layout parameters are picked from
/// the `--ds-layout-hasher` / `--ds-layout-pruning` CLI flags by
/// `with_hash_trie_layout!` — the one place that product is expanded —
/// and the `--ds-config` values ride along on the family, so the relation
/// this measures is the one the report's `ds_*` axes describe.
#[allow(clippy::too_many_arguments)]
pub(crate) fn dispatch_ds_bench(
    ds: IndexStructure, hasher: HasherChoice, pruning: PruningChoice, config: HashTrieConfig,
    relation: &Path, metrics: &[Metric], queries_per_build: u32, group_name: &str,
    bench_args: &BenchArgs,
) -> anyhow::Result<BenchReport> {
    match Execution::for_structure(ds, hasher, pruning, config) {
        | Execution::TrieLftj(SortedTrie::TreeTrie) => run_ds_bench(
            &SortedTrieFamily::<kermit_ds::TreeTrie>::new(),
            relation,
            metrics,
            queries_per_build,
            group_name,
            bench_args,
        ),
        | Execution::TrieLftj(SortedTrie::ColumnTrie) => run_ds_bench(
            &SortedTrieFamily::<kermit_ds::ColumnTrie>::new(),
            relation,
            metrics,
            queries_per_build,
            group_name,
            bench_args,
        ),
        | Execution::HashHtj {
            hasher,
            pruning,
            config,
        } => with_hash_trie_layout!(hasher, pruning, |H, P| run_ds_bench(
            &HashTrieFamily::<H, P>::new(config),
            relation,
            metrics,
            queries_per_build,
            group_name,
            bench_args,
        )),
    }
}
