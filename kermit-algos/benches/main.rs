use criterion::{black_box, criterion_group, criterion_main, BatchSize, BenchmarkId, Criterion};
use kermit_ds::tuple_trie::tuple_trie::Trie;
use kermit_iters::trie::{TrieIterable, TrieIterator};

use rand::{distributions::uniform::SampleUniform, Rng};

pub fn criterion_benchmark(c: &mut Criterion) {
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
