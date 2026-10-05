//! The orientation rewrite: makes `--column-orders any` look like `stored`
//! to the executors.
//!
//! Leapfrog Triejoin opens an atom's k-th trie level for its k-th variable
//! in descent order, and Hash Triejoin maps leaf tuples positionally, so
//! both need every atom's term order to equal its trie's column order
//! (`QueryPlan::validate` rejects a plan that would break that). Under
//! [`ColumnOrderPolicy::Any`] the planner ignores stored column orders, so
//! after planning this pass permutes each disagreeing atom's terms into
//! plan order and points the atom at a *copy* of its relation with the
//! columns permuted the same way, `Index_<π>_<base>`. The catalog
//! (`kermit::db::Database`) builds and holds those copies per query; the
//! executors and the data structures never learn about the policy. Runs
//! after the const, placeholder and selection rewrites and after
//! planning, immediately before `JoinAlgo::join_for_each`.

use {
    crate::{
        analysis::{analyse, canonical_names},
        const_rewrite::is_const_predicate,
        optimiser::{ColumnOrderPolicy, QueryPlan},
        selection_rewrite::{ColumnEquality, SelectionSpec},
    },
    kermit_parser::JoinQuery,
    std::collections::{BTreeMap, HashMap},
};

/// Prefix of the synthetic predicate names this rewrite introduces (e.g.
/// `Index_1_0_edge`). Load-bearing beyond this module: `kermit::db`
/// resolves such a name in its copy store rather than among the base
/// relations, and rejects it in a user's query.
pub const INDEX_PREDICATE_PREFIX: &str = "Index_";

/// Returns `true` iff `name` names a reordered copy introduced by
/// [`orient`].
pub fn is_index_predicate(name: &str) -> bool { name.starts_with(INDEX_PREDICATE_PREFIX) }

/// A relation copy with its columns permuted: column `i` of the copy is
/// column `permutation[i]` of `base`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexSpec {
    /// The synthetic predicate name, `Index_<π joined by _>_<base>`, e.g.
    /// `Index_1_0_edge`.
    pub name: String,
    /// The relation the copy is built from, e.g. `edge`.
    pub base: String,
    /// `permutation[i]` is the base column that becomes column `i`.
    pub permutation: Vec<usize>,
}

impl IndexSpec {
    /// The copy of `base` under `permutation`, named by both.
    pub fn new(base: &str, permutation: Vec<usize>) -> Self {
        let digits: Vec<String> = permutation.iter().map(usize::to_string).collect();
        Self {
            name: format!("{INDEX_PREDICATE_PREFIX}{}_{base}", digits.join("_")),
            base: base.to_string(),
            permutation,
        }
    }

    /// `tuple` of the base relation, reordered into the copy's columns.
    pub fn permute(&self, tuple: &[usize]) -> Vec<usize> {
        self.permutation
            .iter()
            .map(|&column| tuple[column])
            .collect()
    }

    /// Every tuple of the base relation, reordered, in the same order.
    pub fn permute_all(&self, tuples: &[Vec<usize>]) -> Vec<Vec<usize>> {
        tuples.iter().map(|tuple| self.permute(tuple)).collect()
    }

    /// The copy for a human: `edge (1, 0)`. The `bench run` metadata line.
    pub fn describe(&self) -> String {
        let digits: Vec<String> = self.permutation.iter().map(usize::to_string).collect();
        format!("{} ({})", self.base, digits.join(", "))
    }
}

/// The executor-ready form of a planned query: the query the executors
/// run, its plan over that query's numbering, the copies it needs and the
/// selection views, which now point at a copy where their atom was
/// reoriented.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Oriented {
    /// The query to execute.
    pub query: JoinQuery,
    /// `plan`, renumbered for `query`.
    pub plan: QueryPlan,
    /// The copies `query` reads, one per distinct `(base, permutation)`,
    /// in body order of first use. Empty under `stored`.
    pub index_specs: Vec<IndexSpec>,
    /// The selection views, with `relation` and `equalities` moved to the
    /// copy where the view's atom was reoriented.
    pub selection_specs: Vec<SelectionSpec>,
}

