use crate::{
    benchmark::{BenchmarkConfig, BenchmarkMetadata, GenerationParams, SubTask, Task},
    generation::tuples::generate_factorial_tuples,
};

pub struct FactorialBenchmark;

static METADATA: BenchmarkMetadata = BenchmarkMetadata {
    name: "factorial",
    description: "k-ary tuples where position d has domain 0..=d, producing k! tuples",
    tasks: &[Task {
        name: "Factorial",
        description: "Factorial growth workload",
        subtasks: &[
            SubTask {
                name: "k1",
                description: "k=1, 1 tuple",
                params: GenerationParams::Factorial {
                    k: 1,
                },
            },
            SubTask {
                name: "k2",
                description: "k=2, 2 tuples",
                params: GenerationParams::Factorial {
                    k: 2,
                },
            },
            SubTask {
                name: "k3",
                description: "k=3, 6 tuples",
                params: GenerationParams::Factorial {
                    k: 3,
                },
            },
            SubTask {
                name: "k4",
                description: "k=4, 24 tuples",
                params: GenerationParams::Factorial {
                    k: 4,
                },
            },
            SubTask {
                name: "k5",
                description: "k=5, 120 tuples",
                params: GenerationParams::Factorial {
                    k: 5,
                },
            },
            SubTask {
                name: "k6",
                description: "k=6, 720 tuples",
                params: GenerationParams::Factorial {
                    k: 6,
                },
            },
            SubTask {
                name: "k7",
                description: "k=7, 5040 tuples",
                params: GenerationParams::Factorial {
                    k: 7,
                },
            },
            SubTask {
                name: "k8",
                description: "k=8, 40320 tuples",
                params: GenerationParams::Factorial {
                    k: 8,
                },
            },
            SubTask {
                name: "k9",
                description: "k=9, 362880 tuples",
                params: GenerationParams::Factorial {
                    k: 9,
                },
            },
        ],
    }],
};

impl BenchmarkConfig for FactorialBenchmark {
    fn metadata(&self) -> &BenchmarkMetadata { &METADATA }

    fn generate(&self, subtask: &SubTask) -> Vec<(usize, Vec<Vec<usize>>)> {
        match subtask.params {
            | GenerationParams::Factorial {
                k,
            } => {
                let tuples = generate_factorial_tuples(k);
                vec![(k, tuples)]
            },
            | _ => unreachable!("FactorialBenchmark only uses Factorial params"),
        }
    }
}
