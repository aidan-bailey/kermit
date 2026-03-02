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
