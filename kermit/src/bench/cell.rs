//! The unit the generic runner measures: a [`Cell`]. `bench run`, `bench join`
//! and `bench probe` run one cell over a prepared workload, and the runner
//! (`run_benchmark`) asks a cell for exactly what differs between cells: its
//! labels and report axes, its Criterion group segment, the trie-level policy
//! its queries are prepared under, the metrics it supports, and the
//! [`Subject`] it builds, fallibly, for one prepared query. Everything
//! else (validation, `--verify`, `ReportSink`, group naming, the describe step
//! and `measure`) stays one copy in the runner.
//!
//! [`FamilyCell`] is the cell of kermit's own structures: it wraps an
//! [`ExecutionFamily`] and behaves exactly as the runner did when it was
//! generic over the family.

use {
    super::{
        measure::{QuerySubject, Subject},
        Metric,
    },
    crate::{
        bench_report::MetadataLine,
        execution::{Execution, ExecutionFamily},
    },
    kermit::db::{Catalog, Prepared},
    kermit_ds::{Relation, RelationHeader, TrieLevels, TupleView, Tuples},
    std::collections::BTreeMap,
};

/// The `system` axis of every report that kermit's own structures produce:
/// `bench run`, `bench join`, `bench probe` through a [`FamilyCell`], and
/// `bench ds`. An external engine names itself instead (`duckdb`, `nemo`).
pub(crate) const KERMIT_SYSTEM: &str = "kermit";

/// What the per-query loop of `run_benchmark` needs from one cell.
pub(crate) trait Cell {
    /// What one prepared query is measured as on this cell.
    type Subject<'a>: Subject
    where
        Self: 'a;

    /// For stderr notes and metadata lines: `{ds}/{algo}`.
    fn label(&self) -> String;

    /// The cell's lines in the metadata block, after `benchmark` and `query`
    /// and before `allocator`: `data structure` and `algorithm`.
    fn metadata(&self) -> Vec<MetadataLine>;

    /// The report axes this cell sets: `system`, and for a kermit cell
    /// `data_structure`, `algorithm` and the `ds_*` optimization axes. The
    /// runner adds the axes that belong to the query.
    fn axes(&self) -> BTreeMap<String, serde_json::Value>;

    /// The Criterion group's last segments: `{ds}/{algo}`.
    fn group_segment(&self) -> String;

    /// The trie-level policy this cell's queries are prepared under (#116).
    fn trie_levels(&self) -> TrieLevels;

    /// Whether the cell can measure `metric`. The runner requests only the
    /// metrics a cell supports.
    fn supports(&self, metric: Metric) -> bool;

    /// The subject for one prepared query over `catalog`: untimed, fallible,
    /// once per query, before the describe step. An engine cell runs its
    /// preflight here (see `EngineCell`), so that a failure is the cell's
    /// error rather than a panic in a timed body; a kermit cell cannot fail.
    /// `count` is false when the memory guard skips `materialize` for a
    /// non-streaming subject: the preflight then loads and builds only.
    fn subject<'a>(
        &'a self, catalog: &'a Catalog, prepared: &'a Prepared, count: bool,
    ) -> anyhow::Result<Self::Subject<'a>>;

    /// Axes read from the query's subject, merged into that query's report axes
    /// after [`Cell::axes`]: `engine_version` (engines, from `E::version` of
    /// the subject's `Loaded`). Provided: empty, for kermit cells.
    fn subject_axes(&self, _subject: &Self::Subject<'_>) -> BTreeMap<String, serde_json::Value> {
        BTreeMap::new()
    }

    /// Whether `Subject::heap_bytes` gives one figure per `IndexSpec`, so each
    /// `index` metadata line pairs a spec with its bytes. Provided: true.
    /// `EngineCell` sets false and gets no `index` lines.
    const PER_INDEX_BYTES: bool = true;
}

/// The last two segments of a kermit cell's Criterion group, and its label in
/// stderr notes: `{ds}/{algo}`. The strings are pinned by
/// `IndexStructure::axis_value` and `JoinAlgorithm::axis_value`; see the tests
/// there before changing one, since they name the on-disk
/// `target/criterion/{group}` directories.
pub(crate) fn execution_segment(cell: Execution) -> String {
    format!(
        "{}/{}",
        cell.index_structure().axis_value(),
        cell.algorithm().axis_value()
    )
}

