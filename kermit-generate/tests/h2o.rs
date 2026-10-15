//! The H2O generator through the layer's public API: `run` over an
//! `H2oGenerator`, then the Parquet files, `benchmark.yml` and `meta.json` it
//! leaves behind, read back the way `bench run` reads them.

use {
    kermit_bench::{BenchmarkDefinition, H2oOrder},
    kermit_ds::{read_parquet, RelationHeader, Tuples},
    kermit_generate::{
        h2o::{H2oExtras, H2oGenerator, KeyPlan},
        run, GeneratedMeta,
    },
    kermit_rdf::generator::{read_meta_json, Target},
    std::{collections::HashMap, fs, path::PathBuf},
};

/// Upstream's `set.seed(108)`, the default seed.
const SEED: u32 = 108;

/// The benchmark's name, which is also the last component of its directory.
const BENCH_NAME: &str = "h2o-test";

/// The six relations in write order, with their columns.
const RELATIONS: [(&str, &[&str]); 6] = [
    ("x_id3", &["id3"]),
    ("big", &["id3"]),
    ("x_id1", &["id1", "id3"]),
    ("small", &["id1"]),
    ("x_id2", &["id2", "id3"]),
    ("medium", &["id2"]),
];

/// The queries in order: name, description and rule.
const QUERIES: [(&str, &str, &str); 3] = [
    (
        "q1",
        "H2O J1 q1 (small inner on int): few keys, each with many x rows",
        "Q(K, I) :- small(K), x_id1(K, I).",
    ),
    (
        "q2",
        "H2O J1 q2 (medium inner on int): thousands of keys, each with about a thousand x rows",
        "Q(K, I) :- medium(K), x_id2(K, I).",
    ),
    (
        "q5",
        "H2O J1 q5 (big inner on int): two dense key lists that overlap by 90%",
        "Q(K) :- big(K), x_id3(K).",
    ),
];

/// Each query with the right-hand table and the fact relation it joins.
const JOINS: [(&str, &str, &str); 3] = [
    ("q1", "small", "x_id1"),
    ("q2", "medium", "x_id2"),
    ("q5", "big", "x_id3"),
];

/// 2,000 rows of `x`, 20 `small` keys and 200 `medium` keys.
fn reference_plan() -> KeyPlan { KeyPlan::new(2000, 20, 200).unwrap() }

/// One generated benchmark, read back from disk.
struct Output {
    /// Keeps the directory alive.
    _dir: tempfile::TempDir,
    out_dir: PathBuf,
    meta: GeneratedMeta<H2oExtras>,
    definition: BenchmarkDefinition,
    /// Each relation as `read_parquet` returns it, by name.
    relations: HashMap<String, (RelationHeader, Tuples)>,
}

impl Output {
    fn tuples(&self, name: &str) -> &Tuples { &self.relations[name].1 }

    /// The `expected` count `benchmark.yml` records for `query`.
    fn expected(&self, query: &str) -> u64 {
        let definition = self
            .definition
            .queries
            .iter()
            .find(|q| q.name == query)
            .unwrap_or_else(|| panic!("benchmark.yml has no query {query}"));
        definition
            .expected
            .unwrap_or_else(|| panic!("{query} has no expected count"))
    }
}

/// Generates `plan` under `seed` and `order` into `<tempdir>/h2o-test`, with
/// tag `test` and no spec hash.
fn generate(plan: KeyPlan, seed: u32, order: H2oOrder) -> Output {
    let dir = tempfile::tempdir().unwrap();
    let out_dir = dir.path().join(BENCH_NAME);
    let target = Target {
        out_dir: &out_dir,
        bench_name: BENCH_NAME,
        tag: "test",
        spec_hash: None,
    };
    let meta = run(&H2oGenerator::new(target, plan, seed, order)).unwrap();
    let yaml = fs::read_to_string(out_dir.join("benchmark.yml")).unwrap();
    let definition: BenchmarkDefinition = serde_yaml::from_str(&yaml).unwrap();
    let relations = definition
        .relations
        .iter()
        .map(|relation| {
            let path = out_dir.join(format!("{}.parquet", relation.name));
            (relation.name.clone(), read_parquet(path).unwrap())
        })
        .collect();
    Output {
        _dir: dir,
        out_dir,
        meta,
        definition,
        relations,
    }
}

