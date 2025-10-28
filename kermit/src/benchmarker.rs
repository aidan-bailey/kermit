use {
    kermit_algos::JoinAlgo,
    kermit_bench::{benchmark::Benchmark, manager::BenchmarkManager},
    kermit_ds::Relation,
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
    pub fn new() -> Self {
        Benchmarker {
            phantom_r: std::marker::PhantomData,
            phantom_ja: std::marker::PhantomData,
            manager: BenchmarkManager::new("benchmarks"),
        }
    }

    pub fn add_benchmark(
        &mut self, benchmark: impl Benchmark + 'static,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.manager.add_benchmark(benchmark)
    }

    pub fn execute_benchmarks(&self) {
        todo!("Implement benchmark execution logic");
    }
}
