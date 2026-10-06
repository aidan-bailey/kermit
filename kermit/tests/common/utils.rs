//! Shared driver for the macro-generated join suites.
//!
//! Every pattern runs through the *real* entry points (`lftj_join` /
//! `hash_join` in `kermit::db`), so the const-view and selection rewrites
//! sit on the tested path exactly as they do for `kermit join`. The
//! [`JoinEntry`] trait maps an algorithm type to the entry point that
//! hosts it, mirroring how the CLI's `execution` module pairs them.

use {
    kermit::db::{
        build_index, hash_join, hash_join_for_each, lftj_join, lftj_join_for_each, Database,
        HashFamily, JoinError, SortedFamily,
    },
    kermit_algos::{
        ColumnOrderPolicy, HashTriejoin, IndexSpec, JoinQuery, LeapfrogTriejoin, Planner,
        QueryOptimiser, StatisticsLevel,
    },
    kermit_ds::{
        BuildModeProvider, BuiltWith, Cardinality, ConfigProvider, Configured, ExpansionPolicy,
        HashTrie, HashTrieBuildMode, HashTrieConfig, PruningPolicy, Relation, RelationHeader,
    },
    kermit_iters::{HashStrategy, TrieIterable},
    std::{collections::BTreeMap, path::Path},
};

/// Marks a placeholder (`_`) in a fixture's `rel_variables` entry: the
/// column exists in the relation's tuples but binds no variable.
pub const PLACEHOLDER: usize = usize::MAX;

/// The entry point in `kermit::db` that runs algorithm `Self` over `R`.
pub trait JoinEntry<R> {
    /// The database the entry point reads, with statistics up to `level`.
    fn database(relations: BTreeMap<String, R>, level: StatisticsLevel) -> Database<R>;

    fn join(
        database: &Database<R>, query: JoinQuery, planner: &Planner,
    ) -> Result<Vec<Vec<usize>>, JoinError>;

    /// Counts the result through the streaming `_for_each` entry point —
    /// the path `bench run`'s `iteration` metric times.
    fn count(
        database: &Database<R>, query: JoinQuery, planner: &Planner,
    ) -> Result<usize, JoinError>;
}

impl<R: TrieIterable + Relation + Cardinality> JoinEntry<R> for LeapfrogTriejoin {
    fn database(relations: BTreeMap<String, R>, level: StatisticsLevel) -> Database<R> {
        Database::new::<SortedFamily>(relations, level)
    }

    fn join(
        database: &Database<R>, query: JoinQuery, planner: &Planner,
    ) -> Result<Vec<Vec<usize>>, JoinError> {
        lftj_join::<R, LeapfrogTriejoin>(database, query, planner)
    }

    fn count(
        database: &Database<R>, query: JoinQuery, planner: &Planner,
    ) -> Result<usize, JoinError> {
        let mut rows = 0;
        lftj_join_for_each::<R, LeapfrogTriejoin>(database, query, planner, |_| rows += 1)?;
        Ok(rows)
    }
}

/// `hash_join` needs the relation's hash strategy `H` for constant
/// singletons, so the hash-family impls are per concrete relation type
/// rather than blanket over `HashTrieIterable`.
impl<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy> JoinEntry<HashTrie<H, P, E>>
    for HashTriejoin
{
    fn database(
        relations: BTreeMap<String, HashTrie<H, P, E>>, level: StatisticsLevel,
    ) -> Database<HashTrie<H, P, E>> {
        Database::new::<HashFamily<H>>(relations, level)
    }

    fn join(
        database: &Database<HashTrie<H, P, E>>, query: JoinQuery, planner: &Planner,
    ) -> Result<Vec<Vec<usize>>, JoinError> {
        hash_join::<HashTrie<H, P, E>, H>(database, query, planner)
    }

    fn count(
        database: &Database<HashTrie<H, P, E>>, query: JoinQuery, planner: &Planner,
    ) -> Result<usize, JoinError> {
        let mut rows = 0;
        hash_join_for_each::<HashTrie<H, P, E>, H>(database, query, planner, |_| rows += 1)?;
        Ok(rows)
    }
}

impl<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy, C: ConfigProvider<HashTrieConfig>>
    JoinEntry<Configured<HashTrie<H, P, E>, C>> for HashTriejoin
{
    fn database(
        relations: BTreeMap<String, Configured<HashTrie<H, P, E>, C>>, level: StatisticsLevel,
    ) -> Database<Configured<HashTrie<H, P, E>, C>> {
        Database::new::<HashFamily<H>>(relations, level)
    }

    fn join(
        database: &Database<Configured<HashTrie<H, P, E>, C>>, query: JoinQuery, planner: &Planner,
    ) -> Result<Vec<Vec<usize>>, JoinError> {
        hash_join::<Configured<HashTrie<H, P, E>, C>, H>(database, query, planner)
    }

    fn count(
        database: &Database<Configured<HashTrie<H, P, E>, C>>, query: JoinQuery, planner: &Planner,
    ) -> Result<usize, JoinError> {
        let mut rows = 0;
        hash_join_for_each::<Configured<HashTrie<H, P, E>, C>, H>(
            database,
            query,
            planner,
            |_| rows += 1,
        )?;
        Ok(rows)
    }
}

