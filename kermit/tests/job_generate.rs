//! The JOB generator end to end on the committed IMDb-shaped fixture
//! (`kermit-generate/tests/fixtures/job/`): a raw cache built from it,
//! templates 1 and 2 generated through `kermit_generate::run`, the relation
//! files read back with `kermit_ds::read_parquet`, and every query joined on
//! every (structure, algorithm) cell, optimiser and column-order policy.
//!
//! Needs DuckDB, so each test skips with a stderr line where there is none
//! (CI installs no DuckDB; the LUBM tests skip without Java the same way)
//! and runs under `nix develop`. Raw caches and outputs live in temp dirs.
//!
//! The expected relations and counts were derived by hand from the fixture's
//! CSVs. `kermit-generate`'s `fixture_queries_count_as_the_facts_say` has
//! DuckDB recompute the four counts. Query `2b` selects no name, so its
//! relation `name_n_b` is empty and its count is 0.

mod common;

use {
    clap::ValueEnum,
    common::{
        job::{build_raw_cache, csv_dir, fixture_dir, locate_or_skip},
        utils::{join_under_planner, load_parquet_relations, JoinEntry},
    },
    kermit_algos::{
        ColumnOrderPolicy, HashTriejoin, JoinQuery, LeapfrogTriejoin, Optimiser, Planner,
    },
    kermit_bench::BenchmarkDefinition,
    kermit_ds::{
        BinarySeek, Cardinality, ColumnTrie, GallopingSeek, HashTrie, LazyExpansion, LinearSeek,
        NoPruning, Relation, SingletonPruning, TreeTrie,
    },
    kermit_generate::{
        job::{
            duckdb::Duckdb,
            raw::{RawCache, RawProvenance},
            JobExtras, JobGenerator,
        },
        run, GeneratedMeta,
    },
    kermit_iters::{FxHashStrategy, SipHashStrategy},
    kermit_rdf::generator::Target,
    std::{
        collections::{BTreeMap, BTreeSet},
        fs,
        path::{Path, PathBuf},
    },
};

type HashTrieSip = HashTrie<SipHashStrategy>;
type HashTrieFx = HashTrie<FxHashStrategy>;
type HashTrieSipPruned = HashTrie<SipHashStrategy, SingletonPruning>;
type HashTrieFxPruned = HashTrie<FxHashStrategy, SingletonPruning>;
type HashTrieSipLazy = HashTrie<SipHashStrategy, NoPruning, LazyExpansion>;
type HashTrieFxLazy = HashTrie<FxHashStrategy, NoPruning, LazyExpansion>;
type HashTrieSipPrunedLazy = HashTrie<SipHashStrategy, SingletonPruning, LazyExpansion>;
type HashTrieFxPrunedLazy = HashTrie<FxHashStrategy, SingletonPruning, LazyExpansion>;
type TreeTrieLinear = TreeTrie<LinearSeek>;
type TreeTrieBinary = TreeTrie<BinarySeek>;
type TreeTrieGalloping = TreeTrie<GallopingSeek>;
type ColumnTrieLinear = ColumnTrie<LinearSeek>;
type ColumnTrieBinary = ColumnTrie<BinarySeek>;
type ColumnTrieGalloping = ColumnTrie<GallopingSeek>;

/// The tag and spec hash every generation here passes in its `Target`.
const TAG: &str = "fixture";
const SPEC_HASH: &str = "job-generate-test-spec-hash";

/// The `COUNT(*)` of each fixture query, by hand.
const COUNTS: [(&str, u64); 4] = [("1a", 5), ("1b", 7), ("2a", 2), ("2b", 0)];

fn expected_count(query: &str) -> u64 {
    COUNTS
        .iter()
        .find(|(name, _)| *name == query)
        .unwrap_or_else(|| panic!("no hand count for query {query}"))
        .1
}

