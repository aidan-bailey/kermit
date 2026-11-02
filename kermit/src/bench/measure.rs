//! The measurement methodology every `bench` runner shares: one untimed
//! describe build per subject, read for everything a report states without
//! timing it, then the timed metrics in one fixed order. `bench run`,
//! `bench join` and `bench probe` measure a prepared query
//! ([`QuerySubject`]); `bench ds`
//! measures one relation ([`RelationSubject`]).

use {
    super::Metric,
    crate::{
        bench_report::{CriterionGroupRef, Estimator, ReportMetric, ValueEntry},
        execution::{ExecutionFamily, RelationFamily},
        BenchArgs,
    },
    criterion::{BatchSize, Criterion},
    kermit::db::{materialize_count, Catalog, Indexes, Prepared},
    kermit_ds::{HeapSize, RelationHeader, TupleView, Tuples},
    std::{
        path::PathBuf,
        time::{Duration, Instant},
    },
};

/// What a bench subcommand measures: something built from flat tuples and
/// then exercised.
pub(crate) trait Subject {
    /// What one build produces: a query's indexes, one relation, or an
    /// engine's loaded tables. It may borrow the subject.
    type Built<'s>
    where
        Self: 's;

    /// Whether [`exercise`](Self::exercise) changes what it exercises (a
    /// lazy `HashTrie` builds the children a join reaches). Then `iteration`
    /// runs on a fresh build per sample, never on the describe set.
    const EXERCISE_MUTATES: bool;

    /// Whether [`exercise`](Self::exercise) streams its rows. When false the
    /// subject has no `iteration` or `end_to_end` (`exercise` is never
    /// called), and the describe step counts through
    /// [`materialize`](Self::materialize) instead.
    const STREAMS: bool;

    /// Whether [`materialize`](Self::materialize) changes the build it runs
    /// on (Nemo stores its result beside the tables; a lazy `HashTrie`
    /// expands the children the join reaches). Then `materialize` runs on a
    /// fresh build per sample, never on the describe set, and exactly once on
    /// any one build: a second run on a build can panic (Nemo's does). A
    /// subject that does not stream ([`STREAMS`](Self::STREAMS)) has its one
    /// `materialize` on the describe set as the describe step's count, and
    /// [`Described::materialize_once`] returns that run instead of running
    /// again; one that streams has `materialize_once` run it on a fresh
    /// build.
    const MATERIALIZE_MUTATES: bool;

    /// One build: the timed body of `build`, and the build half of
    /// `end_to_end`.
    fn build(&self) -> Self::Built<'_>;

    /// The heap bytes of each part of `built` (each index, or the one
    /// relation), as built.
    fn heap_bytes(&self, built: &Self::Built<'_>) -> Vec<usize>;

    /// One traversal of `built` (a join's rows counted, or a relation
    /// scanned), returning how many rows it visited: the timed body of
    /// `iteration`, and K times in `end_to_end`.
    fn exercise(&self, built: &Self::Built<'_>) -> u64;

    /// The join with its result kept (see [`Materialized`]): the timed body of
    /// `materialize`. `None` for a subject with no join
    /// ([`RelationSubject`]).
    fn materialize(&self, built: &Self::Built<'_>) -> Option<Materialized>;
}

/// What one [`Subject::materialize`] produced.
pub(crate) struct Materialized {
    /// The distinct full assignments.
    pub(crate) rows: u64,
    /// The sorted batch (kermit and DuckDB), returned so that Criterion frees
    /// it after the timer stops. `None` where the engine keeps its result
    /// itself.
    #[allow(
        dead_code,
        reason = "held only so that Criterion drops it after the timer"
    )]
    pub(crate) kept: Option<Tuples>,
}

/// A prepared query on one execution cell. A build is the query's indexes,
/// each copied once out of the catalog's flat tuples, an exercise counts the
/// join's rows, and a materialize keeps them.
pub(crate) struct QuerySubject<'a, F> {
    pub(crate) family: &'a F,
    pub(crate) catalog: &'a Catalog,
    pub(crate) prepared: &'a Prepared,
}

impl<F: ExecutionFamily> Subject for QuerySubject<'_, F> {
    type Built<'s>
        = Indexes<F::Rel>
    where
        Self: 's;

    const EXERCISE_MUTATES: bool = F::JOIN_MUTATES;
    /// The join `materialize` runs is the one `iteration` runs, so it expands
    /// a lazy trie as well.
    const MATERIALIZE_MUTATES: bool = F::JOIN_MUTATES;
    const STREAMS: bool = true;

    fn build(&self) -> Indexes<F::Rel> { self.family.build_indexes(self.catalog, self.prepared) }

    fn heap_bytes(&self, built: &Indexes<F::Rel>) -> Vec<usize> {
        built
            .iter()
            .map(|(_, index)| index.heap_size_bytes())
            .collect()
    }

    fn exercise(&self, built: &Indexes<F::Rel>) -> u64 {
        self.family.count_rows(self.prepared, built)
    }

    /// Every full assignment of the executed query pushed into a batch of
    /// `Prepared::assignment_width` columns, then `materialize_count`'s sort
    /// and distinct count. The memory guard bounds the batch before this runs.
    ///
    /// # Panics
    ///
    /// If the answer has more than `RowId::MAX` rows, the most a `Tuples`
    /// holds.
    fn materialize(&self, built: &Indexes<F::Rel>) -> Option<Materialized> {
        let mut rows = Tuples::new(self.prepared.assignment_width());
        self.family
            .execute_assignments(self.prepared, built, |row| rows.push(row));
        let (sorted, distinct) = materialize_count(rows);
        Some(Materialized {
            rows: distinct,
            kept: Some(sorted),
        })
    }
}

