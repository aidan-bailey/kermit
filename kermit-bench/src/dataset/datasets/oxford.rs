use {
    crate::dataset::{
        dataset::{DatasetMetadata, DatasetTrait},
        downloader::{DownloadMethod, DownloadSpec, Downloader},
    },
    std::path::Path,
};

pub struct OxfordDataset;

static METADATA: DatasetMetadata = DatasetMetadata::new(
    "Oxford Dataset",
    "Oxford Database Systems and Implementation final course exam",
    DownloadSpec {
        name: "oxford_dataset",
        method: DownloadMethod::CLONE,
        url: "https://github.com/schroederdewitt/leapfrog-triejoin",
    },
);

impl DatasetTrait for OxfordDataset {
    fn metadata(&self) -> &DatasetMetadata { &METADATA }

    fn load(&self, path: &Path) -> Result<(), Box<dyn std::error::Error>> {
        let dest = Downloader::download(self.metadata().download_spec())?;
        Ok(())
    }
}
