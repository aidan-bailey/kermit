//! CLI option groups for the optimisation axes (`--ds-layout-*`,
//! `--ds-config`, `--ds-build`), the planner (`--optimiser`,
//! `--column-orders`), and the places the Layout products are
//! monomorphised: `with_hash_trie_layout!` and `with_sorted_trie_layout!`.

use {
    crate::IndexStructureSelector,
    clap::{Args, ValueEnum},
    kermit_algos::{ColumnOrderPolicy, Optimiser, Planner},
    kermit_ds::{
        ChildCapacity, ColumnTrieBuildMode, ExpansionPolicy, HashTrieBuildMode, HashTrieConfig,
        IndexStructure, LoadFactor, PruningPolicy, RootCapacity, SeekStrategy, TreeTrieBuildMode,
    },
    kermit_iters::{HashStrategy, LayoutOption},
    std::fmt,
};

/// How a join is planned: the optimiser and the column-order policy.
/// Flattened into `join`, `bench join` and `bench run`; `bench ds` joins
/// nothing and has neither flag.
#[derive(Args, Copy, Clone, Debug)]
pub(crate) struct PlannerArgs {
    /// Query optimiser (plans the join's variable ordering). Long-only:
    /// `-o` belongs to `--output`.
    #[arg(long, value_enum, default_value_t = Optimiser::Lexicographic)]
    pub(crate) optimiser: Optimiser,

    /// Column orders the planner may bind an atom's columns in: `stored`
    /// reads each relation in its stored column order, so a plan binds
    /// every atom's columns left to right; `any` lets the planner choose,
    /// and an atom whose plan disagrees with the stored order runs over a
    /// per-query copy with the columns permuted, built before the timed
    /// join (`copies`) and dropped after the query.
    #[arg(long, value_enum, default_value_t = ColumnOrderPolicy::Stored)]
    pub(crate) column_orders: ColumnOrderPolicy,
}

impl PlannerArgs {
    /// The planner these flags select. A `Planner` owns its optimiser, so
    /// each family gets its own.
    pub(crate) fn instantiate(self) -> Planner {
        Planner::new(self.optimiser.instantiate(), self.column_orders)
    }
}

/// One `--ds-*` flag. Each sets an axis that only some index structures
/// have, so a flag given for a run with none of them would be silently
/// ignored. Two checks rule that out, both reading
/// [`DsFlag::structures`]: [`DsChoices::resolve`] against the structures
/// `-i` selects, and `resolve_sweep` (`bench/run.rs`) against the cells
/// left once `-a` has narrowed a `bench run` sweep (#86).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum DsFlag {
    /// `--ds-layout-hasher`.
    LayoutHasher,
    /// `--ds-layout-pruning`.
    LayoutPruning,
    /// `--ds-layout-expansion`.
    LayoutExpansion,
    /// `--ds-layout-seek`.
    LayoutSeek,
    /// `--ds-config`.
    Config,
    /// `--ds-build <structure>=…`: one per pair, each reaching only its
    /// structure.
    Build(IndexStructure),
}

impl DsFlag {
    /// The flags the user passed among `layout`, `config` and `build`, in
    /// that order. A flag left to its default is absent, so defaults pass
    /// every check.
    pub(crate) fn given(
        layout: &LayoutChoices, config: &ConfigChoices, build: &BuildChoices,
    ) -> Vec<DsFlag> {
        let mut given = layout.given();
        given.extend(config.given());
        given.extend(build.given());
        given
    }

    /// The structures that have this flag's axis: the one table of which
    /// flag applies where. A new structure has none of them until it is
    /// listed here.
    pub(crate) fn structures(self) -> &'static [IndexStructure] {
        match self {
            | Self::LayoutHasher | Self::LayoutPruning | Self::LayoutExpansion | Self::Config => {
                &[IndexStructure::HashTrie]
            },
            | Self::LayoutSeek => &[IndexStructure::TreeTrie, IndexStructure::ColumnTrie],
            | Self::Build(IndexStructure::TreeTrie) => &[IndexStructure::TreeTrie],
            | Self::Build(IndexStructure::ColumnTrie) => &[IndexStructure::ColumnTrie],
            | Self::Build(IndexStructure::HashTrie) => &[IndexStructure::HashTrie],
        }
    }

    /// [`structures`](Self::structures) as `-i` spells them, for errors:
    /// `"tree-trie or column-trie"`.
    pub(crate) fn structures_label(self) -> String {
        self.structures()
            .iter()
            .map(|&ds| cli_name(ds))
            .collect::<Vec<_>>()
            .join(" or ")
    }
}

impl fmt::Display for DsFlag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            | Self::LayoutHasher => f.write_str("--ds-layout-hasher"),
            | Self::LayoutPruning => f.write_str("--ds-layout-pruning"),
            | Self::LayoutExpansion => f.write_str("--ds-layout-expansion"),
            | Self::LayoutSeek => f.write_str("--ds-layout-seek"),
            | Self::Config => f.write_str("--ds-config"),
            | Self::Build(ds) => write!(f, "--ds-build {}", cli_name(*ds)),
        }
    }
}

/// A `--ds-*` value that is valid only under another: the one table of
/// prerequisites between optimisation axes
/// (`docs/specs/optimization-standard.md` § Dependencies between
/// optimisations). An axis value means one thing under every other axis,
/// so the only dependency the standard admits is a prerequisite, and the
/// only answer to a missing one is this usage error: no flag implies
/// another. A row applies on the structure that has both axes, which the
/// per-structure checks have already established.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum Prerequisite {
    /// `--ds-build hash-trie=presized:N` cuts its regions from a root sized
    /// before any tuple arrives, so it needs `--ds-config
    /// root-capacity=tuples`.
    PresizedBuildNeedsPresizedRoot,
    /// `--ds-build hash-trie=incremental` creates each child on its first
    /// tuple, before the child's list is known, so it cannot size it: it
    /// needs `--ds-config child-capacity=grow` (#107).
    IncrementalBuildNeedsGrowingChildren,
}

impl Prerequisite {
    /// Every variant, in declaration order. Nothing checks this list against
    /// the enum: a variant missing here is never checked, so add each new
    /// variant here as well as its `violated` arm and its guard-test fixture.
    pub(crate) const ALL: &'static [Prerequisite] = &[
        Self::PresizedBuildNeedsPresizedRoot,
        Self::IncrementalBuildNeedsGrowingChildren,
    ];

    /// `Some(violation)` when `choices` selects the dependent value without
    /// its prerequisite.
    pub(crate) fn violated(self, choices: &DsChoices) -> Option<Violation> {
        match self {
            | Self::PresizedBuildNeedsPresizedRoot => match choices.build.hash_trie {
                | HashTrieBuildMode::Presized(threads)
                    if choices.config.root_capacity != RootCapacity::Tuples =>
                {
                    Some(Violation {
                        dependent: format!("--ds-build hash-trie=presized:{}", threads.get()),
                        requires: "--ds-config root-capacity=tuples",
                        actual: format!(
                            "root-capacity={}{}",
                            choices.config.root_capacity.axis_value(),
                            if choices.config.root_capacity == RootCapacity::default() {
                                " (the default)"
                            } else {
                                ""
                            }
                        ),
                    })
                },
                | _ => None,
            },
            | Self::IncrementalBuildNeedsGrowingChildren => match choices.build.hash_trie {
                | HashTrieBuildMode::Incremental
                    if choices.config.child_capacity != ChildCapacity::Grow =>
                {
                    // `grow`, the required value, is the default, so the actual
                    // value here is never the default.
                    Some(Violation {
                        dependent: "--ds-build hash-trie=incremental".to_owned(),
                        requires: "--ds-config child-capacity=grow",
                        actual: format!(
                            "child-capacity={}",
                            choices.config.child_capacity.axis_value()
                        ),
                    })
                },
                | _ => None,
            },
        }
    }
}

