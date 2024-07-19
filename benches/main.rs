use criterion::{black_box, criterion_group, criterion_main, BatchSize, BenchmarkId, Criterion};
use tuple_trie::tuple_trie::Trie;

use rand::{distributions::uniform::SampleUniform, Rng};

fn generate_vector<T: PartialOrd + SampleUniform + Copy>(
    arity: usize,
    min: T,
    max: T,
) -> Vec<T> {
    let mut rng = rand::thread_rng();
    let mut vector = Vec::<T>::new();
    for _ in 0..arity {
        vector.push(rng.gen_range(min..max));
    }
    vector
}

fn generate_tuples<T: PartialOrd + SampleUniform + Copy>(
    params: &BenchParams<T>
) -> Vec<Vec<T>> {
    let mut vectors = Vec::<Vec<T>>::new();
    while vectors.len() < params.size {
        let vector = generate_vector(params.arity, params.min, params.max);
        if !vectors.contains(&vector) {
            vectors.push(vector);
        }
    }
    vectors
}

struct BenchParams<T: PartialOrd + SampleUniform + Copy> {
    size: usize,
    arity: usize,
    min: T,
    max: T,
}

impl<T: PartialOrd + SampleUniform + Copy + std::fmt::Display> BenchParams<T> {
    fn new(size: usize, arity: usize, min: T, max: T) -> BenchParams<T> {
        BenchParams {
            size,
            arity,
            min,
            max,
        }
    }

    fn to_string(&self) -> String {
        format!("size: {}, arity: {}, min: {}, max: {}", self.size, self.arity, self.min, self.max)
    }
}

pub fn criterion_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("insertion");
    let bench_params = vec![
        BenchParams::new(2, 1000, 0, 1000000),
        BenchParams::new(4, 1000, 0, 1000000),
        BenchParams::new(8, 1000, 0, 1000000),
        BenchParams::new(16, 1000, 0, 1000000),
        BenchParams::new(32, 1000, 0, 1000000),
        BenchParams::new(64, 1000, 0, 1000000),
        BenchParams::new(128, 1000, 0, 1000000),
        BenchParams::new(256, 1000, 0, 1000000),
        BenchParams::new(512, 1000, 0, 1000000),
        BenchParams::new(1024, 1000, 0, 1000000),
        BenchParams::new(2048, 1000, 0, 1000000),
        BenchParams::new(4096, 1000, 0, 1000000),
        BenchParams::new(8192, 1000, 0, 1000000),
    ];

    for bench_param in bench_params {
        group.bench_with_input(BenchmarkId::from_parameter(bench_param.to_string()), &bench_param, |b, bench_param| {
            b.iter_batched(
                || {
                    generate_tuples(bench_param)
                },
                |tuples| black_box(Trie::from_tuples(bench_param.arity, tuples)),
                BatchSize::LargeInput,
            )
        });
    }
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
