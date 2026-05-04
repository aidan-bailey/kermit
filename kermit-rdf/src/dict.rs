//! Dictionary: bidirectional `RdfValue ↔ usize` map.
//!
//! Built by streaming the N-Triples file once; subjects, predicates, and
//! objects are all interned. The order of insertion is preserved (so dict
//! IDs are deterministic given the same input file). Predicates are
//! present in the dict alongside subjects/objects so the SPARQL translator
//! can use predicate IDs to disambiguate sanitization collisions.

use {crate::value::RdfValue, std::collections::HashMap};

/// A bidirectional `RdfValue ↔ usize` map preserving insertion order.
#[derive(Debug, Default, Clone)]
pub struct Dictionary {
    by_value: HashMap<RdfValue, usize>,
    by_id: Vec<RdfValue>,
}

impl Dictionary {
    /// Creates an empty dictionary.
    pub fn new() -> Self { Self::default() }

    /// Returns the ID of `value`, inserting it if not present.
    pub fn intern(&mut self, value: RdfValue) -> usize {
        if let Some(&id) = self.by_value.get(&value) {
            return id;
        }
        let id = self.by_id.len();
        self.by_id.push(value.clone());
        self.by_value.insert(value, id);
        id
    }

    /// Returns the ID for `value` if interned, else `None`.
    pub fn lookup(&self, value: &RdfValue) -> Option<usize> { self.by_value.get(value).copied() }

    /// Returns the value at `id` if it exists.
    pub fn get(&self, id: usize) -> Option<&RdfValue> { self.by_id.get(id) }

    /// Total entries.
    pub fn len(&self) -> usize { self.by_id.len() }

    /// True if no entries.
    pub fn is_empty(&self) -> bool { self.by_id.is_empty() }

    /// Iterates entries in insertion order as `(id, value)` pairs.
    pub fn iter(&self) -> impl Iterator<Item = (usize, &RdfValue)> { self.by_id.iter().enumerate() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intern_assigns_sequential_ids() {
        let mut d = Dictionary::new();
        let a = RdfValue::Iri("a".into());
        let b = RdfValue::Iri("b".into());
        assert_eq!(d.intern(a.clone()), 0);
        assert_eq!(d.intern(b.clone()), 1);
        assert_eq!(d.intern(a), 0);
    }

    #[test]
    fn lookup_returns_none_for_missing() {
        let d = Dictionary::new();
        assert_eq!(d.lookup(&RdfValue::Iri("missing".into())), None);
    }

    #[test]
    fn iter_preserves_insertion_order() {
        let mut d = Dictionary::new();
        d.intern(RdfValue::Iri("first".into()));
        d.intern(RdfValue::Iri("second".into()));
        d.intern(RdfValue::Iri("third".into()));
        let ordered: Vec<_> = d.iter().collect();
        assert_eq!(ordered[0].0, 0);
        assert_eq!(ordered[2].0, 2);
        assert_eq!(*ordered[0].1, RdfValue::Iri("first".into()));
    }

    #[test]
    fn distinguishes_iri_from_literal_with_same_text() {
        let mut d = Dictionary::new();
        let id1 = d.intern(RdfValue::Iri("x".into()));
        let id2 = d.intern(RdfValue::Literal("x".into()));
        assert_ne!(id1, id2);
        assert_eq!(d.len(), 2);
    }
}
