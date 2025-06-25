use {
    criterion::{
        criterion_group, criterion_main, measurement::WallTime, BatchSize, BenchmarkGroup,
        Criterion,
    },
    kermit_ds::{ds::relation_trie::RelationTrie, relation::Relation},
    kermit_iters::trie::TrieIterable,
    num_traits::PrimInt,
    rand::{
        distr::{uniform::SampleUniform, Uniform},
        rng, Rng,
    },
    std::{any::type_name, collections::HashSet, hash::Hash, hint::black_box},
};

pub fn generate_all_tuples<T>(k: T) -> Vec<Vec<T>>
where
    T: PrimInt + num_traits::NumCast,
{
    let k_usize = num_traits::cast::<T, usize>(k).expect("Failed to cast T to usize");
    let mut tuples = Vec::with_capacity((k_usize + 1).pow(3));

    for i in 0..k_usize {
        for j in 0..k_usize {
            for l in 0..k_usize {
                tuples.push(vec![
                    num_traits::cast::<usize, T>(i).unwrap(),
                    num_traits::cast::<usize, T>(j).unwrap(),
                    num_traits::cast::<usize, T>(l).unwrap(),
                ]);
            }
        }
    }

    tuples
}

pub fn generate_factorial_tuple_trie<T>(h: T) -> Vec<Vec<T>>
where
    T: PrimInt + num_traits::NumCast,
{
    let h_usize = num_traits::cast::<T, usize>(h).expect("Failed to cast T to usize");
    let mut tuples: Vec<Vec<T>> = vec![];

    // build Vec<T> with h_usize elements, all set to 0
    let tuple = (0..h_usize).map(|_| num_traits::cast::<usize, T>(0).unwrap()).collect::<Vec<T>>();

    fn recurse<T>(h_curr: usize, h: usize, current: Vec<T>, result: &mut Vec<Vec<T>>) 
    where
        T: PrimInt + num_traits::NumCast,
    {
        if h_curr == h {
            result.push(current);
            return;
        }

        for i in 0..=h_curr {
            let mut new_tuple = current.clone();
            new_tuple.push(num_traits::cast::<usize, T>(i).unwrap());
            recurse(h_curr + 1, h, new_tuple, result);
        }
    }

    recurse(0, h_usize, tuple, &mut tuples);

    tuples
}

pub fn generate_distinct_tuples<T>(n: usize, k: usize) -> Vec<Vec<T>>
where
    T: PrimInt + SampleUniform + Hash,
{
    let mut set = HashSet::new();
    let mut rng = rng();
    let dist = Uniform::new(T::min_value(), T::max_value()).ok().unwrap();

    while set.len() < n {
        let tuple: Vec<T> = (0..k).map(|_| rng.sample(&dist)).collect();
        set.insert(tuple);
    }

    set.into_iter().collect()
}

// Benchmark just the insertion (as before)
fn bench_relation_insert<R: Relation>(group: &mut BenchmarkGroup<WallTime>)
where
    R::KT: Clone + SampleUniform + PrimInt + Hash,
{
    for k in [1, 2, 3] {
        for n in [100, 1000, 10000] {
            group.throughput(criterion::Throughput::Elements(n as u64));
            group.bench_with_input(format!("Insert/Random/{}/{}", k, n), &n, |b, &n| {
                b.iter_batched(
                    || generate_distinct_tuples::<R::KT>(n, 3),
                    |input| {
                        let relation = R::from_tuples(input);
                        black_box(relation);
                    },
                    BatchSize::LargeInput,
                );
            });
        }
    }

    for k in [1, 2, 3, 4, 5] {
        let tuples = generate_all_tuples(num_traits::cast(k).unwrap());
        let n = tuples.len();
        group.throughput(criterion::Throughput::Elements(tuples.len() as u64));
        group.bench_with_input(
            format!("Insert/Exponential/{}/{}", k, n),
            &tuples,
            |b, tuples| {
                b.iter_batched(
                    || tuples.clone(),
                    |input| {
                        let relation = R::from_tuples(input);
                        black_box(relation);
                    },
                    BatchSize::LargeInput,
                );
            },
        );
    }

    for h in [1, 2, 3, 4, 5, 6, 7, 8, 9] {
        let tuples = generate_factorial_tuple_trie(num_traits::cast(h).unwrap());
        let n = tuples.len();
        group.throughput(criterion::Throughput::Elements(n as u64));
        group.bench_with_input(
            format!("Insert/Factorial/{}/{}", h, n),
            &tuples,
            |b, tuples| {
                b.iter_batched(
                    || tuples.clone(),
                    |input| {
                        let relation = R::from_tuples(input);
                        black_box(relation);
                    },
                    BatchSize::LargeInput,
                );
            },
        );
    }

}

fn bench_trie_relation_iteration<R: Relation + TrieIterable>(group: &mut BenchmarkGroup<WallTime>)
where
    R::KT: Clone + SampleUniform + PrimInt + Hash,
{
    for k in [1, 2, 3] {
        for n in [100, 1000, 10000].iter() {
            group.throughput(criterion::Throughput::Elements(*n as u64));
            group.bench_with_input(format!("Iterate/Random/{}/{}", k, n), &n, |b, &n| {
                b.iter_batched(
                    || R::from_tuples(generate_distinct_tuples(*n, k)),
                    |relation| {
                        for tuple in relation.trie_iter() {
                            black_box(tuple);
                        }
                    },
                    BatchSize::LargeInput,
                );
            });
        }
    }

    for k in [1, 2, 3, 4, 5] {
        let tuples = generate_all_tuples(num_traits::cast(k).unwrap());
        let n = tuples.len();
        group.throughput(criterion::Throughput::Elements(n as u64));
        let relation = R::from_tuples(tuples);
        group.bench_with_input(
            format!("Iterate/Exponential/{}/{}", k, n),
            &relation,
            |b, relation| {
                b.iter_batched(
                    || &relation,
                    |relation| {
                        for tuple in relation.trie_iter() {
                            black_box(tuple);
                        }
                    },
                    BatchSize::LargeInput,
                );
            },
        );
    }

    for h in [1, 2, 3, 4, 5, 6, 7, 8, 9] {
        let tuples = generate_factorial_tuple_trie(num_traits::cast(h).unwrap());
        let n = tuples.len();
        group.throughput(criterion::Throughput::Elements(n as u64));
        let relation = R::from_tuples(tuples);
        group.bench_with_input(
            format!("Iterate/Factorial/{}/{}", h, n),
            &relation,
            |b, relation| {
                b.iter_batched(
                    || &relation,
                    |relation| {
                        for tuple in relation.trie_iter() {
                            black_box(tuple);
                        }
                    },
                    BatchSize::LargeInput,
                );
            },
        );
    }
}

// Tie together construction and separate benchmarks
fn bench_trie_relation<R: Relation + TrieIterable>(c: &mut Criterion)
where
    R::KT: Clone + SampleUniform + PrimInt + Hash,
{
    let groupname = type_name::<R>()
        .rsplit("::")
        .next()
        .unwrap_or("UnknownType")
        .to_string();
    let mut group = c.benchmark_group(groupname);
    group.sample_size(1000);
    bench_relation_insert::<R>(&mut group);
    bench_trie_relation_iteration::<R>(&mut group);
}

fn bench_relation_trie(c: &mut Criterion) { bench_trie_relation::<RelationTrie<u16>>(c); }

criterion_group!(benches, bench_relation_trie,);
criterion_main!(benches);
