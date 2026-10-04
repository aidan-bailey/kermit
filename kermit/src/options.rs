//! CLI option groups for the optimisation axes (`--ds-layout-*`,
//! `--ds-config`, `--ds-build`) and the places the Layout products are
//! monomorphised: `with_hash_trie_layout!` and `with_sorted_trie_layout!`.

use {
    crate::IndexStructureSelector,
    clap::Args,
    kermit_ds::{ColumnTrieBuildMode, HashTrieConfig, LoadFactor, PruningPolicy, SeekStrategy},
    kermit_iters::{HashStrategy, LayoutOption},
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

impl HasherChoice {
    /// The choice that monomorphises to the [`HashStrategy`] marker whose
    /// [`LayoutOption::NAME`] is `name`, or `None` if no CLI choice does.
    ///
    /// This is the inverse of the `with_hash_trie_layout!` table, and lets
    /// a monomorphised code path recover its own CLI label from its type
    /// parameter instead of being handed one. The
    /// `hasher_choices_round_trip_through_layout_names` test pins the two
    /// tables against each other.
    pub(crate) fn from_layout_name(name: &str) -> Option<Self> {
        match name {
            | "sip" => Some(Self::Sip),
            | "fxhash" => Some(Self::Fxhash),
            | _ => None,
        }
    }
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

impl PruningChoice {
    /// The choice that monomorphises to the [`PruningPolicy`] marker whose
    /// [`LayoutOption::NAME`] is `name`, or `None` if no CLI choice does.
    /// The counterpart of [`HasherChoice::from_layout_name`].
    pub(crate) fn from_layout_name(name: &str) -> Option<Self> {
        match name {
            | "off" => Some(Self::Off),
            | "on" => Some(Self::On),
            | _ => None,
        }
    }
}

/// The `--ds-layout-hasher` label of the [`HashStrategy`] a code path was
/// monomorphised over, so a report's `ds_layout_hasher` axis is derived
/// from the type that actually ran rather than from a separately threaded
/// value that could disagree with it.
///
/// # Panics
///
/// Panics if `H`'s layout name has no [`HasherChoice`], which is a
/// programming error: a new [`HashStrategy`] marker must be given a CLI
/// choice. Note what the round-trip test does *not* cover — it sweeps
/// `HasherChoice::value_variants()`, so it catches a CLI variant missing
/// from `from_layout_name`'s table, but a marker type with no variant at
/// all is invisible to it and only panics once something instantiates
/// `hasher_of` over it.
pub(crate) fn hasher_of<H: HashStrategy>() -> HasherChoice {
    let name = <H as LayoutOption>::NAME;
    HasherChoice::from_layout_name(name).unwrap_or_else(|| {
        panic!(
            "no --ds-layout-hasher choice for hash strategy {name:?} ({})",
            std::any::type_name::<H>()
        )
    })
}

/// The `--ds-layout-pruning` label of the [`PruningPolicy`] a code path was
/// monomorphised over. The counterpart of [`hasher_of`].
///
/// # Panics
///
/// Panics if `P`'s layout name has no [`PruningChoice`], with the same
/// caveat about what the round-trip test covers; see [`hasher_of`].
pub(crate) fn pruning_of<P: PruningPolicy>() -> PruningChoice {
    let name = <P as LayoutOption>::NAME;
    PruningChoice::from_layout_name(name).unwrap_or_else(|| {
        panic!(
            "no --ds-layout-pruning choice for pruning policy {name:?} ({})",
            std::any::type_name::<P>()
        )
    })
}

/// CLI-side selector for `--ds-layout-seek`: the [`SeekStrategy`]
/// monomorphised into `TreeTrie<S>` / `ColumnTrie<S>`. `Binary`, the
/// default, is the `partition_point` search both sorted tries used before
/// the parameter existed.
///
/// Only valid when the selected index structure is `tree-trie` or
/// `column-trie` (or `all`): `validate_layout_choices` rejects it on
/// `hash-trie`, and `bench ds` rejects it outright, since none of its
/// metrics seeks.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum SeekChoice {
    /// A scan (`LinearSeek`).
    Linear,
    /// A binary search (`BinarySeek`), the default.
    #[default]
    Binary,
    /// A galloping search (`GallopingSeek`).
    Galloping,
}

impl SeekChoice {
    /// The choice that monomorphises to the [`SeekStrategy`] marker whose
    /// [`LayoutOption::NAME`] is `name`, or `None` if no CLI choice does.
    /// The counterpart of [`HasherChoice::from_layout_name`].
    pub(crate) fn from_layout_name(name: &str) -> Option<Self> {
        match name {
            | "linear" => Some(Self::Linear),
            | "binary" => Some(Self::Binary),
            | "galloping" => Some(Self::Galloping),
            | _ => None,
        }
    }
}

/// The `--ds-layout-seek` label of the [`SeekStrategy`] a code path was
/// monomorphised over. The counterpart of [`hasher_of`].
///
/// # Panics
///
/// Panics if `S`'s layout name has no [`SeekChoice`], with the same caveat
/// about what the round-trip test covers; see [`hasher_of`].
pub(crate) fn seek_of<S: SeekStrategy>() -> SeekChoice {
    let name = <S as LayoutOption>::NAME;
    SeekChoice::from_layout_name(name).unwrap_or_else(|| {
        panic!(
            "no --ds-layout-seek choice for seek strategy {name:?} ({})",
            std::any::type_name::<S>()
        )
    })
}

/// Layout-axis CLI choices flattened into every subcommand whose dispatch
/// monomorphises over a Layout-parameterised structure (`HashTrie<H, P>`,
/// `TreeTrie<S>`, `ColumnTrie<S>`). Each field is named `<axis>` and surfaces
/// as the long flag `--ds-layout-<axis>` so the prefix matches the bench-report
/// axis namespace described in CLAUDE.md → "JSON bench reports".
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
    /// Seek strategy of `TreeTrie<S>` / `ColumnTrie<S>` (default:
    /// `binary`). Only valid when `--indexstructure tree-trie` or
    /// `column-trie` (or `all`) is selected, and not on `bench ds`, none of
    /// whose metrics seeks.
    #[arg(long = "ds-layout-seek", value_name = "SEEK", value_enum)]
    sorted_trie_seek: Option<SeekChoice>,
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

    /// Returns the `SeekChoice` to monomorphise on, applying
    /// `SeekChoice::default()` when none was supplied on the command line.
    /// Use this at dispatch sites.
    pub(crate) fn sorted_trie_seek_resolved(&self) -> SeekChoice {
        self.sorted_trie_seek.unwrap_or_default()
    }

    /// Returns whether the user explicitly passed `--ds-layout-seek`.
    pub(crate) fn sorted_trie_seek_explicit(&self) -> bool { self.sorted_trie_seek.is_some() }
}

