//! Java-free miniature LUBM oracle: the 14 real LUBM queries over a
//! hand-built Univ-Bench ABox (57 triples) with hand-counted answers.
//!
//! `lubm_cardinalities.rs` checks the paper's LUBM(1, 0) counts but needs the
//! vendored jar and `java`, so it skips on CI. This test runs the same
//! post-driver path — Univ-Bench entailment, partition, translate, emit
//! (`kermit_rdf::lubm::pipeline::process_artifacts`), then [`lftj_join`]
//! under every optimiser — on the committed `tests/fixtures/lubm-mini/abox.nt`,
//! with no external tool. It lives in the `kermit` crate for the same reason
//! as `lubm_cardinalities.rs`: only the binary depends on both `kermit-rdf`
//! and the join engine.
//!
//! # Derivation
//!
//! Counts follow the rule set `kermit_rdf::lubm::entailment` implements and
//! were derived by hand from the ABox. Every query projects all of its
//! variables, so a count is the number of distinct bindings. *Raw* is the
//! count over the ABox with no entailment; only q1 and q14 cannot differ,
//! because no rule derives `GraduateStudent` or `UndergraduateStudent`.
//! *Rules* lists the rule families the answers need — subClassOf (SC),
//! subPropertyOf (SP), inverseOf (INV), `subOrganizationOf` transitivity
//! (TR), `Chair` realisation (RE) — so dropping one family from the
//! entailment fails exactly the rows naming it. The file has 58 lines, 57
//! distinct triples (one type assertion is repeated, as UBA does); entailment
//! adds 51 (108 in total).
//!
//! | query | expected | raw | rules           |
//! |-------|---------:|----:|-----------------|
//! | q1    |        1 |   1 | —               |
//! | q2    |        1 |   0 | TR              |
//! | q3    |        2 |   1 | SC              |
//! | q4    |        2 |   0 | SC, SP, RE      |
//! | q5    |        5 |   0 | SC, SP, RE      |
//! | q6    |        3 |   0 | SC              |
//! | q7    |        3 |   0 | SC              |
//! | q8    |        2 |   0 | SC, TR          |
//! | q9    |        3 |   0 | SC              |
//! | q10   |        2 |   0 | SC              |
//! | q11   |        1 |   0 | TR              |
//! | q12   |        1 |   0 | SP, TR, RE      |
//! | q13   |        4 |   0 | SC, SP, INV, RE |
//! | q14   |        1 |   1 | —               |
//!
//! The answers, with each individual abbreviated to the distinguishing part
//! of its IRI (`http://www.University0.edu` is University0). Individuals of
//! University1 are prefixed `U1/`:
//!
//! - **q1** GraduateStudent0: asserted type and `takesCourse GraduateCourse0`.
//! - **q2** (GraduateStudent0, University0, Department0): Department0 is a
//!   sub-organisation of University0 only through College0 (TR).
//! - **q3** Publication0 (asserted `Publication`) and Publication1
//!   (`JournalArticle ⊑ Article ⊑ Publication`).
//! - **q4** FullProfessor0, which has no asserted type: `Chair` by RE, then
//!   `Professor`; `worksFor Department0` from `headOf`. AssociateProfessor0:
//!   `AssociateProfessor ⊑ Professor`. AssistantProfessor0 is a `Professor`
//!   working for Department0 but has no name, email or telephone.
//! - **q5** Members of Department0: GraduateStudent0 and UndergraduateStudent0
//!   (asserted); AssociateProfessor0 and AssistantProfessor0 (from `worksFor`
//!   by SP); FullProfessor0 (from `headOf`, by SP twice). All five are `Person`
//!   by SC (FullProfessor0 only after RE).
//! - **q6** GraduateStudent0, UndergraduateStudent0, U1/GraduateStudent0: each
//!   a `Student` by SC.
//! - **q7** (GraduateStudent0, GraduateCourse0), (UndergraduateStudent0,
//!   Course0), (UndergraduateStudent0, GraduateCourse0): AssociateProfessor0
//!   teaches both courses; `GraduateCourse ⊑ Course`.
//! - **q8** GraduateStudent0 and UndergraduateStudent0, each with Department0
//!   and their one email; Department0 reaches University0 by TR.
//! - **q9** The three q7 pairs, each with advisor AssociateProfessor0
//!   (`AssociateProfessor ⊑ Professor ⊑ Faculty`).
//! - **q10** GraduateStudent0, UndergraduateStudent0: both take
//!   GraduateCourse0.
//! - **q11** ResearchGroup0: two TR hops, via Department0 and College0.
//! - **q12** (FullProfessor0, Department0): `Chair` by RE, `worksFor` from
//!   `headOf` by SP, and Department0 under University0 by TR.
//! - **q13** FullProfessor0, AssociateProfessor0, GraduateStudent0: each has a
//!   doctoral, masters or undergraduate degree from University0, which SP lifts
//!   to `degreeFrom` and INV turns into `hasAlumnus`. AssistantProfessor0: an
//!   asserted `hasAlumnus`, which also keeps the relation present when SP or
//!   INV is dropped. All four are `Person` by SC (FullProfessor0 only after
//!   RE).
//! - **q14** UndergraduateStudent0: asserted type.
//!
//! # Distractors
//!
//! Every non-class constant in the queries excludes at least one would-be
//! answer, so a constant the pipeline or the engine fails to enforce changes a
//! count. Each entry gives the counts with that constant widened to an unbound
//! variable (kermit counts every binding, the widened variable included) and
//! the triples it excludes:
//!
//! - GraduateCourse0 (q1 → 2, q10 → 4): U1/GraduateStudent0 takes U1/Course0.
//! - AssistantProfessor0 (q3 → 3): AssociateProfessor0 wrote
//!   AssociateProfessor0/Publication0.
//! - Department0 (q4 → 3, q5 → 7): U1/FullProfessor0 heads, and
//!   U1/GraduateStudent0 belongs to, U1/Department0.
//! - AssociateProfessor0 (q7 → 4): U1/FullProfessor0 teaches U1/Course0.
//! - University0 (q8 → 5, q11 → 5, q12 → 3, q13 → 5): U1/Department0 and its
//!   research group sit under University1, whose alumnus is U1/FullProfessor0.
//!
//! A complete OWL reasoner over the published `univ-bench.owl` reaches some of
//! these memberships by extra routes (`emailAddress` has domain `Person`,
//! `teacherOf` has range `Course`, …) but yields the same 14 counts on this
//! ABox, so the oracle does not depend on where kermit's hardcoded rule set
//! stops.

