use crate::key_type::KeyType;

pub trait Relation {
    type KT: KeyType;
    fn cardinality(&self) -> usize;
    fn insert(&mut self, tuple: Vec<Self::KT>) -> bool;
    fn insert_all(&mut self, tuples: Vec<Vec<Self::KT>>) -> bool;
    // fn from_tuples(cardinality: usize, tuples: Vec<Vec<KT>>) -> Self;
}
