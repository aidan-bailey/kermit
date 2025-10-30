use winnow::{
    Parser, ascii::multispace0, combinator::{delimited, separated}, error::{ContextError, ErrMode}, token::take_while
};

type PResult<T> = Result<T, winnow::error::ErrMode<ContextError>>;

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
pub struct Query {
    pub head: Predicate,
    pub body: Vec<Predicate>,
}

// ---------- helpers ----------
fn ws(input: &mut &str) -> PResult<()> {
    let _: &str = multispace0.parse_next(input)?;
    Ok(())
}

fn ident(input: &mut &str) -> PResult<String> {
    let start = *input;
    take_while(1.., |c: char| c.is_ascii_alphabetic()).parse_next(input)?;
    take_while(0.., |c: char| c.is_ascii_alphanumeric() || c == '_').parse_next(input)?;
    let end = *input;
    let len = start.len() - end.len();
    Ok(start[..len].to_string())
}

fn comma(input: &mut &str) -> PResult<char> { delimited(ws, ',', ws).parse_next(input) }

fn dot(input: &mut &str) -> PResult<char> { delimited(ws, '.', ws).parse_next(input) }

// ---------- term / predicate ----------
fn term(input: &mut &str) -> PResult<Term> {

    if input.starts_with('_') && (input.len() == 1 || !input.chars().nth(1).unwrap().is_ascii_alphanumeric()) {
        let _ = '_'.parse_next(input)?;
        return Ok(Term::Placeholder);
    }

    let name = ident.parse_next(input)?;

    let is_var = name
        .chars()
        .next()
        .map(|c| c.is_ascii_uppercase())
        .unwrap_or(false);

    Ok(if is_var {
        Term::Var(name)
    } else {
        Term::Atom(name)
    })
}

fn term_list(input: &mut &str) -> PResult<Vec<Term>> {
    delimited(
        delimited(ws, '(', ws),
        separated(1.., term, comma),
        delimited(ws, ')', ws),
    )
    .parse_next(input)
}

fn predicate(input: &mut &str) -> PResult<Predicate> {
    ws.parse_next(input)?;
    let name = ident.parse_next(input)?;
    let terms = term_list.parse_next(input)?;
    Ok(Predicate {
        name,
        terms,
    })
}

// ---------- query ----------
fn query(input: &mut &str) -> PResult<Query> {
    let head = predicate.parse_next(input)?;
    // ":-" separates head from body
    let _ = delimited(ws, ":-", ws).parse_next(input)?;
    let body = separated(1.., predicate, comma).parse_next(input)?;
    let _ = dot.parse_next(input)?;
    Ok(Query {
        head,
        body,
    })
}

impl std::str::FromStr for Query {
    type Err = ErrMode<ContextError>;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut input = s;
        query.parse_next(&mut input)
    }
}

