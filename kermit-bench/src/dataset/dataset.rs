pub trait DatasetMetadata {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn url(&self) -> &str;
}

pub trait Dataset {
    fn metadata(&self) -> impl DatasetMetadata;
    fn load(&self) -> Result<(), Box<dyn std::error::Error>>;
}
