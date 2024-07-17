pub mod trie;

#[cfg(test)]
mod tests {
    use crate::trie::Trie;

    #[test]
    fn empty_trie() {
        let empty_tri = Trie::new();
        assert!(empty_tri.is_empty());
    }
}
