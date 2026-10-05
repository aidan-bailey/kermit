//! [`TreeTrieBuildMode`]: how a [`TreeTrie`](super::TreeTrie) is built from
//! a known set of tuples — the BuildMode category of the optimization
//! standard (`docs/specs/optimization-standard.md`).

use crate::morsel::Threads;

/// How a [`TreeTrie`](super::TreeTrie) is built from a known set of tuples.
/// Every mode builds the identical trie, down to each `Vec`'s capacity, so
/// the mode changes how long the build takes, never the trie it builds
/// (issue #94).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TreeTrieBuildMode {
    /// Sort the tuples, then insert them one at a time on the calling
    /// thread: the build `Relation::from_tuples` has always run.
    #[default]
    Serial,
    /// The morsel-driven build on this many threads, the calling thread
    /// included (`docs/data-structures/parallel-build.md`).
    Parallel(Threads),
}

impl kermit_iters::BuildMode for TreeTrieBuildMode {
    fn axis_value(&self) -> String {
        match self {
            | Self::Serial => "serial".to_string(),
            | Self::Parallel(threads) => format!("parallel:{}", threads.get()),
        }
    }
}

#[cfg(test)]
mod tests {
    use {super::*, kermit_iters::BuildMode};

    /// The labels name every TreeTrie report's `ds_build_mode`, and
    /// kermit-lab reads a TreeTrie report without the axis as `"serial"`.
    #[test]
    fn axis_values_and_default_are_pinned() {
        let parallel = |n| TreeTrieBuildMode::Parallel(Threads::new(n).unwrap());
        assert_eq!(TreeTrieBuildMode::Serial.axis_value(), "serial");
        assert_eq!(parallel(1).axis_value(), "parallel:1");
        assert_eq!(parallel(16).axis_value(), "parallel:16");
        assert_eq!(TreeTrieBuildMode::default(), TreeTrieBuildMode::Serial);
    }
}
