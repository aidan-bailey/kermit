use {crate::benchmark::BenchmarkConfig, clap::ValueEnum, std::str::FromStr};

pub mod oxford;

#[derive(Copy, Clone, PartialEq, Eq, Debug, ValueEnum)]
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

    pub fn names() -> Vec<String> { vec![Self::Oxford.name()] }

    pub fn name(self) -> String { self.config().metadata().name.to_string() }

    pub fn config(self) -> Box<dyn BenchmarkConfig + 'static> {
        match self {
            | Self::Oxford => Box::new(oxford::OxfordBenchmark),
        }
    }
}

impl FromStr for Benchmark {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::from_name(s).map_err(|e| e.to_string())
    }
}
