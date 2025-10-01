use {super::downloader::DownloadSpec, std::path::Path};

pub struct DatasetMetadata {
    name: &'static str,
    description: &'static str,
    download_spec: DownloadSpec,
}

impl DatasetMetadata {
    pub const fn new(
        name: &'static str, description: &'static str, download_spec: DownloadSpec,
    ) -> Self {
        Self {
            name,
            description,
            download_spec,
        }
    }

    pub fn name(&self) -> &'static str { self.name }

    pub fn description(&self) -> &'static str { self.description }

    pub fn download_spec(&self) -> &DownloadSpec { &self.download_spec }
}

pub trait DatasetTrait {
    fn metadata(&self) -> &DatasetMetadata;
    fn load(&self, path: &Path) -> Result<(), Box<dyn std::error::Error>>;
}