/// Rejects `LayoutChoices` flags that are incompatible with the chosen
/// `IndexStructureSelector`. Each flag names a Layout of particular
/// structures: `--ds-layout-hasher` and `--ds-layout-pruning` belong to
/// `hash-trie`, and `--ds-layout-seek` to `tree-trie` and `column-trie`.
/// `all` accepts every flag, because its sweep has a cell for each. A flag
/// on a structure without its Layout is a usage error: it would be silently
/// ignored, producing a benchmark report whose `ds_layout_*` axis disagrees
/// with the actual structure used.
pub(crate) fn validate_layout_choices(
    indexstructure: IndexStructureSelector, layout: &LayoutChoices,
) -> anyhow::Result<()> {
    let hash_trie = matches!(
        indexstructure,
        IndexStructureSelector::HashTrie | IndexStructureSelector::All
    );
    let sorted_trie = matches!(
        indexstructure,
        IndexStructureSelector::TreeTrie
            | IndexStructureSelector::ColumnTrie
            | IndexStructureSelector::All
    );
    // (flag, given, applies to the selection, the structures it applies to)
    let flags: &[(&str, bool, bool, &str)] = &[
        (
            "--ds-layout-hasher",
            layout.hash_trie_hasher_explicit(),
            hash_trie,
            "hash-trie",
        ),
        (
            "--ds-layout-pruning",
            layout.hash_trie_pruning_explicit(),
            hash_trie,
            "hash-trie",
        ),
        (
            "--ds-layout-seek",
            layout.sorted_trie_seek_explicit(),
            sorted_trie,
            "tree-trie or column-trie",
        ),
    ];
    for (flag, given, applies, structures) in flags {
        if *given && !applies {
            anyhow::bail!(
                "{flag} is only valid with --indexstructure {structures} (or all); got \
                 --indexstructure {indexstructure:?}"
            );
        }
    }
    Ok(())
}

