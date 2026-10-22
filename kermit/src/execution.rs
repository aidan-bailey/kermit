//! Execution cells for `bench run` and `bench ds`.
//!
//! The CLI exposes an index structure (`-i`) and a join algorithm (`-a`)
//! as two independent selectors, but only three of the six concrete
//! `(IndexStructure, JoinAlgorithm)` pairs are meaningful: the sorted
//! tries pair with [`LeapfrogTriejoin`](kermit_algos::LeapfrogTriejoin)
//! and the hash trie pairs with
//! [`HashTriejoin`]. The two families run
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
        self, hash_join_for_each, index_header, lftj_join_for_each, Catalog, Database, HashFamily,
        Indexes, JoinError, Prepared, SortedFamily,
    },
    kermit_algos::{CopySpec, HashTriejoin, JoinAlgorithm, JoinQuery, LeapfrogTriejoin, Planner},
    kermit_ds::{
        BuildModeRelation, Cardinality, ColumnTrie, ColumnTrieBuildMode,
        ConfiguredBuildModeRelation, ExpansionPolicy, HashTrie, HashTrieBuildMode, HashTrieConfig,
        HeapSize, IndexStructure, PruningPolicy, Relation, RelationFileExt, RelationHeader,
        SeekStrategy, TreeTrie, TreeTrieBuildMode, TupleView, Tuples,
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

    /// Builds one relation from the rows `view` selects by `build`: the rows
    /// are copied once, however `build` arranges them.
    fn build_with(header: RelationHeader, build: Self::BuildMode, view: TupleView<'_>) -> Self;

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

    fn build_with(header: RelationHeader, build: TreeTrieBuildMode, view: TupleView<'_>) -> Self {
        Self::from_view_with_build_mode(header, build, view)
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

    fn build_with(header: RelationHeader, build: ColumnTrieBuildMode, view: TupleView<'_>) -> Self {
        Self::from_view_with_build_mode(header, build, view)
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

    /// Builds one relation from a header and a view of its source tuples,
    /// honouring the family's configuration and build mode.
    ///
    /// Every relation the family builds routes through this one site: the
    /// indexes `ExecutionFamily::build_indexes` builds and, through
    /// [`build_relation`](Self::build_relation), [`load`](Self::load),
    /// [`load_with_tuples`](Self::load_with_tuples) and both families'
    /// [`build_from_tuples`]. So the `build` metric can never
    /// build a relation the report's `ds_config_*` / `ds_build_mode` axes
    /// fail to describe. Required, with no default, so no family can
    /// silently fall back to `Relation::from_view` and build with a
    /// configuration or mode its report does not name.
    ///
    /// [`build_from_tuples`]: ExecutionFamily::build_from_tuples
    fn build_index(&self, header: RelationHeader, view: TupleView<'_>) -> Self::Rel;

    /// Builds one relation from a header and its tuples: [`build_index`]
    /// over the identity view of `tuples`, so the old and the per-query
    /// paths cannot disagree about a structure.
    ///
    /// [`build_index`]: Self::build_index
    fn build_relation(&self, header: RelationHeader, tuples: Tuples) -> Self::Rel {
        self.build_index(header, TupleView::all(&tuples))
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
    #[allow(dead_code)] // Unused until Story 19
    fn load(&self, path: &Path) -> anyhow::Result<Self::Rel> {
        let (header, tuples) = read_relation(path)?;
        Ok(self.build_relation(header, tuples))
    }

    /// Loads one relation file like [`load`](Self::load), and also returns
    /// the tuples it was built from, in the order the reader produced them.
    ///
    /// These are the input of every copy built from file order (the
    /// reordered copies under `--column-orders any`). A structure's own
    /// iteration order is not
    /// neutral input — it is the sorted tries' best case, and it made the
    /// hash trie's build quadratic (issue #66) — whereas file order is the
    /// same for every structure and every Layout, so all of them build from
    /// identical input. The relation is built from a copy of the reader's
    /// buffer, one allocation, and the reader's buffer is returned (#111).
    ///
    /// # Errors
    ///
    /// As [`load`](Self::load).
    #[allow(dead_code)] // Unused until Story 19
    fn load_with_tuples(&self, path: &Path) -> anyhow::Result<(Self::Rel, Tuples)> {
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
pub(crate) fn read_relation(path: &Path) -> anyhow::Result<(RelationHeader, Tuples)> {
    let extension = path.extension().and_then(|s| s.to_str()).unwrap_or("");
    match extension.to_lowercase().as_str() {
        | "csv" => kermit_ds::read_csv(path),
        | "parquet" => kermit_ds::read_parquet(path),
        | _ => anyhow::bail!("Unsupported file extension for {path:?}: '{extension}'"),
    }
    .map_err(|e| anyhow::anyhow!("Failed to load {path:?}: {e}"))
}

/// The join-facing half of a family: building the indexes a prepared query
/// reads and executing it over them (`build_indexes`, `execute`,
/// `count_rows`), plus the engine-based path `kermit join` and `bench join
/// --output` still use. Everything else in a `bench run` cell —
/// query parsing, metadata, Criterion group wiring, report assembly — is
/// shared by the generic runner in `bench/run.rs`.
///
/// The `build` and `end_to_end` metrics time
/// [`ExecutionFamily::build_indexes`], the same call that builds the indexes
/// `iteration` and `space` measure, so the build term is the cost of the build
/// those indexes paid. The engine-based [`ExecutionFamily::build`] and
/// [`ExecutionFamily::build_from_tuples`] must still construct the engine the
/// same way until Story 19 deletes them.
pub trait ExecutionFamily: RelationFamily {
    /// The built, queryable form of a set of relations.
    type Engine;

    /// Whether running a join can change the engine's relations: a lazy
    /// `HashTrie` builds each child a join first reaches (issue #92). When
    /// true, `bench run` never probes the indexes it built for `space`.
    /// `--verify` and `iteration` run on fresh builds, so `space` measures
    /// the relations as built and each timed join pays its own expansion.
    /// Required, like `build_relation`: every family states what its joins
    /// do.
    const JOIN_MUTATES: bool;

    /// Builds the indexes `prepared` reads from the relations of `catalog`,
    /// one [`build_index`](RelationFamily::build_index) per index spec: the
    /// timed `build` of the per-query pipeline. Each index is a view of its
    /// relation's flat tuples, filtered by the query's constants and repeated
    /// variables and copied once, with its columns in plan order.
    fn build_indexes(&self, catalog: &Catalog, prepared: &Prepared) -> Indexes<Self::Rel> {
        db::build_with(catalog, prepared, |h, v| self.build_index(h, v))
    }

    /// Runs `prepared` over its `indexes`, passing each result row, the
    /// head's columns, to `emit` as a borrowed slice; the result is never
    /// materialised. `indexes` must come from
    /// [`build_indexes`](Self::build_indexes) on the same `prepared`. When
    /// [`JOIN_MUTATES`](Self::JOIN_MUTATES), the join expands the indexes it
    /// is given, as `join_for_each` expands its engine.
    fn execute<S: FnMut(&[usize])>(
        &self, prepared: &Prepared, indexes: &Indexes<Self::Rel>, emit: S,
    );

    /// Runs `prepared` over its `indexes` and counts its result rows,
    /// passing each through [`std::hint::black_box`] so the traversal cannot
    /// be optimised away. Allocates no result rows: this is what the
    /// per-query `iteration` and `end_to_end` metrics will time and what
    /// `--verify` will check, as [`count`](Self::count) does for an engine.
    fn count_rows(&self, prepared: &Prepared, indexes: &Indexes<Self::Rel>) -> u64 {
        let mut rows = 0u64;
        self.execute(prepared, indexes, |row| {
            std::hint::black_box(row);
            rows += 1;
        });
        rows
    }

    /// Builds the engine from freshly loaded relations, retaining them for
    /// the copies a query's plan reads.
    #[allow(dead_code)] // Unused until Story 19
    fn build(&self, relations: Vec<Self::Rel>) -> Self::Engine;

    /// Builds an engine from `(header, tuples)` snapshots along the same
    /// path as [`ExecutionFamily::build`]. Used inside the timed
    /// `end_to_end` body, where only [`ExecutionFamily::count`] is needed
    /// afterwards.
    #[allow(dead_code)] // Unused until Story 19
    fn build_from_tuples(&self, inputs: Vec<(RelationHeader, Tuples)>) -> Self::Engine;

    /// The relations an engine built by [`ExecutionFamily::build`] retains.
    #[allow(dead_code)] // Unused until Story 19
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
    #[allow(dead_code)] // Unused until Story 19
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
    #[allow(dead_code)] // Unused until Story 19
    fn required_indexes(
        &self, engine: &Self::Engine, query: &JoinQuery,
    ) -> Result<Vec<CopySpec>, JoinError>;

    /// Builds `spec`'s copy from its base relation's header and file-order
    /// `tuples` through [`build_relation`](RelationFamily::build_relation),
    /// so the copy carries the configuration and build mode the report's
    /// axes name, and adds it to `engine`.
    #[allow(dead_code)] // Unused until Story 19
    fn add_index(
        &self, engine: &mut Self::Engine, spec: CopySpec, base: &RelationHeader, tuples: &Tuples,
    );

    /// Drops every copy `engine` holds.
    #[allow(dead_code)] // Unused until Story 19
    fn clear_indexes(engine: &mut Self::Engine);

    /// Every copy `engine` holds, with its spec: the `space/Index_*`
    /// functions.
    #[allow(dead_code)] // Unused until Story 19
    fn indexes(engine: &Self::Engine) -> Vec<(&CopySpec, &Self::Rel)>;
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

    fn build_index(&self, header: RelationHeader, view: TupleView<'_>) -> R {
        R::build_with(header, self.build, view)
    }

    fn for_each_tuple<V: FnMut(&[usize])>(rel: &R, mut visit: V) {
        let mut tuples = TrieIteratorWrapper::new(rel.trie_iter());
        while let Some(tuple) = tuples.advance() {
            visit(tuple);
        }
    }

    /// An O(1) read of the stored count ([`Cardinality`], a bound of
    /// `SortedTrieRelation`): no walk, no allocation.
    fn tuple_count(rel: &R) -> usize { Cardinality::tuple_count(rel) }

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

    fn build_index(&self, header: RelationHeader, view: TupleView<'_>) -> HashTrie<H, P, E> {
        <HashTrie<H, P, E> as ConfiguredBuildModeRelation>::from_view_with_config_and_build_mode(
            header,
            self.config,
            self.build,
            view,
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
    /// read a `HashTrie` report without the axis, or with the pre-#107
    /// `serial`, as the `incremental` build, the only one before issue #91.
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

    fn build_index(&self, header: RelationHeader, view: TupleView<'_>) -> R {
        self.structure.build_index(header, view)
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

    fn execute<S: FnMut(&[usize])>(&self, prepared: &Prepared, indexes: &Indexes<R>, emit: S) {
        db::execute::<R, LeapfrogTriejoin>(prepared, indexes, emit);
    }

    fn build(&self, relations: Vec<R>) -> Self::Engine {
        let relations = relations
            .into_iter()
            .map(|r| (r.header().name().to_string(), r))
            .collect();
        Database::new::<SortedFamily>(relations, self.planner.required_statistics())
    }

    fn build_from_tuples(&self, inputs: Vec<(RelationHeader, Tuples)>) -> Self::Engine {
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
    ) -> Result<Vec<CopySpec>, JoinError> {
        engine.required_indexes(query, &self.planner)
    }

    fn add_index(
        &self, engine: &mut Self::Engine, spec: CopySpec, base: &RelationHeader, tuples: &Tuples,
    ) {
        let copy = self.build_relation(index_header(&spec, base), spec.permute_all(tuples));
        engine.add_index(spec, copy);
    }

    fn clear_indexes(engine: &mut Self::Engine) { engine.clear_indexes(); }

    fn indexes(engine: &Self::Engine) -> Vec<(&CopySpec, &Self::Rel)> { engine.indexes().collect() }
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

    fn build_index(&self, header: RelationHeader, view: TupleView<'_>) -> HashTrie<H, P, E> {
        self.structure.build_index(header, view)
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

    fn execute<S: FnMut(&[usize])>(
        &self, prepared: &Prepared, indexes: &Indexes<HashTrie<H, P, E>>, emit: S,
    ) {
        db::execute::<HashTrie<H, P, E>, HashTriejoin>(prepared, indexes, emit);
    }

    fn build(&self, relations: Vec<HashTrie<H, P, E>>) -> Self::Engine {
        let relations = relations
            .into_iter()
            .map(|r| (r.header().name().to_string(), r))
            .collect();
        Database::new::<HashFamily<H>>(relations, self.planner.required_statistics())
    }

    fn build_from_tuples(&self, inputs: Vec<(RelationHeader, Tuples)>) -> Self::Engine {
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
    ) -> Result<Vec<CopySpec>, JoinError> {
        engine.required_indexes(query, &self.planner)
    }

    fn add_index(
        &self, engine: &mut Self::Engine, spec: CopySpec, base: &RelationHeader, tuples: &Tuples,
    ) {
        let copy = self.build_relation(index_header(&spec, base), spec.permute_all(tuples));
        engine.add_index(spec, copy);
    }

    fn clear_indexes(engine: &mut Self::Engine) { engine.clear_indexes(); }

    fn indexes(engine: &Self::Engine) -> Vec<(&CopySpec, &Self::Rel)> { engine.indexes().collect() }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::options::BuildModes,
        clap::ValueEnum,
        kermit_algos::LexicographicOptimiser,
        kermit_ds::{
            ChildCapacity, ConfigurableRelation, EagerExpansion, LazyExpansion, LoadFactor,
            NoPruning, RadixBits, RootCapacity, SingletonPruning,
        },
        kermit_iters::{LayoutOption, SipHashStrategy},
        std::cell::Cell,
    };

    /// `query` prepared over a catalog holding `tuples` as the relation
    /// `header` names: what a family's `build_indexes` and `execute` take.
    fn prepared_over(
        header: RelationHeader, tuples: Tuples, query: &str,
    ) -> (kermit::db::Catalog, kermit::db::Prepared) {
        use {
            kermit::db::{prepare, Catalog},
            kermit_algos::{
                ColumnOrderPolicy, JoinQuery, LexicographicOptimiser, Planner, StatisticsLevel,
            },
        };
        let catalog = Catalog::new(vec![(header, tuples)], StatisticsLevel::TupleCounts);
        let query: JoinQuery = query.parse().expect("the query parses");
        let planner = Planner::new(Box::new(LexicographicOptimiser), ColumnOrderPolicy::Any);
        let prepared = prepare(&catalog, &query, &planner).expect("the query prepares");
        (catalog, prepared)
    }

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
                build: HashTrieBuildMode::Bulk,
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
                [HashTrieBuildMode::Bulk, radix].map(|hash_trie| BuildModes {
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
                HashTrieBuildMode::Bulk,
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
        let tuples = Tuples::from(vec![vec![1, 2], vec![1, 2], vec![3, 4]]);
        let rel = family.build_index(header, kermit_ds::TupleView::all(&tuples));
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
        let tuples = || Tuples::from(vec![vec![1, 2], vec![1, 2], vec![1, 3], vec![4, 5]]);

        let tree = SortedTrieFamily::<TreeTrie>::default()
            .build_index(header(), kermit_ds::TupleView::all(&tuples()));
        assert_eq!(SortedTrieFamily::<TreeTrie>::scan(&tree), 3);
        assert_eq!(SortedTrieFamily::<TreeTrie>::tuple_count(&tree), 3);
        assert_eq!(TrieLftj::<TreeTrie>::scan(&tree), 3);

        let column = SortedTrieFamily::<ColumnTrie>::default()
            .build_index(header(), kermit_ds::TupleView::all(&tuples()));
        assert_eq!(SortedTrieFamily::<ColumnTrie>::scan(&column), 3);
        assert_eq!(SortedTrieFamily::<ColumnTrie>::tuple_count(&column), 3);

        let hash = HashTrieFamily::<SipHashStrategy, NoPruning, EagerExpansion>::default()
            .build_index(header(), kermit_ds::TupleView::all(&tuples()));
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
            .build_index(header(), kermit_ds::TupleView::all(&tuples()));
        assert_eq!(
            HashTrieFamily::<SipHashStrategy, SingletonPruning, EagerExpansion>::scan(&pruned),
            4
        );
    }

    /// The `build` metric builds through [`RelationFamily::build_index`], so
    /// it must honour the config too — otherwise it would time an
    /// unconfigured build under a report labelled with the configured axes.
    #[test]
    fn hash_family_build_index_honours_its_config() {
        let config = HashTrieConfig {
            load_factor: kermit_ds::LoadFactor::percent(50).unwrap(),
            ..HashTrieConfig::default()
        };
        let family = HashHtj::<kermit_iters::SipHashStrategy, NoPruning, EagerExpansion>::new(
            config,
            HashTrieBuildMode::Bulk,
            Planner::stored(LexicographicOptimiser),
        );
        let header = RelationHeader::new("r", vec!["a".to_string(), "b".to_string()]);
        let tuples = Tuples::from(vec![vec![1, 2]]);
        let rel = family.build_index(header, kermit_ds::TupleView::all(&tuples));
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
    fn hash_family_build_index_presizes_the_root_under_tuples() {
        let heap = |root_capacity| {
            let family = HashHtj::<SipHashStrategy, NoPruning, EagerExpansion>::new(
                HashTrieConfig {
                    root_capacity,
                    ..HashTrieConfig::default()
                },
                HashTrieBuildMode::Bulk,
                Planner::stored(LexicographicOptimiser),
            );
            let header = RelationHeader::new("r", vec!["a".to_string(), "b".to_string()]);
            let tuples = Tuples::from((0..100).map(|b| vec![1, b]).collect::<Vec<Vec<usize>>>());
            family
                .build_index(header, kermit_ds::TupleView::all(&tuples))
                .heap_size_bytes()
        };
        assert!(heap(RootCapacity::Tuples) > heap(RootCapacity::Grow));
    }

    /// Under `child-capacity=tuples` the family's build sizes children from
    /// their lists, so a report labelled `"tuples"` timed sized children.
    /// Three equal tuples share one child: grown, it keeps 4 buckets; sized
    /// for its 3 tuples at 70 % it has 8. The root is presized in both arms,
    /// so only the children can differ and a `child-capacity` that resized
    /// the root instead would not pass.
    #[test]
    fn hash_family_build_index_sizes_children_under_tuples() {
        let heap = |child_capacity| {
            let family = HashHtj::<SipHashStrategy, NoPruning, EagerExpansion>::new(
                HashTrieConfig {
                    root_capacity: RootCapacity::Tuples,
                    child_capacity,
                    ..HashTrieConfig::default()
                },
                HashTrieBuildMode::Bulk,
                Planner::stored(LexicographicOptimiser),
            );
            let header = RelationHeader::new("r", vec!["a".to_string(), "b".to_string()]);
            let tuples = Tuples::from(vec![vec![1, 1]; 3]);
            family
                .build_index(header, kermit_ds::TupleView::all(&tuples))
                .heap_size_bytes()
        };
        assert!(heap(ChildCapacity::Tuples) > heap(ChildCapacity::Grow));
    }

    /// The pruning Layout reaches the relations the family builds, so a
    /// report's `ds_layout_pruning` axis describes the structure that ran.
    #[test]
    fn pruned_family_reports_the_pruning_layout() {
        let family =
            HashHtj::<kermit_iters::SipHashStrategy, SingletonPruning, EagerExpansion>::new(
                HashTrieConfig::default(),
                HashTrieBuildMode::Bulk,
                Planner::stored(LexicographicOptimiser),
            );
        let header = RelationHeader::new("r", vec!["a".to_string(), "b".to_string()]);
        let tuples = Tuples::from(vec![vec![1, 2]]);
        let rel = family.build_index(header, kermit_ds::TupleView::all(&tuples));
        assert_eq!(
            HashHtj::<kermit_iters::SipHashStrategy, SingletonPruning, EagerExpansion>::optimization_axes(&rel)
                .get("ds_layout_pruning"),
            Some(&serde_json::Value::String("on".into()))
        );
    }

    /// `count_rows` (what `iteration` and `--verify` time and check) and the
    /// rows `execute` streams (what `kermit join` writes) come from one
    /// traversal: they must agree in every family.
    #[test]
    fn count_rows_agrees_with_execute_in_every_family() {
        fn check<F: ExecutionFamily>(family: &F) {
            // Triangles in this graph: (1, 2, 3) and (2, 3, 4).
            let edges = Tuples::from(vec![vec![1, 2], vec![2, 3], vec![1, 3], vec![3, 4], vec![
                2, 4,
            ]]);
            let (catalog, prepared) = prepared_over(
                RelationHeader::new_positional("edge", 2),
                edges,
                "Q(X, Y, Z) :- edge(X, Y), edge(Y, Z), edge(X, Z).",
            );
            let indexes = family.build_indexes(&catalog, &prepared);
            let mut streamed = Vec::new();
            family.execute(&prepared, &indexes, |row| streamed.push(row.to_vec()));
            streamed.sort();
            assert_eq!(streamed, vec![vec![1, 2, 3], vec![2, 3, 4]]);
            assert_eq!(family.count_rows(&prepared, &indexes), 2);
        }
        check(&TrieLftj::<TreeTrie>::new(
            TreeTrieBuildMode::default(),
            Planner::stored(LexicographicOptimiser),
        ));
        check(&TrieLftj::<ColumnTrie>::new(
            ColumnTrieBuildMode::default(),
            Planner::stored(LexicographicOptimiser),
        ));
        check(&HashHtj::<SipHashStrategy, NoPruning, EagerExpansion>::new(
            HashTrieConfig::default(),
            HashTrieBuildMode::Bulk,
            Planner::stored(LexicographicOptimiser),
        ));
        check(
            &HashHtj::<SipHashStrategy, SingletonPruning, LazyExpansion>::new(
                HashTrieConfig::default(),
                HashTrieBuildMode::Bulk,
                Planner::stored(LexicographicOptimiser),
            ),
        );
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
            header: RelationHeader, build: ColumnTrieBuildMode, view: TupleView<'_>,
        ) -> Self {
            BUILT_WITH.set(Some(build));
            <Spy as Relation>::from_view(header, view)
        }

        fn build_mode_axes(build: ColumnTrieBuildMode) -> BTreeMap<String, serde_json::Value> {
            <ColumnTrie as SortedTrieRelation>::build_mode_axes(build)
        }
    }

    /// Both modes build identical tries, so only a spy can see whether the
    /// family's mode reached the build, on every route a relation is built.
    #[test]
    fn sorted_families_build_with_their_mode() {
        let header = || RelationHeader::new_positional("r", 2);
        let tuples = || Tuples::from(vec![vec![1, 2]]);
        // `r` read with its columns swapped: `build_indexes` builds a
        // reordered index.
        let (catalog, prepared) = prepared_over(header(), tuples(), "Q(X, Y) :- r(Y, X).");
        // The mode `build` hands to `build_with`, or `None` if it never does.
        let seen = |build: &dyn Fn()| {
            BUILT_WITH.take();
            build();
            BUILT_WITH.take()
        };

        for mode in [ColumnTrieBuildMode::Incremental, ColumnTrieBuildMode::Bulk] {
            let structure = SortedTrieFamily::<Spy>::new(mode);
            let join = TrieLftj::<Spy>::new(mode, Planner::stored(LexicographicOptimiser));
            let routes: [(&str, &dyn Fn()); 3] = [
                ("SortedTrieFamily::build_index", &|| {
                    structure.build_index(header(), kermit_ds::TupleView::all(&tuples()));
                }),
                ("TrieLftj::build_index", &|| {
                    join.build_index(header(), kermit_ds::TupleView::all(&tuples()));
                }),
                ("TrieLftj::build_indexes", &|| {
                    join.build_indexes(&catalog, &prepared);
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
    /// `TreeTrie::from_view_with_build_mode`, on every route a relation is
    /// built, reordered indexes included (#94).
    #[test]
    fn tree_trie_families_build_with_their_mode() {
        let header = || RelationHeader::new_positional("r", 2);
        let tuples = || Tuples::from(vec![vec![1, 2], vec![2, 1], vec![3, 4]]);
        let (catalog, prepared) = prepared_over(header(), tuples(), "Q(X, Y) :- r(Y, X).");
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
            let routes: [(&str, &dyn Fn()); 3] = [
                ("SortedTrieFamily::build_index", &|| {
                    structure.build_index(header(), kermit_ds::TupleView::all(&tuples()));
                }),
                ("TrieLftj::build_index", &|| {
                    join.build_index(header(), kermit_ds::TupleView::all(&tuples()));
                }),
                ("TrieLftj::build_indexes", &|| {
                    join.build_indexes(&catalog, &prepared);
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
            build_mode_axis("bulk")
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
    /// first attribute once more than the bulk build (it partitions on that
    /// hash first), and the two build tries of the same contents, so this
    /// test tells them apart by the count.
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

    /// Both modes build tries of the same contents, so only a spy can see
    /// whether the family's mode reached the build, on every route a
    /// relation is built, eager or lazy.
    #[test]
    fn hash_families_build_with_their_mode() {
        fn check<E: ExpansionPolicy>() {
            let header = || RelationHeader::new_positional("r", 2);
            let tuples = || Tuples::from(vec![vec![1, 2], vec![1, 3], vec![2, 4]]);
            let (catalog, prepared) = prepared_over(header(), tuples(), "Q(X, Y) :- r(Y, X).");
            let config = HashTrieConfig::default();
            let structure = |mode| HashTrieFamily::<CountingHash, NoPruning, E>::new(config, mode);
            let join = |mode| {
                HashHtj::<CountingHash, NoPruning, E>::new(
                    config,
                    mode,
                    Planner::stored(LexicographicOptimiser),
                )
            };
            let routes: [(&str, &dyn Fn(HashTrieBuildMode)); 3] = [
                ("HashTrieFamily::build_index", &|mode| {
                    structure(mode).build_index(header(), kermit_ds::TupleView::all(&tuples()));
                }),
                ("HashHtj::build_index", &|mode| {
                    join(mode).build_index(header(), kermit_ds::TupleView::all(&tuples()));
                }),
                ("HashHtj::build_indexes", &|mode| {
                    join(mode).build_indexes(&catalog, &prepared);
                }),
            ];
            let hashes = |build: &dyn Fn()| {
                HASHES.set(0);
                build();
                HASHES.get()
            };
            let radix = HashTrieBuildMode::Radix(RadixBits::new(2).unwrap());
            for (route, build) in routes {
                let bulk = hashes(&|| build(HashTrieBuildMode::Bulk));
                let radixed = hashes(&|| build(radix));
                assert!(
                    radixed > bulk,
                    "{} {route}: the radix build hashed {radixed} times and the bulk build \
                     {bulk}: the mode did not reach the build",
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
    /// the build, on every route a relation is built, reordered indexes
    /// included, eager or lazy, and `presized:N` reached the presized fill
    /// (#94).
    #[test]
    fn hash_trie_families_build_with_their_parallel_mode() {
        fn check<E: ExpansionPolicy>() {
            let header = || RelationHeader::new_positional("r", 2);
            let tuples = || Tuples::from(vec![vec![1, 2], vec![2, 1], vec![3, 4]]);
            let (catalog, prepared) = prepared_over(header(), tuples(), "Q(X, Y) :- r(Y, X).");
            // The record of every parallel build `build` runs.
            let parallel_builds = |build: &dyn Fn()| {
                kermit_ds::test_hooks::take_hash_trie_parallel_builds();
                build();
                kermit_ds::test_hooks::take_hash_trie_parallel_builds()
            };
            let presized_root = HashTrieConfig {
                root_capacity: RootCapacity::Tuples,
                ..HashTrieConfig::default()
            };
            let radix = HashTrieBuildMode::Radix(RadixBits::new(2).unwrap());
            // Three threads, not two: a refactor that dropped N on the way to
            // the build would show here as well as in kermit-ds.
            let parallel_three = HashTrieBuildMode::Parallel(kermit_ds::Threads::new(3).unwrap());
            let presized_three = HashTrieBuildMode::Presized(kermit_ds::Threads::new(3).unwrap());
            // `presized:N` requires `root-capacity=tuples`, so it is only
            // paired with `presized_root`; `parallel:N` runs the exact merge
            // build under both configs. Only `presized:N` defers tuples.
            for (config, mode, expected, deferred) in [
                (
                    HashTrieConfig::default(),
                    HashTrieBuildMode::Bulk,
                    vec![],
                    false,
                ),
                (HashTrieConfig::default(), radix, vec![], false),
                (HashTrieConfig::default(), parallel_three, vec![3], false),
                (presized_root, HashTrieBuildMode::Bulk, vec![], false),
                (presized_root, radix, vec![], false),
                (presized_root, parallel_three, vec![3], false),
                (presized_root, presized_three, vec![3], true),
            ] {
                let structure = HashTrieFamily::<SipHashStrategy, NoPruning, E>::new(config, mode);
                let join = HashHtj::<SipHashStrategy, NoPruning, E>::new(
                    config,
                    mode,
                    Planner::stored(LexicographicOptimiser),
                );
                let routes: [(&str, &dyn Fn()); 3] = [
                    ("HashTrieFamily::build_index", &|| {
                        structure.build_index(header(), kermit_ds::TupleView::all(&tuples()));
                    }),
                    ("HashHtj::build_index", &|| {
                        join.build_index(header(), kermit_ds::TupleView::all(&tuples()));
                    }),
                    ("HashHtj::build_indexes", &|| {
                        join.build_indexes(&catalog, &prepared);
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
                        builds.iter().all(|b| b.deferred.is_some() == deferred),
                        "{} {route} under {mode:?}, {config:?}: wrong path",
                        E::NAME
                    );
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
        let tuples = Tuples::from(vec![vec![1, 2]]);
        let rel = family.build_index(
            RelationHeader::new_positional("r", 2),
            kermit_ds::TupleView::all(&tuples),
        );
        assert_eq!(
            TrieLftj::<TreeTrie<GallopingSeek>>::optimization_axes(&rel)["ds_layout_seek"],
            "galloping"
        );
    }

    /// An index the query reads in another column order is built through
    /// `build_index` too, so it carries the family's build mode (seen by
    /// the spy) and config (seen on the index).
    #[test]
    fn families_build_reordered_indexes_with_their_mode_and_config() {
        let header = || RelationHeader::new_positional("edge", 2);
        let edges = || Tuples::from(vec![vec![1, 2], vec![1, 3], vec![2, 3]]);
        // `edge` read with its columns swapped: the one index is columns [1,
        // 0].
        let (catalog, prepared) = prepared_over(header(), edges(), "Q(X, Y) :- edge(Y, X).");

        let join = TrieLftj::<Spy>::new(
            ColumnTrieBuildMode::Incremental,
            Planner::stored(LexicographicOptimiser),
        );
        BUILT_WITH.take();
        let indexes = join.build_indexes(&catalog, &prepared);
        assert_eq!(BUILT_WITH.take(), Some(ColumnTrieBuildMode::Incremental));
        let columns: Vec<&[usize]> = indexes
            .iter()
            .map(|(spec, _)| spec.columns.as_slice())
            .collect();
        assert_eq!(columns, vec![&[1, 0][..]]);
        let (_, index) = indexes.iter().next().expect("one index");
        assert_eq!(index.header().name(), "edge");
        assert_eq!(index.header().arity(), 2);

        let config = HashTrieConfig {
            load_factor: LoadFactor::percent(50).unwrap(),
            ..HashTrieConfig::default()
        };
        let hash = HashHtj::<SipHashStrategy, NoPruning, EagerExpansion>::new(
            config,
            HashTrieBuildMode::Bulk,
            Planner::stored(LexicographicOptimiser),
        );
        let indexes = hash.build_indexes(&catalog, &prepared);
        let (_, index) = indexes.iter().next().expect("one index");
        assert_eq!(index.config(), &config);
        assert_eq!(index.header().name(), "edge");
    }

    /// Five rows of arity 3, the first and the last equal: the source of the
    /// view tests.
    fn view_source() -> Tuples {
        let mut source = Tuples::new(3);
        for row in [[1, 2, 5], [1, 3, 5], [2, 2, 5], [2, 2, 6], [1, 2, 5]] {
            source.push(&row);
        }
        source
    }

    /// `build_index` over `view` builds the relation that `build_index`
    /// builds over the identity view of the view's materialisation: the same
    /// tuples, the same count and the same heap.
    fn assert_view_builds_like_batch<F: RelationFamily>(family: &F, view: &TupleView<'_>) {
        let header = || RelationHeader::new_positional("r", view.arity());
        let materialised = view.materialize();
        let from_view = family.build_index(header(), view.clone());
        let from_batch = family.build_index(header(), TupleView::all(&materialised));
        let (mut viewed, mut batched) = (visited::<F>(&from_view), visited::<F>(&from_batch));
        viewed.sort();
        batched.sort();
        assert_eq!(viewed, batched);
        assert_eq!(F::tuple_count(&from_view), F::tuple_count(&from_batch));
        assert_eq!(from_view.heap_size_bytes(), from_batch.heap_size_bytes());
    }

    /// Every family builds, through `build_index`, what its view
    /// materialises: a view that filters by a constant and permutes, one that
    /// filters by an equality, one that matches nothing, and the identity
    /// view.
    #[test]
    fn build_index_builds_what_its_view_materialises_in_every_family() {
        type Eager = HashTrieFamily<SipHashStrategy, NoPruning, EagerExpansion>;
        type PrunedLazy = HashTrieFamily<SipHashStrategy, SingletonPruning, LazyExpansion>;

        let source = view_source();
        let by_constant = kermit_ds::Filter::new(vec![(2, 5)], vec![]);
        let by_equality = kermit_ds::Filter::new(vec![], vec![(0, 1)]);
        let by_nothing = kermit_ds::Filter::new(vec![(2, 9)], vec![]);
        let views = [
            TupleView::new(&source, by_constant, vec![1, 0]),
            TupleView::new(&source, by_equality, vec![1, 2]),
            TupleView::new(&source, by_nothing, vec![0, 1]),
            TupleView::all(&source),
        ];
        let counted: Vec<usize> = views.iter().map(TupleView::count).collect();
        assert_eq!(
            counted,
            vec![4, 2, 0, 5],
            "the fixture's views must keep, filter and drop rows"
        );

        let lftj = TrieLftj::<TreeTrie>::new(
            TreeTrieBuildMode::default(),
            Planner::stored(LexicographicOptimiser),
        );
        let htj = HashHtj::<SipHashStrategy, NoPruning, EagerExpansion>::new(
            HashTrieConfig::default(),
            HashTrieBuildMode::Bulk,
            Planner::stored(LexicographicOptimiser),
        );
        for view in &views {
            assert_view_builds_like_batch(&SortedTrieFamily::<TreeTrie>::default(), view);
            assert_view_builds_like_batch(&SortedTrieFamily::<ColumnTrie>::default(), view);
            assert_view_builds_like_batch(&Eager::default(), view);
            assert_view_builds_like_batch(&PrunedLazy::default(), view);
            assert_view_builds_like_batch(&lftj, view);
            assert_view_builds_like_batch(&htj, view);
        }
    }

    /// The relation holds exactly the rows its view selects, in the view's
    /// column order: the filter and the permutation both reach the build. A
    /// sorted trie holds each row once, a hash trie keeps the duplicate.
    #[test]
    fn build_index_builds_exactly_the_rows_its_view_selects() {
        type Eager = HashTrieFamily<SipHashStrategy, NoPruning, EagerExpansion>;

        // Column 2 equal to 5, columns 1 and 0 kept in that order: the rows
        // (2, 1) twice, (3, 1) and (2, 2).
        let source = view_source();
        let filter = kermit_ds::Filter::new(vec![(2, 5)], vec![]);
        let view = TupleView::new(&source, filter, vec![1, 0]);
        let header = || RelationHeader::new_positional("r", 2);

        let sorted = SortedTrieFamily::<TreeTrie>::default();
        let tree = sorted.build_index(header(), view.clone());
        let got = visited::<SortedTrieFamily<TreeTrie>>(&tree);
        assert_eq!(got, vec![vec![2, 1], vec![2, 2], vec![3, 1]]);

        let hashed = Eager::default();
        let hash = hashed.build_index(header(), view);
        let mut got = visited::<Eager>(&hash);
        got.sort();
        assert_eq!(got, vec![vec![2, 1], vec![2, 1], vec![2, 2], vec![3, 1]]);
    }

    /// The arity panic keeps its text on the new path: a view of another
    /// arity than the header is a broken invariant wherever it is built.
    #[test]
    #[should_panic(expected = "from_tuples: tuple arity 2 does not match header arity 3")]
    fn build_index_rejects_a_view_of_another_arity_in_a_sorted_family() {
        let source = Tuples::from(vec![vec![1, 2]]);
        let header = RelationHeader::new_positional("r", 3);
        let family = SortedTrieFamily::<TreeTrie>::default();
        family.build_index(header, TupleView::all(&source));
    }

    /// As above, for the hash family.
    #[test]
    #[should_panic(expected = "from_tuples: tuple arity 2 does not match header arity 3")]
    fn build_index_rejects_a_view_of_another_arity_in_a_hash_family() {
        type Eager = HashTrieFamily<SipHashStrategy, NoPruning, EagerExpansion>;

        let source = Tuples::from(vec![vec![1, 2]]);
        let header = RelationHeader::new_positional("r", 3);
        let family = Eager::default();
        family.build_index(header, TupleView::all(&source));
    }

    /// An empty source builds an empty relation under any header, as
    /// `from_tuples(header, vec![])` does.
    #[test]
    fn build_index_of_an_empty_source_accepts_any_header() {
        type Eager = HashTrieFamily<SipHashStrategy, NoPruning, EagerExpansion>;

        let empty = Tuples::new(2);
        let header = || RelationHeader::new_positional("r", 3);

        let sorted = SortedTrieFamily::<TreeTrie>::default();
        let tree = sorted.build_index(header(), TupleView::all(&empty));
        assert_eq!(SortedTrieFamily::<TreeTrie>::tuple_count(&tree), 0);

        let hashed = Eager::default();
        let hash = hashed.build_index(header(), TupleView::all(&empty));
        assert_eq!(Eager::tuple_count(&hash), 0);
    }

    /// `build_indexes` builds every index through the family's one
    /// construction site, so each carries the family's build mode (seen by a
    /// spy and by kermit-ds's record of parallel builds) and config (seen on
    /// the index). `r(Z, Y)` is read reversed, so the query needs two
    /// indexes, `#0` in the stored column order and `#1` reversed.
    #[test]
    fn build_indexes_builds_each_index_by_the_familys_mode_and_config() {
        let tuples = || Tuples::from(vec![vec![1, 2], vec![2, 1], vec![3, 4]]);
        let (catalog, prepared) = prepared_over(
            RelationHeader::new_positional("r", 2),
            tuples(),
            "Q(X, Y, Z) :- r(X, Y), r(Z, Y).",
        );
        let specs = prepared.indexes();
        assert_eq!(specs.len(), 2);
        assert_eq!(specs[0].columns, vec![0, 1]);
        assert_eq!(specs[1].columns, vec![1, 0]);

        // ColumnTrie `incremental`, seen by the spy.
        let spy = TrieLftj::<Spy>::new(
            ColumnTrieBuildMode::Incremental,
            Planner::stored(LexicographicOptimiser),
        );
        BUILT_WITH.take();
        let built = spy.build_indexes(&catalog, &prepared);
        assert_eq!(BUILT_WITH.take(), Some(ColumnTrieBuildMode::Incremental));
        assert_eq!(built.iter().count(), 2);

        // TreeTrie `parallel:2`: one parallel build per index.
        let two = TreeTrieBuildMode::Parallel(kermit_ds::Threads::new(2).unwrap());
        let tree = TrieLftj::<TreeTrie>::new(two, Planner::stored(LexicographicOptimiser));
        kermit_ds::test_hooks::take_tree_trie_parallel_builds();
        let built = tree.build_indexes(&catalog, &prepared);
        let builds = kermit_ds::test_hooks::take_tree_trie_parallel_builds();
        let threads: Vec<usize> = builds.into_iter().map(|(threads, _)| threads).collect();
        assert_eq!(threads, vec![2, 2]);
        assert_eq!(built.iter().count(), 2);

        // HashTrie `presized:3` under a configured load factor and a presized
        // root: the build mode reaches the presized fill and the config
        // reaches both indexes.
        let config = HashTrieConfig {
            root_capacity: RootCapacity::Tuples,
            load_factor: LoadFactor::percent(50).unwrap(),
            ..HashTrieConfig::default()
        };
        let presized = HashTrieBuildMode::Presized(kermit_ds::Threads::new(3).unwrap());
        let hash = HashHtj::<SipHashStrategy, NoPruning, EagerExpansion>::new(
            config,
            presized,
            Planner::stored(LexicographicOptimiser),
        );
        kermit_ds::test_hooks::take_hash_trie_parallel_builds();
        let built = hash.build_indexes(&catalog, &prepared);
        let builds = kermit_ds::test_hooks::take_hash_trie_parallel_builds();
        let threads: Vec<usize> = builds.iter().map(|b| b.threads).collect();
        assert_eq!(threads, vec![3, 3]);
        assert!(
            builds.iter().all(|b| b.deferred.is_some()),
            "the presized fill ran"
        );
        for id in ["#0", "#1"] {
            let index = built.get(id).unwrap();
            assert_eq!(index.config(), &config, "{id}");
            assert_eq!(index.header().name(), "r", "{id}");
        }
    }

    /// Runs `text` over `edges` through `family`'s per-query pipeline: the
    /// rows `execute` streams number the hand-counted `expected`, and
    /// `count_rows` counts the same.
    fn assert_pipeline_answers<F: ExecutionFamily>(
        family: &F, edges: &Tuples, text: &str, expected: u64,
    ) {
        let (catalog, prepared) = prepared_over(
            RelationHeader::new_positional("edge", 2),
            edges.clone(),
            text,
        );
        let indexes = family.build_indexes(&catalog, &prepared);
        let mut rows = Vec::new();
        family.execute(&prepared, &indexes, |row| rows.push(row.to_vec()));
        assert_eq!(rows.len() as u64, expected, "{text}");
        assert_eq!(family.count_rows(&prepared, &indexes), expected, "{text}");
    }

    /// `execute` and `count_rows` answer the hand-counted rows in every
    /// family: a self-join, a constant, a repeated variable, a placeholder
    /// and a query whose index reads `edge` reversed.
    #[test]
    fn execute_and_count_rows_answer_in_every_family() {
        // The edges 1 to 2, 2 to 3, 1 to 3, 3 to 4, 2 to 4, and the loop on 4.
        let mut edges = Tuples::new(2);
        for (src, dst) in [(1, 2), (2, 3), (1, 3), (3, 4), (2, 4), (4, 4)] {
            edges.push(&[src, dst]);
        }
        let queries = [
            // Triangles: (1,2,3), (2,3,4), (2,4,4), (3,4,4) and (4,4,4).
            ("Q(X, Y, Z) :- edge(X, Y), edge(Y, Z), edge(X, Z).", 5),
            // A constant: the successors of 1.
            ("Q(Y) :- edge(c1, Y).", 2),
            // A repeated variable: the loop.
            ("Q(X) :- edge(X, X).", 1),
            // A placeholder: one row per edge.
            ("Q(X) :- edge(X, _).", 6),
            // The index reads `edge` reversed.
            ("Q(Y, X) :- edge(X, Y).", 6),
        ];

        let tree = TrieLftj::<TreeTrie>::new(
            TreeTrieBuildMode::default(),
            Planner::stored(LexicographicOptimiser),
        );
        let column = TrieLftj::<ColumnTrie>::new(
            ColumnTrieBuildMode::default(),
            Planner::stored(LexicographicOptimiser),
        );
        let hash = HashHtj::<SipHashStrategy, NoPruning, EagerExpansion>::new(
            HashTrieConfig::default(),
            HashTrieBuildMode::Bulk,
            Planner::stored(LexicographicOptimiser),
        );
        let lazy = HashHtj::<SipHashStrategy, SingletonPruning, LazyExpansion>::new(
            HashTrieConfig::default(),
            HashTrieBuildMode::Bulk,
            Planner::stored(LexicographicOptimiser),
        );
        for (text, expected) in queries {
            assert_pipeline_answers(&tree, &edges, text, expected);
            assert_pipeline_answers(&column, &edges, text, expected);
            assert_pipeline_answers(&hash, &edges, text, expected);
            assert_pipeline_answers(&lazy, &edges, text, expected);
        }
    }

    /// `execute` keeps `kermit::db::execute`'s debug assertion: indexes built
    /// for another prepared query are a programming error, not an answer.
    /// The two queries read `r` in different column orders, so their index
    /// specs differ.
    #[cfg(debug_assertions)]
    #[test]
    #[should_panic]
    fn execute_refuses_indexes_built_for_another_prepared_query() {
        let tuples = || Tuples::from(vec![vec![1, 2], vec![2, 1], vec![3, 4]]);
        let header = || RelationHeader::new_positional("r", 2);
        let (catalog, forward) = prepared_over(header(), tuples(), "Q(A, B) :- r(A, B).");
        let (_, reversed) = prepared_over(header(), tuples(), "Q(B, A) :- r(A, B).");
        let family = TrieLftj::<TreeTrie>::new(
            TreeTrieBuildMode::default(),
            Planner::stored(LexicographicOptimiser),
        );
        let indexes = family.build_indexes(&catalog, &forward);
        family.execute(&reversed, &indexes, |_| {});
    }
}
