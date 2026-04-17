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
        rewrite_atoms, JoinAlgo, JoinAlgorithm, JoinQuery, LeapfrogTriejoin, SingletonTrieIter,
        TrieIterKind,
    },
    kermit_ds::{ColumnTrie, IndexStructure, Relation, RelationFileExt, TreeTrie},
    kermit_iters::TrieIterable,
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
    phantom_rb: std::marker::PhantomData<R>,
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
            phantom_rb: std::marker::PhantomData,
            phantom_ja: std::marker::PhantomData,
        }
    }

    fn name(&self) -> &String { &self.name }

    fn add_relation(&mut self, name: &str, arity: usize) {
        let relation = R::new(arity.into());
        self.relations.insert(name.to_owned(), relation);
    }

    fn add_keys(&mut self, relation_name: &str, keys: Vec<usize>) {
        self.relations.get_mut(relation_name).unwrap().insert(keys);
    }

    fn add_keys_batch(&mut self, relation_name: &str, keys: Vec<Vec<usize>>) {
        self.relations
            .get_mut(relation_name)
            .unwrap()
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
            if let Some(r) = self.relations.get(&pred.name) {
                wrappers.insert(pred.name.clone(), TrieIterKind::Relation(r));
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
            phantom_rb: std::marker::PhantomData,
            phantom_ja: std::marker::PhantomData,
        }
    }
}

/// Creates a [`DatabaseEngine`] as a `Box<dyn DB>` based on the CLI-selected
/// index structure and join algorithm.
pub fn instantiate_database(ds: IndexStructure, ja: JoinAlgorithm) -> Box<dyn DB> {
    match (ds, ja) {
        | (IndexStructure::TreeTrie, JoinAlgorithm::LeapfrogTriejoin) => Box::new(
            DatabaseEngine::<TreeTrie, LeapfrogTriejoin>::new("test".to_string()),
        ),
        | (IndexStructure::ColumnTrie, JoinAlgorithm::LeapfrogTriejoin) => Box::new(
            DatabaseEngine::<ColumnTrie, LeapfrogTriejoin>::new("test".to_string()),
        ),
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
}
