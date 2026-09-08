//! Runtime configuration flags for [`HashTrie`](super::HashTrie) — the
//! Config category of the optimization standard
//! (`docs/specs/optimization-standard.md`).

use {kermit_iters::ConfigOption, serde_json::Value};

/// Runtime flags read by `HashTrie` while it is built.
///
/// One type covers every configuration (unlike the Layout parameter `H`,
/// which monomorphises). The flag is read at insert time; the iterator
/// branches on the node variant it finds, never on this struct.
#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
pub struct HashTrieConfig {
    /// Singleton pruning (SIGMOD 2020 §3.3.1, Figure 5): a subtrie holding
    /// exactly one tuple is stored as that tuple instead of one hash table
    /// per remaining level. Bench axis `ds_config_singleton_pruning`.
    pub singleton_pruning: bool,
}

impl ConfigOption for HashTrieConfig {
    fn axes(&self) -> Vec<(&'static str, Value)> {
        vec![("singleton_pruning", Value::Bool(self.singleton_pruning))]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_has_pruning_off() {
        assert!(!HashTrieConfig::default().singleton_pruning);
    }

    #[test]
    fn axes_report_singleton_pruning_suffix_and_value() {
        let on = HashTrieConfig {
            singleton_pruning: true,
        };
        assert_eq!(on.axes(), vec![("singleton_pruning", Value::Bool(true))]);
        assert_eq!(
            HashTrieConfig::default().axes(),
            vec![("singleton_pruning", Value::Bool(false))]
        );
    }
}
