//! Nemo, in-process: the [`Engine`] behind the `engine-nemo`
//! feature.
//!
//! [Nemo](https://github.com/knowsys/nemo) answers Datalog with leapfrog triejoins over
//! column-oriented tries, the family `ColumnTrie` belongs to. This module
//! drives its `nemo-physical` crate through the public API only, at the pinned
//! revision [`NEMO_REV`]. How kermit's phases map onto it:
//!
//! - **`load`** (untimed): shared handles on the tuples of each relation the
//!   query reads (`Catalog::shared_tuples`), so Nemo's loader streams from the
//!   catalog's one copy.
//! - **`build`** (timed): a fresh `DatabaseInstance`. For each index spec a
//!   `ViewProvider` streams the spec's view into Nemo's loader as integer
//!   `AnyDataValue`s, and `load_trie_into_memory` sorts, deduplicates and
//!   builds its trie (the one copy). The join is then planned, in kermit's
//!   variable order.
//! - **`materialize`** (timed, opt-in): `execute_plan` stores the result as a
//!   deduplicated trie and `count_rows_in_memory` counts it. Nemo stores its
//!   result, so one build can be materialised once (`MATERIALIZE_MUTATES`).
//! - **`space`**: Nemo's own `table_size_bytes` over the spec tables, read
//!   after loading. It leaves out the dictionary entries that keys above
//!   `i64::MAX` take: Nemo stores those through its dictionary, and
//!   `table_size_bytes` counts the tries only.
//! - **No `stream`**: upstream's streaming join (`evaluate_operation` with
//!   `RowScan`) is private, and the public `execute_plan` stores its result, so
//!   `iteration` and `end_to_end` are not offered.
//!
//! Nemo has set semantics (its loader and its result trie deduplicate), so
//! `KEEPS_DUPLICATES` is false. It answers the full conjunctive query, whose
//! distinct rows are the same under set and bag semantics. It runs on one
//! thread and reports no thread count.

use {
    super::{Engine, EngineError, Unsupported},
    crate::db::{Catalog, Prepared},
    kermit_algos::{IndexSpec, INDEX_ID_PREFIX},
    kermit_ds::{Filter, TupleView, Tuples},
    kermit_parser::Term,
    nemo_physical::{
        datasources::{table_providers::TableProvider, tuple_writer::TupleWriter},
        datavalues::{AnyDataValue, DataValue},
        error::ReadingError,
        management::{
            bytesized::ByteSized,
            database::{
                id::{ExecutionId, PermanentTableId},
                DatabaseInstance,
            },
            execution_plan::{ColumnOrder, ExecutionPlan},
        },
        tabular::operations::{OperationColumnMarker, OperationTable},
    },
    std::{
        cell::{Cell, RefCell},
        collections::{btree_map::Entry, BTreeMap},
        fmt,
        sync::{Arc, Mutex, MutexGuard, PoisonError},
    },
};

/// The revision of `nemo-physical` that `kermit/Cargo.toml` pins. A unit test
/// compares the two, so a move of the pin changes both or fails.
pub const NEMO_REV: &str = "04f0962b91edc1bbe73e6f65f08d41d1ee491980";

/// The workspace version of `nemo-physical` at [`NEMO_REV`]. Moving the pin
/// means reading the new revision's version here.
pub const NEMO_PHYSICAL_VERSION: &str = "0.10.2-dev";

/// The engine's name: the report's `system` axis, and the `engine` of its
/// errors.
const ENGINE: &str = "nemo";

/// An error of this engine.
fn failure(message: impl fmt::Display) -> EngineError {
    EngineError {
        engine: ENGINE,
        message: message.to_string(),
    }
}

/// Held across every call into `nemo-physical` that times itself.
///
/// `execute_plan` (and the reorder a load can need) start and stop one
/// process-wide `TimedCode`, and a debug build of `nemo-physical` asserts that
/// it is never started twice. Two threads inside it at once trip that assertion
/// and poison its mutex, which is what `cargo test`'s parallel tests would do.
/// The runner is one thread, so the lock is never contended there.
static NEMO_PHYSICAL: Mutex<()> = Mutex::new(());

/// Takes [`NEMO_PHYSICAL`]. A panic inside a held guard (a test that expects
/// one) must not disable the engine for the tests after it.
fn exclusive() -> MutexGuard<'static, ()> {
    NEMO_PHYSICAL.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Nemo as an [`Engine`]. It has no settings: it runs on one thread.
