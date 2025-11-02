use crate::benchmark::BenchmarkConfig;

pub mod oxford;

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Benchmark {
    Oxford,
}

impl Benchmark {

    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            | "oxford" => Some(Self::Oxford),
            | _ => None,
        }
    }

    pub fn config(self) -> Box<dyn BenchmarkConfig + 'static> {
        match self {
            | Self::Oxford => Box::new(oxford::OxfordBenchmark),
        }
    }

}