impl<
        H: HashStrategy,
        P: PruningPolicy,
        E: ExpansionPolicy,
        B: BuildModeProvider<HashTrieBuildMode>,
    > JoinEntry<BuiltWith<HashTrie<H, P, E>, B>> for HashTriejoin
{
    fn database(
        relations: BTreeMap<String, BuiltWith<HashTrie<H, P, E>, B>>, level: StatisticsLevel,
    ) -> Database<BuiltWith<HashTrie<H, P, E>, B>> {
        Database::new::<HashFamily<H>>(relations, level)
    }

    fn join(
        database: &Database<BuiltWith<HashTrie<H, P, E>, B>>, query: JoinQuery, planner: &Planner,
    ) -> Result<Vec<Vec<usize>>, JoinError> {
        hash_join::<BuiltWith<HashTrie<H, P, E>, B>, H>(database, query, planner)
    }

    fn count(
        database: &Database<BuiltWith<HashTrie<H, P, E>, B>>, query: JoinQuery, planner: &Planner,
    ) -> Result<usize, JoinError> {
        let mut rows = 0;
        hash_join_for_each::<BuiltWith<HashTrie<H, P, E>, B>, H>(database, query, planner, |_| {
            rows += 1
        })?;
        Ok(rows)
    }
}

impl<
        H: HashStrategy,
        P: PruningPolicy,
        E: ExpansionPolicy,
        C: ConfigProvider<HashTrieConfig>,
        B: BuildModeProvider<HashTrieBuildMode>,
    > JoinEntry<BuiltWith<Configured<HashTrie<H, P, E>, C>, B>> for HashTriejoin
{
    fn database(
        relations: BTreeMap<String, BuiltWith<Configured<HashTrie<H, P, E>, C>, B>>,
        level: StatisticsLevel,
    ) -> Database<BuiltWith<Configured<HashTrie<H, P, E>, C>, B>> {
        Database::new::<HashFamily<H>>(relations, level)
    }

    fn join(
        database: &Database<BuiltWith<Configured<HashTrie<H, P, E>, C>, B>>, query: JoinQuery,
        planner: &Planner,
    ) -> Result<Vec<Vec<usize>>, JoinError> {
        hash_join::<BuiltWith<Configured<HashTrie<H, P, E>, C>, B>, H>(database, query, planner)
    }

    fn count(
        database: &Database<BuiltWith<Configured<HashTrie<H, P, E>, C>, B>>, query: JoinQuery,
        planner: &Planner,
    ) -> Result<usize, JoinError> {
        let mut rows = 0;
        hash_join_for_each::<BuiltWith<Configured<HashTrie<H, P, E>, C>, B>, H>(
            database,
            query,
            planner,
            |_| rows += 1,
        )?;
        Ok(rows)
    }
}

/// The column-order policy a macro-generated suite runs under, lifted to
/// a type so `define_multiway_join_test_suite!` can name it, as
/// `ConfigProvider` and `BuildModeProvider` lift a Config and a BuildMode.
pub trait ColumnOrderProvider {
    fn policy() -> ColumnOrderPolicy;
}

/// `--column-orders stored`: each relation read in its stored column
/// order, the default.
pub struct StoredOrders;

/// `--column-orders any`: the planner is free and the join reads
/// reordered copies.
pub struct AnyOrders;

impl ColumnOrderProvider for StoredOrders {
    fn policy() -> ColumnOrderPolicy { ColumnOrderPolicy::Stored }
}

impl ColumnOrderProvider for AnyOrders {
    fn policy() -> ColumnOrderPolicy { ColumnOrderPolicy::Any }
}

/// A fixture's relations by name: the header the relation was built with
/// and its tuples in fixture (file) order, which copies are built from.
pub type Inputs = BTreeMap<String, (RelationHeader, Vec<Vec<usize>>)>;

/// Reads each named Parquet relation in `dir` (`<name>.parquet`) once,
/// returning the relations as `R`, keyed by name, and the [`Inputs`] the
/// copies under `--column-orders any` are built from: each header and its
/// tuples in file order.
pub fn load_parquet_relations<'n, R: Relation>(
    dir: &Path, names: impl IntoIterator<Item = &'n str>,
) -> (BTreeMap<String, R>, Inputs) {
    let mut relations: BTreeMap<String, R> = BTreeMap::new();
    let mut inputs: Inputs = BTreeMap::new();
    for name in names {
        let path = dir.join(format!("{name}.parquet"));
        let (header, tuples) = kermit_ds::read_parquet(&path)
            .unwrap_or_else(|e| panic!("failed to load relation {path:?}: {e}"));
        relations.insert(
            name.to_string(),
            R::from_tuples(header.clone(), tuples.clone()),
        );
        inputs.insert(name.to_string(), (header, tuples));
    }
    (relations, inputs)
}

