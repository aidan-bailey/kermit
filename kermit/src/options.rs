//! CLI option groups for the optimisation axes (`--ds-layout-*`,
//! `--ds-config`) and the single place the `HashTrie` Layout product is
//! monomorphised.

use {
    crate::IndexStructureSelector,
    clap::Args,
    kermit_ds::{HashTrieConfig, LoadFactor},
};

/// CLI-side selector for `--ds-layout-hasher`. Picks the
/// [`HashStrategy`](kermit_iters::HashStrategy) compile-time parameter
/// monomorphised into `HashTrie<H>` for the run.
///
/// `Sip` (the default) preserves pre-Phase-1 behaviour — `HashTrie`
/// previously had `SipHashStrategy` baked in via a generic default. `Fxhash`
/// monomorphises against the `rustc-hash` `FxHasher`, which is typically
/// ~10x faster per call on small integer keys but lacks SipHash's
/// hash-DoS resistance.
///
/// This flag only applies when the selected index structure is `hash-trie`;
/// `validate_layout_choices` rejects it on other index structures so users
/// cannot silently pass it to a TreeTrie/ColumnTrie run.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum HasherChoice {
    /// SipHash via the standard library's `DefaultHasher`.
    #[default]
    Sip,
    /// FxHash via the `rustc-hash` crate.
    Fxhash,
}

/// CLI-side selector for `--ds-layout-pruning`: the `PruningPolicy`
/// monomorphised into `HashTrie<H, P>`. `Off` is the pre-pruning structure —
/// its `Singleton` node variant and iterator frame are uninhabited, so the
/// instantiation compiles to the code that existed before pruning landed.
///
/// Like [`HasherChoice`], this flag only applies when the selected index
/// structure is `hash-trie`; `validate_layout_choices` rejects it elsewhere.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum PruningChoice {
    /// No pruning (`NoPruning`), the default.
    #[default]
    Off,
    /// Singleton pruning (`SingletonPruning`).
    On,
}

/// Layout-axis CLI choices flattened into every subcommand whose dispatch
/// monomorphises over a `HashTrie<H, P>` (currently `bench Ds` and `bench
/// Run`). Each field is named `<axis>` and surfaces as the long flag
/// `--ds-layout-<axis>` so the prefix matches the bench-report axis namespace
/// described in CLAUDE.md → "JSON bench reports".
///
/// Every field is an `Option<…>` rather than a clap-defaulted value so we
/// can distinguish "not provided" from "explicitly defaulted". The
/// `*_explicit` accessors consult that for the `validate_layout_choices`
/// checks that reject e.g. `--ds-layout-hasher fxhash -i tree-trie` or
/// `--ds-layout-pruning on -i tree-trie`, while the `*_resolved` accessors
/// supply the default at dispatch time.
#[derive(Args, Clone, Debug, Default)]
pub(crate) struct LayoutChoices {
    /// Hash function used by `HashTrie<H, P>` (default: `sip`). Only valid
    /// when `--indexstructure hash-trie` is selected.
    #[arg(long = "ds-layout-hasher", value_name = "HASHER", value_enum)]
    hash_trie_hasher: Option<HasherChoice>,
    /// Singleton pruning Layout of `HashTrie<H, P>` (default: `off`). Only
    /// valid when `--indexstructure hash-trie` is selected.
    #[arg(long = "ds-layout-pruning", value_name = "PRUNING", value_enum)]
    hash_trie_pruning: Option<PruningChoice>,
}

impl LayoutChoices {
    /// Returns the `HasherChoice` to monomorphise on, applying the
    /// `HasherChoice::default()` when none was supplied on the command
    /// line. Use this at dispatch sites.
    pub(crate) fn hash_trie_hasher_resolved(&self) -> HasherChoice {
        self.hash_trie_hasher.unwrap_or_default()
    }

