//! Execution cells for `bench run`.
//!
//! The CLI exposes an index structure (`-i`) and a join algorithm (`-a`)
//! as two independent selectors, but only three of the six concrete
//! `(IndexStructure, JoinAlgorithm)` pairs are meaningful: the sorted
//! tries pair with [`LeapfrogTriejoin`](kermit_algos::LeapfrogTriejoin)
//! and the hash trie pairs with
//! [`HashTriejoin`](kermit_algos::HashTriejoin). The two families cannot
//! share a `DB` impl (`TrieIterable` vs `HashTrieIterable` — coherence
//! forbids one blanket impl spanning both), so historically the CLI carried
//! two hand-reconciled copies of the benchmark runner, each taking the
//! structure and the algorithm as *separate* parameters. That separation
//! is what let `-i all -a leapfrog-triejoin` run `hash_join` on the
//! `HashTrie` cell and stamp the report `LeapfrogTriejoin` (issue #56).
//!
//! This module replaces the pair of independent values with a single
//! [`Execution`] enum whose variants *are* the valid cells. A report's
//! `data_structure` / `algorithm` axes are derived from the `Execution`
//! that actually ran, so no code path can label a measurement with an
//! algorithm it did not execute. [`ExecutionFamily`] captures the handful
//! of operations that differ between the two families; the benchmark
//! runner in `main.rs` is generic over it.

use {
    crate::options::{HasherChoice, PruningChoice},
    kermit::db::{hash_join, DatabaseEngine, DB},
    kermit_algos::{JoinAlgorithm, JoinQuery, LeapfrogTriejoin, Optimiser},
    kermit_ds::{
        Cardinality, ColumnTrie, ConfigurableRelation, HashTrie, HashTrieConfig, HeapSize,
        IndexStructure, PruningPolicy, Relation, RelationFileExt, RelationHeader, TreeTrie,
    },
    kermit_iters::{HasOptimizationAxes, HashStrategy, TrieIterable},
    std::{
        collections::{BTreeMap, HashMap},
        marker::PhantomData,
        path::Path,
    },
};

/// The sorted-family index structures: every `IndexStructure` that
/// implements `TrieIterable` and therefore joins through the `DB` trait
/// under Leapfrog Triejoin.
///
/// A dedicated enum (rather than reusing [`IndexStructure`]) keeps
/// `HashTrie` out of the sorted arm by construction — the dispatch `match`
/// in `main.rs` cannot accidentally route a hash trie through the LFTJ
/// runner, and the compiler flags any new sorted structure that has not
/// been wired into the runner.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum SortedTrie {
    /// Pointer-based trie (`-i tree-trie`).
    TreeTrie,
    /// Column-oriented trie (`-i column-trie`).
    ColumnTrie,
}

impl SortedTrie {
    /// The `IndexStructure` this sorted trie corresponds to.
    pub fn index_structure(self) -> IndexStructure {
        match self {
            | Self::TreeTrie => IndexStructure::TreeTrie,
            | Self::ColumnTrie => IndexStructure::ColumnTrie,
        }
    }
}

/// Ties a sorted-family relation type to its [`SortedTrie`] label so the
/// runner's report axes are derived from the type it monomorphised over,
/// not from a separately threaded value that could disagree with it.
pub trait SortedTrieRelation:
    Relation + RelationFileExt + TrieIterable + Cardinality + HeapSize
{
    /// The CLI-visible identity of this relation type.
    const KIND: SortedTrie;
}

impl SortedTrieRelation for TreeTrie {
    const KIND: SortedTrie = SortedTrie::TreeTrie;
}

impl SortedTrieRelation for ColumnTrie {
    const KIND: SortedTrie = SortedTrie::ColumnTrie;
}

/// One valid `(index structure, join algorithm)` cell of a `bench run`
/// sweep. Each variant fixes *both* halves of the pair, so an `Execution`
/// cannot describe a combination the CLI is unable to run.
// `Copy` relies on `HashTrieConfig: Copy`; a future Config carrying heap
// data would have to drop it here and clone the cells instead.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Execution {
    /// A sorted trie joined by Leapfrog Triejoin through the `DB` trait.
    TrieLftj(SortedTrie),
    /// `HashTrie<H, P>` joined by Hash Triejoin through [`hash_join`],
    /// with `H` chosen by `--ds-layout-hasher`, `P` by
    /// `--ds-layout-pruning`, and the runtime values by `--ds-config`.
    HashHtj {
        /// The `--ds-layout-hasher` choice `H` was monomorphised from.
        hasher: HasherChoice,
        /// The `--ds-layout-pruning` choice `P` was monomorphised from.
        pruning: PruningChoice,
        /// The `--ds-config` runtime values every relation is built with.
        config: HashTrieConfig,
    },
}

