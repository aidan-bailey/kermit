//! Issue #111: no tuple costs a heap allocation of its own between reading a
//! file and building any structure. HashTrie's lists were the one exception
//! until #101's layer 3 threaded them through one link per row.
//!
//! Each test counts the calling thread's allocations with `allocation-counter`,
//! whose counting global allocator this test binary links, as
//! `result_allocation.rs` does. A reallocation counts as one allocation (the
//! counter keeps `GlobalAlloc`'s default `realloc`, an `alloc` plus a
//! `dealloc`), so a `Vec` grown one push at a time costs one allocation per
//! growth step: [`doublings`].
//!
//! - **Readers** are measured at two sizes ten times apart. The CSV reader's
//!   one buffer doubles, so the larger read may cost ⌈log₂ 10⌉ = 4 more
//!   allocations; the Parquet reader sizes its buffer once from the footer, and
//!   arrow decodes in 1 024-row record batches and allocates per batch, never
//!   per row. A per-row allocation costs 9 000 more.
//! - **A batch clone** and **a reordered copy** (`IndexSpec::permute_all`) are
//!   one allocation each.
//! - **TreeTrie and ColumnTrie builds** are bounded by the heap objects the
//!   built trie owns: TreeTrie a children `Vec` per inner node, ColumnTrie two
//!   arrays per layer. A per-row allocation adds 10 000.
//! - **HashTrie builds** thread every list through one 4-byte link per row
//!   (#101's layer 3): Algorithm 2's lists, the leaf chains and the lazy
//!   pending lists are `Chain`s stored in their buckets, so a build allocates
//!   per table, never per list. They are measured on the same 100 distinct
//!   tuples repeated once and 100 times: the tables are the same, and the links
//!   and a partitioning build's copy are one allocation each at either size, so
//!   the larger build may allocate no more than the smaller. A per-row
//!   allocation adds 9 900, and a list grown one push per row, as each was
//!   before, up to 7.
//!
//! - **Parallel builds** (`parallel:N`, `presized:N`) are measured at one
//!   thread. `run_workers` (`kermit_ds::morsel`) runs the work on the calling
//!   thread and spawns `N − 1` helpers, so at `N = 1` every allocation is the
//!   calling thread's and the counter sees all of it. At `N > 1` the helpers
//!   allocate on threads of their own, which the counter cannot see, but they
//!   run the same per-partition code. TreeTrie's `parallel:1` is bounded like
//!   its serial build, plus each partition's lists; HashTrie's are measured at
//!   both repetitions like its other builds.

use {
    kermit_algos::IndexSpec,
    kermit_ds::{
        test_hooks::{
            take_hash_trie_parallel_builds, take_tree_trie_parallel_builds, HashTrieParallelBuild,
        },
        BuildModeRelation, Cardinality, ColumnTrie, ColumnTrieBuildMode, EagerExpansion,
        ExpansionPolicy, HashTrie, HashTrieBuildMode, HashTrieConfig, LazyExpansion, NoPruning,
        PruningPolicy, RadixBits, Relation, RootCapacity, SingletonPruning, Threads, TreeTrie,
        TreeTrieBuildMode, Tuples,
    },
    kermit_iters::{Filter, RowId, SipHashStrategy, TupleView},
    kermit_rdf::{parquet::write_relation, partition::PartitionedRelation},
    std::{fmt::Write, path::Path},
};

/// Allocations a `Vec` grown one push at a time makes to hold `len`
/// elements: std grows an empty `Vec` of 2- to 1 024-byte elements to 4
/// slots, then doubles it.
fn doublings(len: usize) -> u64 {
    let (mut capacity, mut allocations) = (0, 0);
    while capacity < len {
        capacity = (capacity * 2).max(4);
        allocations += 1;
    }
    allocations
}

/// ⌈log₂ ratio⌉: how many more growth steps a buffer `ratio` times longer
/// can need.
fn ceil_log2(ratio: usize) -> u64 { u64::from(ratio.next_power_of_two().trailing_zeros()) }

// ── Readers ─────────────────────────────────────────────────────────────

/// Rows of the smaller and the larger reader input, ten times apart.
const SMALL: usize = 1_000;
const LARGE: usize = 10_000;