use {
    clap::ValueEnum,
    kermit::db::lftj_join,
    kermit_algos::{JoinQuery, LeapfrogTriejoin, Optimiser},
    kermit_bench::BenchmarkDefinition,
    kermit_ds::{RelationFileExt, TreeTrie},
    kermit_rdf::lubm::{
        driver::{LubmDriverInputs, LubmRawArtifacts, DEFAULT_ONTOLOGY_IRI},
        pipeline::{process_artifacts, LubmPipelineInputs},
        queries::lubm_query_specs,
        sandbox::LubmStagingDir,
    },
    std::{
        collections::{BTreeMap, HashMap},
        fs,
        path::{Path, PathBuf},
    },
};

/// Hand-counted cardinalities; see the module doc for each derivation.
const EXPECTED: &[(&str, u64)] = &[
    ("q1", 1),
    ("q2", 1),
    ("q3", 2),
    ("q4", 2),
    ("q5", 5),
    ("q6", 3),
    ("q7", 3),
    ("q8", 2),
    ("q9", 3),
    ("q10", 2),
    ("q11", 1),
    ("q12", 1),
    ("q13", 4),
    ("q14", 1),
];

fn abox_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/lubm-mini/abox.nt")
}

/// Loads the generated relations into a fresh engine planned by `optimiser`,
/// runs every query, and returns one line per query whose result count
/// differs from the hand-counted cardinality.
fn cardinality_mismatches(
    bench: &BenchmarkDefinition, dir: &Path, optimiser: Optimiser, expected: &HashMap<&str, u64>,
) -> Vec<String> {
    let planner = optimiser.instantiate();
    let mut relations: BTreeMap<String, TreeTrie> = BTreeMap::new();
    for rel in &bench.relations {
        let path = dir.join(format!("{}.parquet", rel.name));
        let trie = TreeTrie::from_parquet(&path)
            .unwrap_or_else(|e| panic!("failed to load relation {path:?}: {e}"));
        relations.insert(rel.name.clone(), trie);
    }

    let mut mismatches = Vec::new();
    for q in &bench.queries {
        let want = expected[q.name.as_str()];
        let parsed: JoinQuery = q.query.parse().expect("datalog parse failure");
        let got = lftj_join::<TreeTrie, LeapfrogTriejoin>(&relations, parsed, planner.as_ref())
            .len() as u64;
        if got != want {
            mismatches.push(format!(
                "  [{}] {}: got {got}, expected {want}\n    query: {}",
                optimiser.axis_value(),
                q.name,
                q.query
            ));
        }
    }
    mismatches
}

