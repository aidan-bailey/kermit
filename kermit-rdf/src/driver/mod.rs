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
    /// `<max-query-size>` in stress templates.
    pub max_query_size: u32,
    /// `<query-count>` per template.
    pub query_count: u32,
    /// `<constants-per-query>`.
    pub constants_per_query: u32,
    /// `<allow-join-vertex>`.
    pub allow_join_vertex: bool,
}

impl Default for StressParams {
    fn default() -> Self {
        Self {
            max_query_size: 5,
            query_count: 20,
            constants_per_query: 2,
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

/// Runs watdiv end-to-end and returns paths to the raw outputs.
pub fn drive(inputs: &DriverInputs) -> Result<RawArtifacts, RdfError> {
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

    let stress_arg = "stress-templates";
    std::fs::create_dir_all(bin_release.join(stress_arg))?;
    let templates = invoke::run_stress(&cfg, stress_arg, inputs.stress.query_count)?;
    let queries = invoke::run_queries(&cfg, &templates, inputs.query_count_per_template)?;

    Ok(RawArtifacts {
        data_nt,
        templates,
        queries,
        stage,
    })
}
