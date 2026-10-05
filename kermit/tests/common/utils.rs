//! Shared driver for the macro-generated join suites.
//!
//! Every pattern runs through the *real* entry points (`lftj_join` /
//! `hash_join` in `kermit::db`), so the const-view and selection rewrites
//! sit on the tested path exactly as they do for `kermit join`. The
//! [`JoinEntry`] trait maps an algorithm type to the entry point that
//! hosts it, mirroring how the CLI's `execution` module pairs them.

use {
    kermit::db::{
        hash_join, hash_join_for_each, lftj_join, lftj_join_for_each, Database, HashFamily,
        JoinError, SortedFamily,
    },
    kermit_algos::{HashTriejoin, JoinQuery, LeapfrogTriejoin, QueryOptimiser, StatisticsLevel},
    kermit_ds::{
        Cardinality, ConfigProvider, Configured, HashTrie, HashTrieConfig, PruningPolicy, Relation,
    },
    kermit_iters::{HashStrategy, TrieIterable},
    std::collections::BTreeMap,
};

/// Marks a placeholder (`_`) in a fixture's `rel_variables` entry: the
/// column exists in the relation's tuples but binds no variable.
pub const PLACEHOLDER: usize = usize::MAX;

/// The entry point in `kermit::db` that runs algorithm `Self` over `R`.
pub trait JoinEntry<R> {
    /// The database the entry point reads, with statistics up to `level`.
    fn database(relations: BTreeMap<String, R>, level: StatisticsLevel) -> Database<R>;

    fn join(
        database: &Database<R>, query: JoinQuery, optimiser: &dyn QueryOptimiser,
    ) -> Result<Vec<Vec<usize>>, JoinError>;

    /// Counts the result through the streaming `_for_each` entry point —
    /// the path `bench run`'s `iteration` metric times.
    fn count(
        database: &Database<R>, query: JoinQuery, optimiser: &dyn QueryOptimiser,
    ) -> Result<usize, JoinError>;
}

impl<R: TrieIterable + Relation + Cardinality> JoinEntry<R> for LeapfrogTriejoin {
    fn database(relations: BTreeMap<String, R>, level: StatisticsLevel) -> Database<R> {
        Database::new::<SortedFamily>(relations, level)
    }

    fn join(
        database: &Database<R>, query: JoinQuery, optimiser: &dyn QueryOptimiser,
    ) -> Result<Vec<Vec<usize>>, JoinError> {
        lftj_join::<R, LeapfrogTriejoin>(database, query, optimiser)
    }

    fn count(
        database: &Database<R>, query: JoinQuery, optimiser: &dyn QueryOptimiser,
    ) -> Result<usize, JoinError> {
        let mut rows = 0;
        lftj_join_for_each::<R, LeapfrogTriejoin>(database, query, optimiser, |_| rows += 1)?;
        Ok(rows)
    }
}

/// `hash_join` needs the relation's hash strategy `H` for constant
/// singletons, so the hash-family impls are per concrete relation type
/// rather than blanket over `HashTrieIterable`.
impl<H: HashStrategy, P: PruningPolicy> JoinEntry<HashTrie<H, P>> for HashTriejoin {
    fn database(
        relations: BTreeMap<String, HashTrie<H, P>>, level: StatisticsLevel,
    ) -> Database<HashTrie<H, P>> {
        Database::new::<HashFamily<H>>(relations, level)
    }

    fn join(
        database: &Database<HashTrie<H, P>>, query: JoinQuery, optimiser: &dyn QueryOptimiser,
    ) -> Result<Vec<Vec<usize>>, JoinError> {
        hash_join::<HashTrie<H, P>, H>(database, query, optimiser)
    }

    fn count(
        database: &Database<HashTrie<H, P>>, query: JoinQuery, optimiser: &dyn QueryOptimiser,
    ) -> Result<usize, JoinError> {
        let mut rows = 0;
        hash_join_for_each::<HashTrie<H, P>, H>(database, query, optimiser, |_| rows += 1)?;
        Ok(rows)
    }
}