/// `rows` 2-ary rows `(i, i % 7)`.
fn pairs(rows: usize) -> Vec<(usize, usize)> { (0..rows).map(|i| (i, i % 7)).collect() }

/// Writes `pairs(rows)` as `<dir>/r.csv` and `<dir>/r.parquet`. One file name
/// at every size, so the relation name the readers allocate is the same.
fn write_inputs(dir: &Path, rows: usize) {
    let mut csv = String::from("s,o\n");
    for (s, o) in pairs(rows) {
        writeln!(csv, "{s},{o}").unwrap();
    }
    std::fs::write(dir.join("r.csv"), csv).unwrap();
    let relation = PartitionedRelation {
        name: "r".into(),
        tuples: pairs(rows),
    };
    write_relation(&relation, &dir.join("r.parquet")).unwrap();
}

fn csv_tuples(path: &Path) -> Tuples { kermit_ds::read_csv(path).unwrap().1 }

fn parquet_tuples(path: &Path) -> Tuples { kermit_ds::read_parquet(path).unwrap().1 }

/// Allocations of one `read` of `path`, after checking it returned `rows`
/// rows. An unmeasured read runs first, so a one-time initialisation on the
/// read path cannot count against the size measured first.
fn read_allocations(read: fn(&Path) -> Tuples, path: &Path, rows: usize) -> u64 {
    drop(read(path));
    let mut tuples = None;
    let info = allocation_counter::measure(|| tuples = Some(read(path)));
    assert_eq!(tuples.expect("measured").len(), rows, "{path:?}");
    info.count_total
}

/// Allocations of `read` over `<file>` at [`SMALL`] and at [`LARGE`] rows.
fn small_and_large(read: fn(&Path) -> Tuples, file: &str) -> (u64, u64) {
    let (small, large) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    write_inputs(small.path(), SMALL);
    write_inputs(large.path(), LARGE);
    (
        read_allocations(read, &small.path().join(file), SMALL),
        read_allocations(read, &large.path().join(file), LARGE),
    )
}

/// One buffer grows with the rows; the record that holds each row is reused.
/// The buffer doubles, so ten times the rows costs at most ⌈log₂ 10⌉ = 4 more
/// allocations. The final `shrink_to_fit` reallocates at both sizes, so it
/// cancels out of the difference; 2 more allow for the reused
/// `StringRecord` growing to hold the larger input's wider fields.
#[test]
fn read_csv_allocates_independently_of_its_row_count() {
    let (small, large) = small_and_large(csv_tuples, "r.csv");
    let bound = small + ceil_log2(LARGE / SMALL) + 2;
    assert!(
        large <= bound,
        "read_csv: {small} allocations for {SMALL} rows, {large} for {LARGE}, over {bound}: some \
         row allocates"
    );
}

/// The buffer is sized once from the footer's row count. Arrow decodes 1 024
/// rows per record batch and allocates a few buffers per batch and column:
/// nine more batches here. The bound allows a quarter of an allocation per
/// extra row (250 per extra batch); one per row would be four times over it.
#[test]
fn read_parquet_allocates_per_batch_not_per_row() {
    let (small, large) = small_and_large(parquet_tuples, "r.parquet");
    let bound = small + (LARGE - SMALL) as u64 / 4;
    assert!(
        large <= bound,
        "read_parquet: {small} allocations for {SMALL} rows, {large} for {LARGE}, over {bound}: \
         some row allocates"
    );
}

// ── Copies ──────────────────────────────────────────────────────────────

/// First keys of the sorted-trie fixture.
const FIRST_KEYS: usize = 2_500;
/// Second keys per first key: what an empty `Vec`'s first allocation holds,
/// so each first key's children cost TreeTrie exactly one allocation.
const FAN_OUT: usize = 4;

/// `(x, y)` for `x < FIRST_KEYS`, `y < FAN_OUT`: 10 000 distinct rows, in
/// descending order so a build's sort moves every one. Exactly sized.
fn fanned_out() -> Tuples {
    let mut tuples = Tuples::with_capacity(2, FIRST_KEYS * FAN_OUT);
    for x in (0..FIRST_KEYS).rev() {
        for y in (0..FAN_OUT).rev() {
            tuples.push(&[x, y]);
        }
    }
    tuples
}