/// What a violated [`Prerequisite`] reports.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Violation {
    /// The flag and value the user gave: `--ds-build hash-trie=presized:8`.
    pub dependent: String,
    /// The flag and value it needs: `--ds-config root-capacity=tuples`.
    pub requires: &'static str,
    /// What the required axis resolved to: `root-capacity=grow (the default)`.
    pub actual: String,
}

/// `ds` as `-i` spells it: `"hash-trie"`.
fn cli_name(ds: IndexStructure) -> String {
    ds.to_possible_value()
        .expect("every IndexStructure is a CLI value")
        .get_name()
        .to_owned()
}

/// The first of `given` that none of `structures` has the axis of: a flag
/// that would reach no cell of a run over `structures`.
pub(crate) fn unreached_flag(given: &[DsFlag], structures: &[IndexStructure]) -> Option<DsFlag> {
    given
        .iter()
        .copied()
        .find(|flag| !flag.structures().iter().any(|ds| structures.contains(ds)))
}

/// Rejects the first of `given` that none of the structures `indexstructure`
/// selects has the axis of. `all` selects every structure, so it accepts
/// every flag here; `bench run` checks again once `-a` has narrowed its
/// sweep (`resolve_sweep`).
fn validate_ds_flags(
    indexstructure: IndexStructureSelector, given: &[DsFlag],
) -> anyhow::Result<()> {
    match unreached_flag(given, &indexstructure.expand()) {
        | Some(flag) => anyhow::bail!(
            "{flag} is only valid with --indexstructure {} (or all); got --indexstructure \
             {indexstructure:?}",
            flag.structures_label()
        ),
        | None => Ok(()),
    }
}

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

/// CLI-side selector for `--ds-layout-expansion`: the `ExpansionPolicy`
/// monomorphised into `HashTrie<H, P, E>`. `Eager` builds every level at
/// construction, the structure that existed before the parameter. `Lazy`
/// builds each child below the root on the first probe that reaches it
/// (SIGMOD 2020 Figure 6).
///
/// Like [`HasherChoice`], this flag only applies when the selected index
/// structure is `hash-trie`; `validate_layout_choices` rejects it elsewhere.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum ExpansionChoice {
    /// Every level built at construction (`EagerExpansion`), the default.
    #[default]
    Eager,
    /// Children built on first probe (`LazyExpansion`).
    Lazy,
}

impl ExpansionChoice {
    /// The choice that monomorphises to the [`ExpansionPolicy`] marker
    /// whose [`LayoutOption::NAME`] is `name`, or `None` if no CLI choice
    /// does. The counterpart of [`HasherChoice::from_layout_name`].
    pub(crate) fn from_layout_name(name: &str) -> Option<Self> {
        match name {
            | "eager" => Some(Self::Eager),
            | "lazy" => Some(Self::Lazy),
            | _ => None,
        }
    }
}

/// The `--ds-layout-expansion` label of the [`ExpansionPolicy`] a code path
/// was monomorphised over. The counterpart of [`hasher_of`].
///
/// # Panics
///
/// Panics if `E`'s layout name has no [`ExpansionChoice`], with the same
/// caveat about what the round-trip test covers; see [`hasher_of`].
pub(crate) fn expansion_of<E: ExpansionPolicy>() -> ExpansionChoice {
    let name = <E as LayoutOption>::NAME;
    ExpansionChoice::from_layout_name(name).unwrap_or_else(|| {
        panic!(
            "no --ds-layout-expansion choice for expansion policy {name:?} ({})",
            std::any::type_name::<E>()
        )
    })
}

/// CLI-side selector for `--ds-layout-seek`: the [`SeekStrategy`]
/// monomorphised into `TreeTrie<S>` / `ColumnTrie<S>`. `Galloping` is the
/// default; `Binary` is the `partition_point` search both sorted tries used
/// before the parameter existed.
///
/// Only valid when the selected index structure is `tree-trie` or
/// `column-trie` (or `all`): `validate_layout_choices` rejects it on
/// `hash-trie`, and `bench ds` rejects it outright, since none of its
/// metrics seeks.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum SeekChoice {
    /// A scan (`LinearSeek`).
    Linear,
    /// A binary search (`BinarySeek`).
    Binary,
    /// A galloping search (`GallopingSeek`), the default.
    #[default]
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
/// monomorphises over a Layout-parameterised structure (`HashTrie<H, P, E>`,
/// `TreeTrie<S>`, `ColumnTrie<S>`). Each field is named `<axis>` and surfaces
/// as the long flag `--ds-layout-<axis>` so the prefix matches the bench-report
/// axis namespace described in CLAUDE.md → "JSON bench reports".
///
/// Every field is an `Option<…>` rather than a clap-defaulted value so we
/// can distinguish "not provided" from "explicitly defaulted". The
/// `*_explicit` accessors consult that for the `validate_layout_choices`
/// checks that reject e.g. `--ds-layout-hasher fxhash -i tree-trie`,
/// `--ds-layout-pruning on -i tree-trie` or `--ds-layout-expansion lazy -i
/// tree-trie`, while the `*_resolved` accessors
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
    /// Child-expansion Layout of `HashTrie<H, P, E>` (default: `eager`).
    /// Only valid when `--indexstructure hash-trie` is selected.
    #[arg(long = "ds-layout-expansion", value_name = "EXPANSION", value_enum)]
    hash_trie_expansion: Option<ExpansionChoice>,
    /// Seek strategy of `TreeTrie<S>` / `ColumnTrie<S>` (default:
    /// `galloping`). Only valid when `--indexstructure tree-trie` or
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

    /// Returns the `ExpansionChoice` to monomorphise on, applying the
    /// `ExpansionChoice::default()` when none was supplied on the command
    /// line. Use this at dispatch sites.
    pub(crate) fn hash_trie_expansion_resolved(&self) -> ExpansionChoice {
        self.hash_trie_expansion.unwrap_or_default()
    }

    /// Returns whether the user explicitly passed `--ds-layout-expansion`.
    pub(crate) fn hash_trie_expansion_explicit(&self) -> bool { self.hash_trie_expansion.is_some() }

    /// Returns the `SeekChoice` to monomorphise on, applying
    /// `SeekChoice::default()` when none was supplied on the command line.
    /// Use this at dispatch sites.
    pub(crate) fn sorted_trie_seek_resolved(&self) -> SeekChoice {
        self.sorted_trie_seek.unwrap_or_default()
    }

    /// Returns whether the user explicitly passed `--ds-layout-seek`.
    pub(crate) fn sorted_trie_seek_explicit(&self) -> bool { self.sorted_trie_seek.is_some() }

    /// The Layout flags the user passed. See [`DsFlag::given`].
    fn given(&self) -> Vec<DsFlag> {
        [
            (DsFlag::LayoutHasher, self.hash_trie_hasher_explicit()),
            (DsFlag::LayoutPruning, self.hash_trie_pruning_explicit()),
            (DsFlag::LayoutExpansion, self.hash_trie_expansion_explicit()),
            (DsFlag::LayoutSeek, self.sorted_trie_seek_explicit()),
        ]
        .into_iter()
        .filter_map(|(flag, given)| given.then_some(flag))
        .collect()
    }
}

