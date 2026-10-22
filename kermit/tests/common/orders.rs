//! A test-only optimiser that forces the variable order, so a test can run
//! one query under every order its variables admit.

use kermit_algos::{canonical_names, CatalogStats, JoinQuery, QueryOptimiser, QueryPlan};

/// Plans the order it holds: the listed variables first, in that order,
/// then any variable it does not list (such as the `K<n>` a rewritten `_`
/// becomes) in canonical order. It reads no statistics, so it needs only
/// the default [`StatisticsLevel`](kermit_algos::StatisticsLevel).
///
/// `plan` is handed the query the optimiser plans, which `prepare` builds
/// from the accesses, and finds each name through [`canonical_names`] of
/// that query. It panics on a name the query lacks, or lists twice: a
/// test that forces an order must name real variables.
#[derive(Clone, Debug)]
pub struct FixedOrder(pub Vec<String>);

impl QueryOptimiser for FixedOrder {
    fn plan(&self, query: &JoinQuery, _stats: &CatalogStats) -> QueryPlan {
        let names = canonical_names(query);
        let mut ordering: Vec<usize> = Vec::with_capacity(names.len());
        for wanted in &self.0 {
            let index = names
                .iter()
                .position(|name| name == wanted)
                .unwrap_or_else(|| panic!("FixedOrder: variable {wanted} is not in the query"));
            assert!(
                !ordering.contains(&index),
                "FixedOrder: variable {wanted} is listed twice"
            );
            ordering.push(index);
        }
        let unlisted: Vec<usize> = (0..names.len())
            .filter(|index| !ordering.contains(index))
            .collect();
        ordering.extend(unlisted);
        QueryPlan {
            variable_ordering: ordering,
        }
    }
}

/// Every permutation of `variables`, each exactly once (`n!` of them; the
/// empty slice has one, the empty order).
pub fn every_order(variables: &[String]) -> Vec<Vec<String>> {
    if variables.is_empty() {
        return vec![Vec::new()];
    }
    let mut orders = Vec::new();
    for (i, first) in variables.iter().enumerate() {
        let mut rest = variables.to_vec();
        rest.remove(i);
        for tail in every_order(&rest) {
            let mut order = vec![first.clone()];
            order.extend(tail);
            orders.push(order);
        }
    }
    orders
}
