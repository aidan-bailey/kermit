use {
    super::implementation::*,
    crate::{
        relation::{Projectable, Relation},
        BinarySeek, GallopingSeek, HeapSize, LinearSeek, SeekStrategy,
    },
    kermit_iters::{HasOptimizationAxes, LinearIterator, TrieIterable, TrieIterator},
};

#[test]
fn trie_insert() {
    let mut trie: TreeTrie = TreeTrie::new(2.into());

    trie.insert(vec![1, 2]);

    assert_eq!(trie.children().len(), 1);
    // check first level child
    let child = &trie.children()[0];
    assert_eq!(child.key(), 1);
    assert_eq!(child.children().len(), 1);
    // check second level child
    let child = &child.children()[0];
    assert_eq!(child.key(), 2);
    assert_eq!(child.children().len(), 0);

    trie.insert(vec![0, 2]);

    assert_eq!(trie.children().len(), 2);
    // check first level child
    let child = &trie.children()[0];
    assert_eq!(child.key(), 0);
    assert_eq!(child.children().len(), 1);
    // check second level child
    let child = &child.children()[0];
    assert_eq!(child.key(), 2);
    assert_eq!(child.children().len(), 0);

    trie.insert(vec![1, 1]);

    assert_eq!(trie.children().len(), 2);
    // check first level child
    let child = &trie.children()[1];
    assert_eq!(child.key(), 1);
    assert_eq!(child.children().len(), 2);
    // check second level child
    let child = &child.children()[0];
    assert_eq!(child.key(), 1);
    assert_eq!(child.children().len(), 0);
}

#[test]
fn linear_iterator() {
    let trie: TreeTrie =
        TreeTrie::from_tuples(1.into(), vec![vec![1], vec![2], vec![3], vec![4], vec![5]]);
    let mut iter = trie.trie_iter();
    assert!(iter.key().is_none());
    assert!(iter.open());
    assert_eq!(iter.key(), Some(1));
    assert_eq!(iter.next(), Some(2));
    assert!(iter.seek(4));
    assert_eq!(iter.key(), Some(4));
    assert_eq!(iter.next(), Some(5));
}

#[test]
fn test_tree_trie() {
    let trie: TreeTrie = TreeTrie::from_tuples(2.into(), vec![vec![2, 4], vec![3, 5]]);
    let mut iter = trie.trie_iter();

    assert!(iter.open());
    assert_eq!(iter.key(), Some(2));
    assert!(iter.open());
    assert_eq!(iter.key(), Some(4));

    assert!(iter.up());
    assert_eq!(iter.key(), Some(2));
    assert_eq!(iter.next(), Some(3));
    assert!(iter.open());
    assert_eq!(iter.key(), Some(5));
}

#[test]
fn trie_iterator() {
    let trie: TreeTrie = TreeTrie::from_tuples(3.into(), vec![
        vec![1, 3, 4],
        vec![1, 3, 5],
        vec![1, 4, 6],
        vec![1, 4, 8],
        vec![1, 4, 9],
        vec![1, 5, 2],
        vec![3, 5, 2],
    ]);
    let mut iter = trie.trie_iter();

    assert!(iter.open());
    assert_eq!(iter.key().unwrap(), 1);

    assert!(iter.open());
    assert_eq!(iter.key().unwrap(), 3);

    assert!(iter.open());
    assert_eq!(iter.key().unwrap(), 4);

    assert_eq!(iter.next().unwrap(), 5);

    assert!(iter.up());
    assert_eq!(iter.key().unwrap(), 3);

    assert_eq!(iter.next().unwrap(), 4);

    assert!(iter.open());
    assert_eq!(iter.key().unwrap(), 6);

    assert!(iter.seek(9));
    assert!(iter.up());
    assert_eq!(iter.key().unwrap(), 4);
    assert!(iter.up());
    assert_eq!(iter.key().unwrap(), 1);
    assert_eq!(iter.next().unwrap(), 3);

    assert!(iter.open());
    assert_eq!(iter.key().unwrap(), 5);

    assert!(iter.open());
    assert_eq!(iter.key().unwrap(), 2);

    assert!(!iter.open());
}

/// Seeks backward on a `TreeTrie<S>`: must panic whatever `S` is, because
/// the check precedes the strategy.
fn seek_backward<S: SeekStrategy>() {
    let trie: TreeTrie<S> = TreeTrie::from_tuples(1.into(), vec![vec![1], vec![3], vec![5]]);
    let mut iter = trie.trie_iter();
    iter.open();
    iter.seek(3);
    iter.seek(1); // should panic — seeking backward
}

#[test]
#[should_panic(expected = "seek_key must be ≥ the key at the current position")]
fn seek_backward_panics_linear() { seek_backward::<LinearSeek>() }

#[test]
#[should_panic(expected = "seek_key must be ≥ the key at the current position")]
fn seek_backward_panics_binary() { seek_backward::<BinarySeek>() }

#[test]
#[should_panic(expected = "seek_key must be ≥ the key at the current position")]
fn seek_backward_panics_galloping() { seek_backward::<GallopingSeek>() }

