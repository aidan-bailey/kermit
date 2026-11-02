//! The left-deep join order of a planned query.
//!
//! A variable order is a sequence of binding events: read left to right, each
//! variable not yet bound is bound by joining an atom that carries it.
//! [`join_order`] turns that reading into a left-deep join order over the
//! atoms of an access query ([`crate::extract_accesses`]), together with the
//! plan `V′` the same joins bind in. It reads only the atoms' variables and
//! the plan, never data, and belongs to no iterator family: index planning
//! applies it under `TrieLevels::ProbeKeys` ([`crate::plan_indexes`]).
//!
//! ```text
//! bound ← ∅;  order ← []
//! while some atom is unjoined:
//!     u ← the first variable of V not in bound
//!     a ← among the unjoined atoms containing u, the one with the most
//!         variables in bound (ties: the earliest in the body)
//!     join a;  bound ← bound ∪ vars(a)
//!     join, in body order, every unjoined atom whose variables are all in
//!         bound (pure filters, probed as soon as their key is complete)
//! V′ ← for each atom in order, its variables not bound before it joined,
//!      each group in V order
//! ```
//!
//! An atom's key is its variables bound before it joins. In `V′` every key
//! variable precedes the atom's new variables, so an index whose columns
//! follow `V′` has its key first. The first atom's key is empty, and so is
//! the key of an atom joined across a cross product: both are an empty key,
//! which selects every row.

// Temporary (Task 1.2 removes this line): `plan_indexes` is the first caller.
#![allow(dead_code)]

use {
    crate::{analysis::analyse, optimiser::QueryPlan},
    kermit_parser::JoinQuery,
    std::cmp::Reverse,
};

/// A left-deep join order over the atoms of an access query.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct JoinOrder {
    /// Indices into `query.body`, in join order.
    pub(crate) atoms: Vec<usize>,
    /// `key_lens[i]`: how many variables `atoms[i]` shares with `atoms[..i]`.
    pub(crate) key_lens: Vec<usize>,
    /// V′: the plan, over `query`'s canonical numbering, with every atom's
    /// key variables before its new ones.
    pub(crate) plan: QueryPlan,
}

/// The join order of `query` under the variable order `plan`, by the rule in
/// the module documentation.
///
/// # Panics
///
/// Panics if `plan` is not a plan of `query`: a variable of the plan that no
/// atom carries, or one it does not name.
pub(crate) fn join_order(query: &JoinQuery, plan: &QueryPlan) -> JoinOrder {
    let analysis = analyse(query);
    let atom_vars = &analysis.predicate_variables;
    let atom_count = atom_vars.len();
    let mut joiner = Joiner {
        plan: &plan.variable_ordering,
        atom_vars,
        bound: vec![false; analysis.num_vars],
        joined: vec![false; atom_count],
        atoms: Vec::with_capacity(atom_count),
        key_lens: Vec::with_capacity(atom_count),
        ordering: Vec::with_capacity(analysis.num_vars),
    };
    while joiner.atoms.len() < atom_count {
        // Every unjoined atom has an unbound variable (one with none was
        // joined as a filter), so the plan names one.
        let u = plan
            .variable_ordering
            .iter()
            .copied()
            .find(|&v| !joiner.bound[v])
            .expect("join_order: an unjoined atom has an unbound variable of the plan");
        let driver = (0..atom_count)
            .filter(|&a| !joiner.joined[a] && atom_vars[a].contains(&u))
            .min_by_key(|&a| (Reverse(joiner.bound_in(a)), a))
            .expect("join_order: some unjoined atom carries the plan's next variable");
        joiner.join(driver);
        for a in 0..atom_count {
            if !joiner.joined[a] && atom_vars[a].iter().all(|&v| joiner.bound[v]) {
                joiner.join(a);
            }
        }
    }
    assert_eq!(
        joiner.ordering.len(),
        plan.variable_ordering.len(),
        "join_order: the plan names a variable that no atom carries"
    );
    JoinOrder {
        atoms: joiner.atoms,
        key_lens: joiner.key_lens,
        plan: QueryPlan {
            variable_ordering: joiner.ordering,
        },
    }
}

/// The joins made so far: the atoms in join order with their key lengths,
/// and the variables they bound, in the order they bound them (`V′`).
struct Joiner<'a> {
    /// The plan `V`, in descent order.
    plan: &'a [usize],
    /// The canonical variables of each atom.
    atom_vars: &'a [Vec<usize>],
    /// `bound[v]`: whether a joined atom carries variable `v`.
    bound: Vec<bool>,
    /// `joined[a]`: whether atom `a` is in `atoms`.
    joined: Vec<bool>,
    atoms: Vec<usize>,
    key_lens: Vec<usize>,
    ordering: Vec<usize>,
}

