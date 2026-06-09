//! Database abstraction bridging query parsing and data structures.
//!
//! The [`DB`] trait erases the concrete `Relation` and `JoinAlgo` type
//! parameters so the CLI can hold `Box<dyn DB>` regardless of which data
//! structure or algorithm the user selects. [`DatabaseEngine`] is the sole
//! implementation, parameterised by the chosen types, and
//! `instantiate_database` dispatches on the `IndexStructure` /
//! `JoinAlgorithm` CLI enums to produce the right concrete combination.

use {
    kermit_algos::{
        rewrite_atoms, HashTrieIterKind, HashTriejoin, JoinAlgo, JoinAlgorithm, JoinQuery,
        LeapfrogTriejoin, SingletonHashTrieIter, SingletonTrieIter, TrieIterKind,
    },
    kermit_ds::{ColumnTrie, IndexStructure, Relation, RelationFileExt, TreeTrie},
    kermit_iters::{HashStrategy, HashTrieIterable, TrieIterable},
    std::{collections::HashMap, path::Path},
};

/// Object-safe interface for a relational database that can store relations
/// and execute join queries. Erases the concrete `Relation` and `JoinAlgo`
/// type parameters.
pub trait DB {
    /// Creates a new database with the given name.
    fn new(name: String) -> Self
    where
        Self: Sized;

    /// Returns the database's name.
    fn name(&self) -> &String;

    /// Registers a new empty relation with the given name and arity.
    fn add_relation(&mut self, name: &str, arity: usize);

    /// Inserts a single tuple into the named relation.
    fn add_keys(&mut self, relation_name: &str, keys: Vec<usize>);

    /// Inserts multiple tuples into the named relation.
    fn add_keys_batch(&mut self, relation_name: &str, keys: Vec<Vec<usize>>);

    /// Executes `query` against the registered relations and materialises
    /// the result tuples.
    fn join(&self, query: kermit_algos::JoinQuery) -> Vec<Vec<usize>>;

    /// Loads a relation from a file (CSV or Parquet) and registers it.
    ///
    /// # Errors
    ///
    /// Returns `std::io::Error` if the extension is unsupported, the file
    /// cannot be read, or the relation cannot be parsed.
    fn add_file(&mut self, filepath: &Path) -> Result<(), std::io::Error>;
}

/// A typed relational database parameterized by its data structure `R` and
/// join algorithm `JA`.
///
/// Implements the object-safe [`DB`] trait so it can be used behind `Box<dyn
/// DB>`.
pub struct DatabaseEngine<R, JA>
where
    R: Relation,
{
    name: String,
    relations: HashMap<String, R>,
    // `JA` does not appear in any field; PhantomData satisfies the
    // unused-type-parameter rule. `R` is already used by `relations`.
    phantom_ja: std::marker::PhantomData<JA>,
}

