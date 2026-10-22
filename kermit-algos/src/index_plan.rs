//! Index planning: from a planned query of accesses to the indexes to build.
//!
//! The optimisers plan over the access query of [`crate::extract_accesses`],
//! one atom per access (`@0`, `@1`, …), and produce a variable order. The
//! executors descend every atom's columns in that order, so each access must
//! be read through a copy whose columns are already in it. [`plan_indexes`]
//! computes those copies: an access's kept columns sorted by the plan
//! position of the variable each binds, together with the access's filter,
//! is an [`IndexSpec`]. Two accesses with the same spec share one index.
//!
//! The query it returns names its atoms by index id (`#0`, `#1`, …, see
//! [`INDEX_ID_PREFIX`]), with each atom's variables in index column order,
//! so atom and index agree by construction and [`QueryPlan::validate`]
//! passes. It reads no data.

use {
    crate::{
        access::AccessQuery,
        analysis::{analyse, canonical_names},
        optimiser::QueryPlan,
    },
    kermit_iters::{Filter, TupleView, Tuples},
    kermit_parser::{JoinQuery, Predicate, Term},
    std::collections::HashMap,
};

/// Prefix of the ids of the indexes in an executed query: `#0`, `#1`, …,
/// numbered by first appearance in the query. No relation name can collide
/// with it, since the parser admits `[A-Za-z][A-Za-z0-9_]*` only.
pub const INDEX_ID_PREFIX: &str = "#";

/// One index to build: a relation read through a filter, with the kept
/// source columns in plan order.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct IndexSpec {
    /// The relation the index is built from.
    pub relation: String,
    /// The constants and column equalities its rows satisfy.
    pub filter: Filter,
    /// The source columns it keeps, in plan order: column `i` of the index
    /// is column `columns[i]` of the relation.
    pub columns: Vec<usize>,
}

impl IndexSpec {
    /// This index as a view over `tuples`, the relation's flat rows.
    ///
    /// # Panics
    ///
    /// Panics if a column of the spec, or one its filter names, is not below
    /// `tuples.arity()` (see [`TupleView::new`]).
    pub fn view<'a>(&self, tuples: &'a Tuples) -> TupleView<'a> {
        TupleView::new(tuples, self.filter.clone(), self.columns.clone())
    }
}

/// A planned query, ready to build and execute.
#[derive(Clone, Debug)]
pub struct IndexPlan {
    /// The query to execute: the head unchanged, and `body[j]` the atom
    /// `#{id}` of the index the `j`-th access reads, over its variables in
    /// plan order.
    pub query: JoinQuery,
    /// The plan, translated to `query`'s variable numbering; it validates
    /// against `query`.
    pub plan: QueryPlan,
    /// The indexes to build, `indexes[id]`, with identical specs merged.
    pub indexes: Vec<IndexSpec>,
}

