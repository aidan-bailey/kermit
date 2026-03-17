//! Datalog query parser for Kermit.
//!
//! Parses queries in the form `Head :- Body1, Body2, ... .` into a
//! [`JoinQuery`] AST. Built on the [winnow](https://docs.rs/winnow) parser
//! combinator library.
//!
//! # Syntax
//!
//! - **Variables** start with an uppercase letter: `X`, `Name`
//! - **Atoms** (constants) start with a lowercase letter: `alice`, `edge`
//! - **Placeholders** are the anonymous wildcard `_`
//!
//! ```text
//! path(X, Z) :- edge(X, Y), edge(Y, Z).
//! ```

mod join_query;

pub use join_query::{JoinQuery, Predicate, Term};
use winnow::{
    ascii::multispace0,
    combinator::{delimited, separated},
    error::{ContextError, ErrMode},
    token::take_while,
    Parser,
};

type PResult<T> = Result<T, winnow::error::ErrMode<ContextError>>;

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
    if input.starts_with('_')
        && (input.len() == 1
            || !input
                .chars()
                .nth(1)
                .is_some_and(|c| c.is_ascii_alphanumeric()))
    {
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

fn query(input: &mut &str) -> PResult<JoinQuery> {
    let head = predicate.parse_next(input)?;
    // ":-" separates head from body
    let _ = delimited(ws, ":-", ws).parse_next(input)?;
    let body = separated(1.., predicate, comma).parse_next(input)?;
    let _ = dot.parse_next(input)?;
    Ok(JoinQuery {
        head,
        body,
    })
}

impl std::str::FromStr for JoinQuery {
    type Err = ErrMode<ContextError>;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut input = s;
        let result = query.parse_next(&mut input)?;
        ws.parse_next(&mut input)?;
        if !input.is_empty() {
            return Err(ErrMode::Backtrack(ContextError::new()));
        }
        Ok(result)
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
            | Ok(ast) => {
                println!("{ast:#?}");
                // Remaining input (should be empty or just whitespace)
                let _ = ws.parse_next(&mut src);
                if !src.is_empty() {
                    eprintln!("Unparsed tail: {src:?}");
                }
                assert_eq!(ast.head.name, "P");
                assert_eq!(ast.head.terms.len(), 2);
                assert_eq!(ast.body.len(), 2);
            },
            | Err(e) => panic!("Parse error: {e:?}"),
        }
    }

    #[test]
    fn test_query_from_str_simple() {
        let query_str = "P(X) :- Q(X).";
        let query: JoinQuery = query_str.parse().expect("Failed to parse query");

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
        let query: JoinQuery = query_str.parse().expect("Failed to parse query");

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
        let query: JoinQuery = query_str.parse().expect("Failed to parse query");

        assert_eq!(query.head.name, "likes");
        assert_eq!(query.head.terms[0], Term::Atom("alice".to_string()));
        assert_eq!(query.head.terms[1], Term::Var("X".to_string()));

        assert_eq!(query.body.len(), 2);
    }

    #[test]
    fn test_whitespace_variants() {
        // All of these encode "P(X,Y) :- Q(X), R(Y)." with varying whitespace.
        // The parser should handle all of them identically.
        let cases = [
            ("extra whitespace", "  P(X,Y)  :-  Q(X),R(Y)  .  "),
            ("minimal whitespace", "P(X,Y):-Q(X),R(Y)."),
            ("spaces after commas", "P( X , Y ) :- Q( X ) , R( Y ) ."),
        ];
        for (label, input) in cases {
            let query: JoinQuery = input
                .parse()
                .unwrap_or_else(|e| panic!("Failed to parse {label}: {e:?}"));
            assert_eq!(query.head.name, "P", "{label}");
            assert_eq!(query.head.terms.len(), 2, "{label}");
            assert_eq!(query.body.len(), 2, "{label}");
        }
    }

    #[test]
    fn test_query_from_str_complex() {
        let query_str = "path(X, Z) :- edge(X, Y), edge(Y, Z), vertex(X), vertex(Y), vertex(Z).";
        let query: JoinQuery = query_str.parse().expect("Failed to parse complex query");

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
    fn test_invalid_syntax() {
        let cases = [
            ("missing dot", "P(X) :- Q(X)"),
            ("missing :-", "P(X) Q(X)."),
            ("empty body", "P(X) :- ."),
            ("trailing garbage", "P(X) :- Q(X). GARBAGE"),
            ("two queries", "P(X) :- Q(X). R(Y) :- S(Y)."),
            ("double dot", "P(X) :- Q(X).."),
        ];
        for (label, input) in cases {
            let result: Result<JoinQuery, _> = input.parse();
            assert!(result.is_err(), "{label} should fail to parse");
        }
    }

    #[test]
    fn test_query_from_str_with_placeholder() {
        let query_str = "result(X, _) :- relation(X, _).";
        let query: JoinQuery = query_str
            .parse()
            .expect("Failed to parse query with placeholder");

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
        let query: JoinQuery = query_str
            .parse()
            .expect("Failed to parse query with multiple placeholders");

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
        let query: JoinQuery = query_str
            .parse()
            .expect("Failed to parse query with placeholders and atoms");

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
        let query: JoinQuery = query_str
            .parse()
            .expect("Failed to parse query with only placeholders");

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
        let query: JoinQuery = query_str
            .parse()
            .expect("Failed to parse complex query with placeholders");

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

#[cfg(test)]
mod edge_case_tests {
    use super::*;

    fn parse(s: &str) -> Result<JoinQuery, String> {
        s.parse::<JoinQuery>().map_err(|e| format!("{e:?}"))
    }

    #[test]
    fn identifiers_with_digits() {
        let q = parse("edge2(X1, Y2) :- node3(X1), link4(X1, Y2).").unwrap();
        assert_eq!(q.head.name, "edge2");
        assert_eq!(q.head.terms[0], Term::Var("X1".to_string()));
        assert_eq!(q.body[0].name, "node3");
    }

    #[test]
    fn multiline_query() {
        let q = parse("P(X, Y) :-\n  Q(X),\n  R(Y).").unwrap();
        assert_eq!(q.head.name, "P");
        assert_eq!(q.body.len(), 2);
    }

    #[test]
    fn duplicate_variables() {
        let q = parse("P(X, X) :- Q(X, X).").unwrap();
        assert_eq!(q.head.terms[0], Term::Var("X".to_string()));
        assert_eq!(q.head.terms[1], Term::Var("X".to_string()));
    }

    #[test]
    fn single_char_names() {
        let q = parse("P(X) :- Q(X).").unwrap();
        assert_eq!(q.head.name, "P");
        assert_eq!(q.body[0].name, "Q");
    }

    #[test]
    fn rejects_empty_input() {
        assert!(parse("").is_err());
    }

    #[test]
    fn rejects_whitespace_only() {
        assert!(parse("   ").is_err());
    }

    #[test]
    fn rejects_underscore_as_predicate_name() {
        assert!(parse("_(X) :- Q(X).").is_err());
    }

    #[test]
    fn rejects_underscore_alpha_as_term() {
        assert!(parse("P(_abc) :- Q(X).").is_err());
    }

    #[test]
    fn rejects_underscore_numeric_as_term() {
        assert!(parse("P(_123) :- Q(X).").is_err());
    }

    #[test]
    fn rejects_unicode_identifier() {
        assert!(parse("\u{00d1}ame(X) :- Q(X).").is_err());
    }

    #[test]
    fn rejects_numeric_start_identifier() {
        assert!(parse("123abc(X) :- Q(X).").is_err());
    }

    #[test]
    fn many_body_predicates() {
        let body_preds: Vec<String> = (0..20).map(|i| format!("r{i}(X)")).collect();
        let query_str = format!("P(X) :- {}.", body_preds.join(", "));
        let q = parse(&query_str).unwrap();
        assert_eq!(q.body.len(), 20);
    }

    #[test]
    fn many_terms_in_predicate() {
        let vars: Vec<String> = (0..15).map(|i| format!("X{i}")).collect();
        let terms = vars.join(", ");
        let query_str = format!("P({terms}) :- Q({terms}).");
        let q = parse(&query_str).unwrap();
        assert_eq!(q.head.terms.len(), 15);
        assert_eq!(q.body[0].terms.len(), 15);
    }
}