/// A successful seek moves the stack top; one that runs off the end leaves
/// it on the last node the iterator was positioned on, so `open` still
/// descends into *that* node's children after `at_end`: the descent LFTJ
/// relies on (see `TreeTrieIter`'s position model). The strategy only
/// computes the offset, so this holds under each of them.
fn open_after_failed_seek<S: SeekStrategy>() {
    let trie: TreeTrie<S> =
        TreeTrie::from_tuples(2.into(), vec![vec![1, 5], vec![2, 6], vec![3, 7]]);
    let mut iter = trie.trie_iter();
    assert!(iter.open());
    assert!(iter.seek(2));
    assert!(!iter.seek(100));
    assert!(iter.at_end());
    assert!(iter.open());
    assert_eq!(iter.key(), Some(6));
}

#[test]
fn open_after_failed_seek_descends_from_last_positioned_node() {
    open_after_failed_seek::<LinearSeek>();
    open_after_failed_seek::<BinarySeek>();
    open_after_failed_seek::<GallopingSeek>();
}

#[test]
fn test_tree_trie_iter() {
    let trie: TreeTrie =
        TreeTrie::from_tuples(2.into(), vec![vec![1, 2], vec![1, 3], vec![2, 4], vec![
            3, 5,
        ]]);
    let iter = trie.trie_iter();
    for v in iter {
        assert!(
            !v.is_empty(),
            "Each iteration should yield a non-empty vector."
        );
    }
}

#[test]
fn test_project() {
    let trie: TreeTrie =
        TreeTrie::from_tuples(3.into(), vec![vec![1, 2, 3], vec![4, 5, 6], vec![7, 8, 9]]);

    // Project to columns 0 and 2 (first and third columns)
    let projected = trie.project(vec![0, 2]);
    assert_eq!(projected.header().arity(), 2);

    // Collect all tuples from the projected relation using iterator
    let mut all_tuples: Vec<Vec<usize>> = projected.trie_iter().into_iter().collect();

    // Sort for comparison
    all_tuples.sort();
    assert_eq!(all_tuples, vec![vec![1, 3], vec![4, 6], vec![7, 9]]);
}

#[test]
fn test_project_with_named_attributes() {
    // Create a relation with named attributes
    let header = crate::relation::RelationHeader::new_nameless(vec![
        "x".to_string(),
        "y".to_string(),
        "z".to_string(),
    ]);
    let trie: TreeTrie = TreeTrie::from_tuples(header, vec![vec![1, 2, 3], vec![4, 5, 6]]);

    // Project to columns 0 and 2 (first and third columns)
    let projected = trie.project(vec![0, 2]);
    assert_eq!(projected.header().arity(), 2);
    assert_eq!(projected.header().attrs(), &[
        "x".to_string(),
        "z".to_string()
    ]);

    // Collect all tuples from the projected relation using iterator
    let mut all_tuples: Vec<Vec<usize>> = projected.trie_iter().into_iter().collect();

    // Sort for comparison
    all_tuples.sort();
    assert_eq!(all_tuples, vec![vec![1, 3], vec![4, 6]]);
}

/// The seek strategy is a type-level choice: it adds no field to the trie
/// or its iterator and no heap, so no instantiation pays for the others
/// (the non-user-tax test of `docs/specs/optimization-standard.md`).
#[test]
fn seek_strategy_adds_no_state() {
    use std::mem::{size_of, size_of_val};
    let tuples = vec![vec![1, 2], vec![1, 3], vec![2, 4]];
    let linear: TreeTrie<LinearSeek> = TreeTrie::from_tuples(2.into(), tuples.clone());
    let binary: TreeTrie<BinarySeek> = TreeTrie::from_tuples(2.into(), tuples.clone());
    let galloping: TreeTrie<GallopingSeek> = TreeTrie::from_tuples(2.into(), tuples);
    assert_eq!(
        size_of::<TreeTrie<LinearSeek>>(),
        size_of::<TreeTrie<BinarySeek>>()
    );
    assert_eq!(
        size_of::<TreeTrie<GallopingSeek>>(),
        size_of::<TreeTrie<BinarySeek>>()
    );
    assert_eq!(
        size_of_val(&linear.trie_iter()),
        size_of_val(&binary.trie_iter())
    );
    assert_eq!(
        size_of_val(&galloping.trie_iter()),
        size_of_val(&binary.trie_iter())
    );
    assert_eq!(linear.heap_size_bytes(), binary.heap_size_bytes());
    assert_eq!(galloping.heap_size_bytes(), binary.heap_size_bytes());
}

/// The relation reports its strategy as the `ds_layout_seek` axis, from its
/// type.
#[test]
fn optimization_axes_name_the_seek_strategy() {
    fn seek_axis<S: SeekStrategy>() -> serde_json::Value {
        TreeTrie::<S>::new(1.into()).optimization_axes()["ds_layout_seek"].clone()
    }
    assert_eq!(seek_axis::<LinearSeek>(), "linear");
    assert_eq!(seek_axis::<BinarySeek>(), "binary");
    assert_eq!(seek_axis::<GallopingSeek>(), "galloping");
}
