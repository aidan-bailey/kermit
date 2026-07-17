//! WatDiv binary driver: builds a `RawArtifacts` bundle by running watdiv
//! end-to-end inside a temp-dir sandbox, then leaves the bundle for the
//! pipeline orchestrator to consume.

pub mod invoke;
pub mod sandbox;

use {
    crate::error::RdfError,
    std::path::{Path, PathBuf},
};

/// Stress parameters that affect the watdiv `-s` invocation.
#[derive(Debug, Clone)]
pub struct StressParams {
    /// `<max-query-size>` in stress templates. Forwarded to watdiv `-s`.
    pub max_query_size: u32,
    /// `<query-count>` per template. Forwarded to watdiv `-s`.
    pub query_count: u32,
    /// `<constants-per-query>`. **NOT forwarded** to the current vendored
    /// binary — recorded in `meta.json` for provenance only. Will be wired
    /// up when the binary gains a matching flag.
    pub constants_per_query: u32,
    /// `<allow-join-vertex>`. **NOT forwarded** to the current vendored
    /// binary — recorded in `meta.json` for provenance only. Will be wired
    /// up when the binary gains a matching flag.
    pub allow_join_vertex: bool,
}

impl Default for StressParams {
    fn default() -> Self {
        Self {
            // Largest basic-graph-pattern size (join arity) watdiv `-s` is
            // allowed to emit per stress template.
            max_query_size: 5,
            // How many concrete queries watdiv instantiates from each stress
            // template.
            query_count: 20,
            // Number of bound constants substituted into each generated query.
            // Not forwarded to the current vendored binary (see field doc).
            constants_per_query: 2,
            // Whether generated patterns may share a join vertex. Not forwarded
            // to the current vendored binary (see field doc).
            allow_join_vertex: false,
        }
    }
}

/// Inputs to the driver.
pub struct DriverInputs<'a> {
    /// Resolved path to the watdiv binary.
    pub watdiv_bin: &'a Path,
    /// Path to the vendor `files/` dir holding firstnames/lastnames/words.
    pub vendor_files: &'a Path,
    /// Path to the model file (e.g. wsdbm-data-model.txt).
    pub model_file: &'a Path,
    /// Scale factor passed to `-d`.
    pub scale: u32,
    /// Stress parameters passed to `-s`/`-q` (currently informational; the
    /// vendored binary doesn't accept all of them as flags — this is
    /// preserved for meta.json and future binary patches).
    pub stress: StressParams,
    /// Number of concrete queries per template (passed to `-q`).
    pub query_count_per_template: u32,
    /// Wrap watdiv invocations with bwrap.
    pub use_bwrap: bool,
}

/// Outputs of the driver: paths to the raw watdiv outputs INSIDE the temp
/// stage. The caller MUST copy them out before the staging dir drops.
pub struct RawArtifacts {
    /// Path to data.nt (inside the stage).
    pub data_nt: PathBuf,
    /// One template path per stress template (inside the stage).
    pub templates: Vec<PathBuf>,
    /// (sparql_path, desc_path) tuples per template.
    pub queries: Vec<(PathBuf, PathBuf)>,
    /// The owning staging directory (kept alive so the paths above are
    /// valid).
    pub stage: sandbox::TempStagingDir,
}

/// Shared driver skeleton for the WatDiv workloads. Handles the invariant
/// steps — binary-existence check, staging-dir + [`invoke::InvokeConfig`]
/// setup, the `-d` data generation, the `-q` query generation, and the
/// [`RawArtifacts`] assembly — and delegates only the template-production
/// step (`-s` stress vs. static-template copy) to `produce_templates`.
///
/// `produce_templates` receives the invocation config, the `bin/Release`
/// directory the binary runs in, and the generated `data.nt` path, and
/// returns the template paths to feed to `-q`.
fn drive_common(
    inputs: &DriverInputs,
    produce_templates: impl FnOnce(
        &invoke::InvokeConfig,
        &Path,
        &Path,
    ) -> Result<Vec<PathBuf>, RdfError>,
) -> Result<RawArtifacts, RdfError> {
    if !inputs.watdiv_bin.exists() {
        return Err(RdfError::BinaryNotFound {
            path: inputs.watdiv_bin.to_path_buf(),
        });
    }
    let stage = sandbox::TempStagingDir::create(inputs.watdiv_bin, inputs.vendor_files)?;
    let cfg = invoke::InvokeConfig {
        stage: &stage,
        model_file: inputs.model_file,
        use_bwrap: inputs.use_bwrap,
    };

    let bin_release = stage.binary_path().parent().unwrap().to_path_buf();
    let data_nt = bin_release.join("data.nt");
    invoke::run_data(&cfg, inputs.scale, &data_nt)?;

    let templates = produce_templates(&cfg, &bin_release, &data_nt)?;
    let queries = invoke::run_queries(&cfg, &templates, inputs.query_count_per_template)?;

    Ok(RawArtifacts {
        data_nt,
        templates,
        queries,
        stage,
    })
}

/// Runs watdiv end-to-end and returns paths to the raw outputs.
pub fn drive(inputs: &DriverInputs) -> Result<RawArtifacts, RdfError> {
    drive_common(inputs, |cfg, _bin_release, data_nt| {
        let stress_arg = "stress-templates";
        invoke::run_stress(
            cfg,
            stress_arg,
            data_nt,
            inputs.stress.max_query_size,
            inputs.stress.query_count,
        )
    })
}

/// Like [`drive`], but for the **Basic Testing** workload: runs `-d`, then
/// feeds the static templates in `template_src_dir` (the vendored
/// `testsuite/*.txt`) directly to `-q`, skipping the `-s` stress step.
///
/// The templates are copied into the staging dir first so the `-q` `.sparql`
/// outputs land in the stage rather than polluting the read-only vendored
/// source. `inputs.stress` is ignored (Basic templates carry their own
/// `#mapping` lines); it is retained only so `process_artifacts` can record
/// provenance.
pub fn drive_basic(
    inputs: &DriverInputs, template_src_dir: &Path,
) -> Result<RawArtifacts, RdfError> {
    drive_common(inputs, |_cfg, bin_release, _data_nt| {
        let tpl_dir = bin_release.join("basic-templates");
        std::fs::create_dir_all(&tpl_dir)?;
        let mut sources: Vec<PathBuf> = std::fs::read_dir(template_src_dir)?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("txt"))
            .collect();
        sources.sort();
        if sources.is_empty() {
            return Err(RdfError::Sandbox(format!(
                "no .txt templates found in {template_src_dir:?}"
            )));
        }
        let mut templates = Vec::with_capacity(sources.len());
        for src in &sources {
            let dst = tpl_dir.join(src.file_name().unwrap());
            std::fs::copy(src, &dst)?;
            templates.push(dst);
        }
        Ok(templates)
    })
}
