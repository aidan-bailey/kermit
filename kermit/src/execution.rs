//! Execution cells for `bench run` and `bench ds`.
//!
//! The CLI exposes an index structure (`-i`) and a join algorithm (`-a`)
//! as two independent selectors, but only three of the six concrete
//! `(IndexStructure, JoinAlgorithm)` pairs are meaningful: the sorted
//! tries pair with [`LeapfrogTriejoin`](kermit_algos::LeapfrogTriejoin)
//! and the hash trie pairs with
//! [`HashTriejoin`](kermit_algos::HashTriejoin). The two families run
//! through separate join entry points ([`lftj_join_for_each`] vs
//! [`hash_join_for_each`]), and historically the CLI carried two
//! hand-reconciled copies of the benchmark runner, each taking the structure
//! and the algorithm as *separate* parameters. That separation
//! is what let `-i all -a leapfrog-triejoin` run `hash_join` on the
//! `HashTrie` cell and stamp the report `LeapfrogTriejoin` (issue #56).
//!
//! This module replaces the pair of independent values with a single
//! [`Execution`] enum whose variants *are* the valid cells. A report's
//! `data_structure` / `algorithm` axes are derived from the `Execution`
//! that actually ran, so no code path can label a measurement with an
//! algorithm it did not execute. [`RelationFamily`] captures the handful
//! of relation-level operations that differ between the two families and
//! [`ExecutionFamily`] the join-level ones on top; `bench ds` and
//! `bench run` (`bench/ds.rs`, `bench/run.rs`) are generic over one each.

use {
    crate::options::{
        expansion_of, hasher_of, pruning_of, seek_of, DsChoices, ExpansionChoice, HasherChoice,
        PruningChoice, SeekChoice,
    },
    kermit::db::{
        hash_join_for_each, index_header, lftj_join_for_each, Database, HashFamily, JoinError,
        SortedFamily,
    },
    kermit_algos::{IndexSpec, JoinAlgorithm, JoinQuery, LeapfrogTriejoin, Planner},
    kermit_ds::{
        BuildModeRelation, Cardinality, ColumnTrie, ColumnTrieBuildMode, ExpansionPolicy, HashTrie,
        HashTrieBuildMode, HashTrieConfig, HeapSize, IndexStructure, PruningPolicy, Relation,
        RelationFileExt, RelationHeader, SeekStrategy, TreeTrie, TreeTrieBuildMode,
    },
    kermit_iters::{
        BuildMode, HasOptimizationAxes, HashStrategy, TrieIterable, TrieIteratorWrapper,
    },
    std::{collections::BTreeMap, marker::PhantomData, path::Path},
};

/// The sorted-family index structures: every `IndexStructure` that
/// implements `TrieIterable` and therefore joins through [`lftj_join_for_each`]
/// under Leapfrog Triejoin.
///
/// A dedicated enum (rather than reusing [`IndexStructure`]) keeps
/// `HashTrie` out of the sorted arm by construction — the dispatch `match`
/// in `bench/run.rs` cannot accidentally route a hash trie through the LFTJ
/// runner, and the compiler flags any new sorted structure that has not
/// been wired into the runner.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum SortedTrie {
    /// Pointer-based trie (`-i tree-trie`), seeking with `seek` and built by
    /// the `--ds-build` mode `build`.
    TreeTrie {
        /// The `--ds-layout-seek` choice, in the two roles of
        /// `Execution::HashHtj`'s Layout fields: the request that selects
        /// `S`, and the label re-derived from `S`.
        seek: SeekChoice,
        /// The `--ds-build` mode every relation is built with.
        build: TreeTrieBuildMode,
    },
    /// Column-oriented trie (`-i column-trie`), seeking with `seek` and
    /// built by the `--ds-build` mode `build`.
    ColumnTrie {
        /// As on [`SortedTrie::TreeTrie`].
        seek: SeekChoice,
        /// The `--ds-build` mode every relation is built with.
        build: ColumnTrieBuildMode,
    },
}

impl SortedTrie {
    /// The `IndexStructure` this sorted trie corresponds to.
    pub fn index_structure(self) -> IndexStructure {
        match self {
            | Self::TreeTrie {
                ..
            } => IndexStructure::TreeTrie,
            | Self::ColumnTrie {
                ..
            } => IndexStructure::ColumnTrie,
        }
    }
}

/// Ties a sorted-family relation type to its [`SortedTrie`] label and its
/// build modes, so the runner's report axes are derived from the type it
/// monomorphised over and the mode it built with, not from a separately
/// threaded value that could disagree with them. Its Layout axes come from
/// the relation itself (`HasOptimizationAxes`), its build mode from the
/// family.
pub trait SortedTrieRelation:
    Relation + RelationFileExt + TrieIterable + Cardinality + HeapSize + HasOptimizationAxes
{
    /// How this structure can be built from its tuples: `()` for a
    /// structure with a single build process.
    type BuildMode: Copy + Default;

    /// The CLI-visible identity of this relation type built by `build`.
    fn kind(build: Self::BuildMode) -> SortedTrie;

    /// Builds one relation from `tuples` by `build`.
    fn build_with(header: RelationHeader, build: Self::BuildMode, tuples: Vec<Vec<usize>>) -> Self;

    /// The `ds_build_mode` axis of relations built by `build`. The build
    /// leaves no trace in the built structure, so this — not the relation —
    /// reports it. Empty for a structure with a single build process.
    fn build_mode_axes(build: Self::BuildMode) -> BTreeMap<String, serde_json::Value>;
}

impl<S: SeekStrategy> SortedTrieRelation for TreeTrie<S> {
    type BuildMode = TreeTrieBuildMode;

    fn kind(build: TreeTrieBuildMode) -> SortedTrie {
        SortedTrie::TreeTrie {
            seek: seek_of::<S>(),
            build,
        }
    }

    fn build_with(
        header: RelationHeader, build: TreeTrieBuildMode, tuples: Vec<Vec<usize>>,
    ) -> Self {
        Self::from_tuples_with_build_mode(header, build, tuples)
    }

    /// Every `TreeTrie` report says which build made it, so kermit-lab can
    /// read a `TreeTrie` row *without* the axis as the serial build, the
    /// only one before #94.
    fn build_mode_axes(build: TreeTrieBuildMode) -> BTreeMap<String, serde_json::Value> {
        BTreeMap::from([(
            "ds_build_mode".to_string(),
            serde_json::Value::from(build.axis_value()),
        )])
    }
}

impl<S: SeekStrategy> SortedTrieRelation for ColumnTrie<S> {
    type BuildMode = ColumnTrieBuildMode;

    fn kind(build: ColumnTrieBuildMode) -> SortedTrie {
        SortedTrie::ColumnTrie {
            seek: seek_of::<S>(),
            build,
        }
    }

    fn build_with(
        header: RelationHeader, build: ColumnTrieBuildMode, tuples: Vec<Vec<usize>>,
    ) -> Self {
        Self::from_tuples_with_build_mode(header, build, tuples)
    }

    /// Every `ColumnTrie` report says which build made it, so kermit-lab can
    /// read a `ColumnTrie` row *without* the axis as the pre-#84 incremental
    /// build.
    fn build_mode_axes(build: ColumnTrieBuildMode) -> BTreeMap<String, serde_json::Value> {
        BTreeMap::from([(
            "ds_build_mode".to_string(),
            serde_json::Value::from(build.axis_value()),
        )])
    }
}

/// One valid `(index structure, join algorithm)` cell of a `bench run`
/// sweep. Each variant fixes *both* halves of the pair, so an `Execution`
/// cannot describe a combination the CLI is unable to run.
// `Copy` relies on `HashTrieConfig` and the build modes (`TreeTrieBuildMode`,
// `ColumnTrieBuildMode`, `HashTrieBuildMode`) being `Copy`; a future Config or
// BuildMode carrying heap data would have to drop it here and clone the cells
// instead.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Execution {
    /// A sorted trie joined by Leapfrog Triejoin through
    /// [`lftj_join_for_each`].
    TrieLftj(SortedTrie),
    /// `HashTrie<H, P, E>` joined by Hash Triejoin through
    /// [`hash_join_for_each`], with `H` chosen by `--ds-layout-hasher`, `P`
    /// by `--ds-layout-pruning`, `E` by `--ds-layout-expansion`, and the
    /// runtime values by `--ds-config`, built by the `--ds-build` mode
    /// `build`.
    HashHtj {
        /// The `--ds-layout-hasher` choice. Built by
        /// [`Execution::for_pair`] it is the *request* — the CLI choice
        /// that will pick the `H` to monomorphise over; reported by
        /// [`HashHtj::execution`] it is re-derived from that `H`, so the
        /// two agree by construction rather than by discipline.
        hasher: HasherChoice,
        /// The `--ds-layout-pruning` choice, in the same two roles: the
        /// request that selects `P`, and the label re-derived from `P`.
        pruning: PruningChoice,
        /// The `--ds-layout-expansion` choice, in the same two roles as
        /// `hasher` and `pruning`.
        expansion: ExpansionChoice,
        /// The `--ds-config` runtime values every relation is built with.
        config: HashTrieConfig,
        /// The `--ds-build` mode every relation is built with.
        build: HashTrieBuildMode,
    },
}

