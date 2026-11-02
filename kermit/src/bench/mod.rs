//! The `bench` subcommands' measurement layer: what runs under Criterion
//! once the CLI has resolved *which* structure, algorithm and workload.
//!
//! `workload` is the value `bench run` and `bench probe` (from a
//! `benchmarks/*.yml` definition) and `bench join` (from `--relations` /
//! `--query`) hand to the one generic runner in `run`; `ds` is the
//! structure-only runner behind `bench ds`. `main.rs` keeps the clap types
//! and the thin handlers that build these inputs and write the report.
//!
//! `measure` is the methodology every runner shares: one untimed describe
//! build per subject, then the timed metrics in one fixed order.
//!
//! `gen` is the exception and measures nothing: it holds `bench gen`'s clap
//! subtree and the handlers that write a generated benchmark into the cache,
//! which outgrew `main.rs`.

pub mod ds;
pub mod gen;
pub mod measure;
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
    Build,
    Iteration,
    Space,
    /// Build + K × query in one timed body (`T = build + K × query`), with
    /// K set by `--queries-per-build`. Deliberately NOT in the `--metrics`
    /// default set: a fresh build per Criterion sample
    /// (`BatchSize::PerIteration`) is expensive, and existing invocations
    /// must keep their historical measurement surface.
    EndToEnd,
}

impl Metric {
    /// Whether Criterion times this metric. `space` is exact: it is read off
    /// the describe step, never sampled.
    pub(crate) fn is_timed(self) -> bool { !matches!(self, Metric::Space) }
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

#[cfg(test)]
mod tests {
    use {super::*, std::time::Duration};

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
