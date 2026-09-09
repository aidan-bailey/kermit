//! Query analysis shared by planners and executors.

use {
    kermit_parser::{JoinQuery, Term},
    std::collections::HashMap,
};

/// Structural facts about a [`JoinQuery`]: the canonical variable
/// numbering and which variables each body predicate carries.
///
/// Planners and executors both call [`analyse`] on the same query and
/// therefore agree on the numbering — the `QueryPlan` carries canonical
/// indices, never names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryAnalysis {
    /// Number of distinct variables in the query.
    pub num_vars: usize,
    /// For each body predicate (in body order), the canonical indices of
    /// the variables it carries, in the predicate's term (physical column)
    /// order.
    pub predicate_variables: Vec<Vec<usize>>,
}

/// Indexes the variables in a [`JoinQuery`].
///
/// Two passes assign canonical indices: head variables first (so output
/// tuple order matches the head declaration), then body-only variables in
/// first-appearance order. Specifically, the j-th distinct head variable
/// receives canonical index j. A third pass collects per-predicate variable
/// index lists. Placeholders (`_`) and atoms are skipped in all passes —
/// they occupy trie levels but don't bind a join variable.
///
/// Deterministic: same query, same numbering, always.
pub fn analyse(query: &JoinQuery) -> QueryAnalysis {
    let mut var_to_index: HashMap<String, usize> = HashMap::new();
    let mut next_index: usize = 0;

    // Helper: assigns a fresh index to a variable name on first sight,
    // returns the existing index on subsequent encounters.
    let register_var = |name: &str, map: &mut HashMap<String, usize>, next: &mut usize| {
        *map.entry(name.to_string()).or_insert_with(|| {
            let idx = *next;
            *next += 1;
            idx
        })
    };

    // Pass 1: head variables — establishes output tuple ordering.
    for t in &query.head.terms {
        if let Term::Var(ref vname) = t {
            let _ = register_var(vname, &mut var_to_index, &mut next_index);
        }
    }

    // Pass 2: body-only variables — any variable not already seen in the head.
    for pred in &query.body {
        for t in &pred.terms {
            if let Term::Var(ref vname) = t {
                let _ = register_var(vname, &mut var_to_index, &mut next_index);
            }
        }
    }

    // Pass 3: build per-predicate variable index lists. Placeholders and
    // atoms are skipped — they occupy trie levels but don't bind a join
    // variable.
    let mut predicate_variables: Vec<Vec<usize>> = Vec::with_capacity(query.body.len());
    for pred in &query.body {
        let mut vars_for_pred: Vec<usize> = Vec::new();
        for t in &pred.terms {
            if let Term::Var(ref vname) = t {
                if let Some(idx) = var_to_index.get(vname) {
                    vars_for_pred.push(*idx);
                }
            }
        }
        predicate_variables.push(vars_for_pred);
    }

    QueryAnalysis {
        num_vars: var_to_index.len(),
        predicate_variables,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn triangle_head_first_indexing() {
        let query: JoinQuery = "Q(X, Y, Z) :- R(X, Y), S(Y, Z), T(X, Z).".parse().unwrap();
        let analysis = analyse(&query);
        assert_eq!(analysis.num_vars, 3);
        assert_eq!(analysis.predicate_variables, vec![
            vec![0, 1],
            vec![1, 2],
            vec![0, 2]
        ]);
    }

    #[test]
    fn body_only_variables_index_after_head() {
        // Y never appears in the head — it gets the next index after X.
        let query: JoinQuery = "Q(X) :- R(X, Y), S(Y).".parse().unwrap();
        let analysis = analyse(&query);
        assert_eq!(analysis.num_vars, 2);
        assert_eq!(analysis.predicate_variables, vec![vec![0, 1], vec![1]]);
    }

    #[test]
    fn placeholders_and_atoms_are_skipped() {
        let query: JoinQuery = "Q(X) :- R(X, _), S(X, c5).".parse().unwrap();
        let analysis = analyse(&query);
        assert_eq!(analysis.num_vars, 1);
        assert_eq!(analysis.predicate_variables, vec![vec![0], vec![0]]);
    }

    #[test]
    fn repeated_variable_within_one_predicate() {
        // The same canonical index appears twice in the predicate's list.
        let query: JoinQuery = "Q(X) :- R(X, X).".parse().unwrap();
        let analysis = analyse(&query);
        assert_eq!(analysis.num_vars, 1);
        assert_eq!(analysis.predicate_variables, vec![vec![0, 0]]);
    }
}
