//! [`HashTrieBuildMode`]: how a [`HashTrie`](super::HashTrie) is built from a
//! known set of tuples — the BuildMode category of the optimization standard
//! (`docs/specs/optimization-standard.md`).

use {
    crate::morsel::Threads,
    std::{fmt, str::FromStr},
};

/// How a [`HashTrie`](super::HashTrie) is built from a known set of tuples.
/// Every mode builds an equivalent trie, the same contents with the same
/// capacities (the BuildMode rule). `Bulk`, `Incremental`, `Radix` and
/// `Parallel` also put each key in the same bucket; `Presized` may place root
/// keys in other buckets (Amendment 2). The mode changes how long the build
/// takes, never the trie's contents (issues #91, #94, #107).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum HashTrieBuildMode {
    /// Algorithm 2 of the paper (VLDB 2020, §3.2.2): a table's tuples are
    /// grouped into its buckets, then each bucket's child is built from its
    /// list (`bulk.rs`, #107). The default.
    #[default]
    Bulk,
    /// One `insert_at` per tuple, in input order: the only build before
    /// #91, and the default, spelled `serial`, until #107.
    Incremental,
    /// Radix-partition the tuples on the top bits of their first
    /// attribute's hash, build each partition separately, then merge
    /// (SIGMOD 2020 §3.3.2).
    Radix(RadixBits),
    /// The radix build's partition and build steps on this many threads,
    /// the calling thread included: the morsel-driven parallel build
    /// (`docs/data-structures/parallel-build.md`, issue #94). The exact
    /// merge build under every root capacity: a presized root changes the
    /// root's size, never the process.
    Parallel(Threads),
    /// The partitioned build of a presized root on this many threads: the
    /// input partitioned by its first attribute's hash, as the paper does
    /// (VLDB 2020 §3.3.2); each worker groups its run of the root's regions,
    /// a tail of deferred tuples goes to the calling thread, then each worker
    /// builds its run's children (kermit's scheme, `parallel.rs`). The root
    /// must be sized before any tuple arrives, so this mode **requires
    /// `root-capacity=tuples`**
    /// (`docs/specs/2026-10-07-dependent-optimisations-design.md`);
    /// the constructor panics without it, and the CLI rejects it first.
    Presized(Threads),
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
/// returns: `bulk`, `incremental`, `radix:<bits>`, `parallel:<threads>` and
/// `presized:<threads>`.
impl FromStr for HashTrieBuildMode {
    type Err = ParseHashTrieBuildModeError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let error = |why: String| {
            ParseHashTrieBuildModeError(format!(
                "{why}; expected bulk, incremental, radix:<bits>, parallel:<threads> or \
                 presized:<threads>; bits in {}..={}, threads in 1..={}",
                RadixBits::MIN,
                RadixBits::MAX,
                Threads::MAX
            ))
        };
        match s.split_once(':') {
            | None if s == "bulk" => Ok(Self::Bulk),
            | None if s == "incremental" => Ok(Self::Incremental),
            | None if s == "serial" => Err(error(
                "serial was renamed incremental in #107 (the per-tuple build); the default is now \
                 bulk (Algorithm 2)"
                    .to_owned(),
            )),
            | None if s == "radix" => Err(error("radix needs a bit count".to_owned())),
            | None if s == "parallel" => Err(error("parallel needs a thread count".to_owned())),
            | None if s == "presized" => Err(error("presized needs a thread count".to_owned())),
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
            | Some(("parallel", threads)) => {
                parse_threads("parallel", threads, &error).map(Self::Parallel)
            },
            | Some(("presized", threads)) => {
                parse_threads("presized", threads, &error).map(Self::Presized)
            },
            | _ => Err(error(format!("unknown hash-trie build mode {s:?}"))),
        }
    }
}

/// The thread count of `<mode>:<threads>`, for the threaded modes:
/// TreeTrie's rule (`tree_trie/build_mode.rs`), so every threaded mode
/// accepts and rejects the same thread counts. `error` adds the accepted
/// forms to a message.
fn parse_threads(
    mode: &str, threads: &str, error: &impl Fn(String) -> ParseHashTrieBuildModeError,
) -> Result<Threads, ParseHashTrieBuildModeError> {
    if threads.is_empty() || !threads.bytes().all(|b| b.is_ascii_digit()) {
        return Err(error(format!(
            "{mode} threads must be a whole number, got {threads:?}"
        )));
    }
    // All digits, so a failed parse is a count too large for `usize`: out
    // of range, like any count above the limit.
    threads
        .parse::<usize>()
        .ok()
        .and_then(Threads::new)
        .ok_or_else(|| {
            error(format!(
                "{mode} threads must be between 1 and {}, got {threads}",
                Threads::MAX
            ))
        })
}

