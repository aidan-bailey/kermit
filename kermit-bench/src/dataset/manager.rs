use {
    super::{dataset::DatasetTrait, downloader::Downloader},
    std::path::PathBuf,
};

pub struct DatasetManager {
    dir: PathBuf,
    datasets: Vec<Box<dyn DatasetTrait + 'static>>,
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
        &mut self, dataset: impl DatasetTrait + 'static,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let dl_spec = dataset.metadata().download_spec();
        let source = Downloader::download(dl_spec)?;
        dataset.load(&source, self.dir.as_path())?;
        Downloader::clean(dl_spec);
        self.datasets.push(Box::new(dataset));
        Ok(())
    }
}
