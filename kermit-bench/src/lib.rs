//! Benchmark infrastructure for Kermit.
//!
//! Provides synthetic data generation and benchmark workload definitions.
//! [`BenchmarkConfig`](benchmark::BenchmarkConfig) defines the interface each
//! benchmark must implement.

pub mod benchmark;
pub mod benchmarks;
pub mod generation;
