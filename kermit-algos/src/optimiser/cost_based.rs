//! Cost-based ordering policy: the plan with the lowest estimated cost.

use {
    crate::{
        analysis::analyse,
        optimiser::{
            ordering::{topological_order, Precedence},
            CardinalityOptimiser, CatalogStats, ColumnOrderPolicy, QueryOptimiser, QueryPlan,
            StatisticsLevel,
        },
    },
    kermit_parser::JoinQuery,
    std::{
        cmp::Ordering,
        collections::{hash_map::Entry, HashMap},
    },
};

/// Plans the global attribute order with the lowest estimated cost.
///
/// At depth k, Leapfrog Triejoin binds one key per tuple of the join of
/// every atom projected onto the plan's first k variables, `S_k`. That
/// number depends only on the set `S_k`, not on its order, so a plan's cost
/// is `Σ_k est(S_k)`, where `est` is the System R estimate of the join's
/// size: the product of the atoms' projection sizes, divided, per variable,
/// by every column's distinct count except the smallest. The cheapest plan
/// is then a shortest path over sets of bound variables, found by dynamic
/// programming in the style of Selinger et al. (1979). The search visits
/// only the sets a valid plan can bind, those closed under the column-order
/// constraints, so its plan is valid by construction. Under
/// `--column-orders any` there are no constraints, so every subset of the
/// variables is a state: 2ⁿ − 1 for n variables, which fits the default
/// budget up to 14 variables and falls back to [`CardinalityOptimiser`]
/// above.
///
/// Ties break towards the lexicographically smaller order, so plans are
/// deterministic. A query with more than 64 variables, or whose search
/// would reach more than [`state_budget`](Self::state_budget) sets, gets
/// [`CardinalityOptimiser`]'s plan.
///
/// Reads per-column distinct counts, so it declares
/// [`StatisticsLevel::ColumnDistinct`]. See `docs/optimisers/cost-based.md`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CostBasedOptimiser {
    /// The most variable sets the search may reach before it falls back to
    /// [`CardinalityOptimiser`].
    pub state_budget: usize,
}

impl CostBasedOptimiser {
    /// The default [`state_budget`](Self::state_budget): 20 times the 820
    /// sets of the largest WatDiv stress template (q0034, 13 variables).
    pub const DEFAULT_STATE_BUDGET: usize = 1 << 14;
}

impl Default for CostBasedOptimiser {
    fn default() -> Self {
        Self {
            state_budget: Self::DEFAULT_STATE_BUDGET,
        }
    }
}

impl QueryOptimiser for CostBasedOptimiser {
    fn plan(&self, query: &JoinQuery, stats: &CatalogStats) -> QueryPlan {
        let analysis = analyse(query);
        let model = CostModel::new(
            query,
            &analysis.predicate_variables,
            analysis.num_vars,
            stats,
        );
        let precedence = Precedence::for_query(query, stats);
        let cheapest = precedence
            .predecessor_masks()
            .and_then(|predecessors| cheapest_order(&model, &predecessors, self.state_budget));
        let Some(order) = cheapest else {
            return CardinalityOptimiser.plan(query, stats);
        };

        // The search binds only ready variables, so `order` is already a
        // topological order: ranked by its own positions, `topological_order`
        // returns it unchanged, so this plan comes out of `topological_order`
        // like every optimiser's. The assertion keeps that pass from quietly
        // repairing a search bug.
        let mut position = vec![0; analysis.num_vars];
        for (index, &v) in order.iter().enumerate() {
            position[v] = index;
        }
        let variable_ordering = topological_order(&precedence, |v| position[v]);
        debug_assert_eq!(
            variable_ordering, order,
            "the search returned an order that violates a column-order constraint"
        );
        QueryPlan {
            variable_ordering,
        }
    }

    fn required_statistics(&self) -> StatisticsLevel { StatisticsLevel::ColumnDistinct }
}

/// The one-variable set `{v}`. Sets of variables are `u64` bitmasks.
fn bit(v: usize) -> u64 { 1 << v }

/// Whether `set` holds variable `v`.
fn contains(set: u64, v: usize) -> bool { set & bit(v) != 0 }

/// One body atom's statistics, as the estimate reads them.
struct AtomStats {
    tuples: f64,
    /// Distinct values per column.
    distinct: Vec<f64>,
}

