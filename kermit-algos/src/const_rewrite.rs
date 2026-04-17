//! Const-view rewrite implementing Veldhuizen 2014 §3.4 point 4.
//!
//! Transforms body atoms (e.g. `p(X, c42)`) into fresh variables
//! filtered by synthetic unary `Const_c42` predicates, so the existing
//! LFTJ engine can handle them without modification. Intended to run
//! immediately before [`crate::JoinAlgo::join_iter`].

use {
    kermit_parser::{JoinQuery, Predicate, Term},
    std::fmt,
};

/// Error returned by [`rewrite_atoms`] when an atom does not match the
/// expected `c<digits>` shape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RewriteError {
    /// An atom was not of the form `c<digits>`. kermit currently only
    /// supports constants encoded as dictionary IDs using this
    /// convention.
    BadAtom(String),
}

impl fmt::Display for RewriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            | RewriteError::BadAtom(s) => write!(
                f,
                "atom {s:?} does not match the expected c<digits> shape — kermit currently only \
                 supports constants encoded as dictionary IDs",
            ),
        }
    }
}

impl std::error::Error for RewriteError {}

/// Pairs a synthetic predicate name (e.g. `"Const_c42"`) with its
/// dictionary ID. One entry is produced per rewritten atom occurrence.
pub type ConstSpec = (String, usize);

/// Rewrites `query.body`: each `Term::Atom("c<id>")` becomes a fresh
/// variable `K<i>`, with a new unary predicate `Const_c<id>(K<i>)`
/// appended to the body.
///
/// Each atom occurrence gets its own fresh variable, even if the same
/// dictionary ID appears multiple times. This avoids forcing equality
/// between unrelated body positions.
///
/// # Errors
///
/// Returns [`RewriteError::BadAtom`] if any atom doesn't match `c\d+`.
pub fn rewrite_atoms(mut query: JoinQuery) -> Result<(JoinQuery, Vec<ConstSpec>), RewriteError> {
    let mut next_k = highest_k_index(&query).map_or(0, |n| n + 1);
    let mut specs: Vec<ConstSpec> = Vec::new();
    let mut new_preds: Vec<Predicate> = Vec::new();

    for pred in &mut query.body {
        for term in &mut pred.terms {
            let atom = match term {
                | Term::Atom(s) => s.clone(),
                | _ => continue,
            };
            let id = parse_const_atom(&atom)?;
            let fresh = format!("K{next_k}");
            next_k += 1;
            *term = Term::Var(fresh.clone());
            let const_name = format!("Const_{atom}");
            new_preds.push(Predicate {
                name: const_name.clone(),
                terms: vec![Term::Var(fresh)],
            });
            specs.push((const_name, id));
        }
    }
    query.body.extend(new_preds);
    Ok((query, specs))
}

fn parse_const_atom(s: &str) -> Result<usize, RewriteError> {
    let rest = s
        .strip_prefix('c')
        .ok_or_else(|| RewriteError::BadAtom(s.to_string()))?;
    if rest.is_empty() || !rest.chars().all(|c| c.is_ascii_digit()) {
        return Err(RewriteError::BadAtom(s.to_string()));
    }
    rest.parse::<usize>()
        .map_err(|_| RewriteError::BadAtom(s.to_string()))
}

fn highest_k_index(query: &JoinQuery) -> Option<usize> {
    let scan = |p: &Predicate| -> Option<usize> {
        p.terms
            .iter()
            .filter_map(|t| match t {
                | Term::Var(name) => name.strip_prefix('K').and_then(|r| r.parse::<usize>().ok()),
                | _ => None,
            })
            .max()
    };
    query
        .body
        .iter()
        .chain(std::iter::once(&query.head))
        .filter_map(scan)
        .max()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(q: &str) -> JoinQuery { q.parse().unwrap() }

    #[test]
    fn zero_atoms_is_identity() {
        let q = parse("Q(X) :- p(X), r(X, Y).");
        let (out, specs) = rewrite_atoms(q.clone()).unwrap();
        assert_eq!(out, q);
        assert!(specs.is_empty());
    }

    #[test]
    fn single_atom_produces_one_fresh_var_and_one_const_pred() {
        let q = parse("Q(X) :- p(X, c42).");
        let (out, specs) = rewrite_atoms(q).unwrap();
        assert_eq!(out.body.len(), 2);
        assert_eq!(out.body[0].name, "p");
        assert!(matches!(out.body[0].terms[1], Term::Var(ref n) if n == "K0"));
        assert_eq!(out.body[1].name, "Const_c42");
        assert!(matches!(out.body[1].terms[0], Term::Var(ref n) if n == "K0"));
        assert_eq!(specs, vec![("Const_c42".into(), 42)]);
    }

    #[test]
    fn multiple_atoms_get_distinct_fresh_vars() {
        let q = parse("Q(X) :- p(X, c42), r(Y, c99).");
        let (out, specs) = rewrite_atoms(q).unwrap();
        assert_eq!(out.body.len(), 4);
        assert_eq!(specs, vec![
            ("Const_c42".into(), 42),
            ("Const_c99".into(), 99),
        ]);
    }

    #[test]
    fn repeated_atom_value_gets_distinct_vars_but_same_const_pred() {
        let q = parse("Q(X) :- p(X, c5), r(Y, c5).");
        let (out, specs) = rewrite_atoms(q).unwrap();
        assert_eq!(out.body.len(), 4);
        assert_eq!(specs.len(), 2);
        assert_eq!(specs[0].0, "Const_c5");
        assert_eq!(specs[1].0, "Const_c5");
        let k0 = match &out.body[0].terms[1] {
            | Term::Var(n) => n.clone(),
            | _ => panic!(),
        };
        let k1 = match &out.body[1].terms[1] {
            | Term::Var(n) => n.clone(),
            | _ => panic!(),
        };
        assert_ne!(k0, k1);
    }

    #[test]
    fn fresh_var_allocation_avoids_existing_k_names() {
        let q = parse("Q(K5) :- p(K5, c7).");
        let (out, _) = rewrite_atoms(q).unwrap();
        let fresh = match &out.body[0].terms[1] {
            | Term::Var(n) => n.clone(),
            | _ => panic!(),
        };
        let n: usize = fresh.strip_prefix('K').unwrap().parse().unwrap();
        assert!(n > 5, "got {fresh}, expected > K5");
    }

    #[test]
    fn malformed_atom_errors() {
        for bad in ["foo", "c", "c1x", "cc5", "x42"] {
            let q = JoinQuery {
                head: Predicate {
                    name: "Q".into(),
                    terms: vec![Term::Var("X".into())],
                },
                body: vec![Predicate {
                    name: "p".into(),
                    terms: vec![Term::Var("X".into()), Term::Atom(bad.into())],
                }],
            };
            assert!(
                matches!(rewrite_atoms(q), Err(RewriteError::BadAtom(_))),
                "expected error for {bad}"
            );
        }
    }

    #[test]
    fn placeholders_left_alone() {
        let q = parse("Q(X) :- p(X, _), r(_, c7).");
        let (out, specs) = rewrite_atoms(q).unwrap();
        assert_eq!(out.body.len(), 3);
        assert!(matches!(out.body[0].terms[1], Term::Placeholder));
        assert_eq!(specs, vec![("Const_c7".into(), 7)]);
    }
}
