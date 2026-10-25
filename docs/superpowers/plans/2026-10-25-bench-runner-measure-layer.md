# Bench Runner Measure Layer Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this
> plan task-by-task (the owner chose inline execution, 2026-10-25). Steps use checkbox (`- [ ]`)
> syntax for tracking.

**Goal:** One untimed describe build per query and cell feeds every untimed number, a shared
measure layer fixes the order describe → iteration → build → end_to_end, `space` becomes an
exact `values` entry, `bench probe` screens benchmarks without Criterion, and reports record
each query's plan (report schema v6).

**Architecture:** A new `kermit/src/bench/measure.rs` holds the methodology once: a `Subject`
trait (build, heap bytes, exercise), `Described::new` (the describe step), and
`Described::measure(self, …)`, which consumes the describe set, so no other index set is
alive while a build is timed. `bench run`, `bench join` and `bench probe` measure a
`QuerySubject`, and `bench ds` measures a `RelationSubject`. kermit-lab learns the report's
new `values` list and an `estimator` column first, so every phase keeps the gate green.

**Tech Stack:** Rust nightly (Cargo workspace, clap, Criterion 0.8, serde_json), Python 3.13
(kermit-lab: pandas, matplotlib, pytest, uv), Nix dev shell.

**Spec:** `docs/specs/2026-10-25-bench-runner-measure-layer-design.md` (faf2288). Read it before
Task 1. Where this plan and the spec disagree, the spec governs: stop and ask.

---

## Execution constraints (read first)

- **Host sharing.** The 2026-10-24 sweep holds this host until its chain exits. Check it before
  any `cargo` or `uv run` command:
  ```bash
  pgrep -af "[k]ermit-bench-runs/.*/bin/kermit" ; tail -n 3 /tb/Source/Academia/kermit-bench-runs/full-sweep-2026-10-24/logs/chain.log
  ```
  - If a sweep binary is running or the log has no `CHAIN_EXIT` line yet, **do not build, test
    or run anything**. Edits and commits are allowed; mark each skipped "Run" step as pending.
  - Run the pending steps, in order, once the chain has exited, unless the owner says
    otherwise.
- **Toolchain.** Every `cargo` and `uv` command runs inside the dev shell, with two build jobs:
  ```bash
  CARGO_BUILD_JOBS=2 nix develop --command <command>
  ```
  - Never run `cargo fmt` outside `nix develop` (CLAUDE.md).
  - If `cargo fmt --check` flags files this plan never touched, that is toolchain skew
    (memory: rustfmt drift). Don't reformat them; report it.
- **Commits.**
  - Stage files by name (`git add <path>`), never `git add -A`.
  - Every commit message ends with these two trailer lines:
    ```
    Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
    Claude-Session: https://claude.ai/code/session_01CUsjYmVxrLR7qsDPJz5gPb
    ```
  - Run `nix develop --command cargo fmt --all` before every commit that touches Rust.
- **Branch.** Stay on `aidanb/benchmarking`, the loom branch. Landing on master is a separate
  step, for the owner to decide.

## File structure

| File | Responsibility | Tasks |
|---|---|---|
| `python/kermit-lab/kermit_lab/loader.py` | `ValueEntry`, `BenchReport.values`, `iter_values`, `VALUES_SCHEMA` | 1, 11 |
| `python/kermit-lab/kermit_lab/frame.py` | value rows, `estimator` column, `plan` / `answer_rows` axis keys | 2 |
| `python/kermit-lab/kermit_lab/encoding.py` | the `estimator` channel and its defaults; NaN-safe error bars | 3 |
| `python/kermit-lab/kermit_lab/plot.py` | `kl.plot(…, estimator=)` | 3 |
| `python/kermit-lab/kermit_lab/analysis.py` | `speedup_table` takes Criterion rows only | 3 |
| `python/kermit-lab/kermit_lab/drivers/main.py` | `plot --estimator` | 3 |
| `python/kermit-lab/kermit_lab/__init__.py` | `SCHEMA_VERSION = 6` | 11 |
| `python/kermit-lab/tests/*` | tests for the above; `fixture_values_tree` in `conftest.py` | 1–3, 9–11, 13 |
| `kermit/src/bench_report.rs` | `Estimator`, `ValueEntry`, `BenchReport.values`, `BenchKind::Probe`, `format_bytes`, v6 | 4, 10, 11 |
| `kermit/src/bench/measure.rs` (new) | `Subject`, `Described`, `CriterionSettings`, `MeasureRequest`, `Measured`, `QuerySubject`, `RelationSubject` | 5–7, 10 |
| `kermit/src/bench/mod.rs` | `pub mod measure`, `Metric::is_timed`; space helpers deleted | 5, 6, 10, 13 |
| `kermit/src/db/lifecycle.rs` | `Prepared::variable_order` | 8 |
| `kermit/src/bench/ds.rs` | `bench ds` on the measure layer | 9 |
| `kermit/src/bench/run.rs` | `bench run` / `join` / `probe` on the measure layer, reports pushed per query | 10, 13 |
| `kermit/src/execution.rs` | `JOIN_MUTATES` doc | 10 |
| `kermit/src/measurement.rs` | deleted | 10 |
| `kermit/src/main.rs` | `CriterionSettings` wiring, `RunSelection`, `bench probe` | 9, 10, 12, 13 |
| `kermit/tests/cli_join_tests.rs`, `cli_bench_join_axes.rs`, `cli_hash_trie_layout_expansion.rs` | follow `space` into `values`; v6 | 9–11 |
| `kermit/tests/cli_bench_probe.rs` (new) | `bench probe` end to end | 13 |
| docs (`docs/specs/*`, `CLAUDE.md`, `BENCHMARKING.md`, `USAGE.md`, `ARCHITECTURE.md`, kermit-lab README / CHANGELOG) | v6, the measure layer, `bench probe` | 14, 15 |

---

## Phase 1: kermit-lab reads `values` (no binary change)

### Task 1: loader parses `values`

**Files:**
- Modify: `python/kermit-lab/kermit_lab/loader.py`
- Test: `python/kermit-lab/tests/test_loader.py`

- [ ] **Step 1: Write the failing tests**

In `tests/test_loader.py`, change the import line to
`from kermit_lab.loader import SchemaError, iter_values, load_reports` and append:

```python
def _report_with_values(tmp_path: Path, values: list[dict] | None) -> Path:
    """One schema-5 probe report whose ``values`` list is ``values`` (absent
    when ``None``)."""
    obj = {
        "schema_version": 5,
        "kind": "probe",
        "metadata": [],
        "axes": {"query": "q1", "data_structure": "TreeTrie"},
        "criterion_groups": [],
    }
    if values is not None:
        obj["values"] = values
    p = tmp_path / "values.json"
    p.write_text(json.dumps([obj]))
    return p


def test_values_parse_into_value_entries(tmp_path: Path) -> None:
    p = _report_with_values(tmp_path, [
        {"function": "space", "metric": "space", "estimator": "exact", "value": 65664},
        {"function": "build", "metric": "time", "estimator": "single", "value": 1361000},
    ])
    [r] = load_reports([p])
    assert [(v.function, v.metric, v.estimator, v.value) for v in r.values] == [
        ("space", "space", "exact", 65664.0),
        ("build", "time", "single", 1361000.0),
    ]
    assert r.has_metric("space")
    assert r.has_metric("time")


def test_a_report_without_values_has_none(tmp_path: Path) -> None:
    [r] = load_reports([_report_with_values(tmp_path, None)])
    assert r.values == []
    assert not r.has_metric("space")


def test_iter_values_pairs_each_entry_with_its_report(tmp_path: Path) -> None:
    p = _report_with_values(tmp_path, [
        {"function": "space", "metric": "space", "estimator": "exact", "value": 8},
    ])
    reports = load_reports([p])
    [(report, entry)] = list(iter_values(reports))
    assert report is reports[0]
    assert entry.value == 8.0
```

- [ ] **Step 2: Run them to verify they fail**

Run: `CARGO_BUILD_JOBS=2 nix develop --command bash -c 'cd python/kermit-lab && uv run pytest tests/test_loader.py -q'`
Expected: collection error, `ImportError: cannot import name 'iter_values'`.

- [ ] **Step 3: Implement**

In `kermit_lab/loader.py`, after the `CriterionGroupRef` dataclass, add:

```python
@dataclass(frozen=True)
class ValueEntry:
    """One measurement made without Criterion: a ``values`` entry.

    ``value`` is in the metric's unit, as Criterion's estimates are: ns for
    time, bytes for space. ``estimator`` says how it was taken: ``"exact"``
    for a deterministic quantity (a query's ``space``), ``"single"`` for one
    run's wall time (``bench probe``).
    """

    function: str
    metric: str  # "time" | "space"
    estimator: str  # "exact" | "single"
    value: float
```

In `BenchReport`, add the field between `criterion_groups` and `source_path`:

```python
    criterion_groups: list[CriterionGroupRef]
    values: list[ValueEntry] = field(default_factory=list)
    source_path: Path = field(default_factory=Path)
```

Replace `has_metric` with:

```python
    def has_metric(self, metric: str) -> bool:
        """True iff a ``CriterionGroupRef`` or a ``values`` entry records ``metric``."""
        return any(g.metric == metric for g in self.criterion_groups) or any(
            v.metric == metric for v in self.values
        )
```

In `_parse_one`, inside the `BenchReport(...)` call, after the `criterion_groups=[...]`
argument, add:

```python
            values=[
                ValueEntry(
                    function=v["function"],
                    metric=v["metric"],
                    estimator=v["estimator"],
                    value=float(v["value"]),
                )
                for v in obj.get("values", [])
            ],
```

At the end of the module, after `iter_function_data`, add:

```python
def iter_values(
    reports: Iterable[BenchReport],
) -> Iterator[tuple[BenchReport, ValueEntry]]:
    """Yield ``(report, entry)`` for every ``values`` entry in ``reports``:
    the measurements made without Criterion, which need no
    ``criterion_root``."""
    for report in reports:
        for entry in report.values:
            yield report, entry
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `CARGO_BUILD_JOBS=2 nix develop --command bash -c 'cd python/kermit-lab && uv run pytest tests/test_loader.py -q'`
Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add python/kermit-lab/kermit_lab/loader.py python/kermit-lab/tests/test_loader.py
git commit -F - <<'EOF'
feat(kermit-lab): parse a report's values list

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01CUsjYmVxrLR7qsDPJz5gPb
EOF
```

### Task 2: value rows, the `estimator` column and the new axis keys

**Files:**
- Modify: `python/kermit-lab/kermit_lab/frame.py`
- Modify: `python/kermit-lab/tests/conftest.py` (new fixture)
- Test: `python/kermit-lab/tests/test_frame.py`

- [ ] **Step 1: Add the fixture**

Append to `tests/conftest.py`:

```python
def _add_values(path: Path, values: list[dict]) -> None:
    """Add a ``values`` list to the one report in ``path``."""
    reports = json.loads(path.read_text())
    reports[0]["values"] = values
    path.write_text(json.dumps(reports))


@pytest.fixture
def fixture_values_tree(tmp_path: Path) -> dict:
    """Reports carrying ``values`` (the measurements made without Criterion),
    labelled v5 so they load before the v6 bump: a ``run`` report with
    Criterion ``build`` and ``iteration`` functions and an exact ``space``
    value, and a ``probe`` report with an exact ``space`` and single-run
    ``build`` and ``iteration`` values and no Criterion function. Both carry
    the ``plan`` and ``answer_rows`` axes."""
    criterion_root = tmp_path / "target" / "criterion"
    reports_dir = tmp_path / "reports"
    criterion_root.mkdir(parents=True)
    reports_dir.mkdir()
    axes = {
        "benchmark": "triangle",
        "query": "triangle",
        "data_structure": "TreeTrie",
        "algorithm": "LeapfrogTriejoin",
        "optimiser": "cost-based",
        "tuples": 12,
        "index_rows": 9,
        "plan": "X, Y, Z",
        "answer_rows": 3,
    }
    groups: list[tuple[str, str, str]] = []
    for phase, point in (("build", 300.0), ("iteration", 100.0)):
        samples = [(i + 1, point * (i + 1)) for i in range(10)]
        _write_function_dir(criterion_root, _FunctionSpec("run", phase, "time", point, samples))
        groups.append(("run", phase, "time"))
    run = _write_report(
        reports_dir, "run", kind="run", axes=axes, metadata=[], groups=groups, schema_version=5,
    )
    _add_values(run, [{"function": "space", "metric": "space", "estimator": "exact", "value": 6400}])
    probe = _write_report(
        reports_dir, "probe", kind="probe", axes=axes, metadata=[], groups=[], schema_version=5,
    )
    _add_values(probe, [
        {"function": "space", "metric": "space", "estimator": "exact", "value": 6400},
        {"function": "build", "metric": "time", "estimator": "single", "value": 450},
        {"function": "iteration", "metric": "time", "estimator": "single", "value": 150},
    ])
    return {
        "criterion_root": criterion_root,
        "reports_dir": reports_dir,
        "paths": sorted([probe, run]),
    }
```

- [ ] **Step 2: Write the failing tests**

In `tests/test_frame.py`, add `"estimator"` to the `expected` set in
`test_summary_columns_present`, and append:

```python
def test_criterion_rows_carry_the_criterion_estimator(df):
    assert set(df["estimator"]) == {"criterion"}


def test_values_become_summary_rows(fixture_values_tree):
    df = load(fixture_values_tree["paths"], fixture_values_tree["criterion_root"])
    # run: two Criterion functions and one value; probe: three values.
    assert len(df) == 6
    assert set(df["estimator"]) == {"criterion", "exact", "single"}
    criterion = df[df["estimator"] == "criterion"]
    assert set(criterion["phase"]) == {"build", "iteration"}
    assert criterion["criterion_function"].notna().all()


def test_an_exact_value_is_its_own_zero_width_interval(fixture_values_tree):
    df = load(fixture_values_tree["paths"], fixture_values_tree["criterion_root"])
    space = df[df["metric"] == "space"]
    assert len(space) == 2
    assert set(space["estimator"]) == {"exact"}
    for column in ("mean_ns", "mean_lo", "mean_hi", "median_ns", "median_lo", "median_hi"):
        assert (space[column] == 6400.0).all(), column
    assert (space["mean_se"] == 0.0).all()
    assert space["phase"].isna().all()
    assert space["criterion_group"].isna().all()


def test_a_single_run_has_a_phase_and_no_interval(fixture_values_tree):
    df = load(fixture_values_tree["paths"], fixture_values_tree["criterion_root"])
    single = df[df["estimator"] == "single"].set_index("phase")
    assert single.loc["build", "mean_ns"] == 450.0
    assert single.loc["iteration", "median_ns"] == 150.0
    assert single["mean_lo"].isna().all()
    assert single["mean_se"].isna().all()
    assert set(single["kind"]) == {"probe"}


def test_plan_and_answer_rows_are_typed_columns(fixture_values_tree):
    df = load(fixture_values_tree["paths"], fixture_values_tree["criterion_root"])
    assert set(df["plan"]) == {"X, Y, Z"}
    assert str(df["answer_rows"].dtype) == "Int64"
    assert set(df["answer_rows"]) == {3}
```

- [ ] **Step 3: Run them to verify they fail**

Run: `CARGO_BUILD_JOBS=2 nix develop --command bash -c 'cd python/kermit-lab && uv run pytest tests/test_frame.py -q'`
Expected: failures with `KeyError: 'estimator'` (and `'plan'`).

- [ ] **Step 4: Implement**

In `kermit_lab/frame.py`:

1. Change the loader import to:
   ```python
   from .loader import (
       BenchReport,
       CriterionGroupRef,
       ValueEntry,
       iter_function_data,
       iter_values,
       load_reports,
       phase_of,
   )
   ```
2. Append `"plan",` to `_AXIS_STR_KEYS` (after `"relation_path"`) and `"answer_rows",` to
   `_AXIS_INT_KEYS` (after `"index_rows"`).
3. Change the head of `_SUMMARY_COLUMNS_CORE` to `"kind", "metric", "phase", "estimator",`.
4. Replace `_summary_row` with these three functions:

