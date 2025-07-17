use {
    common::tuple_generation::{
        generate_distinct_tuples, generate_exponential_tuples, generate_factorial_tuples,
    },
    criterion::{
        BatchSize, BenchmarkGroup, Criterion, criterion_group, criterion_main,
        measurement::WallTime,
    },
    kermit_ds::{ds::{relation_trie::RelationTrie, column_trie::ColumnTrie}, relation::Relation},
    kermit_iters::trie::TrieIterable,
    num_traits::PrimInt,
    rand::distr::uniform::SampleUniform,
    std::{hash::Hash, hint::black_box},
};

mod common;

fn bench_relation_insert<R: Relation>(group: &mut BenchmarkGroup<WallTime>)
where
    R::KT: Clone + SampleUniform + PrimInt + Hash,
{
    for k in [1, 2, 3] {
        for n in [100, 1000, 10000] {
            group.throughput(criterion::Throughput::Elements(n as u64));
            group.bench_with_input(format!("Insert/Random/{k}/{n}"), &n, |b, &n| {
                b.iter_batched(
                    || generate_distinct_tuples::<R::KT>(n, k),
                    |input| {
                        black_box(R::from_tuples(input));
                    },
                    BatchSize::LargeInput,
                );
            });
        }
    }

    for k in [1, 2, 3, 4, 5] {
        let tuples = generate_exponential_tuples(num_traits::cast(k).unwrap());
        let n = tuples.len();
        group.throughput(criterion::Throughput::Elements(tuples.len() as u64));
        group.bench_with_input(
            format!("Insert/Exponential/{k}/{n}"),
            &tuples,
            |b, tuples| {
                b.iter_batched(
                    || tuples.clone(),
                    |input| {
                        black_box(R::from_tuples(input));
                    },
                    BatchSize::LargeInput,
                );
            },
        );
    }

    for h in [1, 2, 3, 4, 5, 6, 7, 8, 9] {
        let tuples = generate_factorial_tuples(num_traits::cast(h).unwrap());
        let n = tuples.len();
        group.throughput(criterion::Throughput::Elements(n as u64));
        group.bench_with_input(format!("Insert/Factorial/{h}/{n}"), &tuples, |b, tuples| {
            b.iter_batched(
                || tuples.clone(),
                |input| {
                    black_box(R::from_tuples(input));
                },
                BatchSize::LargeInput,
            );
        });
    }
}

fn bench_trie_relation_iteration<R: Relation + TrieIterable>(group: &mut BenchmarkGroup<WallTime>)
where
    R::KT: Clone + SampleUniform + PrimInt + Hash,
{
    for k in [1, 2, 3] {
        for n in [100, 1000, 10000].iter() {
            group.throughput(criterion::Throughput::Elements(*n as u64));
            group.bench_with_input(format!("Iterate/Random/{k}/{n}"), &n, |b, &n| {
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
        let tuples = generate_exponential_tuples(num_traits::cast(k).unwrap());
        let n = tuples.len();
        group.throughput(criterion::Throughput::Elements(n as u64));
        let relation = R::from_tuples(tuples);
        group.bench_with_input(
            format!("Iterate/Exponential/{k}/{n}"),
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
        let tuples = generate_factorial_tuples(num_traits::cast(h).unwrap());
        let n = tuples.len();
        group.throughput(criterion::Throughput::Elements(n as u64));
        let relation = R::from_tuples(tuples);
        group.bench_with_input(
            format!("Iterate/Factorial/{h}/{n}"),
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
fn bench_trie_relation<R: Relation + TrieIterable>(groupname: &str, c: &mut Criterion)
where
    R::KT: Clone + SampleUniform + PrimInt + Hash,
{
    // let groupname = type_name::<R>()
    // .rsplit("::")
    // .next()
    // .unwrap_or("UnknownType")
    // .to_string();
    let mut group = c.benchmark_group(groupname);
    group.sample_size(10000);
    bench_relation_insert::<R>(&mut group);
    bench_trie_relation_iteration::<R>(&mut group);
}

/*
#[macro_export]
macro_rules! define_trie_relation_benchmarks {
    (
        $(
            $relation_type:ident
        ),+
    ) => {
        paste::paste! {
            $(
                fn [<bench_ $relation_type:lower>](c: &mut Criterion) {
                    bench_trie_relation::<$relation_type<i16>>(format!("{}/i16", stringify!($relation_type)).as_str(), c);
                    bench_trie_relation::<$relation_type<i32>>(format!("{}/i32", stringify!($relation_type)).as_str(), c);
                    bench_trie_relation::<$relation_type<i64>>(format!("{}/i64", stringify!($relation_type)).as_str(), c);
                }

            )+
            criterion_group!(benches, [<bench_ $relation_type:lower>]);
        }
    };
}
*/

#[macro_export]
macro_rules! define_trie_relation_benchmarks {
    (
        $(
            $relation_type:ident
        ),+
    ) => {
        paste::paste! {
            $(
                fn [<bench_ $relation_type:lower>](c: &mut Criterion) {
                    bench_trie_relation::<$relation_type<i16>>(concat!(stringify!($relation_type), "/i16"), c);
                    bench_trie_relation::<$relation_type<i32>>(concat!(stringify!($relation_type), "/i32"), c);
                    bench_trie_relation::<$relation_type<i64>>(concat!(stringify!($relation_type), "/i64"), c);
                }
            )+

            criterion_group!(
                benches,
                $(
                    [<bench_ $relation_type:lower>]
                ),+
            );
        }
    };
}

define_trie_relation_benchmarks!(RelationTrie, ColumnTrie);

criterion_main!(benches);