impl<'a> Joiner<'a> {
    /// How many variables of `atom` are bound.
    fn bound_in(&self, atom: usize) -> usize {
        self.atom_vars[atom]
            .iter()
            .filter(|&&v| self.bound[v])
            .count()
    }

    /// Joins `atom`: its key is the variables already bound, and it binds
    /// the rest, in the order of `V`.
    fn join(&mut self, atom: usize) {
        let (plan, atom_vars): (&'a [usize], &'a [Vec<usize>]) = (self.plan, self.atom_vars);
        let vars = &atom_vars[atom];
        let key_len = self.bound_in(atom);
        self.key_lens.push(key_len);
        for &v in plan {
            if vars.contains(&v) && !self.bound[v] {
                self.bound[v] = true;
                self.ordering.push(v);
            }
        }
        self.atoms.push(atom);
        self.joined[atom] = true;
    }
}

/// The 16 standard join patterns' queries and the variable orders to run
/// them under, shared by the tests of this module and of `index_plan`.
#[cfg(test)]
pub(crate) mod suite {
    use {
        crate::{extract_accesses, rewrite_placeholders, AccessQuery},
        kermit_parser::JoinQuery,
    };

    /// `(name, query)` of the 16 standard patterns, as in
    /// `kermit/tests/all_orders.rs`.
    pub(crate) const QUERIES: [(&str, &str); 16] = [
        ("unary", "Q(V0) :- r0(V0)."),
        (
            "triangle",
            "Q(V0, V1, V2) :- r0(V0, V1), r1(V1, V2), r2(V0, V2).",
        ),
        (
            "chain",
            "Q(V0, V1, V2, V3) :- r0(V0, V1), r1(V1, V2), r2(V2, V3).",
        ),
        ("star", "Q(V0, V1, V2) :- r0(V0, V1), r1(V0, V2)."),
        ("selfjoin", "Q(V0, V1, V2) :- r0(V0, V1), r0(V1, V2)."),
        ("existential", "Q(V0) :- r0(V0), r1(V0)."),
        ("empty-result", "Q(V0, V1, V2) :- r0(V0, V1), r1(V1, V2)."),
        ("single-relation", "Q(V0, V1) :- r0(V0, V1)."),
        (
            "four-way-chain",
            "Q(V0, V1, V2, V3, V4) :- r0(V0, V1), r1(V1, V2), r2(V2, V3), r3(V3, V4).",
        ),
        ("wide-fanout", "Q(V0, V1, V2) :- r0(V0, V1), r1(V0, V2)."),
        ("dead-end", "Q(V0, V1, V2) :- r0(V0, V1), r1(V1, V2)."),
        (
            "deep-dead-end",
            "Q(V0, V1, V2, V3) :- r0(V0, V1), r1(V1, V2), r2(V2, V3).",
        ),
        ("diagonal", "Q(V0) :- r0(V0, V0)."),
        (
            "repeated-nonadjacent",
            "Q(V0, V1) :- r0(V0, V1, V0), r1(V1).",
        ),
        ("trailing-placeholder", "Q(V0) :- r0(V0, _), r1(V0)."),
        ("middle-placeholder", "Q(V0, V2) :- r0(V0, _, V2), r1(V2)."),
    ];

    /// `text` as the access query `kermit::db::prepare` plans over: the
    /// placeholders rewritten, then the accesses extracted.
    pub(crate) fn accesses_of(text: &str) -> AccessQuery {
        let query: JoinQuery = text.parse().expect("parse");
        extract_accesses(&rewrite_placeholders(query)).expect("extract")
    }

    /// Every permutation of `0..n`: the variable orders a plan can take.
    pub(crate) fn permutations(n: usize) -> Vec<Vec<usize>> {
        fn extend(prefix: &mut Vec<usize>, left: &[usize], out: &mut Vec<Vec<usize>>) {
            if left.is_empty() {
                out.push(prefix.clone());
                return;
            }
            for (at, &variable) in left.iter().enumerate() {
                let mut rest = left.to_vec();
                rest.remove(at);
                prefix.push(variable);
                extend(prefix, &rest, out);
                prefix.pop();
            }
        }
        let mut out = Vec::new();
        extend(&mut Vec::new(), &(0..n).collect::<Vec<_>>(), &mut out);
        out
    }
}

#[cfg(test)]
mod tests {
    use {
        super::{
            suite::{accesses_of, permutations, QUERIES},
            *,
        },
        crate::{analyse, canonical_names},
        std::collections::HashSet,
    };

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

