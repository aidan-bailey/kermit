//! The executed query: the one form the joins accept.
//!
//! [`crate::plan_indexes`] rewrites a query into this form: one atom per
//! index, over distinct variables, with no constant and no `_`, every head
//! variable carried by an atom, and a plan valid for it. Both joins are
//! correct only on this form, so [`ExecutedQuery::new`] checks it once and
//! the joins take the type instead of a [`JoinQuery`]. The type also holds
//! what each join used to recompute on every call: the query's analysis and
//! its checked plan.

use {
    crate::{
        analysis::{analyse, QueryAnalysis},
        optimiser::{PlanError, QueryPlan},
    },
    kermit_parser::{JoinQuery, Term},
    std::fmt,
};

/// A query in executed form, with its analysis and a plan checked against
/// it.
///
/// Only [`new`](Self::new) makes one, so every value holds these
/// invariants:
/// - every body term is a variable, and no atom carries a variable twice;
/// - every head variable is carried by some atom;
/// - [`plan`](Self::plan) is valid for [`analysis`](Self::analysis).
///
/// The head's own shape is not checked: the joins do not read it, and
/// `kermit::db` validates it before planning. A query with no atoms is
/// valid: its one answer is the empty row.
#[derive(Clone, Debug)]
pub struct ExecutedQuery {
    query: JoinQuery,
    analysis: QueryAnalysis,
    plan: QueryPlan,
    atom_indexes: Vec<usize>,
    index_count: usize,
}

impl ExecutedQuery {
    /// Checks `query` and `plan`, and holds them together.
    ///
    /// The distinct atom names, numbered by first appearance, are the
    /// index ids: atom `j` reads index
    /// [`atom_indexes()[j]`](Self::atom_indexes). Under that numbering an
    /// atom `#k` of [`crate::plan_indexes`] reads index `k`.
    ///
    /// # Errors
    ///
    /// The first violation found, looking at each body atom column by
    /// column, then at each head variable in head order, then at the plan.
    pub fn new(query: JoinQuery, plan: QueryPlan) -> Result<Self, ExecutedQueryError> {
        for (atom, predicate) in query.body.iter().enumerate() {
            for (column, term) in predicate.terms.iter().enumerate() {
                match term {
                    | Term::Var(variable) => {
                        if predicate.terms[..column].contains(term) {
                            return Err(ExecutedQueryError::RepeatedVariable {
                                atom,
                                variable: variable.clone(),
                            });
                        }
                    },
                    | Term::Atom(_) => {
                        return Err(ExecutedQueryError::Constant {
                            atom,
                            column,
                        })
                    },
                    | Term::Placeholder => {
                        return Err(ExecutedQueryError::Placeholder {
                            atom,
                            column,
                        });
                    },
                }
            }
        }
        for term in &query.head.terms {
            if let Term::Var(variable) = term {
                if !query
                    .body
                    .iter()
                    .any(|predicate| predicate.terms.contains(term))
                {
                    return Err(ExecutedQueryError::UnboundVariable {
                        variable: variable.clone(),
                    });
                }
            }
        }
        let analysis = analyse(&query);
        plan.validate(&analysis).map_err(ExecutedQueryError::Plan)?;

        let mut names: Vec<&str> = Vec::new();
        let atom_indexes = query
            .body
            .iter()
            .map(
                |predicate| match names.iter().position(|&name| name == predicate.name) {
                    | Some(id) => id,
                    | None => {
                        names.push(&predicate.name);
                        names.len() - 1
                    },
                },
            )
            .collect();
        let index_count = names.len();
        Ok(ExecutedQuery {
            query,
            analysis,
            plan,
            atom_indexes,
            index_count,
        })
    }

    /// The query, as given.
    pub fn query(&self) -> &JoinQuery { &self.query }

    /// The query's analysis: its canonical variable numbering, and the
    /// variables each atom carries.
    pub fn analysis(&self) -> &QueryAnalysis { &self.analysis }

    /// The plan, valid for [`analysis`](Self::analysis).
    pub fn plan(&self) -> &QueryPlan { &self.plan }

