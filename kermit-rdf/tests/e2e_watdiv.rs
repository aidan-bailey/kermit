//! End-to-end test that drives the real vendored watdiv binary.
//!
//! Skipped on non-Linux, non-x86_64, or hosts without `bwrap`.

use {
    kermit_rdf::{
        driver::{DriverInputs, StressParams},
        pipeline::{run_pipeline, PipelineInputs},
    },
    std::path::PathBuf,
};

fn vendor_root() -> PathBuf { PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("vendor/watdiv") }

fn skip_if_unsupported() -> bool {
    if cfg!(not(target_os = "linux")) || cfg!(not(target_arch = "x86_64")) {
        eprintln!("skipping watdiv e2e: requires linux x86_64");
        return true;
    }
    if std::process::Command::new("bwrap")
        .arg("--version")
        .output()
        .is_err()
    {
        eprintln!("skipping watdiv e2e: bwrap not found");
        return true;
    }
    // Probe 1: does the vendored binary load? On NixOS-style systems
    // without a global libstdc++, exec returns 127.
    let bin = vendor_root().join("bin/Release/watdiv");
    match std::process::Command::new(&bin).output() {
        | Ok(out) if out.status.code() == Some(127) => {
            eprintln!("skipping watdiv e2e: vendored binary missing dynamic deps");
            return true;
        },
        | Err(_) => {
            eprintln!("skipping watdiv e2e: cannot execute vendored binary");
            return true;
        },
        | _ => {},
    }
    // Probe 2: can bwrap construct the /usr/share/dict/words bind we need?
    // The real driver uses `--bind / /` then binds words at the absolute
    // dict path, which fails on hosts without /usr/share/dict (e.g. NixOS).
    let words = vendor_root().join("files/words");
    let bwrap_ok = std::process::Command::new("bwrap")
        .args(["--bind", "/", "/", "--bind"])
        .arg(&words)
        .args(["/usr/share/dict/words", "true"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if !bwrap_ok {
        eprintln!("skipping watdiv e2e: bwrap cannot bind /usr/share/dict/words on this host");
        return true;
    }
    false
}

#[test]
fn watdiv_sf1_pipeline_succeeds_and_produces_expected_artifacts() {
    if skip_if_unsupported() {
        return;
    }
    let vendor = vendor_root();
    let dir = tempfile::tempdir().unwrap();
    let inputs = PipelineInputs {
        driver: DriverInputs {
            watdiv_bin: &vendor.join("bin/Release/watdiv"),
            vendor_files: &vendor.join("files"),
            model_file: &vendor.join("MODEL.txt"),
            scale: 1,
            stress: StressParams::default(),
            query_count_per_template: 5,
            use_bwrap: true,
        },
        out_dir: dir.path(),
        bench_name: "watdiv-stress-1-e2e",
        tag: "e2e",
    };
    let meta = run_pipeline(&inputs).expect("pipeline failed");
    assert!(meta.triple_count > 0, "no triples generated");
    assert!(meta.relation_count > 0, "no relations partitioned");

    assert!(dir.path().join("benchmark.yml").exists());
    assert!(dir.path().join("dict.parquet").exists());
    assert!(dir.path().join("meta.json").exists());

    let part = kermit_rdf::partition::partition(dir.path().join("raw/data.nt")).unwrap();
    assert_eq!(
        part.relations.len() as u32,
        meta.relation_count,
        "relation count drifted between meta and re-parse"
    );
}
