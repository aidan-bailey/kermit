//! Which columns of an index its trie has levels for (#116).

/// Which columns of an index its trie has levels for, the rest being payload
/// kept in its leaf rows. `kermit::db::prepare` plans each index's level
/// count under the policy of the structure that builds it
/// (`Relation::TRIE_LEVELS`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TrieLevels {
    /// A level per column: every structure's trie before #116, and the
    /// sorted tries' always.
    #[default]
    All,
    /// Levels up to the index's last join-key column, a join key being a
    /// variable two or more atoms of the query carry. On a payload-last plan
    /// that is one level per join key, the trie of Freitag et al., VLDB 2020
    /// §3.1. `HashTrie` builds it under `--ds-layout-levels keys`.
    JoinKeys,
    /// Levels for the columns earlier atoms of the binary join order bind: the
    /// key each index is probed on. Planning under it also reorders the plan
    /// and the executed query's body into join order (`plan_indexes`).
    /// `HashIndex` is planned under it.
    ProbeKeys,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `bench run` keys its prepared queries by policy (a `BTreeMap`), so the
    /// declaration order is the iteration order: `ProbeKeys` is the third.
    #[test]
    fn the_policies_are_ordered_all_then_join_keys_then_probe_keys() {
        assert_eq!(TrieLevels::default(), TrieLevels::All);
        assert!(TrieLevels::All < TrieLevels::JoinKeys);
        assert!(TrieLevels::JoinKeys < TrieLevels::ProbeKeys);
    }
}