/// The rule of each fixture query, as the translator writes it:
/// `(template, query, Datalog)`.
const RULES: [(u8, &str, &str); 4] = [
    (
        1,
        "1a",
        "Q(IT1, IT2, T, K) :- info_type_it1_ab(IT1), info_type_it2_a(IT2), keyword_k_ab(K), \
         movie_info_mi1_a(IT1, T, _), movie_info(IT2, T, _), movie_keyword(T, K, _), title_t_a(T).",
    ),
    (
        1,
        "1b",
        "Q(IT1, T, K) :- info_type_it1_ab(IT1), keyword_k_ab(K), movie_info_mi1_b(IT1, T, _), \
         movie_keyword(T, K, _), title_t_b(T).",
    ),
    (
        2,
        "2a",
        "Q(T, N, KT) :- cast_info(T, N, _), kind_type_kt_a(KT), name_n_a(N), title_t_a(T, KT).",
    ),
    (
        2,
        "2b",
        "Q(T, N) :- cast_info(T, N, _), name_n_b(N), title_t_b(T).",
    ),
];

/// One relation file the fixture predicts: its name, its columns and its
/// rows in file order (`ORDER BY id`).
struct Expect {
    name: &'static str,
    columns: &'static [&'static str],
    rows: &'static [&'static [usize]],
}

/// Template 1 (queries `1a`, `1b`), in the order the planner first uses
/// each relation.
const TEMPLATE_1: &[Expect] = &[
    Expect {
        name: "info_type_it1_ab",
        columns: &["id"],
        rows: &[&[1]],
    },
    Expect {
        name: "info_type_it2_a",
        columns: &["id"],
        rows: &[&[2]],
    },
    Expect {
        name: "keyword_k_ab",
        columns: &["id"],
        rows: &[&[1], &[2]],
    },
    // `info IN ('Action', 'Drama')`; row 14 (`Action`, NULL type) drops out.
    Expect {
        name: "movie_info_mi1_a",
        columns: &["info_type_id", "movie_id", "id"],
        rows: &[&[1, 1, 1], &[1, 2, 3], &[1, 3, 8], &[1, 4, 9], &[1, 7, 11]],
    },
    // Unfiltered; only row 14 drops out, for its NULL `info_type_id`.
    Expect {
        name: "movie_info",
        columns: &["info_type_id", "movie_id", "id"],
        rows: &[
            &[1, 1, 1],
            &[2, 1, 2],
            &[1, 2, 3],
            &[2, 2, 4],
            &[1, 2, 5],
            &[1, 3, 6],
            &[2, 3, 7],
            &[1, 3, 8],
            &[1, 4, 9],
            &[2, 4, 10],
            &[1, 7, 11],
            &[2, 7, 12],
            &[1, 8, 13],
            &[2, 2, 15],
            &[3, 1, 16],
        ],
    },
    Expect {
        name: "movie_keyword",
        columns: &["movie_id", "keyword_id", "id"],
        rows: &[
            &[1, 1, 1],
            &[2, 1, 2],
            &[2, 2, 3],
            &[3, 4, 4],
            &[4, 3, 5],
            &[8, 1, 6],
            &[7, 4, 7],
            &[3, 2, 8],
            &[1, 2, 9],
            &[7, 3, 10],
        ],
    },
    // `production_year > 1990`: 1990 itself, 1985, 1950 and the NULL year drop out.
    Expect {
        name: "title_t_a",
        columns: &["id"],
        rows: &[&[2], &[3], &[4], &[8], &[9]],
    },
    // `info NOT LIKE 'Doc%'`: row 6 (`Documentary`) and row 14 drop out.
    Expect {
        name: "movie_info_mi1_b",
        columns: &["info_type_id", "movie_id", "id"],
        rows: &[
            &[1, 1, 1],
            &[2, 1, 2],
            &[1, 2, 3],
            &[2, 2, 4],
            &[1, 2, 5],
            &[2, 3, 7],
            &[1, 3, 8],
            &[1, 4, 9],
            &[2, 4, 10],
            &[1, 7, 11],
            &[2, 7, 12],
            &[1, 8, 13],
            &[2, 2, 15],
            &[3, 1, 16],
        ],
    },
    // `production_year BETWEEN 1980 AND 1995`: both bounds are kept (1995 is title 2).
    Expect {
        name: "title_t_b",
        columns: &["id"],
        rows: &[&[1], &[2], &[3], &[7]],
    },
];