/// The join's cardinality by definition: how often each key occurs in
/// `table`'s first column, summed over the keys in `x`'s first column.
/// It shares no code with the generator's bitmap.
fn brute_force(out: &Output, table: &str, x: &str) -> u64 {
    let mut occurrences: HashMap<usize, u64> = HashMap::new();
    for row in out.tuples(table).rows() {
        *occurrences.entry(row[0]).or_default() += 1;
    }
    out.tuples(x)
        .rows()
        .map(|row| occurrences.get(&row[0]).copied().unwrap_or(0))
        .sum()
}

/// Whether `x`'s second column is `x3`'s only column, row for row.
fn carries_id3(x: &Tuples, x3: &Tuples) -> bool {
    x.rows()
        .zip(x3.rows())
        .all(|(pair, single)| pair[1] == single[0])
}

#[test]
fn writes_six_relations_and_three_queries() {
    let plan = reference_plan();
    for order in H2oOrder::ALL {
        let out = generate(plan, SEED, order);

        // The relations, in write order, with their columns.
        let written: Vec<&str> = RELATIONS.iter().map(|&(name, _)| name).collect();
        let listed: Vec<&str> = out
            .definition
            .relations
            .iter()
            .map(|relation| relation.name.as_str())
            .collect();
        assert_eq!(listed, written, "{order:?}");
        let recorded: Vec<&str> = out.meta.relations.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(recorded, written, "{order:?}");
        for (name, attrs) in RELATIONS {
            let (header, _) = &out.relations[name];
            assert_eq!(header.attrs(), attrs, "{name} {order:?}");
            let record = out.meta.relations.iter().find(|r| r.name == name).unwrap();
            assert_eq!(record.columns, attrs, "{name} {order:?}");
        }

        // The queries, in order, each with a count.
        let queries: Vec<(&str, &str, &str)> = out
            .definition
            .queries
            .iter()
            .map(|q| (q.name.as_str(), q.description.as_str(), q.query.as_str()))
            .collect();
        assert_eq!(queries, QUERIES, "{order:?}");
        assert!(out.definition.queries.iter().all(|q| q.expected.is_some()));
        assert_eq!(
            out.definition.description,
            format!(
                "H2O db-benchmark join (J1): N = 2000, small = 20, medium = 200, seed 108, {} rows",
                order.name()
            )
        );

        // meta.json: the kind, the counts and the extras, which also read
        // back from the file through the flattened layout.
        assert_eq!(out.meta.kind, "h2o");
        assert_eq!((out.meta.relation_count, out.meta.query_count), (6, 3));
        let extras = H2oExtras {
            seed: SEED,
            order,
            plan,
        };
        assert_eq!(out.meta.extras, extras, "{order:?}");
        let read: GeneratedMeta<H2oExtras> = read_meta_json(&out.out_dir).unwrap();
        assert_eq!(read.extras, extras, "{order:?}");
    }
}

#[test]
fn relation_sizes_follow_the_plan() {
    let plan = reference_plan();
    for order in H2oOrder::ALL {
        let out = generate(plan, SEED, order);
        let sizes = [2000, 2000, 2000, 20, 2000, 200];
        for ((name, attrs), rows) in RELATIONS.into_iter().zip(sizes) {
            let tuples = out.tuples(name);
            assert_eq!(
                (tuples.len(), tuples.arity()),
                (rows, attrs.len()),
                "{name} {order:?}"
            );
        }
        let recorded: Vec<u64> = out.meta.relations.iter().map(|r| r.rows).collect();
        assert_eq!(recorded, [2000, 2000, 2000, 20, 2000, 200], "{order:?}");
    }

    // Under `Shuffled`, x_id1 and x_id2 carry x_id3's id3 column row for row.
    let shuffled = generate(plan, SEED, H2oOrder::Shuffled);
    let x3 = shuffled.tuples("x_id3");
    for name in ["x_id1", "x_id2"] {
        assert!(carries_id3(shuffled.tuples(name), x3), "{name}");
    }
    // Under `Sorted` they no longer do, which shows the check above can fail.
    let sorted = generate(plan, SEED, H2oOrder::Sorted);
    let x3 = sorted.tuples("x_id3");
    for name in ["x_id1", "x_id2"] {
        assert!(!carries_id3(sorted.tuples(name), x3), "{name}");
    }
}

#[test]
fn expected_counts_equal_a_brute_force_join() {
    for order in H2oOrder::ALL {
        let out = generate(reference_plan(), SEED, order);
        for (query, table, x) in JOINS {
            let counted = brute_force(&out, table, x);
            assert_eq!(counted, out.expected(query), "{query} {order:?}");
            // Some of x's rows join and some (the left-only keys) don't.
            assert!(counted > 0, "{query} {order:?}: nothing joins");
            assert!(counted < 2000, "{query} {order:?}: every row joins");
        }
    }
}