    /// Returns whether the user explicitly passed `--ds-layout-hasher`.
    /// Use this in `validate_layout_choices` to reject the flag on
    /// non-HashTrie selectors.
    pub(crate) fn hash_trie_hasher_explicit(&self) -> bool { self.hash_trie_hasher.is_some() }

    /// Returns the `PruningChoice` to monomorphise on, applying the
    /// `PruningChoice::default()` when none was supplied on the command
    /// line. Use this at dispatch sites.
    pub(crate) fn hash_trie_pruning_resolved(&self) -> PruningChoice {
        self.hash_trie_pruning.unwrap_or_default()
    }

    /// Returns whether the user explicitly passed `--ds-layout-pruning`.
    pub(crate) fn hash_trie_pruning_explicit(&self) -> bool { self.hash_trie_pruning.is_some() }
}

/// Rejects `LayoutChoices` flags that are incompatible with the chosen
/// `IndexStructureSelector`. Both layout flags (`--ds-layout-hasher` and
/// `--ds-layout-pruning`) are meaningful only for `hash-trie` (and for
/// `all`, where the HashTrie sweep arm picks them up). Passing one on a
/// non-HashTrie selector is a usage error: the flag would be silently
/// ignored, producing a benchmark report whose `ds_layout_*` axis
/// disagrees with the actual structure used.
pub(crate) fn validate_layout_choices(
    indexstructure: IndexStructureSelector, layout: &LayoutChoices,
) -> anyhow::Result<()> {
    let applies = matches!(
        indexstructure,
        IndexStructureSelector::HashTrie | IndexStructureSelector::All
    );
    let explicit: &[(&str, bool)] = &[
        ("--ds-layout-hasher", layout.hash_trie_hasher_explicit()),
        ("--ds-layout-pruning", layout.hash_trie_pruning_explicit()),
    ];
    for (flag, given) in explicit {
        if *given && !applies {
            anyhow::bail!(
                "{flag} is only valid with --indexstructure hash-trie (or all); got \
                 --indexstructure {indexstructure:?}"
            );
        }
    }
    Ok(())
}

/// Monomorphises `$body` over the `HashTrie` Layout cell selected at
/// runtime.
///
/// Both dispatchers (`dispatch_ds_bench` and `dispatch_run_bench`, in
/// `main.rs`) go
/// through here, so the Layout product is written out once rather than
/// twice. It is not free of the product, though: the arms *are* the cells,
/// so a third Layout dimension doubles them (2^n in general) and also costs
/// a `LayoutChoices` field plus one entry in each of `Execution::HashHtj`,
/// `Execution::for_pair`, `Sweep::expand`, `HashHtj`, and the two command
/// entry points (`run_ds_bench_command`, `run_bench_run_command`). Before a
/// fourth dimension, reach for a nested macro that expands one dimension at
/// a time, or a builder — not another hand-written 16-arm match.
///
/// Hygiene contract: the identifiers named in the closure-like pattern
/// become *type aliases* scoped to the whole arm, so `$body` must not need
/// a different type of either name.
macro_rules! with_hash_trie_layout {
    ($hasher:expr, $pruning:expr, | $H:ident, $P:ident | $body:expr) => {
        match ($hasher, $pruning) {
            | (
                $crate::options::HasherChoice::Sip,
                $crate::options::PruningChoice::Off,
            ) => {
                type $H = kermit_iters::SipHashStrategy;
                type $P = kermit_ds::NoPruning;
                $body
            },
            | (
                $crate::options::HasherChoice::Sip,
                $crate::options::PruningChoice::On,
            ) => {
                type $H = kermit_iters::SipHashStrategy;
                type $P = kermit_ds::SingletonPruning;
                $body
            },
            | (
                $crate::options::HasherChoice::Fxhash,
                $crate::options::PruningChoice::Off,
            ) => {
                type $H = kermit_iters::FxHashStrategy;
                type $P = kermit_ds::NoPruning;
                $body
            },
            | (
                $crate::options::HasherChoice::Fxhash,
                $crate::options::PruningChoice::On,
            ) => {
                type $H = kermit_iters::FxHashStrategy;
                type $P = kermit_ds::SingletonPruning;
                $body
            },
        }
    };
}

