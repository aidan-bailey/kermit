use crate::key_type::KeyType;

pub trait Relation<KT>
where
    KT: KeyType,
{
    fn cardinality(&self) -> usize;
    fn insert(&mut self, tuple: Vec<KT>) -> bool;
    fn insert_all(&mut self, tuples: Vec<Vec<KT>>) -> bool;
    // fn from_tuples(cardinality: usize, tuples: Vec<Vec<KT>>) -> Self;
}
