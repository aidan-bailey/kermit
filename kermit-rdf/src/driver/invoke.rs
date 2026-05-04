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
        // Heavy recipe: tmpfs `/usr`, then re-bind the bits we still need
        // (`/usr/bin`, `/usr/lib`, …) and `--dir /usr/share/dict` to give
        // ourselves a writable parent. This works even on hosts where
        // `/usr/share/` doesn't exist (NixOS) — the simpler
        // `--bind <words> /usr/share/dict/words` recipe fails there
        // because bwrap can't `mkdir` under a host-RO `/usr/`.
        cmd.arg("--bind")
            .arg("/")
            .arg("/")
            .arg("--tmpfs")
            .arg("/usr")
            .arg("--ro-bind-try")
            .arg("/usr/bin")
            .arg("/usr/bin")
            .arg("--ro-bind-try")
            .arg("/usr/lib")
            .arg("/usr/lib")
            .arg("--ro-bind-try")
            .arg("/usr/lib64")
            .arg("/usr/lib64")
            .arg("--ro-bind-try")
            .arg("/usr/local")
            .arg("/usr/local")
            .arg("--dir")
            .arg("/usr/share/dict")
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

/// Splits a watdiv `-s` stdout dump into individual `#mapping…#end` template
/// blocks. Each returned string is one template, with the trailing `#end`
/// marker stripped (so it can be passed back to `-q` as a single-template
/// query file by re-appending `#end`).
fn split_templates(stress_stdout: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    for line in stress_stdout.lines() {
        if line.trim() == "#end" {
            if !current.trim().is_empty() {
                out.push(std::mem::take(&mut current));
            } else {
                current.clear();
            }
        } else {
            current.push_str(line);
            current.push('\n');
        }
    }
    if !current.trim().is_empty() {
        out.push(current);
    }
    out
}

/// Splits a watdiv `-q` stdout dump (concrete queries) into individual
/// SPARQL queries. Queries are separated by `#end` markers; each returned
/// string is a complete SPARQL query (no `#end`).
pub(crate) fn split_queries(query_stdout: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    for line in query_stdout.lines() {
        if line.trim() == "#end" {
            if !current.trim().is_empty() {
                out.push(std::mem::take(&mut current));
            } else {
                current.clear();
            }
        } else {
            current.push_str(line);
            current.push('\n');
        }
    }
    if !current.trim().is_empty() {
        out.push(current);
    }
    out
}

/// Runs `watdiv -s <model> <data.nt> <max-query-size> <query-count>` and
/// splits the resulting stdout into one template file per `#mapping…#end`
/// block under `<stage>/bin/Release/<stress_dir_arg>/template_NNNN.txt`.
///
/// Returns the list of written template paths in deterministic order.
pub fn run_stress(
    cfg: &InvokeConfig, stress_dir_arg: &str, data_nt: &Path, max_query_size: u32, count: u32,
) -> Result<Vec<PathBuf>, RdfError> {
    let max_q_str = max_query_size.to_string();
    let count_str = count.to_string();
    let data_arg = data_nt.to_str().unwrap();
    let mut cmd = build_command(cfg, &[
        "-s",
        cfg.model_file.to_str().unwrap(),
        data_arg,
        &max_q_str,
        &count_str,
    ])?;
    let output = cmd.output()?;
    if !output.status.success() {
        return Err(RdfError::BinaryFailed {
            status: format!("{}", output.status),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        });
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let blocks = split_templates(&stdout);
    let bin_release = cfg.stage.binary_path().parent().unwrap().to_path_buf();
    let dir = bin_release.join(stress_dir_arg);
    std::fs::create_dir_all(&dir)?;
    let mut paths = Vec::with_capacity(blocks.len());
    for (i, body) in blocks.iter().enumerate() {
        let path = dir.join(format!("template_{i:04}.txt"));
        // Re-append the `#end` marker so the file is a self-contained
        // watdiv-q query-file.
        let mut content = body.clone();
        if !content.ends_with('\n') {
            content.push('\n');
        }
        content.push_str("#end\n");
        std::fs::write(&path, content)?;
        paths.push(path);
    }
    paths.sort();
    Ok(paths)
}

/// Runs `watdiv -q <model> <template-file> <count> <recurrence>` for each
/// template, capturing stdout to `<tpl-stem>.sparql` next to the template.
/// Returns the list of `(sparql_path, desc_path)` pairs in template order.
/// `desc_path` points to a sibling `.desc` file that may not exist (the
/// vendored binary doesn't emit one); callers must check `.exists()` before
/// reading.
pub fn run_queries(
    cfg: &InvokeConfig, templates: &[PathBuf], count_per_template: u32,
) -> Result<Vec<(PathBuf, PathBuf)>, RdfError> {
    let mut out = Vec::new();
    for tpl in templates {
        let count_str = count_per_template.to_string();
        let recurrence = "1";
        let mut cmd = build_command(cfg, &[
            "-q",
            cfg.model_file.to_str().unwrap(),
            tpl.to_str().unwrap(),
            &count_str,
            recurrence,
        ])?;
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
        std::fs::write(&sparql, &output.stdout)?;
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
    #[cfg_attr(
        miri,
        ignore = "miri does not support std::fs::set_permissions on Unix"
    )]
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

    #[test]
    fn split_templates_handles_two_blocks() {
        let raw = "#mapping v0 wsdbm:City uniform\nSELECT ?v1 WHERE { %v0% gn:parentCountry ?v1 . \
                   }\n#end\n#mapping v1 wsdbm:Review uniform\nSELECT ?v0 WHERE { ?v0 \
                   rev:hasReview %v1% . }\n#end\n";
        let blocks = split_templates(raw);
        assert_eq!(blocks.len(), 2);
        assert!(blocks[0].contains("wsdbm:City"));
        assert!(blocks[1].contains("wsdbm:Review"));
        assert!(!blocks[0].contains("#end"));
    }

    #[test]
    fn split_queries_handles_multiline_select() {
        let raw = "SELECT ?v0 WHERE {\n\t?v0 <p> <o> .\n}\n#end\nSELECT ?v1 WHERE {\n\t<s> <p> \
                   ?v1 .\n}\n#end\n";
        let qs = split_queries(raw);
        assert_eq!(qs.len(), 2);
        assert!(qs[0].starts_with("SELECT ?v0"));
        assert!(qs[1].starts_with("SELECT ?v1"));
    }

    #[test]
    fn split_queries_drops_trailing_empty_block() {
        let raw = "SELECT ?v0 WHERE { ?v0 <p> <o> . }\n#end\n\n";
        let qs = split_queries(raw);
        assert_eq!(qs.len(), 1);
    }
}