```python
def _axis_cells(report: BenchReport, opt_axes: Sequence[str]) -> dict:
    """The columns every row of ``report`` shares: its kind, schema version,
    axes and source."""
    row: dict = {"kind": report.kind, "schema_version": report.schema_version}
    for key in _AXIS_STR_KEYS:
        v = report.axis(key)
        row[key] = v if isinstance(v, str) else pd.NA
    for key in _AXIS_INT_KEYS:
        v = report.axis(key)
        row[key] = v if isinstance(v, int) and not isinstance(v, bool) else pd.NA
    for key in _AXIS_BOOL_KEYS:
        v = report.axis(key)
        row[key] = v if isinstance(v, bool) else pd.NA
    for key in opt_axes:
        v = report.axis(key)
        # Keep native type (str / bool / int); only None becomes NA. Booleans
        # (config flags) intentionally pass through — they render as True/False
        # categories when bound to a channel.
        row[key] = v if v is not None else pd.NA
    row["source_path"] = str(report.source_path)
    return row


def _summary_row(
    report: BenchReport,
    group_ref: CriterionGroupRef,
    data: FunctionData,
    opt_axes: Sequence[str],
) -> dict:
    """One summary row for a Criterion function: its estimates, labelled
    ``criterion``."""
    phase = phase_of(group_ref.function)
    row = _axis_cells(report, opt_axes)
    row["metric"] = group_ref.metric
    row["phase"] = phase if phase is not None else pd.NA
    row["estimator"] = "criterion"
    row["mean_ns"] = data.mean.point
    row["mean_lo"] = data.mean.lower
    row["mean_hi"] = data.mean.upper
    row["mean_se"] = data.mean.standard_error
    row["median_ns"] = data.median.point
    row["median_lo"] = data.median.lower
    row["median_hi"] = data.median.upper
    row["criterion_group"] = group_ref.group
    row["criterion_function"] = group_ref.function
    return row


def _value_row(report: BenchReport, entry: ValueEntry, opt_axes: Sequence[str]) -> dict:
    """One summary row for a ``values`` entry. An ``exact`` value is its own
    zero-width interval, as Criterion's deterministic space rows read; a
    ``single`` run has no interval, so its bounds and standard error are
    NaN."""
    phase = phase_of(entry.function)
    row = _axis_cells(report, opt_axes)
    row["metric"] = entry.metric
    row["phase"] = phase if phase is not None else pd.NA
    row["estimator"] = entry.estimator
    exact = entry.estimator == "exact"
    bound = entry.value if exact else float("nan")
    row["mean_ns"] = entry.value
    row["mean_lo"] = bound
    row["mean_hi"] = bound
    row["mean_se"] = 0.0 if exact else float("nan")
    row["median_ns"] = entry.value
    row["median_lo"] = bound
    row["median_hi"] = bound
    row["criterion_group"] = pd.NA
    row["criterion_function"] = pd.NA
    return row
```

5. In `_summary_from_reports`, after the `rows = [...]` comprehension, add:
   ```python
       rows += [_value_row(report, entry, opt_axes) for report, entry in iter_values(reports)]
   ```
6. In the docstring of `load`, replace `One row per ``(report × criterion_group)``.` with
   `One row per ``(report × criterion_group)`` and per ``values`` entry.`, and after
   ``` ``kind``, ``metric``, ``phase``, ``` add
   ``` ``estimator`` (``criterion``, ``exact`` or ``single``), ```.

- [ ] **Step 5: Run the tests to verify they pass**

Run: `CARGO_BUILD_JOBS=2 nix develop --command bash -c 'cd python/kermit-lab && uv run pytest -q'`
Expected: all pass (the whole suite, since every frame now has the new column).

- [ ] **Step 6: Commit**

```bash
git add python/kermit-lab/kermit_lab/frame.py python/kermit-lab/tests/conftest.py python/kermit-lab/tests/test_frame.py
git commit -F - <<'EOF'
feat(kermit-lab): summary rows for values, an estimator column, plan and answer_rows

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01CUsjYmVxrLR7qsDPJz5gPb
EOF
```

### Task 3: plots and `speedup_table` take Criterion rows unless asked

**Files:**
- Modify: `python/kermit-lab/kermit_lab/encoding.py`, `plot.py`, `analysis.py`, `drivers/main.py`
- Test: `python/kermit-lab/tests/test_encoding.py`, `test_plot.py`, `test_cli.py`, `test_analysis.py`

- [ ] **Step 1: Write the failing tests**

Append to `tests/test_encoding.py`:

```python
def _df_with_estimators():
    """``_df`` labelled with estimators, plus one single run on iteration."""
    df = _df()
    df["estimator"] = ["criterion", "criterion", "exact"]
    single = pd.DataFrame(
        {
            "metric": ["time"],
            "phase": ["iteration"],
            "data_structure": ["TreeTrie"],
            "algorithm": ["LeapfrogTriejoin"],
            "tuples": pd.array([10], dtype="Int64"),
            "query": ["triangle"],
            "mean_ns": [5000.0],
            "mean_lo": [float("nan")],
            "mean_hi": [float("nan")],
            "estimator": ["single"],
        }
    )
    return pd.concat([df, single], ignore_index=True)


def test_select_takes_criterion_time_rows_by_default() -> None:
    amap = AestheticMap(kind="bar", x="data_structure", y="time")
    out = select(_df_with_estimators(), amap)
    assert set(out.estimator) == {"criterion"}
    assert len(out) == 2


def test_select_takes_single_runs_when_asked() -> None:
    amap = AestheticMap(kind="bar", x="data_structure", y="time", estimator="single")
    out = select(_df_with_estimators(), amap)
    assert list(out.mean_ns) == [5000.0]


def test_select_takes_space_rows_whatever_their_estimator() -> None:
    amap = AestheticMap(kind="bar", x="data_structure", y="space")
    assert len(select(_df_with_estimators(), amap)) == 1


def test_a_frame_without_estimators_has_no_single_runs() -> None:
    amap = AestheticMap(kind="bar", x="data_structure", y="time", estimator="single")
    with pytest.raises(InsufficientAxesError):
        select(_df(), amap)


def test_a_single_run_draws_without_error_bars() -> None:
    amap = AestheticMap(kind="bar", x="data_structure", y="time", estimator="single")
    [series] = resolve_scalar(select(_df_with_estimators(), amap), amap)
    assert series.ys == [5000.0]
    assert series.lo == [0.0]
    assert series.hi == [0.0]
```

Append to `tests/test_plot.py`:

```python
def test_bar_of_single_runs(fixture_values_tree) -> None:
    df = load(fixture_values_tree["paths"], fixture_values_tree["criterion_root"])
    fig = plot(df, kind="bar", x="data_structure", y="time", phase="build", estimator="single")
    assert isinstance(fig, Figure)
    plt.close(fig)
```

Append to `tests/test_cli.py`:

```python
def test_general_plot_subcommand_plots_single_runs(fixture_values_tree, tmp_path: Path) -> None:
    out = tmp_path / "single.pdf"
    rc = main([
        "plot", *[str(p) for p in fixture_values_tree["paths"]],
        "--criterion-root", str(fixture_values_tree["criterion_root"]),
        "--out", str(out),
        "--kind", "bar", "--y", "time", "--x", "data_structure",
        "--phase", "build", "--estimator", "single",
    ])
    assert rc == 0
    assert out.exists() and out.stat().st_size > 0
```

Append to `tests/test_analysis.py`, in the `speedup_table` section:

```python
def test_speedup_table_ignores_single_runs() -> None:
    df = _build_mode_rows({"serial": [100.0], "parallel:2": [50.0]})
    df["estimator"] = "criterion"
    # A probe's single run of the threaded build: were it pooled with the
    # Criterion estimate, the speedup would no longer be 2.
    single = df[df["ds_build_mode"] == "parallel:2"].copy()
    single["estimator"] = "single"
    single["mean_ns"] = 1.0
    single["criterion_group"] = pd.NA
    single["criterion_function"] = pd.NA
    mixed = pd.concat([df, single], ignore_index=True)
    assert speedup_table(mixed).iloc[0]["speedup"] == pytest.approx(2.0)
```

- [ ] **Step 2: Run them to verify they fail**

Run: `CARGO_BUILD_JOBS=2 nix develop --command bash -c 'cd python/kermit-lab && uv run pytest tests/test_encoding.py tests/test_plot.py tests/test_cli.py tests/test_analysis.py -q'`
Expected: failures, including `TypeError: AestheticMap.__init__() got an unexpected keyword argument 'estimator'`.

- [ ] **Step 3: Implement**

In `kermit_lab/encoding.py`:

1. Replace the `AestheticMap` docstring and add the field after `phase`:
   ```python
   @dataclass(frozen=True)
   class AestheticMap:
       """Binding of visual channels to columns. ``y`` selects the metric, and
       ``estimator`` which time rows: Criterion's estimates (the default) or
       ``bench probe``'s single runs (``"single"``)."""
   ```
   ```python
       phase: str = "iteration"
       estimator: str = "criterion"
   ```
2. Replace `select` and add `_time_rows` after it:

```python
def select(df: pd.DataFrame, amap: AestheticMap) -> pd.DataFrame:
    """Filter to the metric/phase rows this map needs; raise if empty.

    Time rows are also filtered to ``amap.estimator``, so a single run never
    joins a plot of Criterion estimates. Space rows are taken whatever their
    estimator: a v5 Criterion ``space`` and a v6 exact one are the same bytes.
    """
    metric = metric_of(amap.y)
    sub = apply_filter(df, amap)
    sub = sub[sub.metric == metric]
    if metric == "time":
        sub = _time_rows(sub, amap)
    if sub.empty:
        raise InsufficientAxesError(
            f"no {metric} rows (phase={amap.phase}, estimator={amap.estimator}) "
            f"after filter {dict(amap.filter)}"
        )
    return sub


def _time_rows(df: pd.DataFrame, amap: AestheticMap) -> pd.DataFrame:
    """``df``'s rows on ``amap.phase`` taken by ``amap.estimator``. A frame
    without an ``estimator`` column (one built by hand) holds Criterion rows
    only."""
    rows = df[df.phase == amap.phase]
    if "estimator" in rows.columns:
        return rows[rows.estimator == amap.estimator]
    return rows if amap.estimator == "criterion" else rows.iloc[0:0]
```

3. In `resolve_scalar`, replace the `lo = …` / `hi = …` lines with:
   ```python
           # A single run has no interval (NaN bounds): it draws without error bars.
           lo = (g["mean_ns"] - g["mean_lo"]).clip(lower=0).fillna(0.0).tolist()
           hi = (g["mean_hi"] - g["mean_ns"]).clip(lower=0).fillna(0.0).tolist()
   ```
4. In `resolve_tradeoff`, replace `t = df[(df.metric == "time") & (df.phase == amap.phase)]`
   with `t = _time_rows(df[df.metric == "time"], amap)`.

In `kermit_lab/plot.py`, add the parameter `estimator: str = "criterion",` after
`phase: str = "iteration",`. Pass `estimator=estimator` to `AestheticMap(...)`, and change the
docstring to:
```python
    """Render one figure. ``kind`` ∈ {bar, line, scatter, violin}. Time rows
    are Criterion estimates unless ``estimator="single"`` asks for
    ``bench probe``'s single runs."""
```

In `kermit_lab/analysis.py`, in `speedup_table`, after the line
`on_phase = ((df["metric"] == "time") & (df["phase"] == phase)).fillna(False).astype(bool)`,
add:
```python
    if "estimator" in df.columns:
        # Criterion estimates only: a single run is not a replicate of a sample.
        on_phase &= (df["estimator"] == "criterion").fillna(False).astype(bool)
```

In `kermit_lab/drivers/main.py`, after the `--y` argument of `p_plot`, add:
```python
    p_plot.add_argument("--estimator", choices=["criterion", "single"], default="criterion",
                        help="time rows to plot: Criterion estimates (default) or "
                             "bench probe single runs")
```
In `_dispatch`'s `plot` branch, pass `estimator=args.estimator` after `phase=args.phase`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `CARGO_BUILD_JOBS=2 nix develop --command bash -c 'cd python/kermit-lab && uv run pytest -q'`
Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add python/kermit-lab/kermit_lab/encoding.py python/kermit-lab/kermit_lab/plot.py python/kermit-lab/kermit_lab/analysis.py python/kermit-lab/kermit_lab/drivers/main.py python/kermit-lab/tests/test_encoding.py python/kermit-lab/tests/test_plot.py python/kermit-lab/tests/test_cli.py python/kermit-lab/tests/test_analysis.py
git commit -F - <<'EOF'
feat(kermit-lab): plots and speedup_table take Criterion rows unless asked

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01CUsjYmVxrLR7qsDPJz5gPb
EOF
```

---

## Phase 2: the measure layer and both runners

### Task 4: report types (`values`, `Estimator`, `BenchKind::Probe`, `format_bytes`)

**Files:**
- Modify: `kermit/src/bench_report.rs`
- Modify: `kermit/src/measurement.rs`, `kermit/src/bench/ds.rs`, `kermit/src/bench/run.rs` (callers)

- [ ] **Step 1: Write the failing tests**

In `kermit/src/bench_report.rs`'s `tests` module:

1. Append:

```rust
    #[test]
    fn a_value_entry_serialises_its_metric_and_estimator_in_lower_case() {
        let entry = ValueEntry {
            function: "space".into(),
            metric: ReportMetric::Space,
            estimator: Estimator::Exact,
            value: 65_664,
        };
        assert_eq!(
            serde_json::to_value(&entry).unwrap(),
            serde_json::json!({
                "function": "space", "metric": "space", "estimator": "exact", "value": 65664
            })
        );
        assert_eq!(serde_json::json!(Estimator::Single), "single");
    }

    #[test]
    fn values_are_always_serialised() {
        let report = BenchReport::new(BenchKind::Run, &[], BTreeMap::new(), vec![], vec![]);
        assert_eq!(serde_json::to_value(&report).unwrap()["values"], serde_json::json!([]));
    }

    #[test]
    fn format_bytes_picks_unit_per_threshold() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(768), "768 B");
        assert_eq!(format_bytes(1024), "1.00 KiB");
        assert_eq!(format_bytes(2 * 1024 * 1024), "2.00 MiB");
        assert_eq!(format_bytes(3 * 1024 * 1024 * 1024), "3.00 GiB");
    }
```

2. In `every_report_records_the_allocator` and `a_kind_displays_as_it_serialises`, change
   `[BenchKind::Join, BenchKind::Ds, BenchKind::Run]` to
   `[BenchKind::Join, BenchKind::Ds, BenchKind::Run, BenchKind::Probe]`.
3. Add a trailing `vec![]` argument to every `BenchReport::new(...)` call in the module (four
   calls). In `json_report_round_trips`, after the `schema_version` assertion, add
   `assert_eq!(json[0]["values"], serde_json::json!([]));`.

- [ ] **Step 2: Run them to verify they fail**

Run: `CARGO_BUILD_JOBS=2 nix develop --command cargo test -p kermit --bin kermit bench_report`
Expected: compile errors (`ValueEntry`, `Estimator`, `BenchKind::Probe`, `format_bytes` not
found; `BenchReport::new` takes 4 arguments).

- [ ] **Step 3: Implement**

In `kermit/src/bench_report.rs`:

1. In the module doc, replace both mentions of ``[`crate::measurement::format_bytes`]`` with
   ``[`format_bytes`]``.
2. Add `Probe` to `BenchKind`, after `Run`:
   ```rust
       /// `bench run`
       Run,
       /// `bench probe`
       Probe,
   ```
   and the arm `| Self::Probe => "probe",` to its `Display` impl.
3. After the `ReportMetric` enum, add:

```rust
/// How a [`ValueEntry`] was taken.
#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Estimator {
    /// A deterministic quantity: a structure's heap bytes, as built.
    Exact,
    /// One run's wall time (`bench probe`): cold, and not comparable with a
    /// Criterion estimate.
    Single,
}

/// One measurement made without Criterion. `value` is in the metric's unit,
/// as Criterion's estimates are: ns for time, bytes for space.
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct ValueEntry {
    /// The measured function: `space` (`{ds}/space` for `bench ds`), or
    /// `build` / `iteration` for a probe's single run.
    pub function: String,
    /// What the value measures.
    pub metric: ReportMetric,
    /// How it was taken.
    pub estimator: Estimator,
    /// ns for time, bytes for space.
    pub value: u64,
}
```

4. In `BenchReport`, after `criterion_groups`, add:
   ```rust
       /// The measurements made without Criterion: exact `space`, and a
       /// probe's single-run times. Always serialised, possibly empty.
       pub values: Vec<ValueEntry>,
   ```
   and change `BenchReport::new` to take and store it:
   ```rust
       pub fn new(
           kind: BenchKind, metadata: &[MetadataLine], mut axes: BTreeMap<String, serde_json::Value>,
           criterion_groups: Vec<CriterionGroupRef>, values: Vec<ValueEntry>,
       ) -> Self {
           axes.insert("allocator".to_string(), serde_json::json!(crate::ALLOCATOR));
           Self {
               schema_version: REPORT_SCHEMA_VERSION,
               kind,
               metadata: metadata.iter().map(ReportField::from).collect(),
               axes,
               criterion_groups,
               values,
           }
       }
   ```
   Also mention the new argument in its doc comment: "…, the list of Criterion functions
   that were run, and the measurements made without Criterion."
5. After `write_metadata_block`, add:

```rust
/// Binary-prefix unit thresholds, so [`byte_unit`] reads as
/// `if typical < BYTES_PER_KIB { ... }` rather than `if typical < 1024.0`.
const BYTES_PER_KIB: f64 = 1024.0;
const BYTES_PER_MIB: f64 = BYTES_PER_KIB * 1024.0;
const BYTES_PER_GIB: f64 = BYTES_PER_MIB * 1024.0;

