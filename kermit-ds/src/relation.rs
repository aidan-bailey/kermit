use std::ops::Index;

pub trait Relational<KT: Clone> : Index<usize> {
    fn cardinality(&self) -> usize;
    fn tuples(&self) -> &Vec<Vec<KT>>;
    fn insert(&mut self, tuple: Vec<KT>);
    fn insert_all(&mut self, tuples: Vec<Vec<KT>>);
    fn clear(&mut self);
}

impl<KT: PartialOrd + PartialEq + Clone> Index<usize> for Relation<KT> {
    type Output = Vec<KT>;

    fn index(&self, index: usize) -> &Self::Output {
        &self.tuples[index]
    }
}

#[derive(Clone)]
pub struct Relation<KT: Clone> {
    cardinality: usize,
    tuples: Vec<Vec<KT>>,
}

impl<KT: Clone> Relation<KT> {
    pub fn new(cardinality: usize) -> Self {
        Relation {
            cardinality,
            tuples: Vec::new(),
        }
    }

    pub fn cardinality(&self) -> usize {
        self.cardinality
    }

    pub fn tuples(&self) -> &Vec<Vec<KT>> {
        &self.tuples
    }

    pub fn insert(&mut self, tuple: Vec<KT>) {
        if tuple.len() != self.cardinality {
            panic!("Tuple has wrong cardinality");
        }
        self.tuples.push(tuple);
    }

    pub fn insert_all(&mut self, tuples: Vec<Vec<KT>>) {
        for tuple in tuples.iter() {
            if tuple.len() != self.cardinality {
                panic!("Tuple has wrong cardinality");
            }
        }
        self.tuples.extend(tuples);
    }

    pub fn clear(&mut self) {
        self.tuples.clear();
    }
}
