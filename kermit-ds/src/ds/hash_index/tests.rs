use {
    super::implementation::*,
    crate::{Cardinality, Filter, HeapSize, Relation, RelationHeader, TupleView, Tuples},
    kermit_iters::{
        FxHashStrategy, HasOptimizationAxes, HashStrategy, KeyedIterable, LayoutOption, RowId,
        SipHashStrategy, TrieLevels,
    },
};

/// Test-only strategy that sends every value to hash 0, so every row of an
/// index lands in bucket 0 and only the key comparison separates the keys.
/// A twin drives the contract suite in `kermit-ds/tests/hash_index_tests.rs`;
/// neither is shared, so no crate ships a colliding strategy.
#[derive(Copy, Clone, Default, Debug)]
struct OneBucket;

impl LayoutOption for OneBucket {
    const NAME: &'static str = "one-bucket";
}

impl HashStrategy for OneBucket {
    fn hash(_key: usize) -> u64 { 0 }
}

/// Test-only strategy `hash(v) = v`, so that the bucket of a key can be
/// computed by hand and pinned.
#[derive(Copy, Clone, Default, Debug)]
struct Identity;

impl LayoutOption for Identity {
    const NAME: &'static str = "identity";
}

impl HashStrategy for Identity {
    fn hash(key: usize) -> u64 { key as u64 }
}

fn header(arity: usize, key_len: usize) -> RelationHeader {
    RelationHeader::new_nameless_positional(arity).with_levels(key_len)
}

fn build<H: HashStrategy>(arity: usize, key_len: usize, rows: Vec<Vec<usize>>) -> HashIndex<H> {
    HashIndex::from_tuples(header(arity, key_len), rows)
}

/// The row ids of bucket `bucket`'s chain, front to back.
fn chain<H: HashStrategy>(index: &HashIndex<H>, bucket: usize) -> Vec<RowId> {
    let mut ids = vec![];
    let mut at = index.directory[bucket];
    while at != NIL {
        ids.push(at);
        at = index.next[at as usize];
    }
    ids
}

fn keeps_duplicates<R: Relation>() -> bool { R::KEEPS_DUPLICATES }

fn trie_levels<R: Relation>() -> TrieLevels { R::TRIE_LEVELS }

/// `slots = max(2, rows.next_power_of_two())`, so the load factor is at most
/// 1 and the bucket shift (`64 - log2(slots)`) is at most 63.
#[test]
fn the_directory_has_the_next_power_of_two_slots_and_at_least_two() {
    for (rows, slots) in [
        (0, 2),
        (1, 2),
        (2, 2),
        (3, 4),
        (4, 4),
        (5, 8),
        (8, 8),
        (9, 16),
    ] {
        let index = build::<SipHashStrategy>(1, 1, (0..rows).map(|i| vec![i]).collect());
        assert_eq!(index.directory.len(), slots, "{rows} rows");
        assert_eq!(index.next.len(), rows, "{rows} rows");
    }
}

#[test]
fn an_index_with_no_key_columns_has_no_directory_and_lends_every_row() {
    let rows = vec![vec![3, 4], vec![1, 2], vec![3, 4]];
    let index = build::<SipHashStrategy>(2, 0, rows.clone());
    assert_eq!(index.key_len(), 0);
    assert!(index.directory.is_empty());
    assert!(index.next.is_empty());
    let lent: Vec<Vec<usize>> = index.probe(&[]).map(<[usize]>::to_vec).collect();
    assert_eq!(lent, rows);
}

#[test]
fn the_bucket_is_the_top_bits_of_the_fibonacci_product_of_the_fx_fold() {
    // Sixteen slots: the top four bits.
    let sixteen = build::<Identity>(2, 2, (0..16_usize).map(|i| vec![i, i + 1]).collect());
    assert_eq!(sixteen.directory.len(), 16);
    assert_eq!(sixteen.bucket(&[]), 0);
    assert_eq!(sixteen.bucket(&[1]), 9);
    assert_eq!(sixteen.bucket(&[2]), 2);
    assert_eq!(sixteen.bucket(&[3]), 11);
    assert_eq!(sixteen.bucket(&[1, 2]), 3);
    assert_eq!(sixteen.bucket(&[2, 1]), 4);
    assert_eq!(sixteen.bucket(&[5, 9]), 8);

    // Two slots: the top bit.
    let two = build::<Identity>(2, 2, vec![vec![1, 2], vec![3, 4]]);
    assert_eq!(two.directory.len(), 2);
    assert_eq!(two.bucket(&[1]), 1);
    assert_eq!(two.bucket(&[2]), 0);
    assert_eq!(two.bucket(&[3]), 1);
    assert_eq!(two.bucket(&[1, 2]), 0);
    assert_eq!(two.bucket(&[5, 9]), 1);
}

