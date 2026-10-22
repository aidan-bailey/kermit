//! Morsel-driven work distribution for the parallel builds (Leis et al.,
//! *Morsel-Driven Parallelism*, SIGMOD 2014).
//!
//! The input is cut into small fixed-size *morsels*. The *dispatcher*, a
//! mutex-guarded queue, hands the next unit of work to whichever worker is
//! free, so a slow worker never stalls the rest. A build uses it twice:
//!
//! 1. [`scatter_rows`] sends row ids into partitions, one morsel at a time, or
//!    [`partition_rows`] copies the rows themselves into partition order (the
//!    paper's materialisation step, #101);
//! 2. [`dispatch`] then runs one task per partition.
//!
//! No result depends on scheduling. A partition lists its rows in input
//! order, and task results come back in task order, so a parallel build
//! can reproduce its serial counterpart exactly
//! (`docs/data-structures/parallel-build.md`).
//!
//! The workers are [`std::thread::scope`] threads, the calling thread among
//! them. Every worker is joined before a call returns, and a worker's panic
//! is re-raised in the caller.

use {
    kermit_iters::{RowId, TupleView, Tuples},
    std::{mem, num::NonZeroUsize, ops::Range, panic, sync::Mutex, thread},
};

#[cfg(test)]
thread_local! {
    /// The thread count of every `run_workers` call on this thread, so a
    /// test can check a build really ran on the threads it was given.
    static WORKER_RUNS: std::cell::RefCell<Vec<usize>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// Drains the thread counts `run_workers` has recorded on this thread.
#[cfg(test)]
pub(crate) fn take_worker_runs() -> Vec<usize> { WORKER_RUNS.with(std::cell::RefCell::take) }

/// How many threads a parallel build uses, the calling thread included:
/// from 1 to [`Threads::MAX`]. No other count can be represented.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Threads(NonZeroUsize);

impl Threads {
    /// The most threads one build may use. It bounds the threads a build
    /// spawns and the per-morsel bucket headers (`4 · N` per morsel), and is
    /// far above any core count this platform measures on.
    pub const MAX: usize = 1024;

    /// `n` threads, or `None` when `n` is zero or above [`Threads::MAX`].
    pub fn new(n: usize) -> Option<Self> {
        if n > Self::MAX {
            return None;
        }
        NonZeroUsize::new(n).map(Self)
    }

    /// The number of threads.
    pub fn get(self) -> usize { self.0.get() }
}

/// Tuples per morsel in the parallel builds. A worker takes one morsel per
/// lock of the shared queue, and one uncontended lock per 16 384 tuples is
/// negligible, while morsels this small still give a mid-sized relation
/// several morsels per thread. (Leis et al. use morsels of about 100 000
/// tuples.)
pub(crate) const MORSEL_TUPLES: usize = 16_384;

/// Partitions per thread in the parallel builds. More partitions than
/// threads let [`dispatch`] balance partitions of uneven size.
pub(crate) const PARTITIONS_PER_THREAD: usize = 4;

/// The queue's next item. The lock is held only while taking it — a guard
/// in a `while let` condition would live through the loop body and
/// serialise the workers.
fn take_next<I: Iterator>(queue: &Mutex<I>) -> Option<I::Item> { queue.lock().unwrap().next() }

/// Runs `work` on `threads` workers, the calling thread and `threads − 1`
/// scoped threads, and returns every worker's result once all have
/// finished. A worker's panic is re-raised here with its original payload.
fn run_workers<R: Send>(threads: Threads, work: impl Fn() -> R + Sync) -> Vec<R> {
    #[cfg(test)]
    WORKER_RUNS.with(|runs| runs.borrow_mut().push(threads.get()));
    thread::scope(|scope| {
        let helpers: Vec<_> = (1..threads.get()).map(|_| scope.spawn(&work)).collect();
        let mut results = vec![work()];
        for helper in helpers {
            match helper.join() {
                | Ok(result) => results.push(result),
                | Err(payload) => panic::resume_unwind(payload),
            }
        }
        results
    })
}

/// The row ids [`scatter_rows`] sent to one partition: one segment per
/// morsel that contributed any, in morsel order. Reading the segments in
/// order reads the partition in input order, so its ids ascend.
#[derive(Debug, Default)]
pub(crate) struct RowPartition {
    segments: Vec<Vec<RowId>>,
}

impl RowPartition {
    /// How many rows the partition holds.
    pub(crate) fn len(&self) -> usize { self.segments.iter().map(Vec::len).sum() }

    /// The partition's row ids in input order, ascending.
    pub(crate) fn ids(&self) -> impl Iterator<Item = RowId> + '_ {
        self.segments.iter().flatten().copied()
    }
}

/// One morsel's row ids, split by partition.
type RowBuckets = Vec<Vec<RowId>>;

/// `position` as a [`RowId`]. A [`Tuples`] batch holds at most `RowId::MAX`
/// rows, so every position up to its length fits. Crate-visible: HashTrie's
/// builds convert their row positions with it too.
pub(crate) fn row_id(position: usize) -> RowId {
    RowId::try_from(position).expect("a `Tuples` batch holds at most `RowId::MAX` rows")
}

