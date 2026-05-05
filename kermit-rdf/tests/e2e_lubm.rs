//! End-to-end test: drive the vendored LUBM-UBA jar and verify it
//! produces a non-empty `Universities.nt` matching the canonical LUBM(1, 0)
//! triple count.
//!
//! Gated on `which java` succeeding — CI runners without a JDK will skip
//! this test (mirrors the gating used by `e2e_watdiv`).

use {
    kermit_rdf::lubm::driver::{drive, LubmDriverInputs, DEFAULT_ONTOLOGY_IRI},
    std::{path::PathBuf, process::Command},
};

fn java_available() -> bool {
    Command::new("java")
        .arg("-version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn vendored_jar() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("vendor")
        .join("lubm-uba")
        .join("lubm-uba.jar")
}

#[test]
#[cfg_attr(miri, ignore = "miri does not support spawning java")]
fn drives_lubm_one_university_end_to_end() {
    if !java_available() {
        eprintln!("skipping: `java` not on PATH");
        return;
    }
    let jar = vendored_jar();
    if !jar.exists() {
        eprintln!("skipping: vendored jar missing at {jar:?}");
        return;
    }

    let inputs = LubmDriverInputs {
        jar_path: &jar,
        scale: 1,
        seed: 0,
        start_index: 0,
        threads: 1,
        ontology_iri: DEFAULT_ONTOLOGY_IRI,
    };
    let raw = drive(&inputs).expect("drive must succeed for LUBM(1, 0)");

    // The gunzipped output must exist and be non-empty.
    assert!(
        raw.data_nt.exists(),
        "data.nt must be present at {:?}",
        raw.data_nt
    );
    let bytes = std::fs::metadata(&raw.data_nt)
        .expect("can stat data.nt")
        .len();
    assert!(
        bytes > 1_000_000,
        "LUBM(1, 0) NT should be >1 MB, got {bytes} bytes"
    );

    // Triple count: ~103,076 was observed at vendor time. Allow ±5 % to
    // tolerate jar regenerations that slightly perturb counts via JDK
    // upgrades (the bit-identity claim in lubm-uba-rs covers byte-level
    // output, not triple counts modulo serialization).
    let nt = std::fs::read_to_string(&raw.data_nt).expect("read data.nt");
    let triples = nt
        .lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .count();
    assert!(
        triples > 95_000 && triples < 110_000,
        "triple count out of expected band for LUBM(1, 0): got {triples}, expected ~103,076"
    );

    // Provenance fields echo back unchanged.
    assert_eq!(raw.scale, 1);
    assert_eq!(raw.seed, 0);
    assert_eq!(raw.start_index, 0);
    assert_eq!(raw.ontology_iri, DEFAULT_ONTOLOGY_IRI);
}
