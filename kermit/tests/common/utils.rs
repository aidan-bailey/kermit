//! Shared driver for the macro-generated join suites.
//!
//! Every pattern runs through the *real* entry points (`lftj_join` /
//! `hash_join` in `kermit::db`), so the const-view and selection rewrites
//! sit on the tested path exactly as they do for `kermit join`. The
//! [`JoinEntry`] trait maps an algorithm type to the entry point that
//! hosts it, mirroring how the CLI's `execution` module pairs them.

use {
    kermit::db::{hash_join, lftj_join},
    kermit_algos::{HashTriejoin, JoinQuery, LeapfrogTriejoin, QueryOptimiser},
    kermit_ds::{Cardinality, Configured, HashTrie, PruningPolicy, Relation},
    kermit_iters::{HashStrategy, TrieIterable},
    std::collections::BTreeMap,
};

/// The entry point in `kermit::db` that runs algorithm `Self` over `R`.
pub trait JoinEntry<R> {
    fn join(
        relations: &BTreeMap<String, R>, query: JoinQuery, optimiser: &dyn QueryOptimiser,
    ) -> Vec<Vec<usize>>;
}

impl<R: TrieIterable + Cardinality> JoinEntry<R> for LeapfrogTriejoin {
    fn join(
        relations: &BTreeMap<String, R>, query: JoinQuery, optimiser: &dyn QueryOptimiser,
    ) -> Vec<Vec<usize>> {
        lftj_join::<R, LeapfrogTriejoin>(relations, query, optimiser)
    }
}

/// `hash_join` needs the relation's hash strategy `H` for constant
/// singletons, so the hash-family impls are per concrete relation type
/// rather than blanket over `HashTrieIterable`.
impl<H: HashStrategy, P: PruningPolicy> JoinEntry<HashTrie<H, P>> for HashTriejoin {
    fn join(
        relations: &BTreeMap<String, HashTrie<H, P>>, query: JoinQuery,
        optimiser: &dyn QueryOptimiser,
    ) -> Vec<Vec<usize>> {
        hash_join::<HashTrie<H, P>, H>(relations, query, optimiser)
    }
}

impl<H: HashStrategy, P: PruningPolicy, C> JoinEntry<Configured<HashTrie<H, P>, C>>
    for HashTriejoin
{
    fn join(
        relations: &BTreeMap<String, Configured<HashTrie<H, P>, C>>, query: JoinQuery,
        optimiser: &dyn QueryOptimiser,
    ) -> Vec<Vec<usize>> {
        hash_join::<Configured<HashTrie<H, P>, C>, H>(relations, query, optimiser)
    }
}

/// Builds one `R` per input relation (named `R0`, `R1`, …), synthesises
/// `Q(V…) :- R0(V…), R1(V…), ….` from `variables` / `rel_variables`, runs
/// it through `JA`'s entry point, projects each row to the head, and
/// asserts multiset equality with `result`.
///
/// Head variables receive canonical indices `0..variables.len()` in head
/// order (`kermit_algos::analyse`), and the entry points emit every
/// variable in canonical order, so truncating each row to the head's
/// length is the projection. Body-only variables — including the fresh
/// ones the rewrites introduce — are dropped this way (issue #71 tracks
/// doing this in the entry points themselves).
pub fn test_join<R, JA, O>(
    input: Vec<Vec<Vec<usize>>>, variables: Vec<usize>, rel_variables: Vec<Vec<usize>>,
    result: Vec<Vec<usize>>,
) where
    R: Relation + Cardinality,
    JA: JoinEntry<R>,
    O: QueryOptimiser + Default,
{
    let relations: BTreeMap<String, R> = input
        .into_iter()
        .enumerate()
        .map(|(i, tuples)| {
            let k = if tuples.is_empty() {
                0
            } else {
                tuples[0].len()
            };
            (format!("R{i}"), R::from_tuples(k.into(), tuples))
        })
        .collect();
    let head_vars: Vec<String> = variables.iter().map(|v| format!("V{v}")).collect();
    let mut body_preds: Vec<String> = Vec::new();
    for (i, rv) in rel_variables.iter().enumerate() {
        let var_list = if rv.is_empty() {
            "_".to_string()
        } else {
            rv.iter()
                .map(|v| format!("V{v}"))
                .collect::<Vec<_>>()
                .join(", ")
        };
        body_preds.push(format!("R{i}({var_list})"));
    }
    let query_str = format!("Q({}) :- {}.", head_vars.join(", "), body_preds.join(", "));
    let query: JoinQuery = query_str.parse().expect("Failed to build JoinQuery");

    let head_arity = variables.len();
    // Multiset equality (relational algebra semantics) — sort both sides
    // before asserting so algorithms with non-sorted output (hash-trie
    // family) and plans with different enumeration orders pass the same
    // suite.
    let mut actual: Vec<Vec<usize>> = JA::join(&relations, query, &O::default())
        .into_iter()
        .map(|row| row[..head_arity].to_vec())
        .collect();
    actual.sort();
    let mut expected = result;
    expected.sort();
    assert_eq!(actual, expected);
}