/// A batch clone is one `Vec<usize>` copy at the batch's exact size: what
/// every `bench ds` / `bench run` setup clone and `load_with_tuples` pay.
#[test]
fn cloning_a_batch_allocates_once() {
    let batch = fanned_out();
    let mut copy = None;
    let info = allocation_counter::measure(|| copy = Some(batch.clone()));
    assert_eq!(info.count_total, 1);
    let copy = copy.expect("measured");
    assert_eq!(copy, batch);
    assert_eq!(
        copy.heap_size_bytes(),
        batch.len() * batch.arity() * std::mem::size_of::<usize>()
    );
}

/// A reordered copy under `--column-orders any` is one buffer.
#[test]
fn a_reordered_copy_allocates_once() {
    let batch = fanned_out();
    let spec = IndexSpec::new("r", vec![1, 0]);
    let mut copy = None;
    let info = allocation_counter::measure(|| copy = Some(spec.permute_all(&batch)));
    assert_eq!(info.count_total, 1);
    assert_eq!(copy.expect("measured").len(), batch.len());
}

// ── Tuple views ─────────────────────────────────────────────────────────

/// Rows a view matches, a power of two so that the row-id `Vec` that
/// `materialize_sorted` collects (4, 8, 16, … slots) ends at exactly this
/// many, and its peak is exactly `VIEW_MATCHING × 4` bytes.
const VIEW_MATCHING: usize = 4_096;
/// Source rows, half of them matching [`view_filter`].
const VIEW_ROWS: usize = 2 * VIEW_MATCHING;
const WORD: usize = std::mem::size_of::<usize>();

/// `VIEW_ROWS` rows of arity 6 whose column 0 alternates 0, 1, 0, 1, … and
/// whose other columns repeat prefixes, so sorting moves rows.
fn view_source() -> Tuples {
    let mut tuples = Tuples::with_capacity(6, VIEW_ROWS);
    for row in 0..VIEW_ROWS {
        let mixed = row.wrapping_mul(2_654_435_761) >> 4;
        tuples.push(&[
            row % 2,
            mixed % 3,
            mixed % 5,
            mixed % 7,
            mixed % 11,
            mixed % 13,
        ]);
    }
    tuples
}

/// Column 0 holds 0: the even rows, `VIEW_MATCHING` of them.
fn view_filter() -> Filter { Filter::new(vec![(0, 0)], vec![]) }

/// Whether `batch` is in lexicographic order.
fn is_sorted(batch: &Tuples) -> bool {
    let rows = batch.to_vecs();
    rows.windows(2).all(|pair| pair[0] <= pair[1])
}

/// A sorted copy is one copy of the data at every arity. Arities 1-4 sort
/// in place; from 5, row ids are sorted against the view and the data is
/// gathered once, so the peak is the data and the row-id array, and a
/// `materialize` followed by `Tuples::sort` (its own gather above arity 4)
/// would hold the data twice.
#[test]
fn a_sorted_copy_of_a_view_holds_the_data_once_at_every_arity() {
    let source = view_source();
    for columns in [vec![1, 0], vec![5, 4, 3, 2, 1], vec![5, 4, 3, 2, 1, 0]] {
        let arity = columns.len();
        let view = TupleView::new(&source, view_filter(), columns);
        assert_eq!(view.count(), VIEW_MATCHING);
        let data_bytes = (VIEW_MATCHING * arity * WORD) as u64;
        let mut copy = None;
        let info = allocation_counter::measure(|| copy = Some(view.materialize_sorted()));
        let copy = copy.expect("measured");
        assert_eq!((copy.arity(), copy.len()), (arity, VIEW_MATCHING));
        assert!(is_sorted(&copy), "arity {arity}");
        if arity <= 4 {
            assert_eq!(info.count_total, 1, "arity {arity}: one allocation");
            assert_eq!(
                info.bytes_total, data_bytes,
                "arity {arity}: the data, once"
            );
        } else {
            assert_eq!(
                info.bytes_max,
                data_bytes + (VIEW_MATCHING * std::mem::size_of::<RowId>()) as u64,
                "arity {arity}: the data and the row ids live at once, and nothing else"
            );
        }
    }
}

