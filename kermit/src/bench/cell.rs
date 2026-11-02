//! The unit the generic runner measures: a [`Cell`]. `bench run`, `bench join`
//! and `bench probe` run one cell over a prepared workload; this module holds
//! what the runner needs from a cell, and the one cell kermit's own
//! structures use, [`FamilyCell`].

use crate::execution::Execution;

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

#[cfg(test)]
mod tests {
    use {super::*, crate::options::DsChoices, kermit_ds::IndexStructure};

    #[test]
    fn a_kermit_cells_segment_is_its_structure_and_algorithm() {
        for (structure, segment) in [
            (IndexStructure::TreeTrie, "TreeTrie/LeapfrogTriejoin"),
            (IndexStructure::ColumnTrie, "ColumnTrie/LeapfrogTriejoin"),
            (IndexStructure::HashTrie, "HashTrie/HashTriejoin"),
        ] {
            let cell = Execution::for_structure(structure, DsChoices::default());
            assert_eq!(execution_segment(cell), segment);
        }
    }
}