/// Template 2 (queries `2a`, `2b`).
const TEMPLATE_2: &[Expect] = &[
    // Unfiltered; row 3 drops out for its NULL `person_id`.
    Expect {
        name: "cast_info",
        columns: &["movie_id", "person_id", "id"],
        rows: &[
            &[2, 2, 1],
            &[2, 4, 2],
            &[2, 1, 4],
            &[3, 2, 5],
            &[9, 4, 6],
            &[9, 2, 7],
            &[1, 4, 8],
            &[8, 3, 9],
        ],
    },
    Expect {
        name: "kind_type_kt_a",
        columns: &["id"],
        rows: &[&[1]],
    },
    Expect {
        name: "name_n_a",
        columns: &["id"],
        rows: &[&[2], &[4]],
    },
    // The two titles holding `"Hero"` (the CSV's backslash-escaped quotes), with their kinds.
    Expect {
        name: "title_t_a",
        columns: &["id", "kind_id"],
        rows: &[&[2, 1], &[9, 2]],
    },
    // `name LIKE '%Zed%'` matches no name: the file keeps its header and has no rows.
    Expect {
        name: "name_n_b",
        columns: &["id"],
        rows: &[],
    },
    Expect {
        name: "title_t_b",
        columns: &["id"],
        rows: &[&[2], &[3], &[4], &[8], &[9]],
    },
];

/// Provenance values no real cache has, so every extras assertion can tell a
/// field from its neighbour.
fn provenance() -> RawProvenance {
    RawProvenance {
        imdb_sha256: "a".repeat(64),
        query_commit: "fixture-commit".to_owned(),
        query_sha256: [
            ("schema.sql", "b"),
            ("1a.sql", "c"),
            ("1b.sql", "d"),
            ("2a.sql", "e"),
            ("2b.sql", "f"),
        ]
        .into_iter()
        .map(|(file, digit)| (file.to_owned(), digit.repeat(64)))
        .collect(),
        duckdb_version: "built-with-another-duckdb".to_owned(),
    }
}

/// A raw cache built from the fixture, and the DuckDB that built it.
struct Fixture {
    /// Owns the raw cache's directory.
    _scratch: tempfile::TempDir,
    duckdb: Duckdb,
    raw: RawCache,
}

impl Fixture {
    /// The fixture with its CSVs loaded as written, or (when `reversed`) in
    /// reverse order; `None`, after a stderr line, without DuckDB.
    fn build(test: &str, reversed: bool) -> Option<Fixture> {
        Fixture::build_with(test, reversed, &fixture_dir().join("queries"))
    }

    /// As [`Fixture::build`], with the query files taken from `queries_dir`.
    fn build_with(test: &str, reversed: bool, queries_dir: &Path) -> Option<Fixture> {
        Fixture::build_recording(test, reversed, queries_dir, provenance())
    }

    /// As [`Fixture::build_with`], with `recorded` written to the cache's
    /// `provenance.json` in place of `provenance()`.
    fn build_recording(
        test: &str, reversed: bool, queries_dir: &Path, recorded: RawProvenance,
    ) -> Option<Fixture> {
        let duckdb = locate_or_skip(test)?;
        let scratch = tempfile::tempdir().unwrap();
        let csv = csv_dir(scratch.path(), reversed);
        let raw = build_raw_cache(
            &duckdb,
            &scratch.path().join("imdb"),
            &csv,
            queries_dir,
            recorded,
        );
        Some(Fixture {
            _scratch: scratch,
            duckdb,
            raw,
        })
    }

    /// Generates `template` into `parent/job-<template>` (`job_dir`) through
    /// `kermit_generate::run`, which requires the output directory to be named
    /// after the benchmark.
    fn try_generate(
        &self, template: u8, parent: &Path,
    ) -> anyhow::Result<GeneratedMeta<JobExtras>> {
        let bench_name = format!("job-{template}");
        let out = job_dir(parent, template);
        let generator = JobGenerator {
            template,
            raw: &self.raw,
            duckdb: &self.duckdb,
            target: Target {
                out_dir: &out,
                bench_name: &bench_name,
                tag: TAG,
                spec_hash: Some(SPEC_HASH),
            },
        };
        run(&generator)
    }

