//! End-to-end test for the WatDiv Basic Testing pipeline: drives the real
//! vendored binary on the 20 vendored L/S/F/C templates.
//!
//! Skipped on non-Linux, non-x86_64, or hosts without `bwrap` — mirrors
//! `e2e_watdiv`.

use {
    kermit_rdf::{
        driver::{DriverInputs, StressParams},
        pipeline::{run_basic_pipeline, PipelineInputs},
    },
    std::path::PathBuf,
};

fn vendor_root() -> PathBuf { PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("vendor/watdiv") }

fn skip_if_unsupported() -> bool {
    if cfg!(not(target_os = "linux")) || cfg!(not(target_arch = "x86_64")) {
        eprintln!("skipping watdiv-basic e2e: requires linux x86_64");
        return true;
    }
    if std::process::Command::new("bwrap").arg("--version").output().is_err() {
        eprintln!("skipping watdiv-basic e2e: bwrap not found");
        return true;
    }
    let bin = vendor_root().join("bin/Release/watdiv");
    match std::process::Command::new(&bin).output() {
        | Ok(out) if out.status.code() == Some(127) => {
            eprintln!("skipping watdiv-basic e2e: vendored binary missing dynamic deps");
            return true;
        },
        | Err(_) => {
            eprintln!("skipping watdiv-basic e2e: cannot execute vendored binary");
            return true;
        },
        | _ => {},
    }
    let words = vendor_root().join("files/words");
    let bwrap_ok = std::process::Command::new("bwrap")
        .args(["--bind", "/", "/"])
        .args(["--tmpfs", "/usr"])
        .args(["--ro-bind-try", "/usr/bin", "/usr/bin"])
        .args(["--ro-bind-try", "/usr/lib", "/usr/lib"])
        .args(["--ro-bind-try", "/usr/lib64", "/usr/lib64"])
        .args(["--dir", "/usr/share/dict"])
        .arg("--bind")
        .arg(&words)
        .args(["/usr/share/dict/words", "true"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if !bwrap_ok {
        eprintln!("skipping watdiv-basic e2e: bwrap cannot bind /usr/share/dict/words");
        return true;
    }
    false
}

#[test]
#[cfg_attr(
    miri,
    ignore = "spawns bwrap/watdiv subprocesses via std::process::Command"
)]
fn watdiv_basic_pipeline_produces_twenty_queries() {
    if skip_if_unsupported() {
        return;
    }
    let vendor = vendor_root();
    let testsuite = vendor.join("testsuite");
    let dir = tempfile::tempdir().unwrap();
    let inputs = PipelineInputs {
        driver: DriverInputs {
            watdiv_bin: &vendor.join("bin/Release/watdiv"),
            vendor_files: &vendor.join("files"),
            model_file: &vendor.join("MODEL.txt"),
            scale: 1,
            stress: StressParams::default(),
            query_count_per_template: 1,
            use_bwrap: true,
        },
        out_dir: dir.path(),
        bench_name: "watdiv-basic-e2e",
        tag: "e2e",
        spec_hash: None,
    };
    let meta = run_basic_pipeline(&inputs, &testsuite).expect("basic pipeline failed");

    assert_eq!(meta.kind, "watdiv-basic-onthefly");
    assert!(meta.triple_count > 0, "no triples generated");
    assert!(meta.relation_count > 0, "no relations partitioned");
    // 20 templates × 1 query each = 20 queries.
    assert_eq!(meta.query_count, 20, "expected one query per L/S/F/C template");

    assert!(dir.path().join("benchmark.yml").exists());
    assert!(dir.path().join("dict.parquet").exists());
    assert!(dir.path().join("meta.json").exists());
}