impl AtomStats {
    /// The statistics of relation `name`, with `arity` columns. Missing
    /// statistics mean "assume large", as for [`CardinalityOptimiser`]; a
    /// missing distinct count assumes the column is a key.
    fn of(name: &str, arity: usize, stats: &CatalogStats) -> Self {
        let tuples = stats.tuples(name).unwrap_or(usize::MAX) as f64;
        let distinct = (0..arity)
            .map(|column| stats.distinct(name, column).map_or(tuples, |d| d as f64))
            .collect();
        Self {
            tuples,
            distinct,
        }
    }

    /// The size of the atom's projection onto `columns` (ascending,
    /// non-empty): its tuples when they are all its columns, one column's
    /// distinct count on its own, and otherwise the product of the
    /// columns' distinct counts capped at the tuples, since no statistic
    /// covers a multi-column projection (independent columns assumed).
    /// Under `stored` the bound columns are always a prefix; under `any`
    /// any subset.
    fn projection_size(&self, columns: &[usize]) -> f64 {
        if columns.len() == self.distinct.len() {
            self.tuples
        } else if let [column] = columns {
            self.distinct[*column]
        } else {
            columns
                .iter()
                .map(|&column| self.distinct[column])
                .product::<f64>()
                .min(self.tuples)
        }
    }
}

/// `est(S)`: the System R estimate of how many keys LFTJ binds at the depth
/// where the bound variables are `S`.
struct CostModel<'q> {
    atoms: Vec<AtomStats>,
    /// Each atom's variables in column order, as `analyse` numbers them.
    predicate_variables: &'q [Vec<usize>],
    num_vars: usize,
    /// The policy the plan is made under: under `stored` every set the
    /// search reaches binds a prefix of each atom, which `estimate` checks.
    column_orders: ColumnOrderPolicy,
}

impl<'q> CostModel<'q> {
    /// The model of `query`, which must be the rewritten query (as
    /// [`QueryOptimiser::plan`] receives it): one variable per column, so
    /// `predicate_variables[i][c]` is the variable in atom `i`'s column `c`.
    fn new(
        query: &JoinQuery, predicate_variables: &'q [Vec<usize>], num_vars: usize,
        stats: &CatalogStats,
    ) -> Self {
        debug_assert!(
            query
                .body
                .iter()
                .zip(predicate_variables)
                .all(|(atom, vars)| vars.len() == atom.terms.len()),
            "the cost model reads the rewritten query: one variable per column"
        );
        let atoms = query
            .body
            .iter()
            .map(|atom| AtomStats::of(&atom.name, atom.terms.len(), stats))
            .collect();
        Self {
            atoms,
            predicate_variables,
            num_vars,
            column_orders: stats.column_orders(),
        }
    }

    /// `est(bound)`: over the atoms with a bound column, the product of
    /// their projections' sizes onto those columns, divided, for each bound
    /// variable, by the distinct counts of every column it occupies except
    /// the smallest (the containment-of-value-sets assumption).
    ///
    /// `bound` is any set of variables; under `stored` it is closed under
    /// the column-order constraints, so each atom's bound columns form a
    /// prefix, and under `any` they are any subset.
    fn estimate(&self, bound: u64) -> f64 {
        if bound == 0 {
            return 1.0;
        }
        let mut numerator = 1.0;
        // Each bound variable's column distinct counts, and the order the
        // variables were first met in: atoms in body order, an atom's
        // variables in canonical order. Any fixed order gives the same value
        // up to rounding; this one follows the #68 prototype
        // (`planners.py`), which the parity check compares against.
        let mut distinct_of: Vec<Vec<f64>> = vec![Vec::new(); self.num_vars];
        let mut met: Vec<usize> = Vec::new();
        for (atom, vars) in self.atoms.iter().zip(self.predicate_variables) {
            let bound_columns: Vec<usize> = (0..vars.len())
                .filter(|&c| contains(bound, vars[c]))
                .collect();
            debug_assert!(
                self.column_orders == ColumnOrderPolicy::Any
                    || bound_columns.iter().enumerate().all(|(i, &c)| i == c),
                "under `stored`, a set closed under column order binds a prefix of every atom"
            );
            if bound_columns.is_empty() {
                continue;
            }
            numerator *= atom.projection_size(&bound_columns);
            let mut columns = bound_columns;
            columns.sort_by_key(|&column| vars[column]);
            for column in columns {
                let v = vars[column];
                if distinct_of[v].is_empty() {
                    met.push(v);
                }
                distinct_of[v].push(atom.distinct[column]);
            }
        }
        let mut denominator = 1.0;
        for v in met {
            let counts = &mut distinct_of[v];
            counts.sort_by(f64::total_cmp);
            for &count in &counts[1..] {
                // Clamped, so an empty relation cannot divide by zero.
                denominator *= count.max(1.0);
            }
        }
        numerator / denominator
    }
}

