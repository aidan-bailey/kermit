//! End-to-end SNAP check (#96): the committed miniature graph goes through
//! `kermit_generate::run`, then every query runs on the three
//! (structure, algorithm) cells under every optimiser through the real
//! `kermit::db` entry points, and must return the count the generator
//! recorded as `expected`.
//!
//! It lives in the `kermit` crate for the reason `lubm_mini_oracle.rs` does:
//! only this crate has both the generator and the join engine, and it needs
//! nothing of the binary. The cells are `TreeTrie` and `ColumnTrie` under
//! `LeapfrogTriejoin`, and `HashTrie` under `HashTriejoin`; the shared
//! `JoinEntry` driver in `common/utils.rs` hosts both families.
//!
//! # The fixture
//!
//! `tests/fixtures/snap/mini.ungraph.txt` holds 29 data lines: a 4-clique on
//! 1..4, a 4-cycle on 5..8, a hub (10) with seven neighbours, a second
//! triangle (20, 21, 22) with a tail, bridges 4-5, 8-10 and 16-20, a pendant
//! 0-1, one self-loop (`3 3`) and one reversed duplicate (`2 1`). That leaves
//! 27 edges on 22 vertices.
//!
//! The generator's counts are pinned in [`MINI_COUNTS`] against a
//! derivation that shares no code with it: the bindings of each rule
//! enumerated directly over the cleaned edges, and the same numbers from
//! tr(A³) (6 × 6 triangles), 1ᵀA³1 and tr(A⁴). The joins and the oracle both
//! read the parser's output, so only a pin like this fails when the parser
//! keeps the self-loop or the duplicate.

mod common;

use {
    clap::ValueEnum,
    common::{
        snap::{fixture_text, gzip, MINI_COUNTS},
        utils::{catalog_of, join_under_planner, load_parquet_inputs, JoinEntry},
    },
    kermit_algos::{HashTriejoin, JoinQuery, LeapfrogTriejoin, Optimiser, Planner},
    kermit_bench::{
        cache::sha256_hex, discovery::load_benchmark, BenchmarkDefinition, GeneratorSpec,
        SnapGraph, SnapQuery,
    },
    kermit_ds::{
        read_parquet, Cardinality, ColumnTrie, GallopingSeek, HashTrie, Relation, TreeTrie,
    },
    kermit_generate::{
        snap::{SnapExtras, SnapGenerator, SnapSource},
        GeneratedMeta,
    },
    kermit_iters::SipHashStrategy,
    kermit_rdf::generator::Target,
    std::{
        collections::HashSet,
        fs,
        path::{Path, PathBuf},
    },
};

/// The benchmark name every generation here uses. `kermit_generate::run`
/// requires the output directory's last component to equal it.
const BENCH_NAME: &str = "snap-mini";

// The three cells' relation types. `TreeTrie` and `ColumnTrie` default to
// `GallopingSeek`; the alias names it so no inference fallback is needed.
type Tree = TreeTrie<GallopingSeek>;
type Column = ColumnTrie<GallopingSeek>;
type Hash = HashTrie<SipHashStrategy>;

/// The data lines of the fixture: everything that is not a `#` comment or
/// blank.
fn data_lines(text: &str) -> Vec<&str> {
    text.lines()
        .filter(|line| !line.starts_with('#') && !line.trim().is_empty())
        .collect()
}

/// A generation of the fixture into `<root>/snap-mini`.
struct Generation {
    /// Keeps the temp dir alive.
    _root: tempfile::TempDir,
    /// The output directory (a cache subdirectory in all but location).
    dir: PathBuf,
    /// The `.gz` the generator read.
    gz: PathBuf,
    /// What `run` returned.
    meta: GeneratedMeta<SnapExtras>,
}