/// Monomorphises `$body` over the `HashTrie` Layout cell selected at
/// runtime.
///
/// The dispatchers (`dispatch_ds_bench` and `dispatch_run_bench`, in
/// `bench/ds.rs` and `bench/run.rs`) and `load_query_runner` (in `main.rs`)
/// all go through here, so the Layout product is written out once rather
/// than once per caller. It is not free of the product, though: the arms
/// *are* the cells,
/// so a third Layout dimension doubles them (2^n in general) and also costs
/// a `LayoutChoices` field, a `DsChoices` field, an `Execution::HashHtj`
/// field, and a type parameter (with its `*_of::<X>()` label) on `HashHtj`
/// and `HashTrieFamily`. Before a
/// fourth dimension, reach for a nested macro that expands one dimension at
/// a time, or a builder — not another hand-written 16-arm match.
///
/// Hygiene contract: the identifiers named in the closure-like pattern
/// become *type aliases* scoped to the whole arm, so `$body` must not need
/// a different type of either name.
macro_rules! with_hash_trie_layout {
    ($hasher:expr, $pruning:expr, | $H:ident, $P:ident | $body:expr) => {
        match ($hasher, $pruning) {
            | ($crate::options::HasherChoice::Sip, $crate::options::PruningChoice::Off) => {
                type $H = ::kermit_iters::SipHashStrategy;
                type $P = ::kermit_ds::NoPruning;
                $body
            },
            | ($crate::options::HasherChoice::Sip, $crate::options::PruningChoice::On) => {
                type $H = ::kermit_iters::SipHashStrategy;
                type $P = ::kermit_ds::SingletonPruning;
                $body
            },
            | ($crate::options::HasherChoice::Fxhash, $crate::options::PruningChoice::Off) => {
                type $H = ::kermit_iters::FxHashStrategy;
                type $P = ::kermit_ds::NoPruning;
                $body
            },
            | ($crate::options::HasherChoice::Fxhash, $crate::options::PruningChoice::On) => {
                type $H = ::kermit_iters::FxHashStrategy;
                type $P = ::kermit_ds::SingletonPruning;
                $body
            },
        }
    };
}

pub(crate) use with_hash_trie_layout;

/// Monomorphises `$body` over the sorted-trie seek strategy selected at
/// runtime. It is the sorted counterpart of `with_hash_trie_layout!`, and
/// the one place a `--ds-layout-seek` choice becomes a type. The two
/// products share no dimension (a sorted cell has no hasher, and a hash
/// cell no seek), so they are separate macros. Called by the sorted arms of
/// `dispatch_run_bench`, `dispatch_ds_bench` and `load_query_runner`.
///
/// Hygiene contract as for `with_hash_trie_layout!`: `$S` becomes a type
/// alias scoped to the arm.
macro_rules! with_sorted_trie_layout {
    ($seek:expr, | $S:ident | $body:expr) => {
        match $seek {
            | $crate::options::SeekChoice::Linear => {
                type $S = ::kermit_ds::LinearSeek;
                $body
            },
            | $crate::options::SeekChoice::Binary => {
                type $S = ::kermit_ds::BinarySeek;
                $body
            },
            | $crate::options::SeekChoice::Galloping => {
                type $S = ::kermit_ds::GallopingSeek;
                $body
            },
        }
    };
}

pub(crate) use with_sorted_trie_layout;

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

/// BuildMode-axis CLI choice, flattened beside [`LayoutChoices`] and
/// [`ConfigChoices`] into `bench ds`, `bench run` and `bench join`. Every
/// build mode builds the same structure, so the flag changes build time
/// only. `kermit join` takes no `--ds-build`, for the same reason it takes
/// no `--ds-config`: it cannot change a query's answers.
#[derive(Args, Clone, Debug, Default)]
pub(crate) struct BuildChoices {
    /// How `ColumnTrie` is built from its tuples (default: `bulk`;
    /// `incremental` is the build before the one-pass bulk build). Only valid
    /// with `--indexstructure column-trie` (or `all`).
    #[arg(long = "ds-build", value_name = "MODE", value_enum)]
    column_trie_build: Option<ColumnTrieBuildMode>,
}

