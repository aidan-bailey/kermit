//! The `bench ds` runner: one index structure over one relation file,
//! generic over the [`RelationFamily`] so all three families share it.

use {
    super::{
        cell::KERMIT_SYSTEM,
        measure::{CriterionSettings, Described, MeasureRequest, RelationSubject},
        Metric,
    },
    crate::{
        bench_report::{format_bytes, write_metadata_block, BenchKind, BenchReport, MetadataLine},
        execution::{
            read_relation, Execution, HashIndexFamily, HashTrieFamily, RelationFamily, SortedTrie,
            SortedTrieFamily,
        },
        options::{
            with_hash_index_layout, with_hash_trie_layout, with_sorted_trie_layout, DsChoices,
        },
    },
    kermit_ds::IndexStructure,
    std::{collections::BTreeMap, fs, io, path::Path},
};

/// `bench ds` measures one relation, which has no join to materialize: it
/// refuses the `materialize` metric before anything is read.
///
/// # Errors
///
/// If `metrics` contains [`Metric::Materialize`].
pub(crate) fn refuse_materialize(metrics: &[Metric]) -> anyhow::Result<()> {
    if metrics.contains(&Metric::Materialize) {
        anyhow::bail!("bench ds has no join, so it cannot measure materialize");
    }
    Ok(())
}

