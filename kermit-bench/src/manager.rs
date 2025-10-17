use {
    super::{benchmark::Benchmark, downloader::Downloader},
    std::path::PathBuf,
};

pub struct DatasetManager {
    // Directory where datasets are stored
    dir: PathBuf,
    datasets: Vec<Box<dyn Benchmark + 'static>>,
}

impl DatasetManager {
    pub fn new<P: Into<PathBuf>>(dataset_dir: P) -> Self {
        let path = dataset_dir.into();
        if !path.exists() {
            std::fs::create_dir_all(&path).expect("Failed to create dataset directory");
        }
        Self {
            dir: path,
            datasets: vec![],
        }
    }

    pub fn init_dataset(
        &mut self, dataset: impl Benchmark + 'static,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let dl_spec = &dataset.metadata().download_spec;
        let source = Downloader::download(dl_spec)?;
        dataset.load(&source, self.dir.as_path())?;
        Downloader::clean(dl_spec);
        self.datasets.push(Box::new(dataset));
        Ok(())
    }

    pub fn rm_dataset(
        &mut self, dataset: impl Benchmark + 'static,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let dl_spec = &dataset.metadata().download_spec;
        let dest = self.dir.join(dl_spec.name);
        if dest.exists() {
            std::fs::remove_dir_all(&dest)?;
        }
        self.datasets
            .retain(|d| d.metadata().name != dataset.metadata().name);
        Ok(())
    }
}
