//! A cached relation file that is not a whole Parquet file — zero bytes, or
//! cut off before its footer — is rejected when the benchmark is resolved,
//! naming the file and how to clear it, instead of surfacing later as an
//! opaque Parquet decode error (#78). Nothing is re-downloaded: the
//! relation's URL is unreachable, and the error must come first.

use {
    std::{fs, path::PathBuf, process::Command},
    tempfile::TempDir,
};

fn kermit_bin() -> PathBuf { PathBuf::from(env!("CARGO_BIN_EXE_kermit")) }

fn skip_unsupported() -> bool {
    if cfg!(not(target_os = "linux")) {
        eprintln!("skipping cli_bench_corrupt_cache test: XDG_CACHE_HOME needs Linux");
        return true;
    }
    false
}

/// A workspace declaring benchmark `cut` with one `url:` relation, and a
/// cache holding `contents` at that relation's cache path.
fn setup(contents: &[u8]) -> (TempDir, TempDir, PathBuf) {
    let workspace = tempfile::tempdir().unwrap();
    let benchmarks = workspace.path().join("benchmarks");
    fs::create_dir_all(&benchmarks).unwrap();
    fs::write(
        benchmarks.join("cut.yml"),
        "name: cut\n\
         description: a truncated cached relation\n\
         relations:\n\
         - name: edge\n  \
           url: http://127.0.0.1:9/never-fetched.parquet\n\
         queries:\n\
         - name: q\n  \
           description: q\n  \
           query: 'Q(X, Y) :- edge(X, Y).'\n",
    )
    .unwrap();
    let cache = tempfile::tempdir().unwrap();
    let dir = cache.path().join("kermit/benchmarks/cut");
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join("edge.parquet");
    fs::write(&file, contents).unwrap();
    (workspace, cache, file)
}

fn kermit(workspace: &TempDir, cache: &TempDir, args: &[&str]) -> std::process::Output {
    Command::new(kermit_bin())
        .env("RUST_BACKTRACE", "0")
        .env("RUST_LIB_BACKTRACE", "0")
        .env("XDG_CACHE_HOME", cache.path())
        .env("KERMIT_WORKSPACE", workspace.path())
        .args(args)
        .output()
        .unwrap()
}

fn assert_rejected(output: &std::process::Output, file: &std::path::Path) {
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(1), "stderr: {stderr}");
    assert!(stderr.contains("not a usable Parquet file"), "{stderr}");
    assert!(stderr.contains(&file.display().to_string()), "{stderr}");
    assert!(stderr.contains("kermit bench clean cut"), "{stderr}");
    assert!(
        !stderr.contains("download"),
        "a cached file must not be re-downloaded: {stderr}"
    );
}

#[test]
fn fetch_rejects_a_zero_byte_cached_relation() {
    if skip_unsupported() {
        return;
    }
    let (workspace, cache, file) = setup(b"");
    assert_rejected(
        &kermit(&workspace, &cache, &["bench", "fetch", "cut"]),
        &file,
    );
    assert!(file.exists(), "the guard reports; it never deletes");
}

#[test]
fn run_rejects_a_truncated_cached_relation_before_decoding_it() {
    if skip_unsupported() {
        return;
    }
    let (workspace, cache, file) = setup(b"PAR1 a row group with no footer");
    let output = kermit(&workspace, &cache, &[
        "bench",
        "run",
        "cut",
        "-i",
        "tree-trie",
        "-a",
        "leapfrog-triejoin",
        "-m",
        "iteration",
    ]);
    assert_rejected(&output, &file);
}
