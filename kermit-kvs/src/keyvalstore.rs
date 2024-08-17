use std::hash::Hash;

pub trait KeyValStore<KT, VT>
where
    KT: Eq + Hash + Clone + PartialOrd,
    VT: Hash,
{
    fn add(&mut self, val: VT) -> KT;
    fn add_all(&mut self, val: Vec<VT>) -> Vec<KT>;
    fn get(&self, key: &KT) -> Option<&VT>;
    fn get_all(&self, key: Vec<&KT>) -> Vec<Option<&VT>>;
    fn keys(&self) -> Vec<KT>;
}
