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
    sha2::{Digest, Sha256},
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

/// The reference plan's relations in `Shuffled` order, each as the SHA-256
/// of its values (see [`digest`]), in write order.
const GOLDEN_SHUFFLED: [(&str, &str); 6] = [
    (
        "x_id3",
        "a6995949eb98f11444e9bd59e5353a46928a2648ae45bb9e8d254afb0399589c",
    ),
    (
        "big",
        "daeb0faaa79972199c71600bf24ac7a8a9dba4c1d0e4a25204b2eec574010102",
    ),
    (
        "x_id1",
        "3a9a6901e57520c8c51da77f54e373758cf47e20d5b29441b2814055a4b1fcab",
    ),
    (
        "small",
        "7a7ca985781668a8c510f5905ad5bd56e7fc6a471bac204accf59a6023bad7fa",
    ),
    (
        "x_id2",
        "01ae30e97a521fced72f5faa1e26e2eaa2304b1bb5c6888d593e98a241a4eb97",
    ),
    (
        "medium",
        "ca2644e7d6a743f68a010af7c2ec46fced86a942bb724e2dba5a46af90f93255",
    ),
];

/// The same relations in `Sorted` order.
const GOLDEN_SORTED: [(&str, &str); 6] = [
    (
        "x_id3",
        "46d0bb7d56f4102f31d36ea3896df676d431bd73fc332b430c7d8d7be430db48",
    ),
    (
        "big",
        "078cf7896fb356d148669cea09eb3dfaf06961c984e3fe397e886315f387ca85",
    ),
    (
        "x_id1",
        "69239a8aac021928a0b533e5fede0b3aff66215bd8d0e1f2f47749156e1c2814",
    ),
    (
        "small",
        "c38798bf9a2d0d3f28a36a50aa81ef6e3b5fefc477f7dbc3029f0705b096a5aa",
    ),
    (
        "x_id2",
        "136e352cc2d950268350010d22d50a4e23d0255e5bf2c564535bada8da8fbc6d",
    ),
    (
        "medium",
        "14135e2b2fc781278aed6f1d400f19d5f12f339f3cf0bf4f5b3f040547f11f03",
    ),
];

/// The reference plan's counts, in both orders.
const GOLDEN_COUNTS: [(&str, u64); 3] = [("q1", 1812), ("q2", 1814), ("q5", 1800)];

/// The SHA-256 of a relation's data, not of its file (the Parquet footer
/// records the `parquet` crate's version): every value of `as_flat()`, in
/// order, as `(v as u64).to_le_bytes()`.
fn digest(tuples: &Tuples) -> String {
    let mut hasher = Sha256::new();
    for &value in tuples.as_flat() {
        hasher.update((value as u64).to_le_bytes());
    }
    hex(&hasher.finalize())
}

/// Lowercase hex, as `bench fetch` compares digests.
fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(out, "{byte:02x}").expect("writing to a String never fails");
    }
    out
}

/// Pins the reference plan's data, in both orders, to digests computed by a
/// Python model of the spec that shares no code with this crate, and the
/// three counts to the same model's.
///
/// If these digests must change, the generator's output changed: bump
/// `META_SCHEMA_VERSION` and add an `h2o` floor to `OUTDATED_BEFORE`, so that
/// caches generated before the change are reported as outdated rather than
/// passing as hits.
#[test]
fn the_reference_plan_matches_its_golden_digests() {
    let cases = [
        (H2oOrder::Shuffled, GOLDEN_SHUFFLED),
        (H2oOrder::Sorted, GOLDEN_SORTED),
    ];
    for (order, golden) in cases {
        let out = generate(reference_plan(), SEED, order);
        for (name, expected) in golden {
            assert_eq!(digest(out.tuples(name)), expected, "{name} {order:?}");
        }
        for (query, count) in GOLDEN_COUNTS {
            assert_eq!(out.expected(query), count, "{query} {order:?}");
        }
    }
}

#[test]
fn the_same_spec_reproduces_and_another_seed_differs() {
    for order in H2oOrder::ALL {
        let first = generate(reference_plan(), SEED, order);
        let second = generate(reference_plan(), SEED, order);
        for (name, _) in RELATIONS {
            assert_eq!(first.tuples(name), second.tuples(name), "{name} {order:?}");
        }
        // The files are byte-identical too: the records hold their hashes.
        assert_eq!(first.meta.relations, second.meta.relations, "{order:?}");

        let other = generate(reference_plan(), SEED + 1, order);
        assert_eq!(other.meta.extras.seed, 109);
        assert_ne!(other.tuples("x_id3"), first.tuples("x_id3"), "{order:?}");
        assert_ne!(other.tuples("x_id1"), first.tuples("x_id1"), "{order:?}");
    }
}

#[test]
fn sorted_relations_are_the_shuffled_relations_sorted() {
    let plan = reference_plan();
    let shuffled = generate(plan, SEED, H2oOrder::Shuffled);
    let sorted = generate(plan, SEED, H2oOrder::Sorted);
    for (name, _) in RELATIONS {
        let mut expected = shuffled.tuples(name).clone();
        expected.sort();
        assert_eq!(sorted.tuples(name), &expected, "{name}");
        assert!(
            sorted.tuples(name).rows().is_sorted(),
            "{name} is not sorted"
        );
    }
    // Sorting moved the rows of the big relations, so the check above
    // compares two different orders.
    for name in ["x_id3", "x_id1", "x_id2", "big"] {
        assert_ne!(sorted.tuples(name), shuffled.tuples(name), "{name}");
    }
    // The draws and the counts are the same in both orders.
    for (query, ..) in JOINS {
        assert_eq!(sorted.expected(query), shuffled.expected(query), "{query}");
    }
}

/// With `small` = 5 and `medium` = 9, n / 10 is 0: those pools have no
/// left-only and no right-only key, so every row of `x` joins `small` and
/// `medium`. `big` = 50 still has 5 of each, and q5 = 50 - 5.
#[test]
fn a_plan_without_left_only_keys_generates() {
    let out = generate(KeyPlan::new(50, 5, 9).unwrap(), SEED, H2oOrder::Shuffled);
    for (query, count) in [("q1", 50), ("q2", 50), ("q5", 45)] {
        assert_eq!(out.expected(query), count, "{query}");
    }
    for (query, table, x) in JOINS {
        assert_eq!(brute_force(&out, table, x), out.expected(query), "{query}");
    }
    for (name, keys) in [("small", 5), ("medium", 9)] {
        let mut written: Vec<usize> = out.tuples(name).rows().map(|row| row[0]).collect();
        written.sort_unstable();
        assert_eq!(written, (1..=keys).collect::<Vec<_>>(), "{name}");
    }
}