/// Sends the id of every source row that `view` keeps to partition
/// `partition_of(output)`, where `output` is the row's output columns and
/// the partition must be below `partitions`. The source stays where it is:
/// workers read it through the view, and only 4-byte ids move. The ids are
/// *source* row ids (positions in `view.source()`), not positions among the
/// kept rows, so a later [`TupleView::gather_sorted`] or `write` can read
/// them back; rows the view's filter rejects are skipped.
///
/// `threads` workers take morsels of `morsel_rows` consecutive source rows
/// from a shared queue, so a worker that finishes early takes more. Every
/// partition lists its ids in input order, whatever order the morsels ran
/// in. Each worker writes a row's output columns into one scratch row it
/// allocates once, so nothing is allocated per row.
///
/// # Panics
///
/// Panics if `morsel_rows` is zero, or if `partition_of` returns
/// `partitions` or more, or panics itself.
pub(crate) fn scatter_rows(
    threads: Threads, view: &TupleView<'_>, morsel_rows: usize, partitions: usize,
    partition_of: impl Fn(&[usize]) -> usize + Sync,
) -> Vec<RowPartition> {
    let len = view.source_rows();
    let queue = Mutex::new((0..len).step_by(morsel_rows).enumerate());
    let mut morsels: Vec<(usize, RowBuckets)> = run_workers(threads, || {
        let mut scattered = Vec::new();
        let mut output = vec![0; view.arity()];
        while let Some((index, first)) = take_next(&queue) {
            let end = first + morsel_rows.min(len - first);
            let mut buckets: RowBuckets = (0..partitions).map(|_| Vec::new()).collect();
            // An identity view keeps every row and its source row is its
            // output row, so it pays no per-row filter or copy (Amendment 2
            // of the per-query index build spec): it is the build path of
            // every `from_tuples*` and of every constant-free index, and a
            // check there would cost only the modes that scatter. The branch
            // is taken once; the two loops differ only in how a row is read.
            if view.is_identity() {
                for id in row_id(first)..row_id(end) {
                    buckets[partition_of(view.source().row(id))].push(id);
                }
            } else {
                for id in row_id(first)..row_id(end) {
                    if !view.matches(id) {
                        continue;
                    }
                    view.write(id, &mut output);
                    buckets[partition_of(&output)].push(id);
                }
            }
            scattered.push((index, buckets));
        }
        scattered
    })
    .into_iter()
    .flatten()
    .collect();
    morsels.sort_unstable_by_key(|&(index, _)| index);

    let mut out: Vec<RowPartition> = (0..partitions).map(|_| RowPartition::default()).collect();
    for (_, buckets) in morsels {
        for (partition, bucket) in out.iter_mut().zip(buckets) {
            if !bucket.is_empty() {
                partition.segments.push(bucket);
            }
        }
    }
    out
}

/// The rows a view selects, copied into partition order by
/// [`partition_rows`]: the paper's materialisation step, which partitions the
/// tuples so "that tuples with similar join key hash values reside in
/// physically close memory locations" (Freitag et al., VLDB 2020, §3.3.2).
/// Partition `p`'s rows are the rows `offsets[p]..offsets[p + 1]` of
/// `tuples`, in source order.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Partitioned {
    /// The partition-ordered copy of the matching rows, as output rows. Its
    /// buffer is sized exactly ([`Tuples::value_capacity`] is `len · arity`),
    /// whatever spare capacity the source has, so a trie that keeps it holds
    /// only the bytes its rows need.
    pub(crate) tuples: Tuples,
    /// `partitions + 1` prefix sums of the partition sizes: `offsets[0]` is
    /// 0 and the last is the number of matching rows.
    pub(crate) offsets: Vec<usize>,
    /// `origin[r]` is the source row id of the copy's row `r`: its position
    /// in the view's source, which the rows the filter rejected leave gaps in.
    pub(crate) origin: Vec<RowId>,
}

impl Partitioned {
    /// How many partitions the rows were copied into.
    pub(crate) fn partitions(&self) -> usize { self.offsets.len() - 1 }

    /// Partition `p`'s rows in the copy: consecutive ids, whose rows keep
    /// input order.
    pub(crate) fn ids(&self, p: usize) -> Range<RowId> {
        row_id(self.offsets[p])..row_id(self.offsets[p + 1])
    }
}

/// The unwritten front of one range of [`partition_rows`]' copy: its values
/// and the matching `origin` slots. Ranges are disjoint `&mut` borrows of
/// the two buffers, so workers write them without a lock.
struct Cursor<'a> {
    values: &'a mut [usize],
    origin: &'a mut [RowId],
}

impl<'a> Cursor<'a> {
    /// Cuts the next `rows` rows of `arity` values off the front, as a range
    /// of their own.
    fn split_off_front(&mut self, rows: usize, arity: usize) -> Cursor<'a> {
        // `mem::take` moves the `&'a mut` slice out, leaving `&mut []`, so both
        // halves keep `'a`, not the lifetime of this call's `&mut self`.
        let (values, rest) = mem::take(&mut self.values).split_at_mut(rows * arity);
        self.values = rest;
        let (origin, rest) = mem::take(&mut self.origin).split_at_mut(rows);
        self.origin = rest;
        Cursor {
            values,
            origin,
        }
    }

    /// Writes the output row of source row `id` at the front, straight from
    /// the view into the copy, with `id` into `origin` alongside, and moves
    /// the front past it.
    fn write(&mut self, view: &TupleView<'_>, id: RowId) {
        let (slot, rest) = mem::take(&mut self.origin)
            .split_first_mut()
            .expect("the histogram counted every row a morsel sends to this range");
        *slot = id;
        self.origin = rest;
        let (values, rest) = mem::take(&mut self.values).split_at_mut(view.arity());
        view.write(id, values);
        self.values = rest;
    }
}

