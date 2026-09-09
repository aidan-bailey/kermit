//! `bench fetch` re-hashes every relation that declares a `sha256`.
//! Uses a fake cache whose relation file already exists, so no download
//! happens and the check runs on the cached bytes.

use {
    std::{fs, path::PathBuf, process::Command},
    tempfile::TempDir,
};

fn kermit_bin() -> PathBuf { PathBuf::from(env!("CARGO_BIN_EXE_kermit")) }

fn fixtures_dir() -> PathBuf { PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures") }

fn skip_unsupported() -> bool {
    if cfg!(not(target_os = "linux")) {
        eprintln!("skipping cli_bench_fetch_integrity test: XDG_CACHE_HOME needs Linux");
        return true;
    }
    false
}

/// A one-relation cache-side benchmark `pinned` whose `parentcountry`
/// parquet is copied from the fixture and declared with `sha256`.
fn make_cache(sha256: &str) -> TempDir {
    let cache = tempfile::tempdir().unwrap();
    let dir = cache.path().join("kermit/benchmarks/pinned");
    fs::create_dir_all(&dir).unwrap();
    let src = fixtures_dir().join("watdiv-mini/artifacts/parentcountry.parquet");
    fs::copy(&src, dir.join("parentcountry.parquet")).unwrap();
    fs::write(
        dir.join("benchmark.yml"),
        format!(
            "name: pinned\n\
             description: integrity fixture\n\
             relations:\n\
             - name: parentcountry\n  \
               url: file:///fixture/parentcountry.parquet\n  \
               sha256: {sha256}\n\
             queries:\n\
             - name: q\n  \
               description: q\n  \
               query: 'Q(X, Y) :- parentcountry(X, Y).'\n"
        ),
    )
    .unwrap();
    fs::write(dir.join("meta.json"), "{}").unwrap();
    cache
}

fn fetch(cache: &TempDir) -> std::process::Output {
    let workspace = tempfile::tempdir().unwrap();
    Command::new(kermit_bin())
        .env("XDG_CACHE_HOME", cache.path())
        .env("KERMIT_WORKSPACE", workspace.path())
        .args(["bench", "fetch", "pinned"])
        .output()
        .unwrap()
}

#[test]
fn fetch_verifies_a_correct_digest() {
    if skip_unsupported() {
        return;
    }
    let digest = kermit_bench::cache::sha256_hex(
        &fixtures_dir().join("watdiv-mini/artifacts/parentcountry.parquet"),
    )
    .unwrap();
    let output = fetch(&make_cache(&digest));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "stderr: {stderr}");
    assert!(stderr.contains("Verified 1 relation(s)"), "{stderr}");
}

#[test]
fn fetch_rejects_a_wrong_digest() {
    if skip_unsupported() {
        return;
    }
    let wrong = "0".repeat(64);
    let output = fetch(&make_cache(&wrong));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "stderr: {stderr}");
    assert!(
        stderr.contains("integrity check failed for relation 'parentcountry'"),
        "{stderr}"
    );
    assert!(stderr.contains(&wrong), "{stderr}");
}
