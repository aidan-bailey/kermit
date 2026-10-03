//! [`ColumnTrieBuildMode`]: how a [`ColumnTrie`](super::ColumnTrie) is built
//! from a known set of tuples — the BuildMode category of the optimization
//! standard (`docs/specs/optimization-standard.md`).

/// How a [`ColumnTrie`](super::ColumnTrie) is built from a known set of
/// tuples. Both modes sort the tuples first and build identical layers —
/// the same arrays and the same capacities — so the mode changes how long
/// the build takes, never the trie it builds (issue #84).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, clap::ValueEnum)]
pub enum ColumnTrieBuildMode {
    /// One `insert` per tuple, in sorted order: the build before issue #84,
    /// kept so its measurements can be reproduced. Each insert scans its
    /// interval from the start, so the build is O(n · a · b), `b` the
    /// average branching factor.
    Incremental,
    /// One pass over the sorted tuples, appending keys layer by layer and
    /// opening a child interval wherever the prefix changes. O(n · a).
    #[default]
    Bulk,
}

impl kermit_iters::BuildMode for ColumnTrieBuildMode {
    fn axis_value(&self) -> String {
        match self {
            | Self::Incremental => "incremental",
            | Self::Bulk => "bulk",
        }
        .to_string()
    }
}

#[cfg(test)]
mod tests {
    use {super::*, clap::ValueEnum, kermit_iters::BuildMode};

    /// Pins `axis_value` to clap's derived value name, so a report's
    /// `ds_build_mode` is always what the user typed after `--ds-build`.
    #[test]
    fn axis_values_match_clap_value_names() {
        for mode in ColumnTrieBuildMode::value_variants() {
            assert_eq!(
                mode.axis_value(),
                mode.to_possible_value().unwrap().get_name()
            );
        }
    }

    /// The labels name every ColumnTrie report's `ds_build_mode`, and
    /// kermit-lab reads a missing axis as `"incremental"`.
    #[test]
    fn axis_values_and_default_are_pinned() {
        assert_eq!(ColumnTrieBuildMode::Incremental.axis_value(), "incremental");
        assert_eq!(ColumnTrieBuildMode::Bulk.axis_value(), "bulk");
        assert_eq!(ColumnTrieBuildMode::default(), ColumnTrieBuildMode::Bulk);
    }
}
