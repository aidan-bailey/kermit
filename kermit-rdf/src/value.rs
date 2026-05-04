//! RDF value types used throughout the preprocessor.
//!
//! `RdfValue` is the canonical key type for the dictionary: every RDF term
//! we encounter (subject IRI, predicate IRI, object IRI, literal, blank
//! node) becomes one of these and is interned to a `usize`. Equality is
//! delegated to the underlying string forms so two parses of the same
//! source N-Triples line produce equal values.

use std::fmt;

/// An RDF term in the dictionary.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum RdfValue {
    /// An IRI (with surrounding angle brackets stripped).
    Iri(String),
    /// A blank node (with the leading `_:` preserved).
    BlankNode(String),
    /// A literal in N-Triples surface form (quotes + optional datatype/lang).
    Literal(String),
}

impl RdfValue {
    /// Returns the canonical string form used for dictionary serialization.
    /// IRIs are wrapped in `<...>`, blank nodes keep `_:`, literals keep
    /// their quoting and any datatype/lang tag exactly as parsed.
    pub fn to_canonical(&self) -> String {
        match self {
            | RdfValue::Iri(s) => format!("<{s}>"),
            | RdfValue::BlankNode(s) => s.clone(),
            | RdfValue::Literal(s) => s.clone(),
        }
    }
}

impl fmt::Display for RdfValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { f.write_str(&self.to_canonical()) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iri_canonical_wraps_in_angle_brackets() {
        let v = RdfValue::Iri("http://example/x".to_string());
        assert_eq!(v.to_canonical(), "<http://example/x>");
    }

    #[test]
    fn blank_node_canonical_preserves_prefix() {
        let v = RdfValue::BlankNode("_:b1".to_string());
        assert_eq!(v.to_canonical(), "_:b1");
    }

    #[test]
    fn literal_canonical_preserves_full_form() {
        let v = RdfValue::Literal("\"hello\"@en".to_string());
        assert_eq!(v.to_canonical(), "\"hello\"@en");
    }

    #[test]
    fn equality_uses_underlying_string() {
        let a = RdfValue::Iri("http://x".to_string());
        let b = RdfValue::Iri("http://x".to_string());
        assert_eq!(a, b);
    }

    #[test]
    fn iri_and_literal_with_same_content_are_distinct() {
        let iri = RdfValue::Iri("hello".to_string());
        let lit = RdfValue::Literal("hello".to_string());
        assert_ne!(iri, lit);
    }
}