impl BuildChoices {
    /// The mode to build `ColumnTrie` relations with, applying the default
    /// when none was supplied.
    pub(crate) fn column_trie_build_resolved(&self) -> ColumnTrieBuildMode {
        self.column_trie_build.unwrap_or_default()
    }

    /// Whether the user explicitly passed `--ds-build`.
    pub(crate) fn column_trie_build_explicit(&self) -> bool { self.column_trie_build.is_some() }
}

/// Rejects `--ds-build` on index structures that have no BuildMode axis, so
/// a report can never carry a `ds_build_mode` the build ignored. Same
/// discipline as [`validate_config_choices`].
pub(crate) fn validate_build_choices(
    indexstructure: IndexStructureSelector, build: &BuildChoices,
) -> anyhow::Result<()> {
    if build.column_trie_build_explicit()
        && !matches!(
            indexstructure,
            IndexStructureSelector::ColumnTrie | IndexStructureSelector::All
        )
    {
        anyhow::bail!(
            "--ds-build is only valid with --indexstructure column-trie (or all); got \
             --indexstructure {indexstructure:?}"
        );
    }
    Ok(())
}

/// The resolved value of every `--ds-*` option for one command — what the
/// execution cells are built from. Commands obtain it from
/// [`DsChoices::resolve`], which rejects a flag the selected structure
/// would ignore before applying any default.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct DsChoices {
    /// `--ds-layout-hasher`; reaches the hash-trie cell only.
    pub hasher: HasherChoice,
    /// `--ds-layout-pruning`; reaches the hash-trie cell only.
    pub pruning: PruningChoice,
    /// `--ds-layout-seek`; reaches the two sorted-trie cells.
    pub seek: SeekChoice,
    /// `--ds-config`; reaches the hash-trie cell only.
    pub config: HashTrieConfig,
    /// `--ds-build`; reaches the column-trie cell only.
    pub build: ColumnTrieBuildMode,
}

impl DsChoices {
    /// Validates every `--ds-*` flag against `indexstructure`, then resolves
    /// them, applying each option's default where no flag was given.
    ///
    /// # Errors
    ///
    /// Returns an error if a flag was given for a structure that lacks its
    /// axis, or if `--ds-config` is malformed.
    pub(crate) fn resolve(
        indexstructure: IndexStructureSelector, layout: &LayoutChoices, config: &ConfigChoices,
        build: &BuildChoices,
    ) -> anyhow::Result<Self> {
        validate_layout_choices(indexstructure, layout)?;
        validate_config_choices(indexstructure, config)?;
        validate_build_choices(indexstructure, build)?;
        Ok(Self {
            hasher: layout.hash_trie_hasher_resolved(),
            pruning: layout.hash_trie_pruning_resolved(),
            seek: layout.sorted_trie_seek_resolved(),
            config: config.hash_trie_config_resolved()?,
            build: build.column_trie_build_resolved(),
        })
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        clap::ValueEnum,
        kermit_ds::{BinarySeek, GallopingSeek, LinearSeek, NoPruning, SingletonPruning},
        kermit_iters::{FxHashStrategy, SipHashStrategy},
    };

    /// Every `--ds-layout-hasher` choice names exactly one `HashStrategy`
    /// marker and is recovered from it. The table is explicit so a new
    /// variant fails the `value_variants()` sweep until it is listed here
    /// (and therefore until `from_layout_name` handles it).
    #[test]
    fn hasher_choices_round_trip_through_layout_names() {
        const TABLE: &[(HasherChoice, &str)] = &[
            (HasherChoice::Sip, <SipHashStrategy as LayoutOption>::NAME),
            (HasherChoice::Fxhash, <FxHashStrategy as LayoutOption>::NAME),
        ];
        for choice in HasherChoice::value_variants() {
            let (_, name) = TABLE
                .iter()
                .find(|(listed, _)| listed == choice)
                .unwrap_or_else(|| panic!("{choice:?} is missing from the round-trip table"));
            assert_eq!(HasherChoice::from_layout_name(name), Some(*choice));
        }
        assert_eq!(hasher_of::<SipHashStrategy>(), HasherChoice::Sip);
        assert_eq!(hasher_of::<FxHashStrategy>(), HasherChoice::Fxhash);
    }

