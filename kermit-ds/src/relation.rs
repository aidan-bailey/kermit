use std::collections::BTreeSet;

pub trait Relational<KT: Clone + PartialEq> {
    fn new(cardinality: usize) -> Self;
    fn cardinality(&self) -> usize;
    fn tuples(&self) -> Vec<Vec<&KT>>;
    fn insert(&mut self, tuple: Vec<KT>) -> bool;
    fn remove(&mut self, tuple: Vec<KT>) -> bool;
    fn clear(&mut self);
}

#[derive(Clone)]
pub struct Relation<KT: Clone + Ord> {
    cardinality: usize,
    tuples: BTreeSet<Vec<KT>>,
}

impl<KT: Clone + Ord> Relational<KT> for Relation<KT> {
    fn new(cardinality: usize) -> Self {
        Relation {
            cardinality,
            tuples: BTreeSet::<Vec<KT>>::new(),
        }
    }

    fn cardinality(&self) -> usize {
        self.cardinality
    }

    fn tuples(&self) -> Vec<Vec<&KT>> {
        self.tuples
            .iter()
            .map(|tuple| tuple.iter().collect())
            .collect()
    }

    fn insert(&mut self, tuple: Vec<KT>) -> bool {
        if tuple.len() != self.cardinality {
            panic!("Tuple has wrong cardinality");
        }
        self.tuples.insert(tuple)
    }

    fn clear(&mut self) {
        self.tuples.clear();
    }

    fn remove(&mut self, tuple: Vec<KT>) -> bool {
        if tuple.len() != self.cardinality {
            panic!("Tuple has wrong cardinality");
        }
        self.tuples.remove(&tuple)
    }
}
