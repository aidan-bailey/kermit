//! The level policy: which columns a `HashTrie` has levels for (#116).
//!
//! [`AllLevels`] (the default) gives every column a level, as before #116:
//! it reads the header's arity and never builds a trie with no level, so its
//! zero-level root is the uninhabited [`Never`] and the instantiation
//! compiles to the trie without the policy. [`KeyLevels`] gives levels to the
//! first `header.levels()` columns, the count `kermit::db::prepare` plans
//! under `TrieLevels::JoinKeys`; the rest stay in the leaf chains, whose rows
//! are whole tuples, and Hash Triejoin's leaf product reads them. Bench axis
//! `ds_layout_levels`.

use {
    super::{
        chain::Chain,
        expansion::ExpansionPolicy,
        node::HashTrieNode,
        pruning::{Never, PruningPolicy},
    },
    crate::relation::RelationHeader,
    kermit_iters::{LayoutOption, TrieLevels},
};

/// What the root of a trie with no level holds under a [`LevelPolicy`]: its
/// one chain of rows. Generic over the chain, so the public policy trait never
/// names the crate-private `Chain` (as `PendingChild<N>` keeps `HashTrieNode`
/// out of `ExpansionPolicy`).
pub trait ZeroLevelRows<C>: Sized + Send {
    /// Wrap `rows` as the root of a trie with no level.
    fn from_rows(rows: C) -> Self;
    /// The rows.
    fn rows(&self) -> &C;
    /// The rows, to append to.
    fn rows_mut(&mut self) -> &mut C;
}

impl<C> ZeroLevelRows<C> for Never {
    fn from_rows(_rows: C) -> Self {
        unreachable!("AllLevels never builds a trie with no level: its level count is the arity")
    }

    fn rows(&self) -> &C { match *self {} }

    fn rows_mut(&mut self) -> &mut C { match *self {} }
}

/// [`KeyLevels`]' zero-level root: the rows themselves.
pub struct ZeroLevel<C>(C);

impl<C: Send> ZeroLevelRows<C> for ZeroLevel<C> {
    fn from_rows(rows: C) -> Self { Self(rows) }

    fn rows(&self) -> &C { &self.0 }

    fn rows_mut(&mut self) -> &mut C { &mut self.0 }
}

/// `HashTrie`'s fourth Layout: how many leading columns its trie has levels
/// for.
pub trait LevelPolicy: LayoutOption + Copy + Default + 'static {
    /// The policy `kermit::db::prepare` plans this trie's index specs under
    /// (`Relation::TRIE_LEVELS`).
    const TRIE_LEVELS: TrieLevels;
    /// What a zero-level root holds: uninhabited under [`AllLevels`].
    type Rows<C: Send>: ZeroLevelRows<C>;
    /// How many leading columns a trie over `header` has levels for.
    fn levels(header: &RelationHeader) -> usize;
}

/// A level per column, the header's level count ignored: the trie before
/// #116, and the default.
#[derive(Copy, Clone, Debug, Default)]
pub struct AllLevels;

impl LayoutOption for AllLevels {
    const NAME: &'static str = "all";
}

impl LevelPolicy for AllLevels {
    type Rows<C: Send> = Never;

    const TRIE_LEVELS: TrieLevels = TrieLevels::All;

    fn levels(header: &RelationHeader) -> usize { header.arity() }
}

/// Levels for the header's first `levels()` columns, the join keys of a
/// payload-last plan: the trie of Freitag et al., VLDB 2020 §3.1
/// (`--ds-layout-levels keys`).
#[derive(Copy, Clone, Debug, Default)]
pub struct KeyLevels;

impl LayoutOption for KeyLevels {
    const NAME: &'static str = "keys";
}

impl LevelPolicy for KeyLevels {
    type Rows<C: Send> = ZeroLevel<C>;

    const TRIE_LEVELS: TrieLevels = TrieLevels::JoinKeys;

    fn levels(header: &RelationHeader) -> usize { header.levels() }
}