/// Copies the rows `view` selects into partition order: partition `p` holds,
/// in source order, the output rows that `partition_of` sends to `p`, which
/// must be below `partitions`, and the partitions follow one another in
/// order. A classic histogram-and-scatter radix partition, in three steps:
///
/// 1. **Histogram.** `threads` workers take morsels of `morsel_rows`
///    consecutive source rows from a shared queue, as in [`scatter_rows`]. For
///    each row of a morsel that the view matches they write its output row into
///    a scratch row, call `partition_of` on it once, keep the row's partition
///    number, and count the morsel's matching rows per partition. A row the
///    filter rejects is skipped.
/// 2. **Offsets.** The counts add up to the copy's row count, and the copy and
///    `origin` are allocated once, at exactly that size. Prefix sums over
///    (partition, morsel) pairs, partition by partition, give every pair its
///    own range of the copy: a partition's ranges lie side by side, in morsel
///    order.
/// 3. **Scatter.** The copy and `origin` are cut into those ranges with
///    `split_at_mut`, grouped by morsel. Workers take the morsels again, each
///    with its ranges and its matching rows' partition numbers, and write every
///    matching row's output columns at the front of its partition's range
///    (`TupleView::write`), its source row id into `origin` alongside. The
///    ranges are disjoint, so no write needs a lock or an atomic.
///
/// `partition_of` runs exactly once per matching row, and never for a row the
/// filter rejects: the scatter reads the numbers the histogram kept (2 bytes a
/// row, freed as each morsel is scattered) instead of calling it again. A
/// build whose `partition_of` hashes therefore hashes each row once here, as
/// it did when it partitioned ids. The scatter does ask the view which rows
/// match a second time, in the same order, to pair the rows with the numbers.
///
/// The copy does not depend on the thread count or on which worker took which
/// morsel: a partition's ranges are in morsel order, and a morsel's rows are
/// written in source order. Its buffer is sized exactly, `len · arity` values,
/// whatever spare capacity the source has. Any arity works, nullary included.
///
/// It starts workers exactly twice, one `run_workers` call per pass
/// (histogram, then scatter), each on `threads` workers, even for a view that
/// matches nothing; a caller that must start none checks for that first.
///
/// Two costs are known and accepted:
///
/// - **The zero-fill.** The copy starts zeroed, since safe Rust cannot hand out
///   uninitialised `&mut` ranges. `vec!` asks the allocator for zeroed memory
///   (`alloc_zeroed`), which costs no serial memset when the allocator hands
///   out fresh pages, as it does for a large first allocation (a reused
///   allocation may still be zeroed by the allocator), and the pages'
///   first-touch faults land in the parallel scatter.
/// - **The bookkeeping** is one count and one range per (partition, morsel)
///   pair. The `radix:K` build, a one-thread caller, gains nothing from several
///   morsels and passes the whole source as one (`view.source_rows()`): the
///   classic one-thread radix partition, one histogram and one scatter;
///   `parallel:1` and `presized:1` keep `MORSEL_TUPLES`-row morsels.
///
/// # Panics
///
/// Panics if `morsel_rows` is zero, if `partitions` exceeds 2¹⁶ (65 536, the
/// most any build uses: `radix:16`), or if `partition_of` returns
/// `partitions` or more, or panics itself.
pub(crate) fn partition_rows(
    threads: Threads, view: &TupleView<'_>, morsel_rows: usize, partitions: usize,
    partition_of: impl Fn(&[usize]) -> usize + Sync,
) -> Partitioned {
    assert!(
        morsel_rows > 0,
        "partition_rows: a morsel holds at least one row"
    );
    // A row's partition number is kept as a `u16` between the two passes, as
    // the radix build kept it before it copied the rows (#101).
    assert!(
        partitions <= 1 << 16,
        "partition_rows: at most 65,536 partitions, got {partitions}"
    );
    let (source_rows, arity) = (view.source_rows(), view.arity());
    let identity = view.is_identity();
    let morsels = source_rows.div_ceil(morsel_rows);
    let morsel_ids = |m: usize| {
        let first = m * morsel_rows;
        row_id(first)..row_id(first + morsel_rows.min(source_rows - first))
    };

    // 1. Histogram: `counts[m][p]` of morsel `m`'s matching rows go to
    //    partition `p`, and `partition_numbers[m]` holds each of them
    //    partition, in row order.
    let queue = Mutex::new(0..morsels);
    let mut counts: Vec<Vec<usize>> = vec![Vec::new(); morsels];
    let mut partition_numbers: Vec<Vec<u16>> = vec![Vec::new(); morsels];
    let counted = run_workers(threads, || {
        let mut counted = Vec::new();
        // One scratch row per worker: the output row `partition_of` sees.
        let mut output = vec![0; arity];
        while let Some(m) = take_next(&queue) {
            let ids = morsel_ids(m);
            let mut histogram = vec![0; partitions];
            // At most one number per row of the morsel, so it never grows.
            let mut morsel_numbers = Vec::with_capacity(ids.len());
            let mut tally = |p: usize| {
                assert!(
                    p < partitions,
                    "partition_rows: partition_of returned {p} for {partitions} partitions"
                );
                histogram[p] += 1;
                // Lossless: `p < partitions <= 2^16`, by the two assertions.
                morsel_numbers.push(p as u16);
            };
            // An identity view keeps every row and its source row is its
            // output row, so it pays no per-row filter or copy (Amendment 2
            // of the per-query index build spec): it is the build path of
            // every `from_tuples*` and of every constant-free index, and a
            // check there would cost only the radix family. The branch is
            // taken once; the two loops differ only in how a row is read,
            // and share `tally`.
            if identity {
                for id in ids {
                    tally(partition_of(view.source().row(id)));
                }
            } else {
                for id in ids {
                    if !view.matches(id) {
                        continue;
                    }
                    view.write(id, &mut output);
                    tally(partition_of(&output));
                }
            }
            counted.push((m, histogram, morsel_numbers));
        }
        counted
    });
    for (m, histogram, morsel_numbers) in counted.into_iter().flatten() {
        counts[m] = histogram;
        partition_numbers[m] = morsel_numbers;
    }

    // 2. Offsets, and the copy cut into one range per (partition, morsel) pair,
    //    partition by partition: `ranges[m][p]` is where morsel `m` writes its
    //    rows of partition `p`. The counts add up to the copy's row count.
    //    `vec!` of zeros allocates through `alloc_zeroed`, at exactly the
    //    copy's size, so the buffer has no spare capacity. `origin` is zeroed
    //    the same way.
    let len: usize = counts.iter().flatten().sum();
    let mut values = vec![0usize; len * arity];
    let mut origin: Vec<RowId> = vec![0; len];
    let mut offsets = Vec::with_capacity(partitions + 1);
    offsets.push(0);
    {
        let mut unassigned = Cursor {
            values: values.as_mut_slice(),
            origin: origin.as_mut_slice(),
        };
        let mut ranges: Vec<Vec<Cursor<'_>>> = (0..morsels)
            .map(|_| Vec::with_capacity(partitions))
            .collect();
        let mut end = 0;
        for p in 0..partitions {
            for (morsel_ranges, morsel_counts) in ranges.iter_mut().zip(&counts) {
                let rows = morsel_counts[p];
                morsel_ranges.push(unassigned.split_off_front(rows, arity));
                end += rows;
            }
            offsets.push(end);
        }
        debug_assert_eq!(end, len, "the histograms count every row once");

        // 3. Scatter: each worker takes a morsel with its ranges and its
        //    matching rows' partition numbers, and writes every matching row at
        //    the front of its partition's range. `partition_of` is not called
        //    again; the view is asked which rows match, in the order the
        //    histogram asked it.
        let queue = Mutex::new(ranges.into_iter().zip(partition_numbers).enumerate());
        run_workers(threads, || {
            while let Some((m, (mut ranges, morsel_numbers))) = take_next(&queue) {
                let mut numbers = morsel_numbers.into_iter();
                let mut send = |id: RowId| {
                    let p = numbers
                        .next()
                        .expect("the histogram numbered every matching row");
                    ranges[p as usize].write(view, id);
                };
                // As in the histogram: an identity view matches every row, so
                // it pays no per-row filter here either (Amendment 2). The
                // `write` is the build's one copy and stays on both paths.
                if identity {
                    morsel_ids(m).for_each(&mut send);
                } else {
                    morsel_ids(m)
                        .filter(|&id| view.matches(id))
                        .for_each(&mut send);
                }
            }
        });
        // The ranges borrow `values` and `origin` until this block ends.
    }
    Partitioned {
        tuples: Tuples::from_flat(arity, len, values),
        offsets,
        origin,
    }
}

