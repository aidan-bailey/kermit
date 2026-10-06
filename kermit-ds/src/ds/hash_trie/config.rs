//! Runtime configuration values for [`HashTrie`](super::HashTrie) — the
//! Config category of the optimization standard
//! (`docs/specs/optimization-standard.md`).

use {
    super::hash_table::{log2_capacity_for, INITIAL_LOG2_CAPACITY},
    kermit_iters::ConfigOption,
    serde_json::Value,
    std::{fmt, str::FromStr},
};

/// Maximum table occupancy before a level's hash table doubles, as an exact
/// percentage so the resize test stays integer arithmetic:
/// `(len + 1) * 100 > capacity * percent`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LoadFactor {
    percent: u8,
}

/// A load factor outside the open unit interval.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidLoadFactor(pub u8);

impl fmt::Display for InvalidLoadFactor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "load factor must be between 1% and 99%, got {}%", self.0)
    }
}

impl std::error::Error for InvalidLoadFactor {}

impl LoadFactor {
    /// The default cap as a constant, for `const` contexts where
    /// [`Default::default`] is unavailable.
    pub const DEFAULT: Self = Self {
        percent: Self::DEFAULT_PERCENT,
    };
    /// The pre-Config constant: 70 %.
    pub const DEFAULT_PERCENT: u8 = 70;

    /// A cap of `percent` %, which must lie in `1..=99`.
    pub fn percent(percent: u8) -> Result<Self, InvalidLoadFactor> {
        if (1..=99).contains(&percent) {
            Ok(Self {
                percent,
            })
        } else {
            Err(InvalidLoadFactor(percent))
        }
    }

    /// The cap as a fraction `numerator / denominator`.
    pub fn numerator(self) -> usize { usize::from(self.percent) }

    /// The denominator of that fraction — always 100, since the cap is an
    /// exact percentage.
    pub fn denominator(self) -> usize { 100 }

    /// The cap as the decimal the bench axis reports.
    pub fn as_f64(self) -> f64 { f64::from(self.percent) / 100.0 }
}

impl Default for LoadFactor {
    fn default() -> Self { Self::DEFAULT }
}

/// How large a build makes the root table (#88).
///
/// A value, not a shape: it replaces the root's starting capacity, a
/// constant on the path every build already takes, and is read once per
/// trie. Child tables are unaffected.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RootCapacity {
    /// Start at 4 buckets and double as keys arrive: the only behaviour
    /// before #88.
    #[default]
    Grow,
    /// Size the root once, from the number of tuples the constructor is
    /// given, so that it never grows during that build: Algorithm 2,
    /// line 3 of the paper, applied to the root. A trie created empty is
    /// sized for no tuples, which is 4 buckets.
    Tuples,
}

impl RootCapacity {
    /// The value the bench axis reports and `--ds-config root-capacity=`
    /// parses.
    pub fn axis_value(self) -> &'static str {
        match self {
            | Self::Grow => "grow",
            | Self::Tuples => "tuples",
        }
    }
}

/// A string that names no [`RootCapacity`]. Its message names both values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseRootCapacityError(String);

impl fmt::Display for ParseRootCapacityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "expected grow or tuples, got {:?}", self.0)
    }
}

impl std::error::Error for ParseRootCapacityError {}

/// Parses the strings [`RootCapacity::axis_value`] returns.
impl FromStr for RootCapacity {
    type Err = ParseRootCapacityError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            | "grow" => Ok(Self::Grow),
            | "tuples" => Ok(Self::Tuples),
            | other => Err(ParseRootCapacityError(other.to_owned())),
        }
    }
}

/// Runtime values read by `HashTrie` while it is built.
///
/// A Config is a *value* on a path the code already takes (the resize
/// comparison runs on every insert; the root's capacity is set on every
/// build); replacing a constant with it adds no branch, so non-users pay
/// nothing. See the shape/value/process rule in
/// `docs/specs/optimization-standard.md`.
#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
pub struct HashTrieConfig {
    /// Occupancy cap before a level's table doubles. Bench axis
    /// `ds_config_load_factor` (e.g. `0.7`).
    pub load_factor: LoadFactor,
    /// How large a build makes the root. Bench axis
    /// `ds_config_root_capacity` (`"grow"` or `"tuples"`).
    pub root_capacity: RootCapacity,
}