/// A binary-prefixed unit (`B`, `KiB`, `MiB`, `GiB`) for `typical` bytes,
/// with the factor that scales a byte count into it.
pub(crate) fn byte_unit(typical: f64) -> (f64, &'static str) {
    if typical < BYTES_PER_KIB {
        (1.0, "B")
    } else if typical < BYTES_PER_MIB {
        (1.0 / BYTES_PER_KIB, "KiB")
    } else if typical < BYTES_PER_GIB {
        (1.0 / BYTES_PER_MIB, "MiB")
    } else {
        (1.0 / BYTES_PER_GIB, "GiB")
    }
}

/// `n` bytes as a human-readable string: whole bytes without a decimal
/// (`"768 B"`), larger units with two decimal places (`"1.50 MiB"`).
pub fn format_bytes(n: u64) -> String {
    let (factor, unit) = byte_unit(n as f64);
    if unit == "B" {
        format!("{n} {unit}")
    } else {
        format!("{:.2} {unit}", n as f64 * factor)
    }
}
```

In `kermit/src/measurement.rs`:
- Delete the three `BYTES_PER_*` constants, `format_bytes`, and its test
  `format_bytes_picks_unit_per_threshold`.
- Replace the body of `BytesFormatter::scale` with `crate::bench_report::byte_unit(typical)`,
  and change its doc comment to "A binary-prefixed unit for `typical`, with its scale factor
  ([`crate::bench_report::byte_unit`])."

In `kermit/src/bench/ds.rs`:
- Replace `measurement::format_bytes(relation_bytes)` with `format_bytes(relation_bytes)`.
- Import `format_bytes` from `crate::bench_report`, and drop `measurement` from the `crate::{…}`
  import list.
- Add a trailing `Vec::new()` to its `BenchReport::new(...)` call.

In `kermit/src/bench/run.rs`, add a trailing `Vec::new()` to its `BenchReport::new(...)` call.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `CARGO_BUILD_JOBS=2 nix develop --command cargo test -p kermit --bin kermit`
Expected: all pass.

- [ ] **Step 5: Commit**

```bash
nix develop --command cargo fmt --all
git add kermit/src/bench_report.rs kermit/src/measurement.rs kermit/src/bench/ds.rs kermit/src/bench/run.rs
git commit -F - <<'EOF'
feat(cli): report values, estimators and the probe kind

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01CUsjYmVxrLR7qsDPJz5gPb
EOF
```

### Task 5: the measure layer, part 1: `Subject` and the describe step

**Files:**
- Create: `kermit/src/bench/measure.rs`
- Modify: `kermit/src/bench/mod.rs`

- [ ] **Step 1: Write the module with its failing tests**

Create `kermit/src/bench/measure.rs`. The module-level `allow` is temporary: the runners start
using the module in Tasks 9 and 10, and Task 10 deletes the line.

```rust
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
            assert_eq!(self.live.get(), 0, "a build ran while another set was alive");
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
```

In `kermit/src/bench/mod.rs`, add `pub mod measure;` between `pub mod gen;` and
`pub mod run;`. In the module doc, after the paragraph that starts "`workload` is the value…",
add the paragraph:

```rust
//! `measure` is the methodology every runner shares: one untimed describe
//! build per subject, then the timed metrics in one fixed order.
```

- [ ] **Step 2: Run the tests**

Run: `CARGO_BUILD_JOBS=2 nix develop --command cargo test -p kermit --bin kermit bench::measure`
Expected: 2 passed.

- [ ] **Step 3: Commit**

```bash
nix develop --command cargo fmt --all
git add kermit/src/bench/measure.rs kermit/src/bench/mod.rs
git commit -F - <<'EOF'
feat(bench): the measure layer's Subject and describe step

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01CUsjYmVxrLR7qsDPJz5gPb
EOF
```

### Task 6: the measure layer, part 2: `measure` and the fixed order

**Files:**
- Modify: `kermit/src/bench/measure.rs`, `kermit/src/bench/mod.rs`

- [ ] **Step 1: Write the failing tests**

Append inside `measure.rs`'s `tests` module:

```rust
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
        let measured = Described::new(&subject, false).measure(&request(&metrics, false, &settings));
        let events = subject.events();
        let dropped = first(&events, |e| *e == Event::Drop(0));
        let describe_exercises: Vec<usize> = events
            .iter()
            .enumerate()
            .filter(|(_, e)| **e == Event::Exercise(0))
            .map(|(at, _)| at)
            .collect();
        assert!(!describe_exercises.is_empty(), "iteration ran on the describe set");
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
            let measured =
                Described::new(&subject, false).measure(&request(&[Metric::Space], false, &settings));
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
        let functions: Vec<&str> = measured.values.iter().map(|v| v.function.as_str()).collect();
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
```

- [ ] **Step 2: Run them to verify they fail**

Run: `CARGO_BUILD_JOBS=2 nix develop --command cargo test -p kermit --bin kermit bench::measure`
Expected: compile errors (`CriterionSettings`, `MeasureRequest`, `measure` and `Metric` not
found).

- [ ] **Step 3: Implement**

In `kermit/src/bench/mod.rs`, after the `Metric` enum, add:

```rust
impl Metric {
    /// Whether Criterion times this metric. `space` is exact: it is read off
    /// the describe step, never sampled.
    pub(crate) fn is_timed(self) -> bool { !matches!(self, Metric::Space) }
}
```

In `kermit/src/bench/measure.rs`, replace the `use` line with:

```rust
use {
    super::Metric,
    crate::{
        bench_report::{CriterionGroupRef, Estimator, ReportMetric, ValueEntry},
        BenchArgs,
    },
    criterion::{BatchSize, Criterion},
    std::{
        path::PathBuf,
        time::{Duration, Instant},
    },
};
```

Insert this between the `Subject` trait and `Counted`:

```rust
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
```

After the existing `impl<'s, S: Subject> Described<'s, S>` block, add:

```rust
impl<S: Subject> Described<'_, S> {
    /// Runs `request`'s metrics in the one order every cell follows:
    /// `iteration` on the describe set (on a fresh build per sample when
    /// exercising mutates), then the describe set is dropped, then `build`
    /// and `end_to_end`. Consuming `self` is what keeps any other set from
    /// being alive while a build is timed. Every timed routine returns what
    /// it built, so Criterion frees it after the timer stops.
    pub(crate) fn measure(self, request: &MeasureRequest<'_>) -> Measured {
        let Described {
            subject,
            built,
            bytes,
            build_time,
            counted,
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
            if let Some(counted) = counted {
                values.push(ValueEntry {
                    function: request.function_id("iteration"),
                    metric: ReportMetric::Time,
                    estimator: Estimator::Single,
                    value: nanos(counted.time),
                });
            }
        }
        if !request.metrics.iter().any(|metric| metric.is_timed()) {
            return Measured {
                criterion_groups: Vec::new(),
                values,
            };
        }

        let mut criterion = request.criterion.criterion();
        let mut group = criterion.benchmark_group(request.group);
        let mut criterion_groups = Vec::new();
        let iteration = request.metrics.contains(&Metric::Iteration);
        if iteration && !S::EXERCISE_MUTATES {
            // Over the describe set, built once outside Criterion. The routine
            // counts rows and returns only the count, so memory does not grow
            // with the answer (#65).
            group.bench_function(request.function_id("iteration"), |b| {
                b.iter(|| subject.exercise(&built));
            });
        }
        // From here on, no set but Criterion's own is alive.
        drop(built);
        if iteration && S::EXERCISE_MUTATES {
            // Cold (#92): each timed exercise runs on a set built in the untimed
            // setup, so it pays the expansion it causes instead of finding it
            // done by an earlier sample. The routine hands the set back, so
            // Criterion frees it after the timer stops.
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
        if iteration {
            criterion_groups.push(request.time_ref("iteration"));
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
        if request.metrics.contains(&Metric::EndToEnd) {
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
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `CARGO_BUILD_JOBS=2 nix develop --command cargo test -p kermit --bin kermit bench::measure`
Expected: 7 passed.

- [ ] **Step 5: Commit**

```bash
nix develop --command cargo fmt --all
git add kermit/src/bench/measure.rs kermit/src/bench/mod.rs
git commit -F - <<'EOF'
feat(bench): measure runs iteration on the describe set, then the timed builds

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01CUsjYmVxrLR7qsDPJz5gPb
EOF
```

### Task 7: the two subjects

**Files:**
- Modify: `kermit/src/bench/measure.rs`

- [ ] **Step 1: Write the failing tests**

Append inside `measure.rs`'s `tests` module:

```rust
    use {
        crate::execution::{HashHtj, SortedTrieFamily, TrieLftj},
        kermit::db::{Catalog, Prepared},
        kermit_ds::{HeapSize, LazyExpansion, NoPruning, RelationHeader, Tuples},
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

    /// The two-hop paths over [`edges`], prepared under the lexicographic
    /// optimiser.
    fn two_hops() -> (Catalog, Prepared) {
        use kermit_algos::{JoinQuery, LexicographicOptimiser, Planner, StatisticsLevel};
        let catalog = Catalog::new(vec![edges()], StatisticsLevel::TupleCounts);
        let query: JoinQuery = "Q(X, Z) :- edge(X, Y), edge(Y, Z).".parse().unwrap();
        let planner = Planner::new(Box::new(LexicographicOptimiser));
        let prepared = kermit::db::prepare(&catalog, &query, &planner).unwrap();
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
        assert!(!<QuerySubject<'static, TrieLftj<kermit_ds::TreeTrie>> as Subject>::EXERCISE_MUTATES);
    }

    #[test]
    fn a_lazy_hash_trie_query_subject_mutates() {
        assert!(<QuerySubject<
            'static,
            HashHtj<SipHashStrategy, NoPruning, LazyExpansion>,
        > as Subject>::EXERCISE_MUTATES);
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
```

- [ ] **Step 2: Run them to verify they fail**

Run: `CARGO_BUILD_JOBS=2 nix develop --command cargo test -p kermit --bin kermit bench::measure`
Expected: compile errors (`QuerySubject`, `RelationSubject` not found).

- [ ] **Step 3: Implement**

In `kermit/src/bench/measure.rs`, extend the `use` block to:

```rust
use {
    super::Metric,
    crate::{
        bench_report::{CriterionGroupRef, Estimator, ReportMetric, ValueEntry},
        execution::{ExecutionFamily, RelationFamily},
        BenchArgs,
    },
    criterion::{BatchSize, Criterion},
    kermit::db::{Catalog, Indexes, Prepared},
    kermit_ds::{HeapSize, RelationHeader, TupleView, Tuples},
    std::{
        path::PathBuf,
        time::{Duration, Instant},
    },
};
```

Change the module doc's first paragraph to:

```rust
//! The measurement methodology every `bench` runner shares: one untimed
//! describe build per subject, read for everything a report states without
//! timing it, then the timed metrics in one fixed order. `bench run` and
//! `bench join` measure a prepared query ([`QuerySubject`]); `bench ds`
//! measures one relation ([`RelationSubject`]).
```

After the `Subject` trait, add:

```rust
/// A prepared query on one execution cell. A build is the query's indexes,
/// each copied once out of the catalog's flat tuples, and an exercise counts
/// the join's rows.
pub(crate) struct QuerySubject<'a, F> {
    pub(crate) family: &'a F,
    pub(crate) catalog: &'a Catalog,
    pub(crate) prepared: &'a Prepared,
}

impl<F: ExecutionFamily> Subject for QuerySubject<'_, F> {
    type Built = Indexes<F::Rel>;

    const EXERCISE_MUTATES: bool = F::JOIN_MUTATES;

    fn build(&self) -> Indexes<F::Rel> { self.family.build_indexes(self.catalog, self.prepared) }

    fn heap_bytes(&self, built: &Indexes<F::Rel>) -> Vec<usize> {
        built.iter().map(|(_, index)| index.heap_size_bytes()).collect()
    }

    fn exercise(&self, built: &Indexes<F::Rel>) -> u64 {
        self.family.count_rows(self.prepared, built)
    }
}

/// One relation on one structure: `bench ds`'s subject. A build is the
/// relation, from its file's tuples in file order, and an exercise scans it.
pub(crate) struct RelationSubject<'a, F> {
    pub(crate) family: &'a F,
    pub(crate) header: &'a RelationHeader,
    pub(crate) tuples: &'a Tuples,
}

impl<F: RelationFamily> Subject for RelationSubject<'_, F> {
    type Built = F::Rel;

    /// A scan reads an unexpanded child's pending rows and expands nothing
    /// (`HashTrie::for_each_tuple`), so no structure's scan mutates it.
    const EXERCISE_MUTATES: bool = false;

    fn build(&self) -> F::Rel {
        self.family
            .build_index(self.header.clone(), TupleView::all(self.tuples))
    }

    fn heap_bytes(&self, built: &F::Rel) -> Vec<usize> { vec![built.heap_size_bytes()] }

    fn exercise(&self, built: &F::Rel) -> u64 { F::scan(built) }
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `CARGO_BUILD_JOBS=2 nix develop --command cargo test -p kermit --bin kermit bench::measure`
Expected: 10 passed.

- [ ] **Step 5: Commit**

```bash
nix develop --command cargo fmt --all
git add kermit/src/bench/measure.rs
git commit -F - <<'EOF'
feat(bench): QuerySubject and RelationSubject

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01CUsjYmVxrLR7qsDPJz5gPb
EOF
```

### Task 8: `Prepared::variable_order`

**Files:**
- Modify: `kermit/src/db/lifecycle.rs`

- [ ] **Step 1: Write the failing tests**

Append inside `lifecycle.rs`'s `tests` module:

```rust
    /// The plan's order by name. The lexicographic optimiser binds the head's
    /// variables in head order, which this body reverses.
    #[test]
    fn the_variable_order_names_the_plan() {
        let c = catalog(vec![("p", 2, vec![vec![1, 10]])]);
        assert_eq!(
            prepared_for(&c, "Answer(Y, X) :- p(X, Y).").variable_order(),
            ["Y", "X"]
        );
        assert_eq!(
            prepared_for(&c, "Q(X) :- p(X, c10).").variable_order(),
            ["X"]
        );
    }

    /// A `_` is planned under the fresh `K<n>` name its rewrite gave it.
    #[test]
    fn a_placeholder_appears_in_the_order_under_its_fresh_name() {
        let c = catalog(vec![("r", 2, vec![vec![1, 2]])]);
        assert_eq!(
            prepared_for(&c, "Q(X) :- r(X, _).").variable_order(),
            ["X", "K0"]
        );
    }
```

- [ ] **Step 2: Run them to verify they fail**

Run: `CARGO_BUILD_JOBS=2 nix develop --command cargo test -p kermit --lib db::lifecycle`
Expected: compile error (`no method named variable_order`).

- [ ] **Step 3: Implement**

In `kermit/src/db/lifecycle.rs`, add `canonical_names` to the `kermit_algos::{…}` import, and
add this method after `head_len` in `impl Prepared`:

```rust
    /// The plan's variable order, by name: the executed query's variables in
    /// the order the join binds them. For one catalog and query it identifies
    /// the whole plan, since [`plan_indexes`] derives every index spec and the
    /// executed query from it. Reports record it as the `plan` axis.
    pub fn variable_order(&self) -> Vec<String> {
        let names = canonical_names(&self.query);
        self.plan
            .variable_ordering
            .iter()
            .map(|&variable| names[variable].clone())
            .collect()
    }
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `CARGO_BUILD_JOBS=2 nix develop --command cargo test -p kermit --lib db::lifecycle`
Expected: all pass.

- [ ] **Step 5: Commit**

```bash
nix develop --command cargo fmt --all
git add kermit/src/db/lifecycle.rs
git commit -F - <<'EOF'
feat(db): Prepared::variable_order names the plan

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01CUsjYmVxrLR7qsDPJz5gPb
EOF
```

### Task 9: `bench ds` on the measure layer

**Files:**
- Modify: `kermit/src/bench/ds.rs`, `kermit/src/main.rs`
- Modify (tests): `kermit/tests/cli_join_tests.rs`, `python/kermit-lab/tests/test_contract.py`

- [ ] **Step 1: Update the tests to the new behaviour (failing)**

In `kermit/tests/cli_join_tests.rs`:

1. Replace `cli_bench_ds_all_metrics` with:

```rust
#[test]
fn cli_bench_ds_all_metrics() {
    let (output, report) = bench_ds_report("first.csv", "tree-trie", &[]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("--- bench ds metadata ---"),
        "stderr missing ds metadata header: {stderr}"
    );
    assert!(
        stderr.contains("data structure:"),
        "missing data structure in metadata"
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("TreeTrie/build"),
        "stdout should contain build benchmark: {stdout}"
    );
    assert!(
        !stdout.contains("TreeTrie/insertion"),
        "the retired insertion function id must not appear: {stdout}"
    );
    assert!(
        stdout.contains("TreeTrie/iteration"),
        "stdout should contain iteration benchmark: {stdout}"
    );
    assert!(
        !stdout.contains("TreeTrie/space"),
        "space is exact, never a Criterion benchmark: {stdout}"
    );
    // The fixed order: iteration on the describe build, then the timed build.
    assert_eq!(functions_of(&report), ["TreeTrie/iteration", "TreeTrie/build"]);
    assert_eq!(report["values"][0]["function"], "TreeTrie/space");
    assert_eq!(report["values"][0]["estimator"], "exact");
    assert!(report["values"][0]["value"].as_u64().unwrap() > 0);
}
```

2. In `cli_bench_ds_writes_json_report`, replace the block from `let groups = report["criterion_groups"]`
   through `assert_eq!(groups[0]["metric"], "space");` with:

```rust
    assert!(
        report["criterion_groups"]
            .as_array()
            .expect("criterion_groups should be an array")
            .is_empty(),
        "space-only runs no Criterion function"
    );
    let values = report["values"]
        .as_array()
        .expect("values should be an array");
    assert_eq!(values.len(), 1, "space-only should yield exactly one value");
    assert_eq!(values[0]["function"], "TreeTrie/space");
    assert_eq!(values[0]["metric"], "space");
    assert_eq!(values[0]["estimator"], "exact");
```

3. Replace `cli_bench_ds_space_only` with:

```rust
#[test]
fn cli_bench_ds_space_only() {
    let (output, report) = bench_ds_report("first.csv", "column-trie", &["-m", "space"]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("ColumnTrie/"),
        "space-only runs no Criterion benchmark: {stdout}"
    );
    assert!(functions_of(&report).is_empty());
    assert_eq!(report["values"][0]["function"], "ColumnTrie/space");
}
```

In `python/kermit-lab/tests/test_contract.py`:
- in `test_bench_ds_report_loads`, add `assert row["estimator"] == "exact"`;
- in `test_bench_ds_reports_the_build_phase`, replace the last line with
  `assert set(df.loc[df["metric"] == "space", "estimator"]) == {"exact"}`.

- [ ] **Step 2: Run them to verify they fail**

Run: `CARGO_BUILD_JOBS=2 nix develop --command cargo test -p kermit --test cli_join_tests cli_bench_ds`
Expected: the three changed tests fail (space still goes through Criterion).

- [ ] **Step 3: Implement**

In `kermit/src/bench/ds.rs`, set the `use` block to:

```rust
use {
    super::{
        measure::{CriterionSettings, Described, MeasureRequest, RelationSubject},
        Metric,
    },
    crate::{
        bench_report::{format_bytes, write_metadata_block, BenchKind, BenchReport, MetadataLine},
        execution::{
            read_relation, Execution, HashTrieFamily, RelationFamily, SortedTrie, SortedTrieFamily,
        },
        options::{with_hash_trie_layout, with_sorted_trie_layout, DsChoices},
    },
    kermit_ds::IndexStructure,
    std::{collections::BTreeMap, fs, io, path::Path},
};
```

Replace the last paragraph of `run_ds_bench`'s doc comment (the one starting "The file is read
once, flat and untimed.") with:

```rust
/// The file is read once, flat and untimed, and measured as a
/// [`RelationSubject`] through the measure layer: one untimed describe
/// build gives the `tuples` metadata, the optimization axes and the exact
/// `{ds}/space`, and `{ds}/iteration` scans that same relation. Only then,
/// with it dropped, are `{ds}/build` and `{ds}/end_to_end` timed, each
/// building from a view of the one buffer.
```

Replace the function itself with:

```rust
fn run_ds_bench<F: RelationFamily>(
    family: &F, relation_path: &Path, metrics: &[Metric], queries_per_build: u32, group_name: &str,
    criterion: &CriterionSettings,
) -> anyhow::Result<BenchReport> {
    // File order is the same whichever structure and Layout ran, so every
    // one of them is fed identical input.
    let (header, tuples) = read_relation(relation_path)?;
    let subject = RelationSubject {
        family,
        header: &header,
        tuples: &tuples,
    };
    let described = Described::new(&subject, false);
    // What the structure stores, which for a set-semantics structure can be
    // fewer than the file's rows; the `tuples` axis has always reported this.
    let tuple_count = F::tuple_count(described.built());

    // This string becomes the report's `data_structure` axis and the
    // `{ds_name}/<metric>` function ids. It is derived from the family's own
    // `Execution`, the value that picked the code path; the label itself is
    // pinned by `IndexStructure::axis_value`.
    let ds_name = family.execution().index_structure().axis_value();
    let relation_bytes = fs::metadata(relation_path).map(|m| m.len()).unwrap_or(0);

    let mut metadata = vec![
        MetadataLine::new("data structure", ds_name),
        MetadataLine::new("relation", relation_path.display()),
        MetadataLine::new("relation size", format_bytes(relation_bytes)),
        MetadataLine::new("tuples", tuple_count),
        MetadataLine::new("arity", header.arity()),
        MetadataLine::new("allocator", crate::ALLOCATOR),
    ];
    if metrics.contains(&Metric::EndToEnd) {
        metadata.push(MetadataLine::new("queries per build", queries_per_build));
    }
    write_metadata_block(&mut io::stderr(), "bench ds metadata", &metadata)?;

    let mut axes = BTreeMap::from([
        ("data_structure".to_string(), serde_json::json!(ds_name)),
        (
            "relation_path".to_string(),
            serde_json::json!(relation_path.display().to_string()),
        ),
        (
            "relation_bytes".to_string(),
            serde_json::json!(relation_bytes),
        ),
        ("tuples".to_string(), serde_json::json!(tuple_count)),
        ("arity".to_string(), serde_json::json!(header.arity())),
    ]);
    // Only meaningful when the end-to-end metric ran; omitting it otherwise
    // keeps historical invocations' reports unchanged.
    if metrics.contains(&Metric::EndToEnd) {
        axes.insert(
            "queries_per_build".to_string(),
            serde_json::json!(queries_per_build),
        );
    }
    // Standard optimization axes: the relation's Layout / Config dimensions
    // (e.g. `ds_layout_seek` for the sorted tries), then the build mode, which
    // only the family that ran the build can report. The `ds_layout_*` /
    // `ds_config_*` / `ds_build_mode` naming convention (see
    // `kermit_iters::HasOptimizationAxes`) guarantees no collision with the
    // base axes assembled above.
    axes.extend(F::optimization_axes(described.built()));
    axes.extend(family.build_mode_axes());

    let measured = described.measure(&MeasureRequest {
        metrics,
        single_run: false,
        queries_per_build,
        criterion,
        group: group_name,
        function_prefix: Some(ds_name),
    });
    Ok(BenchReport::new(
        BenchKind::Ds,
        &metadata,
        axes,
        measured.criterion_groups,
        measured.values,
    ))
}
```

In `dispatch_ds_bench`, change the last parameter from `bench_args: &BenchArgs` to
`criterion: &CriterionSettings`, and pass `criterion` in place of `bench_args` in all three
arms.

In `kermit/src/main.rs`:
- add `measure::CriterionSettings,` to the `bench::{…}` import list;
- in `run_ds_bench_command`, after `let group_name = …;`, add
  `let criterion = CriterionSettings::from_args(bench_args);`, and pass `&criterion` in place
  of `bench_args` to `dispatch_ds_bench`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `CARGO_BUILD_JOBS=2 nix develop --command cargo test -p kermit --test cli_join_tests cli_bench_ds`
Expected: all pass.
Run: `CARGO_BUILD_JOBS=2 nix develop --command cargo clippy --all-targets -p kermit -- -D warnings`
Expected: clean.

- [ ] **Step 5: Run the contract tests**

Run: `CARGO_BUILD_JOBS=2 nix develop --command bash -c 'cargo build -p kermit && cd python/kermit-lab && KERMIT_BIN=$PWD/../../target/debug/kermit uv run pytest tests/test_contract.py -q'`
Expected: all pass.

- [ ] **Step 6: Commit**

```bash
nix develop --command cargo fmt --all
git add kermit/src/bench/ds.rs kermit/src/main.rs kermit/tests/cli_join_tests.rs python/kermit-lab/tests/test_contract.py
git commit -F - <<'EOF'
feat(bench)!: bench ds measures through the measure layer; space is exact

`bench ds` builds its relation once outside Criterion, runs `iteration` on
it, and drops it before `build` and `end_to_end` are timed. Its `space` is
an exact `values` entry, not a Criterion function.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01CUsjYmVxrLR7qsDPJz5gPb
EOF
```

### Task 10: `bench run` and `bench join` on the measure layer

**Files:**
- Modify: `kermit/src/bench/run.rs`, `kermit/src/main.rs`, `kermit/src/bench/mod.rs`,
  `kermit/src/bench/measure.rs`, `kermit/src/bench_report.rs`, `kermit/src/execution.rs`
- Delete: `kermit/src/measurement.rs`
- Modify (tests): `kermit/tests/cli_bench_join_axes.rs`, `kermit/tests/cli_join_tests.rs`,
  `kermit/tests/cli_hash_trie_layout_expansion.rs`, `python/kermit-lab/tests/test_contract.py`

- [ ] **Step 1: Update the tests to the new behaviour (failing)**

In `kermit/tests/cli_bench_join_axes.rs`:

1. In `cli_bench_join_hash_trie_reports_axes_and_all_default_metrics`, replace everything from
   `let mut functions: Vec<&str> = groups` to the end of the function with:

```rust
    let functions: Vec<&str> = groups
        .iter()
        .map(|g| g["function"].as_str().unwrap())
        .collect();
    // The fixed order: iteration on the describe build, then the timed build.
    assert_eq!(functions, ["iteration", "build"]);
    let values = r["values"].as_array().unwrap();
    assert_eq!(values.len(), 1, "{values:?}");
    assert_eq!(values[0]["function"], "space");
    assert_eq!(values[0]["estimator"], "exact");
    assert_eq!(axes["plan"], "X");
    // Nothing verified the query, so nothing counted it.
    assert!(axes.get("answer_rows").is_none(), "{axes}");
}
```

2. In `cli_bench_join_name_is_a_prefix`, change `"-m", "space"` to `"-m", "build"`, since a
   `space`-only run writes no Criterion group.
3. Replace `cli_bench_join_skips_space_for_indexes_that_hold_no_bytes` (doc comment included)
   with:

```rust
/// A TreeTrie built from a view whose filter keeps no row holds no heap: its
/// `space` is an exact 0, recorded like any other value.
#[test]
fn cli_bench_join_records_zero_space_for_indexes_that_hold_no_bytes() {
    let dir = TempDir::new().unwrap();
    let query = dir.path().join("no_match.dl");
    fs::write(&query, "Q(X) :- edge(X, c999).").unwrap();
    let report = tempfile::NamedTempFile::new().unwrap();
    let output = Command::new(kermit_bin())
        .args([
            "bench",
            "--sample-size",
            "10",
            "--measurement-time",
            "1",
            "--warm-up-time",
            "1",
        ])
        .arg("--report-json")
        .arg(report.path())
        .arg("join")
        .arg("--relations")
        .arg(common::cli::edge_fixture())
        .arg("--query")
        .arg(&query)
        .args(["-i", "tree-trie", "-a", "leapfrog-triejoin", "-m", "space"])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "stderr: {stderr}");
    assert!(!stderr.contains("skipping the space metric"), "stderr: {stderr}");
    let reports = reports_of(&report);
    assert_eq!(reports[0]["axes"]["index_rows"], 0);
    assert!(reports[0]["criterion_groups"]
        .as_array()
        .unwrap()
        .is_empty());
    assert_eq!(
        reports[0]["values"],
        serde_json::json!([{"function": "space", "metric": "space", "estimator": "exact", "value": 0}])
    );
}
```

4. In `bench_run_builds_one_index_per_atom_and_reports_each`:
   - change `assert_eq!(functions_of(report), ["build", "iteration", "space"]);` to
     `assert_eq!(functions_of(report), ["build", "iteration"]);`;
   - after `assert_eq!(axes["verified"], true);`, add
     `assert_eq!(axes["plan"], "X, Y");` and `assert_eq!(axes["answer_rows"], 2);`;
   - replace everything from the comment `// \`space\` is one function summing every index` to
     the end of the function with:

```rust
    // `space` is exact: the sum of the `index` lines' bytes.
    let indexed_bytes: u64 = lines
        .iter()
        .map(|line| {
            let bytes = line.strip_suffix(" bytes").unwrap();
            bytes[bytes.rfind(", ").unwrap() + 2..]
                .parse::<u64>()
                .unwrap()
        })
        .sum();
    assert_eq!(
        report["values"],
        serde_json::json!([{"function": "space", "metric": "space", "estimator": "exact", "value": indexed_bytes}])
    );
}
```

5. Delete the now-unused `mean_estimate` helper. Keep `criterion_home`, which
   `bench_run_per_query` still uses.
6. In `bench_run_on_a_lazy_cell_runs_every_metric_over_fresh_builds`, change its doc comment
   to:
   ```rust
   /// A lazy `HashTrie`'s joins expand it, so `iteration` runs on a fresh build
   /// per sample (#92), `end_to_end` builds its own, and `space` is read off
   /// the describe build before its `--verify` count runs.
   ```
   Change the functions assertion to
   `assert_eq!(functions_of(&reports[0]), ["build", "end_to_end", "iteration"]);`, and add
   `assert_eq!(reports[0]["values"][0]["function"], "space");`.

In `kermit/tests/cli_join_tests.rs`, in the `bench run triangle -m space` report test, replace
the block from `let groups = report["criterion_groups"]` through
`assert_eq!(groups[0]["function"], "space");` with:

```rust
    assert!(
        report["criterion_groups"]
            .as_array()
            .expect("criterion_groups should be an array")
            .is_empty(),
        "space-only runs no Criterion function"
    );
    // One exact `space` per query: every index the query reads, together.
    let values = report["values"]
        .as_array()
        .expect("values should be an array");
    assert_eq!(values.len(), 1, "{values:?}");
    assert_eq!(values[0]["metric"], "space");
    assert_eq!(values[0]["function"], "space");
    assert_eq!(values[0]["estimator"], "exact");
```

In `kermit/tests/cli_hash_trie_layout_expansion.rs`, replace the doc comment of
`cli_bench_run_lazy_verify_iteration_and_space_succeed` with:

```rust
/// `space` is read off the describe build before `--verify` counts on it, and
/// this test pins that `--verify` and lazy `iteration` succeed. Triangle's
/// vertex 1 has four out-edges, so its child is a multi-tuple `Unexpanded`
/// list under either pruning setting.
```

In `python/kermit-lab/tests/test_contract.py`, in
`test_bench_join_reports_a_per_query_build_and_space`:
- change the docstring's "one `build` and one `space` function cover both" to "one `build`
  function and one exact `space` value cover both";
- replace the two `space = …` / `assert len(space) …` lines with:
  ```python
      space = raw.loc[raw["metric"] == "space"]
      assert len(space) == 1 and space.iloc[0]["estimator"] == "exact"
  ```

- [ ] **Step 2: Run them to verify they fail**

Run: `CARGO_BUILD_JOBS=2 nix develop --command cargo test -p kermit --test cli_bench_join_axes --test cli_join_tests`
Expected: the changed tests fail.

- [ ] **Step 3: Rewrite the runner**

In `kermit/src/bench/run.rs`:

1. Change the module doc to:
   ```rust
   //! The generic runner behind `bench run` and `bench join`: one execution
   //! cell over one [`PreparedWorkload`], every requested metric, one report
   //! per query, pushed as each query finishes.
   ```
2. Set the `use` block to:

```rust
use {
    super::{
        criterion_directory_name,
        measure::{CriterionSettings, Described, MeasureRequest, QuerySubject},
        workload::PreparedWorkload,
        Metric, CRITERION_MAX_DIRECTORY_NAME_BYTES,
    },
    crate::{
        bench_report::{write_metadata_block, BenchKind, BenchReport, MetadataLine, ReportSink},
        execution::{Execution, ExecutionFamily, HashHtj, SortedTrie, Sweep, TrieLftj},
        options::{
            unreached_flag, with_hash_trie_layout, with_sorted_trie_layout, DsChoices, DsFlag,
            PlannerArgs,
        },
        IndexStructureSelector, JoinAlgorithmSelector,
    },
    kermit::db::{Catalog, Prepared},
    kermit_algos::{IndexSpec, INDEX_ID_PREFIX},
    kermit_bench::BenchmarkDefinition,
    kermit_ds::{RelationHeader, TupleView, Tuples},
    std::{
        collections::{hash_map::Entry, BTreeMap, HashMap},
        io,
    },
};
```

3. In `RunSettings`, replace the last two fields (`verify` and `bench_args`) with:

```rust
    /// Compare each query's answer count with the workload's `expected`
    /// before timing it; a mismatch aborts.
    pub verify: bool,
    /// Criterion sample/measurement/warm-up settings.
    pub criterion: &'a CriterionSettings,
```

4. Replace `run_benchmark`, from its doc comment to its end, with:

```rust
/// Runs every query of `workload` on one execution cell and pushes one
/// report per query to `sink` as soon as that query finishes, each stamped
/// with `settings.kind` and grouped under `settings.prefix`.
///
/// Generic over the [`ExecutionFamily`] so the sorted family
/// (`TrieLftj<R>`) and the hash family (`HashHtj<H, P, E>`) share one body:
/// metadata, Criterion group wiring and report assembly are identical, and
/// the report's `data_structure` / `algorithm` axes come from
/// `family.execution()` — the same value that picked the code path — so a
/// report can never name an algorithm it did not run (issue #56).
///
/// The workload arrives prepared (`Workload::prepare`): every query is
/// validated and planned, and the relations the queries name sit in the
/// catalog as flat tuples, so a query that cannot run never reaches this
/// function (issue #78). Per query, the cell measures a [`QuerySubject`]
/// through the measure layer. One describe build outside Criterion gives the
/// `index` lines, the exact `space` and the count `--verify` checks; then
/// [`Described::measure`] runs `iteration` on that build, drops it, and times
/// `build` and `end_to_end`. With `settings.verify`, a query with an
/// `expected` count is checked before any timer starts, and a mismatch aborts
/// with every earlier query's report already in `sink`.
fn run_benchmark<F: ExecutionFamily>(
    family: &F, workload: &PreparedWorkload, settings: RunSettings<'_>, sink: &mut ReportSink,
) -> anyhow::Result<()> {
    let RunSettings {
        kind,
        prefix,
        planner,
        metrics,
        queries_per_build,
        verify,
        criterion,
    } = settings;
    let PreparedWorkload {
        workload: definition,
        catalog,
        prepared: prepared_queries,
    } = workload;

    // `ds_name`/`algo_name` become the report's identity axes and the
    // on-disk `target/criterion/{group}` names (the group_name below embeds
    // them). The labels are pinned by `IndexStructure::axis_value` and
    // `JoinAlgorithm::axis_value`; see the tests there before changing one.
    let execution = family.execution();
    let ds_name = execution.index_structure().axis_value();
    let algo_name = execution.algorithm().axis_value();

    // Standard optimization axes: the dimensions the structure emits (the
    // `ds_*` prefix convention, see `kermit_iters::HasOptimizationAxes`,
    // guarantees no collision with the base axes assembled per query below)
    // plus the build mode, which comes from the family because it leaves no
    // trace in the structure. They are read off one empty relation, since
    // they depend on the type, not on the data, and a query whose atoms are
    // all constant checks builds no index to read them from.
    let optimization_axes = {
        let empty = Tuples::new(1);
        let probe = family.build_index(
            RelationHeader::new_positional("axes", 1),
            TupleView::all(&empty),
        );
        let mut axes = F::optimization_axes(&probe);
        axes.extend(family.build_mode_axes());
        axes
    };

    for (query_def, prepared) in definition.queries.iter().zip(prepared_queries) {
        // The describe step: this query's one build outside Criterion. Every
        // untimed number in its report is read from it, and `measure`
        // consumes it before any build is timed.
        let subject = QuerySubject {
            family,
            catalog,
            prepared,
        };
        let count = verify && query_def.expected.is_some();
        let described = Described::new(&subject, count);

        let mut metadata = vec![
            MetadataLine::new("benchmark", &definition.name),
            MetadataLine::new("query", &query_def.name),
            MetadataLine::new("data structure", ds_name),
            MetadataLine::new("algorithm", algo_name),
            MetadataLine::new("allocator", crate::ALLOCATOR),
        ];
        for (position, ((spec, &rows), &bytes)) in prepared
            .indexes()
            .iter()
            .zip(prepared.index_rows())
            .zip(described.bytes())
            .enumerate()
        {
            metadata.push(MetadataLine::new(
                "index",
                describe_index(position, spec, rows, bytes),
            ));
        }
        // Correctness gate: the describe step's count is compared before any
        // measurement time is spent on the query, so a wrong answer can never
        // yield a plausible timing. The count came through the same streaming
        // path `iteration` times, so verifying a huge result never
        // materialised it.
        let verified = match (verify, query_def.expected) {
            | (true, Some(expected)) => {
                let actual = described
                    .rows()
                    .expect("the describe step counts every query it verifies");
                if actual != expected {
                    anyhow::bail!(
                        "verification failed: benchmark '{}' query '{}' on {}/{} returned {} \
                         tuples, expected {}",
                        definition.name,
                        query_def.name,
                        ds_name,
                        algo_name,
                        actual,
                        expected
                    );
                }
                metadata.push(MetadataLine::new("verified", "yes"));
                true
            },
            | (true, None) => {
                eprintln!(
                    "bench {kind}: no expected cardinality for query '{}' in benchmark '{}'; not \
                     verified",
                    query_def.name, definition.name
                );
                false
            },
            | (false, _) => false,
        };
        let answer_rows = described.rows();
        if let Some(rows) = answer_rows {
            metadata.push(MetadataLine::new("answer rows", rows));
        }
        if metrics.contains(&Metric::EndToEnd) {
            metadata.push(MetadataLine::new("queries per build", queries_per_build));
        }
        for name in prepared.relations() {
            if let Some(header) = catalog.header(name) {
                metadata.push(MetadataLine::new(
                    "relation",
                    format!("{:?} (arity {})", header.name(), header.arity()),
                ));
            }
        }
        write_metadata_block(&mut io::stderr(), "bench run metadata", &metadata)?;

        let group_name = criterion_group_name(prefix, &definition.name, &query_def.name, execution);
        let measured = described.measure(&MeasureRequest {
            metrics,
            single_run: false,
            queries_per_build,
            criterion,
            group: &group_name,
            function_prefix: None,
        });

        let mut axes = BTreeMap::from([
            ("benchmark".to_string(), serde_json::json!(definition.name)),
            ("query".to_string(), serde_json::json!(query_def.name)),
            ("data_structure".to_string(), serde_json::json!(ds_name)),
            ("algorithm".to_string(), serde_json::json!(algo_name)),
            (
                "optimiser".to_string(),
                serde_json::json!(planner.optimiser.axis_value()),
            ),
            (
                "tuples".to_string(),
                serde_json::json!(tuples_read(catalog, prepared)),
            ),
            (
                "index_rows".to_string(),
                serde_json::json!(prepared.index_rows().iter().sum::<usize>()),
            ),
            (
                "plan".to_string(),
                serde_json::json!(prepared.variable_order().join(", ")),
            ),
        ]);
        if let Some(rows) = answer_rows {
            axes.insert("answer_rows".to_string(), serde_json::json!(rows));
        }
        // Only meaningful when the end-to-end metric ran; omitting it
        // otherwise keeps historical invocations' reports unchanged.
        if metrics.contains(&Metric::EndToEnd) {
            axes.insert(
                "queries_per_build".to_string(),
                serde_json::json!(queries_per_build),
            );
        }
        axes.extend(optimization_axes.clone());
        if verified {
            axes.insert("verified".to_string(), serde_json::json!(true));
        }
        sink.push(vec![BenchReport::new(
            kind,
            &metadata,
            axes,
            measured.criterion_groups,
            measured.values,
        )])?;
    }

    Ok(())
}
```

5. Change `dispatch_run_bench` to take and pass on the sink:

```rust
/// Runs one `bench run` cell by monomorphising [`run_benchmark`] over the
/// [`ExecutionFamily`] the cell names, pushing each query's report to
/// `sink`. There is no separate algorithm parameter to ignore: the
/// `Execution` fixes both halves of the pair.
pub(crate) fn dispatch_run_bench(
    cell: Execution, workload: &PreparedWorkload, settings: RunSettings<'_>, sink: &mut ReportSink,
) -> anyhow::Result<()> {
```
   In each of its three arms, add `sink,` after `settings,` in the `run_benchmark(…)` call.

- [ ] **Step 4: Rewire `main.rs`**

In `run_bench_join`, replace everything from `let settings = RunSettings {` to the end of the
function with:

```rust
    let criterion = CriterionSettings::from_args(bench_args);
    let settings = RunSettings {
        kind: BenchKind::Join,
        prefix: bench_args.name.as_deref().unwrap_or(DEFAULT_JOIN_GROUP),
        planner: query_args.planner,
        metrics,
        queries_per_build,
        verify: false,
        criterion: &criterion,
    };
    let mut sink = ReportSink::open(bench_args.report_json.as_deref(), BenchKind::Join)?;
    dispatch_run_bench(cell, &workload, settings, &mut sink)?;
    sink.finish()?;
    Ok(())
}
```

In `run_bench_run_command`:
- add `let criterion = CriterionSettings::from_args(bench_args);` after `let prepare_with = …;`;
- in the `RunSettings { … }` literal, replace `bench_args,` with `criterion: &criterion,`;
- change the comment above `ReportSink::open` to "Opened before the loop so every finished
  query is on disk before the next one starts; a crash mid-sweep keeps the completed
  queries.";
- replace the inner `for &cell in &cells { … }` loop with:

```rust
        for &cell in &cells {
            dispatch_run_bench(cell, &workload, settings, &mut sink).with_context(|| {
                format!(
                    "bench run failed on benchmark '{}' cell {:?}; partial report retained at {}",
                    benchmark.name,
                    cell,
                    sink.path().display()
                )
            })?;
        }
```

Delete `mod measurement;`.

- [ ] **Step 5: Delete what nothing uses any more**

1. Delete `kermit/src/measurement.rs`: `git rm kermit/src/measurement.rs`.
2. In `kermit/src/bench/mod.rs`:
   - delete `build_time_criterion`, `build_space_criterion`, `heap_bytes_of`,
     `add_space_bench`, and the tests `heap_bytes_of_sums_every_relation`,
     `heap_bytes_of_no_relation_is_zero` and `heap_bytes_of_saturates_instead_of_wrapping`,
     together with their `Bytes` stand-in struct and its `HeapSize` impl;
   - delete the module's top-level `use { … };` block: once those items are gone, no non-test
     item uses `CriterionGroupRef`, `ReportMetric`, `measurement`, `BenchArgs`, `HeapSize` or
     `Duration`. Add `use std::time::Duration;` after `use super::*;` in `mod tests`, because
     `criterion_directory_name_matches_criterion` still needs it.
3. In `kermit/src/bench/measure.rs`, delete the `#![allow(dead_code)]` line and its comment.
4. In `kermit/src/bench_report.rs`:
   - change the `ReportMetric::Space` doc to
     `/// Exact heap bytes, reported in [`BenchReport::values`] (from v6).`;
   - change the `ReportSink` doc's "`bench run` produces reports one cell at a time" to
     "`bench run` produces reports one query at a time", and "holding every cell that has
     finished. An interrupted sweep therefore keeps its completed cells; only the cell in
     flight is lost." to "holding every query that has finished. An interrupted sweep
     therefore keeps its completed queries; only the query in flight is lost.";
   - change "`byte_unit`" from `pub(crate)` to private (`fn byte_unit`), since
     `measurement.rs` is gone.
5. In `kermit/src/execution.rs`, replace the doc comment of `const JOIN_MUTATES: bool;` with:

```rust
    /// Whether running a join can change the indexes it reads: a lazy
    /// `HashTrie` builds each child a join first reaches (issue #92). When
    /// true, `bench run` never times a join over indexes a join has already
    /// run on: `iteration` runs on a fresh build per sample, so each timed
    /// join pays its own expansion, and the describe step reads its bytes
    /// before its `--verify` count, so `space` is the as-built footprint.
    /// Required, like `build_index`: every family states what its joins do.
```

- [ ] **Step 6: Run the tests to verify they pass**

Run: `CARGO_BUILD_JOBS=2 nix develop --command cargo test -p kermit`
Expected: all pass.
Run: `CARGO_BUILD_JOBS=2 nix develop --command cargo clippy --all-targets -- -D warnings`
Expected: clean. If the lint `dead_code` names an item, it is one this task should have
deleted: delete it.
Run: `CARGO_BUILD_JOBS=2 nix develop --command cargo rustdoc -p kermit --bin kermit -- --document-private-items -D warnings`
Expected: clean.
Run: `CARGO_BUILD_JOBS=2 nix develop --command bash -c 'cargo build -p kermit && cd python/kermit-lab && KERMIT_BIN=$PWD/../../target/debug/kermit uv run pytest -q'`
Expected: all pass.

- [ ] **Step 7: Commit**

```bash
nix develop --command cargo fmt --all
git add kermit/src/bench/run.rs kermit/src/main.rs kermit/src/bench/mod.rs kermit/src/bench/measure.rs kermit/src/bench_report.rs kermit/src/execution.rs kermit/tests/cli_bench_join_axes.rs kermit/tests/cli_join_tests.rs kermit/tests/cli_hash_trie_layout_expansion.rs python/kermit-lab/tests/test_contract.py
git commit -F - <<'EOF'
feat(bench)!: bench run and bench join measure through the measure layer

Per query and cell, one describe build outside Criterion feeds the index
lines, the exact `space` and the verified count; `iteration` runs on it,
and it is dropped before `build` and `end_to_end` are timed. Reports gain
the `plan` and `answer_rows` axes and are pushed after every query.
`SpaceMeasurement` and the space helpers are deleted.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01CUsjYmVxrLR7qsDPJz5gPb
EOF
```

### Task 11: report schema v6

**Files:**
- Modify: `kermit/src/bench_report.rs`, `python/kermit-lab/kermit_lab/__init__.py`,
  `python/kermit-lab/kermit_lab/loader.py`
- Modify (tests): `kermit/tests/cli_join_tests.rs`, `python/kermit-lab/tests/test_contract.py`,
  `python/kermit-lab/tests/test_loader.py`

- [ ] **Step 1: Write the failing tests**

- In `kermit/src/bench_report.rs`'s `json_report_round_trips`, change
  `assert_eq!(json[0]["schema_version"], 5);` to `… 6);`.
- In `kermit/tests/cli_join_tests.rs`, change all three `["schema_version"], 5)` assertions to
  `6)`.
- In `python/kermit-lab/tests/test_contract.py`, change both `== {5}` schema assertions to
  `== {6}`.
- Append to `python/kermit-lab/tests/test_loader.py`:

```python
def test_v5_and_v6_reports_load_together(tmp_path: Path) -> None:
    paths = []
    for version in (5, 6):
        p = tmp_path / f"v{version}.json"
        p.write_text(json.dumps([{
            "schema_version": version, "kind": "run", "metadata": [], "axes": {},
            "criterion_groups": [], "values": [],
        }]))
        paths.append(p)
    assert [r.schema_version for r in load_reports(paths)] == [5, 6]
```

- [ ] **Step 2: Run them to verify they fail**

Run: `CARGO_BUILD_JOBS=2 nix develop --command cargo test -p kermit --bin kermit bench_report`
Expected: `json_report_round_trips` fails (5 ≠ 6).

- [ ] **Step 3: Implement**

- In `kermit/src/bench_report.rs`, set `REPORT_SCHEMA_VERSION` to `6`, and append to its doc
  comment: "6: the measurements made without Criterion move into `values`: `space` is an exact
  entry there and never a Criterion function, `bench probe` reports (`kind: probe`) add
  single-run `build` / `iteration` times, and the `plan` and `answer_rows` axes are new. No
  metric changes what it measures."
- In `python/kermit-lab/kermit_lab/__init__.py`, set `SCHEMA_VERSION = 6`.
- In `python/kermit-lab/kermit_lab/loader.py`, after `PER_QUERY_BUILD_SCHEMA`, add:

```python
VALUES_SCHEMA = 6
"""First schema version whose measurements made without Criterion sit in
the report's ``values`` list: ``space`` is an exact entry there, never a
Criterion function, and ``bench probe`` reports add single-run times. No
metric changes meaning, so it is not in :data:`MEANING_CHANGES`, and v5 and
v6 reports load together."""
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `CARGO_BUILD_JOBS=2 nix develop --command cargo test -p kermit`
Expected: all pass, including `python_schema_version_matches_rust`.
Run: `CARGO_BUILD_JOBS=2 nix develop --command bash -c 'cargo build -p kermit && cd python/kermit-lab && KERMIT_BIN=$PWD/../../target/debug/kermit uv run pytest -q'`
Expected: all pass.

- [ ] **Step 5: Commit**

```bash
nix develop --command cargo fmt --all
git add kermit/src/bench_report.rs kermit/tests/cli_join_tests.rs python/kermit-lab/kermit_lab/__init__.py python/kermit-lab/kermit_lab/loader.py python/kermit-lab/tests/test_contract.py python/kermit-lab/tests/test_loader.py
git commit -F - <<'EOF'
feat!: bench report schema v6

`space` and other measurements made without Criterion live in the
report's `values` list. No metric changes meaning: kermit-lab loads v5
and v6 reports together.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01CUsjYmVxrLR7qsDPJz5gPb
EOF
```

---

## Phase 3: `bench probe`

### Task 12: `RunSelection` (refactor, no behaviour change)

**Files:**
- Modify: `kermit/src/main.rs`

- [ ] **Step 1: Implement**

Add this struct before `enum BenchSubcommand`:

```rust
/// Which benchmarks, queries and cells a `bench run` covers, and how each
/// query is planned.
#[derive(Args)]
struct RunSelection {
    /// Benchmark name (omit for --all)
    #[arg(value_name = "NAME")]
    name: Option<String>,

    /// Run all available benchmarks
    #[arg(long, conflicts_with = "name")]
    all: bool,

    /// Run only the named query (omit to run all queries)
    #[arg(short, long, value_name = "QUERY")]
    query: Option<String>,

    /// Data structure (`all` sweeps every available structure)
    #[arg(
        short,
        long,
        value_name = "INDEXSTRUCTURE",
        required = true,
        value_enum
    )]
    indexstructure: IndexStructureSelector,

    /// Join algorithm (`all` sweeps every available algorithm)
    #[arg(short, long, value_name = "ALGORITHM", required = true, value_enum)]
    algorithm: JoinAlgorithmSelector,

    #[command(flatten)]
    planner: PlannerArgs,

    /// Regenerate generator-driven benchmarks if their cached
    /// `meta.json` spec_hash differs from the current YAML's params.
    /// Without this flag, a drifted benchmark is an error instead of
    /// silently re-running an expensive pipeline. No-op for static
    /// benchmarks.
    #[arg(long)]
    force: bool,

    #[command(flatten)]
    layout: LayoutChoices,

    #[command(flatten)]
    config: ConfigChoices,

    #[command(flatten)]
    build: BuildChoices,
}
```

Replace the `Run { … }` variant with:

```rust
    /// Run a named benchmark from benchmarks/ YAML files
    Run {
        #[command(flatten)]
        selection: RunSelection,

        /// Metrics to benchmark (`end-to-end` is opt-in, not in the default
        /// set)
        #[arg(
            short,
            long,
            value_enum,
            num_args = 1..,
            default_values_t = vec![Metric::Build, Metric::Iteration, Metric::Space]
        )]
        metrics: Vec<Metric>,

        /// Query executions per index build in the `end-to-end` metric's
        /// timed body (T = build + K × query). Ignored by other metrics.
        #[arg(long, value_name = "K", default_value = "1", value_parser = clap::value_parser!(u32).range(1..))]
        queries_per_build: u32,

        /// Run each query once before timing and check its result count
        /// against the benchmark's `expected` value; a mismatch aborts.
        /// Queries without `expected` are noted and skipped.
        #[arg(long)]
        verify: bool,
    },
