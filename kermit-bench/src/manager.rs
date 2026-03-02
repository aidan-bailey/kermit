use {
    super::{benchmarks::Benchmark, downloader::Downloader},
    std::path::PathBuf,
};

/// Manages downloading, loading, and removing benchmark datasets.
///
/// Maintains a list of active benchmarks and their on-disk location.
pub struct BenchmarkManager {
    /// Root directory where benchmark datasets are stored.
    dir: PathBuf,
    /// Currently loaded benchmarks.
    datasets: Vec<Benchmark>,
}

impl BenchmarkManager {
    /// Creates a new manager rooted at the given directory, creating it if it
    /// doesn't exist.
    pub fn new<P: Into<PathBuf>>(benchmark_dir: P) -> Self {
        let path = benchmark_dir.into();
        if !path.exists() {
            std::fs::create_dir_all(&path).expect("Failed to create dataset directory");
        }
        Self {
            dir: path,
            datasets: vec![],
        }
    }

    /// Downloads, loads, and registers a benchmark. Returns an error if the
    /// benchmark is already registered or if the download/load fails.
    pub fn add_benchmark(
        &mut self, benchmark: Benchmark,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if self.datasets.iter().any(|d| d == &benchmark) {
            let name = benchmark.config().metadata().name;
            return Err(format!("Benchmark '{}' already exists in manager", name).into());
        }

        let config = benchmark.config();
        let dl_spec = &config.metadata().download_spec;
        let source = Downloader::download(dl_spec)?;
        config.load(&source, self.dir.as_path())?;
        Downloader::clean(dl_spec);
        self.datasets.push(benchmark);
        Ok(())
    }

    /// Removes a benchmark's on-disk data and deregisters it from the manager.
    pub fn rm_benchmark(&mut self, benchmark: Benchmark) -> Result<(), Box<dyn std::error::Error>> {
        let config = benchmark.config();
        let dl_spec = &config.metadata().download_spec;
        let dest = self.dir.join(dl_spec.name);
        if dest.exists() {
            std::fs::remove_dir_all(&dest)?;
        }
        self.datasets.retain(|d| d != &benchmark);
        Ok(())
    }
}
