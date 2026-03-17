//! Benchmark infrastructure for Kermit.
//!
//! Provides synthetic data generation and benchmark workload definitions.
//! [`BenchmarkConfig`](benchmark::BenchmarkConfig) defines the interface each
//! benchmark must implement.

pub mod benchmark;
pub mod benchmarks;
pub mod generation;

#[cfg(test)]
mod tests {
    use crate::benchmarks::Benchmark;

    #[test]
    fn exponential_benchmark_generates_correct_tuple_counts() {
        let config = Benchmark::Exponential.config();
        let metadata = config.metadata();
        assert_eq!(metadata.name, "exponential");

        for task in metadata.tasks {
            for subtask in task.subtasks {
                let relations = config.generate(subtask);
                assert_eq!(relations.len(), 1);
                let (arity, tuples) = &relations[0];
                assert!(tuples.iter().all(|t| t.len() == *arity));
                assert!(!tuples.is_empty());
            }
        }
    }

    #[test]
    fn factorial_benchmark_generates_correct_tuple_counts() {
        let config = Benchmark::Factorial.config();
        let metadata = config.metadata();
        assert_eq!(metadata.name, "factorial");

        for task in metadata.tasks {
            for subtask in task.subtasks {
                let relations = config.generate(subtask);
                assert_eq!(relations.len(), 1);
                let (arity, tuples) = &relations[0];
                assert!(tuples.iter().all(|t| t.len() == *arity));
                assert!(!tuples.is_empty());
            }
        }
    }

    #[test]
    fn benchmark_names_match_variants() {
        assert_eq!(Benchmark::Exponential.name(), "exponential");
        assert_eq!(Benchmark::Factorial.name(), "factorial");
    }

    #[test]
    fn benchmark_from_name_roundtrips() {
        for name in Benchmark::names() {
            assert!(Benchmark::from_name(name).is_ok());
        }
    }
}