```

Change `run_bench_run_command`'s signature and head to:

```rust
/// Handler for `bench run`: materialise the selected benchmarks and run
/// every valid (index-structure, algorithm) cell of the requested sweep,
/// writing each query's report as it finishes.
fn run_bench_run_command(
    bench_args: &BenchArgs, selection: RunSelection, metrics: Vec<Metric>, queries_per_build: u32,
    verify: bool,
) -> anyhow::Result<()> {
    let RunSelection {
        name,
        all,
        query,
        indexstructure,
        algorithm,
        planner,
        force,
        layout,
        config,
        build,
    } = selection;
```

Delete its `#[allow(clippy::too_many_arguments)]`. The body is unchanged.

Replace the `BenchSubcommand::Run { … } => run_bench_run_command(…)?` arm of `main` with:

```rust
            | BenchSubcommand::Run {
                selection,
                metrics,
                queries_per_build,
                verify,
            } => run_bench_run_command(&bench_args, selection, metrics, queries_per_build, verify)?,
```

- [ ] **Step 2: Run the existing CLI tests**

Run: `CARGO_BUILD_JOBS=2 nix develop --command cargo test -p kermit --test cli_bench_run_sweep --test cli_bench_run_verify --test cli_bench_run_ds_flag_reach --test cli_query_errors --test cli_bench_run_group_collision`
Expected: all pass. `bench run`'s command line is unchanged.

- [ ] **Step 3: Commit**

```bash
nix develop --command cargo fmt --all
git add kermit/src/main.rs
git commit -F - <<'EOF'
refactor(cli): bench run's selection flags as one RunSelection

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01CUsjYmVxrLR7qsDPJz5gPb
EOF
```

### Task 13: the `bench probe` subcommand

**Files:**
- Create: `kermit/tests/cli_bench_probe.rs`
- Modify: `kermit/src/main.rs`, `kermit/src/bench/run.rs`, `kermit/src/bench/mod.rs`,
  `kermit/src/bench/measure.rs`, `python/kermit-lab/tests/test_contract.py`

- [ ] **Step 1: Write the failing CLI tests**

Create `kermit/tests/cli_bench_probe.rs`:

```rust
//! `bench probe` screens a benchmark without Criterion. Per query and cell it
//! builds once and runs once, timing both, verifies every query that declares
//! `expected`, and reports the exact `space`, the answer count and the plan.

use {
    std::{
        fs,
        path::PathBuf,
        process::{Command, Output},
    },
    tempfile::TempDir,
};

fn kermit_bin() -> PathBuf { PathBuf::from(env!("CARGO_BIN_EXE_kermit")) }

/// A workspace whose one benchmark, `probe`, reads `edge.csv` (1 -> 2,
/// 2 -> 1, 2 -> 3) with two queries: `mutual` (2 answers, declared), then
/// `pinned` (1 answer) declared as `pinned_expected`. Returns the workspace
/// and an empty cache directory.
fn workspace(pinned_expected: u64) -> (TempDir, TempDir) {
    let workspace = TempDir::new().unwrap();
    let cache = TempDir::new().unwrap();
    let data = workspace.path().join("data");
    fs::create_dir_all(&data).unwrap();
    fs::write(data.join("edge.csv"), "src,dst\n1,2\n2,1\n2,3\n").unwrap();
    let benchmarks = workspace.path().join("benchmarks");
    fs::create_dir_all(&benchmarks).unwrap();
    fs::write(
        benchmarks.join("probe.yml"),
        format!(
            r#"name: probe
description: "two queries over one relation"
relations:
  - name: edge
    path: "data/edge.csv"
queries:
  - name: mutual
    description: "edges present in both directions"
    query: "Q(X, Y) :- edge(X, Y), edge(Y, X)."
    expected: 2
  - name: pinned
    description: "the sources of edges into 2"
    query: "Q(X) :- edge(X, c2)."
    expected: {pinned_expected}
"#
        ),
    )
    .unwrap();
    (workspace, cache)
}

/// `kermit bench <bench_args> probe probe <args>` in `workspace`, with
/// Criterion pointed at `workspace/criterion` so a test can check that
/// nothing is written there. Returns the output and the parsed report.
fn probe(
    workspace: &TempDir, cache: &TempDir, bench_args: &[&str], args: &[&str],
) -> (Output, Vec<serde_json::Value>) {
    let report = workspace.path().join("report.json");
    let output = Command::new(kermit_bin())
        .env("RUST_BACKTRACE", "0")
        .env("RUST_LIB_BACKTRACE", "0")
        .env("KERMIT_WORKSPACE", workspace.path())
        .env("XDG_CACHE_HOME", cache.path())
        .env("CRITERION_HOME", workspace.path().join("criterion"))
        .current_dir(workspace.path())
        .arg("bench")
        .args(bench_args)
        .arg("--report-json")
        .arg(&report)
        .args(["probe", "probe"])
        .args(args)
        .output()
        .unwrap();
    let reports = fs::read_to_string(&report)
        .ok()
        .map(|text| serde_json::from_str(&text).unwrap())
        .unwrap_or_default();
    (output, reports)
}

/// The `values` entry of `report` named `function`.
fn value<'r>(report: &'r serde_json::Value, function: &str) -> &'r serde_json::Value {
    report["values"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["function"] == function)
        .unwrap_or_else(|| panic!("no {function} value in {report}"))
}

#[test]
fn a_probe_verifies_each_query_and_times_one_build_and_one_run() {
    let (workspace, cache) = workspace(1);
    let (output, reports) = probe(&workspace, &cache, &[], &["-i", "all", "-a", "all"]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stderr}");
    assert_eq!(reports.len(), 6, "two queries on three cells");
    for report in &reports {
        assert_eq!(report["kind"], "probe");
        assert_eq!(report["schema_version"], 6);
        assert!(report["criterion_groups"].as_array().unwrap().is_empty());
        assert_eq!(report["axes"]["verified"], true);
        let space = value(report, "space");
        assert_eq!(space["metric"], "space");
        assert_eq!(space["estimator"], "exact");
        for phase in ["build", "iteration"] {
            let single = value(report, phase);
            assert_eq!(single["metric"], "time", "{phase}");
            assert_eq!(single["estimator"], "single", "{phase}");
            assert!(single["value"].is_u64(), "{phase}: {single}");
        }
    }
    for report in reports.iter().filter(|r| r["axes"]["query"] == "mutual") {
        assert_eq!(report["axes"]["answer_rows"], 2);
        assert_eq!(report["axes"]["plan"], "X, Y");
    }
    assert!(stderr.contains("--- bench probe metadata ---"), "{stderr}");
    assert!(stderr.contains("single run:"), "{stderr}");
    assert!(
        !workspace.path().join("criterion").exists(),
        "a probe writes no Criterion output"
    );
}

#[test]
fn a_wrong_expected_aborts_the_probe_and_keeps_the_finished_queries() {
    let (workspace, cache) = workspace(5);
    let (output, reports) = probe(&workspace, &cache, &[], &[
        "-i",
        "tree-trie",
        "-a",
        "leapfrog-triejoin",
    ]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(
            "verification failed: benchmark 'probe' query 'pinned' on TreeTrie/LeapfrogTriejoin \
             returned 1 tuples, expected 5"
        ),
        "{stderr}"
    );
    assert_eq!(reports.len(), 1, "{reports:?}");
    assert_eq!(reports[0]["axes"]["query"], "mutual");
}

#[test]
fn a_probe_ignores_benchs_criterion_settings() {
    let (workspace, cache) = workspace(1);
    let (output, reports) = probe(
        &workspace,
        &cache,
        &["--name", "ignored", "--sample-size", "50", "--warm-up-time", "9"],
        &["-q", "pinned", "-i", "hash-trie", "-a", "hash-triejoin"],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(reports.len(), 1);
}

#[test]
fn a_probe_writes_its_default_report_under_its_own_kind() {
    let (workspace, cache) = workspace(1);
    let output = Command::new(kermit_bin())
        .env("KERMIT_WORKSPACE", workspace.path())
        .env("XDG_CACHE_HOME", cache.path())
        .current_dir(workspace.path())
        .args(["bench", "probe", "probe", "-q", "pinned"])
        .args(["-i", "tree-trie", "-a", "leapfrog-triejoin"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let names: Vec<String> = fs::read_dir(workspace.path().join("bench-runs"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    assert_eq!(names.len(), 1, "{names:?}");
    assert!(
        names[0].starts_with("probe-") && names[0].ends_with(".json"),
        "{names:?}"
    );
}
```

Append to `python/kermit-lab/tests/test_contract.py`:

```python
def test_bench_probe_report_loads(tmp_path: Path) -> None:
    report = tmp_path / "probe.json"
    _run(tmp_path, report, "probe", "triangle", "-i", "tree-trie", "-a", "leapfrog-triejoin")
    df = kl.load(report, criterion_root=tmp_path / "target" / "criterion")
    assert set(df["kind"]) == {"probe"}
    assert set(df["schema_version"]) == {6}
    assert set(df["estimator"]) == {"exact", "single"}
    single = df[df["estimator"] == "single"]
    assert set(single["phase"]) == {"build", "iteration"}
    assert (single["mean_ns"] > 0).all()
    assert df["verified"].all()
    assert df["answer_rows"].notna().all()
    assert df["plan"].notna().all()
```

- [ ] **Step 2: Run them to verify they fail**

Run: `CARGO_BUILD_JOBS=2 nix develop --command cargo test -p kermit --test cli_bench_probe`
Expected: all four fail (`unrecognized subcommand 'probe'`).

- [ ] **Step 3: Implement**

In `kermit/src/bench/run.rs`:
- Change the module doc's first line to "The generic runner behind `bench run`, `bench join`
  and `bench probe`: one execution".
- Add to `RunSettings`, after `verify`:
  ```rust
      /// Count every query and record the describe step's build and
      /// execution times as single-run values (`bench probe`).
      pub single_run: bool,
  ```
- In `run_benchmark`:
  - add `single_run,` to the `RunSettings { … }` destructuring;
  - change `let count = verify && query_def.expected.is_some();` to
    `let count = (verify && query_def.expected.is_some()) || single_run;`;
  - after the `if let Some(rows) = answer_rows { … }` metadata push, add:
    ```rust
            if single_run {
                let build = described.build_time();
                let iteration = described
                    .exercise_time()
                    .expect("a probe counts every query");
                metadata.push(MetadataLine::new(
                    "single run",
                    format!("build {build:?}, iteration {iteration:?}"),
                ));
            }
    ```
  - replace `write_metadata_block(&mut io::stderr(), "bench run metadata", &metadata)?;` with:
    ```rust
            let title = if kind == BenchKind::Probe {
                "bench probe metadata"
            } else {
                "bench run metadata"
            };
            write_metadata_block(&mut io::stderr(), title, &metadata)?;
    ```
  - in the `MeasureRequest { … }` literal, replace `single_run: false,` with `single_run,`.
  - In the doc comment, after "…and the count `--verify` checks", add ", which `bench probe`
    also times".

In `kermit/src/bench/mod.rs`, change "`workload` is the value both `bench run` (from a
`benchmarks/*.yml` definition) and `bench join` …" to "`workload` is the value `bench run`
and `bench probe` (from a `benchmarks/*.yml` definition) and `bench join` …".