impl Execution {
    /// The only way to obtain an `Execution` from a concrete pair. Returns
    /// `None` for the three incompatible pairs, which the sweep skips.
    pub fn for_pair(
        ds: IndexStructure, algo: JoinAlgorithm, hasher: HasherChoice, pruning: PruningChoice,
        config: HashTrieConfig,
    ) -> Option<Execution> {
        match (ds, algo) {
            | (IndexStructure::TreeTrie, JoinAlgorithm::LeapfrogTriejoin) => {
                Some(Execution::TrieLftj(SortedTrie::TreeTrie))
            },
            | (IndexStructure::ColumnTrie, JoinAlgorithm::LeapfrogTriejoin) => {
                Some(Execution::TrieLftj(SortedTrie::ColumnTrie))
            },
            | (IndexStructure::HashTrie, JoinAlgorithm::HashTriejoin) => Some(Execution::HashHtj {
                hasher,
                pruning,
                config,
            }),
            | (
                IndexStructure::TreeTrie | IndexStructure::ColumnTrie,
                JoinAlgorithm::HashTriejoin,
            )
            | (IndexStructure::HashTrie, JoinAlgorithm::LeapfrogTriejoin) => None,
        }
    }

    /// The index structure this cell runs.
    pub fn index_structure(self) -> IndexStructure {
        match self {
            | Self::TrieLftj(sorted) => sorted.index_structure(),
            | Self::HashHtj {
                ..
            } => IndexStructure::HashTrie,
        }
    }

    /// The join algorithm this cell runs.
    pub fn algorithm(self) -> JoinAlgorithm {
        match self {
            | Self::TrieLftj(_) => JoinAlgorithm::LeapfrogTriejoin,
            | Self::HashHtj {
                ..
            } => JoinAlgorithm::HashTriejoin,
        }
    }
}

/// The result of expanding a `bench run` selector pair: the cells to run,
/// in `structures × algorithms` order, plus the concrete pairs that were
/// dropped as incompatible (reported to the user, never silently lost).
#[derive(Debug, PartialEq, Eq)]
pub struct Sweep {
    /// Valid cells, in the order they will run.
    pub cells: Vec<Execution>,
    /// Incompatible pairs that the expansion skipped.
    pub skipped: Vec<(IndexStructure, JoinAlgorithm)>,
}

impl Sweep {
    /// Expands the cross product of `structures × algorithms` into valid
    /// cells, partitioning off the incompatible pairs. `hasher`,
    /// `pruning` and `config` are attached to every `HashTrie` cell.
    pub fn expand(
        structures: &[IndexStructure], algorithms: &[JoinAlgorithm], hasher: HasherChoice,
        pruning: PruningChoice, config: HashTrieConfig,
    ) -> Sweep {
        let mut cells = Vec::new();
        let mut skipped = Vec::new();
        for &ds in structures {
            for &algo in algorithms {
                match Execution::for_pair(ds, algo, hasher, pruning, config) {
                    | Some(cell) => cells.push(cell),
                    | None => skipped.push((ds, algo)),
                }
            }
        }
        Sweep {
            cells,
            skipped,
        }
    }
}

/// The operations that differ between the sorted and hash join families.
/// Everything else in a `bench run` cell — query parsing, metadata,
/// Criterion group wiring, report assembly — is shared by the generic
/// runner in `main.rs`.
///
/// Both `build` paths must construct the engine the same way: the
/// `end_to_end` metric times [`ExecutionFamily::build_from_tuples`] and
/// reports it as the cost of the build that the `iteration` metric's
/// engine paid via [`ExecutionFamily::build`].
pub trait ExecutionFamily {
    /// The relation type loaded from the benchmark's relation files.
    type Rel: Relation + HeapSize + 'static;
    /// The built, queryable form of a set of relations.
    type Engine;

    /// The cell this family instance runs; source of the report's
    /// `data_structure` / `algorithm` axes.
    fn execution(&self) -> Execution;