    /// The join order of `text` under the variable order `order`.
    fn joined(text: &str, order: &[&str]) -> JoinOrder {
        let accesses = accesses_of(text);
        let plan = plan_of(&accesses.query, order);
        join_order(&accesses.query, &plan)
    }

    fn expected(atoms: &[usize], key_lens: &[usize], ordering: &[usize]) -> JoinOrder {
        JoinOrder {
            atoms: atoms.to_vec(),
            key_lens: key_lens.to_vec(),
            plan: QueryPlan {
                variable_ordering: ordering.to_vec(),
            },
        }
    }

    /// Worked example 1: a tie between `R` and `T` goes to the earlier atom,
    /// and `T` is probed on both its variables once `S` has bound `Z`.
    #[test]
    fn a_triangle_joins_in_body_order_with_growing_keys() {
        let got = joined("Q(A, B, C) :- R(A, B), S(B, C), T(A, C).", &["A", "B", "C"]);
        assert_eq!(got, expected(&[0, 1, 2], &[0, 1, 2], &[0, 1, 2]));
    }

    /// Worked example 2: `B(y, z)` is probed on `z`, which `A(x, z)` bound
    /// before `y`, so `z` moves ahead of `y` in `V′`.
    #[test]
    fn a_shared_variable_is_bound_before_the_variable_that_precedes_it() {
        let got = joined("Q(X, Y, Z) :- P(X, Z), S(Y, Z).", &["X", "Y", "Z"]);
        // X = 0, Y = 1, Z = 2; V′ = [X, Z, Y].
        assert_eq!(got, expected(&[0, 1], &[0, 1], &[0, 2, 1]));
    }

    /// Worked example 3: atoms that share nothing join as a cross product,
    /// each with an empty key, and `V′` groups each atom's variables.
    #[test]
    fn atoms_that_share_nothing_join_as_a_cross_product() {
        let got = joined("Q(A, B, C, D) :- R(A, B), S(C, D).", &["A", "C", "B", "D"]);
        assert_eq!(got, expected(&[0, 1], &[0, 0], &[0, 1, 2, 3]));
    }

    /// The rule follows `V`: with `D` first among the unbound variables, `T`
    /// (which carries it, and shares nothing bound) beats `S` (which shares
    /// `B` but does not carry `D`), so `V` forces a cross product, as it does
    /// for LFTJ. `S` is then probed as a filter on both its variables.
    #[test]
    fn a_connected_atom_without_the_next_variable_loses_to_an_unconnected_one_with_it() {
        let got = joined("Q(A, B, C, D) :- R(A, B), S(B, C), T(D, C).", &[
            "A", "D", "B", "C",
        ]);
        // A = 0, B = 1, C = 2, D = 3; V′ = [A, B, D, C].
        assert_eq!(got, expected(&[0, 2, 1], &[0, 0, 2], &[0, 1, 3, 2]));
    }

    /// Among the atoms that carry the next variable, the one with the most
    /// bound variables wins over an earlier one with fewer.
    #[test]
    fn the_atom_with_the_most_bound_variables_wins_over_an_earlier_one() {
        let got = joined("Q(A, B, C, D) :- R(A, B), S(C, D), T(C, B, D).", &[
            "A", "B", "C", "D",
        ]);
        // After R, C is next: S has no bound variable, T has B.
        assert_eq!(got, expected(&[0, 2, 1], &[0, 1, 2], &[0, 1, 2, 3]));
    }

    /// An atom whose variables are all bound is probed at once, in body
    /// order, before the next unbound variable picks its atom.
    #[test]
    fn a_complete_atom_is_probed_as_soon_as_its_key_is_bound() {
        let got = joined("Q(X, Y, Z) :- R(X, Y), S(Z), T(X), U(Y, Z).", &[
            "X", "Y", "Z",
        ]);
        // R drives and binds X, Y, so T is probed at once; Z then goes to U
        // (Y is bound) rather than S, and S is probed last.
        assert_eq!(got, expected(&[0, 2, 3, 1], &[0, 1, 1, 1], &[0, 1, 2]));
    }

    #[test]
    fn a_query_with_no_accesses_has_the_empty_order() {
        let mut query: JoinQuery = "Q(c9) :- r0(c1, c2), s(c3).".parse().expect("parse");
        query.head.terms.clear();
        let accesses =
            crate::extract_accesses(&crate::rewrite_placeholders(query)).expect("extract");
        assert!(accesses.accesses.is_empty());
        let plan = QueryPlan {
            variable_ordering: vec![],
        };
        assert_eq!(join_order(&accesses.query, &plan), expected(&[], &[], &[]));
    }

