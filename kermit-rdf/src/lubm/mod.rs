//! LUBM (Lehigh University Benchmark) on-the-fly generation pipeline.
//!
//! Mirrors `kermit-rdf::driver` but for the LUBM-UBA Java jar. Unlike the
//! WatDiv binary, the LUBM-UBA jar is self-contained and does not require
//! bind-mounted host files, so the sandbox is just a temp directory for
//! output staging — no `bwrap` involvement.
//!
//! Pipeline stages 4–6 (partition, parquet, translate) run after entailment
//! and live in `crate::pipeline`.

pub mod driver;
pub mod entailment;
pub mod pipeline;
pub mod queries;
pub mod sandbox;