/// Orients `query` to `plan` under `policy`.
///
/// Under [`ColumnOrderPolicy::Stored`] everything is returned unchanged
/// with no copies: the plan already binds every atom left to right.
/// Under [`ColumnOrderPolicy::Any`], for each body atom other than a
/// `Const_*` singleton, π is its columns sorted by plan position (every
/// atom's variables are distinct after the selection rewrite, so the
/// positions are). An identity π leaves the atom alone. Otherwise the
/// atom's terms are permuted; a plain atom is renamed to the copy, and a
/// `Select_<n>_<base>` atom keeps its name while its [`SelectionSpec`]
/// points at the copy, each of its equality classes carried through π and
/// re-rooted at its new first column (the selection views are positional
/// and keep one source per column). Atoms with equal `(base, π)` share
/// one [`IndexSpec`].
///
/// `analyse` numbers body-only variables by first appearance, so
/// permuting terms can renumber them: the plan is translated by variable
/// name through a fresh analysis of the oriented query, and in debug
/// builds checked to validate against it.
///
/// `query` must be the rewritten query (one variable per column), and
/// `plan` its plan.
pub fn orient(
    policy: ColumnOrderPolicy, mut query: JoinQuery, plan: QueryPlan,
    mut selection_specs: Vec<SelectionSpec>,
) -> Oriented {
    if policy == ColumnOrderPolicy::Stored {
        return Oriented {
            query,
            plan,
            index_specs: Vec::new(),
            selection_specs,
        };
    }

    let analysis = analyse(&query);
    let names = canonical_names(&query);
    let mut position = vec![usize::MAX; analysis.num_vars];
    for (pos, &v) in plan.variable_ordering.iter().enumerate() {
        position[v] = pos;
    }
    let plan_names: Vec<&str> = plan
        .variable_ordering
        .iter()
        .map(|&v| names[v].as_str())
        .collect();
    let selection_of: HashMap<String, usize> = selection_specs
        .iter()
        .enumerate()
        .map(|(i, spec)| (spec.name.clone(), i))
        .collect();

    let mut index_specs: Vec<IndexSpec> = Vec::new();
    for (atom, vars) in query.body.iter_mut().zip(&analysis.predicate_variables) {
        if is_const_predicate(&atom.name) {
            continue;
        }
        debug_assert_eq!(
            vars.len(),
            atom.terms.len(),
            "orient reads the rewritten query: one variable per column"
        );
        let mut pi: Vec<usize> = (0..vars.len()).collect();
        pi.sort_by_key(|&column| position[vars[column]]);
        if pi.iter().enumerate().all(|(i, &column)| i == column) {
            continue;
        }
        atom.terms = pi
            .iter()
            .map(|&column| atom.terms[column].clone())
            .collect();

        let selection = selection_of.get(&atom.name).copied();
        let base = match selection {
            | Some(i) => selection_specs[i].relation.clone(),
            | None => atom.name.clone(),
        };
        let spec = IndexSpec::new(&base, pi.clone());
        if !index_specs.iter().any(|s| s.name == spec.name) {
            index_specs.push(spec.clone());
        }
        match selection {
            | Some(i) => {
                let view = &mut selection_specs[i];
                view.relation = spec.name;
                view.equalities = remap_equalities(&view.equalities, &pi);
            },
            | None => atom.name = spec.name,
        }
    }

    // Translate the plan by name: the oriented query may number its
    // body-only variables differently.
    let renumbered = canonical_names(&query);
    let index_of: HashMap<&str, usize> = renumbered
        .iter()
        .enumerate()
        .map(|(i, name)| (name.as_str(), i))
        .collect();
    let plan = QueryPlan {
        variable_ordering: plan_names.iter().map(|name| index_of[name]).collect(),
    };
    debug_assert_eq!(
        plan.validate(&analyse(&query)),
        Ok(()),
        "an oriented query's plan binds every atom left to right"
    );

    Oriented {
        query,
        plan,
        index_specs,
        selection_specs,
    }
}

