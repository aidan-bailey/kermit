//! Issue #111: no tuple costs a heap allocation of its own between reading a
//! file and building any structure, except HashTrie's chains.
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
//! - **HashTrie builds** keep a chain `Vec` per distinct tuple, by design until
//!   #101's layer 3, so they are measured on the same 100 distinct tuples
//!   repeated once and 100 times: the tables and chains are the same, and only
//!   the row-id lists grow, by at most ⌈log₂ 100⌉ = 7 growth steps each. A
//!   per-row allocation adds 9 900.
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
    kermit_iters::SipHashStrategy,
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
/// tables, the chains and the lazy pending lists are the same at both
/// repetitions. Only the row-id lists hold more ids: one per first key and
/// one chain per distinct tuple, each by at most ⌈log₂ 100⌉ = 7 growth steps.
/// The bound also counts the root's list and four partition lists. Only
/// `parallel:1`'s four partitions grow, one push per row id: `scatter_rows`
/// keeps each in one list, as both inputs fit one 16 384-row morsel. The
/// root's list is `all_rows`, a `Range` never built, and `radix::partition`
/// sizes `radix:2`'s four partitions exactly from a histogram, so under
/// every other mode those five lists are part of the bound's slack, 5 × 7 =
/// 35 allocations. `parallel:1`'s workers, merge and test-hook record cost
/// the same at both repetitions.
#[test]
fn hash_trie_builds_allocate_per_list_not_per_row() {
    let lists = (1 + KEYS + KEYS * KEYS + 4) as u64;
    let bound = lists * ceil_log2(REPEATS);
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
            many <= once + bound,
            "HashTrie {layout} {mode:?}: {once} allocations for {} rows, {many} for {}, more than \
             {bound} apart: some row allocates",
            KEYS * KEYS,
            REPEATS * KEYS * KEYS
        );
    }
}

/// `presized:1` under `root-capacity=tuples`, its prerequisite. The root is
/// sized once from the row count, one allocation at either repetition, and
/// its lists and chains grow as under the other builds. Its partitions are
/// whole runs of the root's 4 096-bucket regions, at most
/// [`ONE_THREAD_PARTITIONS`]: the 100 rows' 256-bucket root is one region and
/// one partition, the 10 000 rows' 16 384-bucket root four. So the larger
/// build has three partitions more, each a row-id bucket grown from empty (at
/// most `doublings(rows)` steps) and a segment list, and the one partition
/// both builds share is one more growing list. The fixture's ten keys never
/// probe past a region's end, so no row is deferred to the tail, whose list
/// the bound does not count.
#[test]
fn hash_trie_presized_build_allocates_per_list_not_per_row() {
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
    let lists = (KEYS + KEYS * KEYS + 1) as u64;
    let new_partitions = (many_partitions - once_partitions) as u64;
    let bound = lists * ceil_log2(REPEATS) + new_partitions * (doublings(rows) + 1);
    assert!(
        many <= once + bound,
        "HashTrie presized:1: {once} allocations for {} rows, {many} for {rows}, more than \
         {bound} apart: some row allocates",
        KEYS * KEYS,
    );
}