    /// Checks the properties of the rule on one result, without re-running
    /// the rule: it reads `ordered` back against `query` and `order`.
    fn check(context: &str, query: &JoinQuery, order: &[usize], ordered: &JoinOrder) {
        let vars = analyse(query).predicate_variables;
        let prime = &ordered.plan.variable_ordering;

        // V′ is a permutation, and each atom joins once.
        let mut sorted = prime.clone();
        sorted.sort_unstable();
        assert_eq!(
            sorted,
            (0..order.len()).collect::<Vec<_>>(),
            "{context}: V′ is a permutation"
        );
        let mut once = ordered.atoms.clone();
        once.sort_unstable();
        assert_eq!(
            once,
            (0..vars.len()).collect::<Vec<_>>(),
            "{context}: each atom joins once"
        );
        assert_eq!(ordered.key_lens.len(), ordered.atoms.len(), "{context}");

        let position_in = |ordering: &[usize]| -> Vec<usize> {
            let mut position = vec![0; ordering.len()];
            for (at, &variable) in ordering.iter().enumerate() {
                position[variable] = at;
            }
            position
        };
        let in_prime = position_in(prime);
        let in_order = position_in(order);

        let mut bound: HashSet<usize> = HashSet::new();
        let mut rebuilt: Vec<usize> = Vec::new();
        let mut previous_filter: Option<usize> = None;
        for (rank, &atom) in ordered.atoms.iter().enumerate() {
            // The key is a prefix of the atom's V′-ordered variables, and it
            // is exactly the variables an earlier atom bound.
            let mut by_prime = vars[atom].clone();
            by_prime.sort_by_key(|&v| in_prime[v]);
            let key: Vec<usize> = by_prime
                .iter()
                .copied()
                .filter(|v| bound.contains(v))
                .collect();
            assert_eq!(
                key.len(),
                ordered.key_lens[rank],
                "{context}: the key length of atom {atom}"
            );
            assert_eq!(
                by_prime[..key.len()],
                key[..],
                "{context}: the key of atom {atom} leads its columns"
            );
            let fresh = &by_prime[key.len()..];
            assert!(
                fresh
                    .windows(2)
                    .all(|pair| in_order[pair[0]] < in_order[pair[1]]),
                "{context}: the new variables of atom {atom} are in V order"
            );
            rebuilt.extend_from_slice(fresh);

            if fresh.is_empty() {
                // A pure filter: filters that follow one another are in body
                // order.
                if let Some(previous) = previous_filter {
                    assert!(previous < atom, "{context}: filters are in body order");
                }
                previous_filter = Some(atom);
            } else {
                previous_filter = None;
                // A driving join: it carries the first unbound variable of
                // V, no unjoined atom is complete (it would have been probed
                // already), and no unjoined atom that carries that variable
                // has more bound variables, or as many and an earlier place
                // in the body.
                let u = order
                    .iter()
                    .copied()
                    .find(|v| !bound.contains(v))
                    .expect("an unbound variable");
                assert!(
                    vars[atom].contains(&u),
                    "{context}: atom {atom} carries {u}"
                );
                let bound_in = |a: usize| vars[a].iter().filter(|v| bound.contains(v)).count();
                for &other in &ordered.atoms[rank + 1..] {
                    assert!(
                        !vars[other].iter().all(|v| bound.contains(v)),
                        "{context}: atom {other} was complete before atom {atom} joined"
                    );
                    if vars[other].contains(&u) {
                        let (mine, theirs) = (bound_in(atom), bound_in(other));
                        assert!(
                            theirs < mine || (theirs == mine && atom < other),
                            "{context}: atom {other} beats atom {atom} on variable {u}"
                        );
                    }
                }
            }
            bound.extend(vars[atom].iter().copied());
        }
        assert_eq!(
            &rebuilt, prime,
            "{context}: V′ is each atom's new variables, in join order"
        );
    }

    /// The properties on the 16 suite shapes under every variable order
    /// (the number of orders is the sum of `n!` over their variable counts:
    /// 1 + 6 + 24 + 6 + 6 + 1 + 6 + 2 + 120 + 6 + 6 + 24 + 1 + 2 + 2 + 6).
    #[test]
    fn the_properties_hold_on_every_suite_shape_under_every_order() {
        let mut checked = 0usize;
        for (name, text) in QUERIES {
            let accesses = accesses_of(text);
            let query = &accesses.query;
            for order in permutations(analyse(query).num_vars) {
                let plan = QueryPlan {
                    variable_ordering: order.clone(),
                };
                let ordered = join_order(query, &plan);
                check(&format!("{name} under {order:?}"), query, &order, &ordered);
                checked += 1;
            }
        }
        assert_eq!(checked, 219);
    }
}
