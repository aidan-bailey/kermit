use criterion::{black_box, criterion_group, criterion_main, BatchSize, BenchmarkId, Criterion};
use kermit_ds::{iterator::TrieIter, tuple_trie::Trie};
use kermit_iters::trie::TrieIterator;

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
        BenchParams::new(1024, 3, i32::MIN, i32::MAX),
        BenchParams::new(2048, 3, i32::MIN, i32::MAX),
        BenchParams::new(4096, 3, i32::MIN, i32::MAX),
        BenchParams::new(8192, 3, i32::MIN, i32::MAX),
        BenchParams::new(16384, 3, i32::MIN, i32::MAX),
        BenchParams::new(32768, 3, i32::MIN, i32::MAX),
        BenchParams::new(65536, 3, i32::MIN, i32::MAX),
        BenchParams::new(131072, 3, i32::MIN, i32::MAX),
        BenchParams::new(262144, 3, i32::MIN, i32::MAX),
        BenchParams::new(524288, 3, i32::MIN, i32::MAX),
        BenchParams::new(1048576, 3, i32::MIN, i32::MAX),
    ];

    let mut insertion_group = c.benchmark_group("insertion");
    //insertion_group.sampling_mode(criterion::SamplingMode::Flat);
    //insertion_group.sample_size(10);
    for bench_param in &bench_params {
        insertion_group.bench_with_input(
            BenchmarkId::from_parameter(bench_param.to_string()),
            &bench_param,
            |b, bench_param| {
                b.iter_batched(
                    || generate_tuples(bench_param),
                    |tuples| black_box(Trie::from_tuples_presort(bench_param.arity, tuples)),
                    BatchSize::SmallInput,
                )
            },
        );
    }
    insertion_group.finish();

    let mut tri_iterator_group = c.benchmark_group("tri-iterator");
    //tri_iterator_group.sampling_mode(criterion::SamplingMode::Flat);
    //tri_iterator_group.sample_size(10);
    for bench_param in &bench_params {
        tri_iterator_group.bench_with_input(
            BenchmarkId::from_parameter(bench_param.to_string()),
            &bench_param,
            |b, bench_param| {
                b.iter_batched(
                    || Trie::from_tuples_presort(bench_param.arity, generate_tuples(bench_param)),
                    |trie| {
                        black_box({
                            let mut iter = TrieIter::new(&trie);
                            while iter.open().is_some() {
                                while iter.next().is_some() {
                                }
                            }
                        })
                    },
                    BatchSize::SmallInput,
                )
            },
        );
    }
    tri_iterator_group.finish();
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
