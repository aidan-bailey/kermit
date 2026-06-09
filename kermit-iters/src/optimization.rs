//! Vocabulary for data-structure and algorithm optimizations.
//!
//! An optimization is classified as one of three structural categories,
//! each with a prescribed Rust shape, naming convention, and lifetime:
//!
//! - **Layout** (compile-time): a type parameter on the DS or algorithm.
//!   Implementations are typically zero-sized marker types. Each layout
//!   combination is a distinct monomorphized type.
//! - **Config** (runtime): a struct of boolean/enum flags passed to a
//!   constructor or set on a builder. Branches at hot-path call sites
//!   read the flag values.
//! - **BuildMode** (construction-time): a discriminated value (often an
//!   enum) selected when constructing the DS. Affects the build process
//!   but not in-memory representation.
//!
//! The `HasOptimizationAxes` trait is the umbrella the bench reporter
//! consumes; it returns the structured axes for inclusion in
//! `BenchReport.axes`.
//!
//! See `docs/specs/optimization-standard.md` for the full standard.

use {serde_json::Value, std::collections::BTreeMap};

/// Marker trait for a compile-time layout option (e.g., pointer encoding,
/// hash strategy). Implementing types are typically zero-sized.
pub trait LayoutOption {
    /// Short identifier used in CLI selectors and bench-report axis values.
    /// Must be stable across kermit releases — bench reports embed it.
    const NAME: &'static str;
}

/// Trait for runtime configuration options (boolean/enum flags toggled
/// without monomorphization).
pub trait ConfigOption: Default + Clone {
    /// Serialize the config as a sequence of `(suffix, value)` pairs.
    /// The bench reporter prepends `ds_config_` (or `algo_config_`) to
    /// each suffix when populating the axes map.
    fn axes(&self) -> Vec<(&'static str, Value)>;
}

/// Trait for construction-time build modes (e.g., serial vs parallel,
/// radix-partitioned vs not).
pub trait BuildMode {
    /// String identifier for the bench-report `ds_build_mode` axis.
    /// Format convention: `"<mode>[:<params>]"`, e.g. `"serial"`,
    /// `"parallel:8"`, `"radix:4"`.
    fn axis_value(&self) -> String;
}

/// Umbrella trait consumed by the bench reporter to populate optimization
/// axes in `BenchReport.axes`. A DS or algorithm that adopts the standard
/// implements this; the implementation typically composes results from
/// `LayoutOption::NAME`, `ConfigOption::axes`, and `BuildMode::axis_value`
/// under the correct key prefixes.
///
/// Key naming convention (the bench reporter merges these into
/// `BenchReport.axes`):
/// - Layout: `ds_layout_<dimension>` (e.g., `ds_layout_hasher`)
/// - Config: `ds_config_<flag>` (e.g., `ds_config_singleton_pruning`)
/// - BuildMode: `ds_build_mode`
pub trait HasOptimizationAxes {
    /// Returns the structured axes for inclusion in `BenchReport.axes`.
    fn optimization_axes(&self) -> BTreeMap<String, Value>;
}