/// An unfiltered view matches every row, so its row ids are collected from
/// the range: the id array is sized exactly, and the sorted copy costs two
/// allocations (the ids, then the data) of exactly `n × 4` and the data's
/// bytes in total, not the doubling collect's 4, 8, 16, … slots.
#[test]
fn a_sorted_copy_of_an_unfiltered_view_sizes_its_row_ids_exactly() {
    let source = view_source();
    for columns in [vec![5, 4, 3, 2, 1], vec![5, 4, 3, 2, 1, 0]] {
        let arity = columns.len();
        let view = TupleView::new(&source, Filter::new(vec![], vec![]), columns);
        assert_eq!(view.count(), VIEW_ROWS);
        let data_bytes = (VIEW_ROWS * arity * WORD) as u64;
        let mut copy = None;
        let info = allocation_counter::measure(|| copy = Some(view.materialize_sorted()));
        let copy = copy.expect("measured");
        assert_eq!((copy.arity(), copy.len()), (arity, VIEW_ROWS));
        assert!(is_sorted(&copy), "arity {arity}");
        assert_eq!(
            info.count_total, 2,
            "arity {arity}: the row ids, then the data"
        );
        assert_eq!(
            info.bytes_total,
            data_bytes + (VIEW_ROWS * std::mem::size_of::<RowId>()) as u64,
            "arity {arity}: the data and the row ids, each sized exactly"
        );
    }
}

/// `gather_sorted` over a supplied row set sorts the ids in place and
/// allocates the output buffer, once, at its exact size.
#[test]
fn gathering_a_sorted_row_set_allocates_the_output_once() {
    let source = view_source();
    for columns in [vec![5, 4, 3, 2, 1], vec![5, 4, 3, 2, 1, 0]] {
        let arity = columns.len();
        let view = TupleView::new(&source, view_filter(), columns);
        let mut rows: Vec<RowId> = (0..VIEW_ROWS as RowId)
            .filter(|&r| view.matches(r))
            .collect();
        let mut copy = None;
        let info = allocation_counter::measure(|| copy = Some(view.gather_sorted(&mut rows)));
        let copy = copy.expect("measured");
        assert_eq!(copy.len(), VIEW_MATCHING);
        assert!(is_sorted(&copy), "arity {arity}");
        assert_eq!(info.count_total, 1, "arity {arity}");
        assert_eq!(info.bytes_total, (VIEW_MATCHING * arity * WORD) as u64);
    }
}

/// An identity view is one `memcpy` of the source: one allocation of its
/// exact size, with no scan of the rows.
#[test]
fn materializing_an_identity_view_allocates_once() {
    let source = view_source();
    let view = TupleView::all(&source);
    let mut copy = None;
    let info = allocation_counter::measure(|| copy = Some(view.materialize()));
    assert_eq!(info.count_total, 1);
    assert_eq!(info.bytes_total, (VIEW_ROWS * 6 * WORD) as u64);
    assert_eq!(copy.expect("measured"), source);
}

// ── Builds ──────────────────────────────────────────────────────────────

/// Room for a sort's scratch buffers (a permutation and a gather, at arities
/// above 4 only) and a constant or two. A per-row allocation adds 10 000.
const SCRATCH: u64 = 4;

/// TreeTrie owns one heap object per inner node, its children `Vec`: the
/// root's, grown one push per first key, and one per first key, whose
/// `FAN_OUT` leaves fit its first allocation. Leaves own nothing. Expected
/// count: 2 500 + 11.
#[test]
fn tree_trie_build_allocates_once_per_inner_node() {
    let tuples = fanned_out();
    let rows = tuples.len();
    let mut built: Option<TreeTrie> = None;
    let info =
        allocation_counter::measure(|| built = Some(TreeTrie::from_tuples(2.into(), tuples)));
    assert_eq!(built.expect("measured").tuple_count(), rows);
    let nodes = FIRST_KEYS as u64 + doublings(FIRST_KEYS);
    assert!(
        info.count_total <= nodes + SCRATCH,
        "TreeTrie: {} allocations for {rows} rows; its inner nodes account for {nodes}",
        info.count_total
    );
}

