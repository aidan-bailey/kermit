//! The query plan consumed by join algorithms.

use {crate::optimiser::analysis::QueryAnalysis, std::fmt};

/// An executable plan for a join query, produced by a query optimiser.
///
/// v1 carries only the global attribute order. Future planning decisions
/// (body-atom order, join trees for binary algorithms) grow as new fields,
/// non-breaking.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryPlan {
    /// Canonical variable indices (see
    /// [`analyse`](crate::optimiser::analyse)) in descent order.
    pub variable_ordering: Vec<usize>,
}

/// Error returned by [`QueryPlan::validate`] when a plan cannot execute
/// against the query it claims to plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanError {
    /// The ordering is not a permutation of `0..num_vars`.
    NotAPermutation,
    /// A relation's physical column order is violated: body predicate
    /// `predicate` requires `earlier` to be bound before `later`.
    ColumnOrderViolated {
        /// Index of the violated body predicate.
        predicate: usize,
        /// Variable that must come first.
        earlier: usize,
        /// Variable the plan wrongly ordered before `earlier`.
        later: usize,
    },
}

impl fmt::Display for PlanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            | PlanError::NotAPermutation => {
                write!(
                    f,
                    "variable ordering is not a permutation of the query's variables"
                )
            },
            | PlanError::ColumnOrderViolated {
                predicate,
                earlier,
                later,
            } => write!(
                f,
                "body predicate {predicate} requires variable {earlier} bound before {later}, but \
                 the plan orders them the other way"
            ),
        }
    }
}

impl std::error::Error for PlanError {}

impl QueryPlan {
    /// Checks that this plan can execute against a query with the given
    /// [`QueryAnalysis`]: the ordering must be a permutation of
    /// `0..num_vars` respecting every predicate's physical column order.
    ///
    /// Executors call this before descending; the provided optimisers are
    /// valid by construction (they only pick among Kahn-ready variables),
    /// so a failure here means a hand-built or third-party plan is broken.
    ///
    /// # Errors
    ///
    /// Returns the first violation found, checking the permutation
    /// property before column-order constraints.
    pub fn validate(&self, analysis: &QueryAnalysis) -> Result<(), PlanError> {
        let n = analysis.num_vars;
        if self.variable_ordering.len() != n {
            return Err(PlanError::NotAPermutation);
        }
        let mut position = vec![usize::MAX; n];
        for (pos, &v) in self.variable_ordering.iter().enumerate() {
            if v >= n || position[v] != usize::MAX {
                return Err(PlanError::NotAPermutation);
            }
            position[v] = pos;
        }
        for (p, vars) in analysis.predicate_variables.iter().enumerate() {
            for pair in vars.windows(2) {
                let (earlier, later) = (pair[0], pair[1]);
                if earlier != later && position[earlier] > position[later] {
                    return Err(PlanError::ColumnOrderViolated {
                        predicate: p,
                        earlier,
                        later,
                    });
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use {super::*, crate::optimiser::analysis::analyse, kermit_parser::JoinQuery};

    fn triangle() -> QueryAnalysis {
        let q: JoinQuery = "Q(X, Y, Z) :- R(X, Y), S(Y, Z), T(X, Z).".parse().unwrap();
        analyse(&q)
    }

    #[test]
    fn valid_plan_passes() {
        let plan = QueryPlan {
            variable_ordering: vec![0, 1, 2],
        };
        assert_eq!(plan.validate(&triangle()), Ok(()));
    }

    #[test]
    fn wrong_length_is_not_a_permutation() {
        let plan = QueryPlan {
            variable_ordering: vec![0, 1],
        };
        assert_eq!(plan.validate(&triangle()), Err(PlanError::NotAPermutation));
    }

    #[test]
    fn repeated_variable_is_not_a_permutation() {
        let plan = QueryPlan {
            variable_ordering: vec![0, 1, 1],
        };
        assert_eq!(plan.validate(&triangle()), Err(PlanError::NotAPermutation));
    }

    #[test]
    fn out_of_range_variable_is_not_a_permutation() {
        // Correct length, all distinct, but 5 is not a variable of the query.
        let plan = QueryPlan {
            variable_ordering: vec![0, 1, 5],
        };
        assert_eq!(plan.validate(&triangle()), Err(PlanError::NotAPermutation));
    }

    #[test]
    fn column_order_violation_is_rejected() {
        // Ordering Y before X violates R(X, Y)'s physical column order.
        let plan = QueryPlan {
            variable_ordering: vec![1, 0, 2],
        };
        assert_eq!(
            plan.validate(&triangle()),
            Err(PlanError::ColumnOrderViolated {
                predicate: 0,
                earlier: 0,
                later: 1,
            })
        );
    }
}