    /// Every `with_hash_trie_layout!` arm binds the marker pair its
    /// `(HasherChoice, PruningChoice)` pattern names. Reading the labels
    /// back out of the aliases the macro defines pins the four pairings
    /// against `hasher_of`/`pruning_of`, so a transposed arm fails here
    /// rather than silently mislabelling a bench report.
    #[test]
    fn layout_macro_binds_the_marker_pair_its_arm_names() {
        for (hasher, pruning) in [
            (HasherChoice::Sip, PruningChoice::Off),
            (HasherChoice::Sip, PruningChoice::On),
            (HasherChoice::Fxhash, PruningChoice::Off),
            (HasherChoice::Fxhash, PruningChoice::On),
        ] {
            let bound = with_hash_trie_layout!(hasher, pruning, |H, P| (
                hasher_of::<H>(),
                pruning_of::<P>()
            ));
            assert_eq!(bound, (hasher, pruning));
        }
    }

    /// The same round trip for `--ds-layout-pruning` and `PruningPolicy`.
    #[test]
    fn pruning_choices_round_trip_through_layout_names() {
        const TABLE: &[(PruningChoice, &str)] = &[
            (PruningChoice::Off, <NoPruning as LayoutOption>::NAME),
            (PruningChoice::On, <SingletonPruning as LayoutOption>::NAME),
        ];
        for choice in PruningChoice::value_variants() {
            let (_, name) = TABLE
                .iter()
                .find(|(listed, _)| listed == choice)
                .unwrap_or_else(|| panic!("{choice:?} is missing from the round-trip table"));
            assert_eq!(PruningChoice::from_layout_name(name), Some(*choice));
        }
        assert_eq!(pruning_of::<NoPruning>(), PruningChoice::Off);
        assert_eq!(pruning_of::<SingletonPruning>(), PruningChoice::On);
    }

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

    #[test]
    fn ds_choices_resolve_applies_flags_and_defaults() {
        let layout = LayoutChoices {
            hash_trie_hasher: Some(HasherChoice::Fxhash),
            ..LayoutChoices::default()
        };
        let config = ConfigChoices {
            ds_config: vec!["load-factor=0.5".into()],
        };
        let choices = DsChoices::resolve(
            IndexStructureSelector::HashTrie,
            &layout,
            &config,
            &BuildChoices::default(),
        )
        .unwrap();
        assert_eq!(choices.hasher, HasherChoice::Fxhash);
        assert_eq!(choices.pruning, PruningChoice::Off);
        assert_eq!(choices.config.load_factor, LoadFactor::percent(50).unwrap());
        assert_eq!(
            DsChoices::resolve(
                IndexStructureSelector::TreeTrie,
                &LayoutChoices::default(),
                &ConfigChoices::default(),
                &BuildChoices::default()
            )
            .unwrap(),
            DsChoices::default()
        );
    }

    /// `resolve` validates before it resolves, so no command can act on a
    /// flag the selected structure would ignore.
    #[test]
    fn ds_choices_resolve_rejects_flags_the_structure_lacks() {
        let layout = LayoutChoices {
            hash_trie_pruning: Some(PruningChoice::On),
            ..LayoutChoices::default()
        };
        let msg = DsChoices::resolve(
            IndexStructureSelector::TreeTrie,
            &layout,
            &ConfigChoices::default(),
            &BuildChoices::default(),
        )
        .unwrap_err()
        .to_string();
        assert!(msg.contains("--ds-layout-pruning"), "{msg}");
        let config = ConfigChoices {
            ds_config: vec!["load-factor=0.5".into()],
        };
        let msg = DsChoices::resolve(
            IndexStructureSelector::ColumnTrie,
            &LayoutChoices::default(),
            &config,
            &BuildChoices::default(),
        )
        .unwrap_err()
        .to_string();
        assert!(msg.contains("--ds-config"), "{msg}");
    }