    /// As [`Fixture::try_generate`], for a generation that must succeed.
    fn generate(&self, template: u8, parent: &Path) -> GeneratedMeta<JobExtras> {
        self.try_generate(template, parent)
            .unwrap_or_else(|error| panic!("generating template {template}: {error:#}"))
    }
}

/// The output directory of `template`'s generation under `parent`: named
/// after the benchmark, as `kermit_generate::run` requires.
fn job_dir(parent: &Path, template: u8) -> PathBuf { parent.join(format!("job-{template}")) }

/// Every `.parquet` file in `dir`, by stem, with its bytes.
fn parquet_files(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    fs::read_dir(dir)
        .unwrap()
        .filter_map(|entry| {
            let path = entry.unwrap().path();
            (path.extension()? == "parquet").then(|| {
                let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
                (stem, fs::read(&path).unwrap())
            })
        })
        .collect()
}

/// The stems of the `.parquet` files in `dir`.
fn parquet_stems(dir: &Path) -> BTreeSet<String> { parquet_files(dir).into_keys().collect() }

/// The names of everything in `dir`, sorted.
fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// The `benchmark.yml` a generation wrote into `dir`, validated.
fn emitted_benchmark(dir: &Path) -> BenchmarkDefinition {
    let yaml = fs::read_to_string(dir.join("benchmark.yml")).expect("benchmark.yml is missing");
    let bench: BenchmarkDefinition =
        serde_yaml::from_str(&yaml).expect("benchmark.yml is malformed");
    bench.validate().expect("benchmark.yml must validate");
    bench
}

#[test]
fn relations_hold_filtered_rows_in_id_order() {
    // Once with the CSVs as written and once loaded in reverse id order: DuckDB
    // returns rows in load order unless the query sorts, so only the second
    // pass notices an `ORDER BY "id"` going missing.
    for reversed in [false, true] {
        let Some(fixture) = Fixture::build("relations_hold_filtered_rows_in_id_order", reversed)
        else {
            return;
        };
        for (template, expected) in [(1u8, TEMPLATE_1), (2, TEMPLATE_2)] {
            let context = format!("template {template}, reversed load = {reversed}");
            let parent = tempfile::tempdir().unwrap();
            let meta = fixture.generate(template, parent.path());
            let out = job_dir(parent.path(), template);

            // The files are exactly the planned relations, recorded in plan
            // order.
            let planned: BTreeSet<String> = expected.iter().map(|e| e.name.to_owned()).collect();
            assert_eq!(parquet_stems(&out), planned, "{context}");
            let recorded: Vec<(&str, u64)> = meta
                .relations
                .iter()
                .map(|record| (record.name.as_str(), record.rows))
                .collect();
            let want: Vec<(&str, u64)> = expected
                .iter()
                .map(|e| (e.name, e.rows.len() as u64))
                .collect();
            assert_eq!(recorded, want, "{context}");

            for e in expected {
                let path = out.join(format!("{}.parquet", e.name));
                let (header, tuples) = kermit_ds::read_parquet(&path)
                    .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
                assert_eq!(header.name(), e.name, "{context}");
                assert_eq!(header.attrs(), e.columns, "{} ({context})", e.name);
                let rows: Vec<Vec<usize>> = e.rows.iter().map(|row| row.to_vec()).collect();
                assert_eq!(tuples.to_vecs(), rows, "{} ({context})", e.name);
            }
        }
    }
}