impl<R, JA> DB for DatabaseEngine<R, JA>
where
    R: Relation + TrieIterable,
    JA: for<'a> JoinAlgo<TrieIterKind<'a, R>>,
{
    fn new(name: String) -> Self
    where
        Self: Sized,
    {
        DatabaseEngine {
            name,
            relations: HashMap::new(),
            phantom_ja: std::marker::PhantomData,
        }
    }

    fn name(&self) -> &String { &self.name }

    fn add_relation(&mut self, name: &str, arity: usize) {
        let relation = R::new(arity.into());
        self.relations.insert(name.to_owned(), relation);
    }

    fn add_keys(&mut self, relation_name: &str, keys: Vec<usize>) {
        self.relations
            .get_mut(relation_name)
            .unwrap_or_else(|| {
                panic!(
                    "DB::add_keys: relation {relation_name:?} not registered; call add_relation \
                     first"
                )
            })
            .insert(keys);
    }

    fn add_keys_batch(&mut self, relation_name: &str, keys: Vec<Vec<usize>>) {
        self.relations
            .get_mut(relation_name)
            .unwrap_or_else(|| {
                panic!(
                    "DB::add_keys_batch: relation {relation_name:?} not registered; call \
                     add_relation first"
                )
            })
            .insert_all(keys);
    }

    fn join(&self, query: JoinQuery) -> Vec<Vec<usize>> {
        let (rewritten, const_specs) =
            rewrite_atoms(query).expect("malformed constant atom in query");

        let mut wrappers: HashMap<String, TrieIterKind<'_, R>> = HashMap::new();
        for pred in &rewritten.body {
            if wrappers.contains_key(&pred.name) {
                continue;
            }
            // Const_* predicates are synthetic — created by rewrite_atoms
            // above and materialised from const_specs below. They aren't
            // expected to live in self.relations.
            if pred.name.starts_with("Const_") {
                continue;
            }
            match self.relations.get(&pred.name) {
                | Some(r) => {
                    wrappers.insert(pred.name.clone(), TrieIterKind::Relation(r));
                },
                | None => panic!(
                    "DatabaseEngine::join: query body references unknown relation {:?}; known \
                     relations: {:?}",
                    pred.name,
                    self.relations.keys().collect::<Vec<_>>(),
                ),
            }
        }
        for (name, id) in const_specs {
            wrappers
                .entry(name)
                .or_insert_with(|| TrieIterKind::Singleton(SingletonTrieIter::new(id)));
        }

        let ds_map: HashMap<String, &TrieIterKind<'_, R>> =
            wrappers.iter().map(|(k, v)| (k.clone(), v)).collect();

        JA::join_iter(rewritten, ds_map).collect()
    }

    /// Loads a relation from a file (CSV or Parquet) and adds it to the
    /// database.
    ///
    /// The file type is determined by the extension (.csv or .parquet).
    /// The relation name is extracted from the filename.
    fn add_file(&mut self, filepath: &Path) -> Result<(), std::io::Error> {
        let path = filepath;
        let extension = path.extension().and_then(|s| s.to_str()).unwrap_or("");

        let relation = match extension.to_lowercase().as_str() {
            | "csv" => R::from_csv(path)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?,
            | "parquet" => R::from_parquet(path)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?,
            | _ => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    format!("Unsupported file extension: {}", extension),
                ))
            },
        };

        let relation_name = relation.header().name().to_string();
        self.relations.insert(relation_name, relation);

        Ok(())
    }
}

impl<R, JA> DatabaseEngine<R, JA>
where
    R: Relation,
{
    /// Inherent constructor so tests can build the engine without needing
    /// the full [`DB`] trait bound in scope.
    pub fn new(name: String) -> Self {
        DatabaseEngine {
            name,
            relations: HashMap::new(),
            phantom_ja: std::marker::PhantomData,
        }
    }
}

/// Hash-family join entry point. Mirror of [`DatabaseEngine::join`] for
/// algorithms in the hash-trie family (currently [`HashTriejoin`]).
///
/// Lives as a free function rather than a [`DB`] trait method because
/// Rust's coherence rules (E0119) reject a parallel `impl<R:
/// HashTrieIterable, JA: ...> DB for DatabaseEngine<R, JA>` block that
/// would overlap with the existing LFTJ-family impl, even though the
/// bounds are disjoint in practice. The CLI dispatches directly to this
/// function for hash-family algorithms, sidestepping the [`DB`] trait
/// entirely on that path.
///
/// Mirrors the sorted-family body, but builds [`HashTrieIterKind`]
/// wrappers and synthesises [`SingletonHashTrieIter`] singletons for the
/// `Const_*` predicates introduced by [`rewrite_atoms`]. Constant atoms
/// hashed for the singleton use the same [`HashStrategy`] `H` that the
/// peer `HashTrie<H>` relations were built with, so the algorithm sees
/// matching hashes on both sides of the intersection.
///
/// The second type parameter `H` selects the hash function used for
/// any constant-atom singletons; callers must thread the same `H` used
/// when constructing the `HashTrie<H>` relations. Rust forbids defaults
/// on free-function type parameters (see issue #36887), so all callers
/// must specify the strategy explicitly via turbofish — Phase 4 of the
/// optimization-standard plan threads this through the CLI dispatch.
///
/// # Panics
///
/// Panics if the query references a relation name not present in
/// `relations` (matching the behaviour of [`DatabaseEngine::join`]) or if
/// the query contains a malformed constant atom.
pub fn hash_join<R, H>(relations: &HashMap<String, R>, query: JoinQuery) -> Vec<Vec<usize>>
where
    R: HashTrieIterable,
    H: HashStrategy,
{
    let (rewritten, const_specs) = rewrite_atoms(query).expect("malformed constant atom in query");

    let mut wrappers: HashMap<String, HashTrieIterKind<'_, R>> = HashMap::new();
    for pred in &rewritten.body {
        if wrappers.contains_key(&pred.name) {
            continue;
        }
        // Const_* predicates are synthetic — created by rewrite_atoms
        // above and materialised from const_specs below. They aren't
        // expected to live in `relations`.
        if pred.name.starts_with("Const_") {
            continue;
        }
        match relations.get(&pred.name) {
            | Some(r) => {
                wrappers.insert(pred.name.clone(), HashTrieIterKind::Relation(r));
            },
            | None => panic!(
                "hash_join: query body references unknown relation {:?}; known relations: {:?}",
                pred.name,
                relations.keys().collect::<Vec<_>>(),
            ),
        }
    }
    for (name, id) in const_specs {
        wrappers.entry(name).or_insert_with(|| {
            let hash = H::hash(id);
            HashTrieIterKind::Singleton(SingletonHashTrieIter::new(id, hash))
        });
    }

    let ds_map: HashMap<String, &HashTrieIterKind<'_, R>> =
        wrappers.iter().map(|(k, v)| (k.clone(), v)).collect();

    <HashTriejoin as JoinAlgo<HashTrieIterKind<'_, R>>>::join_iter(rewritten, ds_map).collect()
}

