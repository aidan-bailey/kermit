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
}