In `kermit/src/bench/measure.rs`'s module doc, change "`bench run` and `bench join` measure a
prepared query" to "`bench run`, `bench join` and `bench probe` measure a prepared query".

In `kermit/src/main.rs`:

1. Add this variant after `Run { … }`:

```rust
    /// Screen a named benchmark without Criterion: verify each query, time
    /// one build and one run, record the exact space
    ///
    /// Per query and cell, the query's indexes are built once and the query
    /// is run once, both timed (single runs, recorded as `values` with
    /// estimator `single`), beside the exact `space`, the answer count and
    /// the plan. Every query whose YAML declares `expected` is verified, and
    /// a mismatch aborts. Takes `bench run`'s selection flags. `bench`'s
    /// Criterion settings (`--name`, `--sample-size`, `--measurement-time`,
    /// `--warm-up-time`) do not apply; `--report-json` does. A single run is
    /// cold: compare it with other single runs, never with a Criterion
    /// estimate.
    Probe {
        #[command(flatten)]
        selection: RunSelection,
    },
```

2. Change `RunSelection`'s doc to "Which benchmarks, queries and cells a `bench run` or `bench
   probe` covers, and how each query is planned."
3. Change `run_bench_run_command` to take the kind and the probe flag:

```rust
/// Handler for `bench run` and `bench probe`: materialise the selected
/// benchmarks and run every valid (index-structure, algorithm) cell of the
/// requested sweep, writing each query's report as it finishes.
fn run_bench_run_command(
    bench_args: &BenchArgs, selection: RunSelection, kind: BenchKind, metrics: Vec<Metric>,
    queries_per_build: u32, verify: bool, single_run: bool,
) -> anyhow::Result<()> {
```
   In its body:
   - set the `RunSettings` literal's `kind: BenchKind::Run,` to `kind,` and add
     `single_run,` after `verify,`;
   - replace the `check_sweep_group_directories(…)?;` statement and its comment with:
     ```rust
         // Names only — nothing is fetched — so a sweep whose cells would
         // overwrite each other's Criterion results fails before any is timed.
         // A run with no time metric (`bench probe`, `-m space`) writes no
         // Criterion group, so nothing can collide.
         if metrics.iter().any(|metric| metric.is_timed()) {
             check_sweep_group_directories(prefix, &materialized, query.as_deref(), &cells)?;
         }
     ```
   - change `ReportSink::open(bench_args.report_json.as_deref(), BenchKind::Run)?` to
     `ReportSink::open(bench_args.report_json.as_deref(), kind)?`;
   - in both `with_context` messages, change `"bench run failed on benchmark` to
     `"bench {kind} failed on benchmark`. That is an inline capture, so the positional
     arguments don't change.
4. Add the probe handler after it:

```rust
/// Handler for `bench probe`: `bench run`'s runner with the probe's fixed
/// settings. Every query is counted and, where it declares `expected`,
/// verified; its describe step's build and execution times are recorded as
/// single-run values beside its exact `space`; nothing goes through
/// Criterion.
fn run_bench_probe_command(bench_args: &BenchArgs, selection: RunSelection) -> anyhow::Result<()> {
    run_bench_run_command(
        bench_args,
        selection,
        BenchKind::Probe,
        vec![Metric::Space],
        1,
        true,
        true,
    )
}
```

5. In `main`, change the `Run` arm's call to
   `run_bench_run_command(&bench_args, selection, BenchKind::Run, metrics, queries_per_build, verify, false)?`
   and add:
   ```rust
               | BenchSubcommand::Probe {
                   selection,
               } => run_bench_probe_command(&bench_args, selection)?,
   ```
6. Add `bench probe` to the module doc's subcommand description: change "and `bench`
   (Criterion-based benchmarks, including the YAML-defined benchmarks under `benchmarks/`)" to
   "and `bench` (Criterion-based benchmarks, including the YAML-defined benchmarks under
   `benchmarks/`, and `bench probe`, their Criterion-free screen)".

