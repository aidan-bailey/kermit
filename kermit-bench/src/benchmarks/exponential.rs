use crate::{
    benchmark::{BenchmarkConfig, BenchmarkMetadata, GenerationParams, SubTask, Task},
    generation::tuples::generate_exponential_tuples,
};

pub struct ExponentialBenchmark;

static METADATA: BenchmarkMetadata = BenchmarkMetadata {
    name: "exponential",
    description: "k-ary tuples over domain 0..k, producing k^k tuples",
    tasks: &[Task {
        name: "Exponential",
        description: "Exponential growth workload",
        subtasks: &[
            SubTask {
                name: "k1",
                description: "k=1, 1 tuple",
                params: GenerationParams::Exponential { k: 1 },
            },
            SubTask {
                name: "k2",
                description: "k=2, 4 tuples",
                params: GenerationParams::Exponential { k: 2 },
            },
            SubTask {
                name: "k3",
                description: "k=3, 27 tuples",
                params: GenerationParams::Exponential { k: 3 },
            },
            SubTask {
                name: "k4",
                description: "k=4, 256 tuples",
                params: GenerationParams::Exponential { k: 4 },
            },
            SubTask {
                name: "k5",
                description: "k=5, 3125 tuples",
                params: GenerationParams::Exponential { k: 5 },
            },
        ],
    }],
};

impl BenchmarkConfig for ExponentialBenchmark {
    fn metadata(&self) -> &BenchmarkMetadata {
        &METADATA
    }

    fn generate(&self, subtask: &SubTask) -> Vec<(usize, Vec<Vec<usize>>)> {
        match subtask.params {
            | GenerationParams::Exponential { k } => {
                let tuples = generate_exponential_tuples(k);
                vec![(k, tuples)]
            },
            | _ => unreachable!("ExponentialBenchmark only uses Exponential params"),
        }
    }
}
