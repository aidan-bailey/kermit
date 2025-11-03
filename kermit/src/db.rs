use {
    kermit_algos::{JoinAlgo, JoinAlgorithm, JoinQuery, LeapfrogTriejoin},
    kermit_ds::{ColumnTrie, IndexStructure, Relation, RelationFileExt, TreeTrie},
    std::{collections::HashMap, path::Path},
};

pub trait DB {
    fn new(name: String) -> Self
    where
        Self: Sized;

    fn name(&self) -> &String;

    fn add_relation(&mut self, name: &str, arity: usize);

    fn add_keys(&mut self, relation_name: &str, keys: Vec<usize>);

    fn add_keys_batch(&mut self, relation_name: &str, keys: Vec<Vec<usize>>);

    fn join(&self, query: kermit_algos::JoinQuery);

    fn add_file(&mut self, filepath: &Path) -> Result<(), std::io::Error>;
}

pub struct Database<R, JA>
where
    R: Relation,
    JA: JoinAlgo<R>,
{
    name: String,
    relations: HashMap<String, R>,
    phantom_rb: std::marker::PhantomData<R>,
    phantom_ja: std::marker::PhantomData<JA>,
}

impl<R, JA> DB for Database<R, JA>
where
    R: Relation,
    JA: JoinAlgo<R>,
{
    fn new(name: String) -> Self
    where
        Self: Sized,
    {
        Database {
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

    fn join(&self, query: JoinQuery) {
        // Build datastructure map from predicate names in the query body
        let mut ds_map: HashMap<String, &R> = HashMap::new();
        for pred in &query.body {
            let r = self
                .relations
                .get(&pred.name)
                .expect("missing relation in DB for predicate");
            ds_map.entry(pred.name.clone()).or_insert(r);
        }

        // Execute join and collect results (discard relation construction for now)
        let _tuples: Vec<Vec<usize>> = JA::join_iter(query, ds_map).collect();
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
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?,
            | "parquet" => R::from_parquet(path)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?,
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

impl<R, JA> Database<R, JA>
where
    R: Relation,
    JA: JoinAlgo<R>,
{
    pub fn new(name: String) -> Self { <Self as DB>::new(name) }
}

#[cfg(test)]
mod tests {

    use {super::*, kermit_algos::{JoinQuery, LeapfrogTriejoin}, kermit_ds::TreeTrie};

    #[test]
    fn test_relation() {
        let mut db: Database<TreeTrie, LeapfrogTriejoin> = Database::new("test".to_string());
        let relation_name = "apple".to_string();
        db.add_relation(&relation_name, 3);
        db.add_keys(&relation_name, vec![1, 2, 3])
    }

    #[test]
    fn test_join() {
        let mut db: Database<TreeTrie, LeapfrogTriejoin> = Database::new("test".to_string());

        db.add_relation("first", 1);
        db.add_keys_batch("first", vec![vec![1_usize], vec![2], vec![3]]);

        db.add_relation("second", 1);
        db.add_keys_batch("second", vec![vec![1_usize], vec![2], vec![3]]);

        let query: JoinQuery = "Q(X) :- first(X), second(X).".parse().unwrap();
        let _res = db.join(query);
    }
}

pub fn instantiate_database(ds: IndexStructure, ja: JoinAlgorithm) -> Box<dyn DB> {
    match (ds, ja) {
        | (IndexStructure::TreeTrie, JoinAlgorithm::LeapfrogTriejoin) => {
            Box::new(Database::<TreeTrie, LeapfrogTriejoin>::new(
                "test".to_string(),
            ))
        },
        | (IndexStructure::ColumnTrie, JoinAlgorithm::LeapfrogTriejoin) => {
            Box::new(Database::<ColumnTrie, LeapfrogTriejoin>::new(
                "test".to_string(),
            ))
        },
    }
}
