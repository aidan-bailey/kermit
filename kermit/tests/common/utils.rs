use {
    kermit_algos::{CatalogStats, JoinAlgo, JoinQuery, QueryOptimiser},
    kermit_ds::{Cardinality, Relation},
    std::collections::HashMap,
};

pub fn test_join<R, JA, O>(
    input: Vec<Vec<Vec<usize>>>, variables: Vec<usize>, rel_variables: Vec<Vec<usize>>,
    result: Vec<Vec<usize>>,
) where
    R: Relation + Cardinality,
    JA: JoinAlgo<R>,
    O: QueryOptimiser + Default,
{
    let relations: Vec<_> = input
        .into_iter()
        .map(|tuples| {
            let k = if tuples.is_empty() {
                0
            } else {
                tuples[0].len()
            };
            R::from_tuples(k.into(), tuples)
        })
        .collect();
    // Build synthetic query and map like the library helper
    let head_vars: Vec<String> = variables.iter().map(|v| format!("V{}", v)).collect();
    let mut body_preds: Vec<String> = Vec::new();
    for (i, rv) in rel_variables.iter().enumerate() {
        let var_list = if rv.is_empty() {
            "_".to_string()
        } else {
            rv.iter()
                .map(|v| format!("V{}", v))
                .collect::<Vec<_>>()
                .join(", ")
        };
        body_preds.push(format!("R{}({})", i, var_list));
    }
    let query_str = format!("Q({}) :- {}.", head_vars.join(", "), body_preds.join(", "));
    let query: JoinQuery = query_str.parse().expect("Failed to build JoinQuery");

    let mut ds_map: HashMap<String, &R> = HashMap::new();
    for (i, rel) in relations.iter().enumerate() {
        ds_map.insert(format!("R{}", i), rel);
    }

    let stats = CatalogStats::for_query(&query, |name| ds_map.get(name).map(|r| r.tuple_count()));
    let plan = O::default().plan(&query, &stats);

    // Multiset equality (relational algebra semantics) — sort both sides
    // before asserting so algorithms with non-sorted output (hash-trie
    // family) and plans with different enumeration orders pass the same
    // suite.
    let mut actual: Vec<Vec<usize>> = JA::join_iter(&plan, query, ds_map).collect();
    actual.sort();
    let mut expected = result;
    expected.sort();
    assert_eq!(actual, expected);
}
