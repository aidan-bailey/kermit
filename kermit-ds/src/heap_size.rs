/// Trait for calculating heap-allocated memory usage.
///
/// Returns only the heap bytes owned by the data structure (Vec backing
/// buffers, etc.), not the stack size of the struct itself.
pub trait HeapSize {
    fn heap_size_bytes(&self) -> usize;
}