    /// Builds one relation from a `(header, tuples)` snapshot, honouring
    /// the family's configuration.
    ///
    /// Every relation the family builds *from a tuple snapshot* routes
    /// through this one site — the `insertion` metric, [`load`](Self::load)
    /// and `HashHtj::build_from_tuples` — so such a measurement can never
    /// build a relation the report's `ds_config_*` axes fail to describe.
    /// `TrieLftj::build_from_tuples` is the exception: it populates a
    /// `DatabaseEngine` through `add_keys_batch` instead, so giving a
    /// sorted structure a Config axis means routing that path here too.
    fn build_relation(&self, header: RelationHeader, tuples: Vec<Vec<usize>>) -> Self::Rel {
        Self::Rel::from_tuples(header, tuples)
    }

    /// Loads one relation file into `Self::Rel`, honouring the family's
    /// configuration: the reader is chosen by extension and the relation
    /// is built through [`build_relation`](Self::build_relation), so no
    /// family needs to override this to pick up its own configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if the extension is neither `csv` nor `parquet`,
    /// or if the reader fails.
    fn load(&self, path: &Path) -> anyhow::Result<Self::Rel> {
        let extension = path.extension().and_then(|s| s.to_str()).unwrap_or("");
        let (header, tuples) = match extension.to_lowercase().as_str() {
            | "csv" => kermit_ds::read_csv(path),
            | "parquet" => kermit_ds::read_parquet(path),
            | _ => anyhow::bail!("Unsupported file extension for {path:?}: '{extension}'"),
        }
        .map_err(|e| anyhow::anyhow!("Failed to load {path:?}: {e}"))?;
        Ok(self.build_relation(header, tuples))
    }

    /// Every stored tuple of `rel`, in the structure's native iteration
    /// order.
    fn tuples(rel: &Self::Rel) -> Vec<Vec<usize>>;

    /// Number of stored tuples in `rel`. Must agree with
    /// `Self::tuples(rel).len()` but should avoid materialising the tuples.
    fn tuple_count(rel: &Self::Rel) -> usize;

    /// Builds the engine from freshly loaded relations, retaining them for
    /// the per-relation metrics (`insertion`, `space`).
    fn build(&self, relations: Vec<Self::Rel>) -> Self::Engine;

    /// Builds an engine from `(header, tuples)` snapshots along the same
    /// path as [`ExecutionFamily::build`]. Used inside the timed
    /// `end_to_end` body, where only [`ExecutionFamily::join`] is needed
    /// afterwards.
    fn build_from_tuples(&self, inputs: Vec<(RelationHeader, Vec<Vec<usize>>)>) -> Self::Engine;

    /// The relations an engine built by [`ExecutionFamily::build`] retains.
    fn relations(engine: &Self::Engine) -> Vec<&Self::Rel>;

    /// Runs `query` against `engine`.
    fn join(&self, engine: &Self::Engine, query: JoinQuery) -> Vec<Vec<usize>>;

    /// The `ds_*` optimization axes emitted by the relation type, merged
    /// into the report's axes. Empty for structures without any.
    fn optimization_axes(rel: &Self::Rel) -> BTreeMap<String, serde_json::Value>;
}

/// Sorted family: `R` under Leapfrog Triejoin through the `DB` trait.
pub struct TrieLftj<R> {
    optimiser: Optimiser,
    /// Passed to the engine as [`DB::name`]; callers use the benchmark
    /// name so downstream tooling can correlate engines with workloads.
    engine_name: String,
    _relation: PhantomData<R>,
}

impl<R> TrieLftj<R> {
    /// Creates the family for benchmark `engine_name` planned by `optimiser`.
    pub fn new(optimiser: Optimiser, engine_name: String) -> Self {
        Self {
            optimiser,
            engine_name,
            _relation: PhantomData,
        }
    }
}

/// The sorted family's engine: the `DB` plus the loaded relations it was
/// built from. `relations` is empty for engines produced by
/// [`ExecutionFamily::build_from_tuples`], which only ever serve `join`.
pub struct TrieLftjEngine<R: Relation> {
    db: DatabaseEngine<R, LeapfrogTriejoin>,
    relations: Vec<R>,
}

