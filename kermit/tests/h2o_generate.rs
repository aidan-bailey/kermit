//! End-to-end H2O check (#95): the reference plan goes through
//! `kermit_generate::run` in both row orders, then every query runs on the
//! three (structure, algorithm) cells under every optimiser and column-order
//! policy through the real `kermit::db` entry points, and must return the
//! count the generator recorded as `expected`.
//!
//! It lives in the `kermit` crate for the reason `snap_generate.rs` does: only
//! this crate has both the generator and the join engine, and it needs nothing
//! of the binary. The 1e7 floor stops any small `bench gen h2o`, so the data
//! comes from `kermit_generate::run` directly, and the `ExecutionFamily::load`
//! path that `bench run` takes is covered by the manual `--verify` acceptance
//! at 1e7. The cells are `TreeTrie` and `ColumnTrie` under `LeapfrogTriejoin`,
//! and `HashTrie` under `HashTriejoin`; the shared `JoinEntry` driver in
//! `common/utils.rs` hosts both families.
//!
//! # The reference plan
//!
//! `KeyPlan::new(2000, 20, 200)` with seed 108: N = 2,000 rows of `x`, `small`
//! with 20 keys and `medium` with 200. `REFERENCE_COUNTS` pins the counts
//! against a Python model written from the spec alone, which shares no code
//! with the generator: q5 is N - N / 10 = 1800 by construction (`x_id3` is a
//! permutation, so the share of its keys that `big` holds does not depend on
//! the draws), and q1 and q2 (1812 and 1814) depend on every draw. The joins
//! and the generator's own bitmap count both read the generated data, so only
//! a pin like this fails when the data and its count drift together.

mod common;

use {
    clap::ValueEnum,
    common::utils::{join_under_planner, load_parquet_relations, JoinEntry},
    kermit_algos::{
        ColumnOrderPolicy, HashTriejoin, JoinQuery, LeapfrogTriejoin, Optimiser, Planner,
    },
    kermit_bench::{discovery::load_benchmark, BenchmarkDefinition, GeneratorSpec, H2oOrder},
    kermit_ds::{Cardinality, ColumnTrie, GallopingSeek, HashTrie, Relation, TreeTrie},
    kermit_generate::h2o::{H2oGenerator, KeyPlan},
    kermit_iters::SipHashStrategy,
    kermit_rdf::generator::Target,
    std::{
        fs,
        path::{Path, PathBuf},
    },
};

/// The benchmark name every generation here uses. `kermit_generate::run`
/// requires the output directory's last component to equal it.
const BENCH_NAME: &str = "h2o-test";

/// Upstream's seed (`set.seed(108)`), which the committed YAMLs default to.
const SEED: u32 = 108;

/// The reference plan's answer counts, in query order, from the independent
/// model (see the module doc).
const REFERENCE_COUNTS: [(&str, u64); 3] = [("q1", 1812), ("q2", 1814), ("q5", 1800)];

/// The relations of the emitted `benchmark.yml`, in write order.
const RELATIONS: [&str; 6] = ["x_id3", "big", "x_id1", "small", "x_id2", "medium"];

// The three cells' relation types. `TreeTrie` and `ColumnTrie` default to
// `GallopingSeek`; the alias names it so no inference fallback is needed.
type Tree = TreeTrie<GallopingSeek>;
type Column = ColumnTrie<GallopingSeek>;
type Hash = HashTrie<SipHashStrategy>;

/// A generation of the reference plan into `<root>/h2o-test`.
struct Generation {
    /// Keeps the temp dir alive.
    _root: tempfile::TempDir,
    /// The output directory (a cache subdirectory in all but location).
    dir: PathBuf,
}

/// Runs the generator on the reference plan in `order`.
fn generate_reference(order: H2oOrder) -> Generation {
    let root = tempfile::tempdir().expect("create temp dir");
    let dir = root.path().join(BENCH_NAME);
    let plan = KeyPlan::new(2000, 20, 200).expect("the reference plan is valid");
    {
        let generator = H2oGenerator::new(
            Target {
                out_dir: &dir,
                bench_name: BENCH_NAME,
                tag: "test",
                spec_hash: None,
            },
            plan,
            SEED,
            order,
        );
        kermit_generate::run(&generator).expect("the reference plan must generate");
    }
    Generation {
        _root: root,
        dir,
    }
}

/// Parses the `benchmark.yml` a generation emitted into `dir`.
fn emitted_benchmark(dir: &Path) -> BenchmarkDefinition {
    let yaml =
        fs::read_to_string(dir.join("benchmark.yml")).expect("emitted benchmark.yml missing");
    serde_yaml::from_str(&yaml).expect("benchmark.yml malformed")
}

/// Each emitted query's name and `expected`, in order.
fn emitted_counts(bench: &BenchmarkDefinition) -> Vec<(String, u64)> {
    bench
        .queries
        .iter()
        .map(|q| {
            let expected = q
                .expected
                .unwrap_or_else(|| panic!("query {} has no expected count", q.name));
            (q.name.clone(), expected)
        })
        .collect()
}

