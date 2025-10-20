pub mod benchmark;
pub mod benchmarks;
pub mod downloader;
pub mod manager;
mod utils;

#[cfg(test)]
mod tests {
    use {
        crate::{
            benchmark::Benchmark, benchmarks::oxford::OxfordBenchmark, manager::DatasetManager,
        },
        std::path::PathBuf,
    };

    #[test]
    fn oxford_benchmark() {
        let tmp_dir = PathBuf::from("data"); // env::temp_dir().join("kermit-bench-testing");
        if tmp_dir.exists() {
            std::fs::remove_dir_all(&tmp_dir).expect("Failed to remove temporary directory");
        }
        std::fs::create_dir_all(&tmp_dir).expect("Failed to create temporary directory");
        let mut man = DatasetManager::new(&tmp_dir);
        assert!(man.init_dataset(OxfordBenchmark).is_ok());

        let ds_dir = tmp_dir.join(OxfordBenchmark.metadata().download_spec.name);
        assert!(ds_dir.exists());

        // Test validation - should succeed when dataset is properly loaded
        assert!(OxfordBenchmark.validate(&tmp_dir).is_ok());

        // assert!(man.rm_dataset(OxfordBenchmark).is_ok());
    }
}
