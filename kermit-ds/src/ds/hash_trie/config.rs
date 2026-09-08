//! Runtime configuration values for [`HashTrie`](super::HashTrie) — the
//! Config category of the optimization standard
//! (`docs/specs/optimization-standard.md`).

use {kermit_iters::ConfigOption, serde_json::Value, std::fmt};

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
    /// The pre-Config constant: 70 %.
    pub const DEFAULT_PERCENT: u8 = 70;
    /// The default cap as a constant, for `const` contexts where
    /// [`Default::default`] is unavailable.
    pub const DEFAULT: Self = Self {
        percent: Self::DEFAULT_PERCENT,
    };

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

/// Runtime values read by `HashTrie` while it is built.
///
/// A Config is a *value* on a path the code already takes (the resize
/// comparison runs on every insert); replacing a constant with it adds no
/// branch, so non-users pay nothing. See the shape/value/process rule in
/// `docs/specs/optimization-standard.md`.
#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
pub struct HashTrieConfig {
    /// Occupancy cap before a level's table doubles. Bench axis
    /// `ds_config_load_factor` (e.g. `0.7`).
    pub load_factor: LoadFactor,
}

impl ConfigOption for HashTrieConfig {
    fn axes(&self) -> Vec<(&'static str, Value)> {
        vec![("load_factor", Value::from(self.load_factor.as_f64()))]
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
        };
        assert!(cfg
            .axes()
            .contains(&("load_factor", Value::from(0.5_f64))));
    }
}