/// A selection view's `equalities` after its atom's columns are permuted
/// by `pi` (column `i` of the copy is column `pi[i]` of the base), in the
/// form the selection rewrite produces and the views read: one equality
/// per repeat column, whose source is the first column of its class.
///
/// The rewrite emits one class per repeated variable, a star from the
/// variable's first column to each later one. Mapping each pair through π
/// on its own would not keep that shape: when π moves a class's first
/// column behind another member, two equalities land on one repeat
/// column, and `EqualitySelectionTrieIter` / `EqualitySelectionHashTrieIter`
/// keep a single source per column, so one constraint would be lost.
/// Each class is therefore mapped as a whole and re-rooted at its new
/// first column.
fn remap_equalities(equalities: &[ColumnEquality], pi: &[usize]) -> Vec<ColumnEquality> {
    // `inverse[old] = new`: where each base column went.
    let mut inverse = vec![0; pi.len()];
    for (new, &old) in pi.iter().enumerate() {
        inverse[old] = new;
    }
    let mut classes: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for equality in equalities {
        classes
            .entry(equality.source)
            .or_insert_with(|| vec![equality.source])
            .push(equality.repeat);
    }
    let mut remapped: Vec<ColumnEquality> = classes
        .into_values()
        .flat_map(|class| {
            let mut columns: Vec<usize> = class.iter().map(|&column| inverse[column]).collect();
            columns.sort_unstable();
            let source = columns[0];
            columns
                .into_iter()
                .skip(1)
                .map(move |repeat| ColumnEquality {
                    source,
                    repeat,
                })
        })
        .collect();
    remapped.sort_by_key(|equality| equality.repeat);
    remapped
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::{
            optimiser::CatalogStats, rewrite_atoms, rewrite_placeholders,
            rewrite_repeated_variables, LexicographicOptimiser, QueryOptimiser,
        },
    };

    /// `text` after the three rewrites, with its selection views.
    fn rewritten(text: &str) -> (JoinQuery, Vec<SelectionSpec>) {
        let (query, _) = rewrite_atoms(text.parse().expect("parse")).expect("const rewrite");
        rewrite_repeated_variables(rewrite_placeholders(query))
    }

    /// `plan` as canonical indices of `query`, from variable names.
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

    fn body_text(query: &JoinQuery) -> Vec<String> {
        query.body.iter().map(ToString::to_string).collect()
    }

    fn eq(source: usize, repeat: usize) -> ColumnEquality {
        ColumnEquality {
            source,
            repeat,
        }
    }

    #[test]
    fn stored_returns_its_input_with_no_copies() {
        let (query, specs) = rewritten("Q(X, Y) :- edge(Y, X).");
        let plan = plan_of(&query, &["X", "Y"]);
        let out = orient(
            ColumnOrderPolicy::Stored,
            query.clone(),
            plan.clone(),
            specs.clone(),
        );
        assert_eq!(out, Oriented {
            query,
            plan,
            index_specs: vec![],
            selection_specs: specs,
        });
    }

    #[test]
    fn an_atom_whose_plan_disagrees_is_renamed_and_permuted() {
        let (query, specs) = rewritten("Q(X, Y) :- edge(Y, X).");
        let out = orient(
            ColumnOrderPolicy::Any,
            query.clone(),
            plan_of(&query, &["X", "Y"]),
            specs,
        );
        assert_eq!(body_text(&out.query), vec!["Index_1_0_edge(X, Y)"]);
        assert_eq!(out.index_specs, vec![IndexSpec::new("edge", vec![1, 0])]);
        assert_eq!(out.index_specs[0].name, "Index_1_0_edge");
        assert_eq!(out.plan.variable_ordering, vec![0, 1]);
    }

    #[test]
    fn an_atom_that_agrees_is_untouched() {
        let (query, specs) = rewritten("Q(X, Y, Z) :- R(X, Y), S(Y, Z), T(X, Z).");
        let out = orient(
            ColumnOrderPolicy::Any,
            query.clone(),
            plan_of(&query, &["X", "Y", "Z"]),
            specs,
        );
        assert_eq!(out.query, query);
        assert!(out.index_specs.is_empty());
    }

    #[test]
    fn equal_base_and_permutation_share_one_copy() {
        let (query, specs) = rewritten("Q(X, Y, Z) :- edge(Y, X), edge(Z, X).");
        let out = orient(
            ColumnOrderPolicy::Any,
            query.clone(),
            plan_of(&query, &["X", "Y", "Z"]),
            specs,
        );
        assert_eq!(body_text(&out.query), vec![
            "Index_1_0_edge(X, Y)",
            "Index_1_0_edge(X, Z)"
        ]);
        assert_eq!(out.index_specs, vec![IndexSpec::new("edge", vec![1, 0])]);
    }

    #[test]
    fn two_permutations_of_one_base_are_two_copies() {
        let (query, specs) = rewritten("Q(X, Y, Z) :- r(Y, X, Z), r(Z, X, Y).");
        let out = orient(
            ColumnOrderPolicy::Any,
            query.clone(),
            plan_of(&query, &["X", "Y", "Z"]),
            specs,
        );
        assert_eq!(body_text(&out.query), vec![
            "Index_1_0_2_r(X, Y, Z)",
            "Index_1_2_0_r(X, Y, Z)"
        ]);
        assert_eq!(out.index_specs, vec![
            IndexSpec::new("r", vec![1, 0, 2]),
            IndexSpec::new("r", vec![1, 2, 0]),
        ]);
    }

    #[test]
    fn const_singletons_are_never_oriented() {
        // `edge(c5, X)` rewrites to `edge(K0, X), Const_c5(K0)`; the plan
        // binds X first.
        let (query, specs) = rewritten("Q(X) :- edge(c5, X).");
        let out = orient(
            ColumnOrderPolicy::Any,
            query.clone(),
            plan_of(&query, &["X", "K0"]),
            specs,
        );
        assert_eq!(body_text(&out.query), vec![
            "Index_1_0_edge(X, K0)",
            "Const_c5(K0)"
        ]);
        assert_eq!(out.index_specs.len(), 1);
    }

    /// `r(X, X, Y)` is `Select_0_r(X, K0, Y)` with the equality (0, 1).
    /// Binding Y first permutes the atom to (Y, X, K0): the view points
    /// at `Index_2_0_1_r` and the equality becomes (1, 2).
    #[test]
    fn a_selection_view_points_at_its_copy_and_remaps_its_equality() {
        let (query, specs) = rewritten("Q(X, Y) :- r(X, X, Y).");
        assert_eq!(specs[0].equalities, vec![eq(0, 1)]);
        let out = orient(
            ColumnOrderPolicy::Any,
            query.clone(),
            plan_of(&query, &["Y", "X", "K0"]),
            specs,
        );
        assert_eq!(body_text(&out.query), vec!["Select_0_r(Y, X, K0)"]);
        assert_eq!(out.index_specs, vec![IndexSpec::new("r", vec![2, 0, 1])]);
        assert_eq!(out.selection_specs[0].name, "Select_0_r");
        assert_eq!(out.selection_specs[0].relation, "Index_2_0_1_r");
        assert_eq!(out.selection_specs[0].equalities, vec![eq(1, 2)]);
    }

    /// When the repeat column comes first in the plan, the remapped pair is
    /// re-sorted so the source is still the earlier column.
    #[test]
    fn a_remapped_equality_keeps_its_source_first() {
        let (query, specs) = rewritten("Q(X, Y) :- r(X, Y, X).");
        // Select_0_r(X, Y, K0), equality (0, 2). Plan Y, K0, X: π = [1, 2,
        // 0], which sends the source column 0 to 2 and the repeat column 2
        // to 1, so the pair arrives as (2, 1) and is re-sorted.
        let out = orient(
            ColumnOrderPolicy::Any,
            query.clone(),
            plan_of(&query, &["Y", "K0", "X"]),
            specs,
        );
        assert_eq!(body_text(&out.query), vec!["Select_0_r(Y, K0, X)"]);
        assert_eq!(out.selection_specs[0].relation, "Index_1_2_0_r");
        assert_eq!(out.selection_specs[0].equalities, vec![eq(1, 2)]);
    }

    /// `r(X, X, X)` is `Select_0_r(X, K0, K1)` with (0, 1) and (0, 2).
    /// Reversed, mapping each pair on its own gives (1, 2) and (0, 2): two
    /// equalities on column 2, of which a view keeps one, so `col0 = col1`
    /// would be lost. The class is re-rooted at its new first column.
    #[test]
    fn a_triple_repeat_keeps_one_source_per_repeat_column() {
        let (query, specs) = rewritten("Q(X) :- r(X, X, X).");
        assert_eq!(specs[0].equalities, vec![eq(0, 1), eq(0, 2)]);
        let out = orient(
            ColumnOrderPolicy::Any,
            query.clone(),
            plan_of(&query, &["K1", "K0", "X"]),
            specs,
        );
        assert_eq!(body_text(&out.query), vec!["Select_0_r(K1, K0, X)"]);
        assert_eq!(out.selection_specs[0].equalities, vec![eq(0, 1), eq(0, 2)]);
    }

    /// Two repeated variables are two classes, each re-rooted on its own.
    #[test]
    fn two_repeated_variables_remap_as_two_classes() {
        // r(X, Y, X, Y) is Select_0_r(X, Y, K0, K1), (0, 2) and (1, 3).
        // Plan Y, K1, X, K0: π = [1, 3, 0, 2], so X's class {0, 2} lands
        // on {2, 3} and Y's class {1, 3} on {0, 1}.
        let (query, specs) = rewritten("Q(X, Y) :- r(X, Y, X, Y).");
        assert_eq!(specs[0].equalities, vec![eq(0, 2), eq(1, 3)]);
        let out = orient(
            ColumnOrderPolicy::Any,
            query.clone(),
            plan_of(&query, &["Y", "K1", "X", "K0"]),
            specs,
        );
        assert_eq!(body_text(&out.query), vec!["Select_0_r(Y, K1, X, K0)"]);
        assert_eq!(out.selection_specs[0].equalities, vec![eq(0, 1), eq(2, 3)]);
    }

    /// `r(X, A, B)` reoriented to `(X, B, A)` makes `B` the first body-only
    /// variable, so its canonical index changes; the plan follows the
    /// names, not the old indices.
    #[test]
    fn the_plan_is_translated_by_name() {
        let (query, specs) = rewritten("Q(X) :- r(X, A, B), s(B, A).");
        // Canonical: X = 0, A = 1, B = 2. Plan X, B, A = [0, 2, 1].
        let plan = plan_of(&query, &["X", "B", "A"]);
        assert_eq!(plan.variable_ordering, vec![0, 2, 1]);
        let out = orient(ColumnOrderPolicy::Any, query, plan, specs);
        assert_eq!(body_text(&out.query), vec![
            "Index_0_2_1_r(X, B, A)",
            "s(B, A)"
        ]);
        // Now B = 1, A = 2, and the plan still reads X, B, A.
        assert_eq!(canonical_names(&out.query), vec!["X", "B", "A"]);
        assert_eq!(out.plan.variable_ordering, vec![0, 1, 2]);
        assert_eq!(out.plan.validate(&analyse(&out.query)), Ok(()));
    }

    /// End to end with a real optimiser: the cyclic-under-`stored` query
    /// orients to one copy and a plan the executors accept.
    #[test]
    fn a_cyclic_query_orients_to_a_valid_plan() {
        let (query, specs) = rewritten("Q(X, Y) :- edge(X, Y), edge(Y, X).");
        let stats = CatalogStats::for_query(&query, ColumnOrderPolicy::Any, |_| None);
        let plan = LexicographicOptimiser.plan(&query, &stats);
        let out = orient(ColumnOrderPolicy::Any, query, plan, specs);
        assert_eq!(body_text(&out.query), vec![
            "edge(X, Y)",
            "Index_1_0_edge(X, Y)"
        ]);
        assert_eq!(out.plan.validate(&analyse(&out.query)), Ok(()));
    }

    #[test]
    fn specs_permute_tuples_and_describe_themselves() {
        let spec = IndexSpec::new("edge", vec![1, 0]);
        assert_eq!(spec.permute(&[7, 9]), vec![9, 7]);
        assert_eq!(spec.permute_all(&[vec![1, 2], vec![3, 4]]), vec![
            vec![2, 1],
            vec![4, 3]
        ]);
        assert_eq!(spec.describe(), "edge (1, 0)");
        assert!(is_index_predicate(&spec.name));
        assert!(!is_index_predicate("edge"));
    }
}
