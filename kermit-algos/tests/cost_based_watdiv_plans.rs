//! The cost-based optimiser reproduces the plans #68 simulated for the
//! WatDiv stress templates it was built to fix (issue #81).
//!
//! #68 counted, exactly, the bindings LFTJ makes under any plan of each
//! `watdiv-stress-100-test-1` template, and scored a Python prototype of
//! this optimiser (`plan_est_dp` in its `planners.py`) with those counts.
//! The planner sees only statistics, never data, so the 61 relations'
//! tuple and distinct counts (`fixtures/watdiv-stress-100-stats.tsv`, #68's
//! `relstats.txt`) reproduce its decisions without the data. Each test pins
//! the prototype's plan; its exact cost, from #68's counter, is in its doc
//! comment.

use {
    kermit_algos::{
        rewrite_atoms, rewrite_placeholders, rewrite_repeated_variables, CatalogStats,
        ColumnOrderPolicy, CostBasedOptimiser, JoinQuery, QueryOptimiser, RelationStats,
    },
    kermit_parser::Term,
    std::collections::HashMap,
};

/// `relation<TAB>tuples<TAB>distinct subjects<TAB>distinct objects`.
const STATS: &str = include_str!("fixtures/watdiv-stress-100-stats.tsv");

const Q0020: &str = "Q_test_1_q0020(V3, V2, V1, V4, V5, V0, V6) :- purchasefor(c450066, V3), \
                     includes(V2, V3), eligibleregion(V2, V1), validthrough(V2, V4), \
                     eligiblequantity(V2, V5), parentcountry(V0, V1), nationality(V6, V1).";

const Q0034: &str = "Q_test_1_q0034(V0, V4, V1, V9, V10, V11, V12, V5, V6, V7, V8, V2) :- \
                     type(V0, c206066), hasreview(V0, V4), tag(V0, V1), contentsize(V0, V9), \
                     description(V0, V10), keywords(V0, V11), purchasefor(V12, V0), rating(V4, \
                     V5), reviewer(V4, V6), text_601771(V4, V7), title_601766(V4, V8), tag(V2, \
                     V1).";

const Q0008: &str = "Q_test_1_q0008(V2, V0, V3, V4, V5, V8, V1, V10, V11, V6, V7) :- \
                     eligibleregion(V2, c17), offers(V0, V2), price(V2, V3), validfrom(V2, V4), \
                     validthrough(V2, V5), contactpoint(V0, V8), name(V0, V1), email(V0, V10), \
                     openinghours(V0, V11), reviewer(V6, V8), rating(V6, V7).";

/// The query as `kermit::db` hands it to the planner: after the const,
/// placeholder and selection rewrites.
fn rewritten(text: &str) -> JoinQuery {
    let (query, _) = rewrite_atoms(text.parse().expect("parse")).expect("const rewrite");
    rewrite_repeated_variables(rewrite_placeholders(query)).0
}

/// The fixture's statistics for `query`'s relations.
fn catalog(query: &JoinQuery) -> CatalogStats {
    let table: HashMap<&str, Vec<usize>> = STATS
        .lines()
        .filter(|line| !line.starts_with('#') && !line.is_empty())
        .map(|line| {
            let mut fields = line.split('\t');
            let name = fields.next().expect("relation name");
            (name, fields.map(|f| f.parse().expect("a count")).collect())
        })
        .collect();
    CatalogStats::for_query(query, ColumnOrderPolicy::Stored, |name| {
        let counts = table.get(name)?;
        Some(RelationStats::new(counts[0], 2).with_column_distinct(counts[1..].to_vec()))
    })
}

/// `query`'s variables in canonical order (`kermit_algos::analyse`): the
/// head's, then the body's in order of first appearance.
fn canonical_names(query: &JoinQuery) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    let terms = query
        .head
        .terms
        .iter()
        .chain(query.body.iter().flat_map(|atom| &atom.terms));
    for term in terms {
        if let Term::Var(name) = term {
            if !names.contains(name) {
                names.push(name.clone());
            }
        }
    }
    names
}

/// The cost-based plan for `text`, by variable name.
fn plan(text: &str) -> Vec<String> {
    let query = rewritten(text);
    let names = canonical_names(&query);
    CostBasedOptimiser::default()
        .plan(&query, &catalog(&query))
        .variable_ordering
        .into_iter()
        .map(|v| names[v].clone())
        .collect()
}

/// Both current optimisers bind the small `parentcountry` and
/// `nationality` subjects before the constant and cost 9.68e10 bindings.
/// This plan binds the constant's offer first and costs 1.81e4 (#68).
#[test]
fn q0020_binds_the_constants_side_first() {
    assert_eq!(plan(Q0020), [
        "K0", "V2", "V3", "V4", "V5", "V0", "V6", "V1"
    ]);
}

/// `cardinality` binds `tag`'s 15,080 objects first and costs 2.27e9
/// bindings, 187x `lexicographic`. This plan costs 9.67e5 (#68).
#[test]
fn q0034_avoids_the_tag_cross_product() {
    assert_eq!(plan(Q0034), [
        "V12", "V0", "K0", "V9", "V10", "V11", "V4", "V5", "V6", "V7", "V8", "V2", "V1"
    ]);
}

/// The documented regression: 8.85e7 bindings, against `cardinality`'s
/// 3.81e7 (2.32x; #68). Pinned so that a change to it is deliberate.
#[test]
fn q0008_keeps_its_documented_regression() {
    assert_eq!(plan(Q0008), [
        "V0", "V1", "V10", "V11", "V6", "V8", "V7", "V2", "K0", "V3", "V4", "V5"
    ]);
}