/// kermit's count of `bench`'s queries over the relations in `dir`, joined as
/// `R` by `JA` under every optimiser and column-order policy, against each
/// query's `expected` (DuckDB's `COUNT(*)`). One line per disagreement.
fn mismatches<R: Relation + Cardinality, JA: JoinEntry<R>>(
    bench: &BenchmarkDefinition, dir: &Path,
) -> Vec<String> {
    let mut found = Vec::new();
    for &policy in ColumnOrderPolicy::value_variants() {
        for &optimiser in Optimiser::value_variants() {
            let planner = Planner::new(optimiser.instantiate(), policy);
            let label = format!(
                "{} / {} / {}",
                std::any::type_name::<R>(),
                optimiser.axis_value(),
                policy.axis_value()
            );
            let (relations, inputs) = load_parquet_relations::<R>(
                dir,
                bench
                    .relations
                    .iter()
                    .map(|relation| relation.name.as_str()),
            );
            let mut database = JA::database(relations, planner.required_statistics());
            for query in &bench.queries {
                let parsed: JoinQuery = query.query.parse().expect("generated Datalog parses");
                let want = query.expected.expect("every JOB query carries its count");
                match join_under_planner::<R, JA>(&mut database, &inputs, parsed, &planner) {
                    | Ok((rows, _)) if rows.len() as u64 == want => {},
                    | Ok((rows, _)) => found.push(format!(
                        "[{label}] {}: got {}, expected {want}",
                        query.name,
                        rows.len()
                    )),
                    | Err(error) => found.push(format!("[{label}] {}: {error}", query.name)),
                }
            }
        }
    }
    found
}

/// One sweep per (structure, algorithm) cell: the six sorted tries under
/// LFTJ and the eight HashTrie Layouts under Hash Triejoin.
fn cells() -> [fn(&BenchmarkDefinition, &Path) -> Vec<String>; 14] {
    [
        mismatches::<TreeTrieLinear, LeapfrogTriejoin>,
        mismatches::<TreeTrieBinary, LeapfrogTriejoin>,
        mismatches::<TreeTrieGalloping, LeapfrogTriejoin>,
        mismatches::<ColumnTrieLinear, LeapfrogTriejoin>,
        mismatches::<ColumnTrieBinary, LeapfrogTriejoin>,
        mismatches::<ColumnTrieGalloping, LeapfrogTriejoin>,
        mismatches::<HashTrieSip, HashTriejoin>,
        mismatches::<HashTrieFx, HashTriejoin>,
        mismatches::<HashTrieSipPruned, HashTriejoin>,
        mismatches::<HashTrieFxPruned, HashTriejoin>,
        mismatches::<HashTrieSipLazy, HashTriejoin>,
        mismatches::<HashTrieFxLazy, HashTriejoin>,
        mismatches::<HashTrieSipPrunedLazy, HashTriejoin>,
        mismatches::<HashTrieFxPrunedLazy, HashTriejoin>,
    ]
}

#[test]
fn counts_match_duckdb_on_every_cell() {
    let Some(fixture) = Fixture::build("counts_match_duckdb_on_every_cell", false) else {
        return;
    };
    let mut found = Vec::new();
    for template in [1u8, 2] {
        let parent = tempfile::tempdir().unwrap();
        fixture.generate(template, parent.path());
        let out = job_dir(parent.path(), template);
        let bench = emitted_benchmark(&out);
        // DuckDB's count is the oracle; it must itself equal the hand count.
        for query in &bench.queries {
            assert_eq!(
                query.expected,
                Some(expected_count(&query.name)),
                "query {}: DuckDB's COUNT(*) disagrees with the hand count",
                query.name
            );
        }
        for sweep in cells() {
            found.extend(sweep(&bench, &out));
        }
    }
    assert!(
        found.is_empty(),
        "{} count mismatches across 14 cells x {} optimisers x {} column-order policies x 2 \
         templates:\n{}",
        found.len(),
        Optimiser::value_variants().len(),
        ColumnOrderPolicy::value_variants().len(),
        found.join("\n")
    );
}

