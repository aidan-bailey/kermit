use criterion::{black_box, criterion_group, criterion_main, BatchSize, BenchmarkId, Criterion};
use kermit_algos::leapfrog_triejoin::leapfrog_triejoin;
use kermit_ds::relational_trie::relational_trie::RelationalTrie;
use kermit_iters::trie::{TrieIterable, TrieIterator};

use rand::{distributions::uniform::SampleUniform, Rng};

fn generate_vector<T: PartialOrd + SampleUniform + Copy>(arity: usize, min: T, max: T) -> Vec<T> {
    let mut rng = rand::thread_rng();
    let mut vector = Vec::<T>::new();
    for _ in 0..arity {
        vector.push(rng.gen_range(min..max));
    }
    vector
}

fn generate_tuples<T: PartialOrd + SampleUniform + Copy>(params: &BenchParams<T>) -> Vec<Vec<T>> {
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
        format!(
            "size: {}, arity: {}, min: {}, max: {}",
            self.size, self.arity, self.min, self.max
        )
    }
}

pub fn criterion_benchmark(c: &mut Criterion) {
    let bench_params = vec![
        BenchParams::new(1, 3, i32::MIN, i32::MAX),
        BenchParams::new(2, 3, i32::MIN, i32::MAX),
        BenchParams::new(4, 3, i32::MIN, i32::MAX),
        BenchParams::new(8, 3, i32::MIN, i32::MAX),
        BenchParams::new(16, 3, i32::MIN, i32::MAX),
        BenchParams::new(32, 3, i32::MIN, i32::MAX),
        BenchParams::new(64, 3, i32::MIN, i32::MAX),
        BenchParams::new(128, 3, i32::MIN, i32::MAX),
        BenchParams::new(256, 3, i32::MIN, i32::MAX),
        BenchParams::new(512, 3, i32::MIN, i32::MAX),
    ];

    let mut insertion_group = c.benchmark_group("identical tries");
    //insertion_group.sampling_mode(criterion::SamplingMode::Flat);
    //insertion_group.sample_size(10);

    for bench_param in &bench_params {
        insertion_group.bench_with_input(
            BenchmarkId::from_parameter(bench_param.to_string()),
            &bench_param,
            |b, bench_param| {
                b.iter_batched(
                    || {
                        let trie = RelationalTrie::from_tuples_presort(
                            bench_param.arity,
                            generate_tuples(bench_param),
                        );
                        vec![trie.clone(), trie.clone()]
                    },
                    |tries| {
                        let input = tries.iter().collect::<Vec<_>>();
                        black_box(leapfrog_triejoin(input))
                    },
                    BatchSize::PerIteration,
                )
            },
        );
    }
    insertion_group.finish();
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
