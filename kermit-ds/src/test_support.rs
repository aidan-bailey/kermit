//! Helpers shared by the crate's inline tests.

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
