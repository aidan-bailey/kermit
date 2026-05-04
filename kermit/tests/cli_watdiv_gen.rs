//! End-to-end CLI smoke test for `kermit bench watdiv-gen`.
//!
//! Skipped on non-Linux/non-x86_64 hosts, hosts without `bwrap`, hosts where
//! the vendored binary can't load (e.g. NixOS w/o libstdc++ on the FHS path),
//! and hosts where bwrap can't bind /usr/share/dict/words.

use std::{path::PathBuf, process::Command};

fn vendor_watdiv_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("kermit-rdf/vendor/watdiv/bin/Release/watdiv")
}

fn vendor_words() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("kermit-rdf/vendor/watdiv/files/words")
}

fn skip_unsupported() -> bool {
    if cfg!(not(target_os = "linux")) || cfg!(not(target_arch = "x86_64")) {
        eprintln!("skipping cli_watdiv_gen: needs linux x86_64");
        return true;
    }
    if Command::new("bwrap").arg("--version").output().is_err() {
        eprintln!("skipping cli_watdiv_gen: bwrap not available");
        return true;
    }
    let bin = vendor_watdiv_bin();
    match Command::new(&bin).output() {
        | Ok(out) if out.status.code() == Some(127) => {
            eprintln!("skipping cli_watdiv_gen: vendored binary missing dynamic deps");
            return true;
        },
        | Err(_) => {
            eprintln!("skipping cli_watdiv_gen: cannot execute vendored binary");
            return true;
        },
        | _ => {},
    }
    let words = vendor_words();
    let bwrap_ok = Command::new("bwrap")
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
        eprintln!("skipping cli_watdiv_gen: bwrap cannot bind /usr/share/dict/words on this host");
        return true;
    }
    false
}

#[test]
fn watdiv_gen_sf1_smoke_test() {
    if skip_unsupported() {
        return;
    }
    let out = tempfile::tempdir().unwrap();
    let bin = env!("CARGO_BIN_EXE_kermit");
    let status = Command::new(bin)
        .arg("bench")
        .arg("watdiv-gen")
        .arg("--scale")
        .arg("1")
        .arg("--tag")
        .arg("smoke")
        .arg("--query-count")
        .arg("3")
        .arg("--output-dir")
        .arg(out.path())
        .status()
        .expect("failed to run kermit");
    assert!(status.success(), "watdiv-gen failed");

    let bench_dir = out.path().join("watdiv-stress-1-smoke");
    assert!(bench_dir.join("benchmark.yml").exists());
    assert!(bench_dir.join("meta.json").exists());
    assert!(bench_dir.join("dict.parquet").exists());
}