pub(crate) use with_hash_trie_layout;

/// Config-axis CLI choices, flattened beside [`LayoutChoices`] into `bench
/// ds` and `bench run`. One flag, `--ds-config`, takes comma-separated
/// `key=value` pairs (the shape prescribed by
/// `docs/specs/optimization-standard.md`); the keys are resolved per index
/// structure by [`hash_trie_config_resolved`](Self::hash_trie_config_resolved).
#[derive(Args, Clone, Debug, Default)]
pub(crate) struct ConfigChoices {
    /// Runtime values for the selected index structure, as `key=value`
    /// pairs. `HashTrie` accepts `load-factor=<decimal in (0, 1)>`. Only
    /// valid with
    /// `--indexstructure hash-trie` (or `all`).
    #[arg(
        long = "ds-config",
        value_name = "KEY=VALUE,...",
        value_delimiter = ','
    )]
    ds_config: Vec<String>,
}

impl ConfigChoices {
    /// The `--ds-config` keys `HashTrie` accepts, named in the usage error
    /// raised for any other key.
    pub(crate) const HASH_TRIE_KEYS: &'static [&'static str] = &["load-factor"];

    /// Whether the user passed any `--ds-config` pair.
    pub(crate) fn explicit(&self) -> bool { !self.ds_config.is_empty() }

    /// Resolves the pairs into a [`HashTrieConfig`], starting from the
    /// default. Unknown keys, repeated keys and malformed values are
    /// usage errors.
    pub(crate) fn hash_trie_config_resolved(&self) -> anyhow::Result<HashTrieConfig> {
        let mut config = HashTrieConfig::default();
        let mut seen: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
        for pair in &self.ds_config {
            let (key, value) = pair.split_once('=').ok_or_else(|| {
                anyhow::anyhow!("--ds-config expects key=value pairs; got {pair:?}")
            })?;
            if !seen.insert(key) {
                anyhow::bail!("--ds-config: key {key:?} given more than once");
            }
            // keep in sync with HASH_TRIE_KEYS
            match key {
                | "load-factor" => {
                    config.load_factor = parse_load_factor(value)
                        .map_err(|why| anyhow::anyhow!("--ds-config {key}: {why}"))?;
                },
                | other => anyhow::bail!(
                    "--ds-config: unknown key {other:?} for hash-trie; accepted keys: {}",
                    Self::HASH_TRIE_KEYS.join(", ")
                ),
            }
        }
        Ok(config)
    }
}

/// Parses `--ds-config load-factor=<decimal>`: a value in (0, 1) with at
/// most two decimal places, mapped onto `LoadFactor`'s exact percent.
pub(crate) fn parse_load_factor(value: &str) -> Result<LoadFactor, String> {
    let v: f64 = value
        .parse()
        .map_err(|_| format!("expected a decimal in (0, 1), got {value:?}"))?;
    if !v.is_finite() || v <= 0.0 || v >= 1.0 {
        return Err(format!("expected a decimal in (0, 1), got {value:?}"));
    }
    let scaled = v * 100.0;
    let percent = scaled.round();
    if (scaled - percent).abs() > 1e-9 {
        return Err(format!(
            "at most two decimal places are supported, got {value:?}"
        ));
    }
    // `percent` is 1..=99 for every value a user would type; the
    // constructor stays the single source of the range and catches the
    // float edge case (the largest double below 1 rounds to 100).
    LoadFactor::percent(percent as u8).map_err(|e| e.to_string())
}