/// Runs `query` over `database` under `planner` through `JA`'s entry
/// points the way the CLI does: builds the copies `required_indexes` names
/// from `inputs`, joins (collected, and counted through the streaming
/// entry point, the path `bench run` times), and drops the copies. Returns
/// the rows and the specs it built (empty under `stored`).
pub fn join_under_planner<R: Relation + Cardinality, JA: JoinEntry<R>>(
    database: &mut Database<R>, inputs: &Inputs, query: JoinQuery, planner: &Planner,
) -> Result<(Vec<Vec<usize>>, Vec<IndexSpec>), JoinError> {
    let specs = database.required_indexes(&query, planner)?;
    for spec in &specs {
        let (header, tuples) = &inputs[&spec.base];
        database.add_index(spec.clone(), build_index::<R>(spec, header, tuples));
    }
    assert!(
        database.required_indexes(&query, planner)?.is_empty(),
        "held copies must not be required again"
    );
    let rows = JA::join(database, query.clone(), planner);
    let streamed = JA::count(database, query, planner);
    database.clear_indexes();
    let rows = rows?;
    assert_eq!(
        streamed?,
        rows.len(),
        "streamed count disagrees with the collected rows"
    );
    Ok((rows, specs))
}

/// Builds one `R` per input relation (named `R0`, `R1`, …), synthesises
/// `Q(V…) :- R0(V…), R1(V…), ….` from `variables` / `rel_variables` (a
/// [`PLACEHOLDER`] entry becomes `_`), runs it through `JA`'s entry point
/// under `O` and the column-order policy `P` names, and asserts multiset
/// equality with `result`.
///
/// Through [`join_under_planner`], so under `any` the copies the plan needs
/// are built from the fixture's tuples before the join and dropped after,
/// and the result is also counted through the streaming entry point, the
/// path `bench run` times, for every structure × algorithm × optimiser ×
/// policy invocation. The relations are put in a `Database` built at the
/// optimiser's `required_statistics()`, as the CLI's engines are.
///
/// The rows are compared as returned: the entry points project to the
/// head themselves (issue #71), so a body-only variable — including the
/// fresh ones the rewrites introduce — leaking into a row fails the
/// comparison.
pub fn test_join<R, JA, O, P>(
    input: Vec<Vec<Vec<usize>>>, variables: Vec<usize>, rel_variables: Vec<Vec<usize>>,
    result: Vec<Vec<usize>>,
) where
    R: Relation + Cardinality,
    JA: JoinEntry<R>,
    O: QueryOptimiser + Default + 'static,
    P: ColumnOrderProvider,
{
    // Each relation's arity is its atom's term count: the query is what
    // fixes a column count, and an empty relation has no first tuple to
    // read one from (the join rejects an atom whose arity disagrees). The
    // header is named, so a copy's base header is the relation's own.
    let mut inputs: Inputs = BTreeMap::new();
    let relations: BTreeMap<String, R> = input
        .into_iter()
        .zip(&rel_variables)
        .enumerate()
        .map(|(i, (tuples, rv))| {
            assert!(
                tuples.iter().all(|t| t.len() == rv.len()),
                "fixture R{i}: every tuple must have one value per atom term"
            );
            let header = RelationHeader::new_positional(format!("R{i}"), rv.len());
            inputs.insert(format!("R{i}"), (header.clone(), tuples.clone()));
            (format!("R{i}"), R::from_tuples(header, tuples))
        })
        .collect();
    let head_vars: Vec<String> = variables.iter().map(|v| format!("V{v}")).collect();
    let mut body_preds: Vec<String> = Vec::new();
    for (i, rv) in rel_variables.iter().enumerate() {
        let var_list = rv
            .iter()
            .map(|&v| {
                if v == PLACEHOLDER {
                    "_".to_string()
                } else {
                    format!("V{v}")
                }
            })
            .collect::<Vec<_>>()
            .join(", ");
        body_preds.push(format!("R{i}({var_list})"));
    }
    let query_str = format!("Q({}) :- {}.", head_vars.join(", "), body_preds.join(", "));
    let query: JoinQuery = query_str.parse().expect("Failed to build JoinQuery");

    // The database is analysed to exactly what the optimiser reads, as the
    // CLI's engines are.
    let planner = Planner::new(Box::new(O::default()), P::policy());
    let mut database = JA::database(relations, planner.required_statistics());
    let (mut actual, _) = join_under_planner::<R, JA>(&mut database, &inputs, query, &planner)
        .unwrap_or_else(|e| panic!("{query_str}: {e}"));

    // Multiset equality (relational algebra semantics) — sort both sides
    // before asserting so algorithms with non-sorted output (hash-trie
    // family) and plans with different enumeration orders pass the same
    // suite.
    actual.sort();
    let mut expected = result;
    expected.sort();
    assert_eq!(actual, expected);
}
