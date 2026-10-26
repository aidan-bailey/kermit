//! The measurement methodology every `bench` runner shares: one untimed
//! describe build per subject, read for everything a report states without
//! timing it, then the timed metrics in one fixed order.

// Unused until the runners call it (Tasks 9 and 10 of the measure-layer plan).
#![allow(dead_code)]

use std::time::{Duration, Instant};

/// What a bench subcommand measures: something built from flat tuples and
/// then exercised.
pub(crate) trait Subject {
    /// What one build produces: a query's indexes, or one relation.
    type Built;

    /// Whether [`exercise`](Self::exercise) changes what it exercises (a
    /// lazy `HashTrie` builds the children a join reaches). Then `iteration`
    /// runs on a fresh build per sample, never on the describe set.
    const EXERCISE_MUTATES: bool;

    /// One build: the timed body of `build`, and the build half of
    /// `end_to_end`.
    fn build(&self) -> Self::Built;

    /// The heap bytes of each part of `built` (each index, or the one
    /// relation), as built.
    fn heap_bytes(&self, built: &Self::Built) -> Vec<usize>;

    /// One traversal of `built` (a join's rows counted, or a relation
    /// scanned), returning how many rows it visited: the timed body of
    /// `iteration`, and K times in `end_to_end`.
    fn exercise(&self, built: &Self::Built) -> u64;
}

/// The describe step's one exercise: the rows it visited and how long it
/// took.
#[derive(Clone, Copy, Debug)]
struct Counted {
    rows: u64,
    time: Duration,
}

/// A subject built once, outside every Criterion timer: the describe step.
///
/// The bytes are read before anything exercises the set, so they are its
/// as-built footprint even when exercising mutates it.
pub(crate) struct Described<'s, S: Subject> {
    subject: &'s S,
    built: S::Built,
    bytes: Vec<usize>,
    build_time: Duration,
    counted: Option<Counted>,
}

impl<'s, S: Subject> Described<'s, S> {
    /// Builds `subject` once, timing the build, then reads its bytes. With
    /// `count`, it then exercises the set once, timing that too.
    pub(crate) fn new(subject: &'s S, count: bool) -> Self {
        let started = Instant::now();
        let built = subject.build();
        let build_time = started.elapsed();
        let bytes = subject.heap_bytes(&built);
        let counted = count.then(|| {
            let started = Instant::now();
            let rows = subject.exercise(&built);
            Counted {
                rows,
                time: started.elapsed(),
            }
        });
        Self {
            subject,
            built,
            bytes,
            build_time,
            counted,
        }
    }

    /// The describe set.
    pub(crate) fn built(&self) -> &S::Built { &self.built }

    /// The heap bytes of each part of the describe set, as built.
    pub(crate) fn bytes(&self) -> &[usize] { &self.bytes }

    /// The rows the describe step's exercise visited, if it counted.
    pub(crate) fn rows(&self) -> Option<u64> { self.counted.map(|counted| counted.rows) }

    /// How long the describe build took.
    pub(crate) fn build_time(&self) -> Duration { self.build_time }

    /// How long the describe step's exercise took, if it counted.
    pub(crate) fn exercise_time(&self) -> Option<Duration> {
        self.counted.map(|counted| counted.time)
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        std::{
            cell::{Cell, RefCell},
            rc::Rc,
        },
    };

    /// What a [`Recording`] subject saw, in order. Builds are numbered from
    /// 0, so the describe set is always `0`.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Event {
        Build(usize),
        Bytes(usize),
        Exercise(usize),
        Drop(usize),
    }

    /// A built value that logs its own drop and keeps the live count.
    struct Tracked {
        id: usize,
        log: Rc<RefCell<Vec<Event>>>,
        live: Rc<Cell<usize>>,
    }

    impl Drop for Tracked {
        fn drop(&mut self) {
            self.live.set(self.live.get() - 1);
            self.log.borrow_mut().push(Event::Drop(self.id));
        }
    }

    /// A subject that logs every call. A build while another set is alive
    /// panics, which pins the at-most-one-set rule wherever Criterion calls
    /// it.
    struct Recording<const MUTATES: bool> {
        log: Rc<RefCell<Vec<Event>>>,
        live: Rc<Cell<usize>>,
        next: Cell<usize>,
        bytes: Vec<usize>,
    }

    impl<const MUTATES: bool> Recording<MUTATES> {
        fn new(bytes: Vec<usize>) -> Self {
            Self {
                log: Rc::default(),
                live: Rc::default(),
                next: Cell::new(0),
                bytes,
            }
        }

        fn events(&self) -> Vec<Event> { self.log.borrow().clone() }
    }

    impl<const MUTATES: bool> Subject for Recording<MUTATES> {
        type Built = Tracked;

        const EXERCISE_MUTATES: bool = MUTATES;

        fn build(&self) -> Tracked {
            assert_eq!(
                self.live.get(),
                0,
                "a build ran while another set was alive"
            );
            let id = self.next.get();
            self.next.set(id + 1);
            self.live.set(1);
            self.log.borrow_mut().push(Event::Build(id));
            Tracked {
                id,
                log: Rc::clone(&self.log),
                live: Rc::clone(&self.live),
            }
        }

        fn heap_bytes(&self, built: &Tracked) -> Vec<usize> {
            self.log.borrow_mut().push(Event::Bytes(built.id));
            self.bytes.clone()
        }

        fn exercise(&self, built: &Tracked) -> u64 {
            self.log.borrow_mut().push(Event::Exercise(built.id));
            7
        }
    }

    #[test]
    fn describe_builds_once_and_reads_the_bytes_before_any_exercise() {
        let subject = Recording::<false>::new(vec![3, 4]);
        let described = Described::new(&subject, true);
        assert_eq!(subject.events(), [
            Event::Build(0),
            Event::Bytes(0),
            Event::Exercise(0)
        ]);
        assert_eq!(described.bytes(), [3, 4]);
        assert_eq!(described.rows(), Some(7));
        assert!(described.exercise_time().is_some());
    }

    #[test]
    fn describe_without_count_never_exercises() {
        let subject = Recording::<false>::new(vec![5]);
        let described = Described::new(&subject, false);
        assert_eq!(subject.events(), [Event::Build(0), Event::Bytes(0)]);
        assert_eq!(described.rows(), None);
        assert_eq!(described.exercise_time(), None);
    }
}
