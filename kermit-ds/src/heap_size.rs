/// Trait for calculating heap-allocated memory usage.
///
/// Returns only the heap bytes owned by the data structure (Vec backing
/// buffers, etc.), not the stack size of the struct itself. Used by the
/// space benchmarks (`bench ds --metrics space` / `bench run --metrics
/// space`) routed through `kermit::measurement::SpaceMeasurement`.
pub trait HeapSize {
    /// Returns the total number of bytes this value owns on the heap.
    ///
    /// Implementations should use `Vec::capacity` (not `len`) and recurse
    /// into any heap-allocating children. `capacity` is the right metric
    /// because `Vec` over-allocates to amortise growth, and the benchmark
    /// goal is resident memory rather than the populated subset.
    fn heap_size_bytes(&self) -> usize;
}