#[test]
fn mini_lubm_abox_query_cardinalities_match_hand_derivation() {
    let specs = lubm_query_specs(false);
    let expected: HashMap<&str, u64> = EXPECTED.iter().copied().collect();
    let spec_names: Vec<&str> = specs.iter().map(|s| s.name.as_str()).collect();
    let expected_names: Vec<&str> = EXPECTED.iter().map(|(name, _)| *name).collect();
    assert_eq!(
        spec_names, expected_names,
        "every LUBM query needs a hand-counted cardinality"
    );

    // Stand in for the driver: put the ABox where the jar's gunzipped output
    // would be. No jar runs, but `LubmMeta` records the jar's SHA-256, so
    // point `jar_path` at a placeholder file.
    let stage = LubmStagingDir::create().expect("create staging dir");
    let data_nt = stage.ntriples_output_path();
    fs::copy(abox_path(), &data_nt).expect("stage the ABox");
    let jar_placeholder = stage.root().join("no-jar-was-run");
    fs::write(&jar_placeholder, b"").expect("write jar placeholder");

    let out = tempfile::tempdir().expect("create temp dir");
    let inputs = LubmPipelineInputs {
        driver: LubmDriverInputs {
            jar_path: &jar_placeholder,
            scale: 1,
            seed: 0,
            start_index: 0,
            threads: 1,
            ontology_iri: DEFAULT_ONTOLOGY_IRI,
        },
        out_dir: out.path(),
        bench_name: "lubm-mini",
        tag: "mini-oracle",
        queries: &specs,
        spec_hash: None,
    };
    let raw = LubmRawArtifacts {
        data_nt,
        scale: 1,
        seed: 0,
        start_index: 0,
        ontology_iri: DEFAULT_ONTOLOGY_IRI.to_string(),
        stage,
    };
    let meta = process_artifacts(&inputs, &raw).expect("mini LUBM pipeline must succeed");

    let yaml = fs::read_to_string(out.path().join("benchmark.yml"))
        .expect("emitted benchmark.yml missing");
    let bench: BenchmarkDefinition = serde_yaml::from_str(&yaml).expect("benchmark.yml malformed");
    assert_eq!(bench.queries.len(), EXPECTED.len());

    // The plan an optimiser picks changes the join's descent order, never its
    // answer. Iterating the CLI enum covers every optimiser, including ones
    // added later, with no edit here.
    let optimisers = Optimiser::value_variants();
    let mut mismatches: Vec<String> = Vec::new();
    for &optimiser in optimisers {
        mismatches.extend(cardinality_mismatches(
            &bench,
            out.path(),
            optimiser,
            &expected,
        ));
    }
    assert!(
        mismatches.is_empty(),
        "mini LUBM cardinality mismatches ({} across {} queries x {} optimisers):\n{}",
        mismatches.len(),
        bench.queries.len(),
        optimisers.len(),
        mismatches.join("\n"),
    );

    // 58 input lines, 57 of them distinct; `derived_triple_count` must count
    // against the distinct triples.
    assert_eq!(meta.triple_count_pre_entailment, 58);
    assert_eq!(meta.derived_triple_count, 51);
    assert_eq!(meta.triple_count_post_entailment, 108);
}