/// One relation on one structure: `bench ds`'s subject. A build is the
/// relation, from its file's tuples in file order, and an exercise scans it.
/// It has no join, so nothing to materialize.
pub(crate) struct RelationSubject<'a, F> {
    pub(crate) family: &'a F,
    pub(crate) header: &'a RelationHeader,
    pub(crate) tuples: &'a Tuples,
}

impl<F: RelationFamily> Subject for RelationSubject<'_, F> {
    type Built<'s>
        = F::Rel
    where
        Self: 's;

    /// A scan reads an unexpanded child's pending rows and expands nothing
    /// (`HashTrie::for_each_tuple`), so no structure's scan mutates it.
    const EXERCISE_MUTATES: bool = false;
    const MATERIALIZE_MUTATES: bool = false;
    const STREAMS: bool = true;

    fn build(&self) -> F::Rel {
        self.family
            .build_index(self.header.clone(), TupleView::all(self.tuples))
    }

    fn heap_bytes(&self, built: &F::Rel) -> Vec<usize> { vec![built.heap_size_bytes()] }

    fn exercise(&self, built: &F::Rel) -> u64 { F::scan(built) }

    fn materialize(&self, _built: &F::Rel) -> Option<Materialized> { None }
}

/// How Criterion samples: from `bench`'s flags, or, in tests, milliseconds
/// into a temporary directory.
#[derive(Clone, Debug)]
pub(crate) struct CriterionSettings {
    pub(crate) sample_size: usize,
    pub(crate) measurement_time: Duration,
    pub(crate) warm_up_time: Duration,
    /// Where Criterion writes; `None` keeps its default (`CRITERION_HOME`,
    /// else `target/criterion`).
    pub(crate) output_directory: Option<PathBuf>,
}

impl CriterionSettings {
    /// `bench`'s `--sample-size`, `--measurement-time` and `--warm-up-time`,
    /// the last two in whole seconds.
    pub(crate) fn from_args(args: &BenchArgs) -> Self {
        Self {
            sample_size: args.sample_size,
            measurement_time: Duration::from_secs(args.measurement_time),
            warm_up_time: Duration::from_secs(args.warm_up_time),
            output_directory: None,
        }
    }

    fn criterion(&self) -> Criterion {
        let criterion = Criterion::default()
            .sample_size(self.sample_size)
            .measurement_time(self.measurement_time)
            .warm_up_time(self.warm_up_time);
        match &self.output_directory {
            | Some(dir) => criterion.output_directory(dir),
            | None => criterion,
        }
    }
}

/// What [`Described::measure`] runs, and how it names what it records.
pub(crate) struct MeasureRequest<'a> {
    /// The metrics to record; `space` comes from the describe step's bytes.
    pub(crate) metrics: &'a [Metric],
    /// Record the describe step's two times as single-run `build` and
    /// `iteration` values (`bench probe`).
    pub(crate) single_run: bool,
    /// K in `end_to_end`'s `build + K × exercise`.
    pub(crate) queries_per_build: u32,
    pub(crate) criterion: &'a CriterionSettings,
    /// The Criterion group the time metrics run in.
    pub(crate) group: &'a str,
    /// The prefix of every function id: `bench ds`'s `{ds}`, `None` elsewhere.
    pub(crate) function_prefix: Option<&'a str>,
}

impl MeasureRequest<'_> {
    fn function_id(&self, phase: &str) -> String {
        match self.function_prefix {
            | Some(prefix) => format!("{prefix}/{phase}"),
            | None => phase.to_string(),
        }
    }

    /// Whether `metric` was asked for and runs under Criterion: every timed
    /// metric, except that a single run records `materialize` as a value
    /// (`Described::materialize_once`) and starts no Criterion function.
    fn criterion_runs(&self, metric: Metric) -> bool {
        self.metrics.contains(&metric)
            && metric.is_timed()
            && !(self.single_run && metric == Metric::Materialize)
    }

    fn time_ref(&self, phase: &str) -> CriterionGroupRef {
        CriterionGroupRef {
            group: self.group.to_string(),
            function: self.function_id(phase),
            metric: ReportMetric::Time,
        }
    }
}

/// What [`Described::measure`] recorded.
pub(crate) struct Measured {
    /// One entry per Criterion function that ran.
    pub(crate) criterion_groups: Vec<CriterionGroupRef>,
    /// The measurements made without Criterion.
    pub(crate) values: Vec<ValueEntry>,
}

