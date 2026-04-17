/// Trait for calculating heap-allocated memory usage.
///
/// Returns only the heap bytes owned by the data structure (Vec backing
/// buffers, etc.), not the stack size of the struct itself.
pub trait HeapSize {
    /// Returns the total number of bytes this value owns on the heap.
    ///
    /// Implementations should use `Vec::capacity` (not `len`) to account for
    /// unused-but-reserved capacity, and should recurse into any owned
    /// heap-allocating children.
    fn heap_size_bytes(&self) -> usize;
}
