use crate::generation::graphs::GraphModel;

/// Parameters for synthetic data generation within a [`SubTask`].
pub enum GenerationParams {
    /// k-ary tuples over domain 0..k, producing k^k tuples.
    Exponential { k: usize },
    /// k-ary tuples where position d has domain 0..=d, producing k! tuples.
    Factorial { k: usize },
    /// Graph-based generation using a [`GraphModel`].
    Graph(GraphModel),
    /// Custom generation logic — the [`BenchmarkConfig`] implementation
    /// handles generation directly in its [`generate`](BenchmarkConfig::generate) method.
    Custom,
}

/// A single benchmark sub-task: a specific scale or configuration.
pub struct SubTask {
    pub name: &'static str,
    pub description: &'static str,
    pub params: GenerationParams,
}

/// A group of related benchmark sub-tasks.
pub struct Task {
    pub name: &'static str,
    pub description: &'static str,
    pub subtasks: &'static [SubTask],
}

/// Static metadata describing a benchmark workload.
pub struct BenchmarkMetadata {
    pub name: &'static str,
    pub description: &'static str,
    pub tasks: &'static [Task],
}

/// Trait that each benchmark must implement to define its metadata and
/// data generation logic.
pub trait BenchmarkConfig {
    /// Returns the static metadata for this benchmark.
    fn metadata(&self) -> &BenchmarkMetadata;

    /// Generates the relation data for a given sub-task.
    ///
    /// Returns a list of `(arity, tuples)` pairs — one per relation needed
    /// by the benchmark.
    fn generate(&self, subtask: &SubTask) -> Vec<(usize, Vec<Vec<usize>>)>;
}
