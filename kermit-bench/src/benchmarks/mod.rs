use crate::benchmark::BenchmarkConfig;

pub mod oxford;

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Benchmark {
    Oxford,
}

impl Benchmark {

    pub fn from_name(name: &str) -> Result<Self, Box<dyn std::error::Error>> {
        if name == Benchmark::Oxford.name() {
            Ok(Self::Oxford)
        } else {
            Err(format!("Benchmark '{}' not found", name).into())
        }
    }

    pub fn name(self) -> String {
        self.config().metadata().name.to_string()
    }

    pub fn config(self) -> Box<dyn BenchmarkConfig + 'static> {
        match self {
            | Self::Oxford => Box::new(oxford::OxfordBenchmark),
        }
    }

}