#[test]
fn the_build_places_every_row_in_its_pinned_bucket() {
    let sixteen = build::<Identity>(2, 2, (0..16_usize).map(|i| vec![i, i + 1]).collect());
    let expected: [&[RowId]; 16] = [
        &[14],
        &[10],
        &[],
        &[1, 9],
        &[],
        &[],
        &[2],
        &[12],
        &[5, 6],
        &[0, 8, 13],
        &[15],
        &[3, 11],
        &[],
        &[7],
        &[4],
        &[],
    ];
    for (bucket, ids) in expected.iter().enumerate() {
        assert_eq!(chain(&sixteen, bucket), *ids, "bucket {bucket}");
    }
}

#[test]
fn rows_with_one_key_share_a_chain_in_source_order() {
    // Keyed on the first column: rows 0 and 2 carry key 2 (bucket 0 of 4),
    // row 1 carries key 3 (bucket 2).
    let index = build::<Identity>(2, 1, vec![vec![2, 10], vec![3, 11], vec![2, 12]]);
    assert_eq!(index.directory.len(), 4);
    assert_eq!(chain(&index, 0), vec![0, 2]);
    assert_eq!(chain(&index, 1), Vec::<RowId>::new());
    assert_eq!(chain(&index, 2), vec![1]);
    assert_eq!(chain(&index, 3), Vec::<RowId>::new());
}

#[test]
fn a_hasher_that_maps_everything_to_zero_chains_every_row_in_source_order() {
    let rows: Vec<Vec<usize>> = (0..9_usize).map(|i| vec![i % 3, i]).collect();
    let index = build::<OneBucket>(2, 1, rows);
    assert_eq!(index.directory.len(), 16);
    assert_eq!(chain(&index, 0), (0..9).collect::<Vec<RowId>>());
    for bucket in 1..16 {
        assert!(chain(&index, bucket).is_empty(), "bucket {bucket}");
    }
    // The chain holds three keys, so the key comparison does the separating.
    let lent: Vec<Vec<usize>> = index.probe(&[1]).map(<[usize]>::to_vec).collect();
    assert_eq!(lent, vec![vec![1, 1], vec![1, 4], vec![1, 7]]);
}

#[test]
fn every_chain_is_in_source_order_and_the_chains_partition_the_rows() {
    let rows: Vec<Vec<usize>> = (0..100_usize).map(|i| vec![(i * 7919) % 31, i]).collect();
    let index = build::<SipHashStrategy>(2, 1, rows);
    assert!(
        index.directory.len() >= index.rows.len(),
        "load factor at most 1"
    );
    let mut seen: Vec<RowId> = vec![];
    for bucket in 0..index.directory.len() {
        let ids = chain(&index, bucket);
        assert!(
            ids.windows(2).all(|pair| pair[0] < pair[1]),
            "bucket {bucket}: {ids:?}"
        );
        for &id in &ids {
            assert_eq!(index.bucket(&index.rows.row(id)[..1]), bucket, "row {id}");
        }
        seen.extend(ids);
    }
    seen.sort_unstable();
    assert_eq!(seen, (0..100).collect::<Vec<RowId>>());
}

/// Rows inserted one at a time end in the arrays the bulk build makes of
/// them: the directory doubles exactly when rows outnumber slots, a rehash
/// links last to first like the build, and a plain insert appends to its
/// chain's tail, so every chain stays in source order.
#[test]
fn insert_builds_the_arrays_the_bulk_build_builds() {
    let rows: Vec<Vec<usize>> = (0..40_usize).map(|i| vec![(i * 13) % 7, i]).collect();
    let mut inserted = HashIndex::<SipHashStrategy>::new(header(2, 1));
    for (n, row) in rows.iter().enumerate() {
        inserted.insert(row);
        let built = build::<SipHashStrategy>(2, 1, rows[..=n].to_vec());
        assert_eq!(
            inserted.directory,
            built.directory,
            "after {} inserts",
            n + 1
        );
        assert_eq!(inserted.next, built.next, "after {} inserts", n + 1);
        assert_eq!(inserted.rows, built.rows, "after {} inserts", n + 1);
    }
}

