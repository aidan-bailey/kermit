//! LUBM (Lehigh University Benchmark) on-the-fly generation pipeline.
//!
//! Mirrors `kermit-rdf::driver` but for the LUBM-UBA Java jar. Unlike the
//! WatDiv binary, the LUBM-UBA jar is self-contained and does not require
//! bind-mounted host files, so the sandbox is just a temp directory for
//! output staging — no `bwrap` involvement.
//!
//! The post-driver stages (partition, parquet, translate, emit) are the
//! shared `crate::generator::process_artifacts` orchestrator;
//! `crate::lubm::pipeline` supplies the LUBM-specific hooks (entailment as
//! the staging step, the hand-written queries, `LubmMeta`).

pub mod driver;
pub mod entailment;
pub mod pipeline;
pub mod queries;
pub mod sandbox;
