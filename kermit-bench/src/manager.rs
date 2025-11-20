use {
    super::{benchmarks::Benchmark, downloader::Downloader},
    std::path::PathBuf,
};

pub struct BenchmarkManager {
    dir: PathBuf,
    datasets: Vec<Benchmark>,
}

impl BenchmarkManager {
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
