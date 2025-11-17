use {
    kermit_algos::JoinAlgo,
    kermit_bench::{benchmarks::Benchmark, manager::BenchmarkManager},
    kermit_ds::Relation,
    std::path::PathBuf,
};

pub struct Benchmarker<R, JA>
where
    R: Relation,
    JA: JoinAlgo<R>,
{
    phantom_r: std::marker::PhantomData<R>,
    phantom_ja: std::marker::PhantomData<JA>,
    manager: BenchmarkManager,
}

impl<R, JA> Benchmarker<R, JA>
where
    R: Relation,
    JA: JoinAlgo<R>,
{
    pub fn new<P1: Into<PathBuf>>(benchmark_dir: P1) -> Self {
        Benchmarker {
            phantom_r: std::marker::PhantomData,
            phantom_ja: std::marker::PhantomData,
            manager: BenchmarkManager::new(benchmark_dir),
        }
    }

    pub fn add_benchmark(
        &mut self, benchmark: Benchmark,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.manager.add_benchmark(benchmark)
    }

    pub fn execute_benchmarks(&self) {
        todo!("Implement benchmark execution logic");
    }
}
