use {super::downloader::DownloadSpec, std::path::Path};

/// A single benchmark sub-task: a specific data scale or configuration within a
/// [`Task`].
pub struct SubTask {
    pub name: &'static str,
    pub description: &'static str,
    pub data_paths: &'static [&'static str],
    pub query_paths: &'static [&'static str],
}

/// A group of related benchmark sub-tasks (e.g. "Uniform" or "Zipf"
/// distribution).
pub struct Task {
    pub name: &'static str,
    pub description: &'static str,
    pub subtasks: &'static [SubTask],
}

/// Static metadata describing a benchmark: its name, download source, and task
/// hierarchy.
pub struct BenchmarkMetadata {
    pub name: &'static str,
    pub description: &'static str,
    pub download_spec: DownloadSpec,
    pub tasks: &'static [Task],
}

/// Trait that each benchmark must implement to define its metadata, loading
/// logic, and validation.
pub trait BenchmarkConfig {
    /// Returns the static metadata for this benchmark.
    fn metadata(&self) -> &BenchmarkMetadata;

    /// Loads and transforms the raw downloaded dataset from `source` into the
    /// benchmark directory at `path`.
    fn load(&self, source: &Path, path: &Path) -> Result<(), Box<dyn std::error::Error>>;
    /// Validates that all expected data and query files exist under `path`.
    /// Uses the paths declared in [`SubTask`] definitions.
    fn validate(&self, path: &Path) -> Result<(), Box<dyn std::error::Error>> {
        let metadata = self.metadata();

        for task in metadata.tasks {
            for subtask in task.subtasks {
                // Validate data paths
                for data_path in subtask.data_paths {
                    let full_data_path = path.join(metadata.download_spec.name).join(data_path);
                    if !full_data_path.exists() {
                        return Err(format!(
                            "Data path does not exist: {} (expected at: {})",
                            data_path,
                            full_data_path.display()
                        )
                        .into());
                    }
                }

                // Validate query paths
                for query_path in subtask.query_paths {
                    let full_query_path = path.join(metadata.download_spec.name).join(query_path);
                    if !full_query_path.exists() {
                        return Err(format!(
                            "Query path does not exist: {} (expected at: {})",
                            query_path,
                            full_query_path.display()
                        )
                        .into());
                    }
                }
            }
        }

        Ok(())
    }
}