/// Creates a [`DatabaseEngine`] as a `Box<dyn DB>` based on the CLI-selected
/// index structure and join algorithm. `name` is exposed via [`DB::name`] —
/// callers typically pass the benchmark or query identifier so downstream
/// tooling can correlate engines with workloads.
///
/// # Panics
///
/// Panics on incompatible `(IndexStructure, JoinAlgorithm)` pairs. The
/// CLI is expected to reject such combinations upstream via
/// `IndexStructureSelector::supports_algorithm`; these panics are
/// defence-in-depth for direct programmatic callers. The hash-trie
/// family (`HashTrie` + `HashTriejoin`) deliberately panics here too —
/// see the function's body for the dedicated [`hash_join`] free-function
/// path the CLI takes for that combination.
pub fn instantiate_database(ds: IndexStructure, ja: JoinAlgorithm, name: String) -> Box<dyn DB> {
    match (ds, ja) {
        | (IndexStructure::TreeTrie, JoinAlgorithm::LeapfrogTriejoin) => {
            Box::new(DatabaseEngine::<TreeTrie, LeapfrogTriejoin>::new(name))
        },
        | (IndexStructure::ColumnTrie, JoinAlgorithm::LeapfrogTriejoin) => {
            Box::new(DatabaseEngine::<ColumnTrie, LeapfrogTriejoin>::new(name))
        },
        // The hash-trie family does not flow through the `DB` trait —
        // `DB::join` is implementation-coupled to `TrieIterKind`, which
        // is incompatible with `HashTrieIterable`. The CLI dispatches
        // directly to the [`hash_join`] free function for this pair, so
        // `instantiate_database` is never called with it from the CLI.
        // Programmatic callers reaching this arm have a usage bug.
        | (IndexStructure::HashTrie, JoinAlgorithm::HashTriejoin) => panic!(
            "instantiate_database: (HashTrie, HashTriejoin) does not go through the DB trait — \
             use kermit::db::hash_join directly (the CLI dispatch handles this in \
             BenchSubcommand::Run / Ds)"
        ),
        // Incompatible pairings: `HashTrie` only joins via `HashTriejoin`
        // (different trait family — `HashTrieIterable` rather than
        // `TrieIterable`), and `HashTriejoin` only consumes `HashTrie`.
        | (IndexStructure::HashTrie, JoinAlgorithm::LeapfrogTriejoin) => panic!(
            "incompatible pair: (HashTrie, LeapfrogTriejoin) — HashTrie can only be joined with \
             HashTriejoin; the CLI's supports_algorithm gate should reject this upstream"
        ),
        | (IndexStructure::TreeTrie | IndexStructure::ColumnTrie, JoinAlgorithm::HashTriejoin) => {
            panic!(
                "incompatible pair: ({ds:?}, HashTriejoin) — HashTriejoin can only be used with \
                 HashTrie; the CLI's supports_algorithm gate should reject this upstream"
            )
        },
    }
}

#[cfg(test)]
mod tests {

    use {
        super::*,
        kermit_algos::{JoinQuery, LeapfrogTriejoin},
        kermit_ds::TreeTrie,
    };

    #[test]
    fn test_relation() {
        let mut db: DatabaseEngine<TreeTrie, LeapfrogTriejoin> =
            DatabaseEngine::new("test".to_string());
        let relation_name = "apple".to_string();
        db.add_relation(&relation_name, 3);
        db.add_keys(&relation_name, vec![1, 2, 3])
    }

