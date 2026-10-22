//! Shared driver for the macro-generated join suites.
//!
//! Every pattern runs through the real lifecycle of `kermit::db`: a
//! [`Catalog`] holds the fixture's relations flat, `prepare` plans the query
//! and names the indexes it reads, `build` builds each from the catalog
//! through a view (constants and repeated variables pushed down into its
//! filter) and `execute` joins over them, exactly as `kermit join` does. The
//! [`JoinEntry`] trait maps an algorithm type to the execution that hosts
//! it, mirroring how the CLI's `execution` module pairs them.

use {
    kermit::db::{build, execute, prepare, Catalog, JoinError},
    kermit_algos::{
        ColumnOrderPolicy, HashTriejoin, IndexSpec, JoinAlgo, JoinQuery, LeapfrogTriejoin, Planner,
        QueryOptimiser, StatisticsLevel,
    },
    kermit_ds::{Relation, RelationHeader, Tuples},
    kermit_iters::{HashTrieIterable, TrieIterable},
    std::{collections::BTreeMap, path::Path},
};

/// Marks a placeholder (`_`) in a fixture's `rel_variables` entry: the
/// column exists in the relation's tuples but binds no variable.
pub const PLACEHOLDER: usize = usize::MAX;

/// The execution in `kermit::db` that runs algorithm `Self` over `R`.
pub trait JoinEntry<R> {
    /// Every row of `query`'s head, collected.
    fn join(
        catalog: &Catalog, query: JoinQuery, planner: &Planner,
    ) -> Result<Vec<Vec<usize>>, JoinError>;

    /// Counts the result through the streamed `execute` — the path
    /// `bench run`'s `iteration` metric times.
    fn count(catalog: &Catalog, query: JoinQuery, planner: &Planner) -> Result<u64, JoinError>;
}

/// Prepares `query` over `catalog`, builds the indexes it reads as `R` and
/// streams each row's head columns to `emit`.
fn run<R: Relation, JA: JoinAlgo<R>>(
    catalog: &Catalog, query: &JoinQuery, planner: &Planner, emit: impl FnMut(&[usize]),
) -> Result<(), JoinError> {
    let prepared = prepare(catalog, query, planner)?;
    let indexes = build::<R>(catalog, &prepared);
    execute::<R, JA>(&prepared, &indexes, emit);
    Ok(())
}

fn collect<R: Relation, JA: JoinAlgo<R>>(
    catalog: &Catalog, query: &JoinQuery, planner: &Planner,
) -> Result<Vec<Vec<usize>>, JoinError> {
    let mut rows = Vec::new();
    run::<R, JA>(catalog, query, planner, |row| rows.push(row.to_vec()))?;
    Ok(rows)
}

fn count_rows<R: Relation, JA: JoinAlgo<R>>(
    catalog: &Catalog, query: &JoinQuery, planner: &Planner,
) -> Result<u64, JoinError> {
    let mut rows = 0u64;
    run::<R, JA>(catalog, query, planner, |_| rows += 1)?;
    Ok(rows)
}

impl<R: Relation + TrieIterable> JoinEntry<R> for LeapfrogTriejoin {
    fn join(
        catalog: &Catalog, query: JoinQuery, planner: &Planner,
    ) -> Result<Vec<Vec<usize>>, JoinError> {
        collect::<R, LeapfrogTriejoin>(catalog, &query, planner)
    }

    fn count(catalog: &Catalog, query: JoinQuery, planner: &Planner) -> Result<u64, JoinError> {
        count_rows::<R, LeapfrogTriejoin>(catalog, &query, planner)
    }
}

/// One impl for every hash-family relation (`HashTrie`, `Configured`,
/// `BuiltWith`): the hash entry point no longer hashes constants, so it
/// needs no hash strategy.
impl<R: Relation + HashTrieIterable> JoinEntry<R> for HashTriejoin {
    fn join(
        catalog: &Catalog, query: JoinQuery, planner: &Planner,
    ) -> Result<Vec<Vec<usize>>, JoinError> {
        collect::<R, HashTriejoin>(catalog, &query, planner)
    }

    fn count(catalog: &Catalog, query: JoinQuery, planner: &Planner) -> Result<u64, JoinError> {
        count_rows::<R, HashTriejoin>(catalog, &query, planner)
    }
}