/// A trie's root: a table, when it has levels, or else its rows, one chain
/// in source order (`None` while there is no row).
pub(crate) enum Root<P: PruningPolicy, E: ExpansionPolicy, L: LevelPolicy> {
    /// The table of the first level.
    Table(HashTrieNode<P, E>),
    /// The rows of a trie with no level.
    Rows(L::Rows<Option<Chain>>),
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::{
            ds::hash_trie::{
                identity, EagerExpansion, HashTrie, HashTrieBuildMode, HashTrieConfig,
                LazyExpansion, NoPruning, RadixBits, RootCapacity, SingletonPruning,
            },
            morsel::Threads,
            relation::Relation,
        },
        kermit_iters::{SipHashStrategy, Tuples},
        std::mem::size_of,
    };

    /// Under `AllLevels` the zero-level root is uninhabited, so the root is
    /// the node: `all` pays nothing for the policy.
    #[test]
    fn root_does_not_grow_under_the_level_policy() {
        assert_eq!(
            size_of::<Root<NoPruning, EagerExpansion, AllLevels>>(),
            size_of::<HashTrieNode<NoPruning, EagerExpansion>>()
        );
        assert_eq!(
            size_of::<Root<SingletonPruning, LazyExpansion, AllLevels>>(),
            size_of::<HashTrieNode<SingletonPruning, LazyExpansion>>()
        );
    }

    type Keys<P, E> = HashTrie<SipHashStrategy, P, E, KeyLevels>;

    fn header(levels: usize) -> RelationHeader {
        RelationHeader::new_positional("r", 3).with_levels(levels)
    }

    fn built<P: PruningPolicy, E: ExpansionPolicy>(
        levels: usize, config: HashTrieConfig, mode: HashTrieBuildMode, tuples: &Tuples,
    ) -> Keys<P, E> {
        Keys::<P, E>::from_tuples_with_config_and_build_mode(
            header(levels),
            config,
            mode,
            tuples.clone(),
        )
    }

    /// `KeyLevels` over a full-depth header builds the `AllLevels` trie.
    #[test]
    fn full_depth_key_levels_build_the_all_levels_trie() {
        for (input, tuples) in identity::inputs(3) {
            let all: HashTrie<SipHashStrategy> = HashTrie::from_tuples(3.into(), tuples.clone());
            let keys: Keys<NoPruning, EagerExpansion> = HashTrie::from_tuples(3.into(), tuples);
            identity::assert_same_trie(&all, &keys, input);
        }
    }

    /// Amendment 2 at every level count: `incremental` is array-identical to
    /// `bulk`, `radix:K` and `parallel:N` have its contents, and `presized:N`
    /// is equivalent to the bulk build under its presized root.
    fn the_build_modes_agree_at_every_level_count<P: PruningPolicy, E: ExpansionPolicy>() {
        let default = HashTrieConfig::default();
        let presized = HashTrieConfig {
            root_capacity: RootCapacity::Tuples,
            ..HashTrieConfig::default()
        };
        let two = Threads::new(2).expect("2 is not zero");
        for levels in 1..=3 {
            for (input, tuples) in identity::inputs(3) {
                let label = format!("{levels} levels, {input}");
                let bulk = built::<P, E>(levels, default, HashTrieBuildMode::Bulk, &tuples);
                let incremental =
                    built::<P, E>(levels, default, HashTrieBuildMode::Incremental, &tuples);
                identity::assert_same_trie(&bulk, &incremental, &format!("{label}, incremental"));
                let radix = HashTrieBuildMode::Radix(RadixBits::new(2).unwrap());
                let radix = built::<P, E>(levels, default, radix, &tuples);
                identity::assert_same_contents(&bulk, &radix, &format!("{label}, radix:2"));
                let parallel =
                    built::<P, E>(levels, default, HashTrieBuildMode::Parallel(two), &tuples);
                identity::assert_same_contents(&bulk, &parallel, &format!("{label}, parallel:2"));
                let bulk = built::<P, E>(levels, presized, HashTrieBuildMode::Bulk, &tuples);
                let regions =
                    built::<P, E>(levels, presized, HashTrieBuildMode::Presized(two), &tuples);
                identity::assert_equivalent_trie(&bulk, &regions, &format!("{label}, presized:2"));
            }
        }
    }

    #[test]
    fn the_build_modes_agree_at_every_level_count_eager() {
        the_build_modes_agree_at_every_level_count::<NoPruning, EagerExpansion>();
    }

    #[test]
    fn the_build_modes_agree_at_every_level_count_pruned() {
        the_build_modes_agree_at_every_level_count::<SingletonPruning, EagerExpansion>();
    }

    #[test]
    fn the_build_modes_agree_at_every_level_count_lazy() {
        the_build_modes_agree_at_every_level_count::<NoPruning, LazyExpansion>();
    }

    #[test]
    fn the_build_modes_agree_at_every_level_count_pruned_lazy() {
        the_build_modes_agree_at_every_level_count::<SingletonPruning, LazyExpansion>();
    }

    /// At zero levels every mode makes the same copy, threaded in source
    /// order: array-identical to `bulk`, buffer and links included.
    #[test]
    fn every_build_mode_makes_the_same_copy_at_zero_levels() {
        let presized = HashTrieConfig {
            root_capacity: RootCapacity::Tuples,
            ..HashTrieConfig::default()
        };
        let two = Threads::new(2).expect("2 is not zero");
        for (input, tuples) in identity::inputs(3) {
            let bulk = built::<NoPruning, EagerExpansion>(
                0,
                HashTrieConfig::default(),
                HashTrieBuildMode::Bulk,
                &tuples,
            );
            assert_eq!(
                bulk.root_rows().map(|rows| rows.to_vecs()),
                Some(tuples.to_vecs()),
                "{input}: one chain, in source order"
            );
            for (mode, config) in [
                (HashTrieBuildMode::Incremental, HashTrieConfig::default()),
                (
                    HashTrieBuildMode::Radix(RadixBits::new(2).unwrap()),
                    HashTrieConfig::default(),
                ),
                (HashTrieBuildMode::Parallel(two), HashTrieConfig::default()),
                (HashTrieBuildMode::Presized(two), presized),
            ] {
                let other = built::<NoPruning, EagerExpansion>(0, config, mode, &tuples);
                identity::assert_same_trie(&bulk, &other, &format!("{input}, {mode:?}"));
            }
        }
    }

    /// `insert`, one row at a time into an empty trie, places every row as
    /// the bulk build does, at every level count.
    #[test]
    fn insert_places_rows_as_the_bulk_build_does() {
        for levels in 0..=3 {
            for (input, tuples) in identity::inputs(3) {
                let label = format!("{levels} levels, {input}");
                let bulk = built::<NoPruning, EagerExpansion>(
                    levels,
                    HashTrieConfig::default(),
                    HashTrieBuildMode::Bulk,
                    &tuples,
                );
                let mut inserted: Keys<NoPruning, EagerExpansion> = HashTrie::new(header(levels));
                for row in tuples.rows() {
                    inserted.insert(row);
                }
                assert_eq!(inserted.tuples(), bulk.tuples(), "{label}: buffer");
                assert_eq!(
                    inserted.links().to_vec(),
                    bulk.links().to_vec(),
                    "{label}: links"
                );
                match (inserted.root_table(), bulk.root_table()) {
                    | (Some(a), Some(b)) => {
                        identity::assert_same_node(inserted.links(), a, bulk.links(), b, &label);
                    },
                    | (None, None) => assert_eq!(
                        inserted.root_rows().map(|rows| rows.to_vecs()),
                        bulk.root_rows().map(|rows| rows.to_vecs()),
                        "{label}: rows"
                    ),
                    | _ => panic!("{label}: one trie has levels and the other none"),
                }
            }
        }
    }
}
