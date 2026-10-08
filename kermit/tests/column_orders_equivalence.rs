//! `any` returns the same multiset as `stored` for every query `stored`
//! accepts, and answers every query `stored` rejects only for its column
//! order (issue #93); both return the rows a nested-loop evaluator
//! computes. Random small conjunctive queries over random small relations,
//! from fixed seeds, so a failure reproduces; no new crate.

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
        SingletonPruning, TreeTrie, Tuples,
    },
    kermit_iters::SipHashStrategy,
    kermit_parser::{Predicate, Term},
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
        inputs.insert(name.clone(), (header.clone(), Tuples::from(tuples.clone())));
        store.insert(name.clone(), R::from_tuples(header, tuples.clone()));
    }
    let planner = Planner::new(Box::new(O::default()), policy);
    let mut database = JA::database(store, planner.required_statistics());
    let (mut rows, _) =
        join_under_planner::<R, JA>(&mut database, &inputs, query.clone(), &planner)?;
    rows.sort();
    Ok(rows)
}

/// `tuples` as `R` holds them: the hash trie keeps duplicates (a
/// multiset), the sorted tries drop them (a set), and `R`'s tuple count
/// says which.
fn as_held<R: Relation + Cardinality>(
    name: &str, arity: usize, tuples: &[Vec<usize>],
) -> Vec<Vec<usize>> {
    let held = R::from_tuples(RelationHeader::new_positional(name, arity), tuples.to_vec());
    let mut distinct = tuples.to_vec();
    distinct.sort();
    distinct.dedup();
    if held.tuple_count() == tuples.len() {
        tuples.to_vec()
    } else {
        assert_eq!(
            held.tuple_count(),
            distinct.len(),
            "{name}: neither a set nor a bag"
        );
        distinct
    }
}

/// The rows `query` must return over `relations` as `R` holds them, by
/// nested loops in body order: each way of matching every atom to one held
/// tuple, a variable taking one value throughout and a constant `c<k>`
/// matching `k`, is one row, projected to the head. Bag semantics, so a
/// duplicate tuple matches twice. No planner, rewrite or executor is
/// involved, so this checks the rows of the queries `stored` rejects as
/// well as those it accepts.
fn oracle<R: Relation + Cardinality>(relations: &Relations, query: &JoinQuery) -> Vec<Vec<usize>> {
    let held: BTreeMap<&str, Vec<Vec<usize>>> = relations
        .iter()
        .map(|(name, arity, tuples)| (name.as_str(), as_held::<R>(name, *arity, tuples)))
        .collect();
    let mut rows = Vec::new();
    match_atoms(
        &query.body,
        &held,
        &mut BTreeMap::new(),
        &query.head,
        &mut rows,
    );
    rows.sort();
    rows
}

/// Matches `atoms` in order under `binding`, pushing one head row per
/// complete match.
fn match_atoms<'q>(
    atoms: &'q [Predicate], held: &BTreeMap<&str, Vec<Vec<usize>>>,
    binding: &mut BTreeMap<&'q str, usize>, head: &Predicate, rows: &mut Vec<Vec<usize>>,
) {
    let Some((atom, rest)) = atoms.split_first() else {
        let row = head
            .terms
            .iter()
            .map(|term| match term {
                | Term::Var(v) => binding[v.as_str()],
                | _ => unreachable!("the generator's head is variables"),
            })
            .collect();
        rows.push(row);
        return;
    };
    for tuple in &held[atom.name.as_str()] {
        let mut bound_here: Vec<&str> = Vec::new();
        let matches = atom
            .terms
            .iter()
            .zip(tuple)
            .all(|(term, &value)| match term {
                | Term::Var(v) => match binding.get(v.as_str()) {
                    | Some(&bound) => bound == value,
                    | None => {
                        binding.insert(v.as_str(), value);
                        bound_here.push(v.as_str());
                        true
                    },
                },
                | Term::Atom(c) => c[1..].parse() == Ok(value),
                | Term::Placeholder => true,
            });
        if matches {
            match_atoms(rest, held, binding, head, rows);
        }
        for v in bound_here {
            binding.remove(v);
        }
    }
}

/// How the seeds fell: queries `stored` accepts, queries it rejects only
/// for their column order, and how many of those return a row.
#[derive(Debug, Default)]
struct Cases {
    agreed: usize,
    lifted: usize,
    lifted_with_rows: usize,
}

/// For each seed: a query `stored` accepts gives the oracle's rows under
/// both policies; a query `stored` rejects only for its column order gives
/// them under `any`; any other rejection is identical under both.
fn equivalent<
    R: Relation + Cardinality,
    JA: JoinEntry<R>,
    O: QueryOptimiser + Default + 'static,
>(
    seeds: std::ops::Range<u64>,
) -> Cases {
    let mut cases = Cases::default();
    for seed in seeds {
        let mut rng = Lcg(seed);
        let relations = random_relations(&mut rng);
        let text = random_query(&mut rng, &relations);
        let query: JoinQuery = text
            .parse()
            .unwrap_or_else(|e| panic!("seed {seed}: unparsable query {text}: {e}"));
        let want = oracle::<R>(&relations, &query);
        let stored = rows::<R, JA, O>(&relations, &query, ColumnOrderPolicy::Stored);
        let any = rows::<R, JA, O>(&relations, &query, ColumnOrderPolicy::Any);
        match stored {
            | Ok(stored) => {
                assert_eq!(stored, want, "seed {seed}: {text} under stored");
                assert_eq!(any.ok(), Some(want), "seed {seed}: {text} under any");
                cases.agreed += 1;
            },
            | Err(JoinError::CyclicAttributeOrder {
                ..
            }) => {
                cases.lifted_with_rows += usize::from(!want.is_empty());
                assert_eq!(any.ok(), Some(want), "seed {seed}: {text} under any");
                cases.lifted += 1;
            },
            | Err(other) => assert_eq!(any, Err(other), "seed {seed}: {text}"),
        }
    }
    cases
}

const SEEDS: std::ops::Range<u64> = 0..1000;

/// The bounds catch a generator that stops producing interesting cases:
/// the first 1000 seeds give 903 queries `stored` accepts and 97 it
/// rejects for their column order (no other rejection), 20 of which return
/// rows.
#[test]
fn tree_trie_under_lexicographic() {
    let cases = equivalent::<TreeTrie, LeapfrogTriejoin, LexicographicOptimiser>(SEEDS);
    assert!(
        cases.agreed >= 300 && cases.lifted >= 30 && cases.lifted_with_rows >= 10,
        "{cases:?}"
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