// ---------- demo ----------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_query_parser() {
        let mut src = "P(A, C) :- Q(A, B), R(B, C).";
        match query.parse_next(&mut src) {
            Ok(ast) => {
                println!("{ast:#?}");
                // Remaining input (should be empty or just whitespace)
                let _ = ws.parse_next(&mut src);
                if !src.is_empty() {
                    eprintln!("Unparsed tail: {src:?}");
                }
                assert_eq!(ast.head.name, "P");
                assert_eq!(ast.head.terms.len(), 2);
                assert_eq!(ast.body.len(), 2);
            }
            Err(e) => panic!("Parse error: {e:?}"),
        }
    }

    #[test]
    fn test_query_from_str_simple() {
        let query_str = "P(X) :- Q(X).";
        let query: Query = query_str.parse().expect("Failed to parse query");
        
        assert_eq!(query.head.name, "P");
        assert_eq!(query.head.terms.len(), 1);
        assert_eq!(query.head.terms[0], Term::Var("X".to_string()));
        
        assert_eq!(query.body.len(), 1);
        assert_eq!(query.body[0].name, "Q");
        assert_eq!(query.body[0].terms.len(), 1);
        assert_eq!(query.body[0].terms[0], Term::Var("X".to_string()));
    }

    #[test]
    fn test_query_from_str_multiple_body_predicates() {
        let query_str = "ancestor(X, Z) :- parent(X, Y), parent(Y, Z).";
        let query: Query = query_str.parse().expect("Failed to parse query");
        
        assert_eq!(query.head.name, "ancestor");
        assert_eq!(query.head.terms.len(), 2);
        assert_eq!(query.head.terms[0], Term::Var("X".to_string()));
        assert_eq!(query.head.terms[1], Term::Var("Z".to_string()));
        
        assert_eq!(query.body.len(), 2);
        assert_eq!(query.body[0].name, "parent");
        assert_eq!(query.body[1].name, "parent");
    }

    #[test]
    fn test_query_from_str_with_atoms() {
        let query_str = "likes(alice, X):- food(X), healthy(X).";
        let query: Query = query_str.parse().expect("Failed to parse query");
        
        assert_eq!(query.head.name, "likes");
        assert_eq!(query.head.terms[0], Term::Atom("alice".to_string()));
        assert_eq!(query.head.terms[1], Term::Var("X".to_string()));
        
        assert_eq!(query.body.len(), 2);
    }

    #[test]
    fn test_query_from_str_with_whitespace() {
        let query_str = "  P(X,Y)  :-  Q(X),R(Y)  .  ";
        let query: Query = query_str.parse().expect("Failed to parse query with whitespace");
        
        assert_eq!(query.head.name, "P");
        assert_eq!(query.head.terms.len(), 2);
        assert_eq!(query.body.len(), 2);
    }

    #[test]
    fn test_query_from_str_minimal_whitespace() {
        let query_str = "P(X,Y):-Q(X),R(Y).";
        let query: Query = query_str.parse().expect("Failed to parse query with minimal whitespace");
        
        assert_eq!(query.head.name, "P");
        assert_eq!(query.head.terms.len(), 2);
        assert_eq!(query.body.len(), 2);
    }

    #[test]
    fn test_query_from_str_spaces_after_commas() {
        let query_str = "result(X, Y, Z) :- relation1(X, Y), relation2(Y, Z).";
        let query: Query = query_str.parse().expect("Failed to parse query with spaces after commas");
        
        assert_eq!(query.head.name, "result");
        assert_eq!(query.head.terms.len(), 3);
        assert_eq!(query.body.len(), 2);
        assert_eq!(query.body[0].terms.len(), 2);
        assert_eq!(query.body[1].terms.len(), 2);
    }

    #[test]
    fn test_query_from_str_complex() {
        let query_str = "path(X, Z) :- edge(X, Y), edge(Y, Z), vertex(X), vertex(Y), vertex(Z).";
        let query: Query = query_str.parse().expect("Failed to parse complex query");
        
        assert_eq!(query.head.name, "path");
        assert_eq!(query.head.terms.len(), 2);
        assert_eq!(query.body.len(), 5);
        
        assert_eq!(query.body[0].name, "edge");
        assert_eq!(query.body[1].name, "edge");
        assert_eq!(query.body[2].name, "vertex");
        assert_eq!(query.body[3].name, "vertex");
        assert_eq!(query.body[4].name, "vertex");
    }

    #[test]
    fn test_query_from_str_invalid_no_dot() {
        let query_str = "P(X) :- Q(X)";
        let result: Result<Query, _> = query_str.parse();
        assert!(result.is_err());
    }

    #[test]
    fn test_query_from_str_invalid_no_arrow() {
        let query_str = "P(X) Q(X).";
        let result: Result<Query, _> = query_str.parse();
        assert!(result.is_err());
    }

    #[test]
    fn test_query_from_str_invalid_empty_body() {
        let query_str = "P(X) :- .";
        let result: Result<Query, _> = query_str.parse();
        assert!(result.is_err());
    }

    #[test]
    fn test_query_from_str_with_placeholder() {
        let query_str = "result(X, _) :- relation(X, _).";
        let query: Query = query_str.parse().expect("Failed to parse query with placeholder");
        
        assert_eq!(query.head.name, "result");
        assert_eq!(query.head.terms.len(), 2);
        assert_eq!(query.head.terms[0], Term::Var("X".to_string()));
        assert_eq!(query.head.terms[1], Term::Placeholder);
        
        assert_eq!(query.body.len(), 1);
        assert_eq!(query.body[0].terms[0], Term::Var("X".to_string()));
        assert_eq!(query.body[0].terms[1], Term::Placeholder);
    }

    #[test]
    fn test_query_from_str_multiple_placeholders() {
        let query_str = "query(_, _, Z) :- data(_, _, Z).";
        let query: Query = query_str.parse().expect("Failed to parse query with multiple placeholders");
        
        assert_eq!(query.head.terms[0], Term::Placeholder);
        assert_eq!(query.head.terms[1], Term::Placeholder);
        assert_eq!(query.head.terms[2], Term::Var("Z".to_string()));
        
        assert_eq!(query.body[0].terms[0], Term::Placeholder);
        assert_eq!(query.body[0].terms[1], Term::Placeholder);
        assert_eq!(query.body[0].terms[2], Term::Var("Z".to_string()));
    }

    #[test]
    fn test_query_from_str_placeholder_with_atoms() {
        let query_str = "match(alice, _) :- person(alice, _), active(_).";
        let query: Query = query_str.parse().expect("Failed to parse query with placeholders and atoms");
        
        assert_eq!(query.head.terms[0], Term::Atom("alice".to_string()));
        assert_eq!(query.head.terms[1], Term::Placeholder);
        
        assert_eq!(query.body[0].name, "person");
        assert_eq!(query.body[0].terms[0], Term::Atom("alice".to_string()));
        assert_eq!(query.body[0].terms[1], Term::Placeholder);
        
        assert_eq!(query.body[1].name, "active");
        assert_eq!(query.body[1].terms[0], Term::Placeholder);
    }

    #[test]
    fn test_query_from_str_only_placeholders() {
        let query_str = "any(_,_) :- data(_,_).";
        let query: Query = query_str.parse().expect("Failed to parse query with only placeholders");
        
        assert_eq!(query.head.terms.len(), 2);
        assert_eq!(query.head.terms[0], Term::Placeholder);
        assert_eq!(query.head.terms[1], Term::Placeholder);
        
        assert_eq!(query.body[0].terms.len(), 2);
        assert_eq!(query.body[0].terms[0], Term::Placeholder);
        assert_eq!(query.body[0].terms[1], Term::Placeholder);
    }

    #[test]
    fn test_query_from_str_placeholder_mixed() {
        let query_str = "filter(X, _, bob, Y) :- source(X, _, bob, Y), check(X, Y).";
        let query: Query = query_str.parse().expect("Failed to parse complex query with placeholders");
        
        assert_eq!(query.head.terms.len(), 4);
        assert_eq!(query.head.terms[0], Term::Var("X".to_string()));
        assert_eq!(query.head.terms[1], Term::Placeholder);
        assert_eq!(query.head.terms[2], Term::Atom("bob".to_string()));
        assert_eq!(query.head.terms[3], Term::Var("Y".to_string()));
        
        assert_eq!(query.body[0].terms.len(), 4);
        assert_eq!(query.body[0].terms[1], Term::Placeholder);
        assert_eq!(query.body[1].terms.len(), 2);
    }
}
