use {
    common::tuple_generation::{generate_exponential_tuples, generate_factorial_tuples},
    criterion::{
        criterion_group, criterion_main,
        measurement::{Measurement, ValueFormatter},
        BenchmarkGroup, Criterion, Throughput,
    },
    kermit_ds::{ColumnTrie, HeapSize, Relation, TreeTrie},
};

mod common;

// --- Custom Measurement ---

struct BytesFormatter;

impl BytesFormatter {
    fn scale(typical: f64) -> (f64, &'static str) {
        if typical < 1024.0 {
            (1.0, "B")
        } else if typical < 1024.0 * 1024.0 {
            (1.0 / 1024.0, "KiB")
        } else if typical < 1024.0 * 1024.0 * 1024.0 {
            (1.0 / (1024.0 * 1024.0), "MiB")
        } else {
            (1.0 / (1024.0 * 1024.0 * 1024.0), "GiB")
        }
    }
}

impl ValueFormatter for BytesFormatter {
    fn scale_values(&self, typical_value: f64, values: &mut [f64]) -> &'static str {
        let (factor, unit) = Self::scale(typical_value);
        for val in values {
            *val *= factor;
        }
        unit
    }

    fn scale_throughputs(
        &self, _typical_value: f64, throughput: &Throughput, values: &mut [f64],
    ) -> &'static str {
        match *throughput {
            | Throughput::Elements(elems) => {
                for val in values {
                    *val /= elems as f64;
                }
                "B/elem"
            },
            | _ => {
                // No meaningful throughput interpretation for other variants
                "B"
            },
        }
    }

    fn scale_for_machines(&self, _values: &mut [f64]) -> &'static str {
        "B"
    }
}

struct SpaceMeasurement;

impl Measurement for SpaceMeasurement {
    type Intermediate = ();
    type Value = usize;

    fn start(&self) -> Self::Intermediate {}

    fn end(&self, _i: Self::Intermediate) -> Self::Value {
        0
    }

    fn add(&self, v1: &Self::Value, v2: &Self::Value) -> Self::Value {
        v1 + v2
    }

    fn zero(&self) -> Self::Value {
        0
    }

    fn to_f64(&self, value: &Self::Value) -> f64 {
        *value as f64
    }

    fn formatter(&self) -> &dyn ValueFormatter {
        &BytesFormatter
    }
}

// --- Benchmark functions ---

fn bench_relation_space<R: Relation + HeapSize>(group: &mut BenchmarkGroup<SpaceMeasurement>) {
    for k in [1, 2, 3, 4, 5] {
        let tuples = generate_exponential_tuples(num_traits::cast(k).unwrap());
        let n = tuples.len();
        group.throughput(Throughput::Elements(n as u64));
        group.bench_function(format!("Space/Exponential/{k}/{n}"), |b| {
            b.iter_custom(|iters| {
                let mut total = 0usize;
                for _ in 0..iters {
                    let relation = R::from_tuples(k.into(), tuples.clone());
                    total += relation.heap_size_bytes();
                }
                total
            });
        });
    }

    for h in [1, 2, 3, 4, 5, 6, 7, 8, 9] {
        let tuples = generate_factorial_tuples(num_traits::cast(h).unwrap());
        let n = tuples.len();
        group.throughput(Throughput::Elements(n as u64));
        group.bench_function(format!("Space/Factorial/{h}/{n}"), |b| {
            b.iter_custom(|iters| {
                let mut total = 0usize;
                for _ in 0..iters {
                    let relation = R::from_tuples(h.into(), tuples.clone());
                    total += relation.heap_size_bytes();
                }
                total
            });
        });
    }
}

fn bench_trie_relation_space<R: Relation + HeapSize>(
    groupname: &str, c: &mut Criterion<SpaceMeasurement>,
) {
    let mut group = c.benchmark_group(groupname);
    group.sample_size(10);
    bench_relation_space::<R>(&mut group);
    group.finish();
}

// --- Macro + harness ---

macro_rules! define_space_benchmarks {
    (
        $(
            $relation_type:ident
        ),+
    ) => {
        paste::paste! {
            $(
                fn [<bench_space_ $relation_type:lower>](
                    c: &mut Criterion<SpaceMeasurement>,
                ) {
                    bench_trie_relation_space::<$relation_type>(
                        stringify!($relation_type),
                        c,
                    );
                }
            )+

            criterion_group! {
                name = space_benches;
                config = Criterion::default().with_measurement(SpaceMeasurement);
                targets = $(
                    [<bench_space_ $relation_type:lower>]
                ),+
            }
        }
    };
}

define_space_benchmarks!(TreeTrie, ColumnTrie);

criterion_main!(space_benches);
