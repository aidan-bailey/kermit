use crate::dataset::dataset::{Dataset, DatasetMetadata};
use std::fs;
use std::path::Path;
use std::process::Command;

pub struct OxfordDatasetMetadata {}

impl DatasetMetadata for OxfordDatasetMetadata {
    fn name(&self) -> &str {
        "Oxford Dataset"
    }

    fn description(&self) -> &str {
        "Oxford Database Systems and Implementation final course exam"
    }

    fn url(&self) -> &str {
        "https://github.com/schroederdewitt/leapfrog-triejoin"
    }
}

pub struct OxfordDataset;

impl Dataset for OxfordDataset {
    fn metadata(&self) -> impl DatasetMetadata {
        OxfordDatasetMetadata {}
    }

    fn load(&self) -> Result<(), Box<dyn std::error::Error>> {
        // Create data directory if it doesn't exist
        let data_dir = Path::new("data/oxford");
        if !data_dir.exists() {
            fs::create_dir_all(data_dir)?;
        }

        // Check if dataset is already downloaded
        let dataset_path = data_dir.join("leapfrog-triejoin");
        if dataset_path.exists() {
            println!("Oxford dataset already exists at {:?}", dataset_path);
            return Ok(());
        }

        // Download the dataset using git clone
        println!("Downloading Oxford dataset...");
        let output = Command::new("git")
            .arg("clone")
            .arg("https://github.com/schroederdewitt/leapfrog-triejoin.git")
            .arg(dataset_path.to_str().unwrap())
            .output()?;

        if !output.status.success() {
            let error_msg = String::from_utf8_lossy(&output.stderr);
            return Err(format!("Failed to clone repository: {}", error_msg).into());
        }

        println!("Oxford dataset downloaded successfully to {:?}", dataset_path);
        Ok(())
    }
}
