//! The `bench` subcommands' measurement layer: what runs under Criterion
//! once the CLI has resolved *which* structure, algorithm and workload.
//!
//! `workload` is the value both `bench run` (from a `benchmarks/*.yml`
//! definition) and `bench join` (from `--relations` / `--query`) hand to
//! the one generic runner in `run`; `ds` is the structure-only runner
//! behind `bench ds`. `main.rs` keeps the clap types and the thin handlers
//! that build these inputs and write the report.

use {
    crate::{
        bench_report::{CriterionGroupRef, ReportMetric},
        measurement, BenchArgs,
    },
    kermit_ds::HeapSize,
    std::time::Duration,
};

pub mod ds;
pub mod run;
pub mod workload;

// `NamedQuery` is unused as an import until Task 4 routes `bench join`
// through `Workload::adhoc`; the type itself is already load-bearing as
// the element type of `Workload::queries`.
#[allow(unused_imports)]
pub use workload::{NamedQuery, Workload};
pub(crate) use {
    ds::dispatch_ds_bench,
    run::{dispatch_run_bench, resolve_sweep},
};

/// A measurement `bench ds` / `bench run` can record.
#[derive(Copy, Clone, Debug, PartialEq, clap::ValueEnum)]
pub enum Metric {
    Insertion,
    Iteration,
    Space,
    /// Build + K × query in one timed body (`T = build + K × query`), with
    /// K set by `--queries-per-build`. Deliberately NOT in the `--metrics`
    /// default set: a fresh build per Criterion sample
    /// (`BatchSize::PerIteration`) is expensive, and existing invocations
    /// must keep their historical measurement surface.
    EndToEnd,
}

pub(super) fn build_time_criterion(args: &BenchArgs) -> criterion::Criterion {
    criterion::Criterion::default()
        .sample_size(args.sample_size)
        .measurement_time(Duration::from_secs(args.measurement_time))
        .warm_up_time(Duration::from_secs(args.warm_up_time))
}

pub(super) fn build_space_criterion(
    args: &BenchArgs,
) -> criterion::Criterion<measurement::SpaceMeasurement> {
    criterion::Criterion::default()
        .with_measurement(measurement::SpaceMeasurement)
        .sample_size(args.sample_size)
        .measurement_time(Duration::from_secs(args.measurement_time))
        .warm_up_time(Duration::from_secs(args.warm_up_time))
}

/// Adds a single space-metric `bench_function` to `group` measuring
/// `relation`'s heap size in bytes. Centralises the `iter_custom`
/// calibration-trap workaround (see [`measurement::SpaceMeasurement`]
/// docs for the full explanation): an O(N) `heap_size_bytes()` call
/// inside the loop is required, and `black_box` is required to defeat
/// LICM. Without these, Criterion's wall-clock warm-up makes `iters`
/// ramp toward `u64::MAX` and the `usize` math saturates.
///
/// Returns the [`CriterionGroupRef`] the caller should append to its
/// report's `criterion_groups`.
pub(super) fn add_space_bench<R>(
    group: &mut criterion::BenchmarkGroup<'_, measurement::SpaceMeasurement>, group_name: &str,
    function: String, relation: &R,
) -> CriterionGroupRef
where
    R: HeapSize,
{
    debug_assert!(
        std::hint::black_box(relation).heap_size_bytes() > 0,
        "SpaceMeasurement requires O(N) work inside iter_custom; passing a zero-cost HeapSize \
         stub re-triggers Criterion's iters-toward-u64::MAX calibration trap. See CLAUDE.md → \
         Space benchmarks gotcha."
    );
    group.bench_function(&function, |b| {
        b.iter_custom(|iters| {
            let mut total = 0usize;
            for _ in 0..iters {
                total = total.saturating_add(std::hint::black_box(relation).heap_size_bytes());
            }
            total
        });
    });
    CriterionGroupRef {
        group: group_name.to_string(),
        function,
        metric: ReportMetric::Space,
    }
}
