//! Observation points for other crates' tests, behind the `test-hooks`
//! feature, which only `kermit`'s dev-dependency enables.
//!
//! Every build mode of a structure builds the identical structure (the
//! BuildMode rule), so a test outside this crate cannot tell from the result
//! which build ran. These hooks expose the records the builds already keep
//! for this crate's own tests.

/// Takes the record of every [`TreeTrieBuildMode::Parallel`] build run on
/// the calling thread since the last call, oldest first: each build's thread
/// count and the size of each partition it built. A serial build adds
/// nothing, and neither does a parallel build of no tuples or of nullary
/// ones, which return before partitioning.
///
/// [`TreeTrieBuildMode::Parallel`]: crate::TreeTrieBuildMode::Parallel
pub fn take_tree_trie_parallel_builds() -> Vec<(usize, Vec<usize>)> {
    crate::ds::take_tree_trie_parallel_builds()
}
