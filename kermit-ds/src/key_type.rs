use std::{fmt::Debug, hash::Hash, str::FromStr};

pub trait KeyType: PartialOrd + PartialEq + Clone + FromStr + Debug + Eq + Hash + Ord {}
impl<KT> KeyType for KT where KT: PartialOrd + PartialEq + Clone + FromStr + Debug + Eq + Hash + Ord {}
