//! [`TreeTrieBuildMode`]: how a [`TreeTrie`](super::TreeTrie) is built from
//! a known set of tuples — the BuildMode category of the optimization
//! standard (`docs/specs/optimization-standard.md`).

use {
    crate::morsel::Threads,
    std::{fmt, str::FromStr},
};

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

/// A string that names no [`TreeTrieBuildMode`]. Its message names the
/// accepted forms.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseTreeTrieBuildModeError(String);

impl fmt::Display for ParseTreeTrieBuildModeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { f.write_str(&self.0) }
}

impl std::error::Error for ParseTreeTrieBuildModeError {}

/// Parses the strings [`axis_value`](kermit_iters::BuildMode::axis_value)
/// returns: `serial` and `parallel:<threads>`.
impl FromStr for TreeTrieBuildMode {
    type Err = ParseTreeTrieBuildModeError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let error = |why: String| {
            ParseTreeTrieBuildModeError(format!(
                "{why}; expected serial or parallel:<threads>, threads in 1..={}",
                Threads::MAX
            ))
        };
        let out_of_range = |threads: &str| {
            error(format!(
                "parallel threads must be between 1 and {}, got {threads}",
                Threads::MAX
            ))
        };
        match s.split_once(':') {
            | None if s == "serial" => Ok(Self::Serial),
            | None if s == "parallel" => Err(error("parallel needs a thread count".to_owned())),
            | Some(("parallel", threads)) => {
                if threads.is_empty() || !threads.bytes().all(|b| b.is_ascii_digit()) {
                    return Err(error(format!(
                        "parallel threads must be a whole number, got {threads:?}"
                    )));
                }
                // All digits, so a failed parse is a count too large for
                // `usize`: out of range, like any count above the limit.
                threads
                    .parse::<usize>()
                    .ok()
                    .and_then(Threads::new)
                    .map(Self::Parallel)
                    .ok_or_else(|| out_of_range(threads))
            },
            | _ => Err(error(format!("unknown tree-trie build mode {s:?}"))),
        }
    }
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

    fn parallel(n: usize) -> TreeTrieBuildMode {
        TreeTrieBuildMode::Parallel(Threads::new(n).unwrap())
    }

    /// The labels name every TreeTrie report's `ds_build_mode`, and
    /// kermit-lab reads a TreeTrie report without the axis as `"serial"`.
    #[test]
    fn axis_values_and_default_are_pinned() {
        assert_eq!(TreeTrieBuildMode::Serial.axis_value(), "serial");
        assert_eq!(parallel(1).axis_value(), "parallel:1");
        assert_eq!(parallel(16).axis_value(), "parallel:16");
        assert_eq!(TreeTrieBuildMode::default(), TreeTrieBuildMode::Serial);
    }

    /// What a report's `ds_build_mode` says is what `--ds-build tree-trie=…`
    /// parses back, at both ends of the thread range and between.
    #[test]
    fn axis_values_round_trip_through_from_str() {
        for mode in [
            TreeTrieBuildMode::Serial,
            parallel(1),
            parallel(2),
            parallel(8),
            parallel(Threads::MAX),
        ] {
            assert_eq!(mode.axis_value().parse::<TreeTrieBuildMode>(), Ok(mode));
        }
    }

    /// Every rejection names what was wrong and the accepted forms, the
    /// thread limit included.
    #[test]
    fn malformed_modes_are_rejected_with_the_accepted_forms() {
        let cases = [
            ("", "unknown tree-trie build mode \"\""),
            ("bulk", "unknown tree-trie build mode \"bulk\""),
            ("radix:8", "unknown tree-trie build mode \"radix:8\""),
            ("Serial", "unknown tree-trie build mode \"Serial\""),
            ("parallel", "parallel needs a thread count"),
            ("parallel:", "whole number"),
            ("parallel:x", "whole number"),
            ("parallel:-1", "whole number"),
            ("parallel:2:1", "whole number"),
            ("parallel:0", "between 1 and 1024, got 0"),
            ("parallel:1025", "between 1 and 1024, got 1025"),
            ("parallel:99999999999999999999999", "between 1 and 1024"),
        ];
        for (input, why) in cases {
            let msg = input.parse::<TreeTrieBuildMode>().unwrap_err().to_string();
            assert!(msg.contains(why), "{input:?}: {msg}");
            assert!(
                msg.contains("expected serial or parallel:<threads>, threads in 1..=1024"),
                "{input:?}: {msg}"
            );
        }
    }
}