impl HashTrieConfig {
    /// The root's log2 capacity for a build from `tuple_count` tuples: 4
    /// buckets under [`RootCapacity::Grow`], and under
    /// [`RootCapacity::Tuples`] the smallest capacity at which
    /// `tuple_count` keys never make the root grow
    /// (`hash_table::log2_capacity_for`). Distinct keys cannot outnumber
    /// tuples, so a root built at this capacity never grows during the
    /// build. This is a pure function of the tuple count and the load
    /// factor, so a partitioned build can compute it before partitioning.
    pub(crate) fn root_log2_capacity(self, tuple_count: usize) -> u32 {
        match self.root_capacity {
            | RootCapacity::Grow => INITIAL_LOG2_CAPACITY,
            | RootCapacity::Tuples => log2_capacity_for(tuple_count, self.load_factor),
        }
    }
}

impl ConfigOption for HashTrieConfig {
    fn axes(&self) -> Vec<(&'static str, Value)> {
        vec![
            ("load_factor", Value::from(self.load_factor.as_f64())),
            (
                "root_capacity",
                Value::from(self.root_capacity.axis_value()),
            ),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_load_factor_is_seventy_percent() {
        let lf = HashTrieConfig::default().load_factor;
        assert_eq!(lf, LoadFactor::percent(70).unwrap());
        assert_eq!(lf.numerator(), 70);
        assert_eq!(lf.denominator(), 100);
        assert!((lf.as_f64() - 0.7).abs() < 1e-12);
    }

    #[test]
    fn load_factor_rejects_out_of_range_percentages() {
        assert!(LoadFactor::percent(0).is_err());
        assert!(LoadFactor::percent(100).is_err());
        assert!(LoadFactor::percent(1).is_ok());
        assert!(LoadFactor::percent(99).is_ok());
    }

    #[test]
    fn axes_report_load_factor_as_a_number() {
        let cfg = HashTrieConfig {
            load_factor: LoadFactor::percent(50).unwrap(),
            ..HashTrieConfig::default()
        };
        assert!(cfg.axes().contains(&("load_factor", Value::from(0.5_f64))));
    }

    #[test]
    fn default_root_capacity_is_grow() {
        assert_eq!(HashTrieConfig::default().root_capacity, RootCapacity::Grow);
        assert_eq!(RootCapacity::default(), RootCapacity::Grow);
    }

    /// What a report's `ds_config_root_capacity` says is what
    /// `--ds-config root-capacity=…` parses back.
    #[test]
    fn root_capacity_axis_values_round_trip() {
        assert_eq!(RootCapacity::Grow.axis_value(), "grow");
        assert_eq!(RootCapacity::Tuples.axis_value(), "tuples");
        for value in [RootCapacity::Grow, RootCapacity::Tuples] {
            assert_eq!(value.axis_value().parse::<RootCapacity>(), Ok(value));
        }
    }

    #[test]
    fn malformed_root_capacities_name_both_values() {
        for bad in ["", "Grow", "tuple", "1024", "grow,tuples"] {
            let msg = bad.parse::<RootCapacity>().unwrap_err().to_string();
            assert!(msg.contains("expected grow or tuples"), "{bad:?}: {msg}");
            assert!(msg.contains(&format!("{bad:?}")), "{bad:?}: {msg}");
        }
    }

    #[test]
    fn axes_report_root_capacity_as_a_string() {
        let tuples = HashTrieConfig {
            root_capacity: RootCapacity::Tuples,
            ..HashTrieConfig::default()
        };
        assert!(tuples
            .axes()
            .contains(&("root_capacity", Value::from("tuples"))));
        assert!(HashTrieConfig::default()
            .axes()
            .contains(&("root_capacity", Value::from("grow"))));
    }

    #[test]
    fn root_log2_capacity_is_four_buckets_under_grow_and_sized_under_tuples() {
        let grow = HashTrieConfig::default();
        let tuples = HashTrieConfig {
            root_capacity: RootCapacity::Tuples,
            ..grow
        };
        for n in [0, 1, 3, 1_000, 1 << 20] {
            assert_eq!(grow.root_log2_capacity(n), INITIAL_LOG2_CAPACITY);
            assert_eq!(
                tuples.root_log2_capacity(n),
                log2_capacity_for(n, grow.load_factor)
            );
        }
        // 1,000 keys at 70 % need ⌈1000 / 0.7⌉ = 1,429 buckets: 2^11.
        assert_eq!(tuples.root_log2_capacity(1_000), 11);
    }
}
