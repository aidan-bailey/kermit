//! Thin wrapper around `spargebra::Query::parse`.

use {crate::error::RdfError, spargebra::Query};

/// Parses a SPARQL query string into a `spargebra::Query` AST.
///
/// All errors from `spargebra` are mapped to [`RdfError::SparqlParse`].
pub fn parse_query(text: &str) -> Result<Query, RdfError> {
    Query::parse(text, None).map_err(|e| RdfError::SparqlParse(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_simple_select() {
        let q = parse_query("SELECT ?x WHERE { ?x <http://p> ?y . }").unwrap();
        assert!(matches!(q, Query::Select { .. }));
    }

    #[test]
    fn rejects_garbage() {
        let err = parse_query("not a query").unwrap_err();
        assert!(matches!(err, RdfError::SparqlParse(_)));
    }
}
