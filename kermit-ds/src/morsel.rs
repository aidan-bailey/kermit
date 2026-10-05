//! Morsel-driven work distribution for the parallel builds (Leis et al.,
//! *Morsel-Driven Parallelism*, SIGMOD 2014).
//!
//! The input is cut into small fixed-size *morsels*. The *dispatcher*, a
//! mutex-guarded queue, hands the next unit of work to whichever worker is
//! free, so a slow worker never stalls the rest. A build uses it twice:
//!
//! 1. [`scatter`] moves tuples into partitions, one morsel at a time;
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

use std::{num::NonZeroUsize, panic, sync::Mutex, thread};

/// How many threads a parallel build uses, the calling thread included.
/// Zero threads cannot be represented.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Threads(NonZeroUsize);

impl Threads {
    /// `n` threads, or `None` when `n` is zero.
    pub fn new(n: usize) -> Option<Self> { NonZeroUsize::new(n).map(Self) }

    /// The number of threads.
    pub fn get(self) -> usize { self.0.get() }
}

/// Tuples per morsel in the parallel builds. A worker takes one morsel per
/// lock of the shared queue, so a morsel must hold enough work to make that
/// lock negligible. Leis et al. use morsels of about 100 000 tuples; these
/// are smaller because partitioning one tuple is cheap.
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
    fn threads_rejects_zero() {
        assert_eq!(Threads::new(0), None);
        assert_eq!(threads(3).get(), 3);
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
    /// capacity, which a trie that stores tuples (HashTrie, plan 2) counts in
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
}