/// The one thread of the measured parallel builds: `run_workers` spawns no
/// helper, so the counter sees every allocation.
fn one_thread() -> Threads { Threads::new(1).expect("one thread is a valid count") }

/// Partitions of a one-thread parallel build: `PARTITIONS_PER_THREAD`
/// (crate-private in `kermit_ds::morsel`) × 1. HashTrie's `parallel:1`
/// rounds it to a power of two, still 4, and `presized:1` caps it at the
/// root's regions.
const ONE_THREAD_PARTITIONS: u64 = 4;

/// What TreeTrie's one-thread parallel build allocates besides its trie and
/// its partitions, at any input size. Each `run_workers` call (`scatter_rows`'
/// and `dispatch`'s) allocates the scope's shared state, the worker's own
/// results and the `Vec` holding them, and its caller merges the results
/// into one more: 4 each. `scatter_rows` adds its one morsel's bucket list
/// and the partition list (2), TreeTrie its splitter sample and splitters
/// (2), and the test hook its record of the build (2).
const ONE_THREAD_MACHINERY: u64 = 2 * 4 + 2 + 2 + 2;

/// `parallel:1` builds the serial trie, down to each `Vec`'s capacity, so it
/// allocates the serial build's inner nodes. On top of them it allocates,
/// per partition, the row-id bucket `scatter_rows` grows one push per id
/// (at most `doublings(rows)` steps), its segment list, the gathered rows
/// (sized once) and the node list its rows insert into, grown one push per
/// first key; and [`ONE_THREAD_MACHINERY`]. The fixture's 10 000 rows fit
/// one 16 384-row morsel. A per-row allocation adds 10 000.
#[test]
fn tree_trie_parallel_build_on_one_thread_allocates_per_partition_not_per_row() {
    let tuples = fanned_out();
    let rows = tuples.len();
    let mode = TreeTrieBuildMode::Parallel(one_thread());
    let mut built: Option<TreeTrie> = None;
    let info = allocation_counter::measure(|| {
        built = Some(TreeTrie::from_tuples_with_build_mode(
            2.into(),
            mode,
            tuples,
        ));
    });
    assert_eq!(built.expect("measured").tuple_count(), rows);
    let builds = take_tree_trie_parallel_builds();
    assert!(
        matches!(builds.as_slice(), [(1, _)]),
        "parallel:1 ran one build on one thread: {builds:?}"
    );
    let nodes = FIRST_KEYS as u64 + doublings(FIRST_KEYS);
    let partitions = ONE_THREAD_PARTITIONS * (doublings(rows) + 1 + 1 + doublings(FIRST_KEYS));
    let bound = nodes + partitions + ONE_THREAD_MACHINERY + SCRATCH;
    assert!(
        info.count_total <= bound,
        "TreeTrie parallel:1: {} allocations for {rows} rows, over {bound}: its inner nodes \
         account for {nodes} and its partitions for {partitions}",
        info.count_total
    );
}

/// ColumnTrie owns its `layers` `Vec` and each layer's `data` and `interval`
/// arrays, each at most one entry per row and grown one push at a time, under
/// both build modes. Expected count: 1 + 11 + 1 + 13 + 11 = 37 of at most 57.
#[test]
fn column_trie_builds_allocate_only_their_layer_arrays() {
    for mode in [ColumnTrieBuildMode::Bulk, ColumnTrieBuildMode::Incremental] {
        let tuples = fanned_out();
        let rows = tuples.len();
        let mut built: Option<ColumnTrie> = None;
        let info = allocation_counter::measure(|| {
            built = Some(ColumnTrie::from_tuples_with_build_mode(
                2.into(),
                mode,
                tuples,
            ));
        });
        assert_eq!(built.expect("measured").tuple_count(), rows, "{mode:?}");
        let arrays = 1 + 2 * 2 * doublings(rows);
        assert!(
            info.count_total <= arrays + SCRATCH,
            "ColumnTrie {mode:?}: {} allocations for {rows} rows; its layer arrays account for \
             {arrays}",
            info.count_total
        );
    }
}

/// Distinct first keys, and distinct second keys under each, of the HashTrie
/// fixture: 100 distinct tuples.
const KEYS: usize = 10;
/// How many times the larger HashTrie input repeats the 100 tuples.
const REPEATS: usize = 100;