/// Rejects `LayoutChoices` flags that are incompatible with the chosen
/// `IndexStructureSelector`. Each flag names a Layout of particular
/// structures ([`DsFlag::structures`]): `--ds-layout-hasher`,
/// `--ds-layout-pruning` and `--ds-layout-expansion` belong to `hash-trie`, and
/// `--ds-layout-seek` to `tree-trie` and `column-trie`. A flag on a structure
/// without its Layout is a usage error: it would be silently ignored, producing
/// a benchmark report whose `ds_layout_*` axis disagrees with the actual
/// structure used. `all` passes here; see [`validate_ds_flags`].
pub(crate) fn validate_layout_choices(
    indexstructure: IndexStructureSelector, layout: &LayoutChoices,
) -> anyhow::Result<()> {
    validate_ds_flags(indexstructure, &layout.given())
}

/// Monomorphises `$body` over the `HashTrie` Layout cell selected at
/// runtime.
///
/// The dispatchers (`dispatch_ds_bench` and `dispatch_run_bench`, in
/// `bench/ds.rs` and `bench/run.rs`) and `load_query_runner` (in `main.rs`)
/// all go through here, so the Layout product is written out once rather
/// than once per caller. It is not free of the product, though: the arms
/// *are* the cells, so each Layout dimension doubles them (2^n in general).
/// The third, expansion (#92), took them to 8, and also cost a
/// `LayoutChoices` field, a `DsChoices` field, an `Execution::HashHtj`
/// field, and a type parameter (with its `*_of::<X>()` label) on `HashHtj`
/// and `HashTrieFamily`. Before a fourth dimension, reach for a nested macro
/// that expands one dimension at a time, or a builder — not another
/// hand-written 16-arm match.
///
/// Hygiene contract: the identifiers named in the closure-like pattern
/// become *type aliases* scoped to the whole arm, so `$body` must not need
/// a different type of any of those names.
macro_rules! with_hash_trie_layout {
    ($hasher:expr, $pruning:expr, $expansion:expr, | $H:ident, $P:ident, $E:ident | $body:expr) => {
        match ($hasher, $pruning, $expansion) {
            | (
                $crate::options::HasherChoice::Sip,
                $crate::options::PruningChoice::Off,
                $crate::options::ExpansionChoice::Eager,
            ) => {
                type $H = ::kermit_iters::SipHashStrategy;
                type $P = ::kermit_ds::NoPruning;
                type $E = ::kermit_ds::EagerExpansion;
                $body
            },
            | (
                $crate::options::HasherChoice::Sip,
                $crate::options::PruningChoice::Off,
                $crate::options::ExpansionChoice::Lazy,
            ) => {
                type $H = ::kermit_iters::SipHashStrategy;
                type $P = ::kermit_ds::NoPruning;
                type $E = ::kermit_ds::LazyExpansion;
                $body
            },
            | (
                $crate::options::HasherChoice::Sip,
                $crate::options::PruningChoice::On,
                $crate::options::ExpansionChoice::Eager,
            ) => {
                type $H = ::kermit_iters::SipHashStrategy;
                type $P = ::kermit_ds::SingletonPruning;
                type $E = ::kermit_ds::EagerExpansion;
                $body
            },
            | (
                $crate::options::HasherChoice::Sip,
                $crate::options::PruningChoice::On,
                $crate::options::ExpansionChoice::Lazy,
            ) => {
                type $H = ::kermit_iters::SipHashStrategy;
                type $P = ::kermit_ds::SingletonPruning;
                type $E = ::kermit_ds::LazyExpansion;
                $body
            },
            | (
                $crate::options::HasherChoice::Fxhash,
                $crate::options::PruningChoice::Off,
                $crate::options::ExpansionChoice::Eager,
            ) => {
                type $H = ::kermit_iters::FxHashStrategy;
                type $P = ::kermit_ds::NoPruning;
                type $E = ::kermit_ds::EagerExpansion;
                $body
            },
            | (
                $crate::options::HasherChoice::Fxhash,
                $crate::options::PruningChoice::Off,
                $crate::options::ExpansionChoice::Lazy,
            ) => {
                type $H = ::kermit_iters::FxHashStrategy;
                type $P = ::kermit_ds::NoPruning;
                type $E = ::kermit_ds::LazyExpansion;
                $body
            },
            | (
                $crate::options::HasherChoice::Fxhash,
                $crate::options::PruningChoice::On,
                $crate::options::ExpansionChoice::Eager,
            ) => {
                type $H = ::kermit_iters::FxHashStrategy;
                type $P = ::kermit_ds::SingletonPruning;
                type $E = ::kermit_ds::EagerExpansion;
                $body
            },
            | (
                $crate::options::HasherChoice::Fxhash,
                $crate::options::PruningChoice::On,
                $crate::options::ExpansionChoice::Lazy,
            ) => {
                type $H = ::kermit_iters::FxHashStrategy;
                type $P = ::kermit_ds::SingletonPruning;
                type $E = ::kermit_ds::LazyExpansion;
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
    /// pairs. `HashTrie` accepts `load-factor=<decimal in (0, 1)>`,
    /// `root-capacity=grow|tuples` and `child-capacity=grow|tuples`. Only
    /// valid with `--indexstructure hash-trie` (or `all`).
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
    pub(crate) const HASH_TRIE_KEYS: &'static [&'static str] =
        &["load-factor", "root-capacity", "child-capacity"];

    /// Whether the user passed any `--ds-config` pair.
    pub(crate) fn explicit(&self) -> bool { !self.ds_config.is_empty() }

    /// `--ds-config` if the user passed it. See [`DsFlag::given`].
    fn given(&self) -> Option<DsFlag> { self.explicit().then_some(DsFlag::Config) }

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
                | "root-capacity" => {
                    config.root_capacity = value
                        .parse::<RootCapacity>()
                        .map_err(|why| anyhow::anyhow!("--ds-config {key}: {why}"))?;
                },
                | "child-capacity" => {
                    config.child_capacity = value
                        .parse::<ChildCapacity>()
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
    validate_ds_flags(indexstructure, config.given().as_slice())
}

/// BuildMode-axis CLI choices, flattened beside [`LayoutChoices`] and
/// [`ConfigChoices`] into `bench ds`, `bench run` and `bench join`. One flag,
/// `--ds-build`, takes comma-separated `structure=mode` pairs, resolved per
/// structure by [`resolved`](Self::resolved). Every build mode builds an
/// equivalent structure (the same contents and capacities), so the flag
/// changes build time, and placement at most. `kermit join` takes
/// no `--ds-build`, for the same reason it takes no `--ds-config`: it cannot
/// change a query's answers.
#[derive(Args, Clone, Debug, Default)]
pub(crate) struct BuildChoices {
    /// How each named structure is built from its tuples, as
    /// `structure=mode` pairs: `tree-trie=serial|parallel:<threads>` (default
    /// `serial`; threads in 1..=1024), `column-trie=bulk|incremental` (default
    /// `bulk`; `incremental` is the build before the one-pass bulk build) and
    /// `hash-trie=bulk` (the default), `incremental`, `radix:<bits>`,
    /// `parallel:<threads>` or `presized:<threads>` (bits in 1..=16, threads
    /// in 1..=1024; `presized` requires `--ds-config root-capacity=tuples`,
    /// and `incremental` requires `--ds-config child-capacity=grow`).
    /// A pair is only valid when `--indexstructure` selects its structure
    /// (or `all`).
    #[arg(
        long = "ds-build",
        value_name = "STRUCTURE=MODE,...",
        value_delimiter = ','
    )]
    ds_build: Vec<String>,
}

impl BuildChoices {
    /// The structures that have a build mode, as `--ds-build` keys.
    const STRUCTURES: &'static [IndexStructure] = &[
        IndexStructure::TreeTrie,
        IndexStructure::ColumnTrie,
        IndexStructure::HashTrie,
    ];

    /// One [`DsFlag::Build`] per pair whose key names a structure with a
    /// build mode, each reaching only that structure. Malformed pairs yield
    /// no flag here; [`resolved`](Self::resolved) rejects them.
    fn given(&self) -> Vec<DsFlag> {
        self.ds_build
            .iter()
            .filter_map(|pair| {
                let (key, _) = pair.split_once('=')?;
                let ds = <IndexStructure as ValueEnum>::from_str(key, false).ok()?;
                Self::STRUCTURES.contains(&ds).then_some(DsFlag::Build(ds))
            })
            .collect()
    }

    /// Resolves the pairs into each structure's mode, starting from the
    /// defaults. A bare mode, a structure without build modes, an unknown
    /// structure or mode, and a repeated structure are usage errors.
    pub(crate) fn resolved(&self) -> anyhow::Result<BuildModes> {
        let mut modes = BuildModes::default();
        let mut seen = Vec::new();
        for pair in &self.ds_build {
            let Some((key, mode)) = pair.split_once('=') else {
                anyhow::bail!(
                    "--ds-build expects structure=mode pairs; got {pair:?}{}",
                    bare_mode_hint(pair)
                );
            };
            let ds = <IndexStructure as ValueEnum>::from_str(key, false).map_err(|_| {
                anyhow::anyhow!(
                    "--ds-build: unknown structure {key:?}; structures with build modes: {}",
                    Self::structures_label()
                )
            })?;
            if !Self::STRUCTURES.contains(&ds) {
                anyhow::bail!(
                    "--ds-build: {key} has a single build process; structures with build modes: {}",
                    Self::structures_label()
                );
            }
            if seen.contains(&ds) {
                anyhow::bail!("--ds-build: {key} given more than once");
            }
            seen.push(ds);
            match ds {
                | IndexStructure::TreeTrie => {
                    modes.tree_trie = mode
                        .parse()
                        .map_err(|why| anyhow::anyhow!("--ds-build tree-trie: {why}"))?;
                },
                | IndexStructure::ColumnTrie => {
                    modes.column_trie = <ColumnTrieBuildMode as ValueEnum>::from_str(mode, false)
                        .map_err(|_| {
                        anyhow::anyhow!(
                            "--ds-build column-trie: unknown mode {mode:?}; expected {}",
                            column_trie_mode_names().join(" or ")
                        )
                    })?;
                },
                | IndexStructure::HashTrie => {
                    modes.hash_trie = mode
                        .parse()
                        .map_err(|why| anyhow::anyhow!("--ds-build hash-trie: {why}"))?;
                },
            }
        }
        Ok(modes)
    }

    /// [`STRUCTURES`](Self::STRUCTURES) as `-i` spells them.
    fn structures_label() -> String {
        Self::STRUCTURES
            .iter()
            .map(|&ds| cli_name(ds))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// `ColumnTrieBuildMode`'s modes as `--ds-build column-trie=` spells them.
fn column_trie_mode_names() -> Vec<String> {
    ColumnTrieBuildMode::value_variants()
        .iter()
        .map(|mode| {
            mode.to_possible_value()
                .expect("every ColumnTrieBuildMode is a CLI value")
                .get_name()
                .to_owned()
        })
        .collect()
}

/// The rest of the error for a bare `--ds-build` mode: the keyed spelling
/// for each structure that has that mode, or an example when none does.
fn bare_mode_hint(mode: &str) -> String {
    let mut keyed = Vec::new();
    if mode.parse::<TreeTrieBuildMode>().is_ok() {
        keyed.push(format!("tree-trie={mode}"));
    }
    if <ColumnTrieBuildMode as ValueEnum>::from_str(mode, false).is_ok() {
        keyed.push(format!("column-trie={mode}"));
    }
    if mode.parse::<HashTrieBuildMode>().is_ok() {
        keyed.push(format!("hash-trie={mode}"));
    }
    if keyed.is_empty() {
        "; for example --ds-build column-trie=incremental,hash-trie=radix:8".to_owned()
    } else {
        format!("; did you mean --ds-build {}?", keyed.join(" or "))
    }
}

/// Rejects a `--ds-build` pair whose structure `indexstructure` does not
/// select, so a report can never carry a `ds_build_mode` the build ignored.
/// Same discipline as [`validate_config_choices`].
pub(crate) fn validate_build_choices(
    indexstructure: IndexStructureSelector, build: &BuildChoices,
) -> anyhow::Result<()> {
    validate_ds_flags(indexstructure, &build.given())
}

/// The build mode of every structure that has one: what `--ds-build`
/// resolves to, with each structure's default where no pair names it.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct BuildModes {
    /// Reaches the tree-trie cell only.
    pub tree_trie: TreeTrieBuildMode,
    /// Reaches the column-trie cell only.
    pub column_trie: ColumnTrieBuildMode,
    /// Reaches the hash-trie cell only.
    pub hash_trie: HashTrieBuildMode,
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
    /// `--ds-layout-expansion`; reaches the hash-trie cell only.
    pub expansion: ExpansionChoice,
    /// `--ds-layout-seek`; reaches the two sorted-trie cells.
    pub seek: SeekChoice,
    /// `--ds-config`; reaches the hash-trie cell only.
    pub config: HashTrieConfig,
    /// `--ds-build`; each structure's mode reaches its own cell.
    pub build: BuildModes,
}

impl DsChoices {
    /// Validates every `--ds-*` flag against `indexstructure`, then resolves
    /// them, applying each option's default where no flag was given.
    ///
    /// # Errors
    ///
    /// Returns an error if a flag was given for a structure that lacks its
    /// axis, if `--ds-config` or `--ds-build` is malformed, or if a value's
    /// prerequisite is missing ([`Prerequisite`]).
    pub(crate) fn resolve(
        indexstructure: IndexStructureSelector, layout: &LayoutChoices, config: &ConfigChoices,
        build: &BuildChoices,
    ) -> anyhow::Result<Self> {
        validate_layout_choices(indexstructure, layout)?;
        validate_config_choices(indexstructure, config)?;
        validate_build_choices(indexstructure, build)?;
        let choices = Self {
            hasher: layout.hash_trie_hasher_resolved(),
            pruning: layout.hash_trie_pruning_resolved(),
            expansion: layout.hash_trie_expansion_resolved(),
            seek: layout.sorted_trie_seek_resolved(),
            config: config.hash_trie_config_resolved()?,
            build: build.resolved()?,
        };
        if let Some(v) = Prerequisite::ALL.iter().find_map(|p| p.violated(&choices)) {
            anyhow::bail!("{} requires {}; got {}", v.dependent, v.requires, v.actual);
        }
        Ok(choices)
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        clap::{Parser, ValueEnum},
        kermit_ds::{
            BinarySeek, EagerExpansion, GallopingSeek, LazyExpansion, LinearSeek, NoPruning,
            SingletonPruning, Threads,
        },
        kermit_iters::{BuildMode, FxHashStrategy, SipHashStrategy},
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

    /// Every `with_hash_trie_layout!` arm binds the marker triple its
    /// `(HasherChoice, PruningChoice, ExpansionChoice)` pattern names.
    /// Reading the labels back out of the aliases the macro defines pins
    /// the eight cells against `hasher_of` / `pruning_of` / `expansion_of`,
    /// so a transposed arm fails here rather than silently mislabelling a
    /// bench report.
    #[test]
    fn layout_macro_binds_the_marker_triple_its_arm_names() {
        for &hasher in HasherChoice::value_variants() {
            for &pruning in PruningChoice::value_variants() {
                for &expansion in ExpansionChoice::value_variants() {
                    let bound = with_hash_trie_layout!(hasher, pruning, expansion, |H, P, E| (
                        hasher_of::<H>(),
                        pruning_of::<P>(),
                        expansion_of::<E>()
                    ));
                    assert_eq!(bound, (hasher, pruning, expansion));
                }
            }
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

    /// The same round trip for `--ds-layout-expansion` and `ExpansionPolicy`.
    #[test]
    fn expansion_choices_round_trip_through_layout_names() {
        const TABLE: &[(ExpansionChoice, &str)] = &[
            (
                ExpansionChoice::Eager,
                <EagerExpansion as LayoutOption>::NAME,
            ),
            (ExpansionChoice::Lazy, <LazyExpansion as LayoutOption>::NAME),
        ];
        for choice in ExpansionChoice::value_variants() {
            let (_, name) = TABLE
                .iter()
                .find(|(c, _)| c == choice)
                .expect("every ExpansionChoice has a marker");
            assert_eq!(ExpansionChoice::from_layout_name(name), Some(*choice));
        }
        assert_eq!(expansion_of::<EagerExpansion>(), ExpansionChoice::Eager);
        assert_eq!(expansion_of::<LazyExpansion>(), ExpansionChoice::Lazy);
    }

    #[test]
    fn validate_layout_choices_rejects_explicit_expansion_on_non_hash_trie() {
        let layout = LayoutChoices {
            hash_trie_expansion: Some(ExpansionChoice::Lazy),
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
            assert!(msg.contains("--ds-layout-expansion"), "{sel:?}: {msg}");
            assert!(msg.contains("hash-trie"), "{sel:?}: {msg}");
        }
    }

    #[test]
    fn expansion_choice_default_is_eager() {
        assert_eq!(ExpansionChoice::default(), ExpansionChoice::Eager);
        assert_eq!(
            LayoutChoices::default().hash_trie_expansion_resolved(),
            ExpansionChoice::Eager
        );
        assert!(!LayoutChoices::default().hash_trie_expansion_explicit());
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
    fn config_choices_parse_root_capacity() {
        let tuples = ConfigChoices {
            ds_config: vec!["root-capacity=tuples".into()],
        };
        assert_eq!(
            tuples.hash_trie_config_resolved().unwrap().root_capacity,
            RootCapacity::Tuples
        );
        let both = ConfigChoices {
            ds_config: vec!["load-factor=0.5".into(), "root-capacity=grow".into()],
        };
        let resolved = both.hash_trie_config_resolved().unwrap();
        assert_eq!(resolved.root_capacity, RootCapacity::Grow);
        assert_eq!(resolved.load_factor, LoadFactor::percent(50).unwrap());
        assert_eq!(
            ConfigChoices::default()
                .hash_trie_config_resolved()
                .unwrap()
                .root_capacity,
            RootCapacity::Grow
        );
    }

    #[test]
    fn config_choices_reject_bad_root_capacities() {
        for bad in ["", "Grow", "presize", "1024"] {
            let choices = ConfigChoices {
                ds_config: vec![format!("root-capacity={bad}")],
            };
            let msg = choices.hash_trie_config_resolved().unwrap_err().to_string();
            assert!(msg.contains("root-capacity"), "{bad:?}: {msg}");
            assert!(msg.contains("expected grow or tuples"), "{bad:?}: {msg}");
        }
    }

    #[test]
    fn config_choices_parse_child_capacity() {
        let tuples = ConfigChoices {
            ds_config: vec!["child-capacity=tuples".into()],
        };
        assert_eq!(
            tuples.hash_trie_config_resolved().unwrap().child_capacity,
            ChildCapacity::Tuples
        );
        let all = ConfigChoices {
            ds_config: vec![
                "load-factor=0.8".into(),
                "root-capacity=tuples".into(),
                "child-capacity=tuples".into(),
            ],
        };
        let resolved = all.hash_trie_config_resolved().unwrap();
        assert_eq!(resolved.child_capacity, ChildCapacity::Tuples);
        assert_eq!(resolved.root_capacity, RootCapacity::Tuples);
        assert_eq!(resolved.load_factor, LoadFactor::percent(80).unwrap());
        assert_eq!(
            ConfigChoices::default()
                .hash_trie_config_resolved()
                .unwrap()
                .child_capacity,
            ChildCapacity::Grow
        );
    }

    #[test]
    fn config_choices_reject_bad_child_capacities() {
        for bad in ["", "Grow", "keys", "1024"] {
            let choices = ConfigChoices {
                ds_config: vec![format!("child-capacity={bad}")],
            };
            let msg = choices.hash_trie_config_resolved().unwrap_err().to_string();
            assert!(msg.contains("child-capacity"), "{bad:?}: {msg}");
            assert!(msg.contains("expected grow or tuples"), "{bad:?}: {msg}");
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
        const SAMPLE: &[(&str, &str)] = &[
            ("load-factor", "0.5"),
            ("root-capacity", "tuples"),
            ("child-capacity", "tuples"),
        ];
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
        assert_eq!(choices.expansion, ExpansionChoice::Eager);
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

    fn build(pairs: &[&str]) -> BuildChoices {
        BuildChoices {
            ds_build: pairs.iter().map(|pair| pair.to_string()).collect(),
        }
    }

    #[test]
    fn build_choices_resolve_keyed_pairs_per_structure() {
        let radix8 = HashTrieBuildMode::Radix(kermit_ds::RadixBits::new(8).unwrap());
        let parallel2 = TreeTrieBuildMode::Parallel(Threads::new(2).unwrap());
        assert_eq!(build(&[]).resolved().unwrap(), BuildModes::default());
        assert_eq!(
            build(&["hash-trie=bulk"]).resolved().unwrap(),
            BuildModes::default()
        );
        assert_eq!(
            build(&["tree-trie=serial"]).resolved().unwrap(),
            BuildModes::default()
        );
        assert_eq!(
            build(&["hash-trie=radix:8"]).resolved().unwrap(),
            BuildModes {
                tree_trie: TreeTrieBuildMode::Serial,
                column_trie: ColumnTrieBuildMode::Bulk,
                hash_trie: radix8,
            }
        );
        assert_eq!(
            build(&["tree-trie=parallel:2"]).resolved().unwrap(),
            BuildModes {
                tree_trie: parallel2,
                column_trie: ColumnTrieBuildMode::Bulk,
                hash_trie: HashTrieBuildMode::Bulk,
            }
        );
        assert_eq!(
            build(&[
                "column-trie=incremental",
                "hash-trie=radix:8",
                "tree-trie=parallel:2"
            ])
            .resolved()
            .unwrap(),
            BuildModes {
                tree_trie: parallel2,
                column_trie: ColumnTrieBuildMode::Incremental,
                hash_trie: radix8,
            }
        );
    }

    /// Every malformed `--ds-build` is a usage error naming what is wrong.
    #[test]
    fn build_choices_reject_malformed_pairs() {
        let cases: &[(&[&str], &str)] = &[
            (
                &["incremental"],
                "did you mean --ds-build column-trie=incremental or hash-trie=incremental?",
            ),
            (&["radix:8"], "did you mean --ds-build hash-trie=radix:8?"),
            (
                &["parallel:8"],
                "did you mean --ds-build tree-trie=parallel:8 or hash-trie=parallel:8?",
            ),
            (&["serial"], "did you mean --ds-build tree-trie=serial?"),
            (&["fast"], "for example --ds-build"),
            (&["tree-trie=bulk"], "unknown tree-trie build mode \"bulk\""),
            (
                &["tree-trie=radix:8"],
                "unknown tree-trie build mode \"radix:8\"",
            ),
            (&["tree-trie=parallel"], "parallel needs a thread count"),
            (&["tree-trie=parallel:0"], "between 1 and 1024, got 0"),
            (&["tree-trie=parallel:1025"], "between 1 and 1024, got 1025"),
            (
                &["tree-trie=serial", "tree-trie=parallel:2"],
                "tree-trie given more than once",
            ),
            (&["b-tree=bulk"], "unknown structure \"b-tree\""),
            (
                &["hash-trie=bulk", "hash-trie=radix:4"],
                "hash-trie given more than once",
            ),
            (
                &["hash-trie=serial"],
                "serial was renamed incremental in #107",
            ),
            (
                &["bulk"],
                "did you mean --ds-build column-trie=bulk or hash-trie=bulk?",
            ),
            (
                &["column-trie=radix:8"],
                "unknown mode \"radix:8\"; expected incremental or bulk",
            ),
            (&["hash-trie=radix"], "radix needs a bit count"),
            (&["hash-trie=radix:0"], "between 1 and 16"),
            (&["hash-trie=radix:17"], "between 1 and 16"),
            (&["hash-trie=radix:x"], "whole number"),
            (&["hash-trie=parallel"], "parallel needs a thread count"),
            (&["hash-trie=parallel:0"], "between 1 and 1024, got 0"),
            (&["hash-trie=parallel:1025"], "between 1 and 1024, got 1025"),
            (&["hash-trie=parallel:x"], "whole number"),
        ];
        for &(pairs, expected) in cases {
            let msg = build(pairs).resolved().unwrap_err().to_string();
            assert!(msg.contains("--ds-build"), "{pairs:?}: {msg}");
            assert!(msg.contains(expected), "{pairs:?}: {msg}");
        }
    }

    #[test]
    fn validate_build_choices_accepts_each_key_on_its_structure_or_all() {
        use IndexStructureSelector::{All, ColumnTrie, HashTrie, TreeTrie};
        let tree = build(&["tree-trie=parallel:4"]);
        let column = build(&["column-trie=incremental"]);
        let hash = build(&["hash-trie=radix:4"]);
        for (choices, key, home) in [
            (&tree, "tree-trie", TreeTrie),
            (&column, "column-trie", ColumnTrie),
            (&hash, "hash-trie", HashTrie),
        ] {
            assert!(validate_build_choices(home, choices).is_ok());
            assert!(validate_build_choices(All, choices).is_ok());
            for sel in [TreeTrie, ColumnTrie, HashTrie] {
                if sel == home {
                    continue;
                }
                let msg = validate_build_choices(sel, choices)
                    .unwrap_err()
                    .to_string();
                assert!(msg.contains(&format!("--ds-build {key}")), "{msg}");
                assert!(msg.contains(&format!("--indexstructure {key}")), "{msg}");
            }
        }
        for sel in [TreeTrie, ColumnTrie, HashTrie, All] {
            assert!(validate_build_choices(sel, &BuildChoices::default()).is_ok());
        }
    }

    #[test]
    fn ds_choices_resolve_carries_the_build_mode() {
        let radix = HashTrieBuildMode::Radix(kermit_ds::RadixBits::new(4).unwrap());
        let parallel = TreeTrieBuildMode::Parallel(Threads::new(4).unwrap());
        let choices = DsChoices::resolve(
            IndexStructureSelector::All,
            &LayoutChoices::default(),
            &ConfigChoices::default(),
            &build(&[
                "tree-trie=parallel:4",
                "column-trie=incremental",
                "hash-trie=radix:4",
            ]),
        )
        .unwrap();
        assert_eq!(choices.build, BuildModes {
            tree_trie: parallel,
            column_trie: ColumnTrieBuildMode::Incremental,
            hash_trie: radix,
        });
        assert_eq!(DsChoices::default().build, BuildModes::default());
        assert_eq!(BuildModes::default().tree_trie, TreeTrieBuildMode::Serial);
        assert_eq!(BuildModes::default().column_trie, ColumnTrieBuildMode::Bulk);
        assert_eq!(BuildModes::default().hash_trie, HashTrieBuildMode::Bulk);
        assert!(DsChoices::resolve(
            IndexStructureSelector::TreeTrie,
            &LayoutChoices::default(),
            &ConfigChoices::default(),
            &build(&["column-trie=incremental"]),
        )
        .is_err());
        assert!(DsChoices::resolve(
            IndexStructureSelector::ColumnTrie,
            &LayoutChoices::default(),
            &ConfigChoices::default(),
            &build(&["tree-trie=parallel:4"]),
        )
        .is_err());
    }

    /// Every row of the prerequisite table can fire and can be satisfied:
    /// a row whose `violated` arm never matches would be dead. Each row
    /// supplies its own fixtures, one `DsChoices` that violates it and one
    /// that satisfies it, and the `Violation` it must report.
    #[test]
    fn every_prerequisite_is_reachable() {
        for &row in Prerequisite::ALL {
            let (violating, satisfied, expected) = match row {
                | Prerequisite::PresizedBuildNeedsPresizedRoot => {
                    let mut violating = DsChoices::default();
                    violating.build.hash_trie =
                        HashTrieBuildMode::Presized(Threads::new(2).unwrap());
                    let mut satisfied = violating;
                    satisfied.config.root_capacity = RootCapacity::Tuples;
                    (violating, satisfied, Violation {
                        dependent: "--ds-build hash-trie=presized:2".to_owned(),
                        requires: "--ds-config root-capacity=tuples",
                        actual: "root-capacity=grow (the default)".to_owned(),
                    })
                },
                | Prerequisite::IncrementalBuildNeedsGrowingChildren => {
                    let mut violating = DsChoices::default();
                    violating.build.hash_trie = HashTrieBuildMode::Incremental;
                    violating.config.child_capacity = ChildCapacity::Tuples;
                    let mut satisfied = violating;
                    satisfied.config.child_capacity = ChildCapacity::Grow;
                    (violating, satisfied, Violation {
                        dependent: "--ds-build hash-trie=incremental".to_owned(),
                        requires: "--ds-config child-capacity=grow",
                        actual: "child-capacity=tuples".to_owned(),
                    })
                },
            };
            assert_eq!(row.violated(&violating), Some(expected), "{row:?}");
            assert!(
                row.violated(&satisfied).is_none(),
                "{row:?} fires when satisfied"
            );
            assert!(
                row.violated(&DsChoices::default()).is_none(),
                "{row:?} fires by default"
            );
        }
    }

    /// The check runs last in `resolve`, on the resolved values, and its
    /// message names the flag to add.
    #[test]
    fn ds_choices_resolve_rejects_a_violated_prerequisite() {
        let err = DsChoices::resolve(
            IndexStructureSelector::HashTrie,
            &LayoutChoices::default(),
            &ConfigChoices::default(),
            &build(&["hash-trie=presized:2"]),
        )
        .unwrap_err();
        assert_eq!(
            err.to_string(),
            "--ds-build hash-trie=presized:2 requires --ds-config root-capacity=tuples; got \
             root-capacity=grow (the default)"
        );
        let explicit = DsChoices::resolve(
            IndexStructureSelector::All,
            &LayoutChoices::default(),
            &ConfigChoices {
                ds_config: vec!["root-capacity=grow".into()],
            },
            &build(&["hash-trie=presized:2"]),
        )
        .unwrap_err();
        assert!(explicit
            .to_string()
            .ends_with("got root-capacity=grow (the default)"));
        let ok = DsChoices::resolve(
            IndexStructureSelector::All,
            &LayoutChoices::default(),
            &ConfigChoices {
                ds_config: vec!["root-capacity=tuples".into()],
            },
            &build(&["hash-trie=presized:2"]),
        )
        .unwrap();
        assert_eq!(
            ok.build.hash_trie,
            HashTrieBuildMode::Presized(Threads::new(2).unwrap())
        );
        assert_eq!(ok.config.root_capacity, RootCapacity::Tuples);
    }

    /// `incremental` under sized children is rejected with the flag to add.
    #[test]
    fn ds_choices_resolve_rejects_incremental_with_sized_children() {
        let err = DsChoices::resolve(
            IndexStructureSelector::HashTrie,
            &LayoutChoices::default(),
            &ConfigChoices {
                ds_config: vec!["child-capacity=tuples".into()],
            },
            &build(&["hash-trie=incremental"]),
        )
        .unwrap_err();
        assert_eq!(
            err.to_string(),
            "--ds-build hash-trie=incremental requires --ds-config child-capacity=grow; got \
             child-capacity=tuples"
        );
        let ok = DsChoices::resolve(
            IndexStructureSelector::HashTrie,
            &LayoutChoices::default(),
            &ConfigChoices::default(),
            &build(&["hash-trie=incremental"]),
        )
        .unwrap();
        assert_eq!(ok.build.hash_trie, HashTrieBuildMode::Incremental);
        // The row fires only under `incremental`.
        let bulk = DsChoices::resolve(
            IndexStructureSelector::HashTrie,
            &LayoutChoices::default(),
            &ConfigChoices {
                ds_config: vec!["child-capacity=tuples".into()],
            },
            &BuildChoices::default(),
        )
        .unwrap();
        assert_eq!(bulk.config.child_capacity, ChildCapacity::Tuples);
    }

    /// Every structure's `--ds-build` modes resolve to that structure's mode
    /// alone, whose report label is the mode as typed; every other structure
    /// keeps its default. The table is checked against
    /// `IndexStructure::value_variants()`, so a structure added to
    /// [`BuildChoices::STRUCTURES`] fails here until its modes are listed
    /// and [`BuildModes`] carries them.
    #[test]
    fn build_choices_resolve_to_each_structures_labels() {
        fn labels(modes: &BuildModes) -> [(IndexStructure, String); 3] {
            [
                (IndexStructure::TreeTrie, modes.tree_trie.axis_value()),
                (IndexStructure::ColumnTrie, modes.column_trie.axis_value()),
                (IndexStructure::HashTrie, modes.hash_trie.axis_value()),
            ]
        }
        const TABLE: &[(IndexStructure, &[&str])] = &[
            (IndexStructure::TreeTrie, &[
                "serial",
                "parallel:1",
                "parallel:3",
            ]),
            (IndexStructure::ColumnTrie, &["bulk", "incremental"]),
            (IndexStructure::HashTrie, &[
                "bulk",
                "incremental",
                "radix:1",
                "radix:16",
                "parallel:1",
                "parallel:3",
                "presized:1",
                "presized:3",
            ]),
        ];
        let defaults = labels(&BuildModes::default());
        for ds in IndexStructure::value_variants() {
            assert!(
                BuildChoices::STRUCTURES.contains(ds),
                "{ds:?} has no --ds-build key"
            );
            let (_, modes) = TABLE
                .iter()
                .find(|(listed, _)| listed == ds)
                .unwrap_or_else(|| panic!("{ds:?} is missing from the mode table"));
            for mode in *modes {
                let pair = format!("{}={mode}", cli_name(*ds));
                let resolved = build(&[&pair]).resolved().unwrap();
                for ((structure, label), (_, default)) in labels(&resolved).iter().zip(&defaults) {
                    if structure == ds {
                        assert_eq!(label, mode, "{pair} on {structure:?}");
                    } else {
                        assert_eq!(
                            label, default,
                            "{pair} must leave {structure:?} on its default"
                        );
                    }
                }
            }
        }
    }

    /// The `--ds-build` help states the thread range `Threads::MAX` bounds,
    /// once for each trie with a `parallel:<threads>` mode.
    #[test]
    fn ds_build_help_names_the_thread_limit() {
        let command = BuildChoices::augment_args(clap::Command::new("test"));
        let help = command
            .get_arguments()
            .find(|arg| arg.get_id() == "ds_build")
            .and_then(|arg| arg.get_help())
            .expect("--ds-build has help")
            .to_string();
        assert_eq!(
            help.matches(&format!("1..={}", Threads::MAX)).count(),
            2,
            "tree-trie and hash-trie: {help}"
        );
    }

    #[test]
    fn ds_build_flag_names_its_structure() {
        let flag = DsFlag::Build(IndexStructure::TreeTrie);
        assert_eq!(flag.to_string(), "--ds-build tree-trie");
        assert_eq!(flag.structures_label(), "tree-trie");
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

    /// Each explicit option maps to its own flag, and defaults map to none,
    /// so a run that passes no `--ds-*` flag can never be rejected.
    #[test]
    fn ds_flag_given_lists_exactly_the_flags_passed() {
        let none = DsFlag::given(
            &LayoutChoices::default(),
            &ConfigChoices::default(),
            &BuildChoices::default(),
        );
        assert_eq!(none, vec![]);
        let layout = LayoutChoices {
            hash_trie_hasher: Some(HasherChoice::Fxhash),
            hash_trie_pruning: Some(PruningChoice::On),
            hash_trie_expansion: Some(ExpansionChoice::Lazy),
            sorted_trie_seek: Some(SeekChoice::Binary),
        };
        let config = ConfigChoices {
            ds_config: vec!["load-factor=0.5".into()],
        };
        let builds = build(&[
            "column-trie=incremental",
            "hash-trie=radix:4",
            "tree-trie=parallel:2",
        ]);
        assert_eq!(DsFlag::given(&layout, &config, &builds), vec![
            DsFlag::LayoutHasher,
            DsFlag::LayoutPruning,
            DsFlag::LayoutExpansion,
            DsFlag::LayoutSeek,
            DsFlag::Config,
            DsFlag::Build(IndexStructure::ColumnTrie),
            DsFlag::Build(IndexStructure::HashTrie),
            DsFlag::Build(IndexStructure::TreeTrie),
        ]);
        let seek_only = LayoutChoices {
            sorted_trie_seek: Some(SeekChoice::Linear),
            ..LayoutChoices::default()
        };
        assert_eq!(
            DsFlag::given(&seek_only, &ConfigChoices::default(), &builds),
            vec![
                DsFlag::LayoutSeek,
                DsFlag::Build(IndexStructure::ColumnTrie),
                DsFlag::Build(IndexStructure::HashTrie),
                DsFlag::Build(IndexStructure::TreeTrie)
            ]
        );
        // Malformed pairs yield no flag; `resolved` rejects them.
        assert_eq!(
            DsFlag::given(
                &LayoutChoices::default(),
                &ConfigChoices::default(),
                &build(&["incremental", "tree-trie", "b-tree=x"])
            ),
            vec![]
        );
    }

    #[test]
    fn unreached_flag_is_the_first_flag_no_structure_has() {
        let sorted = [IndexStructure::TreeTrie, IndexStructure::ColumnTrie];
        assert_eq!(
            unreached_flag(
                &[
                    DsFlag::LayoutSeek,
                    DsFlag::Build(IndexStructure::ColumnTrie)
                ],
                &sorted
            ),
            None
        );
        assert_eq!(
            unreached_flag(
                &[DsFlag::LayoutSeek, DsFlag::Config, DsFlag::LayoutHasher],
                &sorted
            ),
            Some(DsFlag::Config)
        );
        // A build pair reaches only its own structure: a column-trie pair
        // not TreeTrie, a tree-trie pair not ColumnTrie.
        assert_eq!(
            unreached_flag(&[DsFlag::Build(IndexStructure::ColumnTrie)], &[
                IndexStructure::TreeTrie
            ]),
            Some(DsFlag::Build(IndexStructure::ColumnTrie))
        );
        assert_eq!(
            unreached_flag(&[DsFlag::Build(IndexStructure::TreeTrie)], &sorted),
            None
        );
        assert_eq!(
            unreached_flag(&[DsFlag::Build(IndexStructure::TreeTrie)], &[
                IndexStructure::ColumnTrie
            ]),
            Some(DsFlag::Build(IndexStructure::TreeTrie))
        );
        // A hash-trie build pair reaches no sorted trie.
        assert_eq!(
            unreached_flag(&[DsFlag::Build(IndexStructure::HashTrie)], &sorted),
            Some(DsFlag::Build(IndexStructure::HashTrie))
        );
        assert_eq!(unreached_flag(&[], &[]), None);
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
        assert_eq!(DsChoices::default().seek, SeekChoice::Galloping);
        assert!(DsChoices::resolve(
            IndexStructureSelector::HashTrie,
            &layout,
            &ConfigChoices::default(),
            &BuildChoices::default(),
        )
        .is_err());
    }

    #[test]
    fn planner_args_default_to_lexicographic_stored() {
        #[derive(Parser)]
        struct Cli {
            #[command(flatten)]
            planner: PlannerArgs,
        }
        let cli = Cli::parse_from(["kermit"]);
        assert_eq!(cli.planner.optimiser, Optimiser::Lexicographic);
        assert_eq!(cli.planner.column_orders, ColumnOrderPolicy::Stored);
        assert_eq!(
            cli.planner.instantiate().column_orders(),
            ColumnOrderPolicy::Stored
        );
        let cli = Cli::parse_from([
            "kermit",
            "--column-orders",
            "any",
            "--optimiser",
            "cost-based",
        ]);
        assert_eq!(cli.planner.optimiser, Optimiser::CostBased);
        assert_eq!(
            cli.planner.instantiate().column_orders(),
            ColumnOrderPolicy::Any
        );
    }
}
