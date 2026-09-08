//! Execution cells for `bench run` and `bench ds`.
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
    crate::HasherChoice,
    kermit::db::{hash_join, DatabaseEngine, DB},
    kermit_algos::{JoinAlgorithm, JoinQuery, LeapfrogTriejoin, Optimiser},
    kermit_ds::{
        Cardinality, ColumnTrie, HashTrie, HeapSize, IndexStructure, Relation, RelationFileExt,
        RelationHeader, TreeTrie,
    },
    kermit_iters::{HasOptimizationAxes, HashStrategy, TrieIterable},
    std::{
        collections::{BTreeMap, HashMap},
        marker::PhantomData,
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
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Execution {
    /// A sorted trie joined by Leapfrog Triejoin through the `DB` trait.
    TrieLftj(SortedTrie),
    /// `HashTrie<H>` joined by Hash Triejoin through
    /// [`hash_join`], with `H` chosen by `--ds-layout-hasher`.
    HashHtj(HasherChoice),
}

impl Execution {
    /// The only way to obtain an `Execution` from a concrete pair. Returns
    /// `None` for the three incompatible pairs, which the sweep skips.
    pub fn for_pair(
        ds: IndexStructure, algo: JoinAlgorithm, hasher: HasherChoice,
    ) -> Option<Execution> {
        match (ds, algo) {
            | (IndexStructure::TreeTrie, JoinAlgorithm::LeapfrogTriejoin) => {
                Some(Execution::TrieLftj(SortedTrie::TreeTrie))
            },
            | (IndexStructure::ColumnTrie, JoinAlgorithm::LeapfrogTriejoin) => {
                Some(Execution::TrieLftj(SortedTrie::ColumnTrie))
            },
            | (IndexStructure::HashTrie, JoinAlgorithm::HashTriejoin) => {
                Some(Execution::HashHtj(hasher))
            },
            | (
                IndexStructure::TreeTrie | IndexStructure::ColumnTrie,
                JoinAlgorithm::HashTriejoin,
            )
            | (IndexStructure::HashTrie, JoinAlgorithm::LeapfrogTriejoin) => None,
        }
    }

    /// The cell for a structure selected on its own, as `bench ds` does
    /// (it takes no `--algorithm` flag). Every structure has exactly one
    /// compatible algorithm, so this is total: it is
    /// [`Execution::for_pair`] with that algorithm filled in, and the two
    /// are pinned to agree by `for_structure_agrees_with_for_pair`.
    pub fn for_structure(ds: IndexStructure, hasher: HasherChoice) -> Execution {
        match ds {
            | IndexStructure::TreeTrie => Execution::TrieLftj(SortedTrie::TreeTrie),
            | IndexStructure::ColumnTrie => Execution::TrieLftj(SortedTrie::ColumnTrie),
            | IndexStructure::HashTrie => Execution::HashHtj(hasher),
        }
    }

    /// The index structure this cell runs.
    pub fn index_structure(self) -> IndexStructure {
        match self {
            | Self::TrieLftj(sorted) => sorted.index_structure(),
            | Self::HashHtj(_) => IndexStructure::HashTrie,
        }
    }

    /// The join algorithm this cell runs.
    pub fn algorithm(self) -> JoinAlgorithm {
        match self {
            | Self::TrieLftj(_) => JoinAlgorithm::LeapfrogTriejoin,
            | Self::HashHtj(_) => JoinAlgorithm::HashTriejoin,
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
    /// cells, partitioning off the incompatible pairs. `hasher` is
    /// attached to every `HashTrie` cell.
    pub fn expand(
        structures: &[IndexStructure], algorithms: &[JoinAlgorithm], hasher: HasherChoice,
    ) -> Sweep {
        let mut cells = Vec::new();
        let mut skipped = Vec::new();
        for &ds in structures {
            for &algo in algorithms {
                match Execution::for_pair(ds, algo, hasher) {
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
    type Rel: Relation + RelationFileExt + HeapSize + 'static;
    /// The built, queryable form of a set of relations.
    type Engine;

    /// The cell this family instance runs; source of the report's
    /// `data_structure` / `algorithm` axes.
    fn execution(&self) -> Execution;

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

/// Hash family: `HashTrie<H>` under Hash Triejoin through [`hash_join`].
pub struct HashHtj<H> {
    hasher: HasherChoice,
    optimiser: Box<dyn kermit_algos::QueryOptimiser>,
    _strategy: PhantomData<H>,
}

impl<H> HashHtj<H> {
    /// Creates the family for the `--ds-layout-hasher` choice `hasher`
    /// (which must be the choice `H` was monomorphised from) planned by
    /// `optimiser`.
    pub fn new(hasher: HasherChoice, optimiser: Optimiser) -> Self {
        Self {
            hasher,
            optimiser: optimiser.instantiate(),
            _strategy: PhantomData,
        }
    }
}

impl<H: HashStrategy + 'static> ExecutionFamily for HashHtj<H> {
    /// Relations keyed by name — the shape [`hash_join`] borrows per query
    /// so a Criterion iteration allocates no wrappers.
    type Engine = HashMap<String, HashTrie<H>>;
    type Rel = HashTrie<H>;

    fn execution(&self) -> Execution { Execution::HashHtj(self.hasher) }

    fn tuples(rel: &HashTrie<H>) -> Vec<Vec<usize>> { rel.collect_tuples() }

    fn tuple_count(rel: &HashTrie<H>) -> usize { rel.collect_tuples().len() }

    fn build(&self, relations: Vec<HashTrie<H>>) -> Self::Engine {
        relations
            .into_iter()
            .map(|r| (r.header().name().to_string(), r))
            .collect()
    }

    fn build_from_tuples(&self, inputs: Vec<(RelationHeader, Vec<Vec<usize>>)>) -> Self::Engine {
        inputs
            .into_iter()
            .map(|(header, tuples)| {
                (
                    header.name().to_string(),
                    HashTrie::<H>::from_tuples(header, tuples),
                )
            })
            .collect()
    }

    fn relations(engine: &Self::Engine) -> Vec<&HashTrie<H>> { engine.values().collect() }

    fn join(&self, engine: &Self::Engine, query: JoinQuery) -> Vec<Vec<usize>> {
        hash_join::<HashTrie<H>, H>(engine, query, self.optimiser.as_ref())
    }

    fn optimization_axes(rel: &HashTrie<H>) -> BTreeMap<String, serde_json::Value> {
        rel.optimization_axes()
    }
}

#[cfg(test)]
mod tests {
    use {super::*, clap::ValueEnum};

    fn all_structures() -> Vec<IndexStructure> { IndexStructure::value_variants().to_vec() }

    fn all_algorithms() -> Vec<JoinAlgorithm> { JoinAlgorithm::value_variants().to_vec() }

    /// The full cross product yields exactly the three valid cells and
    /// skips the other three — the `-i all -a all` acceptance criterion.
    #[test]
    fn sweep_of_full_cross_product_yields_exactly_three_cells() {
        let sweep = Sweep::expand(&all_structures(), &all_algorithms(), HasherChoice::Sip);
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
                if let Some(cell) = Execution::for_pair(ds, algo, HasherChoice::Fxhash) {
                    assert_eq!(cell.index_structure(), ds);
                    assert_eq!(cell.algorithm(), algo);
                }
            }
        }
        assert_eq!(
            Execution::for_pair(
                IndexStructure::HashTrie,
                JoinAlgorithm::HashTriejoin,
                HasherChoice::Fxhash
            ),
            Some(Execution::HashHtj(HasherChoice::Fxhash))
        );
    }

    /// `for_structure` must name the one cell `for_pair` accepts for that
    /// structure — otherwise `bench ds` and `bench run` could disagree on
    /// which family a structure belongs to.
    #[test]
    fn for_structure_agrees_with_for_pair() {
        for ds in all_structures() {
            for hasher in [HasherChoice::Sip, HasherChoice::Fxhash] {
                let cell = Execution::for_structure(ds, hasher);
                assert_eq!(cell.index_structure(), ds);
                assert_eq!(
                    Execution::for_pair(ds, cell.algorithm(), hasher),
                    Some(cell)
                );
            }
        }
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
        let hash = HashHtj::<kermit_iters::FxHashStrategy>::new(
            HasherChoice::Fxhash,
            Optimiser::Lexicographic,
        );
        assert_eq!(hash.execution(), Execution::HashHtj(HasherChoice::Fxhash));
        assert_eq!(hash.execution().algorithm(), JoinAlgorithm::HashTriejoin);
    }
}
