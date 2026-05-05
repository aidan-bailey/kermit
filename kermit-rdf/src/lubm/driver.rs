//! LUBM-UBA jar driver: invokes the vendored Java jar inside a temp staging
//! dir, then gunzips the resulting `Universities.nt.gz` for downstream
//! consumption.
//!
//! The jar is invoked with `--consolidate Maximal --compress -f NTRIPLES`,
//! which (per the upstream `GlobalState.java`) routes through
//! `SingleFileConsolidator` and produces exactly one output file regardless
//! of `--threads`. See `kermit-rdf/vendor/lubm-uba/REGENERATE.md` for the
//! provenance of the vendored jar.

use {
    crate::{error::RdfError, lubm::sandbox::LubmStagingDir},
    flate2::read::GzDecoder,
    std::{
        io::{BufRead, BufReader, BufWriter, Write},
        path::{Path, PathBuf},
        process::Command,
    },
};

/// Default Univ-Bench TBox IRI used as the base URL for generated entity
/// IRIs. Stable since 2005; override only if you've mirrored the TBox to
/// a different location.
pub const DEFAULT_ONTOLOGY_IRI: &str = "http://www.lehigh.edu/~zhp2/2004/0401/univ-bench.owl";

/// Inputs to the LUBM driver.
pub struct LubmDriverInputs<'a> {
    /// Path to the vendored or user-supplied `lubm-uba.jar`.
    pub jar_path: &'a Path,
    /// Number of universities to generate (`-u`). Must be ≥ 1.
    pub scale: u32,
    /// RNG seed (`-s`). LUBM-UBA's documented default is 0; we propagate it
    /// explicitly so meta.json captures the actual value used.
    pub seed: u32,
    /// Starting university index (`-i`). Default 0.
    pub start_index: u32,
    /// Worker thread count (`-t`). Default 1 for reproducibility — UBA
    /// claims bit-identical output across thread counts but multi-threading
    /// changes file emission ordering, which we don't want for benchmark
    /// snapshots.
    pub threads: u32,
    /// Ontology IRI passed via `--ontology`. Default
    /// [`DEFAULT_ONTOLOGY_IRI`].
    pub ontology_iri: &'a str,
}

/// Outputs of the driver: paths to the gunzipped N-Triples plus the
/// staging dir that owns them. Caller MUST copy `data_nt` out before
/// `stage` drops, otherwise the file disappears.
pub struct LubmRawArtifacts {
    /// Path to the gunzipped `Universities.nt` inside the stage.
    pub data_nt: PathBuf,
    /// Echoed scale factor (mirrors meta.json fields).
    pub scale: u32,
    /// Echoed seed.
    pub seed: u32,
    /// Echoed starting index.
    pub start_index: u32,
    /// Echoed ontology IRI.
    pub ontology_iri: String,
    /// The owning staging directory (kept alive so `data_nt` is valid).
    pub stage: LubmStagingDir,
}

fn ensure_java() -> Result<(), RdfError> {
    let status = Command::new("java")
        .arg("-version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    match status {
        | Ok(s) if s.success() => Ok(()),
        | _ => Err(RdfError::JavaNotFound),
    }
}

/// Decompresses `src` line-by-line into `dst`, skipping LUBM-UBA's
/// document-self assertions whose subject is `<>` (an empty relative
/// IRI). Strict N-Triples parsers like oxttl reject `<>`, so we filter
/// at the driver stage rather than touching every consumer.
fn gunzip(src: &Path, dst: &Path) -> Result<(), RdfError> {
    let f = std::fs::File::open(src).map_err(|e| RdfError::Gunzip {
        path: src.to_path_buf(),
        message: format!("open: {e}"),
    })?;
    let decoder = GzDecoder::new(f);
    let reader = BufReader::new(decoder);
    let out = std::fs::File::create(dst).map_err(|e| RdfError::Gunzip {
        path: dst.to_path_buf(),
        message: format!("create: {e}"),
    })?;
    let mut writer = BufWriter::new(out);
    for line in reader.lines() {
        let line = line.map_err(|e| RdfError::Gunzip {
            path: src.to_path_buf(),
            message: format!("read: {e}"),
        })?;
        // Two leading triples per LUBM-UBA file describe the document
        // itself (`<> rdf:type owl:Ontology .` and `<> owl:imports …`).
        // They contribute no ABox content for the 14 queries.
        if line.starts_with("<> ") {
            continue;
        }
        writer
            .write_all(line.as_bytes())
            .map_err(|e| RdfError::Gunzip {
                path: dst.to_path_buf(),
                message: format!("write: {e}"),
            })?;
        writer.write_all(b"\n").map_err(|e| RdfError::Gunzip {
            path: dst.to_path_buf(),
            message: format!("write: {e}"),
        })?;
    }
    writer.flush().map_err(|e| RdfError::Gunzip {
        path: dst.to_path_buf(),
        message: format!("flush: {e}"),
    })?;
    Ok(())
}