pub struct Nemo;

/// What `load` leaves: shared handles on the tuples of each relation the
/// query's indexes read. Nothing is copied.
pub struct NemoLoaded {
    relations: BTreeMap<String, Arc<Tuples>>,
}

/// What one timed `build` leaves: a database whose spec tables are loaded, and
/// the join plan that has not run.
///
/// Running the plan changes the database (Nemo stores its result), so a build
/// can be materialised once, and `Nemo::MATERIALIZE_MUTATES` is true. The
/// database and the plan sit behind `RefCell`s and the outcome in a `Cell`
/// because `Engine::materialize` and `Engine::rows` take `&Self::Built`.
pub struct NemoBuilt {
    database: RefCell<DatabaseInstance>,
    /// The join plan, until it runs.
    plan: RefCell<Option<ExecutionPlan>>,
    /// The plan's output node, whose table `execute_plan` names in its result.
    output: ExecutionId,
    /// What running the plan produced.
    outcome: Cell<Outcome>,
    /// One table per index spec, in index order.
    tables: Vec<PermanentTableId>,
    /// The plan's `variable_ordering`, which `rows` permutes the result's
    /// columns by.
    ordering: Vec<usize>,
}

/// Whether the plan has run, and where its result is.
#[derive(Clone, Copy)]
enum Outcome {
    /// The plan has not run.
    NotRun,
    /// The plan ran and the join is empty: `execute_plan` registers no table
    /// for it.
    Empty,
    /// The plan ran and stored its result here.
    Table(PermanentTableId),
}

impl Engine for Nemo {
    type Built<'l>
        = NemoBuilt
    where
        Self: 'l;
    type Loaded = NemoLoaded;
    type Settings = ();

    const HAS_SPACE: bool = true;
    const KEEPS_DUPLICATES: bool = false;
    const MATERIALIZE_MUTATES: bool = true;
    const NAME: &'static str = ENGINE;
    const STREAMS: bool = false;

    fn label(_settings: &()) -> String { ENGINE.to_string() }

    fn threads(_settings: &()) -> Option<usize> { None }

    fn version(_loaded: &NemoLoaded) -> String {
        format!(
            "nemo-physical {NEMO_PHYSICAL_VERSION} @ {}",
            &NEMO_REV[..12]
        )
    }

    /// Nemo joins every query `prepare` produces: each index is one table, an
    /// atom fetches it with its variables as markers, and a constant or a
    /// repeated variable was already compiled into a spec's filter.
    fn supports(_prepared: &Prepared) -> Result<(), Unsupported> { Ok(()) }

    fn load(
        _settings: &(), catalog: &Catalog, prepared: &Prepared,
    ) -> Result<NemoLoaded, EngineError> {
        let mut relations = BTreeMap::new();
        for spec in prepared.indexes() {
            if let Entry::Vacant(slot) = relations.entry(spec.relation.clone()) {
                let tuples = catalog.shared_tuples(&spec.relation).ok_or_else(|| {
                    failure(format!("the catalog holds no relation {}", spec.relation))
                })?;
                slot.insert(tuples);
            }
        }
        Ok(NemoLoaded {
            relations,
        })
    }