    /// `atom_indexes()[j]`: the index body atom `j` reads.
    pub fn atom_indexes(&self) -> &[usize] { &self.atom_indexes }

    /// The number of distinct indexes the atoms read: a join is passed
    /// exactly this many.
    pub fn index_count(&self) -> usize { self.index_count }
}

impl fmt::Display for ExecutedQuery {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { fmt::Display::fmt(&self.query, f) }
}

/// Why a query is not in executed form; see [`ExecutedQuery::new`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecutedQueryError {
    /// A body atom holds a constant.
    Constant {
        /// The atom's position in the body.
        atom: usize,
        /// The constant's column.
        column: usize,
    },
    /// A body atom holds `_`.
    Placeholder {
        /// The atom's position in the body.
        atom: usize,
        /// The placeholder's column.
        column: usize,
    },
    /// A body atom carries a variable more than once.
    RepeatedVariable {
        /// The atom's position in the body.
        atom: usize,
        /// The variable's name.
        variable: String,
    },
    /// No body atom carries a head variable.
    UnboundVariable {
        /// The variable's name.
        variable: String,
    },
    /// The plan is not valid for the query.
    Plan(PlanError),
}

impl fmt::Display for ExecutedQueryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            | Self::Constant {
                atom,
                column,
            } => write!(
                f,
                "body atom {atom} holds a constant at column {column}; the executed form has \
                 variables only"
            ),
            | Self::Placeholder {
                atom,
                column,
            } => write!(
                f,
                "body atom {atom} holds `_` at column {column}; the executed form has named \
                 variables only"
            ),
            | Self::RepeatedVariable {
                atom,
                variable,
            } => {
                write!(
                    f,
                    "body atom {atom} carries variable {variable} more than once"
                )
            },
            | Self::UnboundVariable {
                variable,
            } => {
                write!(f, "head variable {variable} is carried by no body atom")
            },
            | Self::Plan(error) => write!(f, "invalid plan: {error}"),
        }
    }
}

