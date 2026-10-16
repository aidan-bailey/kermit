//! The random numbers behind H2O's data.
//!
//! Three published algorithms, written out here:
//!
//! - `SplitMix64` (Steele, Lea & Flood, "Fast splittable pseudorandom number
//!   generators", OOPSLA 2014), exactly as Vigna's reference `splitmix64.c` has
//!   it. It passes `BigCrush`, and its period of 2⁶⁴ is far beyond the roughly
//!   10¹⁰ draws the largest N needs.
//! - Lemire's nearly divisionless bounded draws (ACM TOMACS 2019): an unbiased
//!   value in `0..n` from one 128-bit multiply, with a rejection step that
//!   almost never runs.
//! - The Fisher-Yates shuffle, in Durstenfeld's form (1964).
//!
//! They are in this repository because a generator's output must be a pure
//! function of its spec (#74), and `rand`'s `StdRng` and its sampling
//! algorithms are not value-stable across versions: a dependency bump would
//! change every relation, and the golden hashes of the tests with it.
//!
//! Key generation gives every (column, step) pair its own stream (see
//! [`stream`]), so no relation's data depends on the order in which the
//! others are made.

/// The increment of `SplitMix64`, the golden-ratio constant of
/// `splitmix64.c`.
const GAMMA: u64 = 0x9e37_79b9_7f4a_7c15;

/// The output function of `SplitMix64`. Each step is a bijection on `u64`, so
/// the whole function is one too.
fn mix(z: u64) -> u64 {
    let z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    let z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// `SplitMix64`, exactly as Vigna's `splitmix64.c`.
#[derive(Debug, Clone)]
pub(crate) struct SplitMix64 {
    /// `splitmix64.c`'s `x`.
    state: u64,
}

impl SplitMix64 {
    /// A generator whose state is `state`, which is `splitmix64.c`'s `x`.
    pub(crate) fn new(state: u64) -> Self {
        Self {
            state,
        }
    }

    /// `splitmix64.c`'s `next()`: add `GAMMA` to the state (wrapping) and
    /// return its image under `mix`.
    pub(crate) fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(GAMMA);
        mix(self.state)
    }

    /// An unbiased value in `0..n`, by Lemire's method. The high 64 bits of
    /// `next * n` (a 128-bit product) are the candidate; it is biased only
    /// when the low 64 bits fall below `2⁶⁴ mod n`, so those draws are
    /// redrawn. The `low < n` test lets the division in the threshold be
    /// skipped almost always.
    ///
    /// # Panics
    ///
    /// Panics if `n` is 0.
    pub(crate) fn bounded(&mut self, n: u64) -> u64 {
        assert!(n > 0, "SplitMix64::bounded: n must be positive");
        let mut product = u128::from(self.next_u64()) * u128::from(n);
        let mut low = product as u64;
        if low < n {
            // 2⁶⁴ mod n, as `(2⁶⁴ - n) mod n`.
            let threshold = n.wrapping_neg() % n;
            while low < threshold {
                product = u128::from(self.next_u64()) * u128::from(n);
                low = product as u64;
            }
        }
        (product >> 64) as u64
    }

    /// Fisher-Yates (Durstenfeld): for `i` from `len - 1` down to 1, swap
    /// item `i` with item `bounded(i + 1)`.
    pub(crate) fn shuffle<T>(&mut self, items: &mut [T]) {
        for i in (1..items.len()).rev() {
            let j = self.bounded(i as u64 + 1) as usize;
            items.swap(i, j);
        }
    }
}

/// H2O's key columns, numbered as the streams number them. Renumbering one
/// changes every relation the generator writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum KeyColumn {
    /// `id1`, the key of `small`.
    Id1 = 0,
    /// `id2`, the key of `medium`.
    Id2 = 1,
    /// `id3`, the key of `big`.
    Id3 = 2,
}

/// The steps of key generation that draw random numbers, numbered as the
/// streams number them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Step {
    /// Shuffling the pool of keys.
    Pool = 0,
    /// Drawing the keys that pad `x`'s column out to N rows.
    Pad = 1,
    /// Shuffling `x`'s column.
    LeftShuffle = 2,
    /// Shuffling the right-hand table.
    RightShuffle = 3,
}

