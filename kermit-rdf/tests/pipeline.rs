//! Hand-crafted N-Triples + SPARQL through stages 4–6 (no watdiv binary).

use {
    kermit_rdf::{
        parquet, partition,
        sparql::translator::translate_query,
        yaml_emit::{write_benchmark_yaml, YamlInputs},
    },
    std::fs,
};

#[test]
fn end_to_end_stages_4_through_5_on_handcrafted_input() {
    let dir = tempfile::tempdir().unwrap();
    let out_dir = dir.path().to_path_buf();

    let nt = "\
<http://x/a> <http://x/follows> <http://x/b> .
<http://x/b> <http://x/follows> <http://x/c> \
              .
<http://x/c> <http://x/follows> <http://x/a> .
";
    let nt_path = out_dir.join("data.nt");
    fs::write(&nt_path, nt).unwrap();

    let part = partition::partition(&nt_path).unwrap();
    let mut dict = part.dict;

    for rel in &part.relations {
        parquet::write_relation(rel, &out_dir.join(format!("{}.parquet", rel.name))).unwrap();
    }
    parquet::write_dict(&dict, &out_dir.join("dict.parquet")).unwrap();

    let q = "SELECT * WHERE { ?x <http://x/follows> ?y . ?y <http://x/follows> ?z . }";
    let dl = translate_query(q, &mut dict, &part.predicate_map, "Q_path").unwrap();
    let queries = vec![("path".to_string(), dl)];

    let predicates: Vec<String> = part.relations.iter().map(|r| r.name.clone()).collect();
    let inputs = YamlInputs {
        name: "test-bench",
        description: "two-hop path",
        queries,
        all_predicates: &predicates,
        base_url: "file:///tmp/x",
    };
    let def = write_benchmark_yaml(&inputs, &out_dir).unwrap();

    assert_eq!(def.queries.len(), 1);
    assert_eq!(
        def.queries[0].query,
        "Q_path(X, Y, Z) :- follows(X, Y), follows(Y, Z)."
    );
    assert!(out_dir.join("benchmark.yml").exists());
    assert!(out_dir.join("dict.parquet").exists());
    assert!(out_dir.join("follows.parquet").exists());
}