/// Runs `task` on every item of `items` and returns the results in item
/// order. `threads` workers take the next item from a shared queue whenever
/// they are free.
///
/// # Panics
///
/// Re-raises a panicking `task`'s panic once every worker has stopped.
pub(crate) fn dispatch<T: Send, R: Send>(
    threads: Threads, items: Vec<T>, task: impl Fn(T) -> R + Sync,
) -> Vec<R> {
    let len = items.len();
    let queue = Mutex::new(items.into_iter().enumerate());
    let done = run_workers(threads, || {
        let mut done = Vec::new();
        while let Some((index, item)) = take_next(&queue) {
            done.push((index, task(item)));
        }
        done
    });
    let mut results: Vec<Option<R>> = (0..len).map(|_| None).collect();
    for (index, result) in done.into_iter().flatten() {
        results[index] = Some(result);
    }
    results
        .into_iter()
        .map(|result| result.expect("the queue hands out every item exactly once"))
        .collect()
}

#[cfg(test)]
mod tests {
    use {super::*, kermit_iters::Filter};

    fn threads(n: usize) -> Threads { Threads::new(n).expect("tests use a nonzero count") }

    /// The tests written for batches keep their call shape: the whole batch,
    /// unfiltered. This shadows the glob-imported `partition_rows`; the tests
    /// of views call `super::partition_rows` by path.
    fn partition_rows(
        threads: Threads, input: &Tuples, morsel_rows: usize, partitions: usize,
        partition_of: impl Fn(&[usize]) -> usize + Sync,
    ) -> Partitioned {
        super::partition_rows(
            threads,
            &TupleView::all(input),
            morsel_rows,
            partitions,
            partition_of,
        )
    }

    #[test]
    fn threads_accepts_only_one_to_max() {
        assert_eq!(Threads::new(0), None);
        assert_eq!(Threads::new(Threads::MAX + 1), None);
        assert_eq!(threads(3).get(), 3);
        assert_eq!(threads(Threads::MAX).get(), Threads::MAX);
    }

    /// `n` rows `[i % 7, i]`, as one batch.
    fn numbered_rows(n: usize) -> Tuples {
        let mut tuples = Tuples::with_capacity(2, n);
        for i in 0..n {
            tuples.push(&[i % 7, i]);
        }
        tuples
    }

    /// Every row's id lands exactly once, in the partition `partition_of`
    /// names, and each partition lists its ids in input order, whatever the
    /// thread count and however the morsels divide the input.
    #[test]
    fn scatter_rows_keeps_every_row_once_in_input_order() {
        let n = 1000;
        let input = numbered_rows(n);
        let view = TupleView::all(&input);
        for t in [1, 2, 3, 8] {
            for morsel in [1, 4, 5, n, n + 1] {
                let case = format!("threads {t}, morsel {morsel}");
                let partitions = scatter_rows(threads(t), &view, morsel, 3, |row| row[0] % 3);
                assert_eq!(partitions.len(), 3, "{case}");
                let mut seen: Vec<RowId> = Vec::new();
                for (p, partition) in partitions.iter().enumerate() {
                    let ids: Vec<RowId> = partition.ids().collect();
                    assert_eq!(partition.len(), ids.len(), "{case}: partition {p} length");
                    assert!(
                        ids.windows(2).all(|pair| pair[0] < pair[1]),
                        "{case}: partition {p} is out of input order"
                    );
                    for &id in &ids {
                        assert_eq!(input.row(id)[0] % 3, p, "{case}: row {id} in partition {p}");
                    }
                    seen.extend(ids);
                }
                seen.sort_unstable();
                let every: Vec<RowId> = (0..).take(n).collect();
                assert_eq!(seen, every, "{case}: every row exactly once");
            }
        }
    }

    /// A partition holds one segment per morsel that sent it any row, in
    /// morsel order, and no empty segment. Ten rows in morsels of 3 are rows
    /// 0–2, 3–5, 6–8 and 9; split by parity, the last morsel sends the even
    /// partition nothing.
    #[test]
    fn scatter_rows_cuts_one_segment_per_contributing_morsel() {
        let input = numbered_rows(10);
        let view = TupleView::all(&input);
        let partitions = scatter_rows(threads(2), &view, 3, 2, |row| row[1] % 2);
        assert_eq!(partitions[0].segments, vec![vec![0, 2], vec![4], vec![
            6, 8
        ]]);
        assert_eq!(partitions[1].segments, vec![
            vec![1],
            vec![3, 5],
            vec![7],
            vec![9]
        ]);
    }

    /// A nullary row is an empty slice but still has an id, so a batch of
    /// them scatters like any other.
    #[test]
    fn scatter_rows_sends_nullary_rows_by_id() {
        let mut input = Tuples::new(0);
        for _ in 0..5 {
            input.push(&[]);
        }
        let view = TupleView::all(&input);
        let partitions = scatter_rows(threads(2), &view, 2, 2, |row| row.len());
        assert_eq!(partitions[0].ids().collect::<Vec<_>>(), vec![0, 1, 2, 3, 4]);
        assert_eq!(partitions[1].len(), 0);
    }