impl Execution {
    /// The only way to obtain an `Execution` from a concrete pair. Returns
    /// `None` for the three incompatible pairs, which the sweep skips.
    /// `choices` reach the cells that have each axis: the hash-trie cell's
    /// hasher, pruning, expansion and config, both sorted cells' seek
    /// strategy, and each structure's build mode.
    pub fn for_pair(
        ds: IndexStructure, algo: JoinAlgorithm, choices: DsChoices,
    ) -> Option<Execution> {
        let DsChoices {
            hasher,
            pruning,
            expansion,
            seek,
            config,
            build,
        } = choices;
        match (ds, algo) {
            | (IndexStructure::TreeTrie, JoinAlgorithm::LeapfrogTriejoin) => {
                Some(Execution::TrieLftj(SortedTrie::TreeTrie {
                    seek,
                    build: build.tree_trie,
                }))
            },
            | (IndexStructure::ColumnTrie, JoinAlgorithm::LeapfrogTriejoin) => {
                Some(Execution::TrieLftj(SortedTrie::ColumnTrie {
                    seek,
                    build: build.column_trie,
                }))
            },
            | (IndexStructure::HashTrie, JoinAlgorithm::HashTriejoin) => Some(Execution::HashHtj {
                hasher,
                pruning,
                expansion,
                config,
                build: build.hash_trie,
            }),
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
    pub fn for_structure(ds: IndexStructure, choices: DsChoices) -> Execution {
        let DsChoices {
            hasher,
            pruning,
            expansion,
            seek,
            config,
            build,
        } = choices;
        match ds {
            | IndexStructure::TreeTrie => Execution::TrieLftj(SortedTrie::TreeTrie {
                seek,
                build: build.tree_trie,
            }),
            | IndexStructure::ColumnTrie => Execution::TrieLftj(SortedTrie::ColumnTrie {
                seek,
                build: build.column_trie,
            }),
            | IndexStructure::HashTrie => Execution::HashHtj {
                hasher,
                pruning,
                expansion,
                config,
                build: build.hash_trie,
            },
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
    /// cells, partitioning off the incompatible pairs. `choices` reach the
    /// cells that have each axis (see [`Execution::for_pair`]).
    pub fn expand(
        structures: &[IndexStructure], algorithms: &[JoinAlgorithm], choices: DsChoices,
    ) -> Sweep {
        let mut cells = Vec::new();
        let mut skipped = Vec::new();
        for &ds in structures {
            for &algo in algorithms {
                match Execution::for_pair(ds, algo, choices) {
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

/// The relation-facing half of a family: how to load a structure's
/// relations, recover their tuples and label them. This is everything
/// `bench ds` needs — it measures a structure on its own, joins nothing,
/// and so bounds on this trait alone. The join-facing half is
/// [`ExecutionFamily`], a subtrait.
///
/// Implemented by the two marker types [`SortedTrieFamily`] and
/// [`HashTrieFamily`], which `bench ds` constructs directly, and by the
/// join families [`TrieLftj`] / [`HashHtj`], which delegate to an embedded
/// marker so the two paths cannot disagree about a structure.
pub trait RelationFamily {
    /// The relation type loaded from the benchmark's relation files.
    type Rel: Relation + HeapSize + 'static;

    /// The cell this family instance runs; source of the report's
    /// `data_structure` / `algorithm` axes.
    fn execution(&self) -> Execution;

    /// Builds one relation from a header and its tuples, honouring the
    /// family's configuration and build mode.
    ///
    /// Every relation the family builds *from tuples* routes through this
    /// one site — the `insertion` metric, [`load`](Self::load),
    /// [`load_with_tuples`](Self::load_with_tuples), and both families'
    /// [`build_from_tuples`] — so such a measurement can never build a
    /// relation the report's `ds_config_*` / `ds_build_mode` axes fail to
    /// describe. Required, with no default, so no family can silently fall
    /// back to `Relation::from_tuples` and build with a configuration or
    /// mode its report does not name.
    ///
    /// [`build_from_tuples`]: ExecutionFamily::build_from_tuples
    fn build_relation(&self, header: RelationHeader, tuples: Vec<Vec<usize>>) -> Self::Rel;

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
        let (header, tuples) = read_relation(path)?;
        Ok(self.build_relation(header, tuples))
    }

    /// Loads one relation file like [`load`](Self::load), and also returns
    /// the tuples it was built from, in the order the reader produced them.
    ///
    /// These are the input of every metric that rebuilds a relation
    /// (`insertion`, `end_to_end`). A structure's own iteration order is not
    /// neutral input — it is the sorted tries' best case, and it made the
    /// hash trie's build quadratic (issue #66) — whereas file order is the
    /// same for every structure and every Layout, so all of them build from
    /// identical input.
    ///
    /// # Errors
    ///
    /// As [`load`](Self::load).
    fn load_with_tuples(&self, path: &Path) -> anyhow::Result<(Self::Rel, Vec<Vec<usize>>)> {
        let (header, tuples) = read_relation(path)?;
        Ok((self.build_relation(header, tuples.clone()), tuples))
    }

    /// Lends every stored tuple of `rel` to `visit`, in the structure's
    /// native iteration order. Each slice is borrowed for that call only,
    /// so the walk never materialises the relation.
    fn for_each_tuple<V: FnMut(&[usize])>(rel: &Self::Rel, visit: V);

    /// Scans every stored tuple of `rel` through
    /// [`for_each_tuple`](Self::for_each_tuple), passing each through
    /// [`std::hint::black_box`] so the walk cannot be optimised away, and
    /// returns how many there were. Allocates no tuple: this is what
    /// `bench ds`'s `iteration` and `end_to_end` metrics time (issue #79).
    fn scan(rel: &Self::Rel) -> u64 {
        let mut tuples = 0u64;
        Self::for_each_tuple(rel, |tuple| {
            std::hint::black_box(tuple);
            tuples += 1;
        });
        tuples
    }

    /// Number of stored tuples in `rel`. Must agree with the number of
    /// tuples [`for_each_tuple`](Self::for_each_tuple) visits.
    fn tuple_count(rel: &Self::Rel) -> usize;

    /// The `ds_*` optimization axes emitted by the relation type, merged
    /// into the report's axes. Empty for structures without any.
    fn optimization_axes(rel: &Self::Rel) -> BTreeMap<String, serde_json::Value>;

    /// The `ds_build_mode` axis of the relations this family builds, merged
    /// into the report's axes. A build mode describes the build, and every
    /// mode builds an equivalent structure, so the family that ran the build
    /// reports it rather than the relation. Empty for structures with a
    /// single build process. Required, with no default, so no family can omit
    /// the axis by accident.
    fn build_mode_axes(&self) -> BTreeMap<String, serde_json::Value>;
}

/// Reads only the header of one relation file — its name and columns, not
/// its tuples — so a query can be validated against a workload before
/// any relation is built. The reader is chosen by extension, as in
/// [`read_relation`], whose header this always equals.
///
/// # Errors
///
/// Returns an error if the extension is neither `csv` nor `parquet`, or if
/// the reader fails.
pub fn read_relation_header(path: &Path) -> anyhow::Result<RelationHeader> {
    let extension = path.extension().and_then(|s| s.to_str()).unwrap_or("");
    match extension.to_lowercase().as_str() {
        | "csv" => kermit_ds::read_csv_header(path),
        | "parquet" => kermit_ds::read_parquet_header(path),
        | _ => anyhow::bail!("Unsupported file extension for {path:?}: '{extension}'"),
    }
    .map_err(|e| anyhow::anyhow!("Failed to load {path:?}: {e}"))
}

/// Reads one relation file into its header and its tuples, in the order the
/// file stores them. The reader is chosen by extension.
///
/// # Errors
///
/// Returns an error if the extension is neither `csv` nor `parquet`, or if
/// the reader fails.
fn read_relation(path: &Path) -> anyhow::Result<(RelationHeader, Vec<Vec<usize>>)> {
    let extension = path.extension().and_then(|s| s.to_str()).unwrap_or("");
    match extension.to_lowercase().as_str() {
        | "csv" => kermit_ds::read_csv(path),
        | "parquet" => kermit_ds::read_parquet(path),
        | _ => anyhow::bail!("Unsupported file extension for {path:?}: '{extension}'"),
    }
    .map_err(|e| anyhow::anyhow!("Failed to load {path:?}: {e}"))
}

/// The join-facing half of a family: building an engine over loaded
/// relations and querying it. Everything else in a `bench run` cell —
/// query parsing, metadata, Criterion group wiring, report assembly — is
/// shared by the generic runner in `bench/run.rs`.
///
/// Both `build` paths must construct the engine the same way: the
/// `end_to_end` metric times [`ExecutionFamily::build_from_tuples`] and
/// reports it as the cost of the build that the `iteration` metric's
/// engine paid via [`ExecutionFamily::build`].
pub trait ExecutionFamily: RelationFamily {
    /// The built, queryable form of a set of relations.
    type Engine;

    /// Whether running a join can change the engine's relations: a lazy
    /// `HashTrie` builds each child a join first reaches (issue #92). When
    /// true, `bench run` never probes the engine it loaded. `--verify` and
    /// `iteration` run on fresh builds, so `space` measures the relations
    /// as built and each timed join pays its own expansion. Required, like
    /// `build_relation`: every family states what its joins do.
    const JOIN_MUTATES: bool;

    /// Builds the engine from freshly loaded relations, retaining them for
    /// the per-relation metrics (`insertion`, `space`).
    fn build(&self, relations: Vec<Self::Rel>) -> Self::Engine;

    /// Builds an engine from `(header, tuples)` snapshots along the same
    /// path as [`ExecutionFamily::build`]. Used inside the timed
    /// `end_to_end` body, where only [`ExecutionFamily::count`] is needed
    /// afterwards.
    fn build_from_tuples(&self, inputs: Vec<(RelationHeader, Vec<Vec<usize>>)>) -> Self::Engine;

    /// The relations an engine built by [`ExecutionFamily::build`] retains.
    fn relations(engine: &Self::Engine) -> Vec<&Self::Rel>;

    /// Runs `query` against `engine`, passing each result tuple to `emit`
    /// as a borrowed slice; the result is never materialised.
    ///
    /// # Errors
    ///
    /// The [`JoinError`] for a query that cannot run over `engine`,
    /// before any tuple is emitted.
    fn join_for_each<S: FnMut(&[usize])>(
        &self, engine: &Self::Engine, query: JoinQuery, emit: S,
    ) -> Result<(), JoinError>;

    /// Runs `query` against `engine` and counts its result tuples, passing
    /// each through [`std::hint::black_box`] so the traversal cannot be
    /// optimised away. Allocates no result rows: this is what the
    /// `iteration` and `end_to_end` metrics time and what `--verify`
    /// checks (issue #65).
    ///
    /// # Errors
    ///
    /// As [`join_for_each`](Self::join_for_each).
    fn count(&self, engine: &Self::Engine, query: JoinQuery) -> Result<u64, JoinError> {
        let mut rows = 0u64;
        self.join_for_each(engine, query, |tuple| {
            std::hint::black_box(tuple);
            rows += 1;
        })?;
        Ok(rows)
    }

    /// The reordered copies `query` needs under this family's planner
    /// that `engine` does not hold yet ([`Database::required_indexes`]).
    /// Empty under `--column-orders stored`. Reads only the engine's
    /// statistics, never a relation, so it is safe on an engine whose
    /// joins mutate it ([`JOIN_MUTATES`](Self::JOIN_MUTATES)).
    ///
    /// # Errors
    ///
    /// As [`join_for_each`](Self::join_for_each).
    fn required_indexes(
        &self, engine: &Self::Engine, query: &JoinQuery,
    ) -> Result<Vec<IndexSpec>, JoinError>;

    /// Builds `spec`'s copy from its base relation's header and file-order
    /// `tuples` through [`build_relation`](RelationFamily::build_relation),
    /// so the copy carries the configuration and build mode the report's
    /// axes name, and adds it to `engine`.
    fn add_index(
        &self, engine: &mut Self::Engine, spec: IndexSpec, base: &RelationHeader,
        tuples: &[Vec<usize>],
    );

    /// Drops every copy `engine` holds.
    fn clear_indexes(engine: &mut Self::Engine);

    /// Every copy `engine` holds, with its spec: the `space/Index_*`
    /// functions.
    fn indexes(engine: &Self::Engine) -> Vec<(&IndexSpec, &Self::Rel)>;
}

/// The sorted-family structure `R` on its own: what `bench ds -i tree-trie`
/// / `-i column-trie` measures. Carries the `--ds-build` mode every
/// relation is built with (`()` for a structure with a single build), but
/// no optimiser or engine, so it cannot join — the type says what
/// `bench ds` does.
pub struct SortedTrieFamily<R: SortedTrieRelation> {
    build: R::BuildMode,
}

impl<R: SortedTrieRelation> SortedTrieFamily<R> {
    /// The family building every relation by `build`.
    pub fn new(build: R::BuildMode) -> Self {
        Self {
            build,
        }
    }
}

impl<R: SortedTrieRelation> Default for SortedTrieFamily<R> {
    fn default() -> Self { Self::new(R::BuildMode::default()) }
}

impl<R: SortedTrieRelation + 'static> RelationFamily for SortedTrieFamily<R> {
    type Rel = R;

    fn execution(&self) -> Execution { Execution::TrieLftj(R::kind(self.build)) }

    fn build_relation(&self, header: RelationHeader, tuples: Vec<Vec<usize>>) -> R {
        R::build_with(header, self.build, tuples)
    }

    fn for_each_tuple<V: FnMut(&[usize])>(rel: &R, mut visit: V) {
        let mut tuples = TrieIteratorWrapper::new(rel.trie_iter());
        while let Some(tuple) = tuples.advance() {
            visit(tuple);
        }
    }

    fn tuple_count(rel: &R) -> usize { rel.trie_iter().into_iter().count() }

    /// The relation's own Layout axes (`ds_layout_seek`), read from the type
    /// it was monomorphised over.
    fn optimization_axes(rel: &R) -> BTreeMap<String, serde_json::Value> { rel.optimization_axes() }

    fn build_mode_axes(&self) -> BTreeMap<String, serde_json::Value> {
        R::build_mode_axes(self.build)
    }
}

/// `HashTrie<H, P, E>` on its own: what `bench ds -i hash-trie` measures.
/// Carries the `--ds-config` runtime values and the `--ds-build` mode every
/// relation is built with; the `--ds-layout-hasher` / `--ds-layout-pruning` /
/// `--ds-layout-expansion` labels are *not* parameters —
/// [`execution`](RelationFamily::execution) derives them from `H`, `P` and
/// `E`, so a report cannot name a Layout the family was not monomorphised
/// over.
pub struct HashTrieFamily<H, P, E> {
    config: HashTrieConfig,
    build: HashTrieBuildMode,
    _layout: PhantomData<(H, P, E)>,
}

impl<H, P, E> HashTrieFamily<H, P, E> {
    /// The family building every relation with the `--ds-config` values
    /// `config`, by the `--ds-build` mode `build`.
    pub fn new(config: HashTrieConfig, build: HashTrieBuildMode) -> Self {
        Self {
            config,
            build,
            _layout: PhantomData,
        }
    }
}

impl<H, P, E> Default for HashTrieFamily<H, P, E> {
    fn default() -> Self { Self::new(HashTrieConfig::default(), HashTrieBuildMode::default()) }
}

impl<H: HashStrategy + 'static, P: PruningPolicy, E: ExpansionPolicy> RelationFamily
    for HashTrieFamily<H, P, E>
{
    type Rel = HashTrie<H, P, E>;

    fn execution(&self) -> Execution {
        Execution::HashHtj {
            hasher: hasher_of::<H>(),
            pruning: pruning_of::<P>(),
            expansion: expansion_of::<E>(),
            config: self.config,
            build: self.build,
        }
    }

    fn build_relation(&self, header: RelationHeader, tuples: Vec<Vec<usize>>) -> HashTrie<H, P, E> {
        HashTrie::<H, P, E>::from_tuples_with_config_and_build_mode(
            header,
            self.config,
            self.build,
            tuples,
        )
    }

    fn for_each_tuple<V: FnMut(&[usize])>(rel: &HashTrie<H, P, E>, visit: V) {
        rel.for_each_tuple(visit);
    }

    /// The trie's own multiset count, kept as it is built, so counting
    /// does not materialise every tuple.
    fn tuple_count(rel: &HashTrie<H, P, E>) -> usize { Cardinality::tuple_count(rel) }

    fn optimization_axes(rel: &HashTrie<H, P, E>) -> BTreeMap<String, serde_json::Value> {
        rel.optimization_axes()
    }

    /// Every `HashTrie` report says which build made it, so kermit-lab can
    /// read a `HashTrie` report without the axis as the `serial` build, the
    /// only one before issue #91.
    fn build_mode_axes(&self) -> BTreeMap<String, serde_json::Value> {
        BTreeMap::from([(
            "ds_build_mode".to_string(),
            serde_json::Value::from(self.build.axis_value()),
        )])
    }
}

/// Sorted family: `R` under Leapfrog Triejoin through [`lftj_join_for_each`].
pub struct TrieLftj<R: SortedTrieRelation> {
    structure: SortedTrieFamily<R>,
    planner: Planner,
}

impl<R: SortedTrieRelation> TrieLftj<R> {
    /// Creates the family building every relation by `build`, planned by
    /// `planner`.
    pub fn new(build: R::BuildMode, planner: Planner) -> Self {
        Self {
            structure: SortedTrieFamily::new(build),
            planner,
        }
    }
}

impl<R: SortedTrieRelation + 'static> RelationFamily for TrieLftj<R> {
    type Rel = R;

    fn execution(&self) -> Execution { self.structure.execution() }

    fn build_relation(&self, header: RelationHeader, tuples: Vec<Vec<usize>>) -> R {
        self.structure.build_relation(header, tuples)
    }

    fn for_each_tuple<V: FnMut(&[usize])>(rel: &R, visit: V) {
        SortedTrieFamily::<R>::for_each_tuple(rel, visit);
    }

    fn tuple_count(rel: &R) -> usize { SortedTrieFamily::<R>::tuple_count(rel) }

    fn optimization_axes(rel: &R) -> BTreeMap<String, serde_json::Value> {
        SortedTrieFamily::<R>::optimization_axes(rel)
    }

    fn build_mode_axes(&self) -> BTreeMap<String, serde_json::Value> {
        self.structure.build_mode_axes()
    }
}

impl<R: SortedTrieRelation + 'static> ExecutionFamily for TrieLftj<R> {
    /// The relations, keyed by name, plus the statistics this family's
    /// planner reads, gathered here once so no timed join pays for them.
    type Engine = Database<R>;

    /// Sorted tries are immutable under a join.
    const JOIN_MUTATES: bool = false;

    fn build(&self, relations: Vec<R>) -> Self::Engine {
        let relations = relations
            .into_iter()
            .map(|r| (r.header().name().to_string(), r))
            .collect();
        Database::new::<SortedFamily>(relations, self.planner.required_statistics())
    }

    fn build_from_tuples(&self, inputs: Vec<(RelationHeader, Vec<Vec<usize>>)>) -> Self::Engine {
        let relations = inputs
            .into_iter()
            .map(|(header, tuples)| {
                let name = header.name().to_string();
                (name, self.build_relation(header, tuples))
            })
            .collect();
        Database::new::<SortedFamily>(relations, self.planner.required_statistics())
    }

    fn relations(engine: &Self::Engine) -> Vec<&R> { engine.relations().collect() }

    fn join_for_each<S: FnMut(&[usize])>(
        &self, engine: &Self::Engine, query: JoinQuery, emit: S,
    ) -> Result<(), JoinError> {
        lftj_join_for_each::<R, LeapfrogTriejoin>(engine, query, &self.planner, emit)
    }

    fn required_indexes(
        &self, engine: &Self::Engine, query: &JoinQuery,
    ) -> Result<Vec<IndexSpec>, JoinError> {
        engine.required_indexes(query, &self.planner)
    }

    fn add_index(
        &self, engine: &mut Self::Engine, spec: IndexSpec, base: &RelationHeader,
        tuples: &[Vec<usize>],
    ) {
        let copy = self.build_relation(index_header(&spec, base), spec.permute_all(tuples));
        engine.add_index(spec, copy);
    }

    fn clear_indexes(engine: &mut Self::Engine) { engine.clear_indexes(); }

    fn indexes(engine: &Self::Engine) -> Vec<(&IndexSpec, &Self::Rel)> {
        engine.indexes().collect()
    }
}

/// Hash family: `HashTrie<H, P, E>` under Hash Triejoin through
/// [`hash_join_for_each`].
pub struct HashHtj<H, P, E> {
    structure: HashTrieFamily<H, P, E>,
    planner: Planner,
}

impl<H, P, E> HashHtj<H, P, E> {
    /// Creates the family for the `--ds-config` values `config` and the
    /// `--ds-build` mode `build`, planned by `planner`. The
    /// `--ds-layout-hasher` / `--ds-layout-pruning` / `--ds-layout-expansion`
    /// labels are *not* parameters: [`execution`](RelationFamily::execution)
    /// derives them from `H`, `P` and `E`, so a report cannot name a Layout
    /// the family was not monomorphised over.
    pub fn new(config: HashTrieConfig, build: HashTrieBuildMode, planner: Planner) -> Self {
        Self {
            structure: HashTrieFamily::new(config, build),
            planner,
        }
    }
}

impl<H: HashStrategy + 'static, P: PruningPolicy, E: ExpansionPolicy> RelationFamily
    for HashHtj<H, P, E>
{
    type Rel = HashTrie<H, P, E>;

    fn execution(&self) -> Execution { self.structure.execution() }

    fn build_relation(&self, header: RelationHeader, tuples: Vec<Vec<usize>>) -> HashTrie<H, P, E> {
        self.structure.build_relation(header, tuples)
    }

    fn for_each_tuple<V: FnMut(&[usize])>(rel: &HashTrie<H, P, E>, visit: V) {
        HashTrieFamily::<H, P, E>::for_each_tuple(rel, visit);
    }

    fn tuple_count(rel: &HashTrie<H, P, E>) -> usize { HashTrieFamily::<H, P, E>::tuple_count(rel) }

    fn optimization_axes(rel: &HashTrie<H, P, E>) -> BTreeMap<String, serde_json::Value> {
        HashTrieFamily::<H, P, E>::optimization_axes(rel)
    }

    fn build_mode_axes(&self) -> BTreeMap<String, serde_json::Value> {
        self.structure.build_mode_axes()
    }
}

impl<H: HashStrategy + 'static, P: PruningPolicy, E: ExpansionPolicy> ExecutionFamily
    for HashHtj<H, P, E>
{
    /// The relations, keyed by name, plus the statistics this family's
    /// planner reads, gathered here once so no timed join pays for them.
    type Engine = Database<HashTrie<H, P, E>>;

    /// A lazy trie expands the children a join reaches.
    const JOIN_MUTATES: bool = E::LAZY;

    fn build(&self, relations: Vec<HashTrie<H, P, E>>) -> Self::Engine {
        let relations = relations
            .into_iter()
            .map(|r| (r.header().name().to_string(), r))
            .collect();
        Database::new::<HashFamily<H>>(relations, self.planner.required_statistics())
    }

    fn build_from_tuples(&self, inputs: Vec<(RelationHeader, Vec<Vec<usize>>)>) -> Self::Engine {
        let relations = inputs
            .into_iter()
            .map(|(header, tuples)| {
                let name = header.name().to_string();
                (name, self.build_relation(header, tuples))
            })
            .collect();
        Database::new::<HashFamily<H>>(relations, self.planner.required_statistics())
    }

    fn relations(engine: &Self::Engine) -> Vec<&HashTrie<H, P, E>> { engine.relations().collect() }

    fn join_for_each<S: FnMut(&[usize])>(
        &self, engine: &Self::Engine, query: JoinQuery, emit: S,
    ) -> Result<(), JoinError> {
        hash_join_for_each::<HashTrie<H, P, E>, H>(engine, query, &self.planner, emit)
    }

    fn required_indexes(
        &self, engine: &Self::Engine, query: &JoinQuery,
    ) -> Result<Vec<IndexSpec>, JoinError> {
        engine.required_indexes(query, &self.planner)
    }

    fn add_index(
        &self, engine: &mut Self::Engine, spec: IndexSpec, base: &RelationHeader,
        tuples: &[Vec<usize>],
    ) {
        let copy = self.build_relation(index_header(&spec, base), spec.permute_all(tuples));
        engine.add_index(spec, copy);
    }

    fn clear_indexes(engine: &mut Self::Engine) { engine.clear_indexes(); }

    fn indexes(engine: &Self::Engine) -> Vec<(&IndexSpec, &Self::Rel)> {
        engine.indexes().collect()
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::options::BuildModes,
        clap::ValueEnum,
        kermit_algos::{ColumnOrderPolicy, IndexSpec, LexicographicOptimiser, Optimiser},
        kermit_ds::{
            ConfigurableRelation, EagerExpansion, LazyExpansion, LoadFactor, NoPruning, RadixBits,
            RootCapacity, SingletonPruning,
        },
        kermit_iters::{LayoutOption, SipHashStrategy},
        std::cell::Cell,
    };

    fn all_structures() -> Vec<IndexStructure> { IndexStructure::value_variants().to_vec() }

    fn all_algorithms() -> Vec<JoinAlgorithm> { JoinAlgorithm::value_variants().to_vec() }

    /// The full cross product yields exactly the three valid cells and
    /// skips the other three — the `-i all -a all` acceptance criterion.
    #[test]
    fn sweep_of_full_cross_product_yields_exactly_three_cells() {
        let sweep = Sweep::expand(&all_structures(), &all_algorithms(), DsChoices::default());
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
            DsChoices::default(),
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
            DsChoices::default(),
        );
        assert!(sweep.cells.is_empty());
        assert_eq!(sweep.skipped.len(), 1);
    }

    /// Every cell reports the pair it was built from, and the hasher
    /// choice rides along on the hash cell.
    #[test]
    fn execution_axes_round_trip_through_for_pair() {
        let fx = DsChoices {
            hasher: HasherChoice::Fxhash,
            ..DsChoices::default()
        };
        for ds in all_structures() {
            for algo in all_algorithms() {
                if let Some(cell) = Execution::for_pair(ds, algo, fx) {
                    assert_eq!(cell.index_structure(), ds);
                    assert_eq!(cell.algorithm(), algo);
                }
            }
        }
        assert_eq!(
            Execution::for_pair(IndexStructure::HashTrie, JoinAlgorithm::HashTriejoin, fx),
            Some(Execution::HashHtj {
                hasher: HasherChoice::Fxhash,
                pruning: PruningChoice::Off,
                expansion: ExpansionChoice::Eager,
                config: HashTrieConfig::default(),
                build: HashTrieBuildMode::Serial,
            })
        );
    }

    /// `for_structure` must name the one cell `for_pair` accepts for that
    /// structure — otherwise `bench ds` and `bench run` could disagree on
    /// which family a structure belongs to.
    #[test]
    fn for_structure_agrees_with_for_pair() {
        let config = HashTrieConfig::default();
        let radix = HashTrieBuildMode::Radix(RadixBits::new(2).unwrap());
        let parallel = TreeTrieBuildMode::Parallel(kermit_ds::Threads::new(4).unwrap());
        let builds = [TreeTrieBuildMode::Serial, parallel]
            .into_iter()
            .flat_map(|tree_trie| {
                [ColumnTrieBuildMode::Incremental, ColumnTrieBuildMode::Bulk]
                    .map(|column_trie| (tree_trie, column_trie))
            })
            .flat_map(|(tree_trie, column_trie)| {
                [HashTrieBuildMode::Serial, radix].map(|hash_trie| BuildModes {
                    tree_trie,
                    column_trie,
                    hash_trie,
                })
            });
        for build in builds {
            for ds in all_structures() {
                for hasher in [HasherChoice::Sip, HasherChoice::Fxhash] {
                    for pruning in [PruningChoice::Off, PruningChoice::On] {
                        for &expansion in ExpansionChoice::value_variants() {
                            for &seek in SeekChoice::value_variants() {
                                let choices = DsChoices {
                                    hasher,
                                    pruning,
                                    expansion,
                                    seek,
                                    config,
                                    build,
                                };
                                let cell = Execution::for_structure(ds, choices);
                                assert_eq!(cell.index_structure(), ds);
                                assert_eq!(
                                    Execution::for_pair(ds, cell.algorithm(), choices),
                                    Some(cell)
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    /// The structure-only markers `bench ds` uses must report the same
    /// cell as the join families `bench run` uses for the same structure.
    #[test]
    fn structure_markers_agree_with_join_families() {
        assert_eq!(
            SortedTrieFamily::<TreeTrie>::default().execution(),
            TrieLftj::<TreeTrie>::new(
                TreeTrieBuildMode::default(),
                Planner::stored(LexicographicOptimiser)
            )
            .execution()
        );
        assert_eq!(
            SortedTrieFamily::<ColumnTrie>::default().execution(),
            TrieLftj::<ColumnTrie>::new(
                ColumnTrieBuildMode::default(),
                Planner::stored(LexicographicOptimiser)
            )
            .execution()
        );
        let config = HashTrieConfig {
            load_factor: kermit_ds::LoadFactor::percent(50).unwrap(),
            ..HashTrieConfig::default()
        };
        let radix = HashTrieBuildMode::Radix(RadixBits::new(2).unwrap());
        assert_eq!(
            HashTrieFamily::<kermit_iters::FxHashStrategy, SingletonPruning, EagerExpansion>::new(
                config, radix
            )
            .execution(),
            HashHtj::<kermit_iters::FxHashStrategy, SingletonPruning, EagerExpansion>::new(
                config,
                radix,
                Planner::stored(LexicographicOptimiser)
            )
            .execution()
        );
        assert_eq!(
            HashTrieFamily::<kermit_iters::FxHashStrategy, SingletonPruning, LazyExpansion>::new(
                config, radix
            )
            .execution(),
            HashHtj::<kermit_iters::FxHashStrategy, SingletonPruning, LazyExpansion>::new(
                config,
                radix,
                Planner::stored(LexicographicOptimiser)
            )
            .execution()
        );
    }

    /// The family's reported execution comes from its type, so the label
    /// can never disagree with the code path that ran.
    #[test]
    fn families_report_their_own_execution() {
        let tree = TrieLftj::<TreeTrie>::new(
            TreeTrieBuildMode::default(),
            Planner::stored(LexicographicOptimiser),
        );
        assert_eq!(
            tree.execution(),
            Execution::TrieLftj(SortedTrie::TreeTrie {
                seek: SeekChoice::Galloping,
                build: TreeTrieBuildMode::Serial,
            })
        );
        let four = TreeTrieBuildMode::Parallel(kermit_ds::Threads::new(4).unwrap());
        assert_eq!(
            TrieLftj::<TreeTrie>::new(four, Planner::stored(LexicographicOptimiser)).execution(),
            Execution::TrieLftj(SortedTrie::TreeTrie {
                seek: SeekChoice::Galloping,
                build: four,
            })
        );
        let column = TrieLftj::<ColumnTrie>::new(
            ColumnTrieBuildMode::Incremental,
            Planner::stored(LexicographicOptimiser),
        );
        assert_eq!(
            column.execution(),
            Execution::TrieLftj(SortedTrie::ColumnTrie {
                seek: SeekChoice::Galloping,
                build: ColumnTrieBuildMode::Incremental
            })
        );
        let config = HashTrieConfig {
            load_factor: kermit_ds::LoadFactor::percent(50).unwrap(),
            ..HashTrieConfig::default()
        };
        let radix = HashTrieBuildMode::Radix(RadixBits::new(2).unwrap());
        let hash = HashHtj::<kermit_iters::FxHashStrategy, SingletonPruning, EagerExpansion>::new(
            config,
            radix,
            Planner::stored(LexicographicOptimiser),
        );
        // No Layout value was passed to `new`: both labels come from the
        // type parameters, so the report cannot disagree with the code
        // path that ran.
        assert_eq!(hash.execution(), Execution::HashHtj {
            hasher: HasherChoice::Fxhash,
            pruning: PruningChoice::On,
            expansion: ExpansionChoice::Eager,
            config,
            build: radix,
        });
        assert_eq!(hash.execution().algorithm(), JoinAlgorithm::HashTriejoin);
    }

    /// All eight `HashTrie` Layout instantiations report the labels their
    /// type parameters imply — the `with_hash_trie_layout!` table read
    /// back out of the monomorphised families.
    #[test]
    fn hash_family_labels_are_derived_from_its_layout_types() {
        use kermit_iters::{FxHashStrategy, SipHashStrategy};
        fn labels<H: HashStrategy + 'static, P: PruningPolicy, E: ExpansionPolicy>(
        ) -> (HasherChoice, PruningChoice, ExpansionChoice) {
            match HashHtj::<H, P, E>::new(
                HashTrieConfig::default(),
                HashTrieBuildMode::Serial,
                Planner::stored(LexicographicOptimiser),
            )
            .execution()
            {
                | Execution::HashHtj {
                    hasher,
                    pruning,
                    expansion,
                    ..
                } => (hasher, pruning, expansion),
                | other => panic!("hash family reported {other:?}"),
            }
        }
        assert_eq!(
            labels::<SipHashStrategy, NoPruning, EagerExpansion>(),
            (
                HasherChoice::Sip,
                PruningChoice::Off,
                ExpansionChoice::Eager
            )
        );
        assert_eq!(
            labels::<SipHashStrategy, NoPruning, LazyExpansion>(),
            (HasherChoice::Sip, PruningChoice::Off, ExpansionChoice::Lazy)
        );
        assert_eq!(
            labels::<SipHashStrategy, SingletonPruning, EagerExpansion>(),
            (HasherChoice::Sip, PruningChoice::On, ExpansionChoice::Eager)
        );
        assert_eq!(
            labels::<SipHashStrategy, SingletonPruning, LazyExpansion>(),
            (HasherChoice::Sip, PruningChoice::On, ExpansionChoice::Lazy)
        );
        assert_eq!(
            labels::<FxHashStrategy, NoPruning, EagerExpansion>(),
            (
                HasherChoice::Fxhash,
                PruningChoice::Off,
                ExpansionChoice::Eager
            )
        );
        assert_eq!(
            labels::<FxHashStrategy, NoPruning, LazyExpansion>(),
            (
                HasherChoice::Fxhash,
                PruningChoice::Off,
                ExpansionChoice::Lazy
            )
        );
        assert_eq!(
            labels::<FxHashStrategy, SingletonPruning, EagerExpansion>(),
            (
                HasherChoice::Fxhash,
                PruningChoice::On,
                ExpansionChoice::Eager
            )
        );
        assert_eq!(
            labels::<FxHashStrategy, SingletonPruning, LazyExpansion>(),
            (
                HasherChoice::Fxhash,
                PruningChoice::On,
                ExpansionChoice::Lazy
            )
        );
    }

    /// Only a lazy HashTrie family's joins change its relations (#92).
    #[test]
    fn only_lazy_hash_families_mutate_on_join() {
        const { assert!(!<TrieLftj<TreeTrie> as ExecutionFamily>::JOIN_MUTATES) };
        const {
            assert!(!<HashHtj<SipHashStrategy, NoPruning, EagerExpansion> as ExecutionFamily>::JOIN_MUTATES)
        };
        const {
            assert!(<HashHtj<SipHashStrategy, NoPruning, LazyExpansion> as ExecutionFamily>::JOIN_MUTATES)
        };
        const {
            assert!(
                <HashHtj<SipHashStrategy, SingletonPruning, LazyExpansion> as ExecutionFamily>::JOIN_MUTATES
            )
        };
    }

    /// The config reaches the relations the family builds, so the report's
    /// `ds_config_*` axes describe the structure that actually ran.
    #[test]
    fn hash_family_builds_relations_with_its_config() {
        let config = HashTrieConfig {
            load_factor: kermit_ds::LoadFactor::percent(50).unwrap(),
            ..HashTrieConfig::default()
        };
        let family = HashHtj::<kermit_iters::SipHashStrategy, NoPruning, EagerExpansion>::new(
            config,
            HashTrieBuildMode::Serial,
            Planner::stored(LexicographicOptimiser),
        );
        let header = RelationHeader::new("r", vec!["a".to_string(), "b".to_string()]);
        let engine = family.build_from_tuples(vec![(header, vec![vec![1, 2]])]);
        let rel = engine.get("r").unwrap();
        assert_eq!(
            HashHtj::<kermit_iters::SipHashStrategy, NoPruning, EagerExpansion>::optimization_axes(
                rel
            )
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
            ..HashTrieConfig::default()
        };
        let family = HashHtj::<kermit_iters::SipHashStrategy, NoPruning, EagerExpansion>::new(
            config,
            HashTrieBuildMode::Serial,
            Planner::stored(LexicographicOptimiser),
        );
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("r.csv");
        std::fs::write(&path, "a,b\n1,2\n").expect("write csv");
        let rel = family.load(&path).expect("load");
        assert_eq!(*rel.config(), config);
        assert_eq!(
            HashHtj::<kermit_iters::SipHashStrategy, NoPruning, EagerExpansion>::optimization_axes(
                &rel
            )
            .get("ds_config_load_factor"),
            Some(&serde_json::Value::from(0.5_f64))
        );
    }

    /// `load_with_tuples` hands back the tuples in the order the reader
    /// produced them, not in the structure's own iteration order: the
    /// `insertion` and `end_to_end` metrics rebuild from them, and a
    /// structure's iteration order is not neutral input — sorted for the
    /// sorted tries, and a degenerate build order for the hash trie (issue
    /// #66).
    #[test]
    fn load_with_tuples_returns_file_order() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("r.csv");
        std::fs::write(&path, "a,b\n3,1\n1,2\n2,0\n1,1\n").expect("write csv");
        let file_order = vec![vec![3, 1], vec![1, 2], vec![2, 0], vec![1, 1]];

        let (tree, tuples) = SortedTrieFamily::<TreeTrie>::default()
            .load_with_tuples(&path)
            .expect("load");
        assert_eq!(tuples, file_order);
        assert_ne!(
            visited::<SortedTrieFamily<TreeTrie>>(&tree),
            file_order,
            "the fixture must not already be in iteration order"
        );

        let (hash, tuples) =
            HashTrieFamily::<kermit_iters::SipHashStrategy, NoPruning, EagerExpansion>::default()
                .load_with_tuples(&path)
                .expect("load");
        assert_eq!(tuples, file_order);
        assert_eq!(hash.header().name(), "r");
    }

    /// Every tuple `F::for_each_tuple` lends, collected — the test-side
    /// stand-in for the materialising walk the families no longer offer.
    fn visited<F: RelationFamily>(rel: &F::Rel) -> Vec<Vec<usize>> {
        let mut tuples = Vec::new();
        F::for_each_tuple(rel, |tuple| tuples.push(tuple.to_vec()));
        tuples
    }

    /// `tuple_count` must agree with the tuples `for_each_tuple` visits —
    /// the trait's contract — including duplicates, which the hash trie
    /// keeps.
    #[test]
    fn hash_family_tuple_count_agrees_with_its_tuples() {
        let family =
            HashTrieFamily::<kermit_iters::SipHashStrategy, NoPruning, EagerExpansion>::default();
        let header = RelationHeader::new("r", vec!["a".to_string(), "b".to_string()]);
        let rel = family.build_relation(header, vec![vec![1, 2], vec![1, 2], vec![3, 4]]);
        assert_eq!(
            HashTrieFamily::<kermit_iters::SipHashStrategy, NoPruning, EagerExpansion>::tuple_count(
                &rel
            ),
            3
        );
        assert_eq!(
            visited::<HashTrieFamily<kermit_iters::SipHashStrategy, NoPruning, EagerExpansion>>(
                &rel
            )
            .len(),
            3
        );
    }

    /// `scan` is what `bench ds` times (issue #79): it must count every
    /// stored tuple, agreeing with `tuple_count` in every family — the
    /// join families delegate — and keep the hash trie's duplicates.
    #[test]
    fn scan_agrees_with_tuple_count_in_every_family() {
        let header = || RelationHeader::new_positional("r", 2);
        let tuples = || vec![vec![1, 2], vec![1, 2], vec![1, 3], vec![4, 5]];

        let tree = SortedTrieFamily::<TreeTrie>::default().build_relation(header(), tuples());
        assert_eq!(SortedTrieFamily::<TreeTrie>::scan(&tree), 3);
        assert_eq!(SortedTrieFamily::<TreeTrie>::tuple_count(&tree), 3);
        assert_eq!(TrieLftj::<TreeTrie>::scan(&tree), 3);

        let column = SortedTrieFamily::<ColumnTrie>::default().build_relation(header(), tuples());
        assert_eq!(SortedTrieFamily::<ColumnTrie>::scan(&column), 3);
        assert_eq!(SortedTrieFamily::<ColumnTrie>::tuple_count(&column), 3);

        let hash = HashTrieFamily::<SipHashStrategy, NoPruning, EagerExpansion>::default()
            .build_relation(header(), tuples());
        assert_eq!(
            HashTrieFamily::<SipHashStrategy, NoPruning, EagerExpansion>::scan(&hash),
            4
        );
        assert_eq!(
            HashTrieFamily::<SipHashStrategy, NoPruning, EagerExpansion>::tuple_count(&hash),
            4
        );
        assert_eq!(
            HashHtj::<SipHashStrategy, NoPruning, EagerExpansion>::scan(&hash),
            4
        );

        let pruned = HashTrieFamily::<SipHashStrategy, SingletonPruning, EagerExpansion>::default()
            .build_relation(header(), tuples());
        assert_eq!(
            HashTrieFamily::<SipHashStrategy, SingletonPruning, EagerExpansion>::scan(&pruned),
            4
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
            ..HashTrieConfig::default()
        };
        let family = HashHtj::<kermit_iters::SipHashStrategy, NoPruning, EagerExpansion>::new(
            config,
            HashTrieBuildMode::Serial,
            Planner::stored(LexicographicOptimiser),
        );
        let header = RelationHeader::new("r", vec!["a".to_string(), "b".to_string()]);
        let rel = family.build_relation(header, vec![vec![1, 2]]);
        assert_eq!(*rel.config(), config);
        assert_eq!(
            HashHtj::<kermit_iters::SipHashStrategy, NoPruning, EagerExpansion>::optimization_axes(
                &rel
            )
            .get("ds_config_load_factor"),
            Some(&serde_json::Value::from(0.5_f64))
        );
    }

    /// Under `root-capacity=tuples` the family's build sizes the root from
    /// the tuple count, so a report labelled `"tuples"` timed a presized
    /// root. 100 tuples sharing one first value hold one root key: the
    /// grown root keeps 4 buckets, while the presized one has room for 100.
    #[test]
    fn hash_family_build_relation_presizes_the_root_under_tuples() {
        let heap = |root_capacity| {
            let family = HashHtj::<SipHashStrategy, NoPruning, EagerExpansion>::new(
                HashTrieConfig {
                    root_capacity,
                    ..HashTrieConfig::default()
                },
                HashTrieBuildMode::Serial,
                Planner::stored(LexicographicOptimiser),
            );
            let header = RelationHeader::new("r", vec!["a".to_string(), "b".to_string()]);
            let tuples = (0..100).map(|b| vec![1, b]).collect();
            family.build_relation(header, tuples).heap_size_bytes()
        };
        assert!(heap(RootCapacity::Tuples) > heap(RootCapacity::Grow));
    }

    /// The pruning Layout reaches the relations the family builds, so a
    /// report's `ds_layout_pruning` axis describes the structure that ran.
    #[test]
    fn pruned_family_reports_the_pruning_layout() {
        let family =
            HashHtj::<kermit_iters::SipHashStrategy, SingletonPruning, EagerExpansion>::new(
                HashTrieConfig::default(),
                HashTrieBuildMode::Serial,
                Planner::stored(LexicographicOptimiser),
            );
        let header = RelationHeader::new("r", vec!["a".to_string(), "b".to_string()]);
        let rel = family.build_relation(header, vec![vec![1, 2]]);
        assert_eq!(
            HashHtj::<kermit_iters::SipHashStrategy, SingletonPruning, EagerExpansion>::optimization_axes(&rel)
                .get("ds_layout_pruning"),
            Some(&serde_json::Value::String("on".into()))
        );
    }

    /// `count` (what `bench run` times and `--verify` checks) and the rows
    /// `join_for_each` streams (what `kermit join` writes) come from one
    /// traversal: they must agree in every family.
    /// How many rows `join_for_each` streams for `query`.
    fn rows<F: ExecutionFamily>(family: &F, engine: &F::Engine, query: JoinQuery) -> usize {
        let mut rows = 0;
        family.join_for_each(engine, query, |_| rows += 1).unwrap();
        rows
    }

    /// Each engine gathers exactly the statistics its optimiser reads,
    /// along both build paths: `iteration` never pays for a walk, and
    /// `end_to_end` pays only for one its optimiser needs.
    #[test]
    fn engines_gather_the_statistics_their_optimiser_reads() {
        let header = || RelationHeader::new_positional("edge", 2);
        let edges = || vec![vec![1, 2], vec![1, 3], vec![2, 3]];
        for &optimiser in Optimiser::value_variants() {
            let want = optimiser.instantiate().required_statistics();
            // One planner per family: a `Planner` owns its optimiser.
            let planner = || Planner::new(optimiser.instantiate(), ColumnOrderPolicy::Stored);
            let tree = TrieLftj::<TreeTrie>::new(TreeTrieBuildMode::default(), planner());
            let built = tree.build(vec![tree.build_relation(header(), edges())]);
            assert_eq!(built.level(), want, "{optimiser:?}");
            assert_eq!(
                tree.build_from_tuples(vec![(header(), edges())]).level(),
                want
            );
            let column = TrieLftj::<ColumnTrie>::new(ColumnTrieBuildMode::default(), planner());
            assert_eq!(
                column.build_from_tuples(vec![(header(), edges())]).level(),
                want
            );
            let hash = HashHtj::<SipHashStrategy, NoPruning, EagerExpansion>::new(
                HashTrieConfig::default(),
                HashTrieBuildMode::Serial,
                planner(),
            );
            let built = hash.build(vec![hash.build_relation(header(), edges())]);
            assert_eq!(built.level(), want, "{optimiser:?}");
            assert_eq!(
                hash.build_from_tuples(vec![(header(), edges())]).level(),
                want
            );
        }
    }

    #[test]
    fn count_agrees_with_join_in_every_family() {
        // Triangles in this graph: (1, 2, 3) and (2, 3, 4).
        let edges = vec![vec![1, 2], vec![2, 3], vec![1, 3], vec![3, 4], vec![2, 4]];
        let inputs = || vec![(RelationHeader::new_positional("edge", 2), edges.clone())];
        let query: JoinQuery = "Q(X, Y, Z) :- edge(X, Y), edge(Y, Z), edge(X, Z)."
            .parse()
            .unwrap();

        let tree = TrieLftj::<TreeTrie>::new(
            TreeTrieBuildMode::default(),
            Planner::stored(LexicographicOptimiser),
        );
        let engine = tree.build_from_tuples(inputs());
        assert_eq!(tree.count(&engine, query.clone()).unwrap(), 2);
        assert_eq!(rows(&tree, &engine, query.clone()), 2);

        let column = TrieLftj::<ColumnTrie>::new(
            ColumnTrieBuildMode::default(),
            Planner::stored(LexicographicOptimiser),
        );
        let engine = column.build_from_tuples(inputs());
        assert_eq!(column.count(&engine, query.clone()).unwrap(), 2);
        assert_eq!(rows(&column, &engine, query.clone()), 2);

        let hash = HashHtj::<SipHashStrategy, NoPruning, EagerExpansion>::new(
            HashTrieConfig::default(),
            HashTrieBuildMode::Serial,
            Planner::stored(LexicographicOptimiser),
        );
        let engine = hash.build_from_tuples(inputs());
        assert_eq!(hash.count(&engine, query.clone()).unwrap(), 2);
        assert_eq!(rows(&hash, &engine, query), 2);
    }

    /// `read_relation_header` is what queries are validated against before
    /// any build, so it must agree with the header the build itself reads.
    /// The empty Parquet file is written by the WatDiv pipeline's own
    /// writer — the shape of a relation seeded for an absent predicate —
    /// and keeps its arity from the schema.
    #[test]
    fn header_only_read_agrees_with_the_full_read() {
        let dir = tempfile::tempdir().unwrap();
        let csv = dir.path().join("edge.csv");
        std::fs::write(&csv, "# a comment\nsrc,dst\n1,2\n3,4\n").unwrap();
        let full = dir.path().join("full.parquet");
        let empty = dir.path().join("empty.parquet");
        for (path, tuples) in [(&full, vec![(1, 2), (3, 4)]), (&empty, vec![])] {
            let rel = kermit_rdf::partition::PartitionedRelation {
                name: "ignored".into(),
                tuples,
            };
            kermit_rdf::parquet::write_relation(&rel, path).unwrap();
        }
        for path in [&csv, &full, &empty] {
            let header = read_relation_header(path).unwrap();
            assert_eq!(header, read_relation(path).unwrap().0, "{path:?}");
            assert_eq!(header.arity(), 2, "{path:?}");
        }
    }

    kermit_ds::define_build_mode_provider!(AnyMode, ColumnTrieBuildMode, ColumnTrieBuildMode::Bulk);

    /// A `ColumnTrie` whose `SortedTrieRelation::build_with` records the mode
    /// it was handed. The provider is irrelevant: `build_with` ignores it.
    type Spy = kermit_ds::BuiltWith<ColumnTrie, AnyMode>;

    thread_local! {
        static BUILT_WITH: Cell<Option<ColumnTrieBuildMode>> = const { Cell::new(None) };
    }

    impl SortedTrieRelation for Spy {
        type BuildMode = ColumnTrieBuildMode;

        fn kind(build: ColumnTrieBuildMode) -> SortedTrie {
            <ColumnTrie as SortedTrieRelation>::kind(build)
        }

        fn build_with(
            header: RelationHeader, build: ColumnTrieBuildMode, tuples: Vec<Vec<usize>>,
        ) -> Self {
            BUILT_WITH.set(Some(build));
            <Spy as Relation>::from_tuples(header, tuples)
        }

        fn build_mode_axes(build: ColumnTrieBuildMode) -> BTreeMap<String, serde_json::Value> {
            <ColumnTrie as SortedTrieRelation>::build_mode_axes(build)
        }
    }

    /// Both modes build identical tries, so only a spy can see whether the
    /// family's mode reached the build, on every route a relation is built.
    #[test]
    fn sorted_families_build_with_their_mode() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("r.csv");
        std::fs::write(&path, "a,b\n1,2\n").expect("write csv");
        let header = || RelationHeader::new_positional("r", 2);
        let tuples = || vec![vec![1, 2]];
        // The mode `build` hands to `build_with`, or `None` if it never does.
        let seen = |build: &dyn Fn()| {
            BUILT_WITH.take();
            build();
            BUILT_WITH.take()
        };

        for mode in [ColumnTrieBuildMode::Incremental, ColumnTrieBuildMode::Bulk] {
            let structure = SortedTrieFamily::<Spy>::new(mode);
            let join = TrieLftj::<Spy>::new(mode, Planner::stored(LexicographicOptimiser));
            let routes: [(&str, &dyn Fn()); 5] = [
                ("SortedTrieFamily::build_relation", &|| {
                    structure.build_relation(header(), tuples());
                }),
                ("SortedTrieFamily::load_with_tuples", &|| {
                    structure.load_with_tuples(&path).expect("load");
                }),
                ("TrieLftj::build_relation", &|| {
                    join.build_relation(header(), tuples());
                }),
                ("TrieLftj::load", &|| {
                    join.load(&path).expect("load");
                }),
                ("TrieLftj::build_from_tuples", &|| {
                    join.build_from_tuples(vec![(header(), tuples())]);
                }),
            ];
            for (route, build) in routes {
                assert_eq!(seen(build), Some(mode), "{route} dropped {mode:?}");
            }
        }
    }

    /// The real `TreeTrie`, not a spy relation: its modes build identical
    /// tries, so only kermit-ds's record of parallel builds (the `test-hooks`
    /// feature) can show that the family's mode reached
    /// `TreeTrie::from_tuples_with_build_mode`, on every route a relation is
    /// built, copies included (#94).
    #[test]
    fn tree_trie_families_build_with_their_mode() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("r.csv");
        std::fs::write(&path, "a,b\n1,2\n2,1\n3,4\n").expect("write csv");
        let header = || RelationHeader::new_positional("r", 2);
        let tuples = || vec![vec![1, 2], vec![2, 1], vec![3, 4]];
        // The thread count of every parallel build `build` runs.
        let parallel_builds = |build: &dyn Fn()| {
            kermit_ds::test_hooks::take_tree_trie_parallel_builds();
            build();
            kermit_ds::test_hooks::take_tree_trie_parallel_builds()
                .into_iter()
                .map(|(threads, _)| threads)
                .collect::<Vec<_>>()
        };

        let two = TreeTrieBuildMode::Parallel(kermit_ds::Threads::new(2).unwrap());
        for (mode, expected) in [(TreeTrieBuildMode::Serial, vec![]), (two, vec![2])] {
            let structure = SortedTrieFamily::<TreeTrie>::new(mode);
            let join = TrieLftj::<TreeTrie>::new(mode, Planner::stored(LexicographicOptimiser));
            let routes: [(&str, &dyn Fn()); 6] = [
                ("SortedTrieFamily::build_relation", &|| {
                    structure.build_relation(header(), tuples());
                }),
                ("SortedTrieFamily::load_with_tuples", &|| {
                    structure.load_with_tuples(&path).expect("load");
                }),
                ("TrieLftj::build_relation", &|| {
                    join.build_relation(header(), tuples());
                }),
                ("TrieLftj::load", &|| {
                    join.load(&path).expect("load");
                }),
                ("TrieLftj::build_from_tuples", &|| {
                    join.build_from_tuples(vec![(header(), tuples())]);
                }),
                ("TrieLftj::add_index", &|| {
                    let mut engine = join.build(Vec::new());
                    let spec = IndexSpec::new("r", vec![1, 0]);
                    join.add_index(&mut engine, spec, &header(), &tuples());
                }),
            ];
            for (route, build) in routes {
                assert_eq!(parallel_builds(build), expected, "{route} under {mode:?}");
            }
        }
    }

    fn build_mode_axis(mode: &str) -> BTreeMap<String, serde_json::Value> {
        BTreeMap::from([("ds_build_mode".to_string(), serde_json::Value::from(mode))])
    }

    /// Every family reports the mode it builds with (issues #84, #91, #94).
    #[test]
    fn families_report_their_build_mode() {
        let radix = HashTrieBuildMode::Radix(RadixBits::new(4).unwrap());
        assert_eq!(
            SortedTrieFamily::<ColumnTrie>::default().build_mode_axes(),
            build_mode_axis("bulk")
        );
        assert_eq!(
            SortedTrieFamily::<ColumnTrie>::new(ColumnTrieBuildMode::Incremental).build_mode_axes(),
            build_mode_axis("incremental")
        );
        assert_eq!(
            TrieLftj::<ColumnTrie>::new(
                ColumnTrieBuildMode::Incremental,
                Planner::stored(LexicographicOptimiser)
            )
            .build_mode_axes(),
            build_mode_axis("incremental")
        );
        assert_eq!(
            SortedTrieFamily::<TreeTrie>::default().build_mode_axes(),
            build_mode_axis("serial")
        );
        let two = TreeTrieBuildMode::Parallel(kermit_ds::Threads::new(2).unwrap());
        assert_eq!(
            SortedTrieFamily::<TreeTrie>::new(two).build_mode_axes(),
            build_mode_axis("parallel:2")
        );
        assert_eq!(
            TrieLftj::<TreeTrie>::new(two, Planner::stored(LexicographicOptimiser))
                .build_mode_axes(),
            build_mode_axis("parallel:2")
        );
        assert_eq!(
            HashTrieFamily::<SipHashStrategy, NoPruning, EagerExpansion>::default()
                .build_mode_axes(),
            build_mode_axis("serial")
        );
        assert_eq!(
            HashTrieFamily::<SipHashStrategy, NoPruning, LazyExpansion>::new(
                HashTrieConfig::default(),
                radix
            )
            .build_mode_axes(),
            build_mode_axis("radix:4")
        );
        assert_eq!(
            HashHtj::<SipHashStrategy, NoPruning, EagerExpansion>::new(
                HashTrieConfig::default(),
                radix,
                Planner::stored(LexicographicOptimiser)
            )
            .build_mode_axes(),
            build_mode_axis("radix:4")
        );
        let hash_two = HashTrieBuildMode::Parallel(kermit_ds::Threads::new(2).unwrap());
        assert_eq!(
            HashTrieFamily::<SipHashStrategy, NoPruning, EagerExpansion>::new(
                HashTrieConfig::default(),
                hash_two
            )
            .build_mode_axes(),
            build_mode_axis("parallel:2")
        );
        assert_eq!(
            HashHtj::<SipHashStrategy, NoPruning, LazyExpansion>::new(
                HashTrieConfig::default(),
                hash_two,
                Planner::stored(LexicographicOptimiser)
            )
            .build_mode_axes(),
            build_mode_axis("parallel:2")
        );
    }

    thread_local! {
        static HASHES: Cell<usize> = const { Cell::new(0) };
    }

    /// SipHash that counts its calls. The radix build hashes each tuple's
    /// first attribute once more than the serial build, and the two build
    /// identical tries, so the count is the only way to see which one ran.
    #[derive(Copy, Clone, Default, Debug)]
    struct CountingHash;

    impl LayoutOption for CountingHash {
        const NAME: &'static str = "counting";
    }

    impl HashStrategy for CountingHash {
        fn hash(key: usize) -> u64 {
            HASHES.set(HASHES.get() + 1);
            SipHashStrategy::hash(key)
        }
    }

    /// Both modes build identical tries, so only a spy can see whether the
    /// family's mode reached the build, on every route a relation is built,
    /// eager or lazy.
    #[test]
    fn hash_families_build_with_their_mode() {
        fn check<E: ExpansionPolicy>() {
            let dir = tempfile::tempdir().expect("tempdir");
            let path = dir.path().join("r.csv");
            std::fs::write(&path, "a,b\n1,2\n1,3\n2,4\n").expect("write csv");
            let header = || RelationHeader::new_positional("r", 2);
            let tuples = || vec![vec![1, 2], vec![1, 3], vec![2, 4]];
            let config = HashTrieConfig::default();
            let structure = |mode| HashTrieFamily::<CountingHash, NoPruning, E>::new(config, mode);
            let join = |mode| {
                HashHtj::<CountingHash, NoPruning, E>::new(
                    config,
                    mode,
                    Planner::stored(LexicographicOptimiser),
                )
            };
            let routes: [(&str, &dyn Fn(HashTrieBuildMode)); 5] = [
                ("HashTrieFamily::build_relation", &|mode| {
                    structure(mode).build_relation(header(), tuples());
                }),
                ("HashTrieFamily::load_with_tuples", &|mode| {
                    structure(mode).load_with_tuples(&path).expect("load");
                }),
                ("HashHtj::build_relation", &|mode| {
                    join(mode).build_relation(header(), tuples());
                }),
                ("HashHtj::load", &|mode| {
                    join(mode).load(&path).expect("load");
                }),
                ("HashHtj::build_from_tuples", &|mode| {
                    join(mode).build_from_tuples(vec![(header(), tuples())]);
                }),
            ];
            let hashes = |build: &dyn Fn()| {
                HASHES.set(0);
                build();
                HASHES.get()
            };
            let radix = HashTrieBuildMode::Radix(RadixBits::new(2).unwrap());
            for (route, build) in routes {
                let serial = hashes(&|| build(HashTrieBuildMode::Serial));
                let radixed = hashes(&|| build(radix));
                assert!(
                    radixed > serial,
                    "{} {route}: the radix build hashed {radixed} times and the serial build \
                     {serial}: the mode did not reach the build",
                    E::NAME
                );
            }
        }
        check::<EagerExpansion>();
        check::<LazyExpansion>();
    }

    /// The real `HashTrie`, not the counting spy, which cannot tell
    /// `parallel:N` from `radix:K`: only kermit-ds's record of parallel
    /// builds (the `test-hooks` feature) shows that the family's mode reached
    /// the build, on every route a relation is built, copies included,
    /// eager or lazy (#94).
    #[test]
    fn hash_trie_families_build_with_their_parallel_mode() {
        fn check<E: ExpansionPolicy>() {
            let dir = tempfile::tempdir().expect("tempdir");
            let path = dir.path().join("r.csv");
            std::fs::write(&path, "a,b\n1,2\n2,1\n3,4\n").expect("write csv");
            let header = || RelationHeader::new_positional("r", 2);
            let tuples = || vec![vec![1, 2], vec![2, 1], vec![3, 4]];
            // The record of every parallel build `build` runs.
            let parallel_builds = |build: &dyn Fn()| {
                kermit_ds::test_hooks::take_hash_trie_parallel_builds();
                build();
                kermit_ds::test_hooks::take_hash_trie_parallel_builds()
            };
            let presized = HashTrieConfig {
                root_capacity: RootCapacity::Tuples,
                ..HashTrieConfig::default()
            };
            let radix = HashTrieBuildMode::Radix(RadixBits::new(2).unwrap());
            // Three threads, not two: a refactor that dropped N on the way to
            // the build would show here as well as in kermit-ds.
            let three = HashTrieBuildMode::Parallel(kermit_ds::Threads::new(3).unwrap());
            // Under `root-capacity=tuples`, `parallel:N` must take the
            // presized path (#94), whose records carry a deferred count.
            for config in [HashTrieConfig::default(), presized] {
                let presized = config.root_capacity == RootCapacity::Tuples;
                for (mode, expected) in [
                    (HashTrieBuildMode::Serial, vec![]),
                    (radix, vec![]),
                    (three, vec![3]),
                ] {
                    let structure =
                        HashTrieFamily::<SipHashStrategy, NoPruning, E>::new(config, mode);
                    let join = HashHtj::<SipHashStrategy, NoPruning, E>::new(
                        config,
                        mode,
                        Planner::stored(LexicographicOptimiser),
                    );
                    let routes: [(&str, &dyn Fn()); 6] = [
                        ("HashTrieFamily::build_relation", &|| {
                            structure.build_relation(header(), tuples());
                        }),
                        ("HashTrieFamily::load_with_tuples", &|| {
                            structure.load_with_tuples(&path).expect("load");
                        }),
                        ("HashHtj::build_relation", &|| {
                            join.build_relation(header(), tuples());
                        }),
                        ("HashHtj::load", &|| {
                            join.load(&path).expect("load");
                        }),
                        ("HashHtj::build_from_tuples", &|| {
                            join.build_from_tuples(vec![(header(), tuples())]);
                        }),
                        ("HashHtj::add_index", &|| {
                            let mut engine = join.build(Vec::new());
                            let spec = IndexSpec::new("r", vec![1, 0]);
                            join.add_index(&mut engine, spec, &header(), &tuples());
                        }),
                    ];
                    for (route, build) in routes {
                        let builds = parallel_builds(build);
                        let threads: Vec<usize> = builds.iter().map(|b| b.threads).collect();
                        assert_eq!(
                            threads,
                            expected,
                            "{} {route} under {mode:?}, {config:?}",
                            E::NAME
                        );
                        assert!(
                            builds.iter().all(|b| b.deferred.is_some() == presized),
                            "{} {route} under {mode:?}, {config:?}: wrong path",
                            E::NAME
                        );
                    }
                }
            }
        }
        check::<EagerExpansion>();
        check::<LazyExpansion>();
    }

    /// Each structure's `--ds-build` mode reaches its own cell of a sweep
    /// and no other.
    #[test]
    fn sweep_attaches_each_build_mode_to_its_own_cell() {
        let radix = HashTrieBuildMode::Radix(RadixBits::new(4).unwrap());
        let four = TreeTrieBuildMode::Parallel(kermit_ds::Threads::new(4).unwrap());
        let choices = DsChoices {
            build: BuildModes {
                tree_trie: four,
                column_trie: ColumnTrieBuildMode::Incremental,
                hash_trie: radix,
            },
            ..DsChoices::default()
        };
        let sweep = Sweep::expand(&all_structures(), &all_algorithms(), choices);
        assert!(sweep
            .cells
            .contains(&Execution::TrieLftj(SortedTrie::ColumnTrie {
                seek: SeekChoice::Galloping,
                build: ColumnTrieBuildMode::Incremental,
            })));
        assert!(sweep
            .cells
            .contains(&Execution::TrieLftj(SortedTrie::TreeTrie {
                seek: SeekChoice::Galloping,
                build: four,
            })));
        assert!(sweep.cells.contains(&Execution::HashHtj {
            hasher: HasherChoice::Sip,
            pruning: PruningChoice::Off,
            expansion: ExpansionChoice::Eager,
            config: HashTrieConfig::default(),
            build: radix,
        }));
    }

    /// Each sorted family reports the seek strategy its type parameter
    /// implies, so the label cannot disagree with the code that ran; and a
    /// sweep attaches the `--ds-layout-seek` choice to both sorted cells.
    #[test]
    fn sorted_families_label_their_seek_strategy_from_the_type() {
        use kermit_ds::{BinarySeek, GallopingSeek, LinearSeek};
        assert_eq!(
            TrieLftj::<TreeTrie<LinearSeek>>::new(
                TreeTrieBuildMode::default(),
                Planner::stored(LexicographicOptimiser)
            )
            .execution(),
            Execution::TrieLftj(SortedTrie::TreeTrie {
                seek: SeekChoice::Linear,
                build: TreeTrieBuildMode::Serial,
            })
        );
        assert_eq!(
            TrieLftj::<ColumnTrie<GallopingSeek>>::new(
                ColumnTrieBuildMode::Incremental,
                Planner::stored(LexicographicOptimiser)
            )
            .execution(),
            Execution::TrieLftj(SortedTrie::ColumnTrie {
                seek: SeekChoice::Galloping,
                build: ColumnTrieBuildMode::Incremental
            })
        );
        assert_eq!(
            SortedTrieFamily::<TreeTrie<BinarySeek>>::default().execution(),
            Execution::TrieLftj(SortedTrie::TreeTrie {
                seek: SeekChoice::Binary,
                build: TreeTrieBuildMode::Serial,
            })
        );
        let choices = DsChoices {
            seek: SeekChoice::Linear,
            ..DsChoices::default()
        };
        let sweep = Sweep::expand(&all_structures(), &all_algorithms(), choices);
        assert!(sweep
            .cells
            .contains(&Execution::TrieLftj(SortedTrie::TreeTrie {
                seek: SeekChoice::Linear,
                build: TreeTrieBuildMode::Serial,
            })));
        assert!(sweep
            .cells
            .contains(&Execution::TrieLftj(SortedTrie::ColumnTrie {
                seek: SeekChoice::Linear,
                build: ColumnTrieBuildMode::default()
            })));
    }

    /// A sorted family's report axes are the relation's own.
    #[test]
    fn sorted_families_report_the_relations_seek_axis() {
        use kermit_ds::GallopingSeek;
        let family = TrieLftj::<TreeTrie<GallopingSeek>>::new(
            TreeTrieBuildMode::default(),
            Planner::stored(LexicographicOptimiser),
        );
        let rel = family.build_relation(RelationHeader::new_positional("r", 2), vec![vec![1, 2]]);
        assert_eq!(
            TrieLftj::<TreeTrie<GallopingSeek>>::optimization_axes(&rel)["ds_layout_seek"],
            "galloping"
        );
    }

    /// A copy is built through `build_relation`, so it carries the
    /// family's build mode (seen by the spy) and config (seen on the
    /// copy), and the engine holds it until cleared.
    #[test]
    fn families_build_copies_through_build_relation() {
        let header = RelationHeader::new_positional("edge", 2);
        let edges = vec![vec![1, 2], vec![1, 3], vec![2, 3]];
        let spec = || IndexSpec::new("edge", vec![1, 0]);

        let join = TrieLftj::<Spy>::new(
            ColumnTrieBuildMode::Incremental,
            Planner::stored(LexicographicOptimiser),
        );
        let mut engine = join.build(vec![join.build_relation(header.clone(), edges.clone())]);
        BUILT_WITH.take();
        join.add_index(&mut engine, spec(), &header, &edges);
        assert_eq!(BUILT_WITH.take(), Some(ColumnTrieBuildMode::Incremental));
        let held: Vec<&IndexSpec> = TrieLftj::<Spy>::indexes(&engine)
            .into_iter()
            .map(|(spec, _)| spec)
            .collect();
        assert_eq!(held, vec![&spec()]);
        let copy = engine.index("Index_1_0_edge").unwrap();
        assert_eq!(copy.header().name(), "Index_1_0_edge");
        assert_eq!(copy.header().arity(), 2);
        TrieLftj::<Spy>::clear_indexes(&mut engine);
        assert!(TrieLftj::<Spy>::indexes(&engine).is_empty());

        let config = HashTrieConfig {
            load_factor: LoadFactor::percent(50).unwrap(),
            ..HashTrieConfig::default()
        };
        let hash = HashHtj::<SipHashStrategy, NoPruning, EagerExpansion>::new(
            config,
            HashTrieBuildMode::Serial,
            Planner::stored(LexicographicOptimiser),
        );
        let mut engine = hash.build(vec![hash.build_relation(header.clone(), edges.clone())]);
        hash.add_index(&mut engine, spec(), &header, &edges);
        let (_, copy) = HashHtj::<SipHashStrategy, NoPruning, EagerExpansion>::indexes(&engine)[0];
        assert_eq!(copy.config(), &config);
        assert_eq!(copy.header().name(), "Index_1_0_edge");
    }

    /// `required_indexes` goes through the engine's planner: the cyclic
    /// query needs one copy under `any` and is rejected under `stored`.
    #[test]
    fn required_indexes_follow_the_familys_planner() {
        let header = || RelationHeader::new_positional("edge", 2);
        let edges = || vec![vec![1, 2], vec![2, 1]];
        let query: JoinQuery = "Q(X, Y) :- edge(X, Y), edge(Y, X).".parse().unwrap();

        let stored = TrieLftj::<TreeTrie>::new(
            TreeTrieBuildMode::default(),
            Planner::stored(LexicographicOptimiser),
        );
        let engine = stored.build(vec![stored.build_relation(header(), edges())]);
        assert!(matches!(
            stored.required_indexes(&engine, &query),
            Err(JoinError::CyclicAttributeOrder { .. })
        ));

        let any = TrieLftj::<TreeTrie>::new(
            TreeTrieBuildMode::default(),
            Planner::new(Box::new(LexicographicOptimiser), ColumnOrderPolicy::Any),
        );
        let mut engine = any.build(vec![any.build_relation(header(), edges())]);
        let specs = any.required_indexes(&engine, &query).unwrap();
        assert_eq!(specs, vec![IndexSpec::new("edge", vec![1, 0])]);
        for spec in specs {
            any.add_index(&mut engine, spec, &header(), &edges());
        }
        assert_eq!(any.count(&engine, query.clone()).unwrap(), 2);
        assert!(any.required_indexes(&engine, &query).unwrap().is_empty());
    }
}
