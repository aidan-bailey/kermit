use {super::downloader::DownloadSpec, std::path::Path};

pub struct SubTask {
    pub name: &'static str,
    pub description: &'static str,
    pub data_paths: &'static [&'static str],
    pub query_paths: &'static [&'static str],
}

pub struct Task {
    pub name: &'static str,
    pub description: &'static str,
    pub subtasks: &'static [SubTask],
}

pub struct BenchmarkMetadata {
    pub name: &'static str,
    pub description: &'static str,
    pub download_spec: DownloadSpec,
    pub tasks: &'static [Task],
}

pub trait Benchmark {
    fn metadata(&self) -> &BenchmarkMetadata;
    fn load(&self, source: &Path, path: &Path) -> Result<(), Box<dyn std::error::Error>>;
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