    #[test]
    fn validate_build_choices_accepts_column_trie_or_all_only() {
        let build = BuildChoices {
            column_trie_build: Some(ColumnTrieBuildMode::Incremental),
        };
        assert!(validate_build_choices(IndexStructureSelector::ColumnTrie, &build).is_ok());
        assert!(validate_build_choices(IndexStructureSelector::All, &build).is_ok());
        for sel in [
            IndexStructureSelector::TreeTrie,
            IndexStructureSelector::HashTrie,
        ] {
            let msg = validate_build_choices(sel, &build).unwrap_err().to_string();
            assert!(msg.contains("--ds-build"), "{msg}");
            assert!(msg.contains("column-trie"), "{msg}");
            assert!(validate_build_choices(sel, &BuildChoices::default()).is_ok());
        }
    }

    #[test]
    fn ds_choices_resolve_carries_the_build_mode() {
        let build = BuildChoices {
            column_trie_build: Some(ColumnTrieBuildMode::Incremental),
        };
        let choices = DsChoices::resolve(
            IndexStructureSelector::ColumnTrie,
            &LayoutChoices::default(),
            &ConfigChoices::default(),
            &build,
        )
        .unwrap();
        assert_eq!(choices.build, ColumnTrieBuildMode::Incremental);
        assert_eq!(DsChoices::default().build, ColumnTrieBuildMode::Bulk);
        assert!(DsChoices::resolve(
            IndexStructureSelector::TreeTrie,
            &LayoutChoices::default(),
            &ConfigChoices::default(),
            &build,
        )
        .is_err());
    }

    /// The same round trip for `--ds-layout-seek` and `SeekStrategy`.
    #[test]
    fn seek_choices_round_trip_through_layout_names() {
        const TABLE: &[(SeekChoice, &str)] = &[
            (SeekChoice::Linear, <LinearSeek as LayoutOption>::NAME),
            (SeekChoice::Binary, <BinarySeek as LayoutOption>::NAME),
            (SeekChoice::Galloping, <GallopingSeek as LayoutOption>::NAME),
        ];
        for choice in SeekChoice::value_variants() {
            let (_, name) = TABLE
                .iter()
                .find(|(listed, _)| listed == choice)
                .unwrap_or_else(|| panic!("{choice:?} is missing from the round-trip table"));
            assert_eq!(SeekChoice::from_layout_name(name), Some(*choice));
        }
        assert_eq!(seek_of::<LinearSeek>(), SeekChoice::Linear);
        assert_eq!(seek_of::<BinarySeek>(), SeekChoice::Binary);
        assert_eq!(seek_of::<GallopingSeek>(), SeekChoice::Galloping);
    }

    /// Every `with_sorted_trie_layout!` arm binds the marker its
    /// `SeekChoice` names, so a transposed arm fails here rather than
    /// mislabelling a bench report.
    #[test]
    fn sorted_layout_macro_binds_the_marker_its_arm_names() {
        for &seek in SeekChoice::value_variants() {
            assert_eq!(with_sorted_trie_layout!(seek, |S| seek_of::<S>()), seek);
        }
    }

    #[test]
    fn validate_layout_choices_accepts_seek_on_sorted_tries_or_all_only() {
        let layout = LayoutChoices {
            sorted_trie_seek: Some(SeekChoice::Galloping),
            ..LayoutChoices::default()
        };
        for sel in [
            IndexStructureSelector::TreeTrie,
            IndexStructureSelector::ColumnTrie,
            IndexStructureSelector::All,
        ] {
            assert!(validate_layout_choices(sel, &layout).is_ok(), "{sel:?}");
        }
        let msg = validate_layout_choices(IndexStructureSelector::HashTrie, &layout)
            .unwrap_err()
            .to_string();
        assert!(msg.contains("--ds-layout-seek"), "{msg}");
        assert!(msg.contains("tree-trie or column-trie"), "{msg}");
    }

    #[test]
    fn ds_choices_resolve_carries_the_seek_strategy() {
        let layout = LayoutChoices {
            sorted_trie_seek: Some(SeekChoice::Linear),
            ..LayoutChoices::default()
        };
        let choices = DsChoices::resolve(
            IndexStructureSelector::TreeTrie,
            &layout,
            &ConfigChoices::default(),
            &BuildChoices::default(),
        )
        .unwrap();
        assert_eq!(choices.seek, SeekChoice::Linear);
        assert_eq!(DsChoices::default().seek, SeekChoice::Binary);
        assert!(DsChoices::resolve(
            IndexStructureSelector::HashTrie,
            &layout,
            &ConfigChoices::default(),
            &BuildChoices::default(),
        )
        .is_err());
    }
}
