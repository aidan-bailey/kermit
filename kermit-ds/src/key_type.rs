use std::{fmt::Debug, str::FromStr};

pub trait KeyType: PartialOrd + PartialEq + Clone + FromStr + Debug {}
impl<KT> KeyType for KT where KT: PartialOrd + PartialEq + Clone + FromStr + Debug {}
