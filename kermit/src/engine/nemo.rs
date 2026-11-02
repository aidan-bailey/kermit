//! Nemo, in-process: the [`Engine`](super::Engine) behind the `engine-nemo`
//! feature.
//!
//! [Nemo](https://github.com/knowsys/nemo) answers Datalog with leapfrog triejoins over
//! column-oriented tries, the family `ColumnTrie` belongs to. This module
//! drives its `nemo-physical` crate through the public API only, at the pinned
//! revision [`NEMO_REV`]. How kermit's phases map onto it:
//!
//! - **`load`** (untimed): shared handles on the tuples of each relation the
//!   query reads (`Catalog::shared_tuples`), so Nemo's loader streams from the
//!   catalog's one copy.
//! - **`build`** (timed): a fresh `DatabaseInstance`. For each index spec a
//!   `ViewProvider` streams the spec's view into Nemo's loader as integer
//!   `AnyDataValue`s, and `load_trie_into_memory` sorts, deduplicates and
//!   builds its trie (the one copy). The join is then planned, in kermit's
//!   variable order.
//! - **`materialize`** (timed, opt-in): `execute_plan` stores the result as a
//!   deduplicated trie and `count_rows_in_memory` counts it. Nemo stores its
//!   result, so one build can be materialised once (`MATERIALIZE_MUTATES`).
//! - **`space`**: Nemo's own `table_size_bytes` over the spec tables, read
//!   after loading.
//! - **No `stream`**: upstream's streaming join (`evaluate_operation` with
//!   `RowScan`) is private, and the public `execute_plan` stores its result, so
//!   `iteration` and `end_to_end` are not offered.
//!
//! Nemo has set semantics (its loader and its result trie deduplicate), so
//! `KEEPS_DUPLICATES` is false. It answers the full conjunctive query, whose
//! distinct rows are the same under set and bag semantics. It runs on one
//! thread and reports no thread count.

// Task 8.2's `Engine` impl uses everything below; it deletes this allow.
#![allow(dead_code)]

use {
    super::EngineError, crate::db::Prepared, kermit_algos::INDEX_ID_PREFIX, kermit_parser::Term,
    std::fmt,
};

/// The revision of `nemo-physical` that `kermit/Cargo.toml` pins. A unit test
/// compares the two, so a move of the pin changes both or fails.
pub const NEMO_REV: &str = "04f0962b91edc1bbe73e6f65f08d41d1ee491980";

/// The workspace version of `nemo-physical` at [`NEMO_REV`]. Moving the pin
/// means reading the new revision's version here.
pub const NEMO_PHYSICAL_VERSION: &str = "0.10.2-dev";

/// The engine's name: the report's `system` axis, and the `engine` of its
/// errors.
const ENGINE: &str = "nemo";

/// An error of this engine.
fn failure(message: impl fmt::Display) -> EngineError {
    EngineError {
        engine: ENGINE,
        message: message.to_string(),
    }
}

/// The markers of one join, as the numbers Nemo's `OperationColumnMarker`
/// wraps.
///
/// A marker is a variable's position in kermit's plan, so the result's markers
/// are `0, 1, …` in the order the plan binds the variables. Nemo orders a
/// join's variables by the order its markers are pushed, so it runs kermit's
/// plan. An atom lists its variables' markers in increasing order, as Nemo's
/// join requires of every input (it panics otherwise): `plan_indexes`
/// gives each atom its variables in plan order, and `plan_markers` checks it.
#[derive(Debug, PartialEq, Eq)]
struct PlanMarkers {
    /// The result table's markers, one per variable of the executed query.
    result: Vec<usize>,
    /// One entry per atom of the executed query, in body order: the index
    /// (`#k`) it reads, whose table the atom fetches, and its variables'
    /// markers.
    atoms: Vec<(usize, Vec<usize>)>,
}

/// The markers of `prepared`'s join.
///
/// # Errors
///
/// An atom of the executed query that does not name one of its indexes, holds
/// anything but a variable, or lists its variables out of plan order: `prepare`
/// guarantees none of these, so each is a broken invariant reported rather than
/// passed on to Nemo.
fn plan_markers(prepared: &Prepared) -> Result<PlanMarkers, EngineError> {
    let order = prepared.variable_order();
    let mut atoms = Vec::with_capacity(prepared.query().body.len());
    for atom in &prepared.query().body {
        let index = atom
            .name
            .strip_prefix(INDEX_ID_PREFIX)
            .and_then(|digits| digits.parse::<usize>().ok())
            .filter(|&index| index < prepared.indexes().len())
            .ok_or_else(|| {
                failure(format!(
                    "atom {atom} does not name one of the query's indexes"
                ))
            })?;
        let mut markers = Vec::with_capacity(atom.terms.len());
        for term in &atom.terms {
            let Term::Var(name) = term else {
                return Err(failure(format!(
                    "atom {atom} holds {term}, which is not a variable"
                )));
            };
            let position = order
                .iter()
                .position(|bound| bound == name)
                .ok_or_else(|| {
                    failure(format!("variable {name} of atom {atom} is not in the plan"))
                })?;
            markers.push(position);
        }
        let columns = prepared.indexes()[index].columns.len();
        if markers.len() != columns || !markers.windows(2).all(|pair| pair[0] < pair[1]) {
            return Err(failure(format!(
                "atom {atom} does not list its {columns} variables in plan order"
            )));
        }
        atoms.push((index, markers));
    }
    Ok(PlanMarkers {
        result: (0..order.len()).collect(),
        atoms,
    })
}