/// Rejects `--ds-config` on index structures that have no Config axis, so
/// a report can never carry a `ds_config_*` axis the structure ignored.
/// Same discipline as [`validate_layout_choices`].
pub(crate) fn validate_config_choices(
    indexstructure: IndexStructureSelector, config: &ConfigChoices,
) -> anyhow::Result<()> {
    if config.explicit()
        && !matches!(
            indexstructure,
            IndexStructureSelector::HashTrie | IndexStructureSelector::All
        )
    {
        anyhow::bail!(
            "--ds-config is only valid with --indexstructure hash-trie (or all); got \
             --indexstructure {indexstructure:?}"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_layout_choices_accepts_explicit_hasher_on_hash_trie_or_all() {
        // The two selectors whose expand() includes a HashTrie variant
        // (HashTrie itself, and the all-sweep) must both accept an
        // explicit --ds-layout-hasher.
        let layout = LayoutChoices {
            hash_trie_hasher: Some(HasherChoice::Fxhash),
            ..LayoutChoices::default()
        };
        assert!(validate_layout_choices(IndexStructureSelector::HashTrie, &layout).is_ok());
        assert!(validate_layout_choices(IndexStructureSelector::All, &layout).is_ok());
    }

    #[test]
    fn validate_layout_choices_rejects_explicit_hasher_on_non_hash_trie() {
        // Passing `--ds-layout-hasher` on a sorted-family structure is a
        // usage error: the flag would be silently ignored, producing a
        // report whose `ds_layout_hasher` axis disagrees with reality.
        let layout = LayoutChoices {
            hash_trie_hasher: Some(HasherChoice::Fxhash),
            ..LayoutChoices::default()
        };
        for sel in [
            IndexStructureSelector::TreeTrie,
            IndexStructureSelector::ColumnTrie,
        ] {
            let err = validate_layout_choices(sel, &layout).unwrap_err();
            let msg = err.to_string();
            assert!(
                msg.contains("--ds-layout-hasher"),
                "error message should mention the flag for {sel:?}, got: {msg}"
            );
            assert!(
                msg.contains("hash-trie"),
                "error message should suggest the compatible selector for {sel:?}, got: {msg}"
            );
        }
    }

    #[test]
    fn validate_layout_choices_rejects_explicit_pruning_on_non_hash_trie() {
        // Same discipline as the hasher flag: a `ds_layout_pruning` axis
        // must never describe a structure that has no such Layout.
        let layout = LayoutChoices {
            hash_trie_pruning: Some(PruningChoice::On),
            ..LayoutChoices::default()
        };
        assert!(validate_layout_choices(IndexStructureSelector::HashTrie, &layout).is_ok());
        assert!(validate_layout_choices(IndexStructureSelector::All, &layout).is_ok());
        for sel in [
            IndexStructureSelector::TreeTrie,
            IndexStructureSelector::ColumnTrie,
        ] {
            let msg = validate_layout_choices(sel, &layout)
                .unwrap_err()
                .to_string();
            assert!(
                msg.contains("--ds-layout-pruning"),
                "error message should mention the flag for {sel:?}, got: {msg}"
            );
            assert!(
                msg.contains("hash-trie"),
                "error message should suggest the compatible selector for {sel:?}, got: {msg}"
            );
        }
    }

    #[test]
    fn pruning_choice_default_is_off() {
        assert_eq!(PruningChoice::default(), PruningChoice::Off);
        assert_eq!(
            LayoutChoices::default().hash_trie_pruning_resolved(),
            PruningChoice::Off
        );
        assert!(!LayoutChoices::default().hash_trie_pruning_explicit());
    }

    #[test]
    fn validate_layout_choices_default_layout_passes_on_any_selector() {
        // No flag provided: validation must always pass regardless of
        // selector. Otherwise users couldn't run TreeTrie/ColumnTrie at
        // all without thinking about layout flags.
        let layout = LayoutChoices::default();
        for sel in [
            IndexStructureSelector::All,
            IndexStructureSelector::TreeTrie,
            IndexStructureSelector::ColumnTrie,
            IndexStructureSelector::HashTrie,
        ] {
            assert!(
                validate_layout_choices(sel, &layout).is_ok(),
                "default LayoutChoices should pass on {sel:?}"
            );
        }
    }

    #[test]
    fn config_choices_parse_load_factor() {
        let half = ConfigChoices {
            ds_config: vec!["load-factor=0.5".into()],
        };
        assert_eq!(
            half.hash_trie_config_resolved().unwrap().load_factor,
            LoadFactor::percent(50).unwrap()
        );
        assert_eq!(
            ConfigChoices::default()
                .hash_trie_config_resolved()
                .unwrap()
                .load_factor,
            LoadFactor::default()
        );
    }

    #[test]
    fn config_choices_reject_bad_load_factors() {
        for bad in ["0", "1", "1.5", "-0.2", "0.555", "abc"] {
            let choices = ConfigChoices {
                ds_config: vec![format!("load-factor={bad}")],
            };
            let msg = choices.hash_trie_config_resolved().unwrap_err().to_string();
            assert!(msg.contains("load-factor"), "{bad}: {msg}");
        }
    }

    #[test]
    fn config_choices_reject_unknown_key_and_bad_value() {
        // `singleton-pruning` is now a Layout flag, so it is exactly the
        // kind of key `--ds-config` must reject.
        let unknown = ConfigChoices {
            ds_config: vec!["singleton-pruning=true".into()],
        };
        let msg = unknown.hash_trie_config_resolved().unwrap_err().to_string();
        assert!(msg.contains("singleton-pruning"), "{msg}");
        assert!(
            msg.contains("load-factor"),
            "should list accepted keys: {msg}"
        );

        let bad = ConfigChoices {
            ds_config: vec!["load-factor=yes".into()],
        };
        let msg = bad.hash_trie_config_resolved().unwrap_err().to_string();
        assert!(msg.contains("yes"), "{msg}");

        let malformed = ConfigChoices {
            ds_config: vec!["load-factor".into()],
        };
        assert!(malformed.hash_trie_config_resolved().is_err());

        let repeated = ConfigChoices {
            ds_config: vec!["load-factor=0.5".into(), "load-factor=0.9".into()],
        };
        let msg = repeated
            .hash_trie_config_resolved()
            .unwrap_err()
            .to_string();
        assert!(msg.contains("more than once"), "{msg}");
    }

    /// Every key the error message advertises must actually resolve, so
    /// `HASH_TRIE_KEYS` cannot drift from the `match` that consumes it.
    #[test]
    fn every_advertised_hash_trie_key_is_accepted() {
        const SAMPLE: &[(&str, &str)] = &[("load-factor", "0.5")];
        assert_eq!(
            SAMPLE.iter().map(|(key, _)| *key).collect::<Vec<_>>(),
            ConfigChoices::HASH_TRIE_KEYS
        );
        for (key, value) in SAMPLE {
            let choices = ConfigChoices {
                ds_config: vec![format!("{key}={value}")],
            };
            assert!(
                choices.hash_trie_config_resolved().is_ok(),
                "advertised key {key:?} was rejected"
            );
        }
    }

    #[test]
    fn validate_config_choices_rejects_flag_on_non_hash_trie() {
        let config = ConfigChoices {
            ds_config: vec!["load-factor=0.5".into()],
        };
        assert!(validate_config_choices(IndexStructureSelector::HashTrie, &config).is_ok());
        assert!(validate_config_choices(IndexStructureSelector::All, &config).is_ok());
        for sel in [
            IndexStructureSelector::TreeTrie,
            IndexStructureSelector::ColumnTrie,
        ] {
            let msg = validate_config_choices(sel, &config)
                .unwrap_err()
                .to_string();
            assert!(msg.contains("--ds-config"), "{msg}");
        }
        assert!(validate_config_choices(
            IndexStructureSelector::TreeTrie,
            &ConfigChoices::default()
        )
        .is_ok());
    }
}