- [ ] **Step 4: Run the tests to verify they pass**

Run: `CARGO_BUILD_JOBS=2 nix develop --command cargo test -p kermit --test cli_bench_probe`
Expected: 4 passed.
Run: `CARGO_BUILD_JOBS=2 nix develop --command cargo test -p kermit`
Expected: all pass.
Run: `CARGO_BUILD_JOBS=2 nix develop --command bash -c 'cargo build -p kermit && cd python/kermit-lab && KERMIT_BIN=$PWD/../../target/debug/kermit uv run pytest tests/test_contract.py -q'`
Expected: all pass.

- [ ] **Step 5: Commit**

```bash
nix develop --command cargo fmt --all
git add kermit/tests/cli_bench_probe.rs kermit/src/main.rs kermit/src/bench/run.rs kermit/src/bench/mod.rs kermit/src/bench/measure.rs python/kermit-lab/tests/test_contract.py
git commit -F - <<'EOF'
feat(cli): bench probe screens a benchmark without Criterion

Per query and cell: one timed build and one timed run (single-run values),
the exact space, the answer count and the plan; every query with an
`expected` count is verified.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01CUsjYmVxrLR7qsDPJz5gPb
EOF
```

---

## Phase 4: documentation

### Task 14: the specs

**Files:**
- Modify: `docs/specs/bench-report-schema.md`, `docs/specs/space-benchmarks.md`,
  `docs/specs/benchmarking-architecture.md`

