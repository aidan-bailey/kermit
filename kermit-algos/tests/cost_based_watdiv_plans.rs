//! The cost-based optimiser reproduces the plans #68 simulated for the
//! WatDiv stress templates it was built to fix (issue #81).
//!
//! #68 counted, exactly, the bindings LFTJ makes under any plan of each
//! `watdiv-stress-100-test-1` template, and scored a Python prototype of
//! this optimiser (`plan_est_dp` in its `planners.py`) with those counts.
//! The planner sees only statistics, never data, so the 61 relations'
//! tuple and distinct counts (`fixtures/watdiv-stress-100-stats.tsv`, #68's
//! `relstats.txt`) reproduce its decisions without the data. Each `stored`
//! test pins the prototype's plan; its exact cost, from #68's counter, is
//! in its doc comment.
//!
//! #68's prototype planned under `stored` only. Under `--column-orders
//! any` (issue #93) the tests pin the plans these statistics give for
//! templates `stored` leaves above 3.7e7 bindings, and check that every
//! committed query of the benchmark fits the search budget.

use {
    kermit_algos::{
        analyse, orient, rewrite_atoms, rewrite_placeholders, rewrite_repeated_variables,
        CatalogStats, ColumnOrderPolicy, CostBasedOptimiser, JoinQuery, QueryOptimiser,
        RelationStats, SelectionSpec,
    },
    kermit_parser::Term,
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

/// The query as `kermit::db` hands it to the planner: after the const,
/// placeholder and selection rewrites, with the selection views those
/// leave for `orient`.
fn rewritten_with_selections(text: &str) -> (JoinQuery, Vec<SelectionSpec>) {
    let (query, _) = rewrite_atoms(text.parse().expect("parse")).expect("const rewrite");
    rewrite_repeated_variables(rewrite_placeholders(query))
}

/// The query as `kermit::db` hands it to the planner.
fn rewritten(text: &str) -> JoinQuery { rewritten_with_selections(text).0 }

/// The fixture's statistics for `query`'s relations, under `policy`.
fn catalog(query: &JoinQuery, policy: ColumnOrderPolicy) -> CatalogStats {
    let table: HashMap<&str, Vec<usize>> = STATS
        .lines()
        .filter(|line| !line.starts_with('#') && !line.is_empty())
        .map(|line| {
            let mut fields = line.split('\t');
            let name = fields.next().expect("relation name");
            (name, fields.map(|f| f.parse().expect("a count")).collect())
        })
        .collect();
    CatalogStats::for_query(query, policy, |name| {
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

/// The cost-based plan for `text` under `--column-orders stored`, by
/// variable name.
fn plan(text: &str) -> Vec<String> { plan_under(text, ColumnOrderPolicy::Stored).0 }

/// The cost-based plan for `text` under `policy`, by variable name, and
/// the copies it reads (`orient`'s `Index_<π>_<base>` names; none under
/// `stored`).
fn plan_under(text: &str, policy: ColumnOrderPolicy) -> (Vec<String>, Vec<String>) {
    let (query, selections) = rewritten_with_selections(text);
    let names = canonical_names(&query);
    let plan = CostBasedOptimiser::default().plan(&query, &catalog(&query, policy));
    let order = plan
        .variable_ordering
        .iter()
        .map(|&v| names[v].clone())
        .collect();
    let copies = orient(policy, query, plan, selections)
        .index_specs
        .into_iter()
        .map(|spec| spec.name)
        .collect();
    (order, copies)
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

/// Under `stored`, `homepage(V0, c382480)` must bind `V0` before the
/// constant, so no plan can start from it. Under `any` this plan does, and
/// reads `homepage` reversed, then `includes` and `purchasefor` from `V0`,
/// their object. With q0085 and q0008 below, one of the 10 templates
/// `stored` leaves above 3.7e7 bindings and `any` answers in under 1 s
/// (`docs/optimisers/cost-based.md`).
#[test]
fn q0264_under_any_binds_the_constant_first() {
    let (order, copies) = plan_under(Q0264, ColumnOrderPolicy::Any);
    assert_eq!(order, ["K0", "V0", "V3", "V4", "V5", "V6", "V2", "V7"]);
    assert_eq!(copies, [
        "Index_1_0_homepage",
        "Index_1_0_includes",
        "Index_1_0_purchasefor"
    ]);
}

/// From `eligibleregion`'s constant, then the 9 and 70 objects of
/// `eligiblequantity` and `pricevaliduntil` before their subject `V0`:
/// five relations read reversed.
#[test]
#[cfg_attr(
    miri,
    ignore = "an 8,191-set search, minutes under Miri; q0264 runs the `any` path there"
)]
fn q0085_under_any_binds_the_constant_first() {
    let (order, copies) = plan_under(Q0085, ColumnOrderPolicy::Any);
    assert_eq!(order, [
        "K0", "V8", "V11", "V0", "V1", "V3", "V4", "V5", "V6", "V10", "V2", "V12", "V7"
    ]);
    assert_eq!(copies, [
        "Index_1_0_eligibleregion",
        "Index_1_0_eligiblequantity",
        "Index_1_0_pricevaliduntil",
        "Index_1_0_offers",
        "Index_1_0_purchasefor"
    ]);
}

/// Under `stored`, the documented regression above, at 10.9 s on
/// `tree-trie`. Under `any`, from `eligibleregion`'s constant and the 13
/// objects of `openinghours`, before their subjects: three relations read
/// reversed.
#[test]
#[cfg_attr(
    miri,
    ignore = "a 4,095-set search, minutes under Miri; q0264 runs the `any` path there"
)]
fn q0008_under_any_binds_the_constant_first() {
    let (order, copies) = plan_under(Q0008, ColumnOrderPolicy::Any);
    assert_eq!(order, [
        "K0", "V11", "V0", "V8", "V1", "V10", "V2", "V3", "V4", "V5", "V6", "V7"
    ]);
    assert_eq!(copies, [
        "Index_1_0_eligibleregion",
        "Index_1_0_openinghours",
        "Index_1_0_reviewer"
    ]);
}

/// Under `any` the search reaches every subset of the variables, 2ⁿ − 1
/// sets. q0034 is the largest template, 13 variables counting the
/// constant's: 8,191 sets fit the default budget, so its plan is the
/// unbounded search's, not `cardinality`'s fallback.
#[test]
#[cfg_attr(
    miri,
    ignore = "two 8,191-set searches, minutes under Miri; q0264 runs the `any` path there"
)]
fn q0034_under_any_fits_the_budget() {
    let query = rewritten(Q0034);
    assert_eq!(analyse(&query).num_vars, 13);
    let stats = catalog(&query, ColumnOrderPolicy::Any);
    let unbounded = CostBasedOptimiser {
        state_budget: usize::MAX,
    };
    assert_eq!(
        CostBasedOptimiser::default().plan(&query, &stats),
        unbounded.plan(&query, &stats)
    );
}

/// Every query of the committed `watdiv-stress-100-test-1` benchmark, not
/// only the templates above, fits the default budget under `any`: the
/// search reaches 2ⁿ − 1 sets there, which fits for n ≤ 14. The largest
/// has 13 variables, counting each constant's, as q0034 does.
#[test]
#[cfg_attr(
    miri,
    ignore = "parses a 2.8 MB benchmark; safe Rust, so Miri adds nothing"
)]
fn every_committed_query_fits_the_budget_under_any() {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../benchmarks/watdiv-stress-100-test-1.yml");
    let text = std::fs::read_to_string(&path).expect("the committed benchmark");
    let benchmark: serde_yaml::Value = serde_yaml::from_str(&text).expect("benchmark YAML");
    let queries = benchmark["queries"].as_sequence().expect("a query list");
    assert_eq!(queries.len(), 12_400);
    let largest = queries
        .iter()
        .map(|q| analyse(&rewritten(q["query"].as_str().expect("query text"))).num_vars)
        .max()
        .expect("queries");
    assert_eq!(largest, 13);
    assert!((1 << largest) - 1 <= CostBasedOptimiser::DEFAULT_STATE_BUDGET);
}
