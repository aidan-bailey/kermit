pub mod benchmark;
pub mod benchmarks;
pub mod downloader;
pub mod generation;
pub mod manager;
mod utils;
mod generator;

#[cfg(test)]
mod tests {
    use {
        crate::{benchmarks::Benchmark, manager::BenchmarkManager},
        std::path::PathBuf,
    };

    #[test]
    fn oxford_benchmark() {
        let tmp_dir = PathBuf::from("data"); // env::temp_dir().join("kermit-bench-testing");
        if tmp_dir.exists() {
            std::fs::remove_dir_all(&tmp_dir).expect("Failed to remove temporary directory");
        }
        std::fs::create_dir_all(&tmp_dir).expect("Failed to create temporary directory");
        let mut man = BenchmarkManager::new(&tmp_dir);
        assert!(man.add_benchmark(Benchmark::Oxford).is_ok());

        let ds_dir = tmp_dir.join(Benchmark::Oxford.config().metadata().download_spec.name);
        assert!(ds_dir.exists());

        // Test validation - should succeed when dataset is properly loaded
        assert!(Benchmark::Oxford.config().validate(&tmp_dir).is_ok());

        assert!(man.rm_benchmark(Benchmark::Oxford).is_ok());
    }
}