/// Plans the indexes of `accesses` under `plan`.
///
/// `plan` must have been made for `accesses.query`. Accesses with equal
/// [`IndexSpec`]s share one index, numbered by first appearance. Checks (the
/// atoms with no kept column) need no index and are not read here.
///
/// # Panics
///
/// Panics if `plan` is not a plan of `accesses.query`.
pub fn plan_indexes(accesses: &AccessQuery, plan: &QueryPlan) -> IndexPlan {
    let source = &accesses.query;
    let names = canonical_names(source);
    let plan_names: Vec<&str> = plan
        .variable_ordering
        .iter()
        .map(|&v| names[v].as_str())
        .collect();
    let position: HashMap<&str, usize> = plan_names
        .iter()
        .enumerate()
        .map(|(at, &name)| (name, at))
        .collect();

    let mut indexes: Vec<IndexSpec> = Vec::new();
    let mut body: Vec<Predicate> = Vec::with_capacity(accesses.accesses.len());
    for access in &accesses.accesses {
        // `order[j]` is the kept column that goes `j`-th: the kept columns
        // sorted by the plan position of the variable each binds. A
        // variable occurs once per access, so the positions are distinct.
        let mut order: Vec<usize> = (0..access.columns.len()).collect();
        order.sort_by_key(|&kept| position[access.variables[kept].as_str()]);
        let spec = IndexSpec {
            relation: access.relation.clone(),
            filter: access.filter.clone(),
            columns: order.iter().map(|&kept| access.columns[kept]).collect(),
        };
        let id = match indexes.iter().position(|existing| *existing == spec) {
            | Some(id) => id,
            | None => {
                indexes.push(spec);
                indexes.len() - 1
            },
        };
        body.push(Predicate {
            name: format!("{INDEX_ID_PREFIX}{id}"),
            terms: order
                .iter()
                .map(|&kept| Term::Var(access.variables[kept].clone()))
                .collect(),
        });
    }
    let query = JoinQuery {
        head: source.head.clone(),
        body,
    };

    // `analyse` numbers body-only variables by first appearance, and the
    // atoms above list their variables in plan order, so the numbers can
    // move: translate the plan by name.
    let renumbered = canonical_names(&query);
    let index_of: HashMap<&str, usize> = renumbered
        .iter()
        .enumerate()
        .map(|(at, name)| (name.as_str(), at))
        .collect();
    let plan = QueryPlan {
        variable_ordering: plan_names.iter().map(|name| index_of[name]).collect(),
    };
    debug_assert_eq!(
        plan.validate(&analyse(&query)),
        Ok(()),
        "every atom lists its variables in plan order"
    );

    IndexPlan {
        query,
        plan,
        indexes,
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::{analyse, canonical_names, extract_accesses, rewrite_placeholders},
    };

    fn accesses_of(text: &str) -> AccessQuery {
        let query: JoinQuery = text.parse().expect("parse");
        extract_accesses(&rewrite_placeholders(query)).expect("extract")
    }

    /// `order` (variable names) as a plan of `query`.
    fn plan_of(query: &JoinQuery, order: &[&str]) -> QueryPlan {
        let names = canonical_names(query);
        QueryPlan {
            variable_ordering: order
                .iter()
                .map(|name| {
                    names
                        .iter()
                        .position(|n| n == name)
                        .expect("a query variable")
                })
                .collect(),
        }
    }

    fn planned(text: &str, order: &[&str]) -> IndexPlan {
        let accesses = accesses_of(text);
        let plan = plan_of(&accesses.query, order);
        plan_indexes(&accesses, &plan)
    }

    fn body_text(query: &JoinQuery) -> Vec<String> {
        query.body.iter().map(ToString::to_string).collect()
    }

    fn spec(relation: &str, filter: Filter, columns: Vec<usize>) -> IndexSpec {
        IndexSpec {
            relation: relation.to_string(),
            filter,
            columns,
        }
    }

    #[test]
    fn view_reads_the_filtered_columns_in_spec_order() {
        let tuples = Tuples::from_flat(3, 3, vec![1, 2, 5, 3, 4, 5, 7, 8, 6]);
        let index = spec("r", Filter::new(vec![(2, 5)], vec![]), vec![1, 0]);
        let view = index.view(&tuples);
        let mut rows: Vec<Vec<usize>> = Vec::new();
        view.for_each(|row| rows.push(row.to_vec()));
        assert_eq!(rows, vec![vec![2, 1], vec![4, 3]]);
        assert_eq!(view.count(), 2);
    }

    #[test]
    fn atoms_that_agree_with_the_plan_keep_their_column_order() {
        let out = planned("Q(X, Y, Z) :- R(X, Y), S(Y, Z), T(X, Z).", &["X", "Y", "Z"]);
        assert_eq!(out.indexes, vec![
            spec("R", Filter::none(), vec![0, 1]),
            spec("S", Filter::none(), vec![0, 1]),
            spec("T", Filter::none(), vec![0, 1]),
        ]);
        assert_eq!(body_text(&out.query), vec![
            "#0(X, Y)", "#1(Y, Z)", "#2(X, Z)"
        ]);
        assert_eq!(out.plan.variable_ordering, vec![0, 1, 2]);
    }

    #[test]
    fn a_reversed_order_reads_the_columns_reversed() {
        let out = planned("Q(X, Y) :- edge(Y, X).", &["X", "Y"]);
        assert_eq!(out.indexes, vec![spec("edge", Filter::none(), vec![1, 0])]);
        assert_eq!(body_text(&out.query), vec!["#0(X, Y)"]);
        assert_eq!(out.plan.variable_ordering, vec![0, 1]);
    }

    #[test]
    fn the_filter_is_carried_and_the_columns_are_source_columns() {
        // `r(X, c5, Y)` keeps columns 0 and 2; binding Y first reads them as
        // (2, 0).
        let out = planned("Q(Y, X) :- r(X, c5, Y).", &["Y", "X"]);
        assert_eq!(out.indexes, vec![spec(
            "r",
            Filter::new(vec![(1, 5)], vec![]),
            vec![2, 0]
        )]);
        assert_eq!(body_text(&out.query), vec!["#0(Y, X)"]);

        // The equality names source columns, so it does not move with the
        // order.
        let out = planned("Q(X, Y) :- r(Y, X, Y).", &["X", "Y"]);
        assert_eq!(out.indexes, vec![spec(
            "r",
            Filter::new(vec![], vec![(0, 2)]),
            vec![1, 0]
        )]);
        assert_eq!(body_text(&out.query), vec!["#0(X, Y)"]);
    }

    #[test]
    fn the_head_is_unchanged() {
        let accesses = accesses_of("Q(Z, X) :- R(X, Y), S(Y, Z).");
        let plan = plan_of(&accesses.query, &["Z", "Y", "X"]);
        let out = plan_indexes(&accesses, &plan);
        assert_eq!(out.query.head, accesses.query.head);
    }

    #[test]
    fn checks_are_not_indexed() {
        let out = planned("Q(X) :- p(X), r(c1, c2).", &["X"]);
        assert_eq!(out.indexes, vec![spec("p", Filter::none(), vec![0])]);
        assert_eq!(body_text(&out.query), vec!["#0(X)"]);
    }

    /// A body of only constants has no access: `extract_accesses` returns an
    /// `AccessQuery` with no accesses, and `plan_indexes` must answer with no
    /// index, an empty body and the empty plan, not panic. The parser needs
    /// a head term, so the head is emptied to give the query no variable.
    #[test]
    fn a_query_with_no_accesses_plans_no_indexes() {
        let mut query: JoinQuery = "Q(c9) :- r(c1, c2), s(c3).".parse().expect("parse");
        query.head.terms.clear();
        let accesses = extract_accesses(&rewrite_placeholders(query)).expect("extract");
        assert!(accesses.accesses.is_empty());
        assert_eq!(accesses.checks.len(), 2);

        let plan = QueryPlan {
            variable_ordering: vec![],
        };
        let out = plan_indexes(&accesses, &plan);
        assert!(out.indexes.is_empty());
        assert!(out.query.body.is_empty());
        assert_eq!(out.query.head, accesses.query.head);
        assert!(out.plan.variable_ordering.is_empty());
        assert_eq!(out.plan.validate(&analyse(&out.query)), Ok(()));
    }

    /// `analyse` numbers body-only variables by first appearance, and the
    /// rebuilt atom lists them in plan order, so the numbers move; the plan
    /// follows the names, not the numbers.
    #[test]
    fn the_plan_is_translated_by_variable_name() {
        let accesses = accesses_of("Q(X) :- r(B, X, A).");
        // Numbering of the access query: X = 0, B = 1, A = 2.
        let old = plan_of(&accesses.query, &["X", "A", "B"]);
        assert_eq!(old.variable_ordering, vec![0, 2, 1]);

        let out = plan_indexes(&accesses, &old);
        assert_eq!(out.indexes, vec![spec("r", Filter::none(), vec![1, 2, 0])]);
        assert_eq!(body_text(&out.query), vec!["#0(X, A, B)"]);
        // Numbering of the executed query: X = 0, A = 1, B = 2.
        assert_eq!(out.plan.variable_ordering, vec![0, 1, 2]);
        assert_eq!(out.plan.validate(&analyse(&out.query)), Ok(()));
        // The untranslated plan would not have validated.
        assert!(old.validate(&analyse(&out.query)).is_err());
    }

    #[test]
    fn every_order_of_the_triangle_yields_a_valid_plan_with_the_same_order() {
        let orders: [[&str; 3]; 6] = [
            ["X", "Y", "Z"],
            ["X", "Z", "Y"],
            ["Y", "X", "Z"],
            ["Y", "Z", "X"],
            ["Z", "X", "Y"],
            ["Z", "Y", "X"],
        ];
        for order in orders {
            let out = planned("Q(X, Y, Z) :- R(X, Y), S(Y, Z), T(X, Z).", &order);
            assert_eq!(out.plan.validate(&analyse(&out.query)), Ok(()), "{order:?}");
            let names = canonical_names(&out.query);
            let bound: Vec<&str> = out
                .plan
                .variable_ordering
                .iter()
                .map(|&v| names[v].as_str())
                .collect();
            assert_eq!(bound, order.to_vec());
        }
    }

    #[test]
    fn a_self_join_that_reads_one_relation_one_way_builds_it_once() {
        let out = planned("Q(X, Y, Z) :- edge(Y, X), edge(Z, X).", &["X", "Y", "Z"]);
        assert_eq!(out.indexes, vec![spec("edge", Filter::none(), vec![1, 0])]);
        assert_eq!(body_text(&out.query), vec!["#0(X, Y)", "#0(X, Z)"]);
        assert_eq!(out.plan.validate(&analyse(&out.query)), Ok(()));
    }

    #[test]
    fn a_self_join_that_reads_both_ways_builds_two_indexes() {
        let out = planned("Q(X, Y, Z) :- edge(X, Y), edge(Z, X).", &["X", "Y", "Z"]);
        assert_eq!(out.indexes, vec![
            spec("edge", Filter::none(), vec![0, 1]),
            spec("edge", Filter::none(), vec![1, 0]),
        ]);
        assert_eq!(body_text(&out.query), vec!["#0(X, Y)", "#1(X, Z)"]);
    }

    #[test]
    fn index_ids_are_numbered_by_first_appearance() {
        let out = planned("Q(X, Y) :- edge(Y, X), edge(X, Y), edge(Y, X).", &[
            "X", "Y",
        ]);
        assert_eq!(out.indexes, vec![
            spec("edge", Filter::none(), vec![1, 0]),
            spec("edge", Filter::none(), vec![0, 1]),
        ]);
        assert_eq!(body_text(&out.query), vec![
            "#0(X, Y)", "#1(X, Y)", "#0(X, Y)"
        ]);
    }

    #[test]
    fn the_same_filter_and_columns_merge_whatever_the_variables() {
        let out = planned("Q(X, Y) :- p(X, c1), p(Y, c1).", &["X", "Y"]);
        assert_eq!(out.indexes, vec![spec(
            "p",
            Filter::new(vec![(1, 1)], vec![]),
            vec![0]
        )]);
        assert_eq!(body_text(&out.query), vec!["#0(X)", "#0(Y)"]);
    }

    #[test]
    fn different_filters_do_not_merge() {
        let out = planned("Q(X) :- p(X, c1), p(X, c2).", &["X"]);
        assert_eq!(out.indexes, vec![
            spec("p", Filter::new(vec![(1, 1)], vec![]), vec![0]),
            spec("p", Filter::new(vec![(1, 2)], vec![]), vec![0]),
        ]);
        assert_eq!(body_text(&out.query), vec!["#0(X)", "#1(X)"]);
    }
}
