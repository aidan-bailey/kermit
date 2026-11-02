//! Build script for the `kermit` package.
//!
//! The `engine-duckdb` feature links the system `libduckdb`, which the
//! `libduckdb-sys` build script finds through `DUCKDB_LIB_DIR`. A dependency's
//! link arguments do not reach this package's executables, so this script adds
//! the rpath: a binary built in the nix dev shell then finds Nix's
//! `libduckdb.so` when it runs outside the shell, as the sweeps' binaries do.
//! With `engine-duckdb-bundled`, DuckDB is compiled into the executable and
//! there is nothing to find.

use std::env;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=DUCKDB_LIB_DIR");

    let links_system_library = env::var_os("CARGO_FEATURE_ENGINE_DUCKDB").is_some()
        && env::var_os("CARGO_FEATURE_ENGINE_DUCKDB_BUNDLED").is_none();
    if !links_system_library {
        return;
    }
    match env::var("DUCKDB_LIB_DIR") {
        | Ok(directory) if !directory.is_empty() => {
            println!("cargo:rustc-link-arg=-Wl,-rpath,{directory}");
        },
        | _ => panic!(
            "engine-duckdb needs DUCKDB_LIB_DIR (the nix dev shell sets it) or the \
             engine-duckdb-bundled feature"
        ),
    }
}
