//! This module defines the `KeyType` trait, which specifies the requirements
//! for a key type used in relations. NOTE: Yes, the number of traits is large
//! and probably not necessary.

use std::{fmt::Debug, hash::Hash};

/// Trait for key types used in relations.
///
/// TODO: Consider if all these traits are necessary.
pub trait KeyType: PartialOrd + PartialEq + Debug + Eq + Hash + Ord + Sized + Copy {}
impl<KT> KeyType for KT where KT: PartialOrd + PartialEq + Debug + Eq + Hash + Ord + Sized + Copy {}