/// The generator of one (column, step) pair under `seed`. Its state is
/// `mix(seed << 32 | (column * 4 + step))`. That input is injective in
/// (seed, column, step) and `mix` is a bijection, so every stream starts at
/// its own point on the 2⁶⁴ cycle, and no relation depends on the order in
/// which the others are generated.
pub(crate) fn stream(seed: u32, column: KeyColumn, step: Step) -> SplitMix64 {
    let id = column as u64 * 4 + step as u64;
    SplitMix64::new(mix((u64::from(seed) << 32) | id))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// All twelve (column, step) pairs, in stream-number order.
    const ALL_STREAMS: [(KeyColumn, Step); 12] = [
        (KeyColumn::Id1, Step::Pool),
        (KeyColumn::Id1, Step::Pad),
        (KeyColumn::Id1, Step::LeftShuffle),
        (KeyColumn::Id1, Step::RightShuffle),
        (KeyColumn::Id2, Step::Pool),
        (KeyColumn::Id2, Step::Pad),
        (KeyColumn::Id2, Step::LeftShuffle),
        (KeyColumn::Id2, Step::RightShuffle),
        (KeyColumn::Id3, Step::Pool),
        (KeyColumn::Id3, Step::Pad),
        (KeyColumn::Id3, Step::LeftShuffle),
        (KeyColumn::Id3, Step::RightShuffle),
    ];

    #[test]
    fn splitmix64_matches_the_reference_implementation() {
        let mut zero = SplitMix64::new(0);
        let got: Vec<u64> = (0..5).map(|_| zero.next_u64()).collect();
        assert_eq!(got, [
            0xe220_a839_7b1d_cdaf,
            0x6e78_9e6a_a1b9_65f4,
            0x06c4_5d18_8009_454f,
            0xf88b_b8a8_724c_81ec,
            0x1b39_896a_51a8_749b,
        ]);
        let mut seeded = SplitMix64::new(1_234_567);
        let got: Vec<u64> = (0..5).map(|_| seeded.next_u64()).collect();
        assert_eq!(got, [
            0x599e_d017_fb08_fc85,
            0x2c73_f084_5854_0fa5,
            0x883e_bce5_a3f2_7c77,
            0x3fbe_f740_e917_7b3f,
            0xe3b8_3467_08cb_5ecd,
        ]);
    }

    #[test]
    fn streams_are_seeded_by_seed_column_and_step() {
        assert_eq!(
            stream(108, KeyColumn::Id3, Step::Pool).next_u64(),
            0x44a7_9af1_687f_f9dc
        );
        assert_eq!(
            stream(108, KeyColumn::Id1, Step::Pad).next_u64(),
            0xa1b0_6f19_77ea_1cb8
        );
        assert_eq!(
            stream(108, KeyColumn::Id2, Step::RightShuffle).next_u64(),
            0xde70_fad6_0a6e_c795
        );
        let firsts: std::collections::HashSet<u64> = ALL_STREAMS
            .iter()
            .map(|&(column, step)| stream(108, column, step).next_u64())
            .collect();
        assert_eq!(firsts.len(), 12, "two (column, step) pairs share a stream");
        assert_ne!(
            stream(109, KeyColumn::Id3, Step::Pool).next_u64(),
            stream(108, KeyColumn::Id3, Step::Pool).next_u64(),
            "the seed must reach the stream"
        );
    }

    #[test]
    fn bounded_matches_the_reference_model() {
        let mut rng = SplitMix64::new(0);
        let got: Vec<u64> = (0..8).map(|_| rng.bounded(10)).collect();
        assert_eq!(got, [8, 4, 0, 9, 1, 3, 1, 7]);
        // n = 2^63 + 1 rejects more than half of the raw draws, so this case
        // runs Lemire's rejection loop. Without the loop the sequence starts
        // 8147104208329303768.
        let n = (1_u64 << 63) + 1;
        let mut rng = SplitMix64::new(0);
        let got: Vec<u64> = (0..8).map(|_| rng.bounded(n)).collect();
        assert_eq!(got, [
            243_808_509_735_772_839,
            8_954_805_688_390_271_222,
            980_875_101_213_047_373,
            1_603_648_013_000_153_456,
            7_116_260_932_800_173_470,
            2_266_080_580_496_311_649,
            8_780_933_256_989_530_195,
            3_656_771_639_923_220_100,
        ]);
    }

    #[test]
    fn bounded_stays_in_range() {
        let bounds = [
            1,
            2,
            3,
            7,
            8,
            9,
            (1_u64 << 32) - 1,
            1 << 32,
            (1 << 32) + 1,
            1 << 63,
            u64::MAX,
        ];
        for n in bounds {
            let mut rng = SplitMix64::new(7);
            for draw in 0..10_000 {
                let value = rng.bounded(n);
                assert!(value < n, "draw {draw} of bounded({n}) gave {value}");
            }
        }
        let mut rng = SplitMix64::new(7);
        assert!(
            (0..100).all(|_| rng.bounded(1) == 0),
            "bounded(1) must be 0"
        );
    }

    #[test]
    fn bounded_is_uniform_on_a_small_range() {
        let mut rng = SplitMix64::new(42);
        let mut counts = [0_u32; 10];
        for _ in 0..100_000 {
            counts[rng.bounded(10) as usize] += 1;
        }
        // The counts of the reference model; pinning them pins the stream.
        assert_eq!(counts, [
            10066, 9876, 10129, 9933, 10060, 10042, 10104, 10155, 9839, 9796
        ]);
        let chi_square: f64 = counts
            .iter()
            .map(|&count| {
                let off = f64::from(count) - 10_000.0;
                off * off / 10_000.0
            })
            .sum();
        // 27.88 is the p = 0.001 point of chi-square with 9 degrees of freedom;
        // the reference model gives 14.8604.
        assert!(chi_square < 27.88, "chi-square {chi_square}");
    }

    #[test]
    #[should_panic(expected = "n must be positive")]
    fn bounded_of_zero_panics() { SplitMix64::new(0).bounded(0); }

    #[test]
    fn shuffle_yields_a_permutation() {
        let original: Vec<u32> = (0..1000).collect();
        let mut items = original.clone();
        SplitMix64::new(3).shuffle(&mut items);
        assert_ne!(
            items, original,
            "a 1000-element shuffle left the order alone"
        );
        items.sort_unstable();
        assert_eq!(items, original);

        let mut empty: [u32; 0] = [];
        SplitMix64::new(3).shuffle(&mut empty);
        let mut one = [5_u32];
        SplitMix64::new(3).shuffle(&mut one);
        assert_eq!(one, [5]);
    }

    #[test]
    fn shuffle_matches_the_reference_model() {
        let mut ten: Vec<u32> = (0..10).collect();
        SplitMix64::new(3).shuffle(&mut ten);
        assert_eq!(ten, [5, 7, 2, 8, 3, 9, 0, 4, 6, 1]);
        let mut twenty: Vec<u32> = (0..20).collect();
        SplitMix64::new(3).shuffle(&mut twenty);
        assert_eq!(twenty, [
            12, 8, 0, 18, 7, 4, 19, 15, 10, 6, 14, 5, 17, 16, 9, 3, 1, 11, 13, 2
        ]);
    }
}