    #[test]
    fn test_join() {
        let mut db: DatabaseEngine<TreeTrie, LeapfrogTriejoin> =
            DatabaseEngine::new("test".to_string());

        db.add_relation("first", 1);
        db.add_keys_batch("first", vec![vec![1_usize], vec![2], vec![3]]);

        db.add_relation("second", 1);
        db.add_keys_batch("second", vec![vec![1_usize], vec![2], vec![3]]);

        let query: JoinQuery = "Q(X) :- first(X), second(X).".parse().unwrap();
        db.join(query);
    }

    #[test]
    fn test_join_with_constant_filter() {
        let mut db: DatabaseEngine<TreeTrie, LeapfrogTriejoin> =
            DatabaseEngine::new("test".to_string());

        db.add_relation("p", 2);
        db.add_keys_batch("p", vec![vec![1, 10], vec![2, 20], vec![3, 30]]);

        let query: JoinQuery = "Q(X) :- p(X, c10).".parse().unwrap();
        let result = db.join(query);
        let mut got: Vec<_> = result.iter().map(|r| r[0]).collect();
        got.sort();
        assert_eq!(
            got,
            vec![1],
            "expected only X=1 to pass the c10 filter, got {got:?}"
        );
    }

    #[test]
    #[should_panic(expected = "unknown relation")]
    fn test_join_panics_on_missing_relation() {
        let db: DatabaseEngine<TreeTrie, LeapfrogTriejoin> =
            DatabaseEngine::new("test".to_string());

        // `missing` was never added; previously the body predicate was
        // silently dropped, which could mask typos or load failures.
        let query: JoinQuery = "Q(X) :- missing(X).".parse().unwrap();
        db.join(query);
    }
}

#[cfg(test)]
mod hash_join_tests {
    use {super::*, kermit_ds::HashTrie, kermit_iters::SipHashStrategy};

    /// Pins the basic happy path: build two unary `HashTrie`s, run a
    /// straight intersection through the free function, verify the
    /// rewrite + dispatch + collect chain end-to-end.
    #[test]
    fn hash_join_unary_intersection() {
        let mut relations: HashMap<String, HashTrie> = HashMap::new();
        relations.insert(
            "R".to_string(),
            HashTrie::from_tuples(1.into(), vec![vec![1], vec![2], vec![3]]),
        );
        relations.insert(
            "S".to_string(),
            HashTrie::from_tuples(1.into(), vec![vec![2], vec![3], vec![4]]),
        );
        let q: JoinQuery = "Q(X) :- R(X), S(X).".parse().unwrap();
        let mut out = hash_join::<HashTrie<SipHashStrategy>, SipHashStrategy>(&relations, q);
        out.sort();
        assert_eq!(out, vec![vec![2], vec![3]]);
    }

    /// `Q(X) :- R(X, c5).` — pins the const-view rewrite path through
    /// `SingletonHashTrieIter`. The rewrite turns the atom `c5` into a
    /// synthetic unary predicate `Const_c5` backed by a singleton, which
    /// `hash_join` materialises into a [`HashTrieIterKind::Singleton`]
    /// wrapper alongside the relation.
    ///
    /// We deliberately place the constant in the trailing column. The
    /// hash-trie iter family descends through *physical* attribute
    /// positions in lockstep with the algorithm's *variable* ordering;
    /// the variable ordering is "head vars first, then any extra body
    /// vars" (`build_variable_index`). With the constant in the last
    /// column, the rewrite's fresh `K0` lands at the tail of the
    /// variable ordering, which matches the trie's physical layout. The
    /// LFTJ const test uses the same shape for the same reason.
    ///
    /// The algorithm emits tuples in `variable_ordering` order (all
    /// query variables, not just head vars), so we project to the head
    /// slot ourselves — mirroring [`tests::test_join_with_constant_filter`]
    /// for LFTJ above.
    #[test]
    fn hash_join_with_constant_atom() {
        let mut relations: HashMap<String, HashTrie> = HashMap::new();
        relations.insert(
            "R".to_string(),
            HashTrie::from_tuples(2.into(), vec![vec![1, 5], vec![2, 5], vec![3, 7]]),
        );
        let q: JoinQuery = "Q(X) :- R(X, c5).".parse().unwrap();
        let result = hash_join::<HashTrie<SipHashStrategy>, SipHashStrategy>(&relations, q);
        let mut got: Vec<usize> = result.iter().map(|r| r[0]).collect();
        got.sort();
        assert_eq!(
            got,
            vec![1, 2],
            "expected only X=1, X=2 to pass the c5 filter, got {got:?}"
        );
    }
}