    /// A filtered, permuted view scatters the *source* ids of the rows it
    /// keeps, in input order, and `partition_of` sees the view's *output*
    /// rows: with columns `[1, 0]` the output row is `[i, i % 7]`, so
    /// `row[0] % 3` is the id's own residue and every id's partition can be
    /// checked from the id alone. Rows the filter rejects (`i % 7 != 3`) are
    /// never sent anywhere.
    #[test]
    fn scatter_rows_sends_the_source_ids_of_a_views_matching_rows() {
        let n = 1000;
        let input = numbered_rows(n); // rows `[i % 7, i]`
        let view = TupleView::new(&input, Filter::new(vec![(0, 3)], vec![]), vec![1, 0]);
        let kept: Vec<RowId> = (0..).take(n).filter(|id| id % 7 == 3).collect();
        assert!(!kept.is_empty());
        for t in [1, 2, 3, 8] {
            for morsel in [1, 4, 5, n, n + 1] {
                let case = format!("threads {t}, morsel {morsel}");
                let partitions = scatter_rows(threads(t), &view, morsel, 3, |row| row[0] % 3);
                let mut seen: Vec<RowId> = Vec::new();
                for (p, partition) in partitions.iter().enumerate() {
                    let ids: Vec<RowId> = partition.ids().collect();
                    assert!(
                        ids.windows(2).all(|pair| pair[0] < pair[1]),
                        "{case}: partition {p} is out of input order"
                    );
                    for &id in &ids {
                        assert_eq!(id as usize % 3, p, "{case}: id {id} in partition {p}");
                    }
                    seen.extend(ids);
                }
                seen.sort_unstable();
                assert_eq!(seen, kept, "{case}: exactly the matching ids, once each");
            }
        }
    }

    /// An unfiltered view that permutes its columns is not an identity view:
    /// `partition_of` must see the *output* row (`[i, i % 7]` with columns
    /// `[1, 0]`), not the source row, so every id's partition is its own
    /// residue, and every id is sent once.
    #[test]
    fn scatter_rows_reads_the_output_row_of_an_unfiltered_permuted_view() {
        let n = 1000;
        let input = numbered_rows(n); // rows `[i % 7, i]`
        let view = TupleView::new(&input, Filter::none(), vec![1, 0]);
        assert!(!view.is_identity(), "the premise");
        let all: Vec<RowId> = (0..).take(n).collect();
        for t in [1, 2, 3, 8] {
            for morsel in [1, 4, 5, n, n + 1] {
                let case = format!("threads {t}, morsel {morsel}");
                let partitions = scatter_rows(threads(t), &view, morsel, 3, |row| row[0] % 3);
                let mut seen: Vec<RowId> = Vec::new();
                for (p, partition) in partitions.iter().enumerate() {
                    let ids: Vec<RowId> = partition.ids().collect();
                    for &id in &ids {
                        assert_eq!(id as usize % 3, p, "{case}: id {id} in partition {p}");
                    }
                    seen.extend(ids);
                }
                seen.sort_unstable();
                assert_eq!(seen, all, "{case}: every id, once");
            }
        }
    }

    /// An equality filter keeps the rows whose two columns agree, and a view
    /// without output columns is legal: `partition_of` sees an empty row,
    /// and the ids are still the matching source ids. Rows `[i % 4, i % 2]`
    /// agree for `i % 4` in `{0, 1}`.
    #[test]
    fn scatter_rows_applies_equalities_and_accepts_a_view_without_columns() {
        let mut input = Tuples::with_capacity(2, 40);
        for i in 0..40 {
            input.push(&[i % 4, i % 2]);
        }
        let kept: Vec<RowId> = (0..).take(40).filter(|id| id % 4 < 2).collect();
        let equal = Filter::new(vec![], vec![(0, 1)]);

        let both = TupleView::new(&input, equal.clone(), vec![0, 1]);
        let partitions = scatter_rows(threads(2), &both, 3, 1, |_| 0);
        assert_eq!(partitions[0].ids().collect::<Vec<_>>(), kept);

        let nullary = TupleView::new(&input, equal, vec![]);
        let partitions = scatter_rows(threads(2), &nullary, 3, 2, |row| row.len());
        assert_eq!(partitions[0].ids().collect::<Vec<_>>(), kept);
        assert_eq!(partitions[1].len(), 0);
    }

    /// `n` rows of `arity` values (at most 3): `[i % 7, i, n - i]`, cut to
    /// `arity`. At arity 1 rows repeat; at arity 3 every row is distinct.
    fn rows_of_arity(n: usize, arity: usize) -> Tuples {
        let mut tuples = Tuples::with_capacity(arity, n);
        for i in 0..n {
            tuples.push(&[i % 7, i, n - i][..arity]);
        }
        tuples
    }

    /// Partitions in the `partition_rows` tests. [`first_mod_3`] never sends
    /// a row to the last one, so an empty partition is always among them.
    const PARTITIONS: usize = 4;

    /// The partition the `partition_rows` tests send a row to.
    fn first_mod_3(row: &[usize]) -> usize { row[0] % 3 }

    /// `partition_rows` at arities 1 and 3, on 1 to 8 threads, with morsels
    /// from one row to more than the whole batch: each case's label, input
    /// and output.
    fn partitioned_cases() -> Vec<(String, Tuples, Partitioned)> {
        let n = 100;
        let mut cases = Vec::new();
        for arity in [1, 3] {
            let input = rows_of_arity(n, arity);
            for t in [1, 2, 3, 8] {
                for morsel in [1, 3, 5, n, n + 1] {
                    let out = partition_rows(threads(t), &input, morsel, PARTITIONS, first_mod_3);
                    let case = format!("arity {arity}, threads {t}, morsel {morsel}");
                    cases.push((case, input.clone(), out));
                }
            }
        }
        cases
    }

