use {super::dataset::DatasetTrait, std::path::PathBuf};

pub struct DatasetManager {
    dir: PathBuf,
    datasets: Vec<Box<dyn DatasetTrait>>,
}

impl DatasetManager {
    pub fn new<P: Into<PathBuf>>(dataset_dir: P) -> Self {
        Self {
            dir: dataset_dir.into(),
            datasets: vec![],
        }
    }

    pub fn init_dataset(
        &mut self, dataset: impl DatasetTrait + 'static,
    ) -> Result<(), Box<dyn std::error::Error>> {
        dataset.load(self.dir.as_path())?;
        self.datasets.push(Box::new(dataset));
        Ok(())
    }
}