impl kermit_iters::BuildMode for HashTrieBuildMode {
    fn axis_value(&self) -> String {
        match self {
            | Self::Bulk => "bulk".to_owned(),
            | Self::Incremental => "incremental".to_owned(),
            | Self::Radix(bits) => format!("radix:{}", bits.get()),
            | Self::Parallel(threads) => format!("parallel:{}", threads.get()),
            | Self::Presized(threads) => format!("presized:{}", threads.get()),
        }
    }
}

#[cfg(test)]
mod tests {
    use {super::*, kermit_iters::BuildMode};

    fn radix(bits: u8) -> HashTrieBuildMode {
        HashTrieBuildMode::Radix(RadixBits::new(bits).unwrap())
    }

    fn parallel(n: usize) -> HashTrieBuildMode {
        HashTrieBuildMode::Parallel(Threads::new(n).unwrap())
    }

    fn presized(n: usize) -> HashTrieBuildMode {
        HashTrieBuildMode::Presized(Threads::new(n).unwrap())
    }

    /// What a report's `ds_build_mode` says is what `--ds-build hash-trie=…`
    /// parses back, for every mode.
    #[test]
    fn axis_values_round_trip_through_from_str() {
        let mut modes = vec![HashTrieBuildMode::Bulk, HashTrieBuildMode::Incremental];
        modes.extend((RadixBits::MIN..=RadixBits::MAX).map(radix));
        modes.extend((1..=8).map(parallel));
        modes.push(parallel(Threads::MAX));
        modes.extend((1..=8).map(presized));
        modes.push(presized(Threads::MAX));
        for mode in modes {
            assert_eq!(mode.axis_value().parse::<HashTrieBuildMode>(), Ok(mode));
        }
    }

    /// The labels name every HashTrie report's `ds_build_mode`. kermit-lab
    /// reads a missing axis, and the pre-#107 `"serial"`, as `"incremental"`.
    #[test]
    fn axis_values_and_default_are_pinned() {
        assert_eq!(HashTrieBuildMode::Bulk.axis_value(), "bulk");
        assert_eq!(HashTrieBuildMode::Incremental.axis_value(), "incremental");
        assert_eq!(radix(8).axis_value(), "radix:8");
        assert_eq!(
            HashTrieBuildMode::Parallel(Threads::new(8).unwrap()).axis_value(),
            "parallel:8"
        );
        assert_eq!(presized(8).axis_value(), "presized:8");
        assert_eq!(HashTrieBuildMode::default(), HashTrieBuildMode::Bulk);
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
            ("serial", "serial was renamed incremental in #107"),
            ("Bulk", "unknown hash-trie build mode \"Bulk\""),
            ("radix", "radix needs a bit count"),
            ("radix:", "whole number"),
            ("radix:x", "whole number"),
            ("radix:8:1", "whole number"),
            ("radix:0", "between 1 and 16, got 0"),
            ("radix:17", "between 1 and 16, got 17"),
            ("radix:300", "between 1 and 16, got 300"),
            ("parallel", "parallel needs a thread count"),
            ("parallel:", "whole number"),
            ("parallel:x", "whole number"),
            ("parallel:-1", "whole number"),
            ("parallel:2:1", "whole number"),
            ("parallel:0", "between 1 and 1024, got 0"),
            ("parallel:1025", "between 1 and 1024, got 1025"),
            (
                "parallel:99999999999999999999999",
                "between 1 and 1024, got 99999999999999999999999",
            ),
            ("presized", "presized needs a thread count"),
            ("presized:", "whole number"),
            ("presized:x", "whole number"),
            ("presized:0", "between 1 and 1024, got 0"),
            ("presized:1025", "between 1 and 1024, got 1025"),
        ];
        for (input, why) in cases {
            let msg = input.parse::<HashTrieBuildMode>().unwrap_err().to_string();
            assert!(msg.contains(why), "{input:?}: {msg}");
            assert!(
                msg.contains(
                    "expected bulk, incremental, radix:<bits>, parallel:<threads> or \
                     presized:<threads>"
                ),
                "{input:?}: {msg}"
            );
        }
    }

    /// `serial` named the per-tuple build until #107; its rejection names
    /// the build's new name and the new default.
    #[test]
    fn serial_is_rejected_with_its_replacements() {
        let msg = "serial"
            .parse::<HashTrieBuildMode>()
            .unwrap_err()
            .to_string();
        assert!(msg.contains("serial was renamed incremental"), "{msg}");
        assert!(
            msg.contains("the default is now bulk (Algorithm 2)"),
            "{msg}"
        );
    }
}