/// `duration` in whole nanoseconds, saturating at `u64::MAX` (about 584
/// years).
fn nanos(duration: Duration) -> u64 { u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX) }

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
pub(crate) struct Described<'s, S: Subject + 's> {
    subject: &'s S,
    built: S::Built<'s>,
    bytes: Vec<usize>,
    build_time: Duration,
    counted: Option<Counted>,
    /// The one materialize of [`materialize_once`](Self::materialize_once).
    materialized: Option<Counted>,
}

impl<'s, S: Subject + 's> Described<'s, S> {
    /// Builds `subject` once, timing the build, then reads its bytes. With
    /// `count`, it then counts the set once, timing that too: a streamed
    /// `exercise`, or, for a subject that does not stream
    /// ([`Subject::STREAMS`]), a `materialize`.
    pub(crate) fn new(subject: &'s S, count: bool) -> Self {
        let started = Instant::now();
        let built = subject.build();
        let build_time = started.elapsed();
        let bytes = subject.heap_bytes(&built);
        let counted = count.then(|| {
            let started = Instant::now();
            let rows = if S::STREAMS {
                subject.exercise(&built)
            } else {
                subject
                    .materialize(&built)
                    .expect("a subject that does not stream materializes")
                    .rows
            };
            Counted {
                rows,
                time: started.elapsed(),
            }
        });
        // A subject that does not stream counted through its one `materialize`,
        // so that run is the `materialize` of `materialize_once`.
        let materialized = counted.filter(|_| !S::STREAMS);
        Self {
            subject,
            built,
            bytes,
            build_time,
            counted,
            materialized,
        }
    }

    /// The describe set.
    pub(crate) fn built(&self) -> &S::Built<'s> { &self.built }

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

    /// The distinct row count of one timed `materialize`, outside every
    /// Criterion timer: `--verify`'s check of the `materialize` count, and a
    /// probe's single-run `materialize` value (kept for
    /// [`measure`](Self::measure)). It never runs `materialize` twice on one
    /// build:
    /// - a subject that does not stream returns the describe step's count,
    ///   which was its one `materialize` (`None` if the describe step did not
    ///   count);
    /// - a streaming subject whose `materialize` mutates
    ///   ([`Subject::MATERIALIZE_MUTATES`]) runs it on a fresh build, which is
    ///   dropped, untimed, after the time is taken;
    /// - any other subject runs it on the describe set, which it leaves as it
    ///   was.
    ///
    /// The result is freed after the time is taken. `None` also for a subject
    /// with no join.
    pub(crate) fn materialize_once(&mut self) -> Option<u64> {
        if !S::STREAMS {
            return self.materialized.map(|materialized| materialized.rows);
        }
        let fresh = S::MATERIALIZE_MUTATES.then(|| self.subject.build());
        let started = Instant::now();
        let materialized = match &fresh {
            | Some(fresh) => self.subject.materialize(fresh),
            | None => self.subject.materialize(&self.built),
        }?;
        let time = started.elapsed();
        drop(fresh);
        let rows = materialized.rows;
        drop(materialized);
        self.materialized = Some(Counted {
            rows,
            time,
        });
        Some(rows)
    }

    /// How long [`materialize_once`](Self::materialize_once) took, if it ran.
    pub(crate) fn materialize_time(&self) -> Option<Duration> {
        self.materialized.map(|materialized| materialized.time)
    }
}

