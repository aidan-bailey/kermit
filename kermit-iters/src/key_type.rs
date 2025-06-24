//! This module defines the `KeyType` trait, which specifies the requirements
//! for a key type used in relations. NOTE: Yes, the number of traits is large
//! and probably not necessary.

use std::fmt::Debug;

/// Trait for key types used in relations.
pub trait KeyType: Debug + Ord + Copy {}
impl<KT> KeyType for KT where KT: Debug + Ord + Copy {}
