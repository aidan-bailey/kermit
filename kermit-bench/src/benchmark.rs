use {super::downloader::DownloadSpec, std::path::Path};

pub struct Task {
    pub name: &'static str,
    pub description: &'static str,
    pub location: &'static str,
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