/// A fixture's relations by name: the header the relation was built with
/// and its tuples, flat, in fixture (file) order.
pub type Inputs = BTreeMap<String, (RelationHeader, Tuples)>;

/// Reads each named Parquet relation in `dir` (`<name>.parquet`) once,
/// flat: the header (named after the file) and the tuples in file order.
pub fn load_parquet_inputs<'n>(dir: &Path, names: impl IntoIterator<Item = &'n str>) -> Inputs {
    names
        .into_iter()
        .map(|name| {
            let path = dir.join(format!("{name}.parquet"));
            let (header, tuples) = kermit_ds::read_parquet(&path)
                .unwrap_or_else(|e| panic!("failed to load relation {path:?}: {e}"));
            assert_eq!(
                header.name(),
                name,
                "{path:?} must name its relation after its file"
            );
            (name.to_string(), (header, tuples))
        })
        .collect()
}

/// The catalog of `inputs`, with statistics up to `level`: what the
/// optimiser reads, as the CLI gathers it. The tuples are copied, so one
/// fixture serves planners that read different levels.
pub fn catalog_of(inputs: &Inputs, level: StatisticsLevel) -> Catalog {
    Catalog::new(inputs.values().cloned().collect(), level)
}

/// Runs `query` over `catalog` under `planner` through `JA`: joined and
/// collected, and counted through the streamed path `bench run` times, which
/// must agree. Returns the rows and the index specs the query built (what
/// `prepare` named; each is built again by the join and by the count).
pub fn join_under_planner<R: Relation, JA: JoinEntry<R>>(
    catalog: &Catalog, query: JoinQuery, planner: &Planner,
) -> Result<(Vec<Vec<usize>>, Vec<IndexSpec>), JoinError> {
    let specs = prepare(catalog, &query, planner)?.indexes().to_vec();
    let rows = JA::join(catalog, query.clone(), planner)?;
    let streamed = JA::count(catalog, query, planner)?;
    assert_eq!(
        streamed,
        rows.len() as u64,
        "streamed count disagrees with the collected rows"
    );
    Ok((rows, specs))
}

/// Builds one flat relation per input (named `R0`, `R1`, …), synthesises
/// `Q(V…) :- R0(V…), R1(V…), ….` from `variables` / `rel_variables` (a
/// [`PLACEHOLDER`] entry becomes `_`), runs it through `JA`'s entry point
/// under `O`, and asserts multiset equality with `result`.
///
/// Through [`join_under_planner`], so each pattern is also counted through
/// the streamed path, the path `bench run` times, for every structure ×
/// algorithm × optimiser invocation. The relations are put in a `Catalog`
/// gathered at the optimiser's `required_statistics()`, as the CLI's is.
///
/// The rows are compared as returned: the entry points project to the
/// head themselves (issue #71), so a body-only variable — including the
/// fresh ones a `_` or a repeated variable introduces — leaking into a row
/// fails the comparison.
pub fn test_join<R, JA, O>(
    input: Vec<Vec<Vec<usize>>>, variables: Vec<usize>, rel_variables: Vec<Vec<usize>>,
    result: Vec<Vec<usize>>,
) where
    R: Relation,
    JA: JoinEntry<R>,
    O: QueryOptimiser + Default + 'static,
{
    // Each relation's arity is its atom's term count: the query is what
    // fixes a column count, and an empty relation has no first tuple to
    // read one from (the join rejects an atom whose arity disagrees), so
    // the flat tuples take that arity too.
    let mut inputs: Inputs = BTreeMap::new();
    for (i, (tuples, rv)) in input.iter().zip(&rel_variables).enumerate() {
        assert!(
            tuples.iter().all(|t| t.len() == rv.len()),
            "fixture R{i}: every tuple must have one value per atom term"
        );
        let header = RelationHeader::new_positional(format!("R{i}"), rv.len());
        let flat: Vec<usize> = tuples.iter().flatten().copied().collect();
        inputs.insert(
            format!("R{i}"),
            (header, Tuples::from_flat(rv.len(), tuples.len(), flat)),
        );
    }
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

    // The catalog is analysed to exactly what the optimiser reads, as the
    // CLI's is.
    let planner = Planner::new(Box::new(O::default()), ColumnOrderPolicy::Any);
    let catalog = catalog_of(&inputs, planner.required_statistics());
    let (mut actual, _) = join_under_planner::<R, JA>(&catalog, query, &planner)
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
