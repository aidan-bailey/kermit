//! The [`Generator`] trait and what a generator returns.

use {
    crate::sink::RelationSink,
    kermit_bench::QueryDefinition,
    kermit_rdf::generator::Target,
    serde::{de::DeserializeOwned, Serialize},
};

/// A benchmark generator that writes its relations directly.
///
/// Implement it, then hand it to [`run`](fn@crate::run), which creates the
/// output directory, calls [`generate`](Self::generate) and writes
/// `benchmark.yml` and `meta.json` around it. Unlike
/// `kermit_rdf::generator::Generator`, which post-processes the output of
/// an external RDF tool, this trait has no tool and no triples: the
/// generator produces tuples and the sink stores them.
pub trait Generator {
    /// `meta.json`'s `kind` (`"h2o"`, `"snap"`, `"job"`).
    const KIND: &'static str;

    /// This generator's own `meta.json` fields, flattened beside the common
    /// ones. It must serialize to a JSON object that uses none of the common
    /// field names ([`GeneratedMeta`](crate::GeneratedMeta)); `run` checks.
    type Extras: Serialize + DeserializeOwned;

    /// Output directory, benchmark name, tag and spec hash. `run` requires
    /// the target's `out_dir` to end in its `bench_name`, which is where
    /// discovery and `materialize` look for the cache.
    fn target(&self) -> Target<'_>;

    /// Writes every relation through `sink` and returns the queries.
    ///
    /// # Errors
    ///
    /// Any error aborts the run before `benchmark.yml` and `meta.json` are
    /// written.
    fn generate(&self, sink: &mut RelationSink) -> anyhow::Result<Generated<Self::Extras>>;
}

/// What [`Generator::generate`] returns besides the relations it wrote.
#[derive(Debug, Clone)]
pub struct Generated<E> {
    /// `benchmark.yml`'s `description`.
    pub description: String,
    /// The benchmark's queries, each with the cardinality it must return
    /// when the generator knows it. Every body predicate must name a
    /// relation the generator wrote.
    pub queries: Vec<QueryDefinition>,
    /// The generator's own `meta.json` fields.
    pub extras: E,
}