    /// The timed build: a fresh database, one table per spec loaded through a
    /// `ViewProvider`, and the join plan.
    fn build<'l>(
        loaded: &'l NemoLoaded, _catalog: &Catalog, prepared: &Prepared,
    ) -> Result<NemoBuilt, EngineError>
    where
        Self: 'l,
    {
        if prepared.indexes().is_empty() {
            return Err(failure("a query of constant checks has no join to run"));
        }
        let markers = plan_markers(prepared)?;
        let mut database = DatabaseInstance::default();
        let mut tables = Vec::with_capacity(prepared.indexes().len());
        for (index, spec) in prepared.indexes().iter().enumerate() {
            let tuples = loaded
                .relations
                .get(&spec.relation)
                .ok_or_else(|| failure(format!("relation {} was not loaded", spec.relation)))?;
            let table = database.register_table(&format!("idx_{index}"), spec.columns.len());
            let provider = ViewProvider::new(Arc::clone(tuples), spec);
            database.add_source_external(table, ColumnOrder::default(), Box::new(provider));
            tables.push(table);
        }
        // Loading is the one copy of the rows, plus Nemo's sort, deduplication
        // and trie. A load touches nemo's global `TimedCode` only when it
        // reorders columns (nemo-physical `order.rs`), which the default column
        // order never does today, so this lock guards against that.
        {
            let _exclusive = exclusive();
            for &table in &tables {
                futures::executor::block_on(
                    database.load_trie_into_memory(table, ColumnOrder::default()),
                )
                .map_err(failure)?;
            }
        }
        let (plan, output) = join_plan(&markers, &tables);
        Ok(NemoBuilt {
            database: RefCell::new(database),
            plan: RefCell::new(Some(plan)),
            output,
            outcome: Cell::new(Outcome::NotRun),
            tables,
            ordering: prepared.plan().variable_ordering.clone(),
        })
    }

    /// Nemo's streaming join is private upstream, so there is none to offer.
    fn stream(_built: &NemoBuilt) -> Result<u64, EngineError> {
        Err(failure(
            "Nemo has no streamed join (STREAMS is false); use materialize",
        ))
    }

    /// Runs the plan: Nemo stores the join as a deduplicated trie, and the
    /// count is its rows. The batch is `None`: the result lives in Nemo's
    /// database and drops with the build, so there is nothing for the caller to
    /// free after the timer.
    ///
    /// # Panics
    ///
    /// If the plan has already run on this build.
    fn materialize(built: &NemoBuilt) -> Result<(u64, Option<Tuples>), EngineError> {
        assert!(
            matches!(built.outcome.get(), Outcome::NotRun),
            "Nemo stores its result, so `materialize` runs once per build, on a fresh `build`"
        );
        let count = match built.result()? {
            | Some(table) => built.database.borrow().count_rows_in_memory(table) as u64,
            | None => 0,
        };
        Ok((count, None))
    }

    /// The sum of Nemo's `table_size_bytes` over the spec tables. Read it
    /// before the plan runs: the figure is Nemo's own accounting of what
    /// loading left, and running a plan may add orders of a table.
    fn space(built: &NemoBuilt) -> Option<usize> {
        let database = built.database.borrow();
        let bytes: u64 = built
            .tables
            .iter()
            .map(|&table| database.table_size_bytes(table))
            .sum();
        Some(usize::try_from(bytes).unwrap_or(usize::MAX))
    }

    /// The result's rows in canonical order, running the plan first if it has
    /// not run.
    fn rows(built: &NemoBuilt) -> Result<Vec<Vec<usize>>, EngineError> {
        let Some(table) = built.result()? else {
            return Ok(Vec::new());
        };
        let database = built.database.borrow();
        let stored = database
            .table_row_iterator_inmemory(table)
            .map_err(failure)?;
        stored
            .map(|values| -> Result<Vec<usize>, EngineError> {
                let plan_row = values
                    .iter()
                    .map(integer)
                    .collect::<Result<Vec<usize>, EngineError>>()?;
                Ok(to_canonical(&plan_row, &built.ordering))
            })
            .collect()
    }
}

impl NemoBuilt {
    /// Where the plan's result is stored, running the plan if it has not run:
    /// `None` for an empty join.
    ///
    /// After a failed `execute_plan` the plan has been taken, so a retry
    /// panics on the `expect` below: a broken invariant, as a second
    /// `materialize` is.
    fn result(&self) -> Result<Option<PermanentTableId>, EngineError> {
        match self.outcome.get() {
            | Outcome::Table(table) => return Ok(Some(table)),
            | Outcome::Empty => return Ok(None),
            | Outcome::NotRun => {},
        }
        let plan = self
            .plan
            .borrow_mut()
            .take()
            .expect("the plan is present until it has run once");
        let results = {
            let _exclusive = exclusive();
            futures::executor::block_on(self.database.borrow_mut().execute_plan(plan))
                .map_err(failure)?
        };
        let outcome = match results.get(&self.output) {
            | Some(&table) => Outcome::Table(table),
            | None => Outcome::Empty,
        };
        self.outcome.set(outcome);
        Ok(match outcome {
            | Outcome::Table(table) => Some(table),
            | Outcome::Empty | Outcome::NotRun => None,
        })
    }
}

