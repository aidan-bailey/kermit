//! Issue #78 row 7: `bench gen watdiv|lubm --scale 0` is a usage error,
//! rejected by the CLI before anything runs. It used to reach the
//! vendored generator — WatDiv aborted with `boost::bad_lexical_cast`, the
//! LUBM jar rejected it — after creating an empty cache directory.
//!
//! `bench gen h2o --scale` has a floor, `H2O_MIN_SCALE` (10000000, upstream's
//! own), and a scale below it is the same usage error (#95).
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

/// `bench gen h2o --scale` is floored at upstream's 1e7: below it `small`
/// (N / 1e6 keys) has no left-only or right-only key. Like `--scale 0`, it
/// is refused by clap before the handler creates anything.
#[test]
fn h2o_scale_below_the_floor_is_a_usage_error() {
    let cache = TempDir::new().unwrap();
    let output = Command::new(kermit_bin())
        .env("XDG_CACHE_HOME", cache.path())
        .args(["bench", "gen", "h2o", "--scale", "1000", "--tag", "t"])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        output.status.code(),
        Some(2),
        "gen h2o --scale 1000 must be a clap usage error; stderr: {stderr}"
    );
    assert!(
        stderr.contains("'1000'") && stderr.contains("--scale") && stderr.contains("10000000"),
        "the error must name the flag, the value and the floor: {stderr}"
    );
    assert_eq!(
        std::fs::read_dir(cache.path()).unwrap().count(),
        0,
        "nothing may be written before the flag is validated"
    );
}
