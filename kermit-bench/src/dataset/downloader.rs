use std::{env, path::PathBuf};

pub struct Downloader;

pub enum DownloadMethod {
    CLONE,
}

pub struct DownloadSpec {
    pub name: &'static str,
    pub url: &'static str,
    pub method: DownloadMethod,
}

impl Downloader {
    fn tmp_dir() -> std::path::PathBuf { env::temp_dir().join("kermit") }

    fn ensure_init() {
        let tmp_dir = Self::tmp_dir();
        if !tmp_dir.exists() {
            std::fs::create_dir_all(&tmp_dir).expect("Failed to create temporary directory");
        }
    }

    pub fn cleanup() {
        let tmp_dir = Self::tmp_dir();
        if tmp_dir.exists() {
            std::fs::remove_dir_all(&tmp_dir).expect("Failed to clean up temporary directory");
        }
    }

    pub fn download(spec: &DownloadSpec) -> Result<PathBuf, Box<dyn std::error::Error>> {
        Self::ensure_init();
        let dest = Self::tmp_dir().join(spec.name);
        match spec.method {
            | DownloadMethod::CLONE => {
                let status = std::process::Command::new("git")
                    .args(["clone", spec.url, dest.to_str().unwrap()])
                    .status()?;
                if !status.success() {
                    if dest.exists() {
                        std::fs::remove_dir_all(&dest)?;
                    }
                    return Err(format!("Git clone failed with status: {}", status).into());
                }
            },
        }
        Ok(dest.to_path_buf())
    }
}
