//! The Join Order Benchmark (JOB) generator (#97).
//!
//! JOB (Leis et al., "How Good Are Query Optimizers, Really?", VLDB 2015) is
//! 113 SQL queries, in 33 templates, over the IMDb dataset. This module turns
//! one template into a kermit benchmark: each alias's filters are applied
//! while the relations are generated, so the Datalog rules join JOB's real
//! tables with JOB's real selectivities.
//!
//! - [`translate`]: SQL text to relation files and Datalog rules. Pure Rust
//!   over `sqlparser`; it reads no data.
//!
//! The design is in `docs/specs/2026-10-15-job-benchmark-design.md`.

pub mod translate;