    /// `offsets` are the prefix sums of the partition sizes, the empty last
    /// partition included, and the copy and `origin` hold every row.
    #[test]
    fn partition_rows_offsets_are_the_prefix_sums_of_the_partition_sizes() {
        for (case, input, out) in partitioned_cases() {
            let mut expected = vec![0];
            let mut end = 0;
            for p in 0..PARTITIONS {
                end += input.rows().filter(|&row| first_mod_3(row) == p).count();
                expected.push(end);
            }
            assert_eq!(out.offsets, expected, "{case}");
            assert_eq!(out.partitions(), PARTITIONS, "{case}");
            assert_eq!(out.tuples.arity(), input.arity(), "{case}");
            assert_eq!(out.tuples.len(), input.len(), "{case}");
            assert_eq!(out.origin.len(), input.len(), "{case}");
        }
    }

    /// Each copied row lies in the range of the partition `partition_of`
    /// names. With the sizes above and `origin` a permutation, partition
    /// `p` holds exactly the rows sent to `p`.
    #[test]
    fn partition_rows_puts_each_row_in_its_partition() {
        for (case, _, out) in partitioned_cases() {
            for p in 0..out.partitions() {
                for id in out.ids(p) {
                    assert_eq!(first_mod_3(out.tuples.row(id)), p, "{case}: row {id}");
                }
            }
        }
    }

    /// Within a partition, rows keep input order: their input positions
    /// ascend, across morsels as within one.
    #[test]
    fn partition_rows_keeps_input_order_within_a_partition() {
        for (case, _, out) in partitioned_cases() {
            for (p, bounds) in out.offsets.windows(2).enumerate() {
                let from = &out.origin[bounds[0]..bounds[1]];
                assert!(
                    from.windows(2).all(|pair| pair[0] < pair[1]),
                    "{case}: partition {p} is out of input order: {from:?}"
                );
            }
        }
    }

    /// `origin` maps each copied row to the input row it was copied from,
    /// and names every input position exactly once.
    #[test]
    fn partition_rows_origin_maps_each_copied_row_to_its_input_row() {
        for (case, input, out) in partitioned_cases() {
            for (id, &from) in (0..).zip(&out.origin) {
                assert_eq!(out.tuples.row(id), input.row(from), "{case}: row {id}");
            }
            let mut positions = out.origin.clone();
            positions.sort_unstable();
            let every: Vec<RowId> = (0..).take(input.len()).collect();
            assert_eq!(positions, every, "{case}: every input row exactly once");
        }
    }

    /// The copy is the same, byte for byte, on every thread count and with
    /// every morsel size: which worker took which morsel cannot show.
    #[test]
    fn partition_rows_is_the_same_on_every_thread_count() {
        for arity in [1, 3] {
            let input = rows_of_arity(100, arity);
            let one = partition_rows(threads(1), &input, 3, PARTITIONS, first_mod_3);
            for t in [2, 3, 8] {
                for morsel in [1, 3, 7, 101] {
                    assert_eq!(
                        partition_rows(threads(t), &input, morsel, PARTITIONS, first_mod_3),
                        one,
                        "arity {arity}, threads {t}, morsel {morsel}"
                    );
                }
            }
        }
    }