#[test]
fn regeneration_is_byte_identical() {
    let Some(fixture) = Fixture::build("regeneration_is_byte_identical", false) else {
        return;
    };
    // The same data loaded in the opposite order: the files must not depend on
    // the order DuckDB happens to hold the rows in.
    let Some(reversed) = Fixture::build("regeneration_is_byte_identical", true) else {
        return;
    };
    for template in [1u8, 2] {
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        let third = tempfile::tempdir().unwrap();
        let a = fixture.generate(template, first.path());
        let b = fixture.generate(template, second.path());
        let c = reversed.generate(template, third.path());
        let (first, second, third) = (
            job_dir(first.path(), template),
            job_dir(second.path(), template),
            job_dir(third.path(), template),
        );

        let files = parquet_files(&first);
        assert!(!files.is_empty(), "template {template} wrote no relation");
        for (label, dir) in [("second run", &second), ("reversed load", &third)] {
            let other = parquet_files(dir);
            assert_eq!(
                files.keys().collect::<Vec<_>>(),
                other.keys().collect::<Vec<_>>(),
                "template {template}, {label}: different files"
            );
            for (name, bytes) in &files {
                assert!(
                    *bytes == other[name],
                    "template {template}, {label}: {name}.parquet differs"
                );
            }
        }
        // The hashes the layer records agree with the bytes on disk, in all
        // three runs...
        for (meta, dir) in [(&a, &first), (&b, &second), (&c, &third)] {
            for record in &meta.relations {
                let path = dir.join(format!("{}.parquet", record.name));
                let on_disk = kermit_bench::cache::sha256_hex(&path)
                    .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
                assert_eq!(
                    record.sha256, on_disk,
                    "template {template}: the recorded hash of {} is not its file's",
                    record.name
                );
            }
        }
        // ...and so they agree between the runs.
        let hashes = |meta: &GeneratedMeta<JobExtras>| -> Vec<(String, String)> {
            meta.relations
                .iter()
                .map(|record| (record.name.clone(), record.sha256.clone()))
                .collect()
        };
        assert_eq!(hashes(&a), hashes(&b), "template {template}");
        assert_eq!(hashes(&a), hashes(&c), "template {template}");
    }
}

/// Two queries filtering on a column the tables lack: `1a`'s first relation
/// (`cast_info_ci_a`) and `2a`'s last (`title_t_a`) fail in DuckDB.
fn write_broken_queries(dir: &Path) {
    fs::write(
        dir.join("1a.sql"),
        "SELECT MIN(t.title) AS movie_title\nFROM cast_info AS ci,\n     title AS t\nWHERE \
         ci.no_such_column = 1\n  AND ci.movie_id = t.id;\n",
    )
    .unwrap();
    fs::write(
        dir.join("2a.sql"),
        "SELECT MIN(t.title) AS movie_title\nFROM cast_info AS ci,\n     title AS t\nWHERE \
         t.no_such_column = 1\n  AND ci.movie_id = t.id;\n",
    )
    .unwrap();
}

#[test]
fn failed_filter_leaves_no_relation_and_no_meta() {
    let queries = tempfile::tempdir().unwrap();
    write_broken_queries(queries.path());
    let Some(fixture) = Fixture::build_with(
        "failed_filter_leaves_no_relation_and_no_meta",
        false,
        queries.path(),
    ) else {
        return;
    };

    // Template 1: the first relation fails, so nothing at all is written. A
    // stale `meta.json` planted first must be gone: `run` removes the old
    // commit marker before it generates.
    let parent = tempfile::tempdir().unwrap();
    let out = job_dir(parent.path(), 1);
    fs::create_dir(&out).unwrap();
    fs::write(out.join("meta.json"), "{}").unwrap();
    let message = format!("{:#}", fixture.try_generate(1, parent.path()).unwrap_err());
    assert!(message.contains("cast_info_ci_a"), "{message}");
    assert!(message.contains("no_such_column"), "{message}");
    assert_eq!(
        names(&out),
        Vec::<String>::new(),
        "no relation file, finished or partial, and no stale meta.json"
    );

    // Template 2: the last relation fails. Relations finished before the
    // failure may stay (`meta.json`'s absence is what marks the output
    // incomplete), but the failing one leaves neither a file nor a `.part`,
    // and nothing commits.
    let parent = tempfile::tempdir().unwrap();
    let out = job_dir(parent.path(), 2);
    fs::create_dir(&out).unwrap();
    fs::write(out.join("meta.json"), "{}").unwrap();
    let message = format!("{:#}", fixture.try_generate(2, parent.path()).unwrap_err());
    assert!(message.contains("title_t_a"), "{message}");
    assert!(message.contains("no_such_column"), "{message}");
    let left = names(&out);
    assert!(
        left.iter().all(|name| name.ends_with(".parquet")),
        "only finished relations may remain, not meta.json, benchmark.yml or a .part: {left:?}"
    );
    assert!(!left.contains(&"title_t_a.parquet".to_owned()), "{left:?}");
}

