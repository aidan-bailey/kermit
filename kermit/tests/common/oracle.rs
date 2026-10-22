//! A nested-loop oracle and the seeded generator its tests draw from, shared
//! by the tests that compare an executor's rows with an evaluator that has no
//! planner, rewrite or trie in it.

use {
    kermit_algos::JoinQuery,
    kermit_ds::{Cardinality, Relation, RelationHeader},
    kermit_parser::{Predicate, Term},
    std::collections::BTreeMap,
};

/// A 64-bit linear congruential generator (Knuth's MMIX constants).
pub struct Lcg(pub u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }

    /// A value in `0..n`.
    pub fn below(&mut self, n: usize) -> usize { (self.next() % n as u64) as usize }
}

/// Relations as `(name, arity, tuples)`.
pub type Relations = Vec<(String, usize, Vec<Vec<usize>>)>;

/// `tuples` as `R` holds them: the hash trie keeps duplicates (a
/// multiset), the sorted tries drop them (a set), and `R`'s tuple count
/// says which.
fn as_held<R: Relation + Cardinality>(
    name: &str, arity: usize, tuples: &[Vec<usize>],
) -> Vec<Vec<usize>> {
    let held = R::from_tuples(RelationHeader::new_positional(name, arity), tuples.to_vec());
    let mut distinct = tuples.to_vec();
    distinct.sort();
    distinct.dedup();
    if held.tuple_count() == tuples.len() {
        tuples.to_vec()
    } else {
        assert_eq!(
            held.tuple_count(),
            distinct.len(),
            "{name}: neither a set nor a bag"
        );
        distinct
    }
}

/// The rows `query` must return over `relations` as `R` holds them, by
/// nested loops in body order: each way of matching every atom to one held
/// tuple, a variable taking one value throughout and a constant `c<k>`
/// matching `k`, is one row, projected to the head. Bag semantics, so a
/// duplicate tuple matches twice. No planner, rewrite or executor is
/// involved, so this checks the rows of every query the executors accept.
pub fn oracle<R: Relation + Cardinality>(
    relations: &Relations, query: &JoinQuery,
) -> Vec<Vec<usize>> {
    let held: BTreeMap<&str, Vec<Vec<usize>>> = relations
        .iter()
        .map(|(name, arity, tuples)| (name.as_str(), as_held::<R>(name, *arity, tuples)))
        .collect();
    let mut rows = Vec::new();
    match_atoms(
        &query.body,
        &held,
        &mut BTreeMap::new(),
        &query.head,
        &mut rows,
    );
    rows.sort();
    rows
}

/// Matches `atoms` in order under `binding`, pushing one head row per
/// complete match.
fn match_atoms<'q>(
    atoms: &'q [Predicate], held: &BTreeMap<&str, Vec<Vec<usize>>>,
    binding: &mut BTreeMap<&'q str, usize>, head: &Predicate, rows: &mut Vec<Vec<usize>>,
) {
    let Some((atom, rest)) = atoms.split_first() else {
        let row = head
            .terms
            .iter()
            .map(|term| match term {
                | Term::Var(v) => binding[v.as_str()],
                | _ => unreachable!("the generator's head is variables"),
            })
            .collect();
        rows.push(row);
        return;
    };
    for tuple in &held[atom.name.as_str()] {
        let mut bound_here: Vec<&str> = Vec::new();
        let matches = atom
            .terms
            .iter()
            .zip(tuple)
            .all(|(term, &value)| match term {
                | Term::Var(v) => match binding.get(v.as_str()) {
                    | Some(&bound) => bound == value,
                    | None => {
                        binding.insert(v.as_str(), value);
                        bound_here.push(v.as_str());
                        true
                    },
                },
                | Term::Atom(c) => c[1..].parse() == Ok(value),
                | Term::Placeholder => true,
            });
        if matches {
            match_atoms(rest, held, binding, head, rows);
        }
        for v in bound_here {
            binding.remove(v);
        }
    }
}