impl<H: HashStrategy, P: PruningPolicy, C: ConfigProvider<HashTrieConfig>>
    JoinEntry<Configured<HashTrie<H, P>, C>> for HashTriejoin
{
    fn database(
        relations: BTreeMap<String, Configured<HashTrie<H, P>, C>>, level: StatisticsLevel,
    ) -> Database<Configured<HashTrie<H, P>, C>> {
        Database::new::<HashFamily<H>>(relations, level)
    }

    fn join(
        database: &Database<Configured<HashTrie<H, P>, C>>, query: JoinQuery,
        optimiser: &dyn QueryOptimiser,
    ) -> Result<Vec<Vec<usize>>, JoinError> {
        hash_join::<Configured<HashTrie<H, P>, C>, H>(database, query, optimiser)
    }

    fn count(
        database: &Database<Configured<HashTrie<H, P>, C>>, query: JoinQuery,
        optimiser: &dyn QueryOptimiser,
    ) -> Result<usize, JoinError> {
        let mut rows = 0;
        hash_join_for_each::<Configured<HashTrie<H, P>, C>, H>(database, query, optimiser, |_| {
            rows += 1
        })?;
        Ok(rows)
    }
}

/// Builds one `R` per input relation (named `R0`, `R1`, …), synthesises
/// `Q(V…) :- R0(V…), R1(V…), ….` from `variables` / `rel_variables` (a
/// [`PLACEHOLDER`] entry becomes `_`), runs
/// it through `JA`'s entry point, and asserts multiset equality with
/// `result`.
///
/// It also counts the result through the streaming entry point and checks
/// that count against `result`, so the path `bench run` times is covered
/// for every structure × algorithm × optimiser invocation. The relations
/// are put in a `Database` built at the optimiser's
/// `required_statistics()`, as the CLI's engines are.
///
/// The rows are compared as returned: the entry points project to the
/// head themselves (issue #71), so a body-only variable — including the
/// fresh ones the rewrites introduce — leaking into a row fails the
/// comparison.
pub fn test_join<R, JA, O>(
    input: Vec<Vec<Vec<usize>>>, variables: Vec<usize>, rel_variables: Vec<Vec<usize>>,
    result: Vec<Vec<usize>>,
) where
    R: Relation + Cardinality,
    JA: JoinEntry<R>,
    O: QueryOptimiser + Default,
{
    // Each relation's arity is its atom's term count: the query is what
    // fixes a column count, and an empty relation has no first tuple to
    // read one from (the join rejects an atom whose arity disagrees).
    let relations: BTreeMap<String, R> = input
        .into_iter()
        .zip(&rel_variables)
        .enumerate()
        .map(|(i, (tuples, rv))| {
            assert!(
                tuples.iter().all(|t| t.len() == rv.len()),
                "fixture R{i}: every tuple must have one value per atom term"
            );
            (format!("R{i}"), R::from_tuples(rv.len().into(), tuples))
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
    let optimiser = O::default();
    let database = JA::database(relations, optimiser.required_statistics());

    // The streamed count is what `bench run --verify` checks and what the
    // `iteration` metric times; it must agree with the expected rows.
    let streamed = JA::count(&database, query.clone(), &optimiser)
        .unwrap_or_else(|e| panic!("{query_str}: {e}"));
    assert_eq!(
        streamed,
        result.len(),
        "streamed count disagrees with the expected row count"
    );

    // Multiset equality (relational algebra semantics) — sort both sides
    // before asserting so algorithms with non-sorted output (hash-trie
    // family) and plans with different enumeration orders pass the same
    // suite.
    let mut actual: Vec<Vec<usize>> =
        JA::join(&database, query, &optimiser).unwrap_or_else(|e| panic!("{query_str}: {e}"));
    actual.sort();
    let mut expected = result;
    expected.sort();
    assert_eq!(actual, expected);
}
