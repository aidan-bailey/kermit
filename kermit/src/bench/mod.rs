//! The `bench` subcommands' measurement layer: what runs under Criterion
//! once the CLI has resolved *which* structure, algorithm and workload.
//!
//! `workload` is the value both `bench run` (from a `benchmarks/*.yml`
//! definition) and `bench join` (from `--relations` / `--query`) hand to
//! the one generic runner in `run`; `ds` is the structure-only runner
//! behind `bench ds`. `main.rs` keeps the clap types and the thin handlers
//! that build these inputs and write the report.

// Task 3 wires `Workload` into the runners; until then it is unused.
#[allow(dead_code)]
pub mod workload;

#[allow(unused_imports)]
pub use workload::{NamedQuery, Workload};
