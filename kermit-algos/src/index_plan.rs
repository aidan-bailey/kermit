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
        join_order::join_order,
        optimiser::QueryPlan,
    },
    kermit_iters::{Filter, TrieLevels, TupleView, Tuples},
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
    /// How many of `columns` the index's trie has levels for, the rest
    /// being payload kept in its leaf rows (#116): `columns.len()` under
    /// [`TrieLevels::All`], fewer under [`TrieLevels::JoinKeys`], and under
    /// [`TrieLevels::ProbeKeys`] the access's key length, the columns an
    /// earlier atom of the join order binds (`0` for the driver).
    /// Identical specs merge, so two specs that differ only here are two
    /// indexes.
    pub levels: usize,
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
    /// plan order. Under [`TrieLevels::ProbeKeys`] the body is in join
    /// order instead: `body[j]` reads the `j`-th atom of the join order,
    /// and `body[0]` is the driver.
    pub query: JoinQuery,
    /// The plan, translated to `query`'s variable numbering; it validates
    /// against `query`. Under [`TrieLevels::ProbeKeys`] it is the reordered
    /// plan `V′` of the join order, the order the joins bind in.
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
/// `levels` decides each spec's level count: every column under
/// [`TrieLevels::All`]; under [`TrieLevels::JoinKeys`], up to the last column
/// whose variable is a join key of `accesses.query` (rule (iii) of the #116
/// design), or none when no column is. On a payload-last plan that is the
/// number of join-key columns, since an index's columns follow the plan.
///
/// Under [`TrieLevels::ProbeKeys`] the plan is first read as a left-deep
/// join order (`join_order`): the columns follow the plan `V′` it returns,
/// each spec's level count is the access's key length (the variables an
/// earlier atom of the join order binds, `0` for the driver), and the
/// accesses are visited in join order, so the spec ids and the executed
/// body follow it, `#0` being the driver. The returned plan is `V′`.
///
/// # Panics
///
/// Panics if `plan` is not a plan of `accesses.query`.
pub fn plan_indexes(accesses: &AccessQuery, plan: &QueryPlan, levels: TrieLevels) -> IndexPlan {
    let source = &accesses.query;
    let names = canonical_names(source);
    // Under `ProbeKeys` the plan is read as a join order: the columns follow
    // its `V′`, the accesses are visited in join order, and each index
    // levels its probe key.
    let join = matches!(levels, TrieLevels::ProbeKeys).then(|| join_order(source, plan));
    let ordering = join
        .as_ref()
        .map_or(&plan.variable_ordering, |join| &join.plan.variable_ordering);
    let visit: Vec<usize> = match &join {
        | Some(join) => join.atoms.clone(),
        | None => (0..accesses.accesses.len()).collect(),
    };
    let plan_names: Vec<&str> = ordering.iter().map(|&v| names[v].as_str()).collect();
    let position: HashMap<&str, usize> = plan_names
        .iter()
        .enumerate()
        .map(|(at, &name)| (name, at))
        .collect();
    let analysis = analyse(source);
    let is_join_key: HashMap<&str, bool> = names
        .iter()
        .enumerate()
        .map(|(v, name)| (name.as_str(), analysis.join_keys[v]))
        .collect();

    let mut indexes: Vec<IndexSpec> = Vec::new();
    let mut body: Vec<Predicate> = Vec::with_capacity(accesses.accesses.len());
    for (rank, &at) in visit.iter().enumerate() {
        let access = &accesses.accesses[at];
        // `order[j]` is the kept column that goes `j`-th: the kept columns
        // sorted by the plan position of the variable each binds. A
        // variable occurs once per access, so the positions are distinct.
        let mut order: Vec<usize> = (0..access.columns.len()).collect();
        order.sort_by_key(|&kept| position[access.variables[kept].as_str()]);
        let count = match levels {
            | TrieLevels::All => order.len(),
            | TrieLevels::JoinKeys => order
                .iter()
                .rposition(|&kept| is_join_key[access.variables[kept].as_str()])
                .map_or(0, |last| last + 1),
            | TrieLevels::ProbeKeys => {
                join.as_ref()
                    .expect("ProbeKeys computes the join order")
                    .key_lens[rank]
            },
        };
        let spec = IndexSpec {
            relation: access.relation.clone(),
            filter: access.filter.clone(),
            columns: order.iter().map(|&kept| access.columns[kept]).collect(),
            levels: count,
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

    fn planned_with(text: &str, order: &[&str], levels: TrieLevels) -> IndexPlan {
        let accesses = accesses_of(text);
        let plan = plan_of(&accesses.query, order);
        plan_indexes(&accesses, &plan, levels)
    }

    fn planned(text: &str, order: &[&str]) -> IndexPlan {
        planned_with(text, order, TrieLevels::All)
    }

    fn body_text(query: &JoinQuery) -> Vec<String> {
        query.body.iter().map(ToString::to_string).collect()
    }

    fn spec(relation: &str, filter: Filter, columns: Vec<usize>) -> IndexSpec {
        let levels = columns.len();
        IndexSpec {
            relation: relation.to_string(),
            filter,
            columns,
            levels,
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
        let out = plan_indexes(&accesses, &plan, TrieLevels::All);
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
        let out = plan_indexes(&accesses, &plan, TrieLevels::All);
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

        let out = plan_indexes(&accesses, &old, TrieLevels::All);
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

    fn levels_of(plan: &IndexPlan) -> Vec<(Vec<usize>, usize)> {
        plan.indexes
            .iter()
            .map(|spec| (spec.columns.clone(), spec.levels))
            .collect()
    }

    const PAYLOAD_QUERY: &str = "Q(X, P, Y) :- R(X, P), S(X, Y), T(Y).";

    #[test]
    fn under_all_every_column_is_a_level() {
        let out = planned_with(PAYLOAD_QUERY, &["X", "Y", "P"], TrieLevels::All);
        assert_eq!(levels_of(&out), vec![
            (vec![0, 1], 2),
            (vec![0, 1], 2),
            (vec![0], 1)
        ]);
    }

    #[test]
    fn under_join_keys_a_payload_last_plan_levels_the_join_keys() {
        let out = planned_with(PAYLOAD_QUERY, &["X", "Y", "P"], TrieLevels::JoinKeys);
        assert_eq!(levels_of(&out), vec![
            (vec![0, 1], 1),
            (vec![0, 1], 2),
            (vec![0], 1)
        ]);
    }

    /// Rule (iii): a trie keeps levels up to its last join-key column, so a
    /// payload column before a join key stays a level.
    #[test]
    fn under_join_keys_payload_before_a_join_key_stays_a_level() {
        let out = planned_with(PAYLOAD_QUERY, &["P", "X", "Y"], TrieLevels::JoinKeys);
        // R is read P then X: its last join key is its second column.
        assert_eq!(levels_of(&out)[0], (vec![1, 0], 2));
    }

    #[test]
    fn under_join_keys_an_index_with_no_join_key_has_no_level() {
        let out = planned_with("Q(X, Y) :- R(X), S(Y).", &["X", "Y"], TrieLevels::JoinKeys);
        assert_eq!(levels_of(&out), vec![(vec![0], 0), (vec![0], 0)]);
    }

    #[test]
    fn under_join_keys_an_index_of_join_keys_keeps_every_level() {
        let out = planned_with(
            "Q(X, Y, Z) :- R(X, Y), S(Y, Z), T(X, Z).",
            &["X", "Y", "Z"],
            TrieLevels::JoinKeys,
        );
        assert!(out
            .indexes
            .iter()
            .all(|spec| spec.levels == spec.columns.len()));
    }

    /// SNAP's `path3` under lexicographic's payload-last plan B, C, A, D:
    /// `edge(B, C)` and `edge(C, D)` read `edge` the same way, but at 2 and 1
    /// levels, so only `all` shares them (exact sharing, the #116 design's
    /// Decision 5).
    #[test]
    fn path3_shares_an_index_under_all_but_not_under_join_keys() {
        let text = "P(A, B, C, D) :- edge(A, B), edge(B, C), edge(C, D).";
        let order = &["B", "C", "A", "D"];
        let all = planned_with(text, order, TrieLevels::All);
        assert_eq!(levels_of(&all), vec![(vec![1, 0], 2), (vec![0, 1], 2)]);
        assert_eq!(body_text(&all.query), vec![
            "#0(B, A)", "#1(B, C)", "#1(C, D)"
        ]);
        let keys = planned_with(text, order, TrieLevels::JoinKeys);
        assert_eq!(levels_of(&keys), vec![
            (vec![1, 0], 1),
            (vec![0, 1], 2),
            (vec![0, 1], 1)
        ]);
        assert_eq!(body_text(&keys.query), vec![
            "#0(B, A)", "#1(B, C)", "#2(C, D)"
        ]);
    }

    fn probe(text: &str, order: &[&str]) -> IndexPlan {
        planned_with(text, order, TrieLevels::ProbeKeys)
    }

    fn relations_of(plan: &IndexPlan) -> Vec<&str> {
        plan.indexes
            .iter()
            .map(|spec| spec.relation.as_str())
            .collect()
    }

    #[test]
    fn under_probe_keys_each_triangle_index_levels_its_key() {
        let out = probe("Q(X, Y, Z) :- R(X, Y), S(Y, Z), T(X, Z).", &["X", "Y", "Z"]);
        assert_eq!(relations_of(&out), vec!["R", "S", "T"]);
        assert_eq!(levels_of(&out), vec![
            (vec![0, 1], 0),
            (vec![0, 1], 1),
            (vec![0, 1], 2)
        ]);
        assert_eq!(body_text(&out.query), vec![
            "#0(X, Y)", "#1(Y, Z)", "#2(X, Z)"
        ]);
        assert_eq!(out.plan.variable_ordering, vec![0, 1, 2]);
        assert_eq!(out.plan.validate(&analyse(&out.query)), Ok(()));
    }

    /// The body is listed in join order, so the specs are numbered by it and
    /// `#0` is the driver: `T` (which carries `C` and has `B` bound) joins
    /// before `S`, and its columns follow `V′`.
    #[test]
    fn under_probe_keys_the_body_is_in_join_order_with_the_driver_first() {
        let out = probe("Q(A, B, C, D) :- R(A, B), S(C, D), T(C, B, D).", &[
            "A", "B", "C", "D",
        ]);
        assert_eq!(relations_of(&out), vec!["R", "T", "S"]);
        // T(C, B, D) is read B, C, D: source columns 1, 0, 2.
        assert_eq!(levels_of(&out), vec![
            (vec![0, 1], 0),
            (vec![1, 0, 2], 1),
            (vec![0, 1], 2)
        ]);
        assert_eq!(body_text(&out.query), vec![
            "#0(A, B)",
            "#1(B, C, D)",
            "#2(C, D)"
        ]);
        assert_eq!(out.plan.variable_ordering, vec![0, 1, 2, 3]);
        assert_eq!(out.plan.validate(&analyse(&out.query)), Ok(()));
    }

    /// The driver is the atom that binds the plan's first variable, not the
    /// first atom of the body.
    #[test]
    fn under_probe_keys_the_driver_binds_the_plans_first_variable() {
        let out = probe("Q(X, Y) :- R(Y), S(X, Y).", &["X", "Y"]);
        assert_eq!(relations_of(&out), vec!["S", "R"]);
        assert_eq!(levels_of(&out), vec![(vec![0, 1], 0), (vec![0], 1)]);
        assert_eq!(body_text(&out.query), vec!["#0(X, Y)", "#1(Y)"]);
        assert_eq!(out.plan.validate(&analyse(&out.query)), Ok(()));
    }

    /// `S(Y, Z)` is probed on `Z`, which `P(X, Z)` bound before `Y`: its
    /// columns become `(Z, Y)` and the returned plan is `V′ = [X, Z, Y]`, in
    /// the executed query's numbering (`X` = 0, `Y` = 1, `Z` = 2). Under `All`
    /// and `JoinKeys` the same query keeps its body and its plan as given.
    #[test]
    fn under_probe_keys_the_returned_plan_is_the_reordered_one() {
        let text = "Q(X, Y, Z) :- P(X, Z), S(Y, Z).";
        let order = ["X", "Y", "Z"];
        let out = probe(text, &order);
        assert_eq!(levels_of(&out), vec![(vec![0, 1], 0), (vec![1, 0], 1)]);
        assert_eq!(body_text(&out.query), vec!["#0(X, Z)", "#1(Z, Y)"]);
        assert_eq!(out.plan.variable_ordering, vec![0, 2, 1]);
        assert_eq!(out.plan.validate(&analyse(&out.query)), Ok(()));

        for levels in [TrieLevels::All, TrieLevels::JoinKeys] {
            let out = planned_with(text, &order, levels);
            assert_eq!(
                body_text(&out.query),
                vec!["#0(X, Z)", "#1(Y, Z)"],
                "{levels:?}"
            );
            assert_eq!(out.plan.variable_ordering, vec![0, 1, 2], "{levels:?}");
        }
        let all = planned(text, &order);
        assert_eq!(levels_of(&all), vec![(vec![0, 1], 2), (vec![0, 1], 2)]);
    }

    /// Atoms that share nothing join as a cross product, each with an empty
    /// key and so no level.
    #[test]
    fn under_probe_keys_a_cross_product_levels_nothing() {
        let out = probe("Q(A, B, C, D) :- R(A, B), S(C, D).", &["A", "C", "B", "D"]);
        assert_eq!(levels_of(&out), vec![(vec![0, 1], 0), (vec![0, 1], 0)]);
        assert_eq!(body_text(&out.query), vec!["#0(A, B)", "#1(C, D)"]);
        assert_eq!(out.plan.variable_ordering, vec![0, 1, 2, 3]);
        assert_eq!(out.plan.validate(&analyse(&out.query)), Ok(()));
    }

    /// A spec's levels are part of the comparison: the second read of `edge`
    /// is a filter on both columns, the first the driver, so they are two
    /// indexes under `ProbeKeys` and one under `All`.
    #[test]
    fn specs_that_differ_only_in_levels_stay_apart_under_probe_keys() {
        let text = "Q(X, Y) :- edge(X, Y), edge(X, Y).";
        let out = probe(text, &["X", "Y"]);
        assert_eq!(levels_of(&out), vec![(vec![0, 1], 0), (vec![0, 1], 2)]);
        assert_eq!(body_text(&out.query), vec!["#0(X, Y)", "#1(X, Y)"]);
        assert_eq!(planned(text, &["X", "Y"]).indexes.len(), 1);
    }

    #[test]
    fn a_query_with_no_accesses_plans_no_indexes_under_probe_keys() {
        let mut query: JoinQuery = "Q(c9) :- r(c1, c2), s(c3).".parse().expect("parse");
        query.head.terms.clear();
        let accesses = extract_accesses(&rewrite_placeholders(query)).expect("extract");
        let plan = QueryPlan {
            variable_ordering: vec![],
        };
        let out = plan_indexes(&accesses, &plan, TrieLevels::ProbeKeys);
        assert!(out.indexes.is_empty());
        assert!(out.query.body.is_empty());
        assert!(out.plan.variable_ordering.is_empty());
    }
}
