//! Helpers shared by the crate's inline tests.

use {
    crate::seek::{BinarySeek, SeekStrategy},
    kermit_iters::LayoutOption,
    std::cell::RefCell,
};

/// Linear-congruential generator, so the randomised tests need no `rand`
/// dev-dependency. The constants are Knuth's MMIX ones.
pub(crate) struct Lcg(pub(crate) u64);

impl Lcg {
    /// The next pseudo-random value, below `2^31`.
    pub(crate) fn next_usize(&mut self) -> usize {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.0 >> 33) as usize
    }
}

thread_local! {
    /// `remaining.len()` of every `SpySeek` call on this thread, in order.
    static SPY_LENGTHS: RefCell<Vec<usize>> = const { RefCell::new(Vec::new()) };
}

/// A seek strategy that records the length of every slice it is asked to
/// search, then defers to [`BinarySeek`]. It lets a test check that an
/// iterator really calls its strategy, and with only the siblings it has
/// not yet passed: a wiring fault no result can show and only a wall-clock
/// test could otherwise see.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct SpySeek;

impl LayoutOption for SpySeek {
    const NAME: &'static str = "spy";
}

impl SeekStrategy for SpySeek {
    fn partition_point<T, P: FnMut(&T) -> bool>(remaining: &[T], below: P) -> usize {
        SPY_LENGTHS.with(|lengths| lengths.borrow_mut().push(remaining.len()));
        BinarySeek::partition_point(remaining, below)
    }
}

/// Drains the slice lengths [`SpySeek`] has recorded on this thread.
pub(crate) fn take_spy_lengths() -> Vec<usize> { SPY_LENGTHS.with(RefCell::take) }

/// Test-only strategy that forces real hash collisions: `hash(k) = k mod
/// 10`, so keys congruent modulo 10 share a bucket at every level and
/// tuples whose attributes all collide share a leaf chain. Every hash is
/// below 10, so all of them fall in radix partition 0. Twins live in
/// `kermit-ds/tests/hash_trie_tests.rs` and the `kermit-algos` tests; none
/// is shared, so that no crate ships a colliding strategy in its API.
#[derive(Copy, Clone, Default, Debug)]
pub(crate) struct Mod10HashStrategy;

impl LayoutOption for Mod10HashStrategy {
    const NAME: &'static str = "mod10";
}

impl kermit_iters::HashStrategy for Mod10HashStrategy {
    fn hash(key: usize) -> u64 { (key % 10) as u64 }
}
