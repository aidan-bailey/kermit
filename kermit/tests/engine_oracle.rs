//! The engines against the nested-loop oracle: the shared shapes over seeded
//! random relations, compared on contents as well as counts. Built only with an
//! engine feature.
#![cfg(feature = "engine-duckdb")]

mod common;

use {
    common::{
        oracle::{oracle, Lcg},
        shapes::{catalog_for, full_query, random_data, shapes, SEEDS},
    },
    kermit::db::{prepare, Catalog, Prepared},
    kermit_algos::{CardinalityOptimiser, JoinQuery, LexicographicOptimiser, Planner},
    kermit_ds::{HashTrie, TrieLevels},
    kermit_iters::SipHashStrategy,
};

type HashTrieSip = HashTrie<SipHashStrategy>;

/// One shape over one seeded database under one planner: what an engine is
/// checked against.
struct Case {
    shape: &'static str,
    label: String,
    catalog: Catalog,
    prepared: Prepared,
    /// The oracle's full assignments in the executed query's canonical order,
    /// sorted, with bag semantics.
    bag: Vec<Vec<usize>>,
}

/// The two planners the cases run under: the index specs' column orders, and so
/// the SQL, differ.
fn planners() -> Vec<(&'static str, Planner)> {
    vec![
        (
            "lexicographic",
            Planner::new(Box::new(LexicographicOptimiser)),
        ),
        ("cardinality", Planner::new(Box::new(CardinalityOptimiser))),
    ]
}

/// Every shape, over every seed, under every planner.
fn cases() -> Vec<Case> {
    let mut cases = Vec::new();
    for (index, shape) in shapes().iter().enumerate() {
        let query: JoinQuery = shape.query.parse().expect("a shape parses");
        for seed in SEEDS {
            let mut rng = Lcg(1000 * index as u64 + seed);
            let relations = random_data(&mut rng, shape.arities);
            for (planner_name, planner) in planners() {
                let label = format!("{} seed {seed} under {planner_name}", shape.name);
                let catalog = catalog_for(&relations);
                let prepared = prepare(&catalog, &query, &planner, TrieLevels::All)
                    .unwrap_or_else(|e| panic!("{label}: {e}"));
                // The full query's head is the executed query's canonical
                // order, which the plan can change for a body-only variable, so
                // the oracle runs once per planner.
                let bag = oracle::<HashTrieSip>(&relations, &full_query(&query, &prepared));
                cases.push(Case {
                    shape: shape.name,
                    label,
                    catalog,
                    prepared,
                    bag,
                });
            }
        }
    }
    cases
}

/// How many distinct rows `rows` holds.
fn distinct_rows(rows: &[Vec<usize>]) -> usize {
    let mut distinct = rows.to_vec();
    distinct.sort();
    distinct.dedup();
    distinct.len()
}

/// The final bounds catch a generator that stops producing answers, which would
/// turn every comparison into `[] == []`, and fixtures with no duplicate
/// derivation, which would make bag and set counts the same.
#[test]
fn the_cases_have_answers_and_duplicate_derivations() {
    let cases = cases();
    let with_rows = cases.iter().filter(|case| !case.bag.is_empty()).count();
    assert!(
        with_rows * 4 >= cases.len(),
        "only {with_rows} of {} cases returned a row",
        cases.len()
    );
    assert!(
        cases
            .iter()
            .any(|case| distinct_rows(&case.bag) < case.bag.len()),
        "no case derives a row twice, so the bag and set counts never differ"
    );
}

mod duckdb_cells {
    use {
        super::{cases, distinct_rows, Case},
        kermit::{
            db::check_multiplicity,
            engine::{
                duckdb::{DuckDb, DuckDbBuilt, DuckDbSettings},
                Engine,
            },
        },
    };

    /// Loads and builds `case` on DuckDB and hands `check` the built state and
    /// the multiplicity of the case's all-constant atoms (DuckDB keeps
    /// duplicates, so each one multiplies).
    fn on_duckdb(
        case: &Case, settings: &DuckDbSettings, check: impl FnOnce(&DuckDbBuilt<'_>, usize),
    ) {
        let label = &case.label;
        let loaded = DuckDb::load(settings, &case.catalog, &case.prepared)
            .unwrap_or_else(|e| panic!("{label}: {e}"));
        let built = DuckDb::build(&loaded, &case.catalog, &case.prepared)
            .unwrap_or_else(|e| panic!("{label}: {e}"));
        let multiplicity =
            check_multiplicity(&case.catalog, &case.prepared, DuckDb::KEEPS_DUPLICATES);
        check(&built, multiplicity);
    }

    /// DuckDB's rows, each repeated once per unit of multiplicity (a check that
    /// holds nowhere leaves none) and sorted: the oracle's bag.
    fn rows_of(built: &DuckDbBuilt<'_>, multiplicity: usize, label: &str) -> Vec<Vec<usize>> {
        let mut rows = Vec::new();
        for row in DuckDb::rows(built).unwrap_or_else(|e| panic!("{label}: {e}")) {
            for _ in 0..multiplicity {
                rows.push(row.clone());
            }
        }
        rows.sort();
        rows
    }

    #[test]
    fn duckdb_returns_the_oracles_full_assignments() {
        for case in cases() {
            on_duckdb(&case, &DuckDbSettings::default(), |built, multiplicity| {
                assert_eq!(
                    rows_of(built, multiplicity, &case.label),
                    case.bag,
                    "{}",
                    case.label
                );
            });
        }
    }

    #[test]
    fn duckdb_streams_the_oracles_bag_count() {
        for case in cases() {
            on_duckdb(&case, &DuckDbSettings::default(), |built, multiplicity| {
                let streamed =
                    DuckDb::stream(built).unwrap_or_else(|e| panic!("{}: {e}", case.label));
                assert_eq!(
                    streamed * multiplicity as u64,
                    case.bag.len() as u64,
                    "{}",
                    case.label
                );
                let again = DuckDb::stream(built).unwrap_or_else(|e| panic!("{}: {e}", case.label));
                assert_eq!(
                    again, streamed,
                    "{}: the prepared statement changed its answer",
                    case.label
                );
            });
        }
    }

    #[test]
    fn duckdb_materializes_the_oracles_distinct_count() {
        for case in cases() {
            on_duckdb(&case, &DuckDbSettings::default(), |built, multiplicity| {
                // A check that holds nowhere means no rows, and the lifecycle
                // never asks the engine then.
                let materialized = if multiplicity == 0 {
                    0
                } else {
                    let (count, kept) = DuckDb::materialize(built)
                        .unwrap_or_else(|e| panic!("{}: {e}", case.label));
                    assert!(
                        kept.is_some(),
                        "{}: DuckDB returns its sorted batch",
                        case.label
                    );
                    count
                };
                assert_eq!(
                    materialized,
                    distinct_rows(&case.bag) as u64,
                    "{}",
                    case.label
                );
            });
        }
    }

    #[test]
    fn duckdb_agrees_with_the_oracle_on_two_threads() {
        let shapes = [
            "triangle",
            "chain",
            "selfjoin",
            "mixed-constants-and-repeats",
        ];
        for case in cases().iter().filter(|case| shapes.contains(&case.shape)) {
            on_duckdb(
                case,
                &DuckDbSettings {
                    threads: 2,
                },
                |built, multiplicity| {
                    assert_eq!(
                        rows_of(built, multiplicity, &case.label),
                        case.bag,
                        "{} on two threads",
                        case.label
                    );
                },
            );
        }
    }
}
