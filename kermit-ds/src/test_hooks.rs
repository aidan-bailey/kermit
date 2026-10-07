//! Observation points for other crates' tests, behind the `test-hooks`
//! feature, which only `kermit`'s dev-dependency enables.
//!
//! Every build mode of a structure builds an equivalent structure (the
//! BuildMode rule: the same contents and capacities), so a test outside this
//! crate cannot tell from the result which build ran. These hooks expose the
//! records the builds already keep for this crate's own tests.

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

pub use crate::ds::HashTrieParallelBuild;

/// Takes the record of every [`HashTrieBuildMode::Parallel`] and
/// [`HashTrieBuildMode::Presized`] build run on the calling thread since the
/// last call, oldest first. Each [`HashTrieParallelBuild`] records the
/// build's thread count, the size of each of its partitions (empty ones
/// included), and, for a `presized:N` build, how many tuples it deferred to
/// the calling thread. Every other mode adds nothing, and neither does a
/// parallel build of no tuples, which returns before partitioning.
///
/// [`HashTrieBuildMode::Parallel`]: crate::HashTrieBuildMode::Parallel
/// [`HashTrieBuildMode::Presized`]: crate::HashTrieBuildMode::Presized
pub fn take_hash_trie_parallel_builds() -> Vec<HashTrieParallelBuild> {
    crate::ds::take_hash_trie_parallel_builds()
}
