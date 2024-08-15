use std::hash::Hash;

trait KeyValStore<KT, VT>
where
    KT: PartialEq + PartialOrd + Clone,
    VT: Hash,
{
    fn add(&mut self, val: VT) -> KT;
    fn add_all(&mut self, val: Vec<VT>) -> Vec<KT>;
    fn get(&self, key: KT) -> Option<VT>;
    fn get_all(&self, key: Vec<KT>) -> Vec<VT>;
}