/// A kermit structure under its join algorithm: a sorted trie under Leapfrog
/// Triejoin (`TrieLftj<R>`), the hash trie under Hash Triejoin
/// (`HashHtj<H, P, E, L>`) or the hash index under the binary hash join
/// (`IndexBhj<H>`). The report's `data_structure` / `algorithm` axes
/// come from `family.execution()`, the same value that picked the code path,
/// so a report can never name an algorithm it did not run (issue #56).
pub(crate) struct FamilyCell<'f, F: ExecutionFamily>(pub(crate) &'f F);

impl<F: ExecutionFamily> Cell for FamilyCell<'_, F> {
    type Subject<'a>
        = QuerySubject<'a, F>
    where
        Self: 'a;

    fn label(&self) -> String { execution_segment(self.0.execution()) }

    fn metadata(&self) -> Vec<MetadataLine> {
        let execution = self.0.execution();
        vec![
            MetadataLine::new("data structure", execution.index_structure().axis_value()),
            MetadataLine::new("algorithm", execution.algorithm().axis_value()),
        ]
    }

    /// The identity axes, the dimensions the structure emits (the `ds_*`
    /// prefix convention, see `kermit_iters::HasOptimizationAxes`, guarantees
    /// no collision with the axes the runner assembles per query) and the
    /// build mode, which comes from the family because it leaves no trace in
    /// the structure. They are read off one empty relation, since they depend
    /// on the type, not on the data, and a query whose atoms are all constant
    /// checks builds no index to read them from.
    fn axes(&self) -> BTreeMap<String, serde_json::Value> {
        let execution = self.0.execution();
        let empty = Tuples::new(1);
        let probe = self.0.build_index(
            RelationHeader::new_positional("axes", 1),
            TupleView::all(&empty),
        );
        let mut axes = BTreeMap::from([
            ("system".to_string(), serde_json::json!(KERMIT_SYSTEM)),
            (
                "data_structure".to_string(),
                serde_json::json!(execution.index_structure().axis_value()),
            ),
            (
                "algorithm".to_string(),
                serde_json::json!(execution.algorithm().axis_value()),
            ),
        ]);
        axes.extend(F::optimization_axes(&probe));
        axes.extend(self.0.build_mode_axes());
        axes
    }

    fn group_segment(&self) -> String { execution_segment(self.0.execution()) }

    fn trie_levels(&self) -> TrieLevels { <F::Rel as Relation>::TRIE_LEVELS }

    fn supports(&self, _metric: Metric) -> bool { true }

    fn subject<'a>(
        &'a self, catalog: &'a Catalog, prepared: &'a Prepared, _count: bool,
    ) -> anyhow::Result<QuerySubject<'a, F>> {
        Ok(QuerySubject {
            family: self.0,
            catalog,
            prepared,
        })
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::{
            bench::measure::Described,
            execution::{HashHtj, TrieLftj},
            options::DsChoices,
        },
        kermit_algos::{JoinQuery, LexicographicOptimiser, Planner, StatisticsLevel},
        kermit_ds::{
            EagerExpansion, HashTrieBuildMode, HashTrieConfig, IndexStructure, KeyLevels,
            NoPruning, TreeTrie,
        },
        kermit_iters::SipHashStrategy,
    };

    #[test]
    fn a_kermit_cells_segment_is_its_structure_and_algorithm() {
        for (structure, segment) in [
            (IndexStructure::TreeTrie, "TreeTrie/LeapfrogTriejoin"),
            (IndexStructure::ColumnTrie, "ColumnTrie/LeapfrogTriejoin"),
            (IndexStructure::HashTrie, "HashTrie/HashTriejoin"),
            (IndexStructure::HashIndex, "HashIndex/BinaryHashJoin"),
        ] {
            let cell = Execution::for_structure(structure, DsChoices::default());
            assert_eq!(execution_segment(cell), segment);
        }
    }

    /// `edge` (1 → 2, 2 → 3, 1 → 3) and the two-hop query over it, prepared
    /// under `levels`: the only answer is 1 → 2 → 3.
    fn two_hops(levels: TrieLevels) -> (Catalog, Prepared) {
        let mut tuples = Tuples::new(2);
        for row in [[1, 2], [2, 3], [1, 3]] {
            tuples.push(&row);
        }
        let catalog = Catalog::new(
            vec![(RelationHeader::new_positional("edge", 2), tuples)],
            StatisticsLevel::TupleCounts,
        );
        let query: JoinQuery = "Q(X, Z) :- edge(X, Y), edge(Y, Z).".parse().unwrap();
        let planner = Planner::new(Box::new(LexicographicOptimiser));
        let prepared = kermit::db::prepare(&catalog, &query, &planner, levels).unwrap();
        (catalog, prepared)
    }

    #[test]
    fn a_family_cell_names_itself_by_structure_and_algorithm() {
        let family = TrieLftj::<TreeTrie>::new(Default::default());
        let cell = FamilyCell(&family);
        assert_eq!(cell.label(), "TreeTrie/LeapfrogTriejoin");
        assert_eq!(cell.group_segment(), "TreeTrie/LeapfrogTriejoin");
        let lines: Vec<(&str, String)> = cell
            .metadata()
            .into_iter()
            .map(|line| (line.label, line.value))
            .collect();
        assert_eq!(lines, [
            ("data structure", "TreeTrie".to_string()),
            ("algorithm", "LeapfrogTriejoin".to_string()),
        ]);
    }

    #[test]
    fn a_sorted_trie_cells_axes_are_its_identity_and_its_optimization_axes() {
        let family = TrieLftj::<TreeTrie>::new(Default::default());
        let axes = FamilyCell(&family).axes();
        assert_eq!(axes["data_structure"], "TreeTrie");
        assert_eq!(axes["algorithm"], "LeapfrogTriejoin");
        assert_eq!(axes["ds_layout_seek"], "galloping");
        assert_eq!(axes["ds_build_mode"], "serial");
    }

    #[test]
    fn a_hash_trie_cell_prepares_under_its_level_policy_and_says_so_in_its_axes() {
        let family = HashHtj::<SipHashStrategy, NoPruning, EagerExpansion, KeyLevels>::new(
            HashTrieConfig::default(),
            HashTrieBuildMode::default(),
        );
        let cell = FamilyCell(&family);
        assert_eq!(cell.trie_levels(), TrieLevels::JoinKeys);
        let axes = cell.axes();
        assert_eq!(axes["data_structure"], "HashTrie");
        assert_eq!(axes["algorithm"], "HashTriejoin");
        assert_eq!(axes["ds_layout_levels"], "keys");
        assert_eq!(cell.label(), "HashTrie/HashTriejoin");

        let sorted = TrieLftj::<TreeTrie>::new(Default::default());
        assert_eq!(FamilyCell(&sorted).trie_levels(), TrieLevels::All);
    }

    #[test]
    fn every_kermit_cell_sets_the_kermit_system_axis() {
        let sorted = TrieLftj::<TreeTrie>::new(Default::default());
        assert_eq!(FamilyCell(&sorted).axes()["system"], "kermit");
        let hash = HashHtj::<SipHashStrategy, NoPruning, EagerExpansion, kermit_ds::AllLevels>::new(
            HashTrieConfig::default(),
            HashTrieBuildMode::default(),
        );
        assert_eq!(FamilyCell(&hash).axes()["system"], "kermit");
    }

    #[test]
    fn a_family_cell_supports_every_metric_and_never_refuses_a_query() {
        let family = TrieLftj::<TreeTrie>::new(Default::default());
        let cell = FamilyCell(&family);
        for metric in [
            Metric::Build,
            Metric::Iteration,
            Metric::Space,
            Metric::EndToEnd,
            Metric::Materialize,
        ] {
            assert!(cell.supports(metric), "{metric:?}");
        }
        let (catalog, prepared) = two_hops(TrieLevels::All);
        cell.subject(&catalog, &prepared, true).unwrap();
        cell.subject(&catalog, &prepared, false).unwrap();
    }

    #[test]
    fn a_family_cell_keeps_the_provided_subject_axes_and_per_index_bytes() {
        let family = TrieLftj::<TreeTrie>::new(Default::default());
        let cell = FamilyCell(&family);
        let (catalog, prepared) = two_hops(TrieLevels::All);
        let subject = cell.subject(&catalog, &prepared, true).unwrap();
        assert!(cell.subject_axes(&subject).is_empty());
        const {
            assert!(<FamilyCell<'_, TrieLftj<TreeTrie>> as Cell>::PER_INDEX_BYTES);
        }
    }

    #[test]
    fn a_family_cells_subject_builds_the_plans_indexes_and_counts_the_answer() {
        let family = TrieLftj::<TreeTrie>::new(Default::default());
        let cell = FamilyCell(&family);
        let (catalog, prepared) = two_hops(TrieLevels::All);
        let subject = cell.subject(&catalog, &prepared, true).unwrap();
        let described = Described::new(&subject, true);
        assert_eq!(described.rows(), Some(1), "only 1 → 2 → 3");
        assert_eq!(described.bytes().len(), prepared.indexes().len());
    }
}
