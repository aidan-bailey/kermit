//! The engines against the nested-loop oracle: the shared shapes over seeded
//! random relations, compared on contents as well as counts. Built only with an
//! engine feature.
#![cfg(any(feature = "engine-duckdb", feature = "engine-nemo"))]

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
#[cfg_attr(not(feature = "engine-duckdb"), allow(dead_code))]
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

#[cfg(feature = "engine-duckdb")]
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
                    // The batch is the engine's rows sorted, before
                    // `check_multiplicity` repeats them: the oracle's bag with
                    // each run of equal rows `multiplicity` times shorter.
                    let expected: Vec<Vec<usize>> =
                        case.bag.iter().step_by(multiplicity).cloned().collect();
                    assert_eq!(
                        kept.unwrap_or_else(|| panic!(
                            "{}: DuckDB returns its sorted batch",
                            case.label
                        ))
                        .to_vecs(),
                        expected,
                        "{}",
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

    /// On these 8-12-row fixtures DuckDB never runs a parallel pipeline, so
    /// this is a smoke test of the two-thread setting, not a test of
    /// parallelism.
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

/// Nemo against the nested-loop oracle (`engine-nemo`).
///
/// Nemo answers the full conjunctive query as a set, so its rows and its
/// `materialize` count are compared with the oracle's *distinct* full
/// assignments: the same set under set and bag semantics, while the fixtures
/// hold duplicate rows. The shared cases run the planners `bench run` has; the
/// every-order test forces each variable order, so `rows`' permutation of
/// the result's plan-ordered columns into canonical order is checked against
/// every plan, and a mis-ordered join would show in the contents, which a count
/// alone could miss on these small fixtures. Canonical order is
/// `canonical_names(prepared.query())` of the `TrieLevels::All` query Nemo ran,
/// so the oracle's query is `full_query(query, &prepared)` for that same
/// `prepared`: the order can change with the plan, and so the oracle runs once
/// per plan.
#[cfg(feature = "engine-nemo")]
mod nemo_cells {
    use {
        super::{cases, distinct_rows, Case, HashTrieSip},
        crate::common::{
            oracle::{oracle, Lcg, Relations},
            orders::{every_order, FixedOrder},
            shapes::{catalog_for, full_query, random_data, shapes, SEEDS},
        },
        kermit::{
            db::{check_multiplicity, prepare, Catalog},
            engine::{
                nemo::{Nemo, NemoBuilt},
                Engine,
            },
        },
        kermit_algos::{canonical_names, rewrite_placeholders, JoinQuery, Planner},
        kermit_ds::TrieLevels,
    };

    /// Loads and builds `case` on Nemo and hands `check` the built state and
    /// the multiplicity of the case's all-constant atoms (Nemo has set
    /// semantics, so each one clamps to one).
    fn on_nemo(case: &Case, check: impl FnOnce(&NemoBuilt, usize)) {
        let label = &case.label;
        Nemo::supports(&case.prepared).unwrap_or_else(|e| panic!("{label}: {e}"));
        let loaded = Nemo::load(&(), &case.catalog, &case.prepared)
            .unwrap_or_else(|e| panic!("{label}: {e}"));
        let built = Nemo::build(&loaded, &case.catalog, &case.prepared)
            .unwrap_or_else(|e| panic!("{label}: {e}"));
        let multiplicity =
            check_multiplicity(&case.catalog, &case.prepared, Nemo::KEEPS_DUPLICATES);
        check(&built, multiplicity);
    }

    #[test]
    fn nemo_returns_the_oracles_distinct_full_assignments() {
        for case in cases() {
            on_nemo(&case, |built, multiplicity| {
                // A check that holds nowhere means no rows, and the lifecycle
                // never asks the engine then. Otherwise Nemo's set is the
                // answer, whatever the check's count. `case.bag` is the
                // oracle's for `full_query(query, &case.prepared)`, so its rows
                // are in the same canonical order as `rows`.
                let mut rows = if multiplicity == 0 {
                    Vec::new()
                } else {
                    Nemo::rows(built).unwrap_or_else(|e| panic!("{}: {e}", case.label))
                };
                rows.sort();
                // The oracle's bag is sorted, so its distinct rows are its
                // deduplication.
                let mut want = case.bag.clone();
                want.dedup();
                assert_eq!(rows, want, "{}", case.label);
            });
        }
    }

    #[test]
    fn nemo_materializes_the_oracles_distinct_count() {
        for case in cases() {
            on_nemo(&case, |built, multiplicity| {
                let materialized = if multiplicity == 0 {
                    0
                } else {
                    let (count, kept) =
                        Nemo::materialize(built).unwrap_or_else(|e| panic!("{}: {e}", case.label));
                    assert!(kept.is_none(), "{}: Nemo keeps no batch", case.label);
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

    /// Nemo's `materialize` count and sorted `rows` for `query` over `catalog`,
    /// with the plan binding `order` first, and the oracle's distinct rows for
    /// `full_query(query, &prepared)` over `relations`, where `prepared` is the
    /// `TrieLevels::All` query Nemo ran. Asserts the plan binds `order` first,
    /// so a planner that stopped honouring `order` fails here instead
    /// of running one order many times. The all-constant atoms are applied as
    /// the runner applies them, through `check_multiplicity`.
    fn nemo_in_order(
        relations: &Relations, catalog: &Catalog, query: &JoinQuery, order: &[String], label: &str,
    ) -> (u64, Vec<Vec<usize>>, Vec<Vec<usize>>) {
        let planner = Planner::new(Box::new(FixedOrder(order.to_vec())));
        let prepared = prepare(catalog, query, &planner, TrieLevels::All)
            .unwrap_or_else(|e| panic!("{label} in order {order:?}: {e}"));
        let names = canonical_names(prepared.query());
        let bound: Vec<String> = prepared
            .plan()
            .variable_ordering
            .iter()
            .map(|&variable| names[variable].clone())
            .collect();
        assert_eq!(
            &bound[..order.len()],
            order,
            "{label}: the plan does not bind the forced variables first"
        );
        assert!(
            !prepared.indexes().is_empty(),
            "{label}: every shape reads at least one index"
        );
        let mut want = oracle::<HashTrieSip>(relations, &full_query(query, &prepared));
        want.dedup();
        Nemo::supports(&prepared).unwrap_or_else(|e| panic!("{label} in order {order:?}: {e}"));
        if check_multiplicity(catalog, &prepared, Nemo::KEEPS_DUPLICATES) == 0 {
            return (0, Vec::new(), want);
        }
        let loaded = Nemo::load(&(), catalog, &prepared)
            .unwrap_or_else(|e| panic!("{label} in order {order:?}: {e}"));
        let built = Nemo::build(&loaded, catalog, &prepared)
            .unwrap_or_else(|e| panic!("{label} in order {order:?}: {e}"));
        let (count, kept) =
            Nemo::materialize(&built).unwrap_or_else(|e| panic!("{label} in order {order:?}: {e}"));
        assert!(
            kept.is_none(),
            "{label} in order {order:?}: Nemo keeps no batch"
        );
        let mut rows =
            Nemo::rows(&built).unwrap_or_else(|e| panic!("{label} in order {order:?}: {e}"));
        rows.sort();
        (count, rows, want)
    }

    /// Every shape over `SEEDS` random relations, under every order of its
    /// variables. The final bound catches a generator that stops producing
    /// queries with answers, which would turn every comparison into `[] == []`.
    #[test]
    fn nemo_returns_the_distinct_full_assignments_in_every_order() {
        let (mut with_rows, mut total) = (0, 0);
        for (index, shape) in shapes().iter().enumerate() {
            for seed in SEEDS {
                let mut rng = Lcg(1000 * index as u64 + seed);
                let relations = random_data(&mut rng, shape.arities);
                let label = format!("{} seed {seed}", shape.name);
                let query: JoinQuery = shape
                    .query
                    .parse()
                    .unwrap_or_else(|e| panic!("{label}: unparsable: {e}"));
                let catalog = catalog_for(&relations);
                let variables = canonical_names(&rewrite_placeholders(query.clone()));
                let mut answered = false;
                for order in every_order(&variables) {
                    let (count, rows, want) =
                        nemo_in_order(&relations, &catalog, &query, &order, &label);
                    assert_eq!(rows, want, "{label} in order {order:?}: rows");
                    assert_eq!(
                        count,
                        want.len() as u64,
                        "{label} in order {order:?}: materialize"
                    );
                    answered |= !want.is_empty();
                }
                total += 1;
                with_rows += usize::from(answered);
            }
        }
        assert!(
            with_rows * 4 >= total,
            "only {with_rows} of {total} queries had an answer"
        );
    }
}