#[test]
fn unknown_templates_are_errors() {
    let Some(fixture) = Fixture::build("unknown_templates_are_errors", false) else {
        return;
    };
    // Outside 1..=33 is refused before any query is read.
    for template in [0u8, 34] {
        let parent = tempfile::tempdir().unwrap();
        let message = format!(
            "{:#}",
            fixture.try_generate(template, parent.path()).unwrap_err()
        );
        assert!(message.contains("1..=33"), "template {template}: {message}");
        assert_eq!(
            names(&job_dir(parent.path(), template)),
            Vec::<String>::new()
        );
    }
    // Inside the range but absent from the cache (the fixture holds 1 and 2).
    let parent = tempfile::tempdir().unwrap();
    let message = format!("{:#}", fixture.try_generate(3, parent.path()).unwrap_err());
    assert!(message.contains("no query files"), "{message}");
    assert!(message.contains("template 3"), "{message}");
    assert_eq!(names(&job_dir(parent.path(), 3)), Vec::<String>::new());
}

#[test]
fn extras_record_sources_and_versions() {
    let Some(fixture) = Fixture::build("extras_record_sources_and_versions", false) else {
        return;
    };
    let parent = tempfile::tempdir().unwrap();
    let meta = fixture.generate(1, parent.path());
    let out = job_dir(parent.path(), 1);

    assert_eq!(meta.kind, "job");
    assert_eq!(meta.tag, TAG);
    assert_eq!(meta.spec_hash.as_deref(), Some(SPEC_HASH));
    assert_eq!((meta.relation_count, meta.query_count), (9, 2));

    let extras = &meta.extras;
    assert_eq!(extras.template, 1);
    assert_eq!(extras.imdb_sha256, "a".repeat(64));
    assert_eq!(extras.query_commit, "fixture-commit");
    // The generation's DuckDB, not the one the cache was built with.
    assert_eq!(extras.duckdb_version, fixture.duckdb.version());
    assert_ne!(extras.duckdb_version, provenance().duckdb_version);
    // `schema.sql` and this template's queries, not template 2's.
    let files: Vec<&str> = extras.query_sha256.keys().map(String::as_str).collect();
    assert_eq!(files, ["1a.sql", "1b.sql", "schema.sql"]);
    assert_eq!(extras.query_sha256["schema.sql"], "b".repeat(64));
    assert_eq!(extras.query_sha256["1a.sql"], "c".repeat(64));
    assert_eq!(extras.query_sha256["1b.sql"], "d".repeat(64));

    let sources: Vec<(&str, &str, &str)> = extras
        .sources
        .iter()
        .map(|s| (s.relation.as_str(), s.table.as_str(), s.filter.as_str()))
        .collect();
    assert_eq!(sources, [
        ("info_type_it1_ab", "info_type", "info = 'genres'"),
        ("info_type_it2_a", "info_type", "info = 'rating'"),
        ("keyword_k_ab", "keyword", "keyword LIKE '%hero%'"),
        (
            "movie_info_mi1_a",
            "movie_info",
            "info IN ('Action', 'Drama')"
        ),
        ("movie_info", "movie_info", ""),
        ("movie_keyword", "movie_keyword", ""),
        ("title_t_a", "title", "production_year > 1990"),
        ("movie_info_mi1_b", "movie_info", "info NOT LIKE 'Doc%'"),
        (
            "title_t_b",
            "title",
            "production_year BETWEEN 1980 AND 1995"
        ),
    ]);
    // `sources` joins the common `relations` list by name, in the same order.
    let recorded: Vec<&str> = meta.relations.iter().map(|r| r.name.as_str()).collect();
    let named: Vec<&str> = extras.sources.iter().map(|s| s.relation.as_str()).collect();
    assert_eq!(recorded, named);

    // meta.json holds the same, with the extras flattened beside the common
    // fields.
    let text = fs::read_to_string(out.join("meta.json")).unwrap();
    let on_disk: GeneratedMeta<JobExtras> = serde_json::from_str(&text).unwrap();
    assert_eq!(on_disk, meta);
    let value: serde_json::Value = serde_json::from_str(&text).unwrap();
    for key in [
        "template",
        "imdb_sha256",
        "query_commit",
        "query_sha256",
        "duckdb_version",
        "sources",
        "relations",
    ] {
        assert!(value.get(key).is_some(), "meta.json has no `{key}`: {text}");
    }
}

