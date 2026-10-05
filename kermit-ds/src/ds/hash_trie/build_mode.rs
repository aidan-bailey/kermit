//! [`HashTrieBuildMode`]: how a [`HashTrie`](super::HashTrie) is built from a
//! known set of tuples — the BuildMode category of the optimization standard
//! (`docs/specs/optimization-standard.md`).

use std::{fmt, str::FromStr};

/// How a [`HashTrie`](super::HashTrie) is built from a known set of tuples.
/// Every mode builds the identical trie — the same buckets and the same
/// capacities — so the mode changes how long the build takes, never the
/// trie it builds (issue #91).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum HashTrieBuildMode {
    /// One insert per tuple, in input order: Algorithm 2 of the paper, and
    /// the only build before issue #91.
    #[default]
    Serial,
    /// Radix-partition the tuples on the top bits of their first
    /// attribute's hash, build each partition separately, then merge
    /// (SIGMOD 2020 §3.3.2).
    Radix(RadixBits),
}

/// The radix bits of a `radix` build: `2^bits` partitions, `bits` in
/// `1..=16`, so a tuple's partition number fits a `u16`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RadixBits(u8);

impl RadixBits {
    /// The most bits: 65,536 partitions, numbered in a `u16`.
    pub const MAX: u8 = 16;
    /// The fewest bits: two partitions.
    pub const MIN: u8 = 1;

    /// `bits` radix bits, which must lie in `MIN..=MAX`.
    pub fn new(bits: u8) -> Result<Self, InvalidRadixBits> {
        if (Self::MIN..=Self::MAX).contains(&bits) {
            Ok(Self(bits))
        } else {
            Err(InvalidRadixBits(bits))
        }
    }

    /// The number of bits.
    pub fn get(self) -> u8 { self.0 }
}

/// A radix bit count outside `1..=16`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidRadixBits(pub u8);

impl fmt::Display for InvalidRadixBits {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "radix bits must be between {} and {}, got {}",
            RadixBits::MIN,
            RadixBits::MAX,
            self.0
        )
    }
}

impl std::error::Error for InvalidRadixBits {}

/// A string that names no [`HashTrieBuildMode`]. Its message names the
/// accepted forms.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseHashTrieBuildModeError(String);

impl fmt::Display for ParseHashTrieBuildModeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { f.write_str(&self.0) }
}

impl std::error::Error for ParseHashTrieBuildModeError {}

/// Parses the strings [`axis_value`](kermit_iters::BuildMode::axis_value)
/// returns: `serial` and `radix:<bits>`.
impl FromStr for HashTrieBuildMode {
    type Err = ParseHashTrieBuildModeError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        const EXPECTED: &str = "expected serial or radix:<bits>, bits in 1..=16";
        let error = |why: String| ParseHashTrieBuildModeError(format!("{why}; {EXPECTED}"));
        match s.split_once(':') {
            | None if s == "serial" => Ok(Self::Serial),
            | None if s == "radix" => Err(error("radix needs a bit count".to_owned())),
            | Some(("radix", bits)) => {
                let n: u32 = bits.parse().map_err(|_| {
                    error(format!("radix bits must be a whole number, got {bits:?}"))
                })?;
                u8::try_from(n)
                    .ok()
                    .and_then(|n| RadixBits::new(n).ok())
                    .map(Self::Radix)
                    .ok_or_else(|| {
                        error(format!(
                            "radix bits must be between {} and {}, got {n}",
                            RadixBits::MIN,
                            RadixBits::MAX
                        ))
                    })
            },
            | _ => Err(error(format!("unknown hash-trie build mode {s:?}"))),
        }
    }
}

impl kermit_iters::BuildMode for HashTrieBuildMode {
    fn axis_value(&self) -> String {
        match self {
            | Self::Serial => "serial".to_owned(),
            | Self::Radix(bits) => format!("radix:{}", bits.get()),
        }
    }
}

#[cfg(test)]
mod tests {
    use {super::*, kermit_iters::BuildMode};

    fn radix(bits: u8) -> HashTrieBuildMode {
        HashTrieBuildMode::Radix(RadixBits::new(bits).unwrap())
    }

    /// What a report's `ds_build_mode` says is what `--ds-build hash-trie=…`
    /// parses back, for every mode.
    #[test]
    fn axis_values_round_trip_through_from_str() {
        let mut modes = vec![HashTrieBuildMode::Serial];
        modes.extend((RadixBits::MIN..=RadixBits::MAX).map(radix));
        for mode in modes {
            assert_eq!(mode.axis_value().parse::<HashTrieBuildMode>(), Ok(mode));
        }
    }

    /// The labels name every HashTrie report's `ds_build_mode`, and
    /// kermit-lab reads a missing axis as `"serial"`.
    #[test]
    fn axis_values_and_default_are_pinned() {
        assert_eq!(HashTrieBuildMode::Serial.axis_value(), "serial");
        assert_eq!(radix(8).axis_value(), "radix:8");
        assert_eq!(HashTrieBuildMode::default(), HashTrieBuildMode::Serial);
    }

    #[test]
    fn radix_bits_are_bounded() {
        assert_eq!(RadixBits::new(0), Err(InvalidRadixBits(0)));
        assert_eq!(RadixBits::new(17), Err(InvalidRadixBits(17)));
        assert_eq!(RadixBits::new(1).map(RadixBits::get), Ok(1));
        assert_eq!(RadixBits::new(16).map(RadixBits::get), Ok(16));
    }

    /// Every rejection names what was wrong and the accepted forms.
    #[test]
    fn malformed_modes_are_rejected_with_the_accepted_forms() {
        let cases = [
            ("", "unknown hash-trie build mode \"\""),
            ("bulk", "unknown hash-trie build mode \"bulk\""),
            ("Serial", "unknown hash-trie build mode \"Serial\""),
            ("radix", "radix needs a bit count"),
            ("radix:", "whole number"),
            ("radix:x", "whole number"),
            ("radix:8:1", "whole number"),
            ("radix:0", "between 1 and 16, got 0"),
            ("radix:17", "between 1 and 16, got 17"),
            ("radix:300", "between 1 and 16, got 300"),
        ];
        for (input, why) in cases {
            let msg = input.parse::<HashTrieBuildMode>().unwrap_err().to_string();
            assert!(msg.contains(why), "{input:?}: {msg}");
            assert!(
                msg.contains("expected serial or radix:<bits>"),
                "{input:?}: {msg}"
            );
        }
    }
}
