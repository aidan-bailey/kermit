//! Non-RDF benchmark generators for Kermit, and the layer they share.
//!
//! `kermit-rdf` turns the output of external RDF tools into relations. The
//! generators in this crate build their relations themselves (a synthetic
//! schema, a downloaded graph, a database dump) and write them straight to
//! Parquet. What every such generator needs is here once, so that each adds
//! only its own module: `pub mod h2o;`, `pub mod snap;` and `pub mod job;`,
//! added by the issues that introduce them (#95, #96 and #97).
//!
//! The shared layer is:
//!
//! - `Generator` and `Generated`: the trait a generator implements. It names
//!   its output directory, benchmark name and tag, writes its relations through
//!   a `RelationSink`, and returns the queries and its own `meta.json` fields.
//! - `RelationSink`, `RelationWriter` and `RelationRecord`: write each relation
//!   as `<name>.parquet`, hash it, and remember its name, columns, row count
//!   and SHA-256.
//! - `run`: drives a generator and leaves a cache directory that `bench list`,
//!   `bench fetch` and `bench run` already understand.
//! - `GeneratedMeta`: the `meta.json` every generator writes, with the
//!   generator's own fields flattened beside the common ones.
//! - `output_dir` and `OutputDir`: where `bench gen <kind>` puts a benchmark,
//!   as a pure function.
//!
//! # Relation to `kermit_rdf::generator::Generator`
//!
//! `kermit_rdf::generator::Generator` drives an external RDF tool (the
//! `WatDiv` binary, the LUBM jar) and partitions the N-Triples it produces.
//! The `Generator` of this crate has no external tool and no triples: it
//! writes relations directly. The two share a name and, through
//! `kermit_rdf::generator::{Target, GeneratorMeta, write_meta_json}`, the
//! cache layout, but not a trait: `kermit-rdf` cannot host this one,
//! because it must not depend on `kermit-ds`.
//!
//! # The cache contract
//!
//! `run` leaves a directory that discovery, spec-drift detection, `--force`
//! and `bench list` treat like any other generated benchmark. To keep a
//! failed run from passing for a finished one it does, in order:
//!
//! 1. check that the output directory ends in the benchmark name, then create
//!    it and delete any old `meta.json`;
//! 2. let the generator write its relations. Each is written to
//!    `<name>.parquet.part` and renamed once complete, so a relation exists
//!    under its real name only when it is whole;
//! 3. check the generator's extras and queries;
//! 4. write `benchmark.yml`, with every relation's SHA-256;
//! 5. write `meta.json` last. It is the commit marker: a directory without it
//!    is not a cache, whatever else it holds.
//!
//! # The binary
//!
//! The binary wires each non-RDF generator kind in as a `materialize`
//! dispatch arm that calls `run`; the arms arrive with the generators
//! (#95, #96, #97). An arm discards the `GeneratedMeta` that `run` returns,
//! and `bench gen` may print a summary from it.
#![deny(missing_docs)]

mod generator;
pub mod h2o;
mod meta;
mod output_dir;
mod run;
mod sink;
pub mod snap;

pub use {
    generator::{Generated, Generator},
    meta::GeneratedMeta,
    output_dir::{output_dir, OutputDir},
    run::run,
    sink::{RelationRecord, RelationSink, RelationWriter},
};
