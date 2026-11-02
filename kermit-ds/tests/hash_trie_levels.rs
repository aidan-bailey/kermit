//! `--ds-layout-levels keys` (#116) at every level count, through the public
//! API: contents, leaf chains of whole rows, where `open` stops, and the
//! zero-level root, for every build mode, pruning and expansion value.

use {
    kermit_ds::{
        EagerExpansion, ExpansionPolicy, HashTrie, HashTrieBuildMode, HashTrieConfig, KeyLevels,
        LazyExpansion, NoPruning, PruningPolicy, RadixBits, Relation, RelationHeader, RootCapacity,
        SingletonPruning, Threads,
    },
    kermit_iters::{HasOptimizationAxes, HashTrieIterable, HashTrieIterator, SipHashStrategy},
};

type Keys<P, E> = HashTrie<SipHashStrategy, P, E, KeyLevels>;

fn rows() -> Vec<Vec<usize>> {
    vec![
        vec![1, 10, 100],
        vec![1, 10, 101],
        vec![1, 11, 102],
        vec![2, 20, 103],
        vec![2, 20, 103],
        vec![3, 30, 104],
    ]
}

/// Every build mode, each under a config it accepts.
fn modes() -> Vec<(HashTrieBuildMode, HashTrieConfig)> {
    let presized = HashTrieConfig {
        root_capacity: RootCapacity::Tuples,
        ..HashTrieConfig::default()
    };
    let two = Threads::new(2).expect("2 is not zero");
    vec![
        (HashTrieBuildMode::Bulk, HashTrieConfig::default()),
        (HashTrieBuildMode::Incremental, HashTrieConfig::default()),
        (
            HashTrieBuildMode::Radix(RadixBits::new(2).unwrap()),
            HashTrieConfig::default(),
        ),
        (HashTrieBuildMode::Parallel(two), HashTrieConfig::default()),
        (HashTrieBuildMode::Presized(two), presized),
    ]
}

fn header(levels: usize) -> RelationHeader {
    RelationHeader::new_positional("r", 3).with_levels(levels)
}

fn built<P: PruningPolicy, E: ExpansionPolicy>(
    levels: usize, mode: HashTrieBuildMode, config: HashTrieConfig, rows: Vec<Vec<usize>>,
) -> Keys<P, E> {
    Keys::<P, E>::from_tuples_with_config_and_build_mode(header(levels), config, mode, rows)
}

fn sorted(mut rows: Vec<Vec<usize>>) -> Vec<Vec<usize>> {
    rows.sort();
    rows
}

fn check_every_level_count<P: PruningPolicy, E: ExpansionPolicy>() {
    for levels in 0..=3 {
        for (mode, config) in modes() {
            let label = format!("{levels} levels, {mode:?}");
            let trie = built::<P, E>(levels, mode, config, rows());
            assert_eq!(trie.levels(), levels, "{label}");

            let mut walked = Vec::new();
            trie.for_each_tuple(|row| walked.push(row.to_vec()));
            assert_eq!(sorted(walked), sorted(rows()), "{label}: contents");

            // Down the first path: `levels` opens succeed, the next fails,
            // and the leaf lends whole rows.
            let mut it = trie.hash_trie_iter();
            for depth in 0..levels {
                assert!(it.open(), "{label}: open at depth {depth}");
            }
            assert!(!it.open(), "{label}: open past the last level");
            let leaf = it.leaf_tuples().expect("at the last level");
            assert!(!leaf.is_empty(), "{label}");
            assert!(
                leaf.to_vecs().iter().all(|row| row.len() == 3),
                "{label}: whole rows"
            );
            if levels == 0 {
                assert_eq!(leaf.to_vecs(), rows(), "{label}: one chain in source order");
            }
        }
    }
}

#[test]
fn every_level_count_eager() { check_every_level_count::<NoPruning, EagerExpansion>(); }

#[test]
fn every_level_count_pruned() { check_every_level_count::<SingletonPruning, EagerExpansion>(); }

#[test]
fn every_level_count_lazy() { check_every_level_count::<NoPruning, LazyExpansion>(); }

#[test]
fn every_level_count_pruned_lazy() { check_every_level_count::<SingletonPruning, LazyExpansion>(); }

#[test]
fn an_empty_zero_level_trie_lends_an_empty_chain() {
    for (mode, config) in modes() {
        let trie = built::<NoPruning, EagerExpansion>(0, mode, config, Vec::new());
        let it = trie.hash_trie_iter();
        let leaf = it
            .leaf_tuples()
            .expect("a zero-level trie lends its chain at the pre-root");
        assert_eq!(leaf.len(), 0, "{mode:?}");
    }
}

#[test]
fn insert_into_a_zero_level_trie_appends_to_its_chain() {
    let mut trie: Keys<NoPruning, EagerExpansion> = HashTrie::new(header(0));
    for row in rows() {
        trie.insert(row);
    }
    assert_eq!(
        trie.hash_trie_iter().leaf_tuples().unwrap().to_vecs(),
        rows()
    );
}

#[test]
fn the_axis_names_the_level_policy() {
    let all: HashTrie<SipHashStrategy> = HashTrie::from_tuples(3.into(), rows());
    let keys: Keys<NoPruning, EagerExpansion> = HashTrie::from_tuples(3.into(), rows());
    assert_eq!(all.optimization_axes()["ds_layout_levels"], "all");
    assert_eq!(keys.optimization_axes()["ds_layout_levels"], "keys");
}
