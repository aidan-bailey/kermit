//! Issue #78 row 7: `bench gen watdiv|lubm --scale 0` is a usage error,
//! rejected by the CLI before anything runs. It used to reach the
//! vendored generator — WatDiv aborted with `boost::bad_lexical_cast`, the
//! LUBM jar rejected it — after creating an empty cache directory.
//!
//! Needs neither Java nor the WatDiv binary: clap rejects the flag first.

use {
    std::{path::PathBuf, process::Command},
    tempfile::TempDir,
};

fn kermit_bin() -> PathBuf { PathBuf::from(env!("CARGO_BIN_EXE_kermit")) }

fn assert_scale_zero_rejected(generator: &str) {
    let cache = TempDir::new().unwrap();
    let output = Command::new(kermit_bin())
        .env("XDG_CACHE_HOME", cache.path())
        .args(["bench", "gen", generator, "--scale", "0", "--tag", "t"])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        output.status.code(),
        Some(2),
        "gen {generator} --scale 0 must be a clap usage error; stderr: {stderr}"
    );
    assert!(
        stderr.contains("'0'") && stderr.contains("--scale"),
        "the error must name the flag and the value: {stderr}"
    );
    assert_eq!(
        std::fs::read_dir(cache.path()).unwrap().count(),
        0,
        "nothing may be written before the flag is validated"
    );
}

#[test]
fn watdiv_scale_zero_is_a_usage_error() { assert_scale_zero_rejected("watdiv"); }

#[test]
fn lubm_scale_zero_is_a_usage_error() { assert_scale_zero_rejected("lubm"); }
