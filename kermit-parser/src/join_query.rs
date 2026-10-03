use std::fmt;

/// A single term in a Datalog predicate.
///
/// Variables start with an uppercase letter (e.g. `X`, `Name`), atoms start
/// with a lowercase letter (e.g. `alice`, `edge`), and `_` is the anonymous
/// placeholder that matches anything without binding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Term {
    /// A named variable (e.g. `X`).
    Var(String),
    /// A ground constant (e.g. `alice`).
    Atom(String),
    /// The anonymous wildcard `_`.
    Placeholder,
}

/// A Datalog predicate application, e.g. `edge(X, Y)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Predicate {
    /// Predicate name (e.g. `"edge"`).
    pub name: String,
    /// Argument terms in order.
    pub terms: Vec<Term>,
}

/// A parsed Datalog join query of the form `Head :- Body1, Body2, ... .`
///
/// For example: `path(X, Z) :- edge(X, Y), edge(Y, Z).`
///
/// Implements [`FromStr`](std::str::FromStr) for parsing from a string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JoinQuery {
    /// The head predicate defining the output schema.
    pub head: Predicate,
    /// The body predicates to be joined.
    pub body: Vec<Predicate>,
}

/// Writes the term as the parser reads it.
impl fmt::Display for Term {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            | Term::Var(name) | Term::Atom(name) => f.write_str(name),
            | Term::Placeholder => f.write_str("_"),
        }
    }
}

/// Writes the predicate as the parser reads it, e.g. `edge(X, _)`.
impl fmt::Display for Predicate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}(", self.name)?;
        for (i, term) in self.terms.iter().enumerate() {
            if i > 0 {
                f.write_str(", ")?;
            }
            write!(f, "{term}")?;
        }
        f.write_str(")")
    }
}

/// Writes the query as the parser reads it: `Head :- Body1, Body2.`
impl fmt::Display for JoinQuery {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} :- ", self.head)?;
        for (i, pred) in self.body.iter().enumerate() {
            if i > 0 {
                f.write_str(", ")?;
            }
            write!(f, "{pred}")?;
        }
        f.write_str(".")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn predicate_displays_as_written() {
        let q: JoinQuery = "Q(X) :- edge(X, _, c5).".parse().unwrap();
        assert_eq!(q.body[0].to_string(), "edge(X, _, c5)");
    }

    /// Display is the parser's inverse, so an error message can quote a
    /// query the user can paste back.
    #[test]
    fn query_display_round_trips_through_the_parser() {
        for text in [
            "Q(X, Y) :- edge(X, Y), edge(Y, X).",
            "Q(X) :- r(X, _, c42), s(_).",
            "Head(A, B, C) :- Rel(A, B), Other(B, C).",
        ] {
            let q: JoinQuery = text.parse().unwrap();
            assert_eq!(q.to_string(), text);
            assert_eq!(q.to_string().parse::<JoinQuery>().unwrap(), q);
        }
    }
}