#[test]
fn a_view_build_equals_the_build_from_its_materialised_batch() {
    let source = Tuples::from(vec![
        vec![1, 1, 5],
        vec![2, 3, 5],
        vec![2, 2, 6],
        vec![1, 1, 6],
        vec![3, 3, 5],
    ]);
    // Column 2 holds 5 and columns 0 and 1 agree (rows 0 and 4), reading
    // columns 2 then 0: the output rows [5, 1] and [5, 3].
    let view = TupleView::new(&source, Filter::new(vec![(2, 5)], vec![(0, 1)]), vec![2, 0]);

    let from_view = HashIndex::<SipHashStrategy>::from_view(header(2, 1), view.clone());
    let from_batch = HashIndex::<SipHashStrategy>::from_tuples(header(2, 1), view.materialize());

    assert_eq!(from_view.rows, Tuples::from(vec![vec![5, 1], vec![5, 3]]));
    assert_eq!(from_view.rows, from_batch.rows);
    assert_eq!(from_view.directory, from_batch.directory);
    assert_eq!(from_view.next, from_batch.next);
    assert_eq!(from_view.heap_size_bytes(), from_batch.heap_size_bytes());

    // A filter that keeps nothing builds an empty index.
    let none = TupleView::new(&source, Filter::new(vec![(2, 99)], vec![]), vec![2, 0]);
    let empty = HashIndex::<SipHashStrategy>::from_view(header(2, 1), none);
    assert_eq!(empty.tuple_count(), 0);
    assert_eq!(empty.directory.len(), 2);
}

#[test]
fn heap_size_counts_the_rows_the_directory_and_the_links() {
    let words = std::mem::size_of::<usize>();
    let links = std::mem::size_of::<RowId>();
    let rows = vec![vec![1, 2], vec![3, 4], vec![1, 5]];

    // Three rows of two values, four slots, three links.
    let keyed = build::<SipHashStrategy>(2, 1, rows.clone());
    assert_eq!(keyed.heap_size_bytes(), 3 * 2 * words + (4 + 3) * links);

    // No key columns: the rows alone.
    let unkeyed = build::<SipHashStrategy>(2, 0, rows);
    assert_eq!(unkeyed.heap_size_bytes(), 3 * 2 * words);

    // An empty keyed index still holds its two empty slots.
    let empty = build::<SipHashStrategy>(2, 1, vec![]);
    assert_eq!(empty.heap_size_bytes(), 2 * links);
}

/// An empty source builds an empty index under any header, as
/// `from_tuples(header, vec![])` does.
#[test]
fn an_empty_source_of_another_arity_builds_an_empty_index() {
    let source = Tuples::new(3);
    let index = HashIndex::<SipHashStrategy>::from_view(header(2, 1), TupleView::all(&source));
    assert_eq!(index.header().arity(), 2);
    assert_eq!(index.tuple_count(), 0);
    assert_eq!(index.directory.len(), 2);
    assert_eq!(index.scan().count(), 0);
}

#[test]
fn key_len_is_the_headers_levels() {
    let rows = vec![vec![1, 2, 3]];
    assert_eq!(build::<SipHashStrategy>(3, 2, rows.clone()).key_len(), 2);
    assert_eq!(build::<SipHashStrategy>(3, 0, rows.clone()).key_len(), 0);
    // A header with no explicit level count has one per column, the keyed
    // header `bench ds` builds from: the index is keyed on every column.
    let whole = HashIndex::<SipHashStrategy>::from_tuples(RelationHeader::from(3), rows);
    assert_eq!(whole.key_len(), 3);
    assert_eq!(HashIndex::<SipHashStrategy>::new(header(3, 1)).key_len(), 1);
}

#[test]
fn it_keeps_duplicates_and_is_planned_under_probe_keys() {
    assert!(keeps_duplicates::<HashIndex>());
    assert_eq!(trie_levels::<HashIndex>(), TrieLevels::ProbeKeys);
    let index = build::<SipHashStrategy>(2, 2, vec![vec![1, 2], vec![1, 2], vec![3, 4]]);
    assert_eq!(index.tuple_count(), 3);
    assert_eq!(index.probe(&[1, 2]).count(), 2);
}

#[test]
fn the_hasher_axis_names_the_strategy() {
    let sip = build::<SipHashStrategy>(1, 1, vec![vec![1]]).optimization_axes();
    assert_eq!(sip.len(), 1);
    assert_eq!(sip["ds_layout_hasher"], "sip");
    let fx = build::<FxHashStrategy>(1, 1, vec![vec![1]]).optimization_axes();
    assert_eq!(fx.len(), 1);
    assert_eq!(fx["ds_layout_hasher"], "fxhash");
}
