//! Constructs and executes the three watdiv invocations.

use {
    crate::{driver::sandbox::TempStagingDir, error::RdfError},
    std::{
        path::{Path, PathBuf},
        process::Command,
    },
};

/// Configuration shared across all three invocations.
pub struct InvokeConfig<'a> {
    /// Staging dir owning the binary symlink + vendored files.
    pub stage: &'a TempStagingDir,
    /// Path to the WatDiv data model file (e.g. wsdbm-data-model.txt).
    pub model_file: &'a Path,
    /// True to wrap each invocation under `bwrap` with the vendored words
    /// list bind-mounted at `/usr/share/dict/words`. False to assume the
    /// host already has that file.
    pub use_bwrap: bool,
}

fn build_command(cfg: &InvokeConfig, watdiv_args: &[&str]) -> Result<Command, RdfError> {
    let bin = cfg.stage.binary_path();
    let bin_release = bin.parent().expect("staged path has parent");
    if cfg.use_bwrap {
        let mut cmd = Command::new("bwrap");
        cmd.arg("--bind")
            .arg("/")
            .arg("/")
            .arg("--bind")
            .arg(cfg.stage.words_path())
            .arg("/usr/share/dict/words")
            .arg("--chdir")
            .arg(bin_release)
            .arg("./watdiv");
        for a in watdiv_args {
            cmd.arg(a);
        }
        Ok(cmd)
    } else {
        let mut cmd = Command::new(&bin);
        cmd.current_dir(bin_release);
        for a in watdiv_args {
            cmd.arg(a);
        }
        Ok(cmd)
    }
}

/// Runs `watdiv -d <model> <scale>`, writing N-Triples to `out_path`.
pub fn run_data(cfg: &InvokeConfig, scale: u32, out_path: &Path) -> Result<(), RdfError> {
    let scale_str = scale.to_string();
    let mut cmd = build_command(cfg, &["-d", cfg.model_file.to_str().unwrap(), &scale_str])?;
    let output = cmd.output()?;
    if !output.status.success() {
        return Err(RdfError::BinaryFailed {
            status: format!("{}", output.status),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        });
    }
    std::fs::write(out_path, &output.stdout)?;
    Ok(())
}

/// Runs `watdiv -s <stress-dir> <count>`, returning the list of generated
/// `.txt` template files (the names exactly as watdiv writes them).
///
/// `stress_dir_arg` is a SHARED working directory under the stage where
/// templates are emitted (watdiv writes them as side effects of the
/// invocation).
pub fn run_stress(
    cfg: &InvokeConfig, stress_dir_arg: &str, count: u32,
) -> Result<Vec<PathBuf>, RdfError> {
    let count_str = count.to_string();
    let mut cmd = build_command(cfg, &["-s", stress_dir_arg, &count_str])?;
    let output = cmd.output()?;
    if !output.status.success() {
        return Err(RdfError::BinaryFailed {
            status: format!("{}", output.status),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        });
    }
    let bin_release = cfg.stage.binary_path().parent().unwrap().to_path_buf();
    let dir = bin_release.join(stress_dir_arg);
    let mut templates = Vec::new();
    if dir.is_dir() {
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            let p = entry.path();
            if p.extension().and_then(|s| s.to_str()) == Some("txt") {
                templates.push(p);
            }
        }
    }
    templates.sort();
    Ok(templates)
}

/// Runs `watdiv -q <template> <count>` for each template, writing one
/// `.sparql` and one `.desc` per template (watdiv's default behavior).
/// Returns the list of `(sparql_path, desc_path)` pairs in template order.
pub fn run_queries(
    cfg: &InvokeConfig, templates: &[PathBuf], count_per_template: u32,
) -> Result<Vec<(PathBuf, PathBuf)>, RdfError> {
    let mut out = Vec::new();
    for tpl in templates {
        let count_str = count_per_template.to_string();
        let mut cmd = build_command(cfg, &["-q", tpl.to_str().unwrap(), &count_str])?;
        let output = cmd.output()?;
        if !output.status.success() {
            return Err(RdfError::BinaryFailed {
                status: format!("{}", output.status),
                stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            });
        }
        let stem = tpl.file_stem().unwrap();
        let sparql = tpl.with_file_name(format!("{}.sparql", stem.to_string_lossy()));
        let desc = tpl.with_file_name(format!("{}.desc", stem.to_string_lossy()));
        out.push((sparql, desc));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use {super::*, std::os::unix::fs::PermissionsExt};

    /// Creates a fake watdiv binary that just echoes its args to stdout
    /// and returns success. Lets us test the Command construction without
    /// the real binary.
    fn make_fake_binary(dir: &Path) -> PathBuf {
        let path = dir.join("watdiv");
        let script = "#!/bin/sh\necho fake-stdout\nexit 0\n";
        std::fs::write(&path, script).unwrap();
        let mut perms = std::fs::metadata(&path).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&path, perms).unwrap();
        path
    }

    #[test]
    fn run_data_writes_stdout_to_file() {
        let dir = tempfile::tempdir().unwrap();
        let bin = make_fake_binary(dir.path());
        let vendor = dir.path().join("vendor");
        std::fs::create_dir_all(&vendor).unwrap();
        for n in ["firstnames.txt", "lastnames.txt", "words"] {
            std::fs::write(vendor.join(n), b"x\n").unwrap();
        }
        let stage = TempStagingDir::create(&bin, &vendor).unwrap();
        let model = dir.path().join("MODEL.txt");
        std::fs::write(&model, b"model").unwrap();
        let cfg = InvokeConfig {
            stage: &stage,
            model_file: &model,
            use_bwrap: false,
        };
        let out = dir.path().join("data.nt");
        run_data(&cfg, 1, &out).unwrap();
        let s = std::fs::read_to_string(&out).unwrap();
        assert!(s.contains("fake-stdout"));
    }
}
