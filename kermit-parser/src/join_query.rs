#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Term {
    Var(String),
    Atom(String),
    Placeholder,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Predicate {
    pub name: String,
    pub terms: Vec<Term>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JoinQuery {
    pub head: Predicate,
    pub body: Vec<Predicate>,
}
