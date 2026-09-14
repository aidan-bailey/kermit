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

pub use workload::Workload;
pub(crate) use {
    ds::dispatch_ds_bench,
    run::{check_sweep_group_directories, dispatch_run_bench, resolve_sweep, RunSettings},
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

/// The directory name Criterion gives a benchmark id on disk: a copy of the
/// private `criterion::report::make_filename_safe` (Criterion 0.8.2), pinned
/// against the real crate by `criterion_directory_name_matches_criterion`.
pub(super) fn criterion_directory_name(id: &str) -> String {
    let mut name = id.replace(['?', '"', '/', '\\', '*', '<', '>', ':', '|', '^'], "_");
    name.truncate(name.floor_char_boundary(CRITERION_MAX_DIRECTORY_NAME_BYTES));
    if cfg!(target_os = "windows") {
        // Windows ignores trailing spaces and case in file names.
        name = name.trim_end().to_lowercase();
    }
    name
}

/// Criterion 0.8.2's `MAX_DIRECTORY_NAME_LEN`.
pub(super) const CRITERION_MAX_DIRECTORY_NAME_BYTES: usize = 64;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn criterion_directory_name_matches_criterion() {
        // Unsafe characters, a separator, and a two-byte `é` at bytes 63..65,
        // straddling the 64-byte cap: every rule `make_filename_safe` applies.
        let group = format!(
            "run:{}/q?/Tree*Trie/{}é/Leapfrog|Triejoin",
            "a".repeat(40),
            "b".repeat(5)
        );
        assert_eq!(group.find('é'), Some(63));
        let out = tempfile::tempdir().unwrap();
        let mut criterion = criterion::Criterion::default()
            .output_directory(out.path())
            .sample_size(10)
            .warm_up_time(Duration::from_millis(1))
            .measurement_time(Duration::from_millis(10));
        let mut bench_group = criterion.benchmark_group(&group);
        bench_group.bench_function("f", |b| b.iter(|| std::hint::black_box(1 + 1)));
        bench_group.finish();

        let on_disk: Vec<String> = std::fs::read_dir(out.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        assert_eq!(on_disk, vec![criterion_directory_name(&group)]);
    }

    #[test]
    fn criterion_directory_name_truncates_on_a_char_boundary() {
        let name = criterion_directory_name(&format!("{}é", "a".repeat(63)));
        assert_eq!(name, "a".repeat(63));
    }
}