impl std::error::Error for ExecutedQueryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            | Self::Plan(error) => Some(error),
            | _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The identity plan of `query`: its variables in canonical order.
    fn identity(query: &JoinQuery) -> QueryPlan {
        QueryPlan {
            variable_ordering: (0..analyse(query).num_vars).collect(),
        }
    }

    fn check(text: &str) -> Result<ExecutedQuery, ExecutedQueryError> {
        let query: JoinQuery = text.parse().expect("parse");
        let plan = identity(&query);
        ExecutedQuery::new(query, plan)
    }

    /// `Q() :- .`, which the parser cannot spell: no head term, no atom.
    fn empty_query() -> JoinQuery {
        let mut query: JoinQuery = "Q(c9) :- r(c1).".parse().expect("parse");
        query.head.terms.clear();
        query.body.clear();
        query
    }

    #[test]
    fn a_query_in_executed_form_is_accepted() {
        let executed = check("Q(X, Y, Z) :- R(X, Y), S(Y, Z), T(X, Z).").unwrap();
        assert_eq!(
            executed.to_string(),
            "Q(X, Y, Z) :- R(X, Y), S(Y, Z), T(X, Z)."
        );
        assert_eq!(executed.query().body.len(), 3);
        assert_eq!(executed.plan().variable_ordering, [0, 1, 2]);
        assert_eq!(executed.analysis().predicate_variables, [
            vec![0, 1],
            vec![1, 2],
            vec![0, 2]
        ]);
        assert_eq!(executed.atom_indexes(), [0, 1, 2]);
        assert_eq!(executed.index_count(), 3);
    }

    #[test]
    fn atoms_that_share_a_name_read_one_index() {
        let executed = check("Q(X, Y, Z) :- R(X, Y), R(Y, Z).").unwrap();
        assert_eq!(executed.atom_indexes(), [0, 0]);
        assert_eq!(executed.index_count(), 1);
    }

    #[test]
    fn indexes_are_numbered_by_first_appearance() {
        let executed = check("Q(X, Y, Z) :- S(X, Y), R(Y, Z), S(X, Z).").unwrap();
        assert_eq!(executed.atom_indexes(), [0, 1, 0]);
        assert_eq!(executed.index_count(), 2);
    }

    /// LFTJ used to answer `[[1], [3]]` over `{(1, 2), (3, 3)}`: it opened
    /// one level for the one variable and never read the second column.
    #[test]
    fn a_repeated_variable_is_rejected() {
        assert_eq!(
            check("Q(X) :- R(X, X).").unwrap_err(),
            ExecutedQueryError::RepeatedVariable {
                atom: 0,
                variable: "X".to_string(),
            }
        );
    }

    /// `validate` used to reject `R(X, Y, X)` as a column-order violation.
    #[test]
    fn a_non_adjacent_repeat_is_rejected_as_a_repeat() {
        assert_eq!(
            check("Q(X, Y) :- R(X, Y, X).").unwrap_err(),
            ExecutedQueryError::RepeatedVariable {
                atom: 0,
                variable: "X".to_string(),
            }
        );
    }

    #[test]
    fn a_constant_is_rejected() {
        assert_eq!(
            check("Q(X) :- R(X, c5).").unwrap_err(),
            ExecutedQueryError::Constant {
                atom: 0,
                column: 1
            }
        );
    }

    #[test]
    fn a_placeholder_is_rejected() {
        assert_eq!(
            check("Q(X) :- S(X), R(X, _).").unwrap_err(),
            ExecutedQueryError::Placeholder {
                atom: 1,
                column: 1
            }
        );
    }

    /// LFTJ used to panic on an empty leapfrog here, and Hash Triejoin to
    /// return no rows.
    #[test]
    fn an_unbound_head_variable_is_rejected() {
        assert_eq!(
            check("Q(X, Y) :- R(X).").unwrap_err(),
            ExecutedQueryError::UnboundVariable {
                variable: "Y".to_string(),
            }
        );
    }

    #[test]
    fn a_plan_the_columns_contradict_is_rejected() {
        let query: JoinQuery = "Q(X, Y) :- R(X, Y).".parse().unwrap();
        let plan = QueryPlan {
            variable_ordering: vec![1, 0],
        };
        assert_eq!(
            ExecutedQuery::new(query, plan).unwrap_err(),
            ExecutedQueryError::Plan(PlanError::ColumnOrderViolated {
                predicate: 0,
                earlier: 0,
                later: 1,
            })
        );
    }

    #[test]
    fn a_plan_that_is_not_a_permutation_is_rejected() {
        let query: JoinQuery = "Q(X, Y) :- R(X, Y).".parse().unwrap();
        let plan = QueryPlan {
            variable_ordering: vec![0],
        };
        assert_eq!(
            ExecutedQuery::new(query, plan).unwrap_err(),
            ExecutedQueryError::Plan(PlanError::NotAPermutation)
        );
    }

    #[test]
    fn the_first_violation_is_reported() {
        assert_eq!(
            check("Q(X, W) :- R(c1, X), S(X, X).").unwrap_err(),
            ExecutedQueryError::Constant {
                atom: 0,
                column: 0
            }
        );
    }

    #[test]
    fn a_query_with_no_atoms_is_accepted() {
        let plan = QueryPlan {
            variable_ordering: vec![],
        };
        let executed = ExecutedQuery::new(empty_query(), plan).unwrap();
        assert!(executed.atom_indexes().is_empty());
        assert_eq!(executed.index_count(), 0);
        assert_eq!(executed.analysis().num_vars, 0);
    }

    #[test]
    fn errors_say_what_is_wrong() {
        let repeat = ExecutedQueryError::RepeatedVariable {
            atom: 0,
            variable: "X".to_string(),
        };
        assert_eq!(
            repeat.to_string(),
            "body atom 0 carries variable X more than once"
        );
        let plan = ExecutedQueryError::Plan(PlanError::NotAPermutation);
        assert!(plan.to_string().starts_with("invalid plan: "));
        assert!(std::error::Error::source(&plan).is_some());
        assert!(std::error::Error::source(&repeat).is_none());
    }
}