/// Every `(x, y)` with `x, y < KEYS`, the whole set `repeats` times over.
fn repeated(repeats: usize) -> Tuples {
    let mut tuples = Tuples::with_capacity(2, repeats * KEYS * KEYS);
    for _ in 0..repeats {
        for x in 0..KEYS {
            for y in 0..KEYS {
                tuples.push(&[x, y]);
            }
        }
    }
    tuples
}

/// Allocations of one `HashTrie<SipHashStrategy, P, E>` build of
/// `repeated(repeats)` by `mode` under `config`, after checking it stored
/// every row, and the record a parallel build left. Taking the record also
/// empties the hook's `Vec`, so every measured build pushes onto an empty
/// one.
fn measure_hash_build<P: PruningPolicy, E: ExpansionPolicy>(
    config: HashTrieConfig, mode: HashTrieBuildMode, repeats: usize,
) -> (u64, Vec<HashTrieParallelBuild>) {
    let tuples = repeated(repeats);
    let rows = tuples.len();
    let mut built: Option<HashTrie<SipHashStrategy, P, E>> = None;
    let info = allocation_counter::measure(|| {
        built = Some(HashTrie::from_tuples_with_config_and_build_mode(
            2.into(),
            config,
            mode,
            tuples,
        ));
    });
    assert_eq!(built.expect("measured").tuple_count(), rows);
    (info.count_total, take_hash_trie_parallel_builds())
}

/// [`measure_hash_build`] under the default config, after checking that a
/// `parallel:1` build ran as one build on one thread and no other mode ran
/// a parallel build.
fn hash_build_allocations<P: PruningPolicy, E: ExpansionPolicy>(
    mode: HashTrieBuildMode, repeats: usize,
) -> u64 {
    let (allocations, builds) =
        measure_hash_build::<P, E>(HashTrieConfig::default(), mode, repeats);
    let threads: Vec<usize> = builds.iter().map(|build| build.threads).collect();
    let expected = match mode {
        | HashTrieBuildMode::Parallel(threads) => vec![threads.get()],
        | _ => vec![],
    };
    assert_eq!(threads, expected, "{mode:?}: the parallel builds that ran");
    allocations
}

/// Under the default `root-capacity=grow` every table's size depends on its
/// keys alone (`tuples` would size the root from the row count), so the
/// tables, and under lazy expansion each root child's box, are the same at
/// both repetitions. No list allocates (#101's layer 3): Algorithm 2's lists,
/// the leaf chains and the lazy pending lists are `Chain`s, a head, a tail and
/// a length in their bucket, threaded through the trie's links, one 4-byte
/// link per row. The links are one allocation at either size: `bulk` and the
/// partitioning builds allocate them at their full length
/// (`ChainLinks::new`), and `incremental` reserves them once
/// (`ChainLinks::with_capacity`) and pushes one link per row into that room.
/// The buffer is the input batch, or a partitioning build's copy, one
/// allocation sized from the batch.
///
/// The partitioning builds, `radix:2` and `parallel:1`, grow no list per
/// partition (#101). `partition_rows` counts each partition's rows in a
/// histogram pass, keeping each row's partition number in a `u16` list sized
/// from its morsel, then copies the rows and their input positions
/// (`origin`) into zeroed buffers sized once from the batch, and a partition
/// is a range of the copy. `radix:2` passes its whole batch as one morsel,
/// and both inputs fit one of `parallel:1`'s 16 384-row morsels, so the
/// histogram, the partition numbers, the offsets and the morsel's ranges are
/// one allocation each at both repetitions. So are the workers, the scratch
/// roots' tables (sized by their keys), the merge, and `parallel:1`'s
/// test-hook record.
///
/// Nothing grows with the row count, so the larger build may make no more
/// allocations than the smaller: 0 extra, where the bound was 777 (111 row-id
/// lists of up to ⌈log₂ 100⌉ = 7 growth steps each) while the lists were
/// `Vec`s. Every cell measured equal when the bound was set (the commit
/// records the counts). The check is one-sided, like the other differential
/// cells: a one-time initialisation on a path's first run would fall in the
/// smaller build, which runs first.
#[test]
fn hash_trie_builds_allocate_independently_of_their_row_count() {
    let radix = HashTrieBuildMode::Radix(RadixBits::new(2).unwrap());
    type Measure = fn(HashTrieBuildMode, usize) -> u64;
    let cells: [(&str, Measure, HashTrieBuildMode); 7] = [
        (
            "eager",
            hash_build_allocations::<NoPruning, EagerExpansion>,
            HashTrieBuildMode::Bulk,
        ),
        (
            "pruned",
            hash_build_allocations::<SingletonPruning, EagerExpansion>,
            HashTrieBuildMode::Bulk,
        ),
        (
            "lazy",
            hash_build_allocations::<NoPruning, LazyExpansion>,
            HashTrieBuildMode::Bulk,
        ),
        (
            "pruned lazy",
            hash_build_allocations::<SingletonPruning, LazyExpansion>,
            HashTrieBuildMode::Bulk,
        ),
        (
            "eager",
            hash_build_allocations::<NoPruning, EagerExpansion>,
            HashTrieBuildMode::Incremental,
        ),
        (
            "eager",
            hash_build_allocations::<NoPruning, EagerExpansion>,
            radix,
        ),
        (
            "eager",
            hash_build_allocations::<NoPruning, EagerExpansion>,
            HashTrieBuildMode::Parallel(one_thread()),
        ),
    ];
    for (layout, allocations, mode) in cells {
        let (once, many) = (allocations(mode, 1), allocations(mode, REPEATS));
        assert!(
            many <= once,
            "HashTrie {layout} {mode:?}: {once} allocations for {} rows, {many} for {}: the \
             larger build allocates more, so some row or list allocates",
            KEYS * KEYS,
            REPEATS * KEYS * KEYS
        );
    }
}