/// The markers of one join, as the numbers Nemo's `OperationColumnMarker`
/// wraps.
///
/// A marker is a variable's position in kermit's plan, so the result's markers
/// are `0, 1, …` in the order the plan binds the variables. Nemo orders a
/// join's variables by the order its markers are pushed, so it runs kermit's
/// plan. An atom lists its variables' markers in increasing order, as Nemo's
/// join requires of every input (it panics otherwise): `plan_indexes`
/// gives each atom its variables in plan order, and `plan_markers` checks it.
#[derive(Debug, PartialEq, Eq)]
struct PlanMarkers {
    /// The result table's markers, one per variable of the executed query.
    result: Vec<usize>,
    /// One entry per atom of the executed query, in body order: the index
    /// (`#k`) it reads, whose table the atom fetches, and its variables'
    /// markers.
    atoms: Vec<(usize, Vec<usize>)>,
}

/// The markers of `prepared`'s join.
///
/// # Errors
///
/// An atom of the executed query that does not name one of its indexes, holds
/// anything but a variable, or lists its variables out of plan order: `prepare`
/// guarantees none of these, so each is a broken invariant reported rather than
/// passed on to Nemo.
fn plan_markers(prepared: &Prepared) -> Result<PlanMarkers, EngineError> {
    let order = prepared.variable_order();
    let mut atoms = Vec::with_capacity(prepared.query().body.len());
    for atom in &prepared.query().body {
        let index = atom
            .name
            .strip_prefix(INDEX_ID_PREFIX)
            .and_then(|digits| digits.parse::<usize>().ok())
            .filter(|&index| index < prepared.indexes().len())
            .ok_or_else(|| {
                failure(format!(
                    "atom {atom} does not name one of the query's indexes"
                ))
            })?;
        let mut markers = Vec::with_capacity(atom.terms.len());
        for term in &atom.terms {
            let Term::Var(name) = term else {
                return Err(failure(format!(
                    "atom {atom} holds {term}, which is not a variable"
                )));
            };
            let position = order
                .iter()
                .position(|bound| bound == name)
                .ok_or_else(|| {
                    failure(format!("variable {name} of atom {atom} is not in the plan"))
                })?;
            markers.push(position);
        }
        let columns = prepared.indexes()[index].columns.len();
        if markers.len() != columns || !markers.windows(2).all(|pair| pair[0] < pair[1]) {
            return Err(failure(format!(
                "atom {atom} does not list its {columns} variables in plan order"
            )));
        }
        atoms.push((index, markers));
    }
    Ok(PlanMarkers {
        result: (0..order.len()).collect(),
        atoms,
    })
}

/// `plan_row`, whose column `p` holds the variable the plan binds `p`-th, as
/// the canonical row: column `v` holds canonical variable `v`, in the order of
/// `canonical_names(prepared.query())` (the executed query's order, head
/// variables first). `ordering` is the plan's `variable_ordering` (canonical
/// indices in descent order), a permutation of `0..plan_row.len()`.
fn to_canonical(plan_row: &[usize], ordering: &[usize]) -> Vec<usize> {
    debug_assert_eq!(plan_row.len(), ordering.len());
    let mut row = vec![0; plan_row.len()];
    for (position, &value) in plan_row.iter().enumerate() {
        row[ordering[position]] = value;
    }
    row
}

/// The join over the spec tables: the result's markers in kermit's plan order,
/// and one fetch of the atom's table per atom (merged specs are one table
/// fetched by several atoms). Returns the plan and its output node.
fn join_plan(markers: &PlanMarkers, tables: &[PermanentTableId]) -> (ExecutionPlan, ExecutionId) {
    let table_of = |numbers: &[usize]| {
        let mut table = OperationTable::default();
        for &number in numbers {
            table.push(OperationColumnMarker(number));
        }
        table
    };
    let mut plan = ExecutionPlan::default();
    let mut join = plan.join_empty(table_of(markers.result.as_slice()));
    for (index, atom) in &markers.atoms {
        let fetch = plan.fetch_table(table_of(atom.as_slice()), tables[*index]);
        join.add_subnode(fetch);
    }
    let output = plan.write_permanent(join, "join", "result");
    (plan, output)
}

