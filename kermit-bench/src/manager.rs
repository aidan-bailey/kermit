use {
    super::{benchmark::BenchmarkConfig, downloader::Downloader},
    std::path::PathBuf,
};

pub struct BenchmarkManager {
    dir: PathBuf,
    datasets: Vec<Box<dyn BenchmarkConfig + 'static>>,
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
        &mut self, benchmark: impl BenchmarkConfig + 'static,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if self
            .datasets
            .iter()
            .any(|d| d.metadata().name == benchmark.metadata().name)
        {
            return Err(format!(
                "Benchmark '{}' already exists in manager",
                benchmark.metadata().name
            )
            .into());
        }
        let dl_spec = &benchmark.metadata().download_spec;
        let source = Downloader::download(dl_spec)?;
        benchmark.load(&source, self.dir.as_path())?;
        Downloader::clean(dl_spec);
        self.datasets.push(Box::new(benchmark));
        Ok(())
    }

    pub fn rm_benchmark(
        &mut self, benchmark: impl BenchmarkConfig + 'static,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let dl_spec = &benchmark.metadata().download_spec;
        let dest = self.dir.join(dl_spec.name);
        if dest.exists() {
            std::fs::remove_dir_all(&dest)?;
        }
        self.datasets
            .retain(|d| d.metadata().name != benchmark.metadata().name);
        Ok(())
    }
}
