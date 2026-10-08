//! Morsel-driven work distribution for the parallel builds (Leis et al.,
//! *Morsel-Driven Parallelism*, SIGMOD 2014).
//!
//! The input is cut into small fixed-size *morsels*. The *dispatcher*, a
//! mutex-guarded queue, hands the next unit of work to whichever worker is
//! free, so a slow worker never stalls the rest. A build uses it twice:
//!
//! 1. [`scatter`] moves tuples into partitions, one morsel at a time
//!    ([`scatter_rows`] sends the ids of a [`Tuples`] batch's rows instead,
//!    leaving the batch where it is);
//! 2. [`dispatch`] then runs one task per partition.
//!
//! Neither result depends on scheduling. A partition lists its tuples in
//! input order, and task results come back in task order, so a parallel
//! build can reproduce its serial counterpart exactly
//! (`docs/data-structures/parallel-build.md`).
//!
//! The workers are [`std::thread::scope`] threads, the calling thread among
//! them. Every worker is joined before a call returns, and a worker's panic
//! is re-raised in the caller.

use {
    kermit_iters::{RowId, Tuples},
    std::{num::NonZeroUsize, panic, sync::Mutex, thread},
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

/// A tuple and its position in the input.
pub(crate) type Positioned = (usize, Vec<usize>);

/// One morsel's tuples, split by partition.
type Buckets = Vec<Vec<Positioned>>;

/// The tuples [`scatter`] sent to one partition: one segment per morsel that
/// contributed any, in morsel order. Reading the segments in order reads the
/// partition in input order.
#[derive(Debug, Default)]
pub(crate) struct Partition {
    segments: Vec<Vec<Positioned>>,
}

impl Partition {
    /// How many tuples the partition holds.
    pub(crate) fn len(&self) -> usize { self.segments.iter().map(Vec::len).sum() }

    /// The partition's tuples in input order, each with its input position.
    pub(crate) fn into_tuples(self) -> impl Iterator<Item = Positioned> {
        self.segments.into_iter().flatten()
    }
}

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

/// Moves every tuple of `tuples` into partition `partition_of(tuple)`, which
/// must be below `partitions`.
///
/// `threads` workers take morsels of `morsel_tuples` tuples from a shared
/// queue, so a worker that finishes early takes more. Every partition lists
/// its tuples in input order, whatever order the morsels ran in. Tuples are
/// moved, not copied: each keeps its heap buffer and its capacity.
///
/// # Panics
///
/// Panics if `morsel_tuples` is zero, or if `partition_of` returns
/// `partitions` or more, or panics itself.
pub(crate) fn scatter(
    threads: Threads, mut tuples: Vec<Vec<usize>>, morsel_tuples: usize, partitions: usize,
    partition_of: impl Fn(&[usize]) -> usize + Sync,
) -> Vec<Partition> {
    let queue = Mutex::new(tuples.chunks_mut(morsel_tuples).enumerate());
    let mut morsels: Vec<(usize, Buckets)> = run_workers(threads, || {
        let mut scattered = Vec::new();
        while let Some((index, morsel)) = take_next(&queue) {
            let first = index * morsel_tuples;
            let mut buckets: Buckets = (0..partitions).map(|_| Vec::new()).collect();
            for (offset, slot) in morsel.iter_mut().enumerate() {
                let tuple = std::mem::take(slot);
                let partition = partition_of(&tuple);
                buckets[partition].push((first + offset, tuple));
            }
            scattered.push((index, buckets));
        }
        scattered
    })
    .into_iter()
    .flatten()
    .collect();
    morsels.sort_unstable_by_key(|&(index, _)| index);

    let mut out: Vec<Partition> = (0..partitions).map(|_| Partition::default()).collect();
    for (_, buckets) in morsels {
        for (partition, bucket) in out.iter_mut().zip(buckets) {
            if !bucket.is_empty() {
                partition.segments.push(bucket);
            }
        }
    }
    out
}

/// The row ids [`scatter_rows`] sent to one partition: one segment per
/// morsel that contributed any, in morsel order. Reading the segments in
/// order reads the partition in input order, so its ids ascend.
#[derive(Debug, Default)]
pub(crate) struct RowPartition {
    pub(crate) segments: Vec<Vec<RowId>>,
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

/// Sends the id of every row of `tuples` to partition `partition_of(row)`,
/// which must be below `partitions`. The batch stays where it is: workers
/// read its rows through `&Tuples`, and only 4-byte ids move.
///
/// `threads` workers take morsels of `morsel_rows` consecutive rows from a
/// shared queue, so a worker that finishes early takes more. Every
/// partition lists its ids in input order, whatever order the morsels ran
/// in, as [`scatter`] lists its tuples.
///
/// # Panics
///
/// Panics if `morsel_rows` is zero, or if `partition_of` returns
/// `partitions` or more, or panics itself.
pub(crate) fn scatter_rows(
    threads: Threads, tuples: &Tuples, morsel_rows: usize, partitions: usize,
    partition_of: impl Fn(&[usize]) -> usize + Sync,
) -> Vec<RowPartition> {
    let len = tuples.len();
    let queue = Mutex::new((0..len).step_by(morsel_rows).enumerate());
    let mut morsels: Vec<(usize, RowBuckets)> = run_workers(threads, || {
        let mut scattered = Vec::new();
        while let Some((index, first)) = take_next(&queue) {
            let end = first + morsel_rows.min(len - first);
            let mut buckets: RowBuckets = (0..partitions).map(|_| Vec::new()).collect();
            for id in row_id(first)..row_id(end) {
                buckets[partition_of(tuples.row(id))].push(id);
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
    use super::*;

    fn threads(n: usize) -> Threads { Threads::new(n).expect("tests use a nonzero count") }

    #[test]
    fn threads_accepts_only_one_to_max() {
        assert_eq!(Threads::new(0), None);
        assert_eq!(Threads::new(Threads::MAX + 1), None);
        assert_eq!(threads(3).get(), 3);
        assert_eq!(threads(Threads::MAX).get(), Threads::MAX);
    }

    /// Every tuple lands exactly once, in the partition `partition_of`
    /// names, and each partition lists its tuples in input order with their
    /// input positions — whatever the thread count, and however the
    /// morsels divide the input.
    #[test]
    fn scatter_keeps_every_tuple_once_in_input_order() {
        let n = if cfg!(miri) {
            23
        } else {
            1000
        };
        let input: Vec<Vec<usize>> = (0..n).map(|i| vec![i % 7, i]).collect();
        let thread_counts: &[usize] = if cfg!(miri) {
            &[1, 3]
        } else {
            &[1, 2, 3, 8]
        };
        let morsel_sizes = if cfg!(miri) {
            vec![1, 5, n + 1]
        } else {
            vec![1, 4, 5, n, n + 1]
        };
        for &t in thread_counts {
            for &morsel in &morsel_sizes {
                let case = format!("threads {t}, morsel {morsel}");
                let partitions =
                    scatter(threads(t), input.clone(), morsel, 3, |tuple| tuple[0] % 3);
                assert_eq!(partitions.len(), 3, "{case}");
                let mut seen = 0;
                for (p, partition) in partitions.into_iter().enumerate() {
                    let tuples: Vec<Positioned> = partition.into_tuples().collect();
                    assert!(
                        tuples.windows(2).all(|pair| pair[0].0 < pair[1].0),
                        "{case}: partition {p} is out of input order"
                    );
                    for (position, tuple) in &tuples {
                        assert_eq!(tuple, &input[*position], "{case}: position {position}");
                        assert_eq!(tuple[0] % 3, p, "{case}: tuple {tuple:?} in partition {p}");
                    }
                    seen += tuples.len();
                }
                assert_eq!(seen, n, "{case}: every tuple exactly once");
            }
        }
    }

    /// Tuples are moved, not copied: each keeps its heap buffer, and so its
    /// capacity, which a trie that stores tuples (HashTrie) counts in
    /// `heap_size_bytes`.
    #[test]
    fn scatter_moves_tuples_without_reallocating() {
        let input: Vec<Vec<usize>> = (0..10)
            .map(|i| {
                let mut tuple = Vec::with_capacity(9);
                tuple.extend([i, i]);
                tuple
            })
            .collect();
        let buffers: Vec<*const usize> = input.iter().map(|tuple| tuple.as_ptr()).collect();
        for partition in scatter(threads(2), input, 3, 2, |tuple| tuple[0] % 2) {
            for (position, tuple) in partition.into_tuples() {
                assert_eq!(tuple.capacity(), 9, "position {position}");
                assert_eq!(tuple.as_ptr(), buffers[position], "position {position}");
            }
        }
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
        for t in [1, 2, 3, 8] {
            for morsel in [1, 4, 5, n, n + 1] {
                let case = format!("threads {t}, morsel {morsel}");
                let partitions = scatter_rows(threads(t), &input, morsel, 3, |row| row[0] % 3);
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
        let partitions = scatter_rows(threads(2), &input, 3, 2, |row| row[1] % 2);
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
        let partitions = scatter_rows(threads(2), &input, 2, 2, |row| row.len());
        assert_eq!(partitions[0].ids().collect::<Vec<_>>(), vec![0, 1, 2, 3, 4]);
        assert_eq!(partitions[1].len(), 0);
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
        let partitions = scatter(threads(4), Vec::new(), 8, 3, |_| 0);
        assert_eq!(partitions.len(), 3);
        assert!(partitions
            .into_iter()
            .all(|partition| partition.into_tuples().next().is_none()));
        let partitions = scatter_rows(threads(4), &Tuples::new(2), 8, 3, |_| 0);
        assert_eq!(partitions.len(), 3);
        assert!(partitions
            .iter()
            .all(|partition| partition.ids().next().is_none()));
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