/// Loads the generated relations as `R` into a fresh engine planned by
/// `planner`, runs every query through `JA`'s entry points (building the
/// reordered copies it reads under `--column-orders any`), and returns one
/// line per query whose result count differs from its `expected`. A join that
/// errors panics, naming the cell: under `stored` that is the
/// `CyclicAttributeOrder` check, which no H2O query may trip.
fn cell_mismatches<R, JA>(
    bench: &BenchmarkDefinition, dir: &Path, planner: &Planner, label: &str,
) -> Vec<String>
where
    R: Relation + Cardinality,
    JA: JoinEntry<R>,
{
    let (relations, inputs) =
        load_parquet_relations::<R>(dir, bench.relations.iter().map(|r| r.name.as_str()));
    let mut database = JA::database(relations, planner.required_statistics());

    let mut mismatches = Vec::new();
    for q in &bench.queries {
        let want = q.expected.expect("every H2O query carries expected");
        let parsed: JoinQuery = q.query.parse().expect("datalog parse failure");
        let (rows, _) = join_under_planner::<R, JA>(&mut database, &inputs, parsed, planner)
            .unwrap_or_else(|e| {
                panic!("[{label}] {} / {}: {e}", std::any::type_name::<R>(), q.name)
            });
        let got = rows.len() as u64;
        if got != want {
            mismatches.push(format!(
                "  [{label}] {} / {}: got {got}, expected {want}\n    query: {}",
                std::any::type_name::<R>(),
                q.name,
                q.query
            ));
        }
    }
    mismatches
}

/// The committed YAMLs declare what their names say, at the floor, and
/// validate (`load_benchmark` validates).
#[test]
fn committed_yamls_declare_h2o_at_the_floor() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("kermit is inside the workspace");
    for (name, order) in [
        ("h2o", H2oOrder::Shuffled),
        ("h2o-sorted", H2oOrder::Sorted),
    ] {
        let def = load_benchmark(root, name).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(
            def.generator,
            Some(GeneratorSpec::H2o {
                scale: 10_000_000,
                seed: 108,
                order,
            }),
            "{name}"
        );
        assert!(
            def.description.contains(order.name()),
            "{name}: description {:?} should say its rows are {}",
            def.description,
            order.name()
        );
        assert!(def.relations.is_empty() && def.queries.is_empty(), "{name}");
    }
}

/// The emitted `expected` values are the independent model's, in both
/// orders, and the relations are the six the queries need, in write order.
#[test]
fn the_emitted_counts_are_the_reference_models() {
    let want: Vec<(String, u64)> = REFERENCE_COUNTS
        .iter()
        .map(|&(name, count)| (name.to_string(), count))
        .collect();
    for order in H2oOrder::ALL {
        let generation = generate_reference(order);
        let bench = emitted_benchmark(&generation.dir);
        assert_eq!(emitted_counts(&bench), want, "{} rows", order.name());
        let relations: Vec<&str> = bench.relations.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(relations, RELATIONS, "{} rows", order.name());
    }
}

/// Every query returns its `expected` count on every cell, under every
/// optimiser and both column-order policies, in both row orders.
///
/// The plan an optimiser picks changes the join's descent order, never its
/// answer; iterating the CLI enums covers optimisers and policies added later
/// with no edit here. Under `stored` all three queries must validate for every
/// optimiser, which pins that each is written with its variables in the stored
/// column order. Under `any` the copies a plan needs are the planner's
/// business and are not asserted, only the answers.
#[test]
fn every_cell_optimiser_and_policy_returns_the_expected_counts() {
    let optimisers = Optimiser::value_variants();
    let policies = ColumnOrderPolicy::value_variants();
    let mut mismatches: Vec<String> = Vec::new();
    let mut joins = 0;
    for order in H2oOrder::ALL {
        let generation = generate_reference(order);
        let bench = emitted_benchmark(&generation.dir);
        assert_eq!(bench.queries.len(), REFERENCE_COUNTS.len());
        assert!(
            bench
                .queries
                .iter()
                .all(|q| q.expected.is_some_and(|count| count > 0)),
            "a zero expected count would make the comparison vacuous: {:?}",
            emitted_counts(&bench)
        );

        for &policy in policies {
            for &optimiser in optimisers {
                let planner = Planner::new(optimiser.instantiate(), policy);
                let label = format!(
                    "{} / {} / {}",
                    order.name(),
                    optimiser.axis_value(),
                    policy.axis_value()
                );
                let dir = &generation.dir;
                mismatches.extend(cell_mismatches::<Tree, LeapfrogTriejoin>(
                    &bench, dir, &planner, &label,
                ));
                mismatches.extend(cell_mismatches::<Column, LeapfrogTriejoin>(
                    &bench, dir, &planner, &label,
                ));
                mismatches.extend(cell_mismatches::<Hash, HashTriejoin>(
                    &bench, dir, &planner, &label,
                ));
                joins += 3 * bench.queries.len();
            }
        }
    }
    assert!(
        mismatches.is_empty(),
        "H2O cardinality mismatches ({} of {joins} joins; {} orders x {} optimisers x {} \
         column-order policies x 3 cells x {} queries):\n{}",
        mismatches.len(),
        H2oOrder::ALL.len(),
        optimisers.len(),
        policies.len(),
        REFERENCE_COUNTS.len(),
        mismatches.join("\n"),
    );
}
