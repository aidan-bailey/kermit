#[derive(Clone)]
pub struct Relation<KT: Clone> {
    name: String,
    cardinality: usize,
    tuples: Vec<Box<[KT]>>,
}

impl<KT: Clone> Relation<KT> {
    pub fn new(name: String, cardinality: usize) -> Self {
        Relation {
            name,
            cardinality,
            tuples: Vec::new(),
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn cardinality(&self) -> usize {
        self.cardinality
    }

    pub fn tuples(&self) -> &Vec<Box<[KT]>> {
        &self.tuples
    }

    pub fn insert(&mut self, tuple: Box<[KT]>) {
        self.tuples.push(tuple);
    }

    pub fn insert_all(&mut self, tuples: Vec<Box<[KT]>>) {
        self.tuples.extend(tuples);
    }

    pub fn clear(&mut self) {
        self.tuples.clear();
    }
}
