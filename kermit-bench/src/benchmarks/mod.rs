use {crate::benchmark::BenchmarkConfig, clap::ValueEnum, std::str::FromStr};

pub mod exponential;
pub mod factorial;

/// The set of available benchmarks.
#[derive(Copy, Clone, PartialEq, Eq, Debug, ValueEnum)]
pub enum Benchmark {
    Exponential,
    Factorial,
}

impl Benchmark {
    pub fn from_name(name: &str) -> Result<Self, Box<dyn std::error::Error>> {
        match name {
            | "exponential" => Ok(Self::Exponential),
            | "factorial" => Ok(Self::Factorial),
            | _ => Err(format!("Benchmark '{}' not found", name).into()),
        }
    }

    pub fn names() -> Vec<&'static str> { vec!["exponential", "factorial"] }

    pub fn name(self) -> &'static str {
        match self {
            | Self::Exponential => "exponential",
            | Self::Factorial => "factorial",
        }
    }

    pub fn config(self) -> Box<dyn BenchmarkConfig + 'static> {
        match self {
            | Self::Exponential => Box::new(exponential::ExponentialBenchmark),
            | Self::Factorial => Box::new(factorial::FactorialBenchmark),
        }
    }
}

impl FromStr for Benchmark {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::from_name(s).map_err(|e| e.to_string())
    }
}
