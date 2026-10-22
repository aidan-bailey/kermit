//! The cost-based optimiser plans the WatDiv stress templates #68 simulated
//! (issue #81), over the accesses `kermit::db::prepare` plans over.
//!
//! #68 counted, exactly, the bindings LFTJ makes under any plan of each
//! `watdiv-stress-100-test-1` template, and scored a Python prototype of
//! this optimiser (`plan_est_dp` in its `planners.py`) with those counts.
//! The planner sees only statistics, never data, so the 61 relations'
//! tuple and distinct counts (`fixtures/watdiv-stress-100-stats.tsv`, #68's
//! `relstats.txt`) reproduce its decisions without the data.
//!
//! A query is planned as `prepare` plans it: each `_` becomes a variable,
//! then each body atom becomes an access (`extract_accesses`), whose
//! constants are a filter and not a variable, and the statistics are keyed
//! by access id. `prepare` scans the data for the exact counts of a
//! filtered access. The statistics fixture has no per-constant counts, so
//! this test estimates them: a filtered access's rows are the relation's
//! rows divided by the filtered column's distinct count (never below one
//! row), and a kept column has no more distinct values than rows.
//!
//! The pins are the plans these statistics give, searched freely: no column
//! order constrains a plan. Four of them are the plans this file pinned
//! under `--column-orders any` (issue #93) without the variable the old
//! rewrite gave each constant; q0034's differs only in binding `V9` before
//! `V0`. Every committed query of the benchmark fits the search budget.

use {
    kermit_algos::{
        analyse, canonical_names, extract_accesses, rewrite_placeholders, AccessQuery,
        CatalogStats, CostBasedOptimiser, QueryOptimiser, RelationStats, ACCESS_PREFIX,
    },
    std::{collections::HashMap, path::Path},
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

const Q0264: &str = "Q_test_1_q0264(V0, V6, V3, V4, V5, V2, V7) :- homepage(V0, c382480), \
                     hasgenre(V0, V6), title(V0, V3), keywords(V0, V4), text(V0, V5), \
                     includes(V2, V0), purchasefor(V7, V0).";

const Q0085: &str = "Q_test_1_q0085(V0, V1, V3, V4, V5, V6, V8, V11, V2, V7, V10, V12) :- \
                     eligibleregion(V0, c86), includes(V0, V1), price(V0, V3), serialnumber(V0, \
                     V4), validfrom(V0, V5), validthrough(V0, V6), eligiblequantity(V0, V8), \
                     pricevaliduntil(V0, V11), offers(V2, V0), tag(V1, V7), keywords(V1, V10), \
                     purchasefor(V12, V1).";

/// The query as `prepare` hands it to the planner: placeholders rewritten,
/// then one access per body atom.
fn accesses(text: &str) -> AccessQuery {
    let query = rewrite_placeholders(text.parse().expect("parse"));
    extract_accesses(&query).expect("constants are c<digits>")
}

/// The fixture's statistics for `access_query`'s accesses, keyed by access
/// id. An access reads its relation's counts for the columns it keeps; a
/// constant filter divides the rows by the filtered column's distinct count,
/// and no kept column has more distinct values than rows.
fn catalog(access_query: &AccessQuery) -> CatalogStats {
    let table: HashMap<&str, Vec<usize>> = STATS
        .lines()
        .filter(|line| !line.starts_with('#') && !line.is_empty())
        .map(|line| {
            let mut fields = line.split('\t');
            let name = fields.next().expect("relation name");
            (name, fields.map(|f| f.parse().expect("a count")).collect())
        })
        .collect();
    CatalogStats::for_query(&access_query.query, |id| {
        let position = id.strip_prefix(ACCESS_PREFIX)?.parse::<usize>().ok()?;
        let access = access_query.accesses.get(position)?;
        let counts = table.get(access.relation.as_str())?;
        let (mut rows, distinct) = (counts[0], &counts[1..]);
        assert!(
            access.filter.equalities().is_empty(),
            "no template repeats a variable inside an atom"
        );
        for &(column, _) in access.filter.constants() {
            rows = (rows / distinct[column]).max(1);
        }
        let kept: Vec<usize> = access
            .columns
            .iter()
            .map(|&column| distinct[column].min(rows))
            .collect();
        Some(RelationStats::new(rows, kept.len()).with_column_distinct(kept))
    })
}

/// The cost-based plan for `text`, by variable name.
fn plan(text: &str) -> Vec<String> {
    let access_query = accesses(text);
    let names = canonical_names(&access_query.query);
    let stats = catalog(&access_query);
    let plan = CostBasedOptimiser::default().plan(&access_query.query, &stats);
    plan.variable_ordering
        .iter()
        .map(|&v| names[v].clone())
        .collect()
}

/// The constant's offer side first: `purchasefor`'s object V3, then
/// `includes`' subject V2 and V2's own properties (V4, V5, V1).
/// `parentcountry` and `nationality`, which share V1, bind last (V0, V6).
#[test]
fn q0020_binds_the_constants_side_first() {
    assert_eq!(plan(Q0020), ["V3", "V2", "V4", "V5", "V1", "V0", "V6"]);
}

/// `tag(V0, V1)` and `tag(V2, V1)` meet only through V1, so binding V2 is
/// a cross product over `tag`'s subjects. It binds last.
#[test]
#[cfg_attr(
    miri,
    ignore = "a 4,095-set search, minutes under Miri; q0264 runs the path there"
)]
fn q0034_binds_the_tag_cross_product_last() {
    assert_eq!(plan(Q0034), [
        "V9", "V0", "V10", "V11", "V4", "V5", "V6", "V7", "V8", "V12", "V1", "V2"
    ]);
}

