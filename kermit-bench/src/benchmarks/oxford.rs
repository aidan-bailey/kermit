use {
    crate::{
        benchmark::{Benchmark, BenchmarkMetadata, Task},
        downloader::{DownloadMethod, DownloadSpec, Downloader},
        utils,
    },
    std::path::Path,
};

pub struct OxfordBenchmark;

static METADATA: BenchmarkMetadata = BenchmarkMetadata {
    name: "Oxford Dataset",
    description: "Oxford Database Systems and Implementation final course exam",
    download_spec: DownloadSpec {
        name: "oxford_dataset",
        method: DownloadMethod::CLONE,
        url: "https://github.com/schroederdewitt/leapfrog-triejoin",
    },
    tasks: &[
        Task {
            name: "Uniform",
            description: "Uniformly distributed data",
            location: "dataset1-uniform",
        },
        Task {
            name: "Zipf",
            description: "Zipf distributed data",
            location: "dataset2-zipf",
        },
    ],
};

fn translate_dataset(source: &Path, dest: &Path) {
    // Get header names
    let header_path = source.join("databasefile");
    let headers: Vec<String> = std::fs::read_to_string(&header_path)
        .unwrap()
        .lines()
        .map(|s| s.to_string())
        .collect();

    for header in headers {
        let header_content: Vec<String> = header.split(",").map(|s| s.trim().to_string()).collect();

        let rel_file_name = &header_content[0];
        let rel_name = &header_content[1];
        let rel_attrs: Vec<String> = header_content[2..]
            .iter()
            .map(|s| s.trim().to_string())
            .filter(|s| *s != "")
            .collect();

        let data_path = source.join(rel_file_name);

        let data_content: Vec<Vec<usize>> = std::fs::read_to_string(data_path)
            .unwrap()
            .trim()
            .lines()
            .map(|line| {
                line.split(",")
                    .take(rel_attrs.len())
                    .map(|s| s.trim().parse::<usize>().unwrap())
                    .collect()
            })
            .collect();

        utils::write_relation_to_parquet(
            &dest.join(format!("{}.parquet", rel_name)),
            &rel_attrs,
            &data_content,
        )
        .unwrap();
    }
}

fn translate_query(source: &Path, dest: &Path) {
    let query_content: Vec<_> = std::fs::read_to_string(source)
        .unwrap()
        .lines()
        .map(|s| s.to_string())
        .collect();
    let rels = query_content[0]
        .split(",")
        .filter(|s| *s != "")
        .map(|s| s.trim())
        .collect::<Vec<_>>();
    let attrs = query_content[1]
        .split(",")
        .filter(|s| *s != "")
        .map(|s| s.trim())
        .collect::<Vec<_>>();
    // Write files
    let mut file = std::fs::File::create(dest).unwrap();
    use std::io::Write;
    writeln!(file, "{}", rels.join(",")).unwrap();
    writeln!(file, "{}", attrs.join(",")).unwrap();
}

impl Benchmark for OxfordBenchmark {
    fn metadata(&self) -> &BenchmarkMetadata { &METADATA }

    fn load(&self, source: &Path, path: &Path) -> Result<(), Box<dyn std::error::Error>> {
        let dl_spec = &self.metadata().download_spec;
        let data_dest = path.join(dl_spec.name).join("data");
        let ds_tmp_path = source.join("datasets");
        for dataset_parent in ["dataset1-uniform", "dataset2-zipf"] {
            for dataset_sub in ["scale1", "scale2", "scale3", "scale4", "scale5", "scale6"] {
                let source_path = ds_tmp_path.join(dataset_parent).join(dataset_sub);
                let dest_path = data_dest.join(dataset_parent).join(dataset_sub);
                if !dest_path.exists() {
                    std::fs::create_dir_all(&dest_path)?;
                }
                translate_dataset(&source_path, &dest_path);
            }
        }
        let queries_dest = path.join(dl_spec.name).join("queries");
        if !queries_dest.exists() {
            std::fs::create_dir_all(&queries_dest)?;
        }
        for query_file in ["query1", "query2", "query3"] {
            let source_path = ds_tmp_path.join(query_file);
            let dest_path = queries_dest.join(format!("{}.txt", query_file));
            translate_query(&source_path, &dest_path);
        }
        Ok(())
    }
}