/// `plan_row`, whose column `p` holds the variable the plan binds `p`-th, as
/// the canonical row: column `v` holds canonical variable `v`, in the order of
/// `canonical_names(prepared.query())` (the executed query's order, head
/// variables first). `ordering` is the plan's `variable_ordering` (canonical
/// indices in descent order), a permutation of `0..plan_row.len()`.
fn to_canonical(plan_row: &[usize], ordering: &[usize]) -> Vec<usize> {
    debug_assert_eq!(plan_row.len(), ordering.len());
    let mut row = vec![0; plan_row.len()];
    for (position, &value) in plan_row.iter().enumerate() {
        row[ordering[position]] = value;
    }
    row
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::db::{prepare, Catalog},
        kermit_algos::{JoinQuery, LexicographicOptimiser, Planner, StatisticsLevel},
        kermit_ds::{RelationHeader, TrieLevels, Tuples},
    };

    /// A catalog of positional relations, each `(name, arity, rows)`.
    fn catalog(relations: Vec<(&str, usize, Vec<Vec<usize>>)>) -> Catalog {
        let relations = relations
            .into_iter()
            .map(|(name, arity, rows)| {
                let mut tuples = Tuples::new(arity);
                for row in &rows {
                    tuples.push(row);
                }
                (RelationHeader::new_positional(name, arity), tuples)
            })
            .collect();
        Catalog::new(relations, StatisticsLevel::TupleCounts)
    }

    /// `text` prepared over `catalog` in the lexicographic plan, under the
    /// levels engines use.
    fn prepared_for(catalog: &Catalog, text: &str) -> Prepared {
        let query: JoinQuery = text.parse().expect("a well-formed query");
        let planner = Planner::new(Box::new(LexicographicOptimiser));
        prepare(catalog, &query, &planner, TrieLevels::All).expect("a valid query")
    }

    /// `NEMO_REV` is a copy of the `rev` in `kermit/Cargo.toml`. The precedent
    /// is the Python `SCHEMA_VERSION` pin: the two move together, and this
    /// fails when only one does.
    #[test]
    fn the_pinned_revision_is_the_manifests() {
        let manifest = include_str!("../../Cargo.toml");
        let dependency = manifest
            .lines()
            .find(|line| line.trim_start().starts_with("nemo-physical"))
            .expect("kermit/Cargo.toml declares the nemo-physical dependency");
        assert!(
            dependency.contains(&format!("rev = \"{NEMO_REV}\"")),
            "NEMO_REV ({NEMO_REV}) is not the rev of `{dependency}`; move both together"
        );
    }

    /// R(X, P), S(X, Y), T(Y): X and Y are join keys and P is payload, so the
    /// lexicographic plan binds X, Y, P while the query's canonical order is X,
    /// P, Y. A marker is a plan position, so R's markers are X's 0 and P's 2,
    /// where canonical numbering would give 0 and 1.
    #[test]
    fn markers_follow_the_plans_variable_order() {
        let c = catalog(vec![
            ("R", 2, vec![vec![1, 7]]),
            ("S", 2, vec![vec![1, 5]]),
            ("T", 1, vec![vec![5]]),
        ]);
        let prepared = prepared_for(&c, "Q(X, P, Y) :- R(X, P), S(X, Y), T(Y).");
        assert_eq!(prepared.variable_order(), vec!["X", "Y", "P"]);
        let markers =
            plan_markers(&prepared).expect("every atom lists its variables in plan order");
        assert_eq!(markers.result, vec![0, 1, 2]);
        assert_eq!(markers.atoms, vec![
            (0, vec![0, 2]),
            (1, vec![0, 1]),
            (2, vec![1])
        ]);
    }

    /// The three atoms of a triangle over one relation read one merged index:
    /// Nemo fetches the one table three times, with different markers.
    #[test]
    fn atoms_over_one_merged_index_share_its_table() {
        let c = catalog(vec![("E", 2, vec![vec![1, 2]])]);
        let prepared = prepared_for(&c, "Q(X, Y, Z) :- E(X, Y), E(Y, Z), E(X, Z).");
        assert_eq!(prepared.indexes().len(), 1);
        let markers =
            plan_markers(&prepared).expect("every atom lists its variables in plan order");
        assert_eq!(markers.result, vec![0, 1, 2]);
        assert_eq!(markers.atoms, vec![
            (0, vec![0, 1]),
            (0, vec![1, 2]),
            (0, vec![0, 2])
        ]);
    }

    /// The result's column `p` holds the variable the plan binds `p`-th, whose
    /// canonical index is `ordering[p]`.
    #[test]
    fn to_canonical_puts_each_plan_column_at_its_variable() {
        // The plan binds canonical variables 0, 2, 1: plan column 1 holds
        // variable 2.
        assert_eq!(to_canonical(&[10, 30, 20], &[0, 2, 1]), vec![10, 20, 30]);
        // The plan binds 2, 0, 1: the plan row [c, a, b] is the canonical row
        // [a, b, c].
        assert_eq!(to_canonical(&[3, 1, 2], &[2, 0, 1]), vec![1, 2, 3]);
        assert_eq!(to_canonical(&[7], &[0]), vec![7]);
        assert_eq!(to_canonical(&[], &[]), Vec::<usize>::new());
    }
}
