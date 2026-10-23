//! Cost-based ordering policy: the plan with the lowest estimated cost.

use {
    crate::{
        analysis::analyse,
        optimiser::{
            CardinalityOptimiser, CatalogStats, QueryOptimiser, QueryPlan, StatisticsLevel,
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
/// programming in the style of Selinger et al. (1979). No column order
/// constrains the plan, since a query builds each index with its columns in
/// plan order, so every subset of the variables is a state: 2ⁿ − 1 for n
/// variables, which fits the default budget up to 14 variables and falls
/// back to [`CardinalityOptimiser`] above.
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
    /// The default [`state_budget`](Self::state_budget): 2¹⁴ = 16,384 sets,
    /// every subset of 14 variables. A query of up to 14 variables is always
    /// searched in full, since its 2ⁿ − 1 sets fit the budget; the budget only
    /// bounds the planning time of a query of more. The most any committed
    /// query has is 12 variables (4,095 sets).
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
        let Some(variable_ordering) = cheapest_order(&model, analysis.num_vars, self.state_budget)
        else {
            return CardinalityOptimiser.plan(query, stats);
        };
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
    /// covers a multi-column projection (independent columns assumed). The
    /// bound columns are any subset of the atom's columns.
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
}

impl<'q> CostModel<'q> {
    /// The model of `query`, which must be the query of accesses (as
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
            "the cost model reads the query of accesses: one variable per column"
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
        }
    }

    /// `est(bound)`: over the atoms with a bound column, the product of
    /// their projections' sizes onto those columns, divided, for each bound
    /// variable, by the distinct counts of every column it occupies except
    /// the smallest (the containment-of-value-sets assumption).
    ///
    /// `bound` is any set of variables, so an atom's bound columns are any
    /// subset of its columns.
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

/// The cheapest order of the `num_vars` variables, by a forward dynamic
/// programme over the sets of bound variables. Layer k holds the k-variable
/// sets, each with its cheapest order; a set grows by any variable not yet
/// in it.
///
/// `None` past 64 variables, which a `u64` set cannot index, and once more
/// than `budget` sets have been reached.
fn cheapest_order(model: &CostModel<'_>, num_vars: usize, budget: usize) -> Option<Vec<usize>> {
    if num_vars > 64 {
        return None;
    }
    let mut layer = HashMap::from([(0u64, Partial {
        cost: 0.0,
        order: Vec::new(),
    })]);
    let mut reached = 0usize;
    for _ in 0..num_vars {
        let mut next: HashMap<u64, Partial> = HashMap::new();
        let mut estimates: HashMap<u64, f64> = HashMap::new();
        for (&bound, partial) in &layer {
            for v in (0..num_vars).filter(|&v| !contains(bound, v)) {
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
    use {super::*, crate::optimiser::RelationStats, kermit_parser::JoinQuery};

    /// Statistics for `q`'s relations: `(name, tuples, per-column distinct
    /// counts)`.
    fn stats_for(q: &JoinQuery, relations: &[(&str, usize, &[usize])]) -> CatalogStats {
        CatalogStats::for_query(q, |name| {
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

    /// The worked micro-example of `docs/optimisers/cost-based.md`, planned
    /// as `prepare` plans it: over the accesses of
    /// `Q(A, B, Y) :- small(A, Y), link(c7, B), owns(B, Y).`, whose
    /// statistics are keyed by access id. `@1` is `link` filtered to
    /// `col0=7`: the 12 rows that match, and B's 12 distinct values.
    /// Canonical indices: A = 0, B = 1, Y = 2.
    fn worked_example() -> (JoinQuery, CatalogStats) {
        let text = "Q(A, B, Y) :- small(A, Y), link(c7, B), owns(B, Y).";
        let accesses = crate::extract_accesses(&text.parse().unwrap()).unwrap();
        assert_eq!(accesses.accesses.len(), 3, "one access per atom");
        let stats = stats_for(&accesses.query, &[
            ("@0", 100, &[100, 10]),
            ("@1", 12, &[12]),
            ("@2", 40_000, &[20_000, 10]),
        ]);
        (accesses.query, stats)
    }

    /// The estimate table of the worked example.
    #[test]
    fn worked_example_estimates() {
        let (q, stats) = worked_example();
        for (vars, want) in [
            (&[0][..], 100.0),
            (&[1][..], 12.0),
            (&[2][..], 10.0),
            (&[0, 1][..], 1_200.0),
            (&[0, 2][..], 100.0),
            (&[1, 2][..], 24.0),
            (&[0, 1, 2][..], 240.0),
        ] {
            assert_eq!(estimate(&q, &stats, vars), want, "est({vars:?})");
        }
    }

    /// The plans table of the worked example: each order's cost, the sum of
    /// the estimates of its prefixes, then the plans each optimiser picks.
    #[test]
    fn worked_example_plans() {
        let (q, stats) = worked_example();
        let cost = |order: &[usize]| -> f64 {
            (1..=order.len())
                .map(|k| estimate(&q, &stats, &order[..k]))
                .sum()
        };
        for (order, want) in [
            (&[2, 1, 0][..], 274.0),
            (&[1, 2, 0][..], 276.0),
            (&[2, 0, 1][..], 350.0),
            (&[1, 0, 2][..], 1_452.0),
            (&[0, 1, 2][..], 1_540.0),
        ] {
            assert_eq!(cost(order), want, "cost({order:?})");
        }
        let plan = CostBasedOptimiser::default().plan(&q, &stats);
        assert_eq!(plan.variable_ordering, vec![2, 1, 0], "cost-based");
        let cardinality = CardinalityOptimiser.plan(&q, &stats);
        assert_eq!(cardinality.variable_ordering, vec![1, 0, 2], "cardinality");
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
        let stats = CatalogStats::for_query(&q, |_| Some(RelationStats::new(100, 2)));
        assert_eq!(estimate(&q, &stats, &[0]), 100.0);
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "the cost model reads the query of accesses")]
    fn a_query_with_a_constant_term_is_refused() {
        let q: JoinQuery = "Q(X) :- R(c5, X).".parse().unwrap();
        let stats = stats_for(&q, &[("R", 10, &[10, 10])]);
        CostBasedOptimiser::default().plan(&q, &stats);
    }

    #[test]
    fn the_budget_counts_every_set_reached() {
        // No constraint limits the search, so the worked example reaches
        // every non-empty subset of its three variables: 3 + 3 + 1 = 7 sets.
        let (q, stats) = worked_example();
        let fits = CostBasedOptimiser {
            state_budget: 7,
        }
        .plan(&q, &stats);
        assert_eq!(fits.variable_ordering, vec![2, 1, 0]);
        let cardinality = CardinalityOptimiser.plan(&q, &stats);
        assert_eq!(
            CostBasedOptimiser {
                state_budget: 6
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

    /// An atom's bound columns need not be a stored-order prefix: `{Y}`
    /// binds `R`'s second column alone, and the estimate reads that
    /// column's distinct count.
    #[test]
    fn the_bound_columns_need_not_be_a_prefix() {
        let q: JoinQuery = "Q(X, Y) :- R(X, Y).".parse().unwrap();
        let stats = stats_for(&q, &[("R", 100, &[10, 20])]);
        assert_eq!(estimate(&q, &stats, &[1]), 20.0);
        assert_eq!(estimate(&q, &stats, &[0]), 10.0);
        assert_eq!(estimate(&q, &stats, &[0, 1]), 100.0);
    }

    /// A non-prefix pair of a ternary atom multiplies those columns'
    /// distinct counts, capped at the tuples, like a prefix does.
    #[test]
    fn a_column_pair_assumes_independent_columns_capped_by_the_tuples() {
        let q: JoinQuery = "Q(X, Y, Z) :- T(X, Y, Z).".parse().unwrap();
        let stats = stats_for(&q, &[("T", 1000, &[10, 5, 7])]);
        assert_eq!(estimate(&q, &stats, &[0, 2]), 70.0);
        let stats = stats_for(&q, &[("T", 1000, &[100, 50, 70])]);
        assert_eq!(estimate(&q, &stats, &[1, 2]), 1000.0);
    }

    /// `S` is tiny, although `R(X, Y)` lists `X` first: no column order
    /// constrains the plan, so the cheap side goes first.
    #[test]
    fn the_cheap_side_goes_first() {
        let q: JoinQuery = "Q(X, Y) :- R(X, Y), S(Y).".parse().unwrap();
        let stats = stats_for(&q, &[("R", 1000, &[1000, 1000]), ("S", 1, &[1])]);
        assert_eq!(
            CostBasedOptimiser::default()
                .plan(&q, &stats)
                .variable_ordering,
            vec![1, 0]
        );
    }

    /// One ternary atom: the search binds its most selective column first,
    /// whatever its position. (`the_budget_counts_every_set_reached` checks
    /// that three variables reach seven sets.)
    #[test]
    fn the_most_selective_column_goes_first() {
        let q: JoinQuery = "Q(X, Y, Z) :- T(X, Y, Z).".parse().unwrap();
        let stats = stats_for(&q, &[("T", 1000, &[10, 5, 7])]);
        // est({Y}) = 5 is the cheapest first step; then est({Y, Z}) = 35
        // beats est({X, Y}) = 50, and every order ends at est({X, Y, Z}) =
        // 1000: Y, Z, X costs 1040, the next best (Z, Y, X) 1042.
        assert_eq!(
            CostBasedOptimiser::default()
                .plan(&q, &stats)
                .variable_ordering,
            vec![1, 2, 0]
        );
    }

    /// A 64-bit linear congruential generator (Knuth's MMIX constants), as
    /// `kermit/tests/common/oracle.rs` has: seeded, so a failing case
    /// reproduces, with no new dependency.
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
    /// distinct variables (an access repeats none) drawn from six, and the
    /// head every variable used.
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

    /// Every order of `num_vars` variables, in lexicographic order.
    fn every_order(num_vars: usize) -> Vec<Vec<usize>> {
        fn extend(num_vars: usize, order: &mut Vec<usize>, out: &mut Vec<Vec<usize>>) {
            if order.len() == num_vars {
                out.push(order.clone());
                return;
            }
            for v in 0..num_vars {
                if !order.contains(&v) {
                    order.push(v);
                    extend(num_vars, order, out);
                    order.pop();
                }
            }
        }
        let mut out = Vec::new();
        extend(num_vars, &mut Vec::new(), &mut out);
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

    /// Plans `q` and enumerates every order of its variables.
    fn search_and_enumerate(q: &JoinQuery, relations: &Relations) -> Searched {
        let relations: Vec<(&str, usize, &[usize])> = relations
            .iter()
            .map(|(name, tuples, distinct)| (name.as_str(), *tuples, distinct.as_slice()))
            .collect();
        let stats = stats_for(q, &relations);
        let analysis = analyse(q);
        let model = CostModel::new(q, &analysis.predicate_variables, analysis.num_vars, &stats);
        let mut best: Option<(f64, Vec<usize>)> = None;
        for order in every_order(analysis.num_vars) {
            let cost = cost_of(&model, &order);
            if best.as_ref().is_none_or(|(least, _)| cost < *least) {
                best = Some((cost, order));
            }
        }
        let (least, cheapest) = best.expect("every query has an order");
        let plan = CostBasedOptimiser::default()
            .plan(q, &stats)
            .variable_ordering;
        Searched {
            plan_cost: cost_of(&model, &plan),
            plan,
            cheapest,
            least,
        }
    }

    /// The seeds each random-query test plans.
    const SEARCH_SEEDS: u64 = 300;

    /// The search is exact for its cost model: on random queries whose
    /// statistics span orders of magnitude, its plan costs what the
    /// cheapest order does, as brute force over every order finds it.
    /// Exactly, although `f64` rounds: the search sums each order's
    /// estimates in the order `cost_of` does, and rounding is monotonic, so
    /// no order can come out cheaper than the plan.
    #[test]
    #[cfg_attr(
        miri,
        ignore = "enumerates every order, minutes under Miri; the tests above run the search there"
    )]
    fn the_search_finds_the_cheapest_order() {
        let mut nontrivial = 0;
        for seed in 0..SEARCH_SEEDS {
            let (text, relations) = random_case(&mut Lcg(seed), Counts::Wide);
            let q: JoinQuery = text.parse().unwrap();
            let searched = search_and_enumerate(&q, &relations);
            assert_eq!(
                searched.plan_cost, searched.least,
                "seed {seed}: {text}: planned {:?}, cheapest {:?}",
                searched.plan, searched.cheapest
            );
            // The lexicographic plan is the canonical order 0, 1, 2, ...
            if searched.plan.iter().enumerate().any(|(i, &v)| i != v) {
                nontrivial += 1;
            }
        }
        // Observed: 215 of the 300 seeds. The floor fails the test if
        // `random_case` degenerates into queries the search plans trivially.
        assert!(
            nontrivial >= 160,
            "only {nontrivial} of {SEARCH_SEEDS} random cases plan off the canonical order"
        );
    }

    /// Among plans of equal cost the search takes the lexicographically
    /// smallest order, as brute force does, on random queries whose
    /// statistics make ties common and exact.
    #[test]
    #[cfg_attr(
        miri,
        ignore = "enumerates every order, minutes under Miri; the tests above run the search there"
    )]
    fn random_ties_take_the_lexicographically_smaller_order() {
        for seed in 0..SEARCH_SEEDS {
            let (text, relations) = random_case(&mut Lcg(seed), Counts::SmallPowersOfTwo);
            let q: JoinQuery = text.parse().unwrap();
            let searched = search_and_enumerate(&q, &relations);
            assert_eq!(searched.plan, searched.cheapest, "seed {seed}: {text}");
        }
    }
}