    /// `partition_of` runs exactly once per row: the histogram pass keeps
    /// each row's partition number and the scatter reads it back, so a
    /// caller that hashes in `partition_of` hashes each row once here.
    #[test]
    fn partition_rows_calls_partition_of_once_per_row() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let input = rows_of_arity(100, 3);
        for t in [1, 3] {
            for morsel in [1, 7, 101] {
                let calls = AtomicUsize::new(0);
                let out = partition_rows(threads(t), &input, morsel, PARTITIONS, |row| {
                    calls.fetch_add(1, Ordering::Relaxed);
                    first_mod_3(row)
                });
                assert_eq!(calls.into_inner(), 100, "threads {t}, morsel {morsel}");
                assert_eq!(out.offsets[PARTITIONS], 100, "threads {t}, morsel {morsel}");
            }
        }
    }

    /// Two worker runs, the histogram then the scatter, each on `threads`
    /// workers, an empty batch included. The radix family's worker-run pins
    /// (`build_modes_reach_their_builds`, `presized_build_reaches_its_own_path`
    /// and `parallel_build_merges_under_every_root_capacity` in
    /// `hash_trie/parallel.rs`) count on it.
    #[test]
    fn partition_rows_starts_workers_twice() {
        for t in [1, 3] {
            take_worker_runs();
            let _ = partition_rows(
                threads(t),
                &rows_of_arity(10, 2),
                4,
                PARTITIONS,
                first_mod_3,
            );
            assert_eq!(take_worker_runs(), vec![t, t], "threads {t}");
            let _ = partition_rows(threads(t), &Tuples::new(2), 4, PARTITIONS, first_mod_3);
            assert_eq!(take_worker_runs(), vec![t, t], "threads {t}, empty batch");
        }
    }

    #[test]
    fn partition_rows_of_nothing_is_empty() {
        let out = partition_rows(threads(4), &Tuples::new(2), 8, 3, |_| 0);
        assert_eq!(out.offsets, vec![0, 0, 0, 0]);
        assert_eq!((out.tuples.arity(), out.tuples.len()), (2, 0));
        assert!(out.origin.is_empty());
        assert!((0..3).all(|p| out.ids(p).is_empty()));
    }

    /// The copy is sized exactly: `len · arity` values, whatever spare
    /// capacity the source has, so a trie that keeps the copy holds only the
    /// bytes its rows need and `space` does not depend on how the source was
    /// built.
    #[test]
    fn partition_rows_sizes_the_copy_exactly() {
        let mut input = Tuples::with_capacity(3, 40);
        for i in 0..10 {
            input.push(&[i, i, i]);
        }
        assert!(
            input.value_capacity() > input.as_flat().len(),
            "the input has spare capacity"
        );
        for t in [1, 3] {
            let out = partition_rows(threads(t), &input, 4, PARTITIONS, first_mod_3);
            assert_eq!(out.tuples.value_capacity(), 30, "threads {t}");
        }
        let empty = Tuples::with_capacity(2, 5);
        let out = partition_rows(threads(2), &empty, 4, PARTITIONS, first_mod_3);
        assert_eq!(out.tuples.value_capacity(), 0);
    }

    /// Over `rows_of_arity(100, 3)`, the rows `[i % 7, i, 100 - i]`: this view
    /// keeps the 14 rows with `i % 7 == 3`, read as columns `[2, 1]`, so its
    /// output rows are `[100 - i, i]` and `first_mod_3` sees `100 - i`.
    fn matching_view(source: &Tuples) -> TupleView<'_> {
        TupleView::new(
            source,
            kermit_iters::Filter::new(vec![(0, 3)], vec![]),
            vec![2, 1],
        )
    }

    /// What `partition_rows` must make of a view, worked out from the source
    /// rows alone: the output rows, their source row ids and the partition
    /// offsets, partition by partition, each in source order. `keeps` is the
    /// filter's predicate and `columns` the view's.
    fn expected_copy(
        source: &Tuples, keeps: impl Fn(&[usize]) -> bool, columns: &[usize],
    ) -> (Vec<Vec<usize>>, Vec<RowId>, Vec<usize>) {
        let (mut rows, mut origin, mut offsets) = (Vec::new(), Vec::new(), vec![0]);
        for p in 0..PARTITIONS {
            for (id, row) in (0..).zip(source.rows()) {
                let output: Vec<usize> = columns.iter().map(|&column| row[column]).collect();
                if keeps(row) && first_mod_3(&output) == p {
                    rows.push(output);
                    origin.push(id);
                }
            }
            offsets.push(rows.len());
        }
        (rows, origin, offsets)
    }

    /// The copy holds only the matching rows, as output rows (the view's
    /// columns, in its order), in partition order, each partition in source
    /// order; `origin` names source rows, with gaps where the filter
    /// rejected; and the copy is sized for its 14 rows exactly. The filter
    /// cuts across morsel boundaries at every size below.
    #[test]
    fn partition_rows_copies_the_matching_rows_in_output_order() {
        let source = rows_of_arity(100, 3);
        let (rows, origin, offsets) = expected_copy(&source, |row| row[0] == 3, &[2, 1]);
        // The premise: 14 rows match, in partitions of 5, 5, 4 and 0.
        assert_eq!(offsets, vec![0, 5, 10, 14, 14]);
        for t in [1, 2, 3, 8] {
            for morsel in [1, 3, 5, 100, 101] {
                let case = format!("threads {t}, morsel {morsel}");
                let out = super::partition_rows(
                    threads(t),
                    &matching_view(&source),
                    morsel,
                    PARTITIONS,
                    first_mod_3,
                );
                assert_eq!(out.offsets, offsets, "{case}");
                assert_eq!(out.tuples.to_vecs(), rows, "{case}");
                assert_eq!(out.origin, origin, "{case}");
                assert_eq!(out.tuples.arity(), 2, "{case}");
                assert_eq!(out.tuples.value_capacity(), 14 * 2, "{case}: exact size");
            }
        }
    }

    /// An unfiltered view that permutes its columns is not an identity view:
    /// the copy holds every row as the output row `[100 - i, i]`, and
    /// `first_mod_3` partitions on `100 - i`, not on the source's `i % 7`.
    #[test]
    fn partition_rows_copies_an_unfiltered_permuted_view_in_output_order() {
        let source = rows_of_arity(100, 3);
        let view = TupleView::new(&source, kermit_iters::Filter::none(), vec![2, 1]);
        assert!(!view.is_identity(), "the premise");
        let (rows, origin, offsets) = expected_copy(&source, |_| true, &[2, 1]);
        assert_eq!(offsets[PARTITIONS], 100, "the premise: every row is kept");
        for t in [1, 2, 3, 8] {
            for morsel in [1, 3, 5, 100, 101] {
                let case = format!("threads {t}, morsel {morsel}");
                let out = super::partition_rows(threads(t), &view, morsel, PARTITIONS, first_mod_3);
                assert_eq!(out.offsets, offsets, "{case}");
                assert_eq!(out.tuples.to_vecs(), rows, "{case}");
                assert_eq!(out.origin, origin, "{case}");
                assert_eq!(out.tuples.value_capacity(), 100 * 2, "{case}: exact size");
            }
        }
    }

    /// An equality filter and a projection to one column: `col0 = col1` holds
    /// only for `i < 7` (`i % 7 == i`), whose output rows are `[i]`.
    #[test]
    fn partition_rows_reads_an_equality_filter_and_a_projection() {
        let source = rows_of_arity(100, 3);
        let view = TupleView::new(
            &source,
            kermit_iters::Filter::new(vec![], vec![(0, 1)]),
            vec![1],
        );
        let (rows, origin, offsets) = expected_copy(&source, |row| row[0] == row[1], &[1]);
        assert_eq!(offsets, vec![0, 3, 5, 7, 7], "the premise: 7 rows match");
        for t in [1, 3] {
            for morsel in [1, 2, 3, 100] {
                let case = format!("threads {t}, morsel {morsel}");
                let out = super::partition_rows(threads(t), &view, morsel, PARTITIONS, first_mod_3);
                assert_eq!(out.offsets, offsets, "{case}");
                assert_eq!(out.tuples.to_vecs(), rows, "{case}");
                assert_eq!(out.origin, origin, "{case}");
            }
        }
    }

    /// `partition_of` runs once per matching row, on the output row, and
    /// never for a row the filter rejects.
    #[test]
    fn partition_rows_calls_partition_of_once_per_matching_row() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let source = rows_of_arity(100, 3);
        for t in [1, 3] {
            for morsel in [1, 7, 101] {
                let calls = AtomicUsize::new(0);
                let out = super::partition_rows(
                    threads(t),
                    &matching_view(&source),
                    morsel,
                    PARTITIONS,
                    |row| {
                        assert_eq!(row.len(), 2, "partition_of sees the output row");
                        calls.fetch_add(1, Ordering::Relaxed);
                        first_mod_3(row)
                    },
                );
                assert_eq!(calls.into_inner(), 14, "threads {t}, morsel {morsel}");
                assert_eq!(out.offsets[PARTITIONS], 14, "threads {t}, morsel {morsel}");
            }
        }
    }

    /// A view that matches nothing copies nothing, however many source rows
    /// it rejects, and still runs both passes (see `partition_rows`).
    #[test]
    fn partition_rows_of_a_view_that_matches_nothing_copies_nothing() {
        let source = rows_of_arity(10, 3);
        let view = TupleView::new(
            &source,
            kermit_iters::Filter::new(vec![(0, 99)], vec![]),
            vec![0, 1, 2],
        );
        for t in [1, 3] {
            take_worker_runs();
            let out = super::partition_rows(threads(t), &view, 4, PARTITIONS, first_mod_3);
            assert_eq!(out.offsets, vec![0; PARTITIONS + 1], "threads {t}");
            assert_eq!(
                (
                    out.tuples.arity(),
                    out.tuples.len(),
                    out.tuples.value_capacity()
                ),
                (3, 0, 0),
                "threads {t}"
            );
            assert!(out.origin.is_empty(), "threads {t}");
            assert_eq!(take_worker_runs(), vec![t, t], "threads {t}");
        }
    }

    /// With one partition the copy is the input, `origin` is the identity
    /// and the partition is every row.
    #[test]
    fn one_partition_is_the_input_order() {
        for arity in [1, 3] {
            let input = rows_of_arity(50, arity);
            for t in [1, 3] {
                let case = format!("arity {arity}, threads {t}");
                let out = partition_rows(threads(t), &input, 7, 1, |_| 0);
                assert_eq!(out.tuples, input, "{case}");
                assert_eq!(out.origin, (0..50).collect::<Vec<RowId>>(), "{case}");
                assert_eq!(out.offsets, vec![0, 50], "{case}");
                assert_eq!(out.ids(0), 0..50, "{case}");
            }
        }
    }

    /// A nullary row is an empty slice but still has an input position, so
    /// a batch of them partitions like any other.
    #[test]
    fn partition_rows_copies_nullary_rows_by_position() {
        let input = Tuples::from_flat(0, 5, Vec::new());
        let out = partition_rows(threads(2), &input, 2, 2, |row| row.len());
        assert_eq!((out.tuples.arity(), out.tuples.len()), (0, 5));
        assert_eq!(out.offsets, vec![0, 5, 5]);
        assert_eq!(out.origin, vec![0, 1, 2, 3, 4]);
    }

    #[test]
    #[should_panic(expected = "partition_rows: a morsel holds at least one row")]
    fn partition_rows_rejects_empty_morsels() {
        let _ = partition_rows(threads(1), &rows_of_arity(4, 1), 0, PARTITIONS, first_mod_3);
    }

    /// Partition numbers are kept as `u16`s, so 2¹⁶ partitions is the most
    /// it takes. The check runs first, an empty batch included.
    #[test]
    #[should_panic(expected = "partition_rows: at most 65,536 partitions, got 65537")]
    fn partition_rows_rejects_more_than_2_16_partitions() {
        let _ = partition_rows(threads(1), &Tuples::new(2), 4, (1 << 16) + 1, |_| 0);
    }

    #[test]
    #[should_panic(expected = "partition_rows: partition_of returned 2 for 2 partitions")]
    fn partition_rows_rejects_a_partition_out_of_range() {
        let _ = partition_rows(threads(1), &rows_of_arity(4, 1), 4, 2, |_| 2);
    }

    #[test]
    fn dispatch_returns_results_in_item_order() {
        let n = if cfg!(miri) {
            9
        } else {
            200
        };
        for t in [1, 2, 3, 8] {
            let items: Vec<usize> = (0..n).collect();
            let expected: Vec<usize> = (0..n).map(|i| i * i).collect();
            assert_eq!(
                dispatch(threads(t), items, |i| i * i),
                expected,
                "threads {t}"
            );
        }
    }

    /// The workers really run at once: the first task can finish only after
    /// another worker has started the second. A single worker would wait
    /// out the timeout and return `false`.
    #[test]
    #[cfg_attr(miri, ignore = "waits on a wall-clock timeout")]
    fn dispatch_runs_tasks_on_more_than_one_thread() {
        let (sender, receiver) = std::sync::mpsc::channel();
        let sender = Mutex::new(sender);
        let receiver = Mutex::new(receiver);
        let met = dispatch(threads(2), vec![0, 1], |i: usize| {
            if i == 0 {
                receiver
                    .lock()
                    .unwrap()
                    .recv_timeout(std::time::Duration::from_secs(10))
                    .is_ok()
            } else {
                sender.lock().unwrap().send(()).unwrap();
                true
            }
        });
        assert_eq!(met, vec![true, true]);
    }

    #[test]
    fn empty_input_needs_no_work() {
        assert!(dispatch(threads(4), Vec::<usize>::new(), |i| i).is_empty());
        let empty = Tuples::new(2);
        let partitions = scatter_rows(threads(4), &TupleView::all(&empty), 8, 3, |_| 0);
        assert_eq!(partitions.len(), 3);
        assert!(partitions.iter().all(|partition| partition.len() == 0));
    }

    #[test]
    #[should_panic(expected = "task failed")]
    fn a_workers_panic_reaches_the_caller() {
        let _results: Vec<usize> = dispatch(threads(3), (0..30).collect::<Vec<usize>>(), |i| {
            if i == 17 {
                panic!("task failed");
            }
            i
        });
    }

    /// A panic on a helper thread reaches the caller with its own message.
    /// The two items rendezvous, so they run on different workers, and only
    /// the one on the helper panics.
    #[test]
    #[should_panic(expected = "task failed")]
    #[cfg_attr(miri, ignore = "waits on a wall-clock timeout")]
    fn a_helpers_panic_reaches_the_caller() {
        let caller = std::thread::current().id();
        let (sender, receiver) = std::sync::mpsc::channel();
        let sender = Mutex::new(sender);
        let receiver = Mutex::new(receiver);
        let _results: Vec<()> = dispatch(threads(2), vec![0, 1], |i: usize| {
            if i == 0 {
                let _ = receiver
                    .lock()
                    .unwrap()
                    .recv_timeout(std::time::Duration::from_secs(10));
            } else {
                sender.lock().unwrap().send(()).unwrap();
            }
            if std::thread::current().id() != caller {
                panic!("task failed");
            }
        });
    }
}