/// The cheapest known way to bind one set of variables.
struct Partial {
    cost: f64,
    order: Vec<usize>,
}

impl Partial {
    /// Whether `self` is preferred to `other`: the lower cost, ties to the
    /// lexicographically smaller order, so the search is deterministic.
    fn beats(&self, other: &Partial) -> bool {
        match self.cost.total_cmp(&other.cost) {
            | Ordering::Less => true,
            | Ordering::Greater => false,
            | Ordering::Equal => self.order < other.order,
        }
    }
}

/// The cheapest order of the variables, by a forward dynamic programme
/// over the sets a valid plan can bind. Layer k holds the k-variable sets,
/// each with its cheapest order; a set grows only by a variable whose
/// `predecessors` are all bound.
///
/// `None` once more than `budget` sets have been reached.
fn cheapest_order(
    model: &CostModel<'_>, predecessors: &[u64], budget: usize,
) -> Option<Vec<usize>> {
    let num_vars = predecessors.len();
    let mut layer = HashMap::from([(0u64, Partial {
        cost: 0.0,
        order: Vec::new(),
    })]);
    let mut reached = 0usize;
    for _ in 0..num_vars {
        let mut next: HashMap<u64, Partial> = HashMap::new();
        let mut estimates: HashMap<u64, f64> = HashMap::new();
        for (&bound, partial) in &layer {
            let ready = |v: usize| !contains(bound, v) && predecessors[v] & !bound == 0;
            for v in (0..num_vars).filter(|&v| ready(v)) {
                let extended = bound | bit(v);
                let estimate = *estimates
                    .entry(extended)
                    .or_insert_with(|| model.estimate(extended));
                let mut order = partial.order.clone();
                order.push(v);
                let candidate = Partial {
                    cost: partial.cost + estimate,
                    order,
                };
                match next.entry(extended) {
                    | Entry::Vacant(slot) => {
                        reached += 1;
                        if reached > budget {
                            return None;
                        }
                        slot.insert(candidate);
                    },
                    | Entry::Occupied(mut slot) => {
                        if candidate.beats(slot.get()) {
                            slot.insert(candidate);
                        }
                    },
                }
            }
        }
        layer = next;
    }
    layer.into_values().next().map(|partial| partial.order)
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::optimiser::{check_attribute_order, ColumnOrderPolicy, RelationStats},
        kermit_parser::JoinQuery,
    };

    /// Statistics for `q`'s relations: `(name, tuples, per-column distinct
    /// counts)`.
    fn stats_for(q: &JoinQuery, relations: &[(&str, usize, &[usize])]) -> CatalogStats {
        CatalogStats::for_query(q, ColumnOrderPolicy::Stored, |name| {
            let &(_, tuples, distinct) = relations.iter().find(|(n, ..)| *n == name)?;
            Some(RelationStats::new(tuples, distinct.len()).with_column_distinct(distinct.to_vec()))
        })
    }

    /// `est` of the set of canonical variable indices `vars`.
    fn estimate(q: &JoinQuery, stats: &CatalogStats, vars: &[usize]) -> f64 {
        let analysis = analyse(q);
        let model = CostModel::new(q, &analysis.predicate_variables, analysis.num_vars, stats);
        model.estimate(vars.iter().fold(0, |set, &v| set | bit(v)))
    }

    #[test]
    fn a_join_on_one_variable_divides_by_the_larger_distinct_count() {
        // |R ⋈ S| on Y = |R| · |S| / max(V(R, Y), V(S, Y)) = 100 · 60 / 40.
        let q: JoinQuery = "Q(X, Y, Z) :- R(X, Y), S(Z, Y).".parse().unwrap();
        let stats = stats_for(&q, &[("R", 100, &[10, 20]), ("S", 60, &[30, 40])]);
        assert_eq!(estimate(&q, &stats, &[0, 1, 2]), 150.0);
    }

    #[test]
    fn a_partly_bound_atom_contributes_its_first_columns_distinct_count() {
        let q: JoinQuery = "Q(X, Y) :- R(X, Y).".parse().unwrap();
        let stats = stats_for(&q, &[("R", 100, &[10, 20])]);
        assert_eq!(estimate(&q, &stats, &[0]), 10.0);
        assert_eq!(estimate(&q, &stats, &[]), 1.0);
    }

    #[test]
    fn a_longer_prefix_assumes_independent_columns_capped_by_the_tuples() {
        let q: JoinQuery = "Q(X, Y, Z) :- T(X, Y, Z).".parse().unwrap();
        let stats = stats_for(&q, &[("T", 1000, &[10, 5, 7])]);
        assert_eq!(estimate(&q, &stats, &[0, 1]), 50.0);
        let stats = stats_for(&q, &[("T", 1000, &[100, 50, 7])]);
        assert_eq!(estimate(&q, &stats, &[0, 1]), 1000.0);
    }

    #[test]
    fn empty_relations_estimate_zero_not_nan() {
        let q: JoinQuery = "Q(X) :- R(X), S(X).".parse().unwrap();
        let stats = stats_for(&q, &[("R", 0, &[0]), ("S", 0, &[0])]);
        assert_eq!(estimate(&q, &stats, &[0]), 0.0);
    }

    /// The worked micro-example of `docs/optimisers/cost-based.md`, already
    /// rewritten: `Const_c7` pins `K0`, which selects few `B`s through
    /// `link`; `small` has few tuples, but its `A`s meet the rest only
    /// through `Y`. Canonical indices: A = 0, B = 1, Y = 2, K0 = 3.
    fn worked_example() -> (JoinQuery, CatalogStats) {
        let q: JoinQuery = "Q(A, B, Y) :- small(A, Y), link(K0, B), owns(B, Y), Const_c7(K0)."
            .parse()
            .unwrap();
        let stats = stats_for(&q, &[
            ("small", 100, &[100, 10]),
            ("link", 50_000, &[5_000, 20_000]),
            ("owns", 40_000, &[20_000, 10]),
        ]);
        (q, stats)
    }

    #[test]
    fn worked_example_estimates() {
        let (q, stats) = worked_example();
        for (vars, want) in [
            (&[3][..], 1.0),
            (&[0][..], 100.0),
            (&[0, 3][..], 100.0),
            (&[1, 3][..], 10.0),
            (&[0, 1, 3][..], 1000.0),
            (&[0, 1, 2, 3][..], 200.0),
        ] {
            assert_eq!(estimate(&q, &stats, vars), want, "est({vars:?})");
        }
    }

    #[test]
    fn worked_example_binds_the_selective_side_first() {
        let (q, stats) = worked_example();
        // K0, B, A, Y costs 1 + 10 + 1000 + 200 = 1211. Cardinality binds
        // small's A second, for 1 + 100 + 1000 + 200 = 1301.
        let plan = CostBasedOptimiser::default().plan(&q, &stats);
        assert_eq!(plan.variable_ordering, vec![3, 1, 0, 2]);
        let cardinality = CardinalityOptimiser.plan(&q, &stats);
        assert_eq!(cardinality.variable_ordering, vec![3, 0, 1, 2]);
    }

    #[test]
    fn column_order_constraints_bind() {
        // Y's relation S is tiny, but R(X, Y) puts X first.
        let q: JoinQuery = "Q(X, Y) :- R(X, Y), S(Y).".parse().unwrap();
        let stats = stats_for(&q, &[("R", 1000, &[1000, 1000]), ("S", 1, &[1])]);
        let plan = CostBasedOptimiser::default().plan(&q, &stats);
        assert_eq!(plan.variable_ordering, vec![0, 1]);
    }

    #[test]
    fn equal_costs_take_the_lexicographically_smaller_order() {
        // X and Y are interchangeable, so both orders cost 10 + 100. The
        // search meets tied candidates in `HashMap` order, which changes
        // from map to map, so a lucky order could hide a missing tie-break:
        // each plan is repeated, every time with fresh maps.
        let q: JoinQuery = "Q(X, Y) :- R(X), S(Y).".parse().unwrap();
        let stats = stats_for(&q, &[("R", 10, &[10]), ("S", 10, &[10])]);
        // Swapping the head swaps the canonical indices; the smaller still
        // comes first.
        let swapped: JoinQuery = "Q(Y, X) :- R(X), S(Y).".parse().unwrap();
        for _ in 0..64 {
            let plan = CostBasedOptimiser::default().plan(&q, &stats);
            assert_eq!(plan.variable_ordering, vec![0, 1]);
            let plan = CostBasedOptimiser::default().plan(&swapped, &stats);
            assert_eq!(plan.variable_ordering, vec![0, 1]);
        }
    }

    #[test]
    fn a_fully_bound_atom_counts_its_tuples_not_its_first_column() {
        // A multiset (the hash trie) can hold more tuples than distinct
        // values; a fully bound unary atom still contributes its tuples.
        let q: JoinQuery = "Q(X) :- R(X).".parse().unwrap();
        let stats = stats_for(&q, &[("R", 10, &[4])]);
        assert_eq!(estimate(&q, &stats, &[0]), 10.0);
    }

    #[test]
    fn an_empty_relation_the_set_does_not_touch_leaves_the_estimate_alone() {
        // At the depth that binds only X, LFTJ enumerates R's keys whatever
        // S holds.
        let q: JoinQuery = "Q(X, Y) :- R(X), S(Y).".parse().unwrap();
        let stats = stats_for(&q, &[("R", 10, &[10]), ("S", 0, &[0])]);
        assert_eq!(estimate(&q, &stats, &[0]), 10.0);
    }

    #[test]
    fn a_relation_without_statistics_is_assumed_large() {
        let q: JoinQuery = "Q(X, Y) :- Known(X), Unknown(Y).".parse().unwrap();
        let stats = stats_for(&q, &[("Known", 10, &[10])]);
        let plan = CostBasedOptimiser::default().plan(&q, &stats);
        assert_eq!(plan.variable_ordering, vec![0, 1]);
        let q: JoinQuery = "Q(Y, X) :- Known(X), Unknown(Y).".parse().unwrap();
        let plan = CostBasedOptimiser::default().plan(&q, &stats);
        assert_eq!(plan.variable_ordering, vec![1, 0]);
    }

    #[test]
    fn a_column_without_a_distinct_count_is_assumed_a_key() {
        let q: JoinQuery = "Q(X, Y) :- R(X, Y).".parse().unwrap();
        let stats = CatalogStats::for_query(&q, ColumnOrderPolicy::Stored, |_| {
            Some(RelationStats::new(100, 2))
        });
        assert_eq!(estimate(&q, &stats, &[0]), 100.0);
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "the cost model reads the rewritten query")]
    fn an_unrewritten_query_is_refused() {
        let q: JoinQuery = "Q(X) :- R(c5, X).".parse().unwrap();
        let stats = stats_for(&q, &[("R", 10, &[10, 10])]);
        CostBasedOptimiser::default().plan(&q, &stats);
    }

    #[test]
    fn the_budget_counts_every_set_reached() {
        // The worked example reaches six sets: {A}, {K0}, {A, K0}, {K0, B},
        // {A, K0, B} and all four.
        let (q, stats) = worked_example();
        let fits = CostBasedOptimiser {
            state_budget: 6,
        }
        .plan(&q, &stats);
        assert_eq!(fits.variable_ordering, vec![3, 1, 0, 2]);
        let cardinality = CardinalityOptimiser.plan(&q, &stats);
        assert_eq!(
            CostBasedOptimiser {
                state_budget: 5
            }
            .plan(&q, &stats),
            cardinality
        );
        assert_eq!(
            CostBasedOptimiser {
                state_budget: 0
            }
            .plan(&q, &stats),
            cardinality
        );
    }

    #[test]
    fn more_than_64_variables_get_cardinalitys_plan() {
        let body: Vec<String> = (0..65).map(|v| format!("R(V{v})")).collect();
        let q: JoinQuery = format!("Q(V0) :- {}.", body.join(", ")).parse().unwrap();
        let stats = stats_for(&q, &[("R", 10, &[10])]);
        assert_eq!(
            CostBasedOptimiser::default().plan(&q, &stats),
            CardinalityOptimiser.plan(&q, &stats)
        );
    }

    #[test]
    fn it_reads_column_distinct_counts() {
        assert_eq!(
            CostBasedOptimiser::default().required_statistics(),
            StatisticsLevel::ColumnDistinct
        );
    }

    fn stats_under(
        q: &JoinQuery, policy: ColumnOrderPolicy, relations: &[(&str, usize, &[usize])],
    ) -> CatalogStats {
        CatalogStats::for_query(q, policy, |name| {
            let &(_, tuples, distinct) = relations.iter().find(|(n, ..)| *n == name)?;
            Some(RelationStats::new(tuples, distinct.len()).with_column_distinct(distinct.to_vec()))
        })
    }

    /// Under `any` a bound set need not be a stored-order prefix of an
    /// atom: `{Y}` binds `R`'s second column alone, and the estimate reads
    /// that column's distinct count.
    #[test]
    fn under_any_the_bound_columns_need_not_be_a_prefix() {
        let q: JoinQuery = "Q(X, Y) :- R(X, Y).".parse().unwrap();
        let stats = stats_under(&q, ColumnOrderPolicy::Any, &[("R", 100, &[10, 20])]);
        assert_eq!(estimate(&q, &stats, &[1]), 20.0);
        assert_eq!(estimate(&q, &stats, &[0]), 10.0);
        assert_eq!(estimate(&q, &stats, &[0, 1]), 100.0);
    }

    /// Under `any` a non-prefix pair of a ternary atom multiplies those
    /// columns' distinct counts, capped at the tuples, like a prefix does.
    #[test]
    fn under_any_a_column_pair_assumes_independent_columns_capped_by_the_tuples() {
        let q: JoinQuery = "Q(X, Y, Z) :- T(X, Y, Z).".parse().unwrap();
        let stats = stats_under(&q, ColumnOrderPolicy::Any, &[("T", 1000, &[10, 5, 7])]);
        assert_eq!(estimate(&q, &stats, &[0, 2]), 70.0);
        let stats = stats_under(&q, ColumnOrderPolicy::Any, &[("T", 1000, &[100, 50, 70])]);
        assert_eq!(estimate(&q, &stats, &[1, 2]), 1000.0);
    }

    /// `column_order_constraints_bind` is the `stored` half: `R(X, Y)`
    /// forces `X` first although `S` is tiny. Under `any` the cheap side
    /// goes first.
    #[test]
    fn under_any_the_cheap_side_goes_first() {
        let q: JoinQuery = "Q(X, Y) :- R(X, Y), S(Y).".parse().unwrap();
        let relations: &[(&str, usize, &[usize])] = &[("R", 1000, &[1000, 1000]), ("S", 1, &[1])];
        let stored = stats_under(&q, ColumnOrderPolicy::Stored, relations);
        assert_eq!(
            CostBasedOptimiser::default()
                .plan(&q, &stored)
                .variable_ordering,
            vec![0, 1]
        );
        let any = stats_under(&q, ColumnOrderPolicy::Any, relations);
        assert_eq!(
            CostBasedOptimiser::default()
                .plan(&q, &any)
                .variable_ordering,
            vec![1, 0]
        );
    }

    /// With no edges every subset is a state: three variables reach seven
    /// sets, so a budget of six falls back to `cardinality` under `any`
    /// while `stored` reaches only three.
    #[test]
    fn a_free_search_reaches_every_subset() {
        let q: JoinQuery = "Q(X, Y, Z) :- T(X, Y, Z).".parse().unwrap();
        let relations: &[(&str, usize, &[usize])] = &[("T", 1000, &[10, 5, 7])];
        let any = stats_under(&q, ColumnOrderPolicy::Any, relations);
        let seven = CostBasedOptimiser {
            state_budget: 7,
        }
        .plan(&q, &any);
        // est({Y}) = 5 is the cheapest first step; then est({Y, Z}) = 35
        // beats est({X, Y}) = 50, and every order ends at est({X, Y, Z}) =
        // 1000: Y, Z, X costs 1040, the next best (Z, Y, X) 1042.
        assert_eq!(seven.variable_ordering, vec![1, 2, 0]);
        assert_eq!(
            CostBasedOptimiser {
                state_budget: 6
            }
            .plan(&q, &any),
            CardinalityOptimiser.plan(&q, &any)
        );
        let stored = stats_under(&q, ColumnOrderPolicy::Stored, relations);
        assert_eq!(
            CostBasedOptimiser {
                state_budget: 3
            }
            .plan(&q, &stored)
            .variable_ordering,
            vec![0, 1, 2]
        );
    }

    /// A 64-bit linear congruential generator (Knuth's MMIX constants), as
    /// `kermit/tests/column_orders_equivalence.rs` has: seeded, so a
    /// failing case reproduces, with no new dependency.
    struct Lcg(u64);

    impl Lcg {
        fn next(&mut self) -> u64 {
            self.0 = self
                .0
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            self.0 >> 33
        }

        /// A value in `0..n`.
        fn below(&mut self, n: usize) -> usize { (self.next() % n as u64) as usize }
    }

    /// Relations as `(name, tuples, per-column distinct counts)`.
    type Relations = Vec<(String, usize, Vec<usize>)>;

    /// How [`random_case`] draws a relation's counts.
    #[derive(Debug, Clone, Copy)]
    enum Counts {
        /// Zero or a power of two up to 8. Every estimate is then a power
        /// of two (or zero) in a range `f64` sums exactly, so plans that tie
        /// mathematically tie in floating point too, and ties are common.
        SmallPowersOfTwo,
        /// Tuples and distinct counts spread log-uniformly up to millions,
        /// so plans' costs differ by orders of magnitude, as real
        /// statistics' do.
        Wide,
    }

    /// A count spread log-uniformly over 1 to 9,000,000.
    fn log_uniform(rng: &mut Lcg) -> usize { (1 + rng.below(9)) * 10usize.pow(rng.below(7) as u32) }

    /// A random query as the planner receives it, and its statistics: two
    /// to four atoms over three relations of arity 1–3, each atom's columns
    /// distinct variables (the selection rewrite leaves no repeat) drawn
    /// from six, and the head every variable used.
    fn random_case(rng: &mut Lcg, counts: Counts) -> (String, Relations) {
        let relations: Relations = (0..3)
            .map(|i| {
                let arity = 1 + rng.below(3);
                let (tuples, distinct) = match counts {
                    | Counts::SmallPowersOfTwo => {
                        let tuples: usize = match rng.below(5) {
                            | 0 => 0,
                            | k => 1 << (k - 1),
                        };
                        let distinct = (0..arity)
                            .map(|_| {
                                if tuples == 0 {
                                    0
                                } else {
                                    1 << rng.below(tuples.trailing_zeros() as usize + 1)
                                }
                            })
                            .collect();
                        (tuples, distinct)
                    },
                    | Counts::Wide => {
                        let tuples = log_uniform(rng);
                        let distinct = (0..arity).map(|_| log_uniform(rng).min(tuples)).collect();
                        (tuples, distinct)
                    },
                };
                (format!("r{i}"), tuples, distinct)
            })
            .collect();
        let mut used: Vec<String> = Vec::new();
        let atoms: Vec<String> = (0..2 + rng.below(3))
            .map(|_| {
                let (name, _, distinct) = &relations[rng.below(relations.len())];
                let mut pool: Vec<usize> = (0..6).collect();
                let terms: Vec<String> = (0..distinct.len())
                    .map(|_| {
                        let v = format!("V{}", pool.remove(rng.below(pool.len())));
                        if !used.contains(&v) {
                            used.push(v.clone());
                        }
                        v
                    })
                    .collect();
                format!("{name}({})", terms.join(", "))
            })
            .collect();
        let text = format!("Q({}) :- {}.", used.join(", "), atoms.join(", "));
        (text, relations)
    }

    /// Every order of the variables that binds each after its
    /// `predecessors`, in lexicographic order.
    fn valid_orders(predecessors: &[u64]) -> Vec<Vec<usize>> {
        fn extend(predecessors: &[u64], order: &mut Vec<usize>, out: &mut Vec<Vec<usize>>) {
            if order.len() == predecessors.len() {
                out.push(order.clone());
                return;
            }
            let bound = order.iter().fold(0, |set, &v| set | bit(v));
            for v in 0..predecessors.len() {
                if !contains(bound, v) && predecessors[v] & !bound == 0 {
                    order.push(v);
                    extend(predecessors, order, out);
                    order.pop();
                }
            }
        }
        let mut out = Vec::new();
        extend(predecessors, &mut Vec::new(), &mut out);
        out
    }

    /// `Σ_k est(S_k)` for `order`, summed in the order the search sums it.
    fn cost_of(model: &CostModel<'_>, order: &[usize]) -> f64 {
        let mut bound = 0;
        order.iter().fold(0.0, |cost, &v| {
            bound |= bit(v);
            cost + model.estimate(bound)
        })
    }

    /// The search's plan for one random case and what brute force finds.
    struct Searched {
        plan: Vec<usize>,
        plan_cost: f64,
        /// The cheapest valid order, ties to the lexicographically smaller.
        cheapest: Vec<usize>,
        least: f64,
    }

    /// Plans `q` under `policy` and enumerates every order the column-order
    /// constraints allow. `None` when `stored` finds `q` cyclic.
    fn search_and_enumerate(
        q: &JoinQuery, relations: &Relations, policy: ColumnOrderPolicy,
    ) -> Option<Searched> {
        if policy == ColumnOrderPolicy::Stored && check_attribute_order(q).is_err() {
            return None;
        }
        let relations: Vec<(&str, usize, &[usize])> = relations
            .iter()
            .map(|(name, tuples, distinct)| (name.as_str(), *tuples, distinct.as_slice()))
            .collect();
        let stats = stats_under(q, policy, &relations);
        let analysis = analyse(q);
        let model = CostModel::new(q, &analysis.predicate_variables, analysis.num_vars, &stats);
        let predecessors = Precedence::for_query(q, &stats)
            .predecessor_masks()
            .unwrap();
        let mut best: Option<(f64, Vec<usize>)> = None;
        for order in valid_orders(&predecessors) {
            let cost = cost_of(&model, &order);
            if best.as_ref().is_none_or(|(least, _)| cost < *least) {
                best = Some((cost, order));
            }
        }
        let (least, cheapest) = best.expect("an acyclic query has a valid order");
        let plan = CostBasedOptimiser::default()
            .plan(q, &stats)
            .variable_ordering;
        Some(Searched {
            plan_cost: cost_of(&model, &plan),
            plan,
            cheapest,
            least,
        })
    }

    /// The seeds each random-query test plans.
    const SEARCH_SEEDS: u64 = 300;

    /// The search is exact for its cost model: on random queries whose
    /// statistics span orders of magnitude, under both policies, its plan
    /// costs what the cheapest order the column-order constraints allow
    /// does, as brute force over every valid order finds it. Exactly,
    /// although `f64` rounds: the search sums each order's estimates in the
    /// order `cost_of` does, and rounding is monotonic, so no order can come
    /// out cheaper than the plan. And `any`, which allows every order
    /// `stored` does, never plans dearer.
    #[test]
    #[cfg_attr(
        miri,
        ignore = "enumerates every valid order, minutes under Miri; the tests above run the \
                  search there"
    )]
    fn the_search_finds_the_cheapest_valid_order() {
        // Cases searched under `stored` (the rest are cyclic there), and
        // cases where `any` found a strictly cheaper plan.
        let (mut stored_cases, mut any_cheaper) = (0, 0);
        for seed in 0..SEARCH_SEEDS {
            let (text, relations) = random_case(&mut Lcg(seed), Counts::Wide);
            let q: JoinQuery = text.parse().unwrap();
            let mut least = Vec::new();
            for policy in [ColumnOrderPolicy::Stored, ColumnOrderPolicy::Any] {
                let Some(searched) = search_and_enumerate(&q, &relations, policy) else {
                    continue;
                };
                assert_eq!(
                    searched.plan_cost, searched.least,
                    "seed {seed}, {policy:?}: {text}: planned {:?}, cheapest {:?}",
                    searched.plan, searched.cheapest
                );
                least.push(searched.least);
            }
            if let [stored, any] = least[..] {
                assert!(any <= stored, "seed {seed}: {text}");
                stored_cases += 1;
                any_cheaper += usize::from(any < stored);
            }
        }
        // The first 300 seeds give 249 queries `stored` can plan (the other
        // 51 are cyclic there), 136 of which `any` plans cheaper. The bounds
        // catch a generator that stops producing cases where the policies
        // differ.
        assert!(
            stored_cases >= 200 && any_cheaper >= 80,
            "stored {stored_cases}, any cheaper {any_cheaper}"
        );
    }

    /// Among plans of equal cost the search takes the lexicographically
    /// smallest order, as brute force does, on random queries whose
    /// statistics make ties common and exact.
    #[test]
    #[cfg_attr(
        miri,
        ignore = "enumerates every valid order, minutes under Miri; the tests above run the \
                  search there"
    )]
    fn random_ties_take_the_lexicographically_smaller_order() {
        for seed in 0..SEARCH_SEEDS {
            let (text, relations) = random_case(&mut Lcg(seed), Counts::SmallPowersOfTwo);
            let q: JoinQuery = text.parse().unwrap();
            for policy in [ColumnOrderPolicy::Stored, ColumnOrderPolicy::Any] {
                if let Some(searched) = search_and_enumerate(&q, &relations, policy) {
                    assert_eq!(
                        searched.plan, searched.cheapest,
                        "seed {seed}, {policy:?}: {text}"
                    );
                }
            }
        }
    }
}