impl<R: SortedTrieRelation + 'static> ExecutionFamily for TrieLftj<R> {
    type Engine = TrieLftjEngine<R>;
    type Rel = R;

    fn execution(&self) -> Execution { Execution::TrieLftj(R::KIND) }

    fn tuples(rel: &R) -> Vec<Vec<usize>> { rel.trie_iter().into_iter().collect() }

    fn tuple_count(rel: &R) -> usize { rel.trie_iter().into_iter().count() }

    fn build(&self, relations: Vec<R>) -> Self::Engine {
        // Populate the DB through `add_relation` + `add_keys_batch` rather
        // than `db.add_file`, which would re-read every parquet — the
        // dominant cost on large workloads like WatDiv-scale-1000.
        let inputs = relations
            .iter()
            .map(|r| (r.header().clone(), Self::tuples(r)))
            .collect();
        let mut engine = self.build_from_tuples(inputs);
        engine.relations = relations;
        engine
    }

    fn build_from_tuples(&self, inputs: Vec<(RelationHeader, Vec<Vec<usize>>)>) -> Self::Engine {
        let mut db = DatabaseEngine::<R, LeapfrogTriejoin>::with_optimiser(
            self.engine_name.clone(),
            self.optimiser.instantiate(),
        );
        for (header, tuples) in inputs {
            db.add_relation(header.name(), header.arity());
            db.add_keys_batch(header.name(), tuples);
        }
        TrieLftjEngine {
            db,
            relations: Vec::new(),
        }
    }

    fn relations(engine: &Self::Engine) -> Vec<&R> { engine.relations.iter().collect() }

    fn join(&self, engine: &Self::Engine, query: JoinQuery) -> Vec<Vec<usize>> {
        engine.db.join(query)
    }

    fn optimization_axes(_rel: &R) -> BTreeMap<String, serde_json::Value> { BTreeMap::new() }
}

/// Hash family: `HashTrie<H, P>` under Hash Triejoin through [`hash_join`].
pub struct HashHtj<H, P> {
    hasher: HasherChoice,
    pruning: PruningChoice,
    config: HashTrieConfig,
    optimiser: Box<dyn kermit_algos::QueryOptimiser>,
    _layout: PhantomData<(H, P)>,
}

impl<H, P> HashHtj<H, P> {
    /// Creates the family for the `--ds-layout-hasher` /
    /// `--ds-layout-pruning` choices `hasher` and `pruning` (which must be
    /// the choices `H` and `P` were monomorphised from) and the
    /// `--ds-config` values `config`, planned by `optimiser`.
    pub fn new(
        hasher: HasherChoice, pruning: PruningChoice, config: HashTrieConfig, optimiser: Optimiser,
    ) -> Self {
        Self {
            hasher,
            pruning,
            config,
            optimiser: optimiser.instantiate(),
            _layout: PhantomData,
        }
    }
}

