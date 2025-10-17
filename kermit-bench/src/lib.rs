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
        std::{env, path::Path},
    };

    #[test]
    fn oxford_benchmark() {
        let tmp_dir = env::temp_dir().join("kermit-bench-testing");
        if tmp_dir.exists() {
            std::fs::remove_dir_all(&tmp_dir).expect("Failed to remove temporary directory");
        }
        std::fs::create_dir_all(&tmp_dir).expect("Failed to create temporary directory");
        let mut man = DatasetManager::new(&tmp_dir);
        assert!(man.init_dataset(OxfordBenchmark).is_ok());

        let ds_dir = tmp_dir.join(OxfordBenchmark.metadata().download_spec.name);
        assert!(ds_dir.exists());

        for task in OxfordBenchmark.metadata().tasks {
            let task_data_dir = ds_dir.join("data").join(task.dir);
            assert!(task_data_dir.exists());
            let task_query_dir = ds_dir.join("queries");
            assert!(task_query_dir.exists());
            for subtask in task.subtasks {
                let subtask_data_dir = task_data_dir.join(subtask.datadir);
                assert!(subtask_data_dir.exists())
            }
        }

        assert!(man.rm_dataset(OxfordBenchmark).is_ok());
    }
}