- [ ] **Step 1: `bench-report-schema.md`**

1. Set `**Current schema version:** \`6\``.
2. In "Top-level shape", change "Every report-producing `bench` subcommand (`join`, `ds`,
   `run`)" to "(`join`, `ds`, `run`, `probe`)", and "and `bench run` (one report per query)" to
   "`bench run` and `bench probe` (one report per query and cell)".
3. In the example JSON, set `"schema_version": 6`, set `"criterion_groups": []`, and add after
   it:
   ```json
       "values": [
         { "function": "TreeTrie/space", "metric": "space", "estimator": "exact", "value": 96 }
       ]
   ```
4. In the field catalogue:
   - `schema_version`: "Currently `6`.";
   - `kind`: `"join"` \| `"ds"` \| `"run"` \| `"probe"`;
   - add the row: `| \`values\` | Array of \`ValueEntry\` | Measurements made without Criterion
     (v6): the exact \`space\`, and a probe's single-run times. Always present from v6,
     possibly empty. |`
5. After the `CriterionGroupRef` table, add:

   ```markdown
   ### `ValueEntry` (v6)

   | Field       | Type                     | Description |
   |-------------|--------------------------|-------------|
   | `function`  | string                   | `space` (`bench run`, `bench join`, `bench probe`), `{ds}/space` (`bench ds`), or `build` / `iteration` (a probe's single runs). |
   | `metric`    | `"time"` \| `"space"`    | What the value measures. |
   | `estimator` | `"exact"` \| `"single"`  | `exact`: a deterministic quantity (heap bytes, as built). `single`: one run's wall time, timed with an `Instant` around one describe build or one execution. A single run is cold; compare it with other single runs, never with a Criterion estimate. |
   | `value`     | number (u64)             | ns for time, bytes for space. |
   ```

6. Replace the first paragraph of "Time and space function ids" with:

   ```markdown
   From v6, `bench run` and `bench join` write, per query, the Criterion time
   functions `iteration`, `build` and `end_to_end` (opt-in), in that order:
   `iteration` runs on the query's describe build, which is then dropped before
   `build` and `end_to_end` are timed. `space`, the heap bytes of the query's
   indexes together, read off the describe build before anything runs on it, is
   an exact `values` entry, never a Criterion function. `bench ds` writes
   `{ds}/iteration`, `{ds}/build` and `{ds}/end_to_end` (opt-in), and the exact
   value `{ds}/space`. `bench probe` writes no Criterion function: per query and
   cell, the exact `space` and the single-run values `build` and `iteration`.
   Each index gets an `index` metadata line, `{id} {relation} filter {filter}
   columns {columns:?}: {rows} rows, {bytes} bytes`, where the filter reads `-`
   for none and otherwise `col{c}={value}` for a constant and `col{a}=col{b}` for
   an equality, comma-separated (for example `#0 edge filter - columns [1, 0]: 4
   rows, 96 bytes`, illustrative).

   In v5 the time functions ran as `build`, `iteration`, `end_to_end`, and
   `space` was a Criterion function (`space`, `{ds}/space`) measured through
   `SpaceMeasurement`.
   ```

7. In "Conventional `axes` keys":
   - in the `verified` row, change "Populated by" to `run`, `probe` and append to its notes:
     "`bench probe` checks every query that declares `expected`.";
   - add two rows after `index_rows`:
     ```markdown
     | `plan`           | `join`, `run`, `probe`   | string           | v6: the plan's variable order by name, joined by `", "` (`Prepared::variable_order`; a `_` appears under its `K<n>` name). For one catalog and query it identifies the whole plan, so two reports of one (benchmark, query) with the same `plan` ran the same build and join. |
     | `answer_rows`    | `run`, `probe`           | number (u64)     | v6: the answer's row count from the describe step's count; present whenever it counted (`bench run --verify` on a query with `expected`, and every probe query). |
     ```
8. Prefix the "Null-valued estimates" paragraph with "Up to v5, ", changing "Criterion writes"
   to "Criterion wrote".
9. In "Versioning policy", append the bullet: "- v6 is a structural bump with no meaning
   change: `space` moved from `criterion_groups` into `values`. kermit-lab loads v5 and v6
   reports together."
10. Add the changelog row:

   ```markdown
   | 6       | 2026-10-25 | The measurements made without Criterion move into a new top-level `values` list (`ValueEntry`: `function`, `metric`, `estimator`, `value`). `space` is an exact `values` entry in every subcommand and never a Criterion function, so `SpaceMeasurement` is gone. `bench probe` (`kind: probe`) is new: per query and cell, one describe build and one execution timed with an `Instant` (single-run `build` and `iteration`), the exact `space`, verification against `expected`, and no Criterion function. New axes: `plan` and `answer_rows`. Per query and cell the runners measure in one order: describe (one untimed build, read for the `index` lines, `space` and verification), `iteration` on that build, then `build` and `end_to_end`; reports are written after every query. No metric changes what it measures, so kermit-lab loads v5 and v6 together. |
   ```

- [ ] **Step 2: `space-benchmarks.md`**

After the `**Date:**` line, insert:

```markdown
> **Superseded (2026-10-25, report schema v6).** `space` is no longer a
> Criterion measurement. The describe build of `bench run`, `bench join`,
> `bench probe` and `bench ds` reads each structure's `heap_size_bytes()` once,
> before anything runs on it, and the report records the sum as an exact
> `values` entry; `SpaceMeasurement` and its `iter_custom` loop are gone. See
> [`2026-10-25-bench-runner-measure-layer-design.md`](2026-10-25-bench-runner-measure-layer-design.md)
> and [`bench-report-schema.md`](bench-report-schema.md). The rest of this
> document describes reports up to v5.
```

- [ ] **Step 3: `benchmarking-architecture.md`**

1. Change `**Updated:**` to append "; 2026-10-25 (the measure layer, `bench probe`, report
   schema 6)".
2. In the Overview, change "with three measurement subcommands" to "with four measurement
   subcommands", and add after the `bench run` bullet:
   `- **\`bench probe\`** — screen a YAML-defined workload without Criterion: per query and
   cell, verify against \`expected\`, time one build and one execution, record the exact
   space.` Add `kermit bench probe          Criterion-free screen of a YAML-defined workload
   (CLI)` to the diagram, after the `bench run` line.
3. After "The query lifecycle" section, add:

   ```markdown
   ## The measure layer

   Every runner measures through `kermit/src/bench/measure.rs`. It measures a
   `Subject`: `QuerySubject` (a prepared query on a family; `bench run`,
   `bench join`, `bench probe`) or `RelationSubject` (one relation; `bench ds`).
   Per query and cell, in one fixed order:

   1. **Describe** (`Described::new`): one build outside Criterion, timed with an
      `Instant`; the heap bytes of each part are read before anything runs on it;
      optionally one execution, also timed. The `index` lines, the exact `space`,
      `--verify`'s count and a probe's single-run times all come from this build.
   2. **`iteration`** on the describe set, or, when exercising mutates
      (`JOIN_MUTATES`, lazy HashTrie), on a fresh build per sample in untimed setup.
   3. **Drop** the describe set. `Described::measure` consumes it, so no other
      index set is alive while a build is timed.
   4. **`build`**, then **`end_to_end`**, each building fresh per sample.

   `bench probe` stops after step 1.
   ```

4. In `## Common arguments`, change the `--name` row's description to "Criterion group name
   (`bench ds`) or prefix (`bench join`, `bench run`); ignored by `bench probe`", and add the
   sentence after the table: "`bench probe` runs no Criterion, so it ignores `--name`,
   `--sample-size`, `--measurement-time` and `--warm-up-time`."
5. In `## kermit bench ds`, replace the metrics table and the paragraph after it with:

   ```markdown
   | Metric | How measured |
   |--------|--------------|
   | `Iteration` | `F::scan` of the describe build via Criterion `iter`: walks every stored tuple through `RelationFamily::for_each_tuple` (`TrieIteratorWrapper::advance` / `HashTrie::for_each_tuple`), counting each through a `black_box` sink; no tuple is materialised |
   | `Build` | `RelationFamily::build_index(header.clone(), TupleView::all(&tuples))` (which applies the family's config or build mode) via Criterion `iter_batched` with a no-op setup and `BatchSize::PerIteration`, after the describe build is dropped: no setup copy of the rows, and each built relation is freed after the timer stops |
   | `EndToEnd` | `RelationFamily::build_index` (same input) then K `F::scan` traversals, one timed body, via `iter_batched` with `BatchSize::PerIteration` (fresh build per sample) |
   | `Space` | the describe build's `heap_size_bytes()`, recorded as an exact `values` entry; no Criterion function |

   The time metrics become Criterion functions in the order above:
   `{ds_name}/iteration`, `{ds_name}/build`, `{ds_name}/end_to_end`; `space` is the
   value `{ds_name}/space`. The building metrics build from the tuples as the
   reader produced them (`read_csv` / `read_parquet`: one flat `Tuples` in file
   order), read once and viewed whole, not from the structure's own iteration
   order: that order is sorted and deduplicated for the sorted tries (their best
   case) and was a degenerate build order for the hash trie (issue #66), whereas
   file order is the same input for every structure and Layout. K is stamped
   into the report's `queries_per_build` axis (only when the metric is
   requested), never into the function id.
   ```

6. In `## kermit bench run`, replace step 5 of the Flow with:

   ```markdown
   5. For each query in the workload (filtered by `--query` if set), per cell,
      through the measure layer: the describe build (its bytes give the `index`
      lines and the exact `space`; with `--verify`, its count, from
      `ExecutionFamily::count_rows`, which streams the join through a `black_box`
      sink and never materialises it, is compared with `expected`, a mismatch
      aborting and a query without `expected` noted as not verified), then the
      chosen time metrics in the fixed order:
      - `iteration`: `ExecutionFamily::execute`, counted through a `black_box`
        sink, over the describe build, or, when `JOIN_MUTATES`, over a fresh build
        per sample made in untimed setup. Planning is excluded;
      - `build`: `ExecutionFamily::build_indexes(catalog, prepared)` — every index
        the query needs plus its all-constant checks — via `iter_batched` with a
        no-op setup and `BatchSize::PerIteration`: no setup copy, at most one set
        of indexes alive, freed after the timer stops;
      - `end_to_end`: `build_indexes` plus K × `execute` in one timed body
        (`--queries-per-build`); it runs the same build and K executions as
        `build` + K × `iteration`, so a gap beyond noise between them is the
        build-to-query interaction it measures.
      Each query's report is pushed to the report file as soon as it finishes.
   ```

7. Replace the `**Function names:**` paragraph's first sentence with "**Function names:**
   `iteration`, `build` and `end_to_end` per query, in that order (the Criterion group,
   `{name}/{benchmark}/{query}/{ds}/{algo}`, already names the query), plus the exact value
   `space`. In v5 `space` was a Criterion function; before schema 5 the functions were
   `insertion`, `iteration`, `end_to_end`, `copies`, `space/{relation_name}` and
   `space/Index_<π>_<base>`."
8. After `## kermit bench run`, add:

   ```markdown
   ## `kermit bench probe`

   Screens a named YAML workload without Criterion. It takes `bench run`'s
   selection flags (`name` / `--all`, `--query`, `-i`, `-a`, `--optimiser`,
   `--force` and the `--ds-*` flags) and goes through the same runner with its
   settings fixed: per query and cell, the describe build and one execution,
   both timed with an `Instant` and recorded as single-run `build` and
   `iteration` values; the exact `space`; `answer_rows` and `plan`; and
   verification of every query that declares `expected` (a mismatch aborts).
   The report's `kind` is `probe` and its default path is
   `bench-runs/probe-{unix-millis}.json`.
   ```

- [ ] **Step 4: Commit**

```bash
git add docs/specs/bench-report-schema.md docs/specs/space-benchmarks.md docs/specs/benchmarking-architecture.md
git commit -F - <<'EOF'
docs(specs): report schema v6, the measure layer and bench probe

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01CUsjYmVxrLR7qsDPJz5gPb
EOF
```

### Task 15: the guides

**Files:**
- Modify: `CLAUDE.md`, `BENCHMARKING.md`, `USAGE.md`, `ARCHITECTURE.md`,
  `python/kermit-lab/README.md`, `python/kermit-lab/CHANGELOG.md`

- [ ] **Step 1: `CLAUDE.md`**

1. After the build-command line `cargo run -- bench run watdiv-example -i tree-trie -a
   leapfrog-triejoin --force  # Force-regenerate a generator-driven benchmark`, add:
   ```
   cargo run -- bench probe triangle -i all -a all                     # Screen a benchmark without Criterion: verify, one timed build and run, exact space
   ```
2. Replace the whole `- **Space benchmarks**: …` bullet with:
   ```markdown
   - **Space is exact (report v6)**: every runner's describe step (`Described::new` in `kermit/src/bench/measure.rs`) builds the measured subject once outside Criterion and reads `heap_size_bytes()` of each part before anything runs on it. `bench run`, `bench join` and `bench probe` record a query's `space` (the sum over its indexes), and `bench ds` its relation's, as an exact entry in the report's `values` list, never as a Criterion function. A zero-byte index set records an exact `0`. Before v6, `space` went through a custom Criterion `Measurement` (`SpaceMeasurement`, deleted with `kermit/src/measurement.rs`), whose `iter_custom` calibration trap no longer exists.
   ```
3. In the `- **No Criterion auto-plots**` bullet, delete ", and the zero-variance panic that
   `SpaceMeasurement` used to trigger no longer applies".
