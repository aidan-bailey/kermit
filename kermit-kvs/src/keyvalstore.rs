use std::{fmt::Debug, hash::Hash, str::FromStr};

pub trait KeyValStore<KT, VT>
where
    KT: PartialOrd + PartialEq + Debug + Eq + Hash + Ord,
    VT: Hash,
{
    fn add(&mut self, val: VT) -> KT;
    fn add_all(&mut self, val: Vec<VT>) -> Vec<KT>;
    fn get(&self, key: &KT) -> Option<&VT>;
    fn get_all(&self, key: Vec<&KT>) -> Vec<Option<&VT>>;
    fn keys(&self) -> Vec<KT>;
    fn size(&self) -> usize;
    fn contains_key(&self, key: &KT) -> bool;
    fn contains_val(&self, val: &VT) -> bool;
}
