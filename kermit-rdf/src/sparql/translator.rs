//! SPARQL BGP → Datalog rule translation.
//!
//! Accepts only Basic Graph Pattern (BGP) queries with optional projection;
//! FILTER, OPTIONAL, UNION, GROUP BY, etc. are rejected via
//! [`RdfError::UnsupportedSparql`]. The shape produced is
//! `head(...) :- body_atom_1, body_atom_2, ...`.
//!
//! `predicate_map` MUST come from `kermit_rdf::partition::partition`'s
//! result so the translator agrees with the partitioner on collision-
//! resolved names.

use {
    crate::{
        dict::Dictionary,
        error::RdfError,
        sparql::{
            bindings::{var_name, VarOrder},
            parser::parse_query,
        },
        value::RdfValue,
    },
    spargebra::{
        algebra::GraphPattern,
        term::{NamedNodePattern, TermPattern, TriplePattern},
        Query,
    },
    std::collections::HashMap,
};

/// Translates one SPARQL query to a Datalog rule.
///
/// `dict` is mutated: URI constants in the query that were never seen in
/// the source data get fresh dictionary IDs, so the caller should re-emit
/// `dict.parquet` if `dict.len()` grew.
pub fn translate_query(
    sparql: &str, dict: &mut Dictionary, predicate_map: &HashMap<String, String>, head_name: &str,
) -> Result<String, RdfError> {
    let parsed = parse_query(sparql)?;
    let pattern = match parsed {
        | Query::Select {
            pattern, ..
        } => pattern,
        | _ => {
            return Err(RdfError::UnsupportedSparql(
                "only SELECT queries are supported".to_string(),
            ));
        },
    };

    let (bgp, projected_vars) = extract_bgp_and_projection(pattern)?;

    let mut order = VarOrder::default();
    let mut body_parts: Vec<String> = Vec::new();

    for triple in &bgp {
        let pred_iri = match &triple.predicate {
            | NamedNodePattern::NamedNode(n) => n.as_str().to_string(),
            | NamedNodePattern::Variable(v) => {
                return Err(RdfError::UnsupportedSparql(format!(
                    "non-ground predicate variable: ?{}",
                    v.as_str()
                )));
            },
        };
        let pred_name = predicate_map.get(&pred_iri).ok_or_else(|| {
            RdfError::UnsupportedSparql(format!("predicate URI not in partition map: {pred_iri}"))
        })?;
        let s_term = term_to_datalog(&triple.subject, dict, &mut order)?;
        let o_term = term_to_datalog(&triple.object, dict, &mut order)?;
        body_parts.push(format!("{pred_name}({s_term}, {o_term})"));
    }

    let head_args: Vec<String> = match projected_vars {
        | Some(p) => {
            for v in &p {
                if !order.contains(v) {
                    return Err(RdfError::UnsupportedSparql(format!(
                        "projected variable {v} not bound by BGP"
                    )));
                }
            }
            p
        },
        | None => order.order().to_vec(),
    };

    let head_terms = head_args.join(", ");
    let body = body_parts.join(", ");
    Ok(format!("{head_name}({head_terms}) :- {body}."))
}

/// Returns `(triples, projected_vars)` where `projected_vars = None` means
/// SELECT *.
fn extract_bgp_and_projection(
    pattern: GraphPattern,
) -> Result<(Vec<TriplePattern>, Option<Vec<String>>), RdfError> {
    match pattern {
        | GraphPattern::Project {
            inner,
            variables,
        } => {
            let triples = expect_bgp(*inner)?;
            let proj = variables.iter().map(|v| var_name(v.as_str())).collect();
            Ok((triples, Some(proj)))
        },
        | other => {
            let triples = expect_bgp(other)?;
            Ok((triples, None))
        },
    }
}

fn expect_bgp(pattern: GraphPattern) -> Result<Vec<TriplePattern>, RdfError> {
    match pattern {
        | GraphPattern::Bgp {
            patterns,
        } => Ok(patterns),
        | GraphPattern::Filter {
            ..
        } => Err(RdfError::UnsupportedSparql(
            "FILTER not supported".to_string(),
        )),
        | GraphPattern::LeftJoin {
            ..
        } => Err(RdfError::UnsupportedSparql(
            "OPTIONAL not supported".to_string(),
        )),
        | GraphPattern::Union {
            ..
        } => Err(RdfError::UnsupportedSparql(
            "UNION not supported".to_string(),
        )),
        | other => Err(RdfError::UnsupportedSparql(format!(
            "unsupported pattern: {other:?}"
        ))),
    }
}

fn term_to_datalog(
    term: &TermPattern, dict: &mut Dictionary, order: &mut VarOrder,
) -> Result<String, RdfError> {
    match term {
        | TermPattern::Variable(v) => {
            let name = var_name(v.as_str());
            order.note(&name);
            Ok(name)
        },
        | TermPattern::NamedNode(n) => {
            let value = RdfValue::Iri(n.as_str().to_string());
            let id = dict.intern(value);
            Ok(format!("c{id}"))
        },
        | TermPattern::Literal(_) => Err(RdfError::UnsupportedSparql(
            "literal terms in BGP not supported".to_string(),
        )),
        | TermPattern::BlankNode(_) => Err(RdfError::UnsupportedSparql(
            "blank node terms in BGP not supported".to_string(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_non_select() {
        let mut d = Dictionary::new();
        let pm = HashMap::new();
        let err = translate_query("ASK { ?x <http://p> ?y }", &mut d, &pm, "Q").unwrap_err();
        assert!(matches!(err, RdfError::UnsupportedSparql(_)));
    }
}