impl<'s, S: Subject + 's> Described<'s, S> {
    /// Runs `request`'s metrics in the one order every cell follows:
    /// `iteration` and `materialize` on the describe set (each on a fresh
    /// build per sample when it mutates), then the describe set is dropped,
    /// then `build` and `end_to_end`. Consuming `self` is what keeps any
    /// other set from being alive while a build is timed. Every timed routine
    /// returns what it built (`materialize`, its sorted batch), so Criterion
    /// frees it after the timer stops. A subject that does not stream has
    /// neither `iteration` nor `end_to_end`.
    pub(crate) fn measure(self, request: &MeasureRequest<'_>) -> Measured {
        let Described {
            subject,
            built,
            bytes,
            build_time,
            counted,
            materialized,
        } = self;

        let mut values = Vec::new();
        if request.metrics.contains(&Metric::Space) {
            values.push(ValueEntry {
                function: request.function_id("space"),
                metric: ReportMetric::Space,
                estimator: Estimator::Exact,
                value: bytes
                    .iter()
                    .map(|&b| u64::try_from(b).unwrap_or(u64::MAX))
                    .fold(0, u64::saturating_add),
            });
        }
        if request.single_run {
            values.push(ValueEntry {
                function: request.function_id("build"),
                metric: ReportMetric::Time,
                estimator: Estimator::Single,
                value: nanos(build_time),
            });
            if let Some(counted) = counted.filter(|_| S::STREAMS) {
                values.push(ValueEntry {
                    function: request.function_id("iteration"),
                    metric: ReportMetric::Time,
                    estimator: Estimator::Single,
                    value: nanos(counted.time),
                });
            }
            if let Some(materialized) =
                materialized.filter(|_| request.metrics.contains(&Metric::Materialize))
            {
                values.push(ValueEntry {
                    function: request.function_id("materialize"),
                    metric: ReportMetric::Time,
                    estimator: Estimator::Single,
                    value: nanos(materialized.time),
                });
            }
        }
        if !request
            .metrics
            .iter()
            .any(|&metric| request.criterion_runs(metric))
        {
            return Measured {
                criterion_groups: Vec::new(),
                values,
            };
        }

        let mut criterion = request.criterion.criterion();
        let mut group = criterion.benchmark_group(request.group);
        let mut criterion_groups = Vec::new();
        let iteration = S::STREAMS && request.criterion_runs(Metric::Iteration);
        let materialize = request.criterion_runs(Metric::Materialize);
        if iteration && !S::EXERCISE_MUTATES {
            // Over the describe set, built once outside Criterion. The routine
            // counts rows and returns only the count, so memory does not grow
            // with the answer (#65).
            group.bench_function(request.function_id("iteration"), |b| {
                b.iter(|| subject.exercise(&built));
            });
        }
        if materialize && !S::MATERIALIZE_MUTATES {
            // Over the describe set too, with no setup. The routine returns
            // its sorted batch, so Criterion frees it after the timer stops,
            // and the batch is the only other thing alive.
            group.bench_function(request.function_id("materialize"), |b| {
                b.iter_batched(
                    || (),
                    |()| subject.materialize(&built),
                    BatchSize::PerIteration,
                );
            });
        }
        // From here on, no set but Criterion's own is alive.
        drop(built);
        if iteration && S::EXERCISE_MUTATES {
            // Cold (#92): each timed exercise runs on a set built in the
            // untimed setup, so it pays the expansion it causes instead of
            // finding it done by an earlier sample. The routine hands the set
            // back, so Criterion frees it after the timer stops.
            group.bench_function(request.function_id("iteration"), |b| {
                b.iter_batched(
                    || subject.build(),
                    |fresh| {
                        let rows = subject.exercise(&fresh);
                        (fresh, rows)
                    },
                    BatchSize::PerIteration,
                );
            });
        }
        if materialize && S::MATERIALIZE_MUTATES {
            // Cold, like a mutating `iteration`: the result lives beside the
            // set it was made on, and the routine hands both back.
            group.bench_function(request.function_id("materialize"), |b| {
                b.iter_batched(
                    || subject.build(),
                    |fresh| {
                        let result = subject.materialize(&fresh);
                        (fresh, result)
                    },
                    BatchSize::PerIteration,
                );
            });
        }
        if iteration {
            criterion_groups.push(request.time_ref("iteration"));
        }
        if materialize {
            criterion_groups.push(request.time_ref("materialize"));
        }
        if request.metrics.contains(&Metric::Build) {
            // A no-op setup, so nothing is copied beforehand. `PerIteration`
            // keeps one set alive at a time, and Criterion frees each after
            // the timer stops.
            group.bench_function(request.function_id("build"), |b| {
                b.iter_batched(|| (), |()| subject.build(), BatchSize::PerIteration);
            });
            criterion_groups.push(request.time_ref("build"));
        }
        if S::STREAMS && request.metrics.contains(&Metric::EndToEnd) {
            // A fresh build per sample is the point of this metric: batching
            // would amortise away the construction cost whose interaction
            // with the exercise it measures.
            let k = request.queries_per_build;
            group.bench_function(request.function_id("end_to_end"), |b| {
                b.iter_batched(
                    || (),
                    |()| {
                        let built = subject.build();
                        for _ in 0..k {
                            std::hint::black_box(subject.exercise(&built));
                        }
                        built
                    },
                    BatchSize::PerIteration,
                );
            });
            criterion_groups.push(request.time_ref("end_to_end"));
        }
        group.finish();
        criterion.final_summary();
        Measured {
            criterion_groups,
            values,
        }
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
        Materialize(usize),
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
    /// it. `MUTATES` is its `EXERCISE_MUTATES`, `STREAMING` its `STREAMS` and
    /// `MATERIALIZE_MUTATING` its `MATERIALIZE_MUTATES`.
    struct Recording<
        const MUTATES: bool,
        const STREAMING: bool = true,
        const MATERIALIZE_MUTATING: bool = false,
    > {
        log: Rc<RefCell<Vec<Event>>>,
        live: Rc<Cell<usize>>,
        next: Cell<usize>,
        bytes: Vec<usize>,
    }

    impl<const MUTATES: bool, const STREAMING: bool, const MATERIALIZE_MUTATING: bool>
        Recording<MUTATES, STREAMING, MATERIALIZE_MUTATING>
    {
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

    impl<const MUTATES: bool, const STREAMING: bool, const MATERIALIZE_MUTATING: bool> Subject
        for Recording<MUTATES, STREAMING, MATERIALIZE_MUTATING>
    {
        type Built<'s>
            = Tracked
        where
            Self: 's;

        const EXERCISE_MUTATES: bool = MUTATES;
        const MATERIALIZE_MUTATES: bool = MATERIALIZE_MUTATING;
        const STREAMS: bool = STREAMING;

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

        fn materialize(&self, built: &Tracked) -> Option<Materialized> {
            self.log.borrow_mut().push(Event::Materialize(built.id));
            Some(Materialized {
                rows: 5,
                kept: None,
            })
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

    use crate::bench_report::{Estimator, ReportMetric, ValueEntry};

    /// Criterion settings for tests: the minimum sample count, milliseconds,
    /// and a temporary directory, so no test writes into the workspace.
    fn quick(dir: &tempfile::TempDir) -> CriterionSettings {
        CriterionSettings {
            sample_size: 10,
            measurement_time: Duration::from_millis(20),
            warm_up_time: Duration::from_millis(5),
            output_directory: Some(dir.path().to_path_buf()),
        }
    }

    fn request<'a>(
        metrics: &'a [Metric], single_run: bool, criterion: &'a CriterionSettings,
    ) -> MeasureRequest<'a> {
        MeasureRequest {
            metrics,
            single_run,
            queries_per_build: 2,
            criterion,
            group: "test",
            function_prefix: None,
        }
    }

    /// The position of the first event `matches` accepts.
    fn first(events: &[Event], matches: impl Fn(&Event) -> bool) -> usize {
        events.iter().position(matches).expect("the event happened")
    }

    #[test]
    fn eager_iteration_runs_on_the_describe_set_before_any_timed_build() {
        let dir = tempfile::tempdir().unwrap();
        let settings = quick(&dir);
        let subject = Recording::<false>::new(vec![8]);
        let metrics = [Metric::Iteration, Metric::Build, Metric::EndToEnd];
        let measured =
            Described::new(&subject, false).measure(&request(&metrics, false, &settings));
        let events = subject.events();
        let dropped = first(&events, |e| *e == Event::Drop(0));
        let describe_exercises: Vec<usize> = events
            .iter()
            .enumerate()
            .filter(|(_, e)| **e == Event::Exercise(0))
            .map(|(at, _)| at)
            .collect();
        assert!(
            !describe_exercises.is_empty(),
            "iteration ran on the describe set"
        );
        assert!(describe_exercises.iter().all(|&at| at < dropped));
        assert!(first(&events, |e| matches!(e, Event::Build(id) if *id > 0)) > dropped);
        assert!(events[..dropped]
            .iter()
            .all(|e| !matches!(e, Event::Exercise(id) if *id > 0)));
        let functions: Vec<&str> = measured
            .criterion_groups
            .iter()
            .map(|g| g.function.as_str())
            .collect();
        assert_eq!(functions, ["iteration", "build", "end_to_end"]);
    }

    #[test]
    fn a_mutating_subject_drops_the_describe_set_before_iteration() {
        let dir = tempfile::tempdir().unwrap();
        let settings = quick(&dir);
        let subject = Recording::<true>::new(vec![8]);
        Described::new(&subject, true).measure(&request(&[Metric::Iteration], false, &settings));
        let events = subject.events();
        let dropped = first(&events, |e| *e == Event::Drop(0));
        // Describe's own count is the only exercise of set 0.
        assert_eq!(
            events.iter().filter(|e| **e == Event::Exercise(0)).count(),
            1
        );
        assert!(first(&events, |e| matches!(e, Event::Build(id) if *id > 0)) > dropped);
        assert!(events
            .iter()
            .any(|e| matches!(e, Event::Exercise(id) if *id > 0)));
    }

    #[test]
    fn space_is_the_exact_sum_of_the_bytes_and_zero_stays_zero() {
        let dir = tempfile::tempdir().unwrap();
        let settings = quick(&dir);
        for (bytes, total) in [(vec![100, 28], 128), (vec![], 0), (vec![0], 0)] {
            let subject = Recording::<false>::new(bytes);
            let measured = Described::new(&subject, false).measure(&request(
                &[Metric::Space],
                false,
                &settings,
            ));
            assert!(measured.criterion_groups.is_empty());
            assert_eq!(measured.values, [ValueEntry {
                function: "space".into(),
                metric: ReportMetric::Space,
                estimator: Estimator::Exact,
                value: total,
            }]);
            // Without a time metric nothing but the describe build ran.
            assert_eq!(subject.events(), [
                Event::Build(0),
                Event::Bytes(0),
                Event::Drop(0)
            ]);
        }
    }

    #[test]
    fn a_single_run_records_the_describe_steps_times() {
        let dir = tempfile::tempdir().unwrap();
        let settings = quick(&dir);
        let counted = Recording::<false>::new(vec![1]);
        let measured = Described::new(&counted, true).measure(&request(&[], true, &settings));
        let single: Vec<(&str, ReportMetric, Estimator)> = measured
            .values
            .iter()
            .map(|v| (v.function.as_str(), v.metric, v.estimator))
            .collect();
        assert_eq!(single, [
            ("build", ReportMetric::Time, Estimator::Single),
            ("iteration", ReportMetric::Time, Estimator::Single),
        ]);

        let uncounted = Recording::<false>::new(vec![1]);
        let measured = Described::new(&uncounted, false).measure(&request(&[], true, &settings));
        let functions: Vec<&str> = measured
            .values
            .iter()
            .map(|v| v.function.as_str())
            .collect();
        assert_eq!(functions, ["build"]);
    }

    #[test]
    fn the_prefix_names_every_function() {
        let dir = tempfile::tempdir().unwrap();
        let settings = quick(&dir);
        let subject = Recording::<false>::new(vec![4]);
        let metrics = [Metric::Build, Metric::Space];
        let mut prefixed = request(&metrics, false, &settings);
        prefixed.function_prefix = Some("TreeTrie");
        let measured = Described::new(&subject, false).measure(&prefixed);
        assert_eq!(measured.criterion_groups[0].function, "TreeTrie/build");
        assert_eq!(measured.values[0].function, "TreeTrie/space");
    }

    #[test]
    fn materialize_runs_on_the_describe_set_before_it_is_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let settings = quick(&dir);
        let subject = Recording::<false>::new(vec![8]);
        let metrics = [Metric::Materialize, Metric::Build];
        let measured =
            Described::new(&subject, false).measure(&request(&metrics, false, &settings));
        let events = subject.events();
        let dropped = first(&events, |e| *e == Event::Drop(0));
        assert!(events[..dropped].contains(&Event::Materialize(0)));
        assert!(first(&events, |e| matches!(e, Event::Build(id) if *id > 0)) > dropped);
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, Event::Materialize(id) if *id > 0)),
            "a materialize that does not mutate never needs a fresh build"
        );
        let functions: Vec<&str> = measured
            .criterion_groups
            .iter()
            .map(|g| g.function.as_str())
            .collect();
        assert_eq!(functions, ["materialize", "build"]);
    }

    #[test]
    fn a_mutating_materialize_runs_on_fresh_builds_after_the_describe_set_is_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let settings = quick(&dir);
        let subject = Recording::<false, true, true>::new(vec![8]);
        Described::new(&subject, false).measure(&request(&[Metric::Materialize], false, &settings));
        let events = subject.events();
        let dropped = first(&events, |e| *e == Event::Drop(0));
        assert!(!events.contains(&Event::Materialize(0)));
        assert!(first(&events, |e| matches!(e, Event::Materialize(id) if *id > 0)) > dropped);
    }

    #[test]
    fn a_single_run_records_materialize_as_a_value_and_runs_no_criterion_group() {
        let dir = tempfile::tempdir().unwrap();
        let settings = quick(&dir);
        let subject = Recording::<false>::new(vec![1]);
        let mut described = Described::new(&subject, true);
        assert_eq!(described.materialize_time(), None);
        assert_eq!(described.materialize_once(), Some(5));
        assert!(described.materialize_time().is_some());
        let metrics = [Metric::Materialize];
        let measured = described.measure(&request(&metrics, true, &settings));
        assert!(measured.criterion_groups.is_empty());
        let single: Vec<(&str, ReportMetric, Estimator)> = measured
            .values
            .iter()
            .map(|v| (v.function.as_str(), v.metric, v.estimator))
            .collect();
        assert_eq!(single, [
            ("build", ReportMetric::Time, Estimator::Single),
            ("iteration", ReportMetric::Time, Estimator::Single),
            ("materialize", ReportMetric::Time, Estimator::Single),
        ]);
        let builds = subject
            .events()
            .iter()
            .filter(|e| matches!(e, Event::Build(_)))
            .count();
        assert_eq!(builds, 1, "Criterion never started");
    }

    #[test]
    fn a_subject_that_does_not_stream_counts_through_materialize() {
        let subject = Recording::<false, false>::new(vec![1]);
        let described = Described::new(&subject, true);
        assert_eq!(subject.events(), [
            Event::Build(0),
            Event::Bytes(0),
            Event::Materialize(0)
        ]);
        assert_eq!(described.rows(), Some(5));
    }

    #[test]
    fn a_subject_that_does_not_stream_has_no_iteration_or_end_to_end() {
        let dir = tempfile::tempdir().unwrap();
        let settings = quick(&dir);
        let subject = Recording::<false, false>::new(vec![1]);
        // A single run records `build` and no `iteration` value.
        let measured = Described::new(&subject, true).measure(&request(&[], true, &settings));
        let functions: Vec<&str> = measured
            .values
            .iter()
            .map(|v| v.function.as_str())
            .collect();
        assert_eq!(functions, ["build"]);

        // Neither Criterion function runs, and `exercise` is never called.
        let subject = Recording::<false, false>::new(vec![1]);
        let metrics = [Metric::Iteration, Metric::EndToEnd, Metric::Build];
        let measured =
            Described::new(&subject, false).measure(&request(&metrics, false, &settings));
        let functions: Vec<&str> = measured
            .criterion_groups
            .iter()
            .map(|g| g.function.as_str())
            .collect();
        assert_eq!(functions, ["build"]);
        assert!(!subject
            .events()
            .iter()
            .any(|e| matches!(e, Event::Exercise(_))));
    }

    /// Nemo panics on a second `materialize` on one build, so a subject that
    /// does not stream and whose `materialize` mutates (`Recording::<false,
    /// false, true>`) must see exactly one per build.
    #[test]
    fn a_mutating_materialize_runs_once_per_build_on_the_single_run_and_verify_paths() {
        let dir = tempfile::tempdir().unwrap();
        let settings = quick(&dir);
        let metrics = [Metric::Materialize];
        let materializes = |events: &[Event], id: usize| {
            events
                .iter()
                .filter(|e| **e == Event::Materialize(id))
                .count()
        };
        let builds = |events: &[Event]| {
            events
                .iter()
                .filter(|e| matches!(e, Event::Build(_)))
                .count()
        };

        // A single run: the describe step's count is the one `materialize`,
        // and `materialize_once` returns it, however often it is asked.
        let subject = Recording::<false, false, true>::new(vec![1]);
        let mut described = Described::new(&subject, true);
        assert_eq!(described.materialize_once(), Some(5));
        assert_eq!(described.materialize_once(), Some(5));
        assert!(described.materialize_time().is_some());
        let measured = described.measure(&request(&metrics, true, &settings));
        assert!(measured.criterion_groups.is_empty());
        let functions: Vec<&str> = measured
            .values
            .iter()
            .map(|v| v.function.as_str())
            .collect();
        assert_eq!(functions, ["build", "materialize"]);
        let events = subject.events();
        assert_eq!(builds(&events), 1);
        assert_eq!(materializes(&events, 0), 1);

        // `--verify` and then Criterion: the describe set's one `materialize`
        // is the check, and each fresh build Criterion makes gets one.
        let subject = Recording::<false, false, true>::new(vec![1]);
        let mut described = Described::new(&subject, true);
        assert_eq!(described.materialize_once(), Some(5));
        let measured = described.measure(&request(&metrics, false, &settings));
        assert_eq!(measured.criterion_groups.len(), 1);
        let events = subject.events();
        assert!(builds(&events) > 1, "Criterion built fresh sets");
        assert_eq!(materializes(&events, 0), 1);
        for id in 1..builds(&events) {
            assert_eq!(materializes(&events, id), 1, "fresh build {id}");
        }
    }

    use {
        crate::execution::{HashHtj, SortedTrieFamily, TrieLftj},
        kermit::db::{Catalog, Prepared},
        kermit_ds::{EagerExpansion, HeapSize, LazyExpansion, NoPruning, RelationHeader, Tuples},
        kermit_iters::SipHashStrategy,
    };

    /// `edge` (1 → 2, 2 → 3, 1 → 3) as flat tuples.
    fn edges() -> (RelationHeader, Tuples) {
        let mut tuples = Tuples::new(2);
        for row in [[1, 2], [2, 3], [1, 3]] {
            tuples.push(&row);
        }
        (RelationHeader::new_positional("edge", 2), tuples)
    }

    /// `edge` with the row 1 → 2 held twice (a set keeps one copy, a bag
    /// both), then 2 → 3.
    fn duplicated_edges() -> (RelationHeader, Tuples) {
        let mut tuples = Tuples::new(2);
        for row in [[1, 2], [1, 2], [2, 3]] {
            tuples.push(&row);
        }
        (RelationHeader::new_positional("edge", 2), tuples)
    }

    /// The two-hop paths over [`edges`], prepared under the lexicographic
    /// optimiser.
    fn two_hops() -> (Catalog, Prepared) { two_hops_over(edges()) }

    /// The two-hop paths over `edge`, prepared under the lexicographic
    /// optimiser.
    fn two_hops_over(edge: (RelationHeader, Tuples)) -> (Catalog, Prepared) {
        use kermit_algos::{JoinQuery, LexicographicOptimiser, Planner, StatisticsLevel};
        let catalog = Catalog::new(vec![edge], StatisticsLevel::TupleCounts);
        let query: JoinQuery = "Q(X, Z) :- edge(X, Y), edge(Y, Z).".parse().unwrap();
        let planner = Planner::new(Box::new(LexicographicOptimiser));
        let prepared =
            kermit::db::prepare(&catalog, &query, &planner, kermit_ds::TrieLevels::All).unwrap();
        (catalog, prepared)
    }

    #[test]
    fn a_query_subject_builds_the_plans_indexes_and_counts_the_answer() {
        let (catalog, prepared) = two_hops();
        let family = TrieLftj::<kermit_ds::TreeTrie>::new(Default::default());
        let subject = QuerySubject {
            family: &family,
            catalog: &catalog,
            prepared: &prepared,
        };
        let described = Described::new(&subject, true);
        assert_eq!(described.rows(), Some(1), "only 1 → 2 → 3");
        assert_eq!(described.bytes().len(), prepared.indexes().len());
        assert!(described.bytes().iter().all(|&bytes| bytes > 0));
        const {
            assert!(
                !<QuerySubject<'static, TrieLftj<kermit_ds::TreeTrie>> as Subject>::EXERCISE_MUTATES
            )
        };
    }

    #[test]
    fn a_lazy_hash_trie_query_subject_mutates() {
        const {
            assert!(
                <QuerySubject<'static, HashHtj<SipHashStrategy, NoPruning, LazyExpansion>> as Subject>::EXERCISE_MUTATES
            )
        };
    }

    #[test]
    fn a_query_subject_streams_and_materialize_mutates_with_its_joins() {
        const {
            assert!(<QuerySubject<'static, TrieLftj<kermit_ds::TreeTrie>> as Subject>::STREAMS);
            assert!(
                !<QuerySubject<'static, TrieLftj<kermit_ds::TreeTrie>> as Subject>::MATERIALIZE_MUTATES
            );
            assert!(
                <QuerySubject<'static, HashHtj<SipHashStrategy, NoPruning, LazyExpansion>> as Subject>::MATERIALIZE_MUTATES
            );
        };
    }

    /// `Q(X, Z) :- edge(X, Y), edge(Y, Z)` has the head `X, Z` and the
    /// body-only `Y`, so an assignment is `(X, Z, Y)`: `(1, 3, 2)`.
    #[test]
    fn a_query_subject_materializes_the_sorted_full_assignments() {
        let (catalog, prepared) = two_hops();
        let family = TrieLftj::<kermit_ds::TreeTrie>::new(Default::default());
        let subject = QuerySubject {
            family: &family,
            catalog: &catalog,
            prepared: &prepared,
        };
        let built = subject.build();
        let materialized = subject
            .materialize(&built)
            .expect("a query subject materializes");
        assert_eq!(materialized.rows, 1, "only 1 → 2 → 3");
        let kept = materialized.kept.expect("the sorted batch is kept");
        assert_eq!((kept.arity(), kept.to_vecs()), (3, vec![vec![1, 3, 2]]));
    }

    /// The bag answers `(1, 3, 2)` twice; the distinct count says one, and the
    /// sorted batch it keeps still holds both rows. A set holds one copy.
    #[test]
    fn the_distinct_count_removes_the_duplicate_assignments_a_bag_keeps() {
        let (catalog, prepared) = two_hops_over(duplicated_edges());
        let hash = HashHtj::<SipHashStrategy, NoPruning, EagerExpansion>::new(
            Default::default(),
            Default::default(),
        );
        let subject = QuerySubject {
            family: &hash,
            catalog: &catalog,
            prepared: &prepared,
        };
        let materialized = subject
            .materialize(&subject.build())
            .expect("a query subject materializes");
        assert_eq!(materialized.rows, 1);
        assert_eq!(materialized.kept.expect("kept").len(), 2);

        let tree = TrieLftj::<kermit_ds::TreeTrie>::new(Default::default());
        let subject = QuerySubject {
            family: &tree,
            catalog: &catalog,
            prepared: &prepared,
        };
        let materialized = subject
            .materialize(&subject.build())
            .expect("a query subject materializes");
        assert_eq!(materialized.rows, 1);
        assert_eq!(materialized.kept.expect("kept").len(), 1);
    }

    #[test]
    fn a_lazy_query_subject_materializes_once_on_a_fresh_build() {
        let (catalog, prepared) = two_hops();
        let family = HashHtj::<SipHashStrategy, NoPruning, LazyExpansion>::new(
            Default::default(),
            Default::default(),
        );
        let subject = QuerySubject {
            family: &family,
            catalog: &catalog,
            prepared: &prepared,
        };
        let mut described = Described::new(&subject, false);
        assert_eq!(described.materialize_once(), Some(1));
        assert!(described.materialize_time().is_some());
        assert_eq!(
            subject.heap_bytes(described.built()),
            described.bytes(),
            "the describe set was not expanded by the materialize"
        );
    }

    #[test]
    fn a_relation_subject_builds_the_relation_and_scans_it() {
        let (header, tuples) = edges();
        let family = SortedTrieFamily::<kermit_ds::TreeTrie>::default();
        let subject = RelationSubject {
            family: &family,
            header: &header,
            tuples: &tuples,
        };
        let described = Described::new(&subject, true);
        assert_eq!(described.rows(), Some(3));
        assert_eq!(described.bytes(), [described.built().heap_size_bytes()]);
    }

    #[test]
    fn a_relation_subject_has_no_join_to_materialize() {
        let (header, tuples) = edges();
        let family = SortedTrieFamily::<kermit_ds::TreeTrie>::default();
        let subject = RelationSubject {
            family: &family,
            header: &header,
            tuples: &tuples,
        };
        let built = subject.build();
        assert!(subject.materialize(&built).is_none());
        const {
            assert!(<RelationSubject<'static, SortedTrieFamily<kermit_ds::TreeTrie>> as Subject>::STREAMS)
        };
    }
}