4. In the `- **JSON bench reports**` bullet:
   - "(one per query for `bench run`, exactly one for `bench join` / `bench ds`)" →
     "(one per query and cell for `bench run` and `bench probe`, one per query for `bench
     join`, one per structure for `bench ds`)";
   - "`index_rows` on a join report, and `allocator` on every report)" → "`index_rows` on a
     join report, `plan` on run, join and probe reports, `answer_rows` whenever the describe
     step counted, and `allocator` on every report)";
   - after "…resolving to `target/criterion/{group}/{directory_name}/`" insert ", and `values`,
     the measurements made without Criterion (`{function, metric, estimator, value}`;
     `estimator` is `exact` for `space` and `single` for a probe's one-run `build` /
     `iteration`; ns for time, bytes for space)";
   - "(currently `5`)" → "(currently `6`)", and "(v3, v4 and v5 are such bumps)" → "(v3, v4
     and v5 are such bumps; v6 is a structural bump: `space` moved from `criterion_groups`
     into `values`, and no metric changed meaning)";
   - "`verified: true` appears in `axes` only when `bench run --verify` checked the query." →
     "`verified: true` appears in `axes` only when `bench run --verify` or `bench probe`
     checked the query against its `expected`.";
   - "rewriting it atomically after every cell, so an interrupted sweep keeps the reports of
     every cell that finished; the failing cell's error names the retained file" →
     "rewriting it atomically after every query, so an interrupted sweep keeps the report of
     every query that finished; the failing cell's error names the retained file";
   - append at the end of the bullet: "v5 and v6 reports load together; kermit-lab's summary
     frame has an `estimator` column (`criterion`, `exact`, `single`), and its plots and
     `speedup_table` take Criterion time rows unless asked for `estimator="single"`."
5. Append to the `- **bench \`--name\` semantics**` bullet: "`bench probe` writes no Criterion
   group, so it ignores `--name`, as it ignores `--sample-size`, `--measurement-time` and
   `--warm-up-time`."
6. In the `- **\`bench run\` sweeps are cells, not pairs**` bullet:
   - change "serves both trait families and both `bench run` and `bench join`" to "serves
     both trait families and `bench run`, `bench join` and `bench probe`";
   - after the sentence ending "…(issue #56).", insert: "Every runner measures through
     `kermit/src/bench/measure.rs`: `Described::new` builds a `Subject` once outside Criterion
     (`QuerySubject`, a prepared query on a family; `RelationSubject`, one relation, for `bench
     ds`), and the `index` lines, exact `space`, the verified count and a probe's single-run
     times all come from that one build. `Described::measure` then consumes it: `iteration`
     runs on it (on a fresh build per sample under `JOIN_MUTATES`), it is dropped, and `build`
     and `end_to_end` are timed, so no other index set is alive while a build is timed."

- [ ] **Step 2: `BENCHMARKING.md`**

1. In "Verify before you trust":
   - change "The check is opt-in because it costs one extra query execution per cell, which is
     unwelcome in a long sweep once the answers are known to be right." to "The check is
     opt-in because it costs one query execution per query and cell (on the build the
     runner makes anyway), which is unwelcome in a long sweep once the answers are known to be
     right.";
   - then add the paragraph and example:

   ````markdown
   `bench probe` is the cheaper screen. It takes `bench run`'s selection flags
   and never runs Criterion. Per query and cell it builds once and runs once,
   timing both, verifies every query that declares `expected` (a mismatch
   aborts), and records the exact `space`, the answer count (`answer_rows`)
   and the plan (`plan`). Its two times are single runs, so they are cold: use
   them to screen queries and estimate a sweep, or to time queries too slow for
   Criterion, never beside a Criterion estimate.

   ```sh
   kermit bench probe <name> -i all -a all --optimiser cost-based
   ```
   ````

2. In "What you can measure", after the first paragraph, add:

   ```markdown
   Per query and cell, every runner measures in one order: one untimed
   *describe* build (read for the `index` lines, `space`, verification and a
   probe's single runs), `iteration` on that same build, then, with it
   dropped, `build` and `end_to_end`. So no other index set is alive while a
   build is timed. Lazy HashTrie cells follow the same order, but their
   `iteration` builds a fresh set per sample (#92). Every report row carries an
   `estimator`: `criterion` for a Criterion estimate, `exact` for `space`, and
   `single` for a probe's one-run times.
   ```

3. After the v5 paragraph ("Reports with `schema_version` 5 (2026-10-22) …"), add:

   ```markdown
   Reports with `schema_version` 6 (2026-10-25) measure what v5 measured:
   `space` moved from a Criterion function into the report's `values` list as
   an exact value, the axes gain `plan` and `answer_rows`, and `bench probe`
   reports (`kind: probe`) are new. `kermit-lab` loads v5 and v6 together.
   Within one binary, `iteration` now runs before `build` in every cell; the
   measure-layer spec's order check
   ([`docs/specs/2026-10-25-bench-runner-measure-layer-design.md`](docs/specs/2026-10-25-bench-runner-measure-layer-design.md),
   Acceptance 7) bounds what that changes.
   ```

4. Replace the body of "### Space — deterministic by construction" with:

   ```markdown
   Space measures `HeapSize::heap_size_bytes()` — heap-allocated bytes only,
   not `size_of::<Self>` — of what one describe build produced, read before
   anything runs on it, so a lazy HashTrie's `space` is its as-built footprint.
   It is exact: since report v6 it is one `values` entry (`estimator: exact`),
   not a Criterion function, and kermit-lab draws it with a zero-width
   interval, as it drew v5's zero-variance Criterion `space`. `bench probe`
   records it too, so one probe pass over a sweep's cells gives every cell's
   space.
   ```

- [ ] **Step 3: `USAGE.md`**

1. In "Run a named benchmark (`bench run`)", change "To benchmark only space:" to "To record
   only space (exact, read off one build; no Criterion run):".
2. Before "### Sweep all index structures and algorithms", add:

   ````markdown
   ### Screen a benchmark (`bench probe`)

   `bench probe` takes `bench run`'s selection flags and screens a benchmark
   without Criterion. Per query and cell it builds the query's indexes once and
   runs the query once, timing both, and records the exact `space`, the answer
   count (`answer_rows`) and the plan (`plan`). Every query whose YAML declares
   `expected` is verified, and a mismatch aborts. The report's `kind` is `probe`,
   and its two times are `values` with estimator `single`:

   ```sh
   kermit bench probe triangle -i all -a all
   kermit bench --report-json /tmp/probe.json probe watdiv-stress-c1 -i hash-trie -a hash-triejoin --optimiser cost-based
   ```

   `bench`'s Criterion settings (`--name`, `--sample-size`,
   `--measurement-time`, `--warm-up-time`) don't apply to a probe. A single
   run is cold, so compare it with other single runs, never with a Criterion
   estimate.
   ````

3. In "## JSON reports", append to the last paragraph: "From v6 each report also carries
   `values`, the measurements made without Criterion: the exact `space`, and a probe's
   single-run times."

- [ ] **Step 4: `ARCHITECTURE.md`**

1. After the bullet `- \`bench run [<NAME> | --all]\` — …` (the CLI subcommand list), add:
   `  - \`bench probe [<NAME> | --all]\` — screen YAML-defined benchmarks without Criterion:
   per query and cell, verify against \`expected\`, time one build and one execution, record
   the exact \`space\`, \`answer_rows\` and \`plan\`.`
2. At the end of the paragraph that starts "In the binary, `RelationFamily::build_index(header,
   view)` is each family's one config- and mode-aware construction site.", add: "Every bench
   runner measures through `kermit/src/bench/measure.rs`: one describe build per query and
   cell outside Criterion (the `index` lines, exact `space`, verification and a probe's
   single runs come from it), then `iteration` on it, then, with it dropped, `build` and
   `end_to_end`."

- [ ] **Step 5: kermit-lab `README.md` and `CHANGELOG.md`**

In `python/kermit-lab/README.md`:

1. In "Public API", after the `kl.ablation(...)` line, add:
   ```python
   kl.plot(df, kind="bar", x="data_structure", y="time",
           phase="build", estimator="single")      # bench probe's single runs
   ```
2. In "## Schema", change "up to v5" to "up to v6", and append the paragraph:

   ```markdown
   A v6 report carries a `values` list, the measurements made without
   Criterion: the exact `space` (`estimator: exact`) and `bench probe`'s
   single-run `build` / `iteration` times (`estimator: single`). `kl.load`
   makes one row per entry, beside the Criterion rows, and every row has an
   `estimator` column (`criterion`, `exact`, `single`). An exact row is its own
   zero-width interval; a single row has NaN bounds. Plots and `speedup_table`
   take Criterion time rows unless asked for `estimator="single"`; space rows
   are taken whatever their estimator. v6 adds the `plan` and `answer_rows`
   columns. No metric changed meaning at v6, so v5 and v6 reports load
   together.
   ```

In `python/kermit-lab/CHANGELOG.md`, add above `## [0.2.0]`:

```markdown
## [Unreleased]

### Added

- Report schema v6: a report's `values` list (the exact `space`, `bench probe`'s single runs)
  becomes summary rows, with an `estimator` column (`criterion`, `exact`, `single`)
- `plan` and `answer_rows` axis columns
- `kl.plot(..., estimator=)` and `kermit-lab plot --estimator`

### Changed

- Plots and `speedup_table` take Criterion time rows unless asked for single runs
```

- [ ] **Step 6: Commit**

```bash
git add CLAUDE.md BENCHMARKING.md USAGE.md ARCHITECTURE.md python/kermit-lab/README.md python/kermit-lab/CHANGELOG.md
git commit -F - <<'EOF'
docs: the measure layer, exact space, bench probe and report v6

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01CUsjYmVxrLR7qsDPJz5gPb
EOF
```

---

## Final checks

### Task 16: the gate

- [ ] **Step 1: Run every gate command**

```bash
nix develop --command cargo fmt --all -- --check
CARGO_BUILD_JOBS=2 nix develop --command cargo clippy --all-targets -- -D warnings
CARGO_BUILD_JOBS=2 nix develop --command cargo test
RUSTDOCFLAGS=-Dwarnings CARGO_BUILD_JOBS=2 nix develop --command cargo doc --workspace --no-deps
CARGO_BUILD_JOBS=2 nix develop --command cargo rustdoc -p kermit --bin kermit -- --document-private-items -D warnings
CARGO_BUILD_JOBS=2 nix develop --command bash -c 'cargo build -p kermit && cd python/kermit-lab && KERMIT_BIN=$PWD/../../target/debug/kermit uv run pytest -q'
```
Expected: each passes. Report any failure with its output; don't paper over it.

- [ ] **Step 2: Check the acceptance criteria against the spec**

Walk through Acceptance 1–6 and 8 of the spec, naming the test or file that shows each.
Acceptance 7 is Task 17.

### Task 17: the order check (spec Acceptance 7)

This is a measurement. It runs only on a quiet host, after the sweep's `CHAIN_EXIT`, with
nothing else timing.

- [ ] **Step 1: Prepare the run directory and the binary**

```bash
RUN=/tb/Source/Academia/kermit-bench-runs/order-check-$(date +%F)
mkdir -p "$RUN/bin" "$RUN/reports" "$RUN/logs" "$RUN/criterion"
CARGO_BUILD_JOBS=2 nix develop --command cargo build --release -p kermit
cp target/release/kermit "$RUN/bin/kermit"
git rev-parse HEAD > "$RUN/COMMIT"
sha256sum "$RUN/bin/kermit" > "$RUN/bin/kermit.sha256"
```

- [ ] **Step 2: Pick the JOB template**

The template is the one with the slowest median build that every cell completes in the sweep's
cost-based Criterion set. Run:

```bash
python3 -I - <<'EOF'
import json, statistics
from pathlib import Path
sweep = Path("/tb/Source/Academia/kermit-bench-runs/full-sweep-2026-10-24")
sets = json.loads((sweep / "queries.json").read_text())["job"]["benches"]
recs = [json.loads(l) for l in (sweep / "prescreen/probe.jsonl").read_text().splitlines()]
best = None
for bench, entry in sets.items():
    cells = entry["cells"]
    qs = entry["by_opt"]["cost-based"]["criterion"]
    if len(cells) < 3 or not qs:
        continue
    builds = [r["build_s"] for r in recs
              if r["label"] == "pre" and r["arm"] == "default" and r["optimiser"] == "cost-based"
              and r["bench"] == f"{bench}-pre" and r["query"] in qs and r["build_s"]]
    med = statistics.median(builds)
    if best is None or med > best[1]:
        best = (bench, med, len(qs))
print(f"JOB={best[0]}-cb  (median build {best[1]:.2f} s over {best[2]} queries)")
EOF
```

Record the printed `JOB=…` value.

- [ ] **Step 3: Estimate the run time and confirm it with the owner**

Per query, cell and replicate, the cost is about `alone ≈ max(1, B) + max(1.2, 20 B) + 1.1` and
`after ≈ alone + max(1, t) + max(1.2, 20 t) + 1.1` seconds, with B and t from
`prescreen/probe.jsonl`, summed over 5 replicates and 3 cells. Before Step 4, tell the owner
the total for `sw-wd-cb` and for the chosen JOB template. If it exceeds 3 h, ask whether to
subsample WatDiv (for example every 4th query, by `-q`).

- [ ] **Step 4: Run the check**

Write `$RUN/run.sh`:

```bash
#!/usr/bin/env bash
# The order check: `build` alone vs `build` after `iteration`, 5 replicates,
# alternating which arm runs first.
set -euo pipefail
RUN=$(cd "$(dirname "$0")" && pwd)
SWEEP=/tb/Source/Academia/kermit-bench-runs/full-sweep-2026-10-24
export XDG_CACHE_HOME=$SWEEP/cache KERMIT_WORKSPACE=$SWEEP/src CRITERION_HOME=$RUN/criterion
JOB=${JOB:?set JOB to the template from Step 2, e.g. sw-job-6-cb}
CRIT=(--sample-size 20 --measurement-time 1 --warm-up-time 1)
for rep in 1 2 3 4 5; do
  for bench in sw-wd-cb "$JOB"; do
    for cell in "tree-trie leapfrog-triejoin" "column-trie leapfrog-triejoin" "hash-trie hash-triejoin"; do
      read -r ds algo <<< "$cell"
      if (( rep % 2 )); then arms="alone after"; else arms="after alone"; fi
      for arm in $arms; do
        if [ "$arm" = alone ]; then metrics=(build); else metrics=(iteration build); fi
        id="$bench-$ds-r$rep-$arm"
        "$RUN/bin/kermit" bench "${CRIT[@]}" --name "oc$rep${arm:0:2}" \
          --report-json "$RUN/reports/$id.json" \
          run "$bench" -i "$ds" -a "$algo" --optimiser cost-based -m "${metrics[@]}" \
          > "$RUN/logs/$id.log" 2>&1
      done
    done
  done
done
echo "ORDER_CHECK_DONE $(date -Is)"
```

Then launch it detached, as long runs are on this host:

```bash
chmod +x "$RUN/run.sh"
cd "$RUN" && JOB=<value from Step 2> setsid nohup ./run.sh > logs/run.out 2>&1 < /dev/null & disown
```

- [ ] **Step 5: Analyse**

Write `$RUN/analyse.py`:

```python
"""The order check: per cell, the geometric mean over queries of
(build after iteration) / (build alone). Each query's ratio is the geometric
mean over the replicates of the ratio of Criterion build medians, and the CI
is a bootstrap over queries. Pass: the CI contains 1, or lies wholly within
[0.97, 1.03] (the codegen precision bound)."""
import sys
from pathlib import Path

import numpy as np

import kermit_lab as kl

run = Path(sys.argv[1])
df = kl.load(sorted(str(p) for p in (run / "reports").glob("*.json")),
             criterion_root=run / "criterion")
df = df[(df.metric == "time") & (df.phase == "build") & (df.estimator == "criterion")].copy()
stem = df.source_path.map(lambda p: Path(p).stem)
df["arm"] = stem.str.rsplit("-", n=1).str[1]
df["rep"] = stem.str.extract(r"-r(\d)-")[0]
piv = df.pivot_table(index=["benchmark", "data_structure", "query", "rep"],
                     columns="arm", values="median_ns")
piv["log_ratio"] = np.log(piv["after"] / piv["alone"])
per_query = piv.groupby(level=["benchmark", "data_structure", "query"])["log_ratio"].mean()
rng = np.random.default_rng(0)
passed_all = True
for (bench, ds), q in per_query.groupby(level=["benchmark", "data_structure"]):
    x = q.to_numpy()
    boots = np.exp(rng.choice(x, size=(10_000, len(x)), replace=True).mean(axis=1))
    lo, hi = np.percentile(boots, [2.5, 97.5])
    passed = (lo <= 1.0 <= hi) or (0.97 <= lo and hi <= 1.03)
    passed_all &= passed
    print(f"{bench:16} {ds:11} n={len(x):3d} geomean={np.exp(x.mean()):.4f} "
          f"CI=[{lo:.4f}, {hi:.4f}] {'PASS' if passed else 'FAIL'}")
sys.exit(0 if passed_all else 1)
```

Run: `nix develop --command bash -c "cd python/kermit-lab && uv run python -I $RUN/analyse.py $RUN"`
Expected: one line per (benchmark, cell), each `PASS`.

- [ ] **Step 6: Report**

Tell the owner the table. If any cell is `FAIL`, stop: per the spec, B's order is then
reconsidered with the owner. Record the outcome in the spec as an amendment only if the owner
asks.