/// Benchmarks one index structure over one relation file.
///
/// Generic over the [`RelationFamily`] so the sorted family
/// (`SortedTrieFamily<R>`) and the hash family (`HashTrieFamily<H, P, E>`)
/// share one body, as `run_benchmark` does for `bench run` (issue #61
/// closed the last hand-mirrored pair). The family supplies the four
/// points where the bodies used to diverge: the relation type to build
/// (`F::Rel`), how to *build* one from a borrowed view of the file's tuples
/// honouring the family's `--ds-config` values and `--ds-build` mode
/// ([`RelationFamily::build_index`]), how to walk its tuples without
/// materialising them (`F::for_each_tuple` — `TrieIteratorWrapper::advance`
/// for sorted tries, `HashTrie::for_each_tuple` for the hash trie, whose
/// iterator yields hashes; `F::scan` counts through it), and its
/// optimization axes. `bench ds` involves no join, which the bound states: a
/// `RelationFamily` has no engine to build or query.
///
/// The file is read once, flat and untimed, and measured as a
/// [`RelationSubject`] through the measure layer: one untimed describe
/// build gives the `tuples` metadata, the optimization axes and the exact
/// `{ds}/space`, and `{ds}/iteration` scans that same relation. Only then,
/// with it dropped, are `{ds}/build` and `{ds}/end_to_end` timed, each
/// building from a view of the one buffer.
fn run_ds_bench<F: RelationFamily>(
    family: &F, relation_path: &Path, metrics: &[Metric], queries_per_build: u32, group_name: &str,
    criterion: &CriterionSettings,
) -> anyhow::Result<BenchReport> {
    // File order is the same whichever structure and Layout ran, so every
    // one of them is fed identical input.
    let (header, tuples) = read_relation(relation_path)?;
    let subject = RelationSubject {
        family,
        header: &header,
        tuples: &tuples,
    };
    let described = Described::new(&subject, false);
    // What the structure stores, which for a set-semantics structure can be
    // fewer than the file's rows; the `tuples` axis has always reported this.
    let tuple_count = F::tuple_count(described.built());

    // This string becomes the report's `data_structure` axis and the
    // `{ds_name}/<metric>` function ids. It is derived from the family's own
    // `Execution`, the value that picked the code path; the label itself is
    // pinned by `IndexStructure::axis_value`.
    let ds_name = family.execution().index_structure().axis_value();
    let relation_bytes = fs::metadata(relation_path).map(|m| m.len()).unwrap_or(0);

    let mut metadata = vec![
        MetadataLine::new("data structure", ds_name),
        MetadataLine::new("relation", relation_path.display()),
        MetadataLine::new("relation size", format_bytes(relation_bytes)),
        MetadataLine::new("tuples", tuple_count),
        MetadataLine::new("arity", header.arity()),
        MetadataLine::new("allocator", crate::ALLOCATOR),
    ];
    if metrics.contains(&Metric::EndToEnd) {
        metadata.push(MetadataLine::new("queries per build", queries_per_build));
    }
    write_metadata_block(&mut io::stderr(), "bench ds metadata", &metadata)?;

    let mut axes = BTreeMap::from([
        ("system".to_string(), serde_json::json!(KERMIT_SYSTEM)),
        ("data_structure".to_string(), serde_json::json!(ds_name)),
        (
            "relation_path".to_string(),
            serde_json::json!(relation_path.display().to_string()),
        ),
        (
            "relation_bytes".to_string(),
            serde_json::json!(relation_bytes),
        ),
        ("tuples".to_string(), serde_json::json!(tuple_count)),
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
    // Standard optimization axes: the relation's Layout / Config dimensions
    // (e.g. `ds_layout_seek` for the sorted tries), then the build mode, which
    // only the family that ran the build can report. The `ds_layout_*` /
    // `ds_config_*` / `ds_build_mode` naming convention (see
    // `kermit_iters::HasOptimizationAxes`) guarantees no collision with the
    // base axes assembled above.
    axes.extend(F::optimization_axes(described.built()));
    axes.extend(family.build_mode_axes());

    let measured = described.measure(&MeasureRequest {
        metrics,
        single_run: false,
        queries_per_build,
        criterion,
        group: group_name,
        function_prefix: Some(ds_name),
    });
    Ok(BenchReport::new(
        BenchKind::Ds,
        &metadata,
        axes,
        measured.criterion_groups,
        measured.values,
    ))
}

/// Runs one `bench ds` measurement by monomorphising [`run_ds_bench`]
/// over the [`RelationFamily`] that [`Execution::for_structure`] names
/// for `ds`. The hash cell's `H` / `P` Layout parameters are picked from
/// the `--ds-layout-hasher` / `--ds-layout-pruning` CLI flags by
/// `with_hash_trie_layout!` — the one place that product is expanded —
/// and the `--ds-config` values and the `--ds-build` mode ride along on the
/// family, so the relation this measures is the one the report's `ds_*` axes
/// describe. The sorted arms go through `with_sorted_trie_layout!` although
/// `bench ds` rejects `--ds-layout-seek`, because that rejection is CLI
/// policy, not a type invariant, and one dispatch shape reads better than a
/// special case.
pub(crate) fn dispatch_ds_bench(
    ds: IndexStructure, choices: DsChoices, relation: &Path, metrics: &[Metric],
    queries_per_build: u32, group_name: &str, criterion: &CriterionSettings,
) -> anyhow::Result<BenchReport> {
    match Execution::for_structure(ds, choices) {
        | Execution::TrieLftj(SortedTrie::TreeTrie {
            seek,
            build,
        }) => with_sorted_trie_layout!(seek, |S| run_ds_bench(
            &SortedTrieFamily::<kermit_ds::TreeTrie<S>>::new(build),
            relation,
            metrics,
            queries_per_build,
            group_name,
            criterion,
        )),
        | Execution::TrieLftj(SortedTrie::ColumnTrie {
            seek,
            build,
        }) => with_sorted_trie_layout!(seek, |S| run_ds_bench(
            &SortedTrieFamily::<kermit_ds::ColumnTrie<S>>::new(build),
            relation,
            metrics,
            queries_per_build,
            group_name,
            criterion,
        )),
        | Execution::HashHtj {
            hasher,
            pruning,
            expansion,
            levels,
            config,
            build,
        } => {
            // `bench ds` refuses `--ds-layout-levels` (its relation has no
            // query, so no join key): every cell here builds `AllLevels`.
            debug_assert_eq!(levels, crate::options::LevelsChoice::All);
            with_hash_trie_layout!(hasher, pruning, expansion, |H, P, E| run_ds_bench(
                &HashTrieFamily::<H, P, E>::new(config, build),
                relation,
                metrics,
                queries_per_build,
                group_name,
                criterion,
            ))
        },
        | Execution::IndexBhj {
            hasher,
        } => with_hash_index_layout!(hasher, |H| run_ds_bench(
            &HashIndexFamily::<H>::new(),
            relation,
            metrics,
            queries_per_build,
            group_name,
            criterion,
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bench_ds_refuses_the_materialize_metric_and_only_that() {
        let message = refuse_materialize(&[Metric::Build, Metric::Materialize])
            .unwrap_err()
            .to_string();
        assert_eq!(
            message,
            "bench ds has no join, so it cannot measure materialize"
        );
        refuse_materialize(&[
            Metric::Build,
            Metric::Iteration,
            Metric::Space,
            Metric::EndToEnd,
        ])
        .unwrap();
    }
}