/// A stored value as kermit's `usize` id. `ViewProvider` writes every value as
/// an unsigned integer, so anything else in a result is a broken invariant.
fn integer(value: &AnyDataValue) -> Result<usize, EngineError> {
    // At the pinned revision Nemo's dictionary hands keys above `i64::MAX` back
    // as an `Other` value of datatype xsd:unsignedLong, which `to_u64` refuses.
    const UNSIGNED_LONG: &str = "http://www.w3.org/2001/XMLSchema#unsignedLong";
    value
        .to_u64()
        .or_else(|| {
            (value.datatype_iri() == UNSIGNED_LONG)
                .then(|| value.lexical_value().parse::<u64>().ok())
                .flatten()
        })
        .and_then(|id| usize::try_from(id).ok())
        .ok_or_else(|| failure(format!("result value {value:?} is not an id")))
}

/// One index spec's view of a relation, written into Nemo's loader as unsigned
/// integers.
///
/// Nemo's `Box<dyn TableProvider>` is `'static`, so the provider owns an `Arc`
/// handle on the relation's tuples and rebuilds the `TupleView` when Nemo asks
/// for the data. The rows are read once, by the loader, which sorts and
/// deduplicates them into a trie.
struct ViewProvider {
    tuples: Arc<Tuples>,
    filter: Filter,
    columns: Vec<usize>,
}

impl ViewProvider {
    fn new(tuples: Arc<Tuples>, spec: &IndexSpec) -> Self {
        Self {
            tuples,
            filter: spec.filter.clone(),
            columns: spec.columns.clone(),
        }
    }
}

impl fmt::Debug for ViewProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ViewProvider")
            .field("source_rows", &self.tuples.len())
            .field("filter", &self.filter)
            .field("columns", &self.columns)
            .finish()
    }
}

impl fmt::Display for ViewProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "a view of {} rows through {} over columns {:?}",
            self.tuples.len(),
            self.filter,
            self.columns
        )
    }
}

impl ByteSized for ViewProvider {
    /// An upper bound that costs no scan: every source row, in the view's
    /// columns, as `u64`s. Nemo reads it only before it loads the table.
    fn size_bytes(&self) -> u64 {
        (self.tuples.len() * self.columns.len() * std::mem::size_of::<u64>()) as u64
    }
}

#[async_trait::async_trait(?Send)]
impl TableProvider for ViewProvider {
    async fn provide_table_data(
        self: Box<Self>, tuple_writer: &mut TupleWriter,
    ) -> Result<(), ReadingError> {
        let view = TupleView::new(&self.tuples, self.filter.clone(), self.columns.clone());
        view.for_each(|row| {
            for &value in row {
                tuple_writer.add_tuple_value(AnyDataValue::new_integer_from_u64(value as u64));
            }
        });
        Ok(())
    }

    fn output_arity(&self) -> usize { self.columns.len() }