/// `presized:1` under `root-capacity=tuples`, its prerequisite. The root is
/// sized once from the row count, one allocation at either repetition, and
/// the children's tables by their keys. No list allocates (#101's layer 3):
/// the root's chains, the children's and the leaf chains are `Chain`s
/// threaded through the trie's links, which `from_tuples_partitioned`
/// allocates once at the row count. Its partitions are whole runs of the
/// root's 4 096-bucket regions, at most [`ONE_THREAD_PARTITIONS`]: the 100
/// rows' 256-bucket root is one region and one partition, the 10 000 rows'
/// 16 384-bucket root four.
///
/// The three extra partitions cost no allocation (#101). `partition_rows`
/// sizes its histogram, partition numbers, offsets, copy and `origin` once,
/// and a partition is a range of the copy. `with_runs` and `map_in_runs` size
/// their run lists once, and each worker's result list holds four entries in
/// its first allocation, so one partition or four cost the same. The
/// fixture's ten keys never probe past a region's end, so no row is deferred
/// to the tail: its queue of deferred ids, the one `Vec` here grown a push at
/// a time, never allocates.
///
/// So the larger build may allocate no more than the smaller: 0 extra, where
/// the bound was 770 (110 row-id lists of up to 7 growth steps each) while
/// the lists were `Vec`s. It measured equal when the bound was set (the
/// commit records the counts).
#[test]
fn hash_trie_presized_build_allocates_independently_of_its_row_count() {
    let config = HashTrieConfig {
        root_capacity: RootCapacity::Tuples,
        ..HashTrieConfig::default()
    };
    let mode = HashTrieBuildMode::Presized(one_thread());
    let measure = |repeats| {
        let (allocations, builds) =
            measure_hash_build::<NoPruning, EagerExpansion>(config, mode, repeats);
        let [build] = builds.as_slice() else {
            panic!("presized:1 ran one build: {builds:?}");
        };
        assert_eq!(build.threads, 1, "presized:1 ran on one thread");
        assert_eq!(build.deferred, Some(0), "no row is deferred to the tail");
        (allocations, build.partition_sizes.len())
    };
    let ((once, once_partitions), (many, many_partitions)) = (measure(1), measure(REPEATS));
    assert_eq!((once_partitions, many_partitions), (1, 4));
    let rows = REPEATS * KEYS * KEYS;
    assert!(
        many <= once,
        "HashTrie presized:1: {once} allocations for {} rows, {many} for {rows}: the larger build \
         allocates more, so some row or list allocates",
        KEYS * KEYS,
    );
}