impl<H: HashStrategy + 'static, P: PruningPolicy> ExecutionFamily for HashHtj<H, P> {
    /// Relations keyed by name — the shape [`hash_join`] borrows per query
    /// so a Criterion iteration allocates no wrappers.
    type Engine = HashMap<String, HashTrie<H, P>>;
    type Rel = HashTrie<H, P>;

    fn execution(&self) -> Execution {
        Execution::HashHtj {
            hasher: self.hasher,
            pruning: self.pruning,
            config: self.config,
        }
    }

    fn build_relation(&self, header: RelationHeader, tuples: Vec<Vec<usize>>) -> HashTrie<H, P> {
        HashTrie::<H, P>::from_tuples_with_config(header, self.config, tuples)
    }

    fn tuples(rel: &HashTrie<H, P>) -> Vec<Vec<usize>> { rel.collect_tuples() }

    fn tuple_count(rel: &HashTrie<H, P>) -> usize { rel.collect_tuples().len() }

    fn build(&self, relations: Vec<HashTrie<H, P>>) -> Self::Engine {
        relations
            .into_iter()
            .map(|r| (r.header().name().to_string(), r))
            .collect()
    }

    fn build_from_tuples(&self, inputs: Vec<(RelationHeader, Vec<Vec<usize>>)>) -> Self::Engine {
        inputs
            .into_iter()
            .map(|(header, tuples)| {
                let name = header.name().to_string();
                (name, self.build_relation(header, tuples))
            })
            .collect()
    }

    fn relations(engine: &Self::Engine) -> Vec<&HashTrie<H, P>> { engine.values().collect() }

    fn join(&self, engine: &Self::Engine, query: JoinQuery) -> Vec<Vec<usize>> {
        hash_join::<HashTrie<H, P>, H>(engine, query, self.optimiser.as_ref())
    }

    fn optimization_axes(rel: &HashTrie<H, P>) -> BTreeMap<String, serde_json::Value> {
        rel.optimization_axes()
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        clap::ValueEnum,
        kermit_ds::{NoPruning, SingletonPruning},
    };

    fn all_structures() -> Vec<IndexStructure> { IndexStructure::value_variants().to_vec() }

    fn all_algorithms() -> Vec<JoinAlgorithm> { JoinAlgorithm::value_variants().to_vec() }

    /// The full cross product yields exactly the three valid cells and
    /// skips the other three — the `-i all -a all` acceptance criterion.
    #[test]
    fn sweep_of_full_cross_product_yields_exactly_three_cells() {
        let sweep = Sweep::expand(
            &all_structures(),
            &all_algorithms(),
            HasherChoice::Sip,
            PruningChoice::Off,
            HashTrieConfig::default(),
        );
        assert_eq!(sweep.cells.len(), 3, "{sweep:?}");
        assert_eq!(sweep.skipped.len(), 3, "{sweep:?}");
        let ran: Vec<(IndexStructure, JoinAlgorithm)> = sweep
            .cells
            .iter()
            .map(|c| (c.index_structure(), c.algorithm()))
            .collect();
        assert!(ran.contains(&(IndexStructure::TreeTrie, JoinAlgorithm::LeapfrogTriejoin)));
        assert!(ran.contains(&(IndexStructure::ColumnTrie, JoinAlgorithm::LeapfrogTriejoin)));
        assert!(ran.contains(&(IndexStructure::HashTrie, JoinAlgorithm::HashTriejoin)));
    }

    /// Regression for issue #56: `-i all -a leapfrog-triejoin` must not
    /// produce a HashTrie cell at all, let alone one labelled LFTJ.
    #[test]
    fn sweep_all_structures_with_lftj_skips_hash_trie() {
        let sweep = Sweep::expand(
            &all_structures(),
            &[JoinAlgorithm::LeapfrogTriejoin],
            HasherChoice::Sip,
            PruningChoice::Off,
            HashTrieConfig::default(),
        );
        assert!(sweep
            .cells
            .iter()
            .all(|c| c.index_structure() != IndexStructure::HashTrie));
        assert_eq!(sweep.skipped, vec![(
            IndexStructure::HashTrie,
            JoinAlgorithm::LeapfrogTriejoin
        )]);
    }

    /// A concrete incompatible pair expands to no cells; the CLI turns
    /// that into a usage error.
    #[test]
    fn sweep_of_incompatible_concrete_pair_is_empty() {
        let sweep = Sweep::expand(
            &[IndexStructure::HashTrie],
            &[JoinAlgorithm::LeapfrogTriejoin],
            HasherChoice::Sip,
            PruningChoice::Off,
            HashTrieConfig::default(),
        );
        assert!(sweep.cells.is_empty());
        assert_eq!(sweep.skipped.len(), 1);
    }

    /// Every cell reports the pair it was built from, and the hasher
    /// choice rides along on the hash cell.
    #[test]
    fn execution_axes_round_trip_through_for_pair() {
        for ds in all_structures() {
            for algo in all_algorithms() {
                if let Some(cell) = Execution::for_pair(
                    ds,
                    algo,
                    HasherChoice::Fxhash,
                    PruningChoice::Off,
                    HashTrieConfig::default(),
                ) {
                    assert_eq!(cell.index_structure(), ds);
                    assert_eq!(cell.algorithm(), algo);
                }
            }
        }
        assert_eq!(
            Execution::for_pair(
                IndexStructure::HashTrie,
                JoinAlgorithm::HashTriejoin,
                HasherChoice::Fxhash,
                PruningChoice::Off,
                HashTrieConfig::default(),
            ),
            Some(Execution::HashHtj {
                hasher: HasherChoice::Fxhash,
                pruning: PruningChoice::Off,
                config: HashTrieConfig::default(),
            })
        );
    }

    /// The family's reported execution comes from its type, so the label
    /// can never disagree with the code path that ran.
    #[test]
    fn families_report_their_own_execution() {
        let tree = TrieLftj::<TreeTrie>::new(Optimiser::Lexicographic, "t".into());
        assert_eq!(tree.execution(), Execution::TrieLftj(SortedTrie::TreeTrie));
        let column = TrieLftj::<ColumnTrie>::new(Optimiser::Lexicographic, "t".into());
        assert_eq!(
            column.execution(),
            Execution::TrieLftj(SortedTrie::ColumnTrie)
        );
        let config = HashTrieConfig {
            load_factor: kermit_ds::LoadFactor::percent(50).unwrap(),
        };
        let hash = HashHtj::<kermit_iters::FxHashStrategy, NoPruning>::new(
            HasherChoice::Fxhash,
            PruningChoice::Off,
            config,
            Optimiser::Lexicographic,
        );
        assert_eq!(hash.execution(), Execution::HashHtj {
            hasher: HasherChoice::Fxhash,
            pruning: PruningChoice::Off,
            config,
        });
        assert_eq!(hash.execution().algorithm(), JoinAlgorithm::HashTriejoin);
    }

    /// The config reaches the relations the family builds, so the report's
    /// `ds_config_*` axes describe the structure that actually ran.
    #[test]
    fn hash_family_builds_relations_with_its_config() {
        let config = HashTrieConfig {
            load_factor: kermit_ds::LoadFactor::percent(50).unwrap(),
        };
        let family = HashHtj::<kermit_iters::SipHashStrategy, NoPruning>::new(
            HasherChoice::Sip,
            PruningChoice::Off,
            config,
            Optimiser::Lexicographic,
        );
        let header = RelationHeader::new("r", vec!["a".to_string(), "b".to_string()]);
        let engine = family.build_from_tuples(vec![(header, vec![vec![1, 2]])]);
        let rel = &engine["r"];
        assert_eq!(
            HashHtj::<kermit_iters::SipHashStrategy, NoPruning>::optimization_axes(rel)
                .get("ds_config_load_factor"),
            Some(&serde_json::Value::from(0.5_f64))
        );
    }

    /// `load` builds through `build_relation`, so a relation read off
    /// disk carries the family's configuration too.
    #[test]
    fn hash_family_load_honours_its_config() {
        let config = HashTrieConfig {
            load_factor: kermit_ds::LoadFactor::percent(50).unwrap(),
        };
        let family = HashHtj::<kermit_iters::SipHashStrategy, NoPruning>::new(
            HasherChoice::Sip,
            PruningChoice::Off,
            config,
            Optimiser::Lexicographic,
        );
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("r.csv");
        std::fs::write(&path, "a,b\n1,2\n").expect("write csv");
        let rel = family.load(&path).expect("load");
        assert_eq!(*rel.config(), config);
        assert_eq!(
            HashHtj::<kermit_iters::SipHashStrategy, NoPruning>::optimization_axes(&rel)
                .get("ds_config_load_factor"),
            Some(&serde_json::Value::from(0.5_f64))
        );
    }

    /// The per-relation `insertion` metric builds through
    /// [`ExecutionFamily::build_relation`], so it must honour the config
    /// too — otherwise it would time an unconfigured build under a report
    /// labelled with the configured axes.
    #[test]
    fn hash_family_build_relation_honours_its_config() {
        let config = HashTrieConfig {
            load_factor: kermit_ds::LoadFactor::percent(50).unwrap(),
        };
        let family = HashHtj::<kermit_iters::SipHashStrategy, NoPruning>::new(
            HasherChoice::Sip,
            PruningChoice::Off,
            config,
            Optimiser::Lexicographic,
        );
        let header = RelationHeader::new("r", vec!["a".to_string(), "b".to_string()]);
        let rel = family.build_relation(header, vec![vec![1, 2]]);
        assert_eq!(*rel.config(), config);
        assert_eq!(
            HashHtj::<kermit_iters::SipHashStrategy, NoPruning>::optimization_axes(&rel)
                .get("ds_config_load_factor"),
            Some(&serde_json::Value::from(0.5_f64))
        );
    }

    /// The pruning Layout reaches the relations the family builds, so a
    /// report's `ds_layout_pruning` axis describes the structure that ran.
    #[test]
    fn pruned_family_reports_the_pruning_layout() {
        let family = HashHtj::<kermit_iters::SipHashStrategy, SingletonPruning>::new(
            HasherChoice::Sip,
            PruningChoice::On,
            HashTrieConfig::default(),
            Optimiser::Lexicographic,
        );
        let header = RelationHeader::new("r", vec!["a".to_string(), "b".to_string()]);
        let rel = family.build_relation(header, vec![vec![1, 2]]);
        assert_eq!(
            HashHtj::<kermit_iters::SipHashStrategy, SingletonPruning>::optimization_axes(&rel)
                .get("ds_layout_pruning"),
            Some(&serde_json::Value::String("on".into()))
        );
    }
}
