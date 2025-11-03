use std::hash::Hash;

pub trait KeyValStore<VT>
where
    VT: Hash,
{
    fn add(&mut self, val: VT) -> usize;
    fn add_all(&mut self, val: Vec<VT>) -> Vec<usize>;
    fn get(&self, key: &usize) -> Option<&VT>;
    fn get_all(&self, key: Vec<&usize>) -> Vec<Option<&VT>>;
    fn keys(&self) -> Vec<usize>;
    fn size(&self) -> usize;
    fn contains_key(&self, key: &usize) -> bool;
    fn contains_val(&self, val: &VT) -> bool;
}