/// From the 13 objects of `openinghours`, before their subjects: the
/// `contactpoint`, `name` and `email` properties of V0 follow it, and the
/// constant's offers (V2, V3, V4, V5) come after, with `reviewer` and
/// `rating` (V6, V7) last.
#[test]
#[cfg_attr(
    miri,
    ignore = "a 2,047-set search, minutes under Miri; q0264 runs the path there"
)]
fn q0008_binds_the_openinghours_objects_first() {
    assert_eq!(plan(Q0008), [
        "V11", "V0", "V8", "V1", "V10", "V2", "V3", "V4", "V5", "V6", "V7"
    ]);
}

/// The constant filters `homepage`, so its subject V0 binds first, then
/// V0's properties, and last `includes` and `purchasefor`, from their
/// object V0 (V2 and V7).
#[test]
fn q0264_binds_the_filtered_subject_first() {
    assert_eq!(plan(Q0264), ["V0", "V3", "V4", "V5", "V6", "V2", "V7"]);
}

/// From the 9 and 70 objects of `eligiblequantity` and `pricevaliduntil`
/// before their subject V0.
#[test]
#[cfg_attr(
    miri,
    ignore = "a 4,095-set search, minutes under Miri; q0264 runs the path there"
)]
fn q0085_binds_the_small_object_sets_first() {
    assert_eq!(plan(Q0085), [
        "V8", "V11", "V0", "V1", "V3", "V4", "V5", "V6", "V10", "V2", "V12", "V7"
    ]);
}

/// The search reaches every subset of the variables, 2ⁿ − 1 sets. q0034 is
/// the largest template: 12 variables, since its constant is a filter and
/// no longer a variable. 4,095 sets fit the default budget, so its plan is
/// the unbounded search's, not `cardinality`'s fallback.
#[test]
#[cfg_attr(
    miri,
    ignore = "two 4,095-set searches, minutes under Miri; q0264 runs the path there"
)]
fn q0034_fits_the_budget() {
    let access_query = accesses(Q0034);
    assert_eq!(analyse(&access_query.query).num_vars, 12);
    let stats = catalog(&access_query);
    let unbounded = CostBasedOptimiser {
        state_budget: usize::MAX,
    };
    assert_eq!(
        CostBasedOptimiser::default().plan(&access_query.query, &stats),
        unbounded.plan(&access_query.query, &stats)
    );
}

/// Every query of the committed `watdiv-stress-100-test-1` benchmark, not
/// only the templates above, fits the default budget: the search reaches
/// 2ⁿ − 1 sets, which fits for n ≤ 14. The largest has 12 variables.
#[test]
#[cfg_attr(
    miri,
    ignore = "parses a 2.8 MB benchmark; safe Rust, so Miri adds nothing"
)]
fn every_committed_query_fits_the_budget() {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../benchmarks/watdiv-stress-100-test-1.yml");
    let text = std::fs::read_to_string(&path).expect("the committed benchmark");
    let benchmark: serde_yaml::Value = serde_yaml::from_str(&text).expect("benchmark YAML");
    let queries = benchmark["queries"].as_sequence().expect("a query list");
    assert_eq!(queries.len(), 12_400);
    let largest = queries
        .iter()
        .map(|q| analyse(&accesses(q["query"].as_str().expect("query text")).query).num_vars)
        .max()
        .expect("queries");
    assert_eq!(largest, 12);
    assert!((1 << largest) - 1 <= CostBasedOptimiser::DEFAULT_STATE_BUDGET);
}