/// Runs the LUBM-UBA jar end-to-end and returns paths to the gunzipped
/// N-Triples output.
pub fn drive(inputs: &LubmDriverInputs) -> Result<LubmRawArtifacts, RdfError> {
    if !inputs.jar_path.exists() {
        return Err(RdfError::LubmJarNotFound {
            path: inputs.jar_path.to_path_buf(),
        });
    }
    ensure_java()?;

    let stage = LubmStagingDir::create()?;
    let scale_str = inputs.scale.to_string();
    let seed_str = inputs.seed.to_string();
    let start_str = inputs.start_index.to_string();
    let threads_str = inputs.threads.to_string();
    let stage_root = stage.root().to_string_lossy().to_string();
    let jar_str = inputs.jar_path.to_string_lossy().to_string();

    let mut cmd = Command::new("java");
    cmd.arg("-jar")
        .arg(&jar_str)
        .arg("-u")
        .arg(&scale_str)
        .arg("-s")
        .arg(&seed_str)
        .arg("-i")
        .arg(&start_str)
        .arg("-t")
        .arg(&threads_str)
        .arg("-f")
        .arg("NTRIPLES")
        .arg("--consolidate")
        .arg("Maximal")
        .arg("--compress")
        .arg("--quiet")
        .arg("--ontology")
        .arg(inputs.ontology_iri)
        .arg("-o")
        .arg(&stage_root);
    let output = cmd.output().map_err(|e| RdfError::LubmFailed {
        status: format!("spawn: {e}"),
        stderr: String::new(),
    })?;
    if !output.status.success() {
        return Err(RdfError::LubmFailed {
            status: format!("{}", output.status),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        });
    }

    let gz = stage.compressed_output_path();
    if !gz.exists() {
        return Err(RdfError::LubmFailed {
            status: "completed but Universities.nt.gz missing".to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        });
    }
    let nt = stage.ntriples_output_path();
    gunzip(&gz, &nt)?;
    // Best-effort cleanup of the .gz; the staging dir's RAII Drop removes
    // the whole tree anyway, so a failure here is harmless.
    let _ = std::fs::remove_file(&gz);

    Ok(LubmRawArtifacts {
        data_nt: nt,
        scale: inputs.scale,
        seed: inputs.seed,
        start_index: inputs.start_index,
        ontology_iri: inputs.ontology_iri.to_string(),
        stage,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_jar_returns_lubm_jar_not_found() {
        let inputs = LubmDriverInputs {
            jar_path: Path::new("/no/such/jar.jar"),
            scale: 1,
            seed: 0,
            start_index: 0,
            threads: 1,
            ontology_iri: DEFAULT_ONTOLOGY_IRI,
        };
        // `LubmRawArtifacts` deliberately does not derive `Debug` (mirrors
        // `crate::driver::RawArtifacts`), so we can't `unwrap_err()` here.
        match drive(&inputs) {
            | Err(RdfError::LubmJarNotFound {
                ..
            }) => {},
            | Err(other) => panic!("expected LubmJarNotFound, got {other:?}"),
            | Ok(_) => panic!("expected error, drive succeeded"),
        }
    }

    #[test]
    #[cfg_attr(miri, ignore = "miri does not support std::fs::set_permissions/copy")]
    fn gunzip_roundtrips_a_known_payload() {
        use {flate2::write::GzEncoder, std::io::Write};
        let dir = tempfile::tempdir().unwrap();
        let gz_path = dir.path().join("payload.gz");
        let nt_path = dir.path().join("payload.nt");
        let payload = b"<a> <b> <c> .\n<d> <e> <f> .\n";
        {
            let f = std::fs::File::create(&gz_path).unwrap();
            let mut enc = GzEncoder::new(f, flate2::Compression::default());
            enc.write_all(payload).unwrap();
            enc.finish().unwrap().sync_all().unwrap();
        }
        gunzip(&gz_path, &nt_path).unwrap();
        let got = std::fs::read(&nt_path).unwrap();
        assert_eq!(got, payload);
    }
}