/// The provenance must hold a digest for `schema.sql` and for every query file
/// of the template: a file without one is an error naming it, raised before the
/// filter stage, so no relation is written and nothing commits.
#[test]
fn a_provenance_without_a_query_digest_fails_before_anything_is_written() {
    let mut recorded = provenance();
    recorded.query_sha256.remove("1b.sql");
    let Some(fixture) = Fixture::build_recording(
        "a_provenance_without_a_query_digest_fails_before_anything_is_written",
        false,
        &fixture_dir().join("queries"),
        recorded,
    ) else {
        return;
    };

    // Template 1 reads 1b.sql. A stale meta.json planted first is gone (`run`
    // removes the old commit marker), and no relation file was written.
    let parent = tempfile::tempdir().unwrap();
    let out = job_dir(parent.path(), 1);
    fs::create_dir(&out).unwrap();
    fs::write(out.join("meta.json"), "{}").unwrap();
    let message = format!("{:#}", fixture.try_generate(1, parent.path()).unwrap_err());
    assert!(
        message.contains("records no SHA-256 for 1b.sql"),
        "{message}"
    );
    assert_eq!(
        names(&out),
        Vec::<String>::new(),
        "no relation file, no meta.json and no benchmark.yml"
    );

    // Template 2 reads 2a.sql and 2b.sql, whose digests are present: the gap
    // does not touch it.
    let parent = tempfile::tempdir().unwrap();
    fixture.generate(2, parent.path());
}

#[test]
fn benchmark_yml_carries_the_rules_and_counts() {
    let Some(fixture) = Fixture::build("benchmark_yml_carries_the_rules_and_counts", false) else {
        return;
    };
    for (template, expected) in [(1u8, TEMPLATE_1), (2, TEMPLATE_2)] {
        let parent = tempfile::tempdir().unwrap();
        let meta = fixture.generate(template, parent.path());
        let bench = emitted_benchmark(&job_dir(parent.path(), template));

        assert_eq!(bench.name, format!("job-{template}"));
        assert_eq!(
            bench.description,
            format!(
                "Join Order Benchmark template {template} (Leis et al., VLDB 2015) over IMDb, \
                 filters applied at generation"
            )
        );
        assert!(
            bench.generator.is_none(),
            "the cache-side benchmark.yml carries no generator block"
        );

        let named: Vec<&str> = bench.relations.iter().map(|r| r.name.as_str()).collect();
        let planned: Vec<&str> = expected.iter().map(|e| e.name).collect();
        assert_eq!(named, planned);
        for (source, record) in bench.relations.iter().zip(&meta.relations) {
            assert_eq!(
                source.sha256.as_deref(),
                Some(record.sha256.as_str()),
                "{}",
                source.name
            );
            let url = source.url.as_deref().expect("a relation URL");
            assert!(
                url.starts_with("file://") && url.ends_with(&format!("/{}.parquet", source.name)),
                "{url}"
            );
        }

        let rules: Vec<(&str, &str)> = RULES
            .iter()
            .filter(|(owner, ..)| *owner == template)
            .map(|(_, name, datalog)| (*name, *datalog))
            .collect();
        let queries: Vec<(&str, &str)> = bench
            .queries
            .iter()
            .map(|q| (q.name.as_str(), q.query.as_str()))
            .collect();
        assert_eq!(queries, rules);
        for query in &bench.queries {
            assert_eq!(
                query.description,
                format!("JOB {}, filters applied at generation", query.name)
            );
            assert_eq!(query.expected, Some(expected_count(&query.name)));
        }
    }
}
