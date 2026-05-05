//! The 14 LUBM benchmark queries (paper Appendix A) and their published
//! LUBM(1, 0) reference cardinalities (paper Table 3).
//!
//! Queries are committed as `kermit-rdf/queries/lubm/q1.sparql … q14.sparql`
//! and embedded here via `include_str!`, so the binary is self-contained
//! and the file paths do not need to be plumbed through the CLI.
//!
//! Reference cardinalities are from the LUBM paper Table 3, columns labelled
//! "DLDB-OWL" (the only system in the paper that achieved 100% completeness
//! across all queries). They serve as the regression target for the
//! `lubm_cardinalities` integration test in Phase 5.

use crate::lubm::pipeline::LubmQuerySpec;

/// Static `(stem, sparql, expected_at_lubm_1_0)` triples for the 14 LUBM
/// queries.
const QUERY_DATA: &[(&str, &str, Option<u64>)] = &[
    ("q1", include_str!("../../queries/lubm/q1.sparql"), Some(4)),
    ("q2", include_str!("../../queries/lubm/q2.sparql"), Some(0)),
    ("q3", include_str!("../../queries/lubm/q3.sparql"), Some(6)),
    ("q4", include_str!("../../queries/lubm/q4.sparql"), Some(34)),
    (
        "q5",
        include_str!("../../queries/lubm/q5.sparql"),
        Some(719),
    ),
    (
        "q6",
        include_str!("../../queries/lubm/q6.sparql"),
        Some(7790),
    ),
    ("q7", include_str!("../../queries/lubm/q7.sparql"), Some(67)),
    (
        "q8",
        include_str!("../../queries/lubm/q8.sparql"),
        Some(7790),
    ),
    (
        "q9",
        include_str!("../../queries/lubm/q9.sparql"),
        Some(208),
    ),
    (
        "q10",
        include_str!("../../queries/lubm/q10.sparql"),
        Some(4),
    ),
    (
        "q11",
        include_str!("../../queries/lubm/q11.sparql"),
        Some(224),
    ),
    (
        "q12",
        include_str!("../../queries/lubm/q12.sparql"),
        Some(15),
    ),
    (
        "q13",
        include_str!("../../queries/lubm/q13.sparql"),
        Some(1),
    ),
    (
        "q14",
        include_str!("../../queries/lubm/q14.sparql"),
        Some(5916),
    ),
];

/// Returns specs for all 14 LUBM queries with LUBM(1, 0) reference
/// cardinalities. Pass `include_expected = false` if generating for a
/// scale ≠ 1 — the cardinalities do not generalise across scales.
pub fn lubm_query_specs(include_expected: bool) -> Vec<LubmQuerySpec> {
    QUERY_DATA
        .iter()
        .map(|(name, sparql, expected)| LubmQuerySpec {
            name: (*name).to_string(),
            sparql: (*sparql).to_string(),
            expected_cardinality: if include_expected {
                *expected
            } else {
                None
            },
        })
        .collect()
}

/// Returns just the names of the 14 LUBM queries in canonical order.
pub fn query_names() -> Vec<&'static str> { QUERY_DATA.iter().map(|(n, ..)| *n).collect() }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fourteen_queries_exposed() {
        assert_eq!(QUERY_DATA.len(), 14);
        let specs = lubm_query_specs(true);
        assert_eq!(specs.len(), 14);
        let names: Vec<_> = specs.iter().map(|s| s.name.clone()).collect();
        for n in 1..=14 {
            assert!(names.contains(&format!("q{n}")), "missing q{n}");
        }
    }

    #[test]
    fn each_query_starts_with_prefix_lines() {
        for spec in lubm_query_specs(false) {
            assert!(
                spec.sparql.starts_with("PREFIX rdf:"),
                "q{} should start with PREFIX rdf:, got: {}",
                spec.name,
                &spec.sparql[..40.min(spec.sparql.len())]
            );
            assert!(spec.sparql.contains("PREFIX ub:"));
            assert!(spec.sparql.contains("SELECT"));
            assert!(spec.sparql.contains("WHERE"));
        }
    }

    #[test]
    fn include_expected_false_strips_cardinalities() {
        let with = lubm_query_specs(true);
        let without = lubm_query_specs(false);
        assert!(with.iter().any(|s| s.expected_cardinality.is_some()));
        assert!(without.iter().all(|s| s.expected_cardinality.is_none()));
    }

    #[test]
    fn reference_cardinalities_match_paper_table_3() {
        // Spot-check a few known LUBM(1, 0) values from the paper.
        let specs = lubm_query_specs(true);
        let by_name: std::collections::HashMap<_, _> = specs
            .iter()
            .map(|s| (s.name.as_str(), s.expected_cardinality))
            .collect();
        assert_eq!(by_name["q1"], Some(4));
        assert_eq!(by_name["q2"], Some(0));
        assert_eq!(by_name["q6"], Some(7790));
        assert_eq!(by_name["q14"], Some(5916));
    }
}