/// Gzips the fixture into a fresh temp dir and runs the generator over it
/// with a `Local` source.
fn generate_mini(queries: Option<&[SnapQuery]>) -> Generation {
    let root = tempfile::tempdir().expect("create temp dir");
    let gz = root.path().join("mini.ungraph.txt.gz");
    fs::write(&gz, gzip(&fixture_text())).expect("write the gzipped fixture");
    let dir = root.path().join(BENCH_NAME);
    let meta = {
        let generator = SnapGenerator::new(
            Target {
                out_dir: &dir,
                bench_name: BENCH_NAME,
                tag: "mini",
                spec_hash: None,
            },
            None,
            SnapSource::Local(gz.clone()),
            queries,
        );
        kermit_generate::run(&generator).expect("the mini SNAP graph must generate")
    };
    Generation {
        _root: root,
        dir,
        gz,
        meta,
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

/// Loads the generated `edge` relation flat into a catalog gathered at what
/// `planner` reads, joins every query as `R` through `JA` (each builds the
/// indexes its plan reads), and returns one line per query whose result
/// count differs from its `expected`. A join that errors panics, naming the
/// cell.
fn cell_mismatches<R, JA>(
    bench: &BenchmarkDefinition, dir: &Path, planner: &Planner, label: &str,
) -> Vec<String>
where
    R: Relation + Cardinality,
    JA: JoinEntry<R>,
{
    let inputs = load_parquet_inputs(dir, bench.relations.iter().map(|r| r.name.as_str()));
    let catalog = catalog_of(&inputs, planner.required_statistics());

    let mut mismatches = Vec::new();
    for q in &bench.queries {
        let want = q.expected.expect("every SNAP query carries expected");
        let parsed: JoinQuery = q.query.parse().expect("datalog parse failure");
        let (rows, _) =
            join_under_planner::<R, JA>(&catalog, parsed, planner).unwrap_or_else(|e| {
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

/// The committed YAMLs declare what their names say, and validate.
#[test]
fn committed_yamls_declare_their_named_graph() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("kermit is inside the workspace");
    for (name, graph) in [
        ("snap-amazon", SnapGraph::ComAmazon),
        ("snap-dblp", SnapGraph::ComDblp),
    ] {
        let def = load_benchmark(root, name).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(
            def.generator,
            Some(GeneratorSpec::Snap {
                graph: Some(graph),
                url: None,
                sha256: None,
                queries: None,
            }),
            "{name}"
        );
        assert!(
            def.description.contains(graph.name()),
            "{name}: description {:?} should name {}",
            def.description,
            graph.name()
        );
        assert!(def.relations.is_empty() && def.queries.is_empty(), "{name}");
    }
}

/// The extras count what the fixture contains, `meta.json` records the
/// provenance of the `.gz`, and `edge.parquet` holds exactly the cleaned,
/// symmetrised rows in file order.
#[test]
fn the_mini_graph_drops_its_self_loop_and_reversed_duplicate() {
    let text = fixture_text();
    let lines = data_lines(&text);
    let generation = generate_mini(None);
    let extras = &generation.meta.extras;

    assert_eq!(extras.input_edges, lines.len() as u64);
    assert_eq!(extras.dropped_self_loops, 1);
    assert_eq!(extras.dropped_duplicates, 1);
    assert_eq!(extras.graph, None);
    assert_eq!(extras.source_url, None, "a local source has no URL");

    let raw = generation.dir.join("raw/mini.ungraph.txt.gz");
    assert_eq!(
        fs::read(&raw).expect("the .gz is kept under raw/"),
        fs::read(&generation.gz).unwrap(),
        "raw/ keeps the input byte for byte"
    );
    assert_eq!(extras.gz_sha256, sha256_hex(&raw).unwrap());

    assert_eq!(generation.meta.kind, "snap");
    assert_eq!(generation.meta.tag, "mini");
    assert_eq!(generation.meta.spec_hash, None);
    assert_eq!(generation.meta.relations.len(), 1);
    let edge = &generation.meta.relations[0];
    assert_eq!(edge.name, "edge");
    assert_eq!(edge.columns, ["src", "dst"]);
    assert_eq!(edge.rows, 2 * (lines.len() as u64 - 2));
    let parquet = generation.dir.join("edge.parquet");
    assert_eq!(edge.sha256, sha256_hex(&parquet).unwrap());

    // The expected rows, from a fresh reading of the text: each kept line's
    // (s, d) then (d, s), skipping a self-loop and a repeat of an unordered
    // pair.
    let mut seen = HashSet::new();
    let mut want: Vec<Vec<usize>> = Vec::new();
    for line in &lines {
        let mut fields = line.split_whitespace();
        let s: usize = fields.next().unwrap().parse().unwrap();
        let d: usize = fields.next().unwrap().parse().unwrap();
        assert!(fields.next().is_none(), "{line:?}");
        if s != d && seen.insert((s.min(d), s.max(d))) {
            want.push(vec![s, d]);
            want.push(vec![d, s]);
        }
    }
    let (_, tuples) = read_parquet(&parquet).expect("edge.parquet reads back");
    assert_eq!(tuples.to_vecs(), want);
}

/// The generator's `expected` equals the independent derivation of the
/// fixture's counts, in the default (issue) order, and every one is
/// non-trivial.
#[test]
fn the_emitted_expected_counts_match_an_independent_count() {
    let generation = generate_mini(None);
    let bench = emitted_benchmark(&generation.dir);
    let want: Vec<(String, u64)> = MINI_COUNTS
        .iter()
        .map(|&(name, count)| (name.to_string(), count))
        .collect();
    assert_eq!(emitted_counts(&bench), want);
    let names: Vec<&str> = SnapQuery::ALL.iter().map(|q| q.name()).collect();
    assert_eq!(
        names,
        MINI_COUNTS
            .iter()
            .map(|&(name, _)| name)
            .collect::<Vec<_>>()
    );
    assert!(MINI_COUNTS.iter().all(|&(_, count)| count > 0));
}

/// Every query returns its `expected` count on every cell, under every
/// optimiser.
///
/// The plan an optimiser picks changes the join's descent order and so the
/// columns each index lists first, never the answer; iterating the CLI enum
/// covers optimisers added later with no edit here.
#[test]
fn every_cell_and_optimiser_returns_the_expected_counts() {
    let generation = generate_mini(None);
    let bench = emitted_benchmark(&generation.dir);
    assert_eq!(bench.queries.len(), SnapQuery::ALL.len());
    assert!(
        bench
            .queries
            .iter()
            .all(|q| q.expected.is_some_and(|count| count > 0)),
        "a zero expected count would make the comparison vacuous: {:?}",
        emitted_counts(&bench)
    );

    let optimisers = Optimiser::value_variants();
    let mut mismatches: Vec<String> = Vec::new();
    for &optimiser in optimisers {
        let planner = Planner::new(optimiser.instantiate());
        let label = optimiser.axis_value().to_string();
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
    }
    assert!(
        mismatches.is_empty(),
        "SNAP cardinality mismatches ({} across {} queries x {} optimisers x 3 cells):\n{}",
        mismatches.len(),
        bench.queries.len(),
        optimisers.len(),
        mismatches.join("\n"),
    );
}

/// One input gives one `edge.parquet`, byte for byte. (Bytes are stable only
/// within a build: the Parquet footer records the writer's version.)
#[test]
fn two_generations_write_a_byte_identical_edge_parquet() {
    let first = generate_mini(None);
    let second = generate_mini(None);
    let a = fs::read(first.dir.join("edge.parquet")).unwrap();
    let b = fs::read(second.dir.join("edge.parquet")).unwrap();
    assert!(!a.is_empty());
    assert!(a == b, "edge.parquet differs between two generations");
    assert_eq!(first.meta.relations, second.meta.relations);
}

/// `meta.json` on disk is `GeneratedMeta<SnapExtras>`, with SNAP's fields
/// flattened beside the common ones.
#[test]
fn meta_json_round_trips_as_generated_meta() {
    let generation = generate_mini(None);
    let text = fs::read_to_string(generation.dir.join("meta.json")).unwrap();
    let read: GeneratedMeta<SnapExtras> = serde_json::from_str(&text).expect("meta.json parses");
    assert_eq!(read, generation.meta);

    let value: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(value["kind"], "snap");
    assert_eq!(value["input_edges"], generation.meta.extras.input_edges);
    assert_eq!(value["dropped_self_loops"], 1);
    assert_eq!(value["dropped_duplicates"], 1);
    assert_eq!(value["query_count"], 4);
}

/// A `queries` subset is emitted in the order given, each query keeping its
/// own count (so `expected` is paired with the right rule, not the right
/// position).
#[test]
fn a_query_subset_is_emitted_in_the_order_given() {
    let generation = generate_mini(Some(&[SnapQuery::Clique4, SnapQuery::Triangle]));
    let bench = emitted_benchmark(&generation.dir);
    assert_eq!(emitted_counts(&bench), vec![
        ("clique4".to_string(), 24),
        ("triangle".to_string(), 36)
    ]);
    assert_eq!(generation.meta.query_count, 2);
}
