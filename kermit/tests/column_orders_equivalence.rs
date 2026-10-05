//! `any` returns the same multiset as `stored` for every query `stored`
//! accepts, and answers every query `stored` rejects only for its column
//! order (issue #93). Random small conjunctive queries over random small
//! relations, from fixed seeds, so a failure reproduces; no new crate.

mod common;

use {
    common::utils::{join_under_planner, Inputs, JoinEntry},
    kermit::db::JoinError,
    kermit_algos::{
        CardinalityOptimiser, ColumnOrderPolicy, CostBasedOptimiser, HashTriejoin, JoinQuery,
        LeapfrogTriejoin, LexicographicOptimiser, Planner, QueryOptimiser,
    },
    kermit_ds::{
        Cardinality, ColumnTrie, HashTrie, LazyExpansion, NoPruning, Relation, RelationHeader,
        SingletonPruning, TreeTrie,
    },
    kermit_iters::SipHashStrategy,
    std::collections::BTreeMap,
};

/// A 64-bit linear congruential generator (Knuth's MMIX constants).
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }

    /// A value in `0..n`.
    fn below(&mut self, n: usize) -> usize { (self.next() % n as u64) as usize }
}

const VALUES: usize = 4;
const VARIABLES: [&str; 4] = ["A", "B", "C", "D"];

type Relations = Vec<(String, usize, Vec<Vec<usize>>)>;

/// One to three relations of arity 1–3 with 0–6 tuples over `0..VALUES`.
fn random_relations(rng: &mut Lcg) -> Relations {
    (0..1 + rng.below(3))
        .map(|i| {
            let arity = 1 + rng.below(3);
            let tuples = (0..rng.below(7))
                .map(|_| (0..arity).map(|_| rng.below(VALUES)).collect())
                .collect();
            (format!("r{i}"), arity, tuples)
        })
        .collect()
}

/// Two or three atoms over `relations`, each column a variable from the
/// pool or (one in eight) a constant. The first column of the first atom
/// is always a variable, so the head, which is every variable used, is
/// never empty and always bound.
fn random_query(rng: &mut Lcg, relations: &Relations) -> String {
    let mut used: Vec<&str> = Vec::new();
    let atoms: Vec<String> = (0..2 + rng.below(2))
        .map(|atom| {
            let (name, arity, _) = &relations[rng.below(relations.len())];
            let terms: Vec<String> = (0..*arity)
                .map(|column| {
                    let first = atom == 0 && column == 0;
                    if !first && rng.below(8) == 0 {
                        format!("c{}", rng.below(VALUES))
                    } else {
                        let v = VARIABLES[rng.below(VARIABLES.len())];
                        if !used.contains(&v) {
                            used.push(v);
                        }
                        v.to_string()
                    }
                })
                .collect();
            format!("{name}({})", terms.join(", "))
        })
        .collect();
    format!("Q({}) :- {}.", used.join(", "), atoms.join(", "))
}

/// Rows of `query` over `relations` as `R` through `JA` under `O` and
/// `policy`, sorted.
fn rows<R: Relation + Cardinality, JA: JoinEntry<R>, O: QueryOptimiser + Default + 'static>(
    relations: &Relations, query: &JoinQuery, policy: ColumnOrderPolicy,
) -> Result<Vec<Vec<usize>>, JoinError> {
    let mut inputs: Inputs = BTreeMap::new();
    let mut store: BTreeMap<String, R> = BTreeMap::new();
    for (name, arity, tuples) in relations {
        let header = RelationHeader::new_positional(name, *arity);
        inputs.insert(name.clone(), (header.clone(), tuples.clone()));
        store.insert(name.clone(), R::from_tuples(header, tuples.clone()));
    }
    let planner = Planner::new(Box::new(O::default()), policy);
    let mut database = JA::database(store, planner.required_statistics());
    let (mut rows, _) =
        join_under_planner::<R, JA>(&mut database, &inputs, query.clone(), &planner)?;
    rows.sort();
    Ok(rows)
}

/// For each seed: a query `stored` accepts gives the same rows under
/// `any`; a query `stored` rejects only for its column order runs under
/// `any`; any other rejection is identical under both. Returns how many
/// cases fell in the first two classes.
fn equivalent<
    R: Relation + Cardinality,
    JA: JoinEntry<R>,
    O: QueryOptimiser + Default + 'static,
>(
    seeds: std::ops::Range<u64>,
) -> (usize, usize) {
    let (mut agreed, mut lifted) = (0, 0);
    for seed in seeds {
        let mut rng = Lcg(seed);
        let relations = random_relations(&mut rng);
        let text = random_query(&mut rng, &relations);
        let query: JoinQuery = text
            .parse()
            .unwrap_or_else(|e| panic!("seed {seed}: unparsable query {text}: {e}"));
        let stored = rows::<R, JA, O>(&relations, &query, ColumnOrderPolicy::Stored);
        let any = rows::<R, JA, O>(&relations, &query, ColumnOrderPolicy::Any);
        match stored {
            | Ok(stored) => {
                assert_eq!(any.as_ref().ok(), Some(&stored), "seed {seed}: {text}");
                agreed += 1;
            },
            | Err(JoinError::CyclicAttributeOrder {
                ..
            }) => {
                assert!(any.is_ok(), "seed {seed}: {text}: {any:?}");
                lifted += 1;
            },
            | Err(other) => assert_eq!(any, Err(other), "seed {seed}: {text}"),
        }
    }
    (agreed, lifted)
}

const SEEDS: std::ops::Range<u64> = 0..300;

/// The bounds catch a generator that stops producing interesting cases:
/// the first 300 seeds give 273 queries `stored` accepts and 27 it rejects
/// for their column order (no other rejection).
#[test]
fn tree_trie_under_lexicographic() {
    let (agreed, lifted) = equivalent::<TreeTrie, LeapfrogTriejoin, LexicographicOptimiser>(SEEDS);
    assert!(
        agreed >= 100 && lifted >= 10,
        "agreed {agreed}, lifted {lifted}"
    );
}

#[test]
fn tree_trie_under_cardinality() {
    equivalent::<TreeTrie, LeapfrogTriejoin, CardinalityOptimiser>(SEEDS);
}

#[test]
fn tree_trie_under_cost_based() {
    equivalent::<TreeTrie, LeapfrogTriejoin, CostBasedOptimiser>(SEEDS);
}

#[test]
fn column_trie_under_lexicographic() {
    equivalent::<ColumnTrie, LeapfrogTriejoin, LexicographicOptimiser>(SEEDS);
}

#[test]
fn column_trie_under_cost_based() {
    equivalent::<ColumnTrie, LeapfrogTriejoin, CostBasedOptimiser>(SEEDS);
}

#[test]
fn hash_trie_under_lexicographic() {
    equivalent::<HashTrie<SipHashStrategy>, HashTriejoin, LexicographicOptimiser>(SEEDS);
}

#[test]
fn hash_trie_under_cost_based() {
    equivalent::<HashTrie<SipHashStrategy>, HashTriejoin, CostBasedOptimiser>(SEEDS);
}

#[test]
fn pruned_hash_trie_under_cardinality() {
    equivalent::<HashTrie<SipHashStrategy, SingletonPruning>, HashTriejoin, CardinalityOptimiser>(
        SEEDS,
    );
}

#[test]
fn lazy_hash_trie_under_cost_based() {
    equivalent::<
        HashTrie<SipHashStrategy, NoPruning, LazyExpansion>,
        HashTriejoin,
        CostBasedOptimiser,
    >(SEEDS);
}