    fn input_arity(&self) -> usize { self.columns.len() }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::db::{prepare, Catalog},
        kermit_algos::{JoinQuery, LexicographicOptimiser, Planner, StatisticsLevel},
        kermit_ds::{RelationHeader, TrieLevels, Tuples},
    };

    /// A catalog of positional relations, each `(name, arity, rows)`.
    fn catalog(relations: Vec<(&str, usize, Vec<Vec<usize>>)>) -> Catalog {
        let relations = relations
            .into_iter()
            .map(|(name, arity, rows)| {
                let mut tuples = Tuples::new(arity);
                for row in &rows {
                    tuples.push(row);
                }
                (RelationHeader::new_positional(name, arity), tuples)
            })
            .collect();
        Catalog::new(relations, StatisticsLevel::TupleCounts)
    }

    /// `text` prepared over `catalog` in the lexicographic plan, under the
    /// levels engines use.
    fn prepared_for(catalog: &Catalog, text: &str) -> Prepared {
        let query: JoinQuery = text.parse().expect("a well-formed query");
        let planner = Planner::new(Box::new(LexicographicOptimiser));
        prepare(catalog, &query, &planner, TrieLevels::All).expect("a valid query")
    }

    /// `NEMO_REV` is a copy of the `rev` in `kermit/Cargo.toml`. The precedent
    /// is the Python `SCHEMA_VERSION` pin: the two move together, and this
    /// fails when only one does.
    #[test]
    fn the_pinned_revision_is_the_manifests() {
        let manifest = include_str!("../../Cargo.toml");
        let dependency = manifest
            .lines()
            .find(|line| line.trim_start().starts_with("nemo-physical"))
            .expect("kermit/Cargo.toml declares the nemo-physical dependency");
        assert!(
            dependency.contains(&format!("rev = \"{NEMO_REV}\"")),
            "NEMO_REV ({NEMO_REV}) is not the rev of `{dependency}`; move both together"
        );
    }

    /// R(X, P), S(X, Y), T(Y): X and Y are join keys and P is payload, so the
    /// lexicographic plan binds X, Y, P while the query's canonical order is X,
    /// P, Y. A marker is a plan position, so R's markers are X's 0 and P's 2,
    /// where canonical numbering would give 0 and 1.
    #[test]
    fn markers_follow_the_plans_variable_order() {
        let c = catalog(vec![
            ("R", 2, vec![vec![1, 7]]),
            ("S", 2, vec![vec![1, 5]]),
            ("T", 1, vec![vec![5]]),
        ]);
        let prepared = prepared_for(&c, "Q(X, P, Y) :- R(X, P), S(X, Y), T(Y).");
        assert_eq!(prepared.variable_order(), vec!["X", "Y", "P"]);
        let markers =
            plan_markers(&prepared).expect("every atom lists its variables in plan order");
        assert_eq!(markers.result, vec![0, 1, 2]);
        assert_eq!(markers.atoms, vec![
            (0, vec![0, 2]),
            (1, vec![0, 1]),
            (2, vec![1])
        ]);
    }

    /// The three atoms of a triangle over one relation read one merged index:
    /// Nemo fetches the one table three times, with different markers.
    #[test]
    fn atoms_over_one_merged_index_share_its_table() {
        let c = catalog(vec![("E", 2, vec![vec![1, 2]])]);
        let prepared = prepared_for(&c, "Q(X, Y, Z) :- E(X, Y), E(Y, Z), E(X, Z).");
        assert_eq!(prepared.indexes().len(), 1);
        let markers =
            plan_markers(&prepared).expect("every atom lists its variables in plan order");
        assert_eq!(markers.result, vec![0, 1, 2]);
        assert_eq!(markers.atoms, vec![
            (0, vec![0, 1]),
            (0, vec![1, 2]),
            (0, vec![0, 2])
        ]);
    }

    /// The result's column `p` holds the variable the plan binds `p`-th, whose
    /// canonical index is `ordering[p]`.
    #[test]
    fn to_canonical_puts_each_plan_column_at_its_variable() {
        // The plan binds canonical variables 0, 2, 1: plan column 1 holds
        // variable 2.
        assert_eq!(to_canonical(&[10, 30, 20], &[0, 2, 1]), vec![10, 20, 30]);
        // The plan binds 2, 0, 1: the plan row [c, a, b] is the canonical row
        // [a, b, c].
        assert_eq!(to_canonical(&[3, 1, 2], &[2, 0, 1]), vec![1, 2, 3]);
        assert_eq!(to_canonical(&[7], &[0]), vec![7]);
        assert_eq!(to_canonical(&[], &[]), Vec::<usize>::new());
    }

    /// Builds `text` over `relations` on Nemo and returns the build, before
    /// anything has run on it.
    fn built_for(relations: Vec<(&str, usize, Vec<Vec<usize>>)>, text: &str) -> NemoBuilt {
        let c = catalog(relations);
        let prepared = prepared_for(&c, text);
        Nemo::supports(&prepared).expect("Nemo runs the query");
        let loaded = Nemo::load(&(), &c, &prepared).expect("the relations load");
        Nemo::build(&loaded, &c, &prepared).expect("the build succeeds")
    }

    /// The `materialize` count and the sorted `rows` of `text` over `relations`
    /// on Nemo.
    fn answer(
        relations: Vec<(&str, usize, Vec<Vec<usize>>)>, text: &str,
    ) -> (u64, Vec<Vec<usize>>) {
        let built = built_for(relations, text);
        let (count, kept) = Nemo::materialize(&built).expect("the plan runs");
        assert!(kept.is_none(), "Nemo's result stays in its database");
        let mut rows = Nemo::rows(&built).expect("the result reads back");
        rows.sort();
        (count, rows)
    }

    /// The edges of the transitive tournament on `1..=n`: `i -> j` for `i < j`.
    fn tournament(n: usize) -> Vec<Vec<usize>> {
        (1..=n)
            .flat_map(|i| (i + 1..=n).map(move |j| vec![i, j]))
            .collect()
    }

    #[test]
    fn its_labels_and_version_are_fixed() {
        assert_eq!(Nemo::NAME, "nemo");
        assert_eq!(Nemo::label(&()), "nemo");
        assert_eq!(Nemo::threads(&()), None);
        let loaded = NemoLoaded {
            relations: BTreeMap::new(),
        };
        assert_eq!(
            Nemo::version(&loaded),
            "nemo-physical 0.10.2-dev @ 04f0962b91ed"
        );
    }

    /// The plan binds X, Y, P and Nemo's result columns follow it; `rows`
    /// returns the canonical X, P, Y. The values differ, so a wrong permutation
    /// cannot pass. R(2, 9) and S(3, 5) find no partner.
    #[test]
    fn a_join_returns_full_assignments_in_canonical_order() {
        let (count, rows) = answer(
            vec![
                ("R", 2, vec![vec![1, 7], vec![2, 9]]),
                ("S", 2, vec![vec![1, 5], vec![3, 5]]),
                ("T", 1, vec![vec![5]]),
            ],
            "Q(X, P, Y) :- R(X, P), S(X, Y), T(Y).",
        );
        assert_eq!((count, rows), (1, vec![vec![1, 7, 5]]));
    }

    /// Three atoms over one relation: one table, fetched three times. The
    /// triangles of the transitive tournament on 1..=4 are its four increasing
    /// triples.
    #[test]
    fn a_self_join_fetches_one_table_for_each_atom() {
        let (count, rows) = answer(
            vec![("E", 2, tournament(4))],
            "Q(X, Y, Z) :- E(X, Y), E(Y, Z), E(X, Z).",
        );
        assert_eq!(count, 4);
        assert_eq!(rows, vec![
            vec![1, 2, 3],
            vec![1, 2, 4],
            vec![1, 3, 4],
            vec![2, 3, 4]
        ]);
    }

    /// `execute_plan` registers no table for an empty result, so `materialize`
    /// and `rows` must read that as no rows.
    #[test]
    fn a_join_with_no_answer_has_no_rows() {
        let (count, rows) = answer(
            vec![
                ("R", 2, vec![vec![1, 7]]),
                ("S", 2, vec![vec![2, 5]]),
                ("T", 1, vec![vec![5]]),
            ],
            "Q(X, P, Y) :- R(X, P), S(X, Y), T(Y).",
        );
        assert_eq!((count, rows), (0, Vec::new()));
    }

    /// The constant admits no row of `r`, so the spec's table is empty.
    #[test]
    fn a_view_that_admits_no_row_has_no_rows() {
        let (count, rows) = answer(
            vec![("r", 2, vec![vec![1, 2], vec![3, 4]])],
            "Q(X) :- r(X, c9).",
        );
        assert_eq!((count, rows), (0, Vec::new()));
    }

    #[test]
    fn an_empty_relation_has_no_rows() {
        let (count, rows) = answer(vec![("r", 1, vec![])], "Q(X) :- r(X).");
        assert_eq!((count, rows), (0, Vec::new()));
    }

    /// The provider writes the rows the spec's filter admits, in the spec's
    /// columns.
    #[test]
    fn the_provider_applies_the_views_constants_and_equalities() {
        let r = || vec![("r", 2, vec![vec![1, 2], vec![3, 4], vec![5, 2], vec![6, 6]])];
        assert_eq!(answer(r(), "Q(X) :- r(X, c2).").1, vec![vec![1], vec![5]]);
        assert_eq!(answer(r(), "Q(X) :- r(X, X).").1, vec![vec![6]]);
    }

    /// Nemo's loader deduplicates: a bag engine would count the repeated row
    /// twice.
    #[test]
    fn duplicate_rows_count_once() {
        let (count, rows) = answer(
            vec![("r", 2, vec![vec![1, 2], vec![1, 2], vec![3, 4]])],
            "Q(X, Y) :- r(X, Y).",
        );
        assert_eq!((count, rows), (2, vec![vec![1, 2], vec![3, 4]]));
    }

    /// Keys above `i64::MAX` take Nemo's `UnsignedLong` domain, stored through
    /// its dictionary where small keys are `Int64`. They must load, filter,
    /// join and come back exactly.
    #[test]
    fn keys_above_i64_max_load_filter_join_and_come_back_exactly() {
        const HIGH: usize = 1 << 63;
        let relations = || {
            vec![
                ("e", 2, vec![
                    vec![usize::MAX, 1],
                    vec![HIGH, 2],
                    vec![5, 3],
                    vec![HIGH, 4],
                ]),
                ("f", 2, vec![vec![usize::MAX, 7], vec![HIGH, 8], vec![6, 9]]),
            ]
        };
        assert_eq!(
            answer(relations(), "Q(X) :- e(X, c4).").1,
            vec![vec![HIGH]],
            "a constant filter on the second column"
        );
        assert_eq!(
            answer(relations(), "Q(Y) :- e(c9223372036854775808, Y).").1,
            vec![vec![2], vec![4]],
            "a constant above i64::MAX"
        );
        assert_eq!(
            answer(relations(), &format!("Q(Y) :- e(c{}, Y).", usize::MAX)).1,
            vec![vec![1]],
            "usize::MAX as a constant"
        );
        assert_eq!(
            answer(relations(), "Q(K, A, B) :- e(K, A), f(K, B).").1,
            vec![vec![HIGH, 2, 8], vec![HIGH, 4, 8], vec![usize::MAX, 1, 7]],
            "a join on the large key"
        );
        assert_eq!(
            answer(relations(), "Q(K, A) :- e(K, A).").1,
            vec![vec![5, 3], vec![HIGH, 2], vec![HIGH, 4], vec![
                usize::MAX,
                1
            ]],
            "the keys come back as written"
        );
    }

    /// `space` is Nemo's figure for the loaded spec tables, and grows with
    /// them.
    #[test]
    fn space_counts_the_loaded_tables() {
        let space_of = |n: usize| {
            let built = built_for(vec![("E", 2, tournament(n))], "Q(X, Y) :- E(X, Y).");
            Nemo::space(&built).expect("Nemo reports space")
        };
        let (small, large) = (space_of(3), space_of(11));
        assert!(small > 0, "the loaded trie has a size");
        assert!(large > small, "3 rows take {small} bytes, 55 rows {large}");
    }

    /// Nemo stores its result, so a second `materialize` on one build would
    /// time nothing: it panics, and the runner builds afresh for every sample.
    #[test]
    #[should_panic(expected = "runs once per build")]
    fn a_second_materialize_on_one_build_panics() {
        let built = built_for(vec![("r", 1, vec![vec![1]])], "Q(X) :- r(X).");
        let (count, kept) = Nemo::materialize(&built).expect("the plan runs");
        assert_eq!(count, 1);
        assert!(kept.is_none());
        let _ = Nemo::materialize(&built);
    }

    /// `rows` runs the plan itself when `materialize` has not.
    #[test]
    fn rows_run_the_plan_when_materialize_has_not() {
        let built = built_for(
            vec![("E", 2, tournament(4))],
            "Q(X, Y, Z) :- E(X, Y), E(Y, Z), E(X, Z).",
        );
        let mut rows = Nemo::rows(&built).expect("the plan runs");
        rows.sort();
        assert_eq!(rows.len(), 4);
        assert_eq!(rows[0], vec![1, 2, 3]);
    }

    #[test]
    fn stream_is_refused() {
        let built = built_for(vec![("r", 1, vec![vec![1]])], "Q(X) :- r(X).");
        let error = Nemo::stream(&built).expect_err("Nemo has no streamed join");
        assert_eq!(error.engine, "nemo");
        assert!(error.message.contains("no streamed join"), "{error}");
    }

    /// A query of constant checks has no index, so no join: the runner never
    /// builds it, and an attempt to is an error rather than a Nemo panic. The
    /// parser cannot write an empty head, so the query is built by clearing
    /// one.
    #[test]
    fn a_query_of_checks_is_refused_at_build() {
        let c = catalog(vec![("s", 2, vec![vec![7, 8]])]);
        let mut query: JoinQuery = "Q(X) :- s(c7, c8).".parse().expect("a well-formed query");
        query.head.terms.clear();
        let planner = Planner::new(Box::new(LexicographicOptimiser));
        let prepared = prepare(&c, &query, &planner, TrieLevels::All).expect("a valid query");
        assert!(prepared.indexes().is_empty());
        let loaded = Nemo::load(&(), &c, &prepared).expect("nothing to load");
        let Err(error) = Nemo::build(&loaded, &c, &prepared) else {
            panic!("a query of checks has no join for Nemo to run");
        };
        assert_eq!(error.engine, "nemo");
        assert!(error.message.contains("constant checks"), "{error}");
    }
}
