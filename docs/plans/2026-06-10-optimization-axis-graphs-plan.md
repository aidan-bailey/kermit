# Optimization-Axis-Aware kermit-lab Plotting — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace kermit-lab's six bespoke plot modules with a single configurable aesthetic-map engine over matplotlib, and fix the loader so the optimization axes (`ds_layout_*`, `ds_config_*`, `ds_build_mode`) reach the DataFrame and can be bound to any visual channel — enabling ablation figures.

**Architecture:** A thin "aesthetic map" binds visual channels (x/y/colour/style/facet/filter) to DataFrame columns. The loader surfaces optimization axes as wildcard-discovered columns (Section B of the spec). Geoms are dumb drawers — y-values and CIs come precomputed from Criterion's `estimates.json`. The six old shapes become thin presets. Clean break: no compat shims; call sites updated.

**Tech Stack:** Python 3.10+, pandas, matplotlib (Agg in tests), uv-managed. Tests via `uv run pytest`.

**Spec:** `docs/specs/2026-06-10-optimization-axis-graphs-design.md`

**Working directory:** all paths below are relative to `python/kermit-lab/`. Run every command from there.

**NixOS note:** if `import matplotlib` fails to find `libstdc++`, run commands inside `nix develop` (the flake exports the needed `LD_LIBRARY_PATH`).

---

## File Structure

| File | Responsibility |
| --- | --- |
| `kermit_lab/defaults.py` (new) | `AXIS_DEFAULTS` registry + `apply_axis_defaults(df)`. |
| `kermit_lab/frame.py` (modify) | Wildcard-wide ingest of `ds_*`/`algo_*`; `_samples_from_reports`; `discover_opt_columns`. |
| `kermit_lab/axis_mapping.py` (modify) | Column-aware `colour_for`/`marker_for`/`linestyle_for`; add `HashTrie`. |
| `kermit_lab/encoding.py` (new) | `AestheticMap`, `ResolvedSeries`, resolution (filter/select/facet/series). |
| `kermit_lab/geoms.py` (new) | `draw_bar`/`draw_line`/`draw_scatter`/`draw_violin`. |
| `kermit_lab/facet.py` (new) | `make_grid`, `shared_legend`, `finish`. |
| `kermit_lab/plot.py` (new) | `plot(...)` entry point; kind dispatch + save. |
| `kermit_lab/presets.py` (new) | `scaling`/`bar_time`/`bar_queries`/`bar_space`/`tradeoff`/`dist`/`ablation`. |
| `kermit_lab/__init__.py` (modify) | Rewire public API; drop `plots.*`. |
| `kermit_lab/drivers/render_all.py` (modify) | Preset-based; auto-discover ablation axes. |
| `kermit_lab/drivers/main.py` (modify) | General `plot` subcommand; presets route. |
| `kermit_lab/plots/` (delete) | Six modules absorbed into engine. |
| `tests/*` | New: `test_defaults`, `test_encoding`, `test_geoms`, `test_facet`, `test_plot`, `test_presets`. Rewrite: `test_frame`, `test_axis_mapping`, `test_render_all`. Delete: `test_scaling`, `test_bar_time`, `test_bar_queries`, `test_bar_space`, `test_dist`, `test_tradeoff`, `test_plots_figure_return`. |
| docs | README, `CLAUDE.md` gotcha, design-doc addendum. |

---

## Task 1: Defaults registry

**Files:**
- Create: `kermit_lab/defaults.py`
- Test: `tests/test_defaults.py`

- [ ] **Step 1: Write the failing test**

```python
# tests/test_defaults.py
"""Default back-fill for missing optimization axes."""
from __future__ import annotations

import pandas as pd

from kermit_lab.defaults import AXIS_DEFAULTS, apply_axis_defaults


def test_backfills_documented_default() -> None:
    df = pd.DataFrame({"ds_layout_hasher": [pd.NA, "fx"], "other": [1, 2]})
    out = apply_axis_defaults(df)
    assert out["ds_layout_hasher"].tolist() == ["sip", "fx"]
    assert AXIS_DEFAULTS["ds_layout_hasher"] == "sip"


def test_ignores_absent_columns() -> None:
    df = pd.DataFrame({"unrelated": [pd.NA]})
    out = apply_axis_defaults(df)
    assert out["unrelated"].isna().all()  # no crash, untouched
```

- [ ] **Step 2: Run test to verify it fails**

Run: `uv run pytest tests/test_defaults.py -v`
Expected: FAIL — `ModuleNotFoundError: No module named 'kermit_lab.defaults'`

- [ ] **Step 3: Write minimal implementation**

```python
# kermit_lab/defaults.py
"""Documented default values for optimization axes.

The loader leaves a missing optimization axis as NaN (sparsity is meaningful).
This registry records the *one* exception the schema mandates: pre-standard
reports that predate an axis ran a known value, so back-filling recovers
ground truth rather than inventing it. Each optimization documents its default
here in one place — the parser (`frame.py`) stays generic.

See `docs/specs/bench-report-schema.md` ("Standard axis prefixes").
"""
from __future__ import annotations

import pandas as pd

# axis column name -> value to substitute for NaN.
AXIS_DEFAULTS: dict[str, object] = {
    "ds_layout_hasher": "sip",  # HashTrie's historical hash function
}


def apply_axis_defaults(df: pd.DataFrame) -> pd.DataFrame:
    """Return ``df`` with documented axis defaults filled in for NaN cells.

    Only columns present in ``df`` are touched; absent columns are ignored.
    Mutates a copy, leaving the caller's frame unchanged.
    """
    out = df.copy()
    for col, default in AXIS_DEFAULTS.items():
        if col in out.columns:
            out[col] = out[col].fillna(default)
    return out
```

- [ ] **Step 4: Run test to verify it passes**

Run: `uv run pytest tests/test_defaults.py -v`
Expected: PASS (2 passed)

- [ ] **Step 5: Commit**

```bash
git add kermit_lab/defaults.py tests/test_defaults.py
git commit -m "feat(kermit-lab): add optimization-axis defaults registry"
```

---

## Task 2: Loader — wildcard-wide ingest + samples/opt helpers

**Files:**
- Modify: `kermit_lab/frame.py`
- Modify: `tests/conftest.py` (add `fixture_opt_tree`)
- Test: `tests/test_frame.py` (rewrite)

- [ ] **Step 1: Add the optimization-axis fixture to `conftest.py`**

Append this fixture to `tests/conftest.py` (reuses the module's existing `_write_function_dir` / `_write_report` helpers):

```python
@pytest.fixture
def fixture_opt_tree(tmp_path: Path) -> dict:
    """HashTrie reports carrying optimization axes for ablation tests.

    2 hashers (sip, fx) × 2 config flags (singleton_pruning true/false), one
    query, time + space each. Gives ≥2 distinct values for both
    ds_layout_hasher and ds_config_singleton_pruning so ablation paths fire.
    """
    criterion_root = tmp_path / "target" / "criterion"
    reports_dir = tmp_path / "reports"
    criterion_root.mkdir(parents=True)
    reports_dir.mkdir()

    paths: list[Path] = []
    for hasher in ("sip", "fx"):
        for pruning in (True, False):
            tag = f"{hasher}-{'on' if pruning else 'off'}"
            iter_fn = f"HashTrie/{tag}/iteration"
            space_fn = f"HashTrie/{tag}/space"
            iter_point = 100.0 if hasher == "sip" else 80.0
            space_point = 6400.0
            iter_samples = [(i + 1, iter_point * (i + 1)) for i in range(10)]
            space_samples = [(i + 1, space_point * (i + 1)) for i in range(10)]
            _write_function_dir(
                criterion_root, _FunctionSpec("run", iter_fn, "time", iter_point, iter_samples)
            )
            _write_function_dir(
                criterion_root, _FunctionSpec("run", space_fn, "space", space_point, space_samples)
            )
            paths.append(
                _write_report(
                    reports_dir,
                    f"run-HashTrie-{tag}",
                    kind="run",
                    axes={
                        "benchmark": "triangle",
                        "query": "triangle",
                        "data_structure": "HashTrie",
                        "algorithm": "LeapfrogTriejoin",
                        "tuples": 100,
                        "ds_layout_hasher": hasher,
                        "ds_config_singleton_pruning": pruning,
                    },
                    metadata=[{"label": "hasher", "value": hasher}],
                    groups=[
                        ("run", iter_fn, "time"),
                        ("run", space_fn, "space"),
                    ],
                )
            )
    return {
        "criterion_root": criterion_root,
        "reports_dir": reports_dir,
        "paths": sorted(paths),
    }
```

- [ ] **Step 2: Write the failing test (`tests/test_frame.py`)**

Replace the file's contents (keep any existing tests that still pass; the asserts below are the new requirements):

```python
# tests/test_frame.py
"""Loader: wildcard-wide optimization-axis ingest + samples/opt helpers."""
from __future__ import annotations

import pandas as pd

from kermit_lab.frame import (
    discover_opt_columns,
    load,
    load_samples,
)


def test_optimization_axes_become_columns(fixture_opt_tree) -> None:
    df = load(fixture_opt_tree["paths"], fixture_opt_tree["criterion_root"])
    assert "ds_layout_hasher" in df.columns
    assert "ds_config_singleton_pruning" in df.columns
    assert set(df["ds_layout_hasher"].dropna()) == {"sip", "fx"}
    # config flag stays boolean-valued
    assert set(df["ds_config_singleton_pruning"].dropna()) == {True, False}


def test_conventional_run_has_no_opt_columns(fixture_tree) -> None:
    df = load(fixture_tree["paths"], fixture_tree["criterion_root"])
    assert not any(c.startswith(("ds_", "algo_")) for c in df.columns)


def test_default_backfill_applied(fixture_tree) -> None:
    # fixture_tree has no ds_layout_hasher; default must NOT fabricate a column.
    df = load(fixture_tree["paths"], fixture_tree["criterion_root"])
    assert "ds_layout_hasher" not in df.columns


def test_discover_opt_columns(fixture_opt_tree) -> None:
    df = load(fixture_opt_tree["paths"], fixture_opt_tree["criterion_root"])
    cols = discover_opt_columns(df)
    assert "ds_layout_hasher" in cols
    assert "ds_config_singleton_pruning" in cols


def test_load_samples_unchanged(fixture_tree) -> None:
    s = load_samples(fixture_tree["paths"], fixture_tree["criterion_root"])
    assert {"criterion_group", "criterion_function", "per_iter_ns"} <= set(s.columns)
```

- [ ] **Step 3: Run test to verify it fails**

Run: `uv run pytest tests/test_frame.py -v`
Expected: FAIL — `ImportError: cannot import name 'discover_opt_columns'`

- [ ] **Step 4: Implement the loader changes**

In `kermit_lab/frame.py`, add the prefix regex near the top imports:

```python
import re

_OPT_AXIS_RE = re.compile(r"^(ds|algo)_(layout_|config_|build_mode)")
```

Replace the `_SUMMARY_COLUMNS` tuple with a split core/stats pair:

```python
_SUMMARY_COLUMNS_CORE: tuple[str, ...] = (
    "kind", "metric", "phase",
    *_AXIS_STR_KEYS,
    *_AXIS_INT_KEYS,
)
_SUMMARY_COLUMNS_STATS: tuple[str, ...] = (
    "mean_ns", "mean_lo", "mean_hi", "mean_se",
    "median_ns", "median_lo", "median_hi",
    "source_path", "criterion_group", "criterion_function",
)
```

Change `_summary_row` to accept and emit the discovered optimization axes (add the `opt_axes` parameter and the loop; everything else stays):

```python
def _summary_row(
    report: BenchReport,
    group_ref: CriterionGroupRef,
    data: FunctionData,
    opt_axes: Sequence[str],
) -> dict:
    phase = phase_of(group_ref.function)
    row: dict = {
        "kind": report.kind,
        "metric": group_ref.metric,
        "phase": phase if phase is not None else pd.NA,
    }
    for key in _AXIS_STR_KEYS:
        v = report.axis(key)
        row[key] = v if isinstance(v, str) else pd.NA
    for key in _AXIS_INT_KEYS:
        v = report.axis(key)
        row[key] = v if isinstance(v, int) and not isinstance(v, bool) else pd.NA
    for key in opt_axes:
        v = report.axis(key)
        # Keep native type (str / bool / int); only None becomes NA. Booleans
        # (config flags) intentionally pass through — they render as True/False
        # categories when bound to a channel.
        row[key] = v if v is not None else pd.NA
    row["mean_ns"] = data.mean.point
    row["mean_lo"] = data.mean.lower
    row["mean_hi"] = data.mean.upper
    row["mean_se"] = data.mean.standard_error
    row["median_ns"] = data.median.point
    row["median_lo"] = data.median.lower
    row["median_hi"] = data.median.upper
    row["source_path"] = str(report.source_path)
    row["criterion_group"] = group_ref.group
    row["criterion_function"] = group_ref.function
    return row
```

Add `Sequence` to the `typing` import line if not present. Add the discovery helper and rewrite `_summary_from_reports`:

```python
def _discover_opt_axes(reports: Sequence[BenchReport]) -> list[str]:
    """Sorted union of every axes key matching the ds_*/algo_* optimization prefixes."""
    keys: set[str] = set()
    for r in reports:
        for k in r.axes:
            if _OPT_AXIS_RE.match(k):
                keys.add(k)
    return sorted(keys)


def discover_opt_columns(df: pd.DataFrame) -> list[str]:
    """Optimization-axis columns present in a loaded summary frame, sorted."""
    return sorted(c for c in df.columns if _OPT_AXIS_RE.match(c))


def _summary_from_reports(
    reports: Sequence[BenchReport],
    criterion_root: Path | str,
    *,
    apply_defaults: bool = True,
) -> pd.DataFrame:
    """Build the summary DataFrame: fixed core columns, then the discovered
    optimization tail, then the stat/join columns."""
    from .defaults import apply_axis_defaults

    opt_axes = _discover_opt_axes(list(reports))
    rows = [
        _summary_row(report, gref, data, opt_axes)
        for report, gref, data in iter_function_data(reports, Path(criterion_root))
    ]
    columns = [*_SUMMARY_COLUMNS_CORE, *opt_axes, *_SUMMARY_COLUMNS_STATS]
    df = pd.DataFrame(rows, columns=columns)
    for key in _AXIS_INT_KEYS:
        df[key] = df[key].astype("Int64")
    if apply_defaults:
        df = apply_axis_defaults(df)
    return df
```

Thread `apply_defaults` through `load`:

```python
def load(
    paths: Iterable[Path | str] | Path | str,
    criterion_root: Path | str = "target/criterion",
    *,
    apply_defaults: bool = True,
) -> pd.DataFrame:
    reports = load_reports(_resolve_paths(paths))
    return _summary_from_reports(reports, criterion_root, apply_defaults=apply_defaults)
```

Add `_samples_from_reports` (factor the body of `load_samples` so `render_all` can reuse it without re-reading paths). Replace `load_samples` with:

```python
def _samples_from_reports(
    reports: Sequence[BenchReport],
    criterion_root: Path | str,
) -> pd.DataFrame:
    rows = [
        {
            "criterion_group": gref.group,
            "criterion_function": gref.function,
            "sample_idx": idx,
            "iters": it,
            "total_ns": tot,
            "per_iter_ns": tot / it,
        }
        for _, gref, data in iter_function_data(reports, Path(criterion_root))
        for idx, (it, tot) in enumerate(zip(data.iters, data.times))
    ]
    return pd.DataFrame(rows)


def load_samples(
    paths: Iterable[Path | str] | Path | str,
    criterion_root: Path | str = "target/criterion",
) -> pd.DataFrame:
    reports = load_reports(_resolve_paths(paths))
    return _samples_from_reports(reports, criterion_root)
```

- [ ] **Step 5: Run test to verify it passes**

Run: `uv run pytest tests/test_frame.py tests/test_defaults.py -v`
Expected: PASS (all)

- [ ] **Step 6: Commit**

```bash
git add kermit_lab/frame.py tests/conftest.py tests/test_frame.py
git commit -m "feat(kermit-lab): wildcard-wide optimization-axis ingest in loader"
```

---

## Task 3: axis_mapping — column-aware encoding + HashTrie

**Files:**
- Modify: `kermit_lab/axis_mapping.py`
- Test: `tests/test_axis_mapping.py` (rewrite)

- [ ] **Step 1: Write the failing test**

```python
# tests/test_axis_mapping.py
"""Column-aware colour/marker/linestyle lookups."""
from __future__ import annotations

from kermit_lab.axis_mapping import (
    DATA_STRUCTURE_COLOURS,
    colour_for,
    linestyle_for,
    marker_for,
)


def test_committed_ds_colours() -> None:
    assert colour_for("data_structure", "TreeTrie") == DATA_STRUCTURE_COLOURS["TreeTrie"]
    assert "HashTrie" in DATA_STRUCTURE_COLOURS  # newly committed


def test_unknown_value_is_stable() -> None:
    a = colour_for("ds_layout_hasher", "fx")
    b = colour_for("ds_layout_hasher", "fx")
    assert a == b  # stable within a process
    assert a != colour_for("ds_layout_hasher", "sip")  # distinct values differ


def test_marker_and_linestyle_fallback() -> None:
    assert marker_for("algorithm", "LeapfrogTriejoin") == "o"
    assert isinstance(marker_for("ds_config_singleton_pruning", True), str)
    assert isinstance(linestyle_for("algorithm", "Unknown"), str)
```

- [ ] **Step 2: Run test to verify it fails**

Run: `uv run pytest tests/test_axis_mapping.py -v`
Expected: FAIL — `ImportError: cannot import name 'colour_for'`

- [ ] **Step 3: Implement the generalization**

In `kermit_lab/axis_mapping.py`, add `HashTrie` to the committed colours:

```python
DATA_STRUCTURE_COLOURS: dict[str, str] = {
    "TreeTrie": WONG_PALETTE[5],    # blue
    "ColumnTrie": WONG_PALETTE[6],  # vermilion
    "HashTrie": WONG_PALETTE[3],    # bluish green
}
```

Append the column-aware layer (keep the existing `colour_for_ds`/`marker_for_algo`/`linestyle_for_algo` functions — they become thin wrappers):

```python
# Marker / linestyle cycles for fallback assignment on arbitrary `style` axes.
_MARKER_CYCLE: list[str] = ["o", "s", "^", "D", "v", "P", "X", "*"]
_LINESTYLE_CYCLE: list[str] = ["-", "--", "-.", ":"]

# column name -> committed value→encoding map.
_COMMITTED_COLOURS: dict[str, dict[str, str]] = {"data_structure": DATA_STRUCTURE_COLOURS}
_COMMITTED_MARKERS: dict[str, dict[str, str]] = {"algorithm": ALGORITHM_MARKERS}
_COMMITTED_LINESTYLES: dict[str, dict[str, str]] = {"algorithm": ALGORITHM_LINESTYLES}


def _stable(cycle: list[str], column: str, value: object) -> str:
    return cycle[hash((column, value)) % len(cycle)]


def colour_for(column: str, value: object) -> str:
    """Colour for ``(column, value)``: committed map if known, else stable fallback."""
    committed = _COMMITTED_COLOURS.get(column)
    if committed is not None and value in committed:
        return committed[value]
    if not _UNKNOWN_COLOURS:
        return WONG_PALETTE[0]
    return _stable(_UNKNOWN_COLOURS, column, value)


def marker_for(column: str, value: object) -> str:
    """Marker for ``(column, value)``: committed map if known, else stable filled marker."""
    committed = _COMMITTED_MARKERS.get(column)
    if committed is not None and value in committed:
        return committed[value]
    return _stable(_MARKER_CYCLE, column, value)


def linestyle_for(column: str, value: object) -> str:
    """Linestyle for ``(column, value)``: committed map if known, else stable fallback."""
    committed = _COMMITTED_LINESTYLES.get(column)
    if committed is not None and value in committed:
        return committed[value]
    return _stable(_LINESTYLE_CYCLE, column, value)
```

- [ ] **Step 4: Run test to verify it passes**

Run: `uv run pytest tests/test_axis_mapping.py -v`
Expected: PASS (3 passed)

- [ ] **Step 5: Commit**

```bash
git add kermit_lab/axis_mapping.py tests/test_axis_mapping.py
git commit -m "feat(kermit-lab): column-aware colour/marker/linestyle + HashTrie colour"
```

---

## Task 4: encoding — AestheticMap + resolution

**Files:**
- Create: `kermit_lab/encoding.py`
- Test: `tests/test_encoding.py`

- [ ] **Step 1: Write the failing test**

```python
# tests/test_encoding.py
"""Aesthetic-map resolution: filter, select, facet, scalar series."""
from __future__ import annotations

import pandas as pd

from kermit_lab.encoding import (
    AestheticMap,
    apply_filter,
    facet_values,
    metric_of,
    resolve_scalar,
    select,
)


def _df():
    return pd.DataFrame(
        {
            "metric": ["time", "time", "space"],
            "phase": ["iteration", "iteration", pd.NA],
            "data_structure": ["TreeTrie", "ColumnTrie", "TreeTrie"],
            "algorithm": ["LeapfrogTriejoin", "LeapfrogTriejoin", "LeapfrogTriejoin"],
            "tuples": pd.array([10, 10, 10], dtype="Int64"),
            "query": ["triangle", "triangle", "triangle"],
            "mean_ns": [100.0, 200.0, 64.0],
            "mean_lo": [99.0, 198.0, 64.0],
            "mean_hi": [101.0, 202.0, 64.0],
        }
    )


def test_metric_of() -> None:
    assert metric_of("time") == "time"
    assert metric_of("space") == "space"


def test_select_filters_metric_and_phase() -> None:
    amap = AestheticMap(kind="bar", x="data_structure", y="time")
    out = select(_df(), amap)
    assert set(out.metric) == {"time"}
    assert len(out) == 2


def test_apply_filter() -> None:
    amap = AestheticMap(kind="bar", x="data_structure", y="time",
                        filter={"data_structure": "TreeTrie"})
    out = apply_filter(_df(), amap)
    assert set(out.data_structure) == {"TreeTrie"}


def test_resolve_scalar_one_series_per_colour() -> None:
    amap = AestheticMap(kind="bar", x="data_structure", y="time", colour="data_structure")
    series = resolve_scalar(select(_df(), amap), amap)
    assert len(series) == 2
    labels = sorted(s.label for s in series)
    assert labels == ["ColumnTrie", "TreeTrie"]


def test_facet_values_none_when_unset() -> None:
    amap = AestheticMap(kind="bar", x="data_structure", y="time")
    assert facet_values(_df(), amap) == [None]
```

> **Note:** `InsufficientAxesError` currently lives in `kermit_lab/plots/__init__.py`, which Task 13 deletes. To avoid a mid-plan circular dependency, Step 3 below first moves it to a tiny new `kermit_lab/plots_errors.py`; `encoding.py` imports it from there.

- [ ] **Step 2: Run test to verify it fails**

Run: `uv run pytest tests/test_encoding.py -v`
Expected: FAIL — `ModuleNotFoundError: No module named 'kermit_lab.encoding'`

- [ ] **Step 3: Create the error module, then `encoding.py`**

```python
# kermit_lab/plots_errors.py
"""Shared plotting errors (kept separate so the engine doesn't import plots/)."""
from __future__ import annotations


class InsufficientAxesError(ValueError):
    """The input reports lack the axis values a plot shape requires."""
```

```python
# kermit_lab/encoding.py
"""Aesthetic map: bind visual channels to DataFrame columns, and resolve a
filtered/grouped frame into drawable series.

Y-values and CIs come straight from Criterion's precomputed estimates
(`mean_ns` / `mean_lo` / `mean_hi`); this layer performs no statistics.
"""
from __future__ import annotations

from dataclasses import dataclass, field
from typing import Mapping, Optional, Sequence

import pandas as pd

from .axis_mapping import WONG_PALETTE, colour_for, linestyle_for, marker_for
from .plots_errors import InsufficientAxesError


@dataclass(frozen=True)
class AestheticMap:
    """Binding of visual channels to columns. ``y`` selects the metric."""

    kind: str  # "bar" | "line" | "scatter" | "violin"
    x: Optional[str]
    y: str = "time"  # "time" | "space"
    colour: Optional[str] = None
    style: Optional[str] = None
    facet: Optional[str] = None
    filter: Mapping[str, object] = field(default_factory=dict)
    phase: str = "iteration"


@dataclass
class ResolvedSeries:
    """One drawable series (a colour×style group), positioned by x."""

    key: tuple
    label: str
    colour: str
    marker: str
    linestyle: str
    xs: list
    ys: list
    lo: list
    hi: list
    samples: Optional[list[list[float]]] = None


def metric_of(y: str) -> str:
    if y not in ("time", "space"):
        raise InsufficientAxesError(f"y must be 'time' or 'space', got {y!r}")
    return y


def apply_filter(df: pd.DataFrame, amap: AestheticMap) -> pd.DataFrame:
    out = df
    for col, val in amap.filter.items():
        if col not in out.columns:
            raise InsufficientAxesError(f"filter column {col!r} not in frame")
        if isinstance(val, (list, tuple, set)):
            out = out[out[col].isin(list(val))]
        else:
            out = out[out[col] == val]
    return out


def select(df: pd.DataFrame, amap: AestheticMap) -> pd.DataFrame:
    """Filter to the metric/phase rows this map needs; raise if empty."""
    metric = metric_of(amap.y)
    sub = apply_filter(df, amap)
    sub = sub[sub.metric == metric]
    if metric == "time":
        sub = sub[sub.phase == amap.phase]
    if sub.empty:
        raise InsufficientAxesError(
            f"no {metric} rows (phase={amap.phase}) after filter {dict(amap.filter)}"
        )
    return sub


def facet_values(df: pd.DataFrame, amap: AestheticMap) -> list:
    if amap.facet is None:
        return [None]
    if amap.facet not in df.columns:
        raise InsufficientAxesError(f"facet column {amap.facet!r} not in frame")
    return sorted(df[amap.facet].dropna().unique().tolist(), key=str)


def _group_cols(amap: AestheticMap) -> list[str]:
    return [c for c in (amap.colour, amap.style) if c]


def _make_series(
    keyt: tuple,
    group_cols: Sequence[str],
    xs: list,
    ys: list,
    lo: list,
    hi: list,
    amap: AestheticMap,
    samples: Optional[list[list[float]]] = None,
) -> ResolvedSeries:
    kv = dict(zip(group_cols, keyt))
    colour = colour_for(amap.colour, kv[amap.colour]) if amap.colour else WONG_PALETTE[5]
    marker = marker_for(amap.style, kv[amap.style]) if amap.style else "o"
    linestyle = linestyle_for(amap.style, kv[amap.style]) if amap.style else "-"
    label = " / ".join(str(kv[c]) for c in group_cols) if group_cols else amap.y
    return ResolvedSeries(keyt, label, colour, marker, linestyle, xs, ys, lo, hi, samples)


def _grouped_items(df: pd.DataFrame, group_cols: Sequence[str]):
    if group_cols:
        for key, g in df.groupby(list(group_cols), dropna=False):
            yield (key if isinstance(key, tuple) else (key,)), g
    else:
        yield (), df


def resolve_scalar(cell: pd.DataFrame, amap: AestheticMap) -> list[ResolvedSeries]:
    """Bar/line/scatter series from precomputed means + CIs, grouped by colour×style."""
    if amap.x is None or amap.x not in cell.columns:
        raise InsufficientAxesError(f"x column {amap.x!r} not in frame")
    group_cols = _group_cols(amap)
    series: list[ResolvedSeries] = []
    for keyt, g in _grouped_items(cell, group_cols):
        g = g.sort_values(amap.x)
        xs = g[amap.x].tolist()
        ys = g["mean_ns"].tolist()
        lo = (g["mean_ns"] - g["mean_lo"]).clip(lower=0).tolist()
        hi = (g["mean_hi"] - g["mean_ns"]).clip(lower=0).tolist()
        series.append(_make_series(keyt, group_cols, xs, ys, lo, hi, amap))
    return series
```

- [ ] **Step 4: Run test to verify it passes**

Run: `uv run pytest tests/test_encoding.py -v`
Expected: PASS (5 passed)

- [ ] **Step 5: Commit**

```bash
git add kermit_lab/plots_errors.py kermit_lab/encoding.py tests/test_encoding.py
git commit -m "feat(kermit-lab): aesthetic-map encoding + scalar series resolution"
```

---

## Task 5: geoms — bar + line drawers

**Files:**
- Create: `kermit_lab/geoms.py`
- Test: `tests/test_geoms.py`

- [ ] **Step 1: Write the failing test**

```python
# tests/test_geoms.py
"""Geom drawers add artists to an Axes."""
from __future__ import annotations

import matplotlib

matplotlib.use("Agg")

import matplotlib.pyplot as plt

from kermit_lab.encoding import ResolvedSeries
from kermit_lab.geoms import draw_bar, draw_line


def _series(label, colour="#000000"):
    return ResolvedSeries(
        key=(label,), label=label, colour=colour, marker="o", linestyle="-",
        xs=["A", "B"], ys=[1.0, 2.0], lo=[0.1, 0.1], hi=[0.1, 0.1],
    )


def test_draw_bar_adds_patches() -> None:
    fig, ax = plt.subplots()
    draw_bar(ax, [_series("TreeTrie"), _series("ColumnTrie")], xlabel="ds", ylabel="ns")
    assert len(ax.patches) > 0
    assert [t.get_text() for t in ax.get_xticklabels()] == ["A", "B"]
    plt.close(fig)


def test_draw_line_adds_lines() -> None:
    fig, ax = plt.subplots()
    draw_line(ax, [_series("TreeTrie")], xlabel="tuples", ylabel="ns", logx=True)
    assert len(ax.lines) > 0
    assert ax.get_xscale() == "log"
    plt.close(fig)
```

- [ ] **Step 2: Run test to verify it fails**

Run: `uv run pytest tests/test_geoms.py -v`
Expected: FAIL — `ModuleNotFoundError: No module named 'kermit_lab.geoms'`

- [ ] **Step 3: Implement bar + line**

```python
# kermit_lab/geoms.py
"""Dumb geom drawers. Each consumes a list[ResolvedSeries] and draws onto an
Axes — no statistics, no data access. Series carry precomputed y + CI.
"""
from __future__ import annotations

import numpy as np
from matplotlib.axes import Axes

from .encoding import ResolvedSeries


def _categories(series: list[ResolvedSeries]) -> list:
    cats: list = []
    for s in series:
        for x in s.xs:
            if x not in cats:
                cats.append(x)
    return sorted(cats, key=str)


def draw_bar(ax: Axes, series: list[ResolvedSeries], *, xlabel: str, ylabel: str) -> None:
    """Grouped bars: one cluster per x category, one bar per series."""
    cats = _categories(series)
    pos = {c: i for i, c in enumerate(cats)}
    base = np.arange(len(cats))
    n = max(len(series), 1)
    width = 0.8 / n
    for i, s in enumerate(series):
        xpos = [base[pos[x]] + (i - (n - 1) / 2) * width for x in s.xs]
        ax.bar(
            xpos, s.ys, width=width, yerr=[s.lo, s.hi], capsize=2,
            color=s.colour, edgecolor="black", linewidth=0.8, label=s.label,
        )
    ax.set_xticks(base)
    ax.set_xticklabels([str(c) for c in cats])
    ax.set_xlabel(xlabel)
    ax.set_ylabel(ylabel)


def draw_line(
    ax: Axes, series: list[ResolvedSeries], *, xlabel: str, ylabel: str,
    logx: bool = False, logy: bool = False,
) -> None:
    for s in series:
        ax.errorbar(
            s.xs, s.ys, yerr=[s.lo, s.hi], capsize=2,
            color=s.colour, marker=s.marker, linestyle=s.linestyle, label=s.label,
        )
    if logx:
        ax.set_xscale("log")
    if logy:
        ax.set_yscale("log")
    ax.set_xlabel(xlabel)
    ax.set_ylabel(ylabel)
```

- [ ] **Step 4: Run test to verify it passes**

Run: `uv run pytest tests/test_geoms.py -v`
Expected: PASS (2 passed)

- [ ] **Step 5: Commit**

```bash
git add kermit_lab/geoms.py tests/test_geoms.py
git commit -m "feat(kermit-lab): bar + line geom drawers"
```

---

## Task 6: facet — grid + shared legend

**Files:**
- Create: `kermit_lab/facet.py`
- Test: `tests/test_facet.py`

- [ ] **Step 1: Write the failing test**

```python
# tests/test_facet.py
"""Facet grid sizing and shared-legend dedup."""
from __future__ import annotations

import matplotlib

matplotlib.use("Agg")

import matplotlib.pyplot as plt

from kermit_lab.facet import make_grid, shared_legend


def test_make_grid_returns_n_visible_axes() -> None:
    fig, axes = make_grid(4)
    assert len(axes) == 4
    assert all(ax.get_visible() for ax in axes)
    plt.close(fig)


def test_shared_legend_dedupes_labels() -> None:
    fig, axes = make_grid(2)
    for ax in axes:
        ax.plot([0, 1], [0, 1], label="TreeTrie")
    shared_legend(fig, axes)
    legend = fig.legends[0]
    assert [t.get_text() for t in legend.get_texts()] == ["TreeTrie"]
    plt.close(fig)
```

- [ ] **Step 2: Run test to verify it fails**

Run: `uv run pytest tests/test_facet.py -v`
Expected: FAIL — `ModuleNotFoundError: No module named 'kermit_lab.facet'`

- [ ] **Step 3: Implement the grid + legend + finish**

```python
# kermit_lab/facet.py
"""Subplot-grid layout + shared, de-duplicated legend across facet cells."""
from __future__ import annotations

import math
from pathlib import Path
from typing import Optional

import matplotlib.pyplot as plt
from matplotlib.axes import Axes
from matplotlib.figure import Figure

_MAX_COLS = 3


def make_grid(n: int) -> tuple[Figure, list[Axes]]:
    """Return a figure + exactly ``n`` visible axes (extra cells hidden)."""
    n = max(n, 1)
    ncols = min(n, _MAX_COLS)
    nrows = math.ceil(n / ncols)
    fig, axes = plt.subplots(nrows, ncols, squeeze=False, figsize=(5 * ncols, 4 * nrows))
    flat = [ax for row in axes for ax in row]
    for ax in flat[n:]:
        ax.set_visible(False)
    return fig, flat[:n]


def shared_legend(fig: Figure, axes: list[Axes]) -> None:
    """Collect handles across all axes, dedupe by label, draw one figure legend."""
    seen: dict[str, object] = {}
    for ax in axes:
        for handle, label in zip(*ax.get_legend_handles_labels()):
            seen.setdefault(label, handle)
    if seen:
        fig.legend(
            list(seen.values()), list(seen.keys()),
            loc="upper center", ncol=min(len(seen), 4),
        )


def finish(
    fig: Figure, axes: list[Axes], *, title: Optional[str], out: Optional[Path]
) -> None:
    """Apply shared legend, optional suptitle, tight layout, optional save."""
    shared_legend(fig, axes)
    if title:
        fig.suptitle(title)
    fig.tight_layout()
    if out is not None:
        fig.savefig(out)
```

- [ ] **Step 4: Run test to verify it passes**

Run: `uv run pytest tests/test_facet.py -v`
Expected: PASS (2 passed)

- [ ] **Step 5: Commit**

```bash
git add kermit_lab/facet.py tests/test_facet.py
git commit -m "feat(kermit-lab): facet grid + shared-legend helper"
```

---

## Task 7: plot — entry point for bar + line

**Files:**
- Create: `kermit_lab/plot.py`
- Test: `tests/test_plot.py`

- [ ] **Step 1: Write the failing test**

```python
# tests/test_plot.py
"""kl.plot end-to-end for bar + line, including faceting and ablation axes."""
from __future__ import annotations

import matplotlib

matplotlib.use("Agg")

import matplotlib.pyplot as plt
from matplotlib.figure import Figure

from kermit_lab.frame import load
from kermit_lab.plot import plot


def test_line_scaling_returns_figure(fixture_tree) -> None:
    df = load(fixture_tree["paths"], fixture_tree["criterion_root"])
    fig = plot(df, kind="line", x="tuples", y="time",
               colour="data_structure", style="algorithm", logx=True, logy=True)
    assert isinstance(fig, Figure)
    plt.close(fig)


def test_bar_ablation_on_config_axis(fixture_opt_tree) -> None:
    df = load(fixture_opt_tree["paths"], fixture_opt_tree["criterion_root"])
    fig = plot(df, kind="bar", x="ds_config_singleton_pruning", y="time",
               colour="data_structure")
    assert isinstance(fig, Figure)
    plt.close(fig)


def test_facet_makes_one_axes_per_value(fixture_tree) -> None:
    df = load(fixture_tree["paths"], fixture_tree["criterion_root"])
    fig = plot(df, kind="bar", x="data_structure", y="time",
               colour="data_structure", facet="query")
    # fixture_tree has queries triangle, chain, star
    visible = [ax for ax in fig.axes if ax.get_visible() and ax.has_data()]
    assert len(visible) >= 1
    plt.close(fig)


def test_writes_file(fixture_tree, tmp_path) -> None:
    df = load(fixture_tree["paths"], fixture_tree["criterion_root"])
    out = tmp_path / "x.pdf"
    plot(df, kind="bar", x="data_structure", y="space", colour="data_structure", out=out)
    assert out.exists() and out.stat().st_size > 0
```

- [ ] **Step 2: Run test to verify it fails**

Run: `uv run pytest tests/test_plot.py -v`
Expected: FAIL — `ModuleNotFoundError: No module named 'kermit_lab.plot'`

- [ ] **Step 3: Implement `plot` (bar + line only; scatter/violin in Task 8)**

```python
# kermit_lab/plot.py
"""kl.plot — the general entry point. Maps channels to columns via AestheticMap,
facets, dispatches to a geom drawer, applies thesis style, optionally saves.
"""
from __future__ import annotations

from pathlib import Path
from typing import Mapping, Optional

import pandas as pd
from matplotlib.figure import Figure

from .encoding import AestheticMap, facet_values, resolve_scalar, select
from .facet import finish, make_grid
from .geoms import draw_bar, draw_line
from .plots_errors import InsufficientAxesError
from .styles import apply as apply_style


def plot(
    df: pd.DataFrame,
    *,
    kind: str,
    x: Optional[str] = None,
    y: str = "time",
    colour: Optional[str] = None,
    style: Optional[str] = None,
    facet: Optional[str] = None,
    filter: Optional[Mapping[str, object]] = None,
    phase: str = "iteration",
    samples: Optional[pd.DataFrame] = None,
    logx: bool = False,
    logy: bool = False,
    title: Optional[str] = None,
    out: Optional[Path] = None,
) -> Figure:
    """Render one figure. ``kind`` ∈ {bar, line, scatter, violin}."""
    amap = AestheticMap(
        kind=kind, x=x, y=y, colour=colour, style=style, facet=facet,
        filter=dict(filter or {}), phase=phase,
    )
    apply_style()

    sub = select(df, amap)
    fvals = facet_values(sub, amap)
    fig, axes = make_grid(len(fvals))
    for ax, fv in zip(axes, fvals):
        cell = sub if fv is None else sub[sub[amap.facet] == fv]
        series = resolve_scalar(cell, amap)
        if kind == "bar":
            draw_bar(ax, series, xlabel=str(amap.x), ylabel=amap.y)
        elif kind == "line":
            draw_line(ax, series, xlabel=str(amap.x), ylabel=amap.y, logx=logx, logy=logy)
        else:
            raise InsufficientAxesError(f"kind {kind!r} not yet supported")
        if fv is not None:
            ax.set_title(f"{amap.facet}={fv}")
    finish(fig, axes, title=title, out=out)
    return fig
```

- [ ] **Step 4: Run test to verify it passes**

Run: `uv run pytest tests/test_plot.py -v`
Expected: PASS (4 passed)

- [ ] **Step 5: Commit**

```bash
git add kermit_lab/plot.py tests/test_plot.py
git commit -m "feat(kermit-lab): kl.plot entry point (bar + line)"
```

---

## Task 8: scatter + violin + tradeoff (metric-x)

**Files:**
- Modify: `kermit_lab/encoding.py` (add `resolve_tradeoff`, `resolve_violin`)
- Modify: `kermit_lab/geoms.py` (add `draw_scatter`, `draw_violin`)
- Modify: `kermit_lab/plot.py` (dispatch scatter/violin; tradeoff metric-x branch)
- Test: `tests/test_plot.py` (extend)

- [ ] **Step 1: Write the failing tests (append to `tests/test_plot.py`)**

```python
def test_tradeoff_scatter(fixture_tree) -> None:
    df = load(fixture_tree["paths"], fixture_tree["criterion_root"])
    fig = plot(df, kind="scatter", x="space", y="time",
               colour="data_structure", style="algorithm", logx=True)
    assert isinstance(fig, Figure)
    assert any(len(ax.collections) > 0 for ax in fig.axes)
    plt.close(fig)


def test_violin_dist(fixture_tree) -> None:
    from kermit_lab.frame import load_samples
    df = load(fixture_tree["paths"], fixture_tree["criterion_root"])
    samples = load_samples(fixture_tree["paths"], fixture_tree["criterion_root"])
    fig = plot(df, kind="violin", x="data_structure", y="time",
               style="algorithm", samples=samples)
    assert isinstance(fig, Figure)
    plt.close(fig)
```

- [ ] **Step 2: Run to verify failure**

Run: `uv run pytest tests/test_plot.py -k "tradeoff or violin" -v`
Expected: FAIL — `InsufficientAxesError: kind 'scatter' not yet supported`

- [ ] **Step 3a: Add resolvers to `encoding.py`**

```python
def resolve_tradeoff(df: pd.DataFrame, amap: AestheticMap) -> list[ResolvedSeries]:
    """Scatter where both axes are metrics: x=space mean, y=time mean per group."""
    keys = ["source_path", "data_structure", "algorithm"]
    keys = [k for k in keys if k in df.columns]
    t = df[(df.metric == "time") & (df.phase == amap.phase)]
    s = df[df.metric == "space"]
    if t.empty or s.empty:
        raise InsufficientAxesError("tradeoff needs both time and space rows")
    t = t.groupby(keys, dropna=False)["mean_ns"].mean().reset_index(name="time_mean")
    s = s.groupby(keys, dropna=False)["mean_ns"].mean().reset_index(name="space_mean")
    m = t.merge(s, on=keys, how="inner")
    if m.empty:
        raise InsufficientAxesError("no group has both a time and a space measurement")
    group_cols = _group_cols(amap)
    series: list[ResolvedSeries] = []
    for keyt, g in _grouped_items(m, group_cols):
        xs = g["space_mean"].tolist()
        ys = g["time_mean"].tolist()
        zeros = [0.0] * len(xs)
        series.append(_make_series(keyt, group_cols, xs, ys, zeros, zeros, amap))
    return series


def resolve_violin(
    cell: pd.DataFrame, samples: pd.DataFrame, amap: AestheticMap
) -> list[ResolvedSeries]:
    """One violin per (colour×style group, x value) from per-iter samples."""
    if samples is None:
        raise InsufficientAxesError("violin requires a samples frame")
    if amap.x is None or amap.x not in cell.columns:
        raise InsufficientAxesError(f"x column {amap.x!r} not in frame")
    merged = cell.merge(samples, on=["criterion_group", "criterion_function"], how="inner")
    if merged.empty:
        raise InsufficientAxesError("no samples joined for violin")
    group_cols = _group_cols(amap)
    series: list[ResolvedSeries] = []
    for keyt, g in _grouped_items(merged, group_cols):
        xs: list = []
        samps: list[list[float]] = []
        for xval, gg in g.groupby(amap.x, dropna=False):
            xs.append(xval)
            samps.append(gg["per_iter_ns"].tolist())
        zeros = [0.0] * len(xs)
        series.append(_make_series(keyt, group_cols, xs, zeros, zeros, zeros, amap, samples=samps))
    return series
```

- [ ] **Step 3b: Add drawers to `geoms.py`**

```python
def draw_scatter(
    ax: Axes, series: list[ResolvedSeries], *, xlabel: str, ylabel: str,
    logx: bool = False, logy: bool = False,
) -> None:
    for s in series:
        ax.scatter(s.xs, s.ys, color=s.colour, marker=s.marker,
                   edgecolor="black", label=s.label)
    if logx:
        ax.set_xscale("log")
    if logy:
        ax.set_yscale("log")
    ax.set_xlabel(xlabel)
    ax.set_ylabel(ylabel)


def draw_violin(ax: Axes, series: list[ResolvedSeries], *, xlabel: str, ylabel: str) -> None:
    datasets: list[list[float]] = []
    labels: list[str] = []
    colours: list[str] = []
    for s in series:
        for x, samp in zip(s.xs, s.samples or []):
            datasets.append(samp)
            labels.append(f"{s.label}\n{x}" if s.label else str(x))
            colours.append(s.colour)
    if not datasets:
        return
    positions = list(range(1, len(datasets) + 1))
    parts = ax.violinplot(datasets, positions=positions, showmedians=True)
    for body, colour in zip(parts["bodies"], colours):
        body.set_facecolor(colour)
        body.set_alpha(0.7)
    ax.set_xticks(positions)
    ax.set_xticklabels(labels)
    ax.set_xlabel(xlabel)
    ax.set_ylabel(ylabel)
```

- [ ] **Step 3c: Dispatch in `plot.py`**

Update the imports:

```python
from .encoding import (
    AestheticMap, apply_filter, facet_values, resolve_scalar,
    resolve_tradeoff, resolve_violin, select,
)
from .geoms import draw_bar, draw_line, draw_scatter, draw_violin
```

Insert a tradeoff short-circuit right after `apply_style()` and before `select(...)`:

```python
    # Tradeoff: both axes are metrics, single panel, no facet/phase-select.
    if kind == "scatter" and x in ("time", "space"):
        import matplotlib.pyplot as plt

        series = resolve_tradeoff(apply_filter(df, amap), amap)
        fig, ax = plt.subplots()
        draw_scatter(ax, series, xlabel=str(x), ylabel=y, logx=logx, logy=logy)
        finish(fig, [ax], title=title, out=out)
        return fig
```

Replace the `else: raise` branch in the facet loop with scatter + violin handling:

```python
        if kind == "bar":
            draw_bar(ax, series, xlabel=str(amap.x), ylabel=amap.y)
        elif kind == "line":
            draw_line(ax, series, xlabel=str(amap.x), ylabel=amap.y, logx=logx, logy=logy)
        elif kind == "scatter":
            draw_scatter(ax, series, xlabel=str(amap.x), ylabel=amap.y, logx=logx, logy=logy)
        elif kind == "violin":
            vseries = resolve_violin(cell, samples, amap)
            draw_violin(ax, vseries, xlabel=str(amap.x), ylabel=amap.y)
        else:
            raise InsufficientAxesError(f"unknown kind {kind!r}")
```

Note: for `kind == "violin"`, `series = resolve_scalar(cell, amap)` would also run at the top of the loop — guard it. Change the loop body so scalar resolution only happens for non-violin kinds:

```python
    for ax, fv in zip(axes, fvals):
        cell = sub if fv is None else sub[sub[amap.facet] == fv]
        if kind == "violin":
            draw_violin(ax, resolve_violin(cell, samples, amap),
                        xlabel=str(amap.x), ylabel=amap.y)
        else:
            series = resolve_scalar(cell, amap)
            if kind == "bar":
                draw_bar(ax, series, xlabel=str(amap.x), ylabel=amap.y)
            elif kind == "line":
                draw_line(ax, series, xlabel=str(amap.x), ylabel=amap.y, logx=logx, logy=logy)
            elif kind == "scatter":
                draw_scatter(ax, series, xlabel=str(amap.x), ylabel=amap.y, logx=logx, logy=logy)
            else:
                raise InsufficientAxesError(f"unknown kind {kind!r}")
        if fv is not None:
            ax.set_title(f"{amap.facet}={fv}")
```

- [ ] **Step 4: Run to verify pass**

Run: `uv run pytest tests/test_plot.py -v`
Expected: PASS (6 passed)

- [ ] **Step 5: Commit**

```bash
git add kermit_lab/encoding.py kermit_lab/geoms.py kermit_lab/plot.py tests/test_plot.py
git commit -m "feat(kermit-lab): scatter + violin geoms and tradeoff metric-x path"
```

---

## Task 9: presets

**Files:**
- Create: `kermit_lab/presets.py`
- Test: `tests/test_presets.py`

- [ ] **Step 1: Write the failing test**

```python
# tests/test_presets.py
"""Presets build the right aesthetic and return a Figure."""
from __future__ import annotations

import matplotlib

matplotlib.use("Agg")

import matplotlib.pyplot as plt
from matplotlib.figure import Figure

from kermit_lab import presets
from kermit_lab.frame import load, load_samples


def test_scaling(fixture_tree) -> None:
    df = load(fixture_tree["paths"], fixture_tree["criterion_root"])
    assert isinstance(presets.scaling(df), Figure)
    plt.close("all")


def test_bar_time(fixture_tree) -> None:
    df = load(fixture_tree["paths"], fixture_tree["criterion_root"])
    assert isinstance(presets.bar_time(df, query="triangle"), Figure)
    plt.close("all")


def test_bar_space(fixture_tree) -> None:
    df = load(fixture_tree["paths"], fixture_tree["criterion_root"])
    assert isinstance(presets.bar_space(df), Figure)
    plt.close("all")


def test_tradeoff(fixture_tree) -> None:
    df = load(fixture_tree["paths"], fixture_tree["criterion_root"])
    assert isinstance(presets.tradeoff(df), Figure)
    plt.close("all")


def test_dist(fixture_tree) -> None:
    df = load(fixture_tree["paths"], fixture_tree["criterion_root"])
    samples = load_samples(fixture_tree["paths"], fixture_tree["criterion_root"])
    assert isinstance(presets.dist(df, samples=samples), Figure)
    plt.close("all")


def test_bar_queries(fixture_tree) -> None:
    df = load(fixture_tree["paths"], fixture_tree["criterion_root"])
    assert isinstance(presets.bar_queries(df, ds=["TreeTrie"]), Figure)
    plt.close("all")


def test_ablation(fixture_opt_tree) -> None:
    df = load(fixture_opt_tree["paths"], fixture_opt_tree["criterion_root"])
    assert isinstance(presets.ablation(df, axis="ds_config_singleton_pruning"), Figure)
    plt.close("all")
```

- [ ] **Step 2: Run to verify failure**

Run: `uv run pytest tests/test_presets.py -v`
Expected: FAIL — `ModuleNotFoundError: No module named 'kermit_lab.presets'`

- [ ] **Step 3: Implement presets**

```python
# kermit_lab/presets.py
"""Named presets: each fills an AestheticMap and calls kl.plot.

These are the legacy shapes, re-expressed as configurations of the general
engine. They exist for ergonomics and for the CLI / render-all to call.
"""
from __future__ import annotations

from pathlib import Path
from typing import Optional, Sequence

import pandas as pd
from matplotlib.figure import Figure

from .plot import plot


def scaling(df: pd.DataFrame, *, phase: str = "iteration", out: Optional[Path] = None) -> Figure:
    return plot(df, kind="line", x="tuples", y="time", colour="data_structure",
                style="algorithm", phase=phase, logx=True, logy=True,
                title="Scaling", out=out)


def bar_time(
    df: pd.DataFrame, *, query: str, phase: str = "iteration", out: Optional[Path] = None
) -> Figure:
    return plot(df, kind="bar", x="data_structure", y="time", colour="data_structure",
                style="algorithm", filter={"query": query}, phase=phase,
                title=f"Time — {query}", out=out)


def bar_space(df: pd.DataFrame, *, out: Optional[Path] = None) -> Figure:
    return plot(df, kind="bar", x="data_structure", y="space", colour="data_structure",
                title="Space per data structure", out=out)


def tradeoff(df: pd.DataFrame, *, phase: str = "iteration", out: Optional[Path] = None) -> Figure:
    return plot(df, kind="scatter", x="space", y="time", colour="data_structure",
                style="algorithm", phase=phase, logx=True, title="Space vs time", out=out)


def dist(
    df: pd.DataFrame, *, samples: pd.DataFrame, phase: str = "iteration",
    out: Optional[Path] = None,
) -> Figure:
    return plot(df, kind="violin", x="data_structure", y="time", style="algorithm",
                phase=phase, samples=samples, title="Per-iter distribution", out=out)


def bar_queries(
    df: pd.DataFrame, *, ds: Optional[Sequence[str]] = None,
    algo: Optional[Sequence[str]] = None, phase: str = "iteration",
    out: Optional[Path] = None,
) -> Figure:
    flt: dict[str, object] = {}
    if ds is not None:
        flt["data_structure"] = list(ds)
    if algo is not None:
        flt["algorithm"] = list(algo)
    return plot(df, kind="bar", x="query", y="time", colour="data_structure",
                style="algorithm", filter=flt, phase=phase,
                title="Across queries", out=out)


def ablation(
    df: pd.DataFrame, *, axis: str, phase: str = "iteration", out: Optional[Path] = None
) -> Figure:
    """Ablation: time vs an optimization axis, coloured by DS, faceted by query when >1."""
    facet = "query" if "query" in df.columns and df["query"].nunique(dropna=True) > 1 else None
    return plot(df, kind="bar", x=axis, y="time", colour="data_structure",
                facet=facet, phase=phase, title=f"Ablation — {axis}", out=out)
```

- [ ] **Step 4: Run to verify pass**

Run: `uv run pytest tests/test_presets.py -v`
Expected: PASS (7 passed)

- [ ] **Step 5: Commit**

```bash
git add kermit_lab/presets.py tests/test_presets.py
git commit -m "feat(kermit-lab): presets over the general plot engine"
```

---

## Task 10: rewire the public API (`__init__.py`)

**Files:**
- Modify: `kermit_lab/__init__.py`
- Test: `tests/test_public_api.py`

- [ ] **Step 1: Write the failing test**

```python
# tests/test_public_api.py
"""The package's public surface exposes the engine + presets."""
from __future__ import annotations

import kermit_lab as kl


def test_public_names() -> None:
    for name in ("plot", "scaling", "bar_time", "bar_space", "tradeoff",
                 "dist", "bar_queries", "ablation", "load", "load_samples",
                 "colour_for", "summary"):
        assert hasattr(kl, name), name


def test_plots_subpackage_gone() -> None:
    import importlib
    try:
        importlib.import_module("kermit_lab.plots")
    except ModuleNotFoundError:
        return
    raise AssertionError("kermit_lab.plots should be deleted")
```

> The second assertion only passes after Task 13 deletes `plots/`. Mark this
> test `@pytest.mark.xfail(strict=False)` for now if running Task 10 in
> isolation; Task 13 removes the marker.

- [ ] **Step 2: Run to verify failure**

Run: `uv run pytest tests/test_public_api.py -v`
Expected: FAIL — missing `kl.plot` / `kl.ablation`

- [ ] **Step 3: Rewrite `__init__.py`**

```python
# kermit_lab/__init__.py
"""Notebook-first analysis of kermit Criterion benchmark output.

Primary surface: :func:`load` / :func:`load_samples` return tidy DataFrames;
:func:`plot` renders any aesthetic; the named presets are thin configurations
of it. The CLI in :mod:`kermit_lab.drivers.main` is a thin wrapper.
"""

SCHEMA_VERSION = 2
"""Highest BenchReport schema version this package can parse."""

from .analysis import bootstrap_ratio_ci, compare, mannwhitney_u, summary
from .axis_mapping import colour_for, linestyle_for, marker_for
from .frame import discover_opt_columns, load, load_samples
from .plot import plot
from .plots_errors import InsufficientAxesError
from .presets import (
    ablation,
    bar_queries,
    bar_space,
    bar_time,
    dist,
    scaling,
    tradeoff,
)
from .styles import apply as apply_style

__all__ = [
    "SCHEMA_VERSION",
    "InsufficientAxesError",
    "ablation",
    "apply_style",
    "bar_queries",
    "bar_space",
    "bar_time",
    "bootstrap_ratio_ci",
    "colour_for",
    "compare",
    "discover_opt_columns",
    "dist",
    "linestyle_for",
    "load",
    "load_samples",
    "mannwhitney_u",
    "marker_for",
    "plot",
    "scaling",
    "summary",
    "tradeoff",
]
```

- [ ] **Step 4: Run to verify pass**

Run: `uv run pytest tests/test_public_api.py::test_public_names -v`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add kermit_lab/__init__.py tests/test_public_api.py
git commit -m "feat(kermit-lab): rewire public API to engine + presets"
```

---

## Task 11: render-all — preset-based + ablation auto-discovery

**Files:**
- Modify: `kermit_lab/drivers/render_all.py`
- Test: `tests/test_render_all.py` (rewrite)

- [ ] **Step 1: Write the failing test**

```python
# tests/test_render_all.py
"""render-all emits the fixed shapes plus one ablation figure per opt axis."""
from __future__ import annotations

import matplotlib

matplotlib.use("Agg")

from pathlib import Path

from kermit_lab.drivers.render_all import render_all
from kermit_lab.loader import load_reports


def test_emits_conventional_shapes(fixture_tree, tmp_path: Path) -> None:
    reports = load_reports(fixture_tree["paths"])
    out = tmp_path / "out"
    out.mkdir()
    render_all(reports, out, fixture_tree["criterion_root"], "pdf")
    names = {p.name for p in out.iterdir()}
    assert "scaling.pdf" in names
    assert "bar-space.pdf" in names
    assert any(n.startswith("bar-time-") for n in names)


def test_emits_ablation_for_opt_axis(fixture_opt_tree, tmp_path: Path) -> None:
    reports = load_reports(fixture_opt_tree["paths"])
    out = tmp_path / "out"
    out.mkdir()
    render_all(reports, out, fixture_opt_tree["criterion_root"], "pdf")
    names = {p.name for p in out.iterdir()}
    assert any("ablation-ds_config_singleton_pruning" in n for n in names)
    assert any("ablation-ds_layout_hasher" in n for n in names)
```

- [ ] **Step 2: Run to verify failure**

Run: `uv run pytest tests/test_render_all.py -v`
Expected: FAIL — ablation files absent (old render_all draws no ablation).

- [ ] **Step 3: Rewrite `render_all.py`**

```python
# kermit_lab/drivers/render_all.py
"""``render-all`` — emit every plot shape the input set supports.

Builds the summary + samples DataFrames once, then renders the fixed shapes,
per-query bar-time, per-(ds,algo) bar-queries, and — new — one ablation figure
per optimization axis carrying ≥2 distinct values. ``InsufficientAxesError`` is
demoted to an info log; other exceptions propagate. Skips are logged so
ablation coverage is never silently truncated.
"""
from __future__ import annotations

import logging
from pathlib import Path

from .. import presets
from ..frame import _samples_from_reports, _summary_from_reports, discover_opt_columns
from ..loader import BenchReport
from ..plots_errors import InsufficientAxesError

log = logging.getLogger(__name__)


def _ablation_axes(df) -> list[str]:
    """Optimization columns with ≥2 distinct non-null values."""
    out = []
    for col in discover_opt_columns(df):
        if df[col].nunique(dropna=True) >= 2:
            out.append(col)
        else:
            log.info("skipped ablation %s: <2 distinct values", col)
    return out


def render_all(
    reports: list[BenchReport],
    out_dir: Path,
    criterion_root: Path,
    fmt: str,
    *,
    phase: str = "iteration",
) -> None:
    suffix = f".{fmt}"
    df = _summary_from_reports(reports, criterion_root)
    samples = _samples_from_reports(reports, criterion_root)

    def _try(label: str, fn) -> None:
        try:
            fn()
            log.info("rendered %s", label)
        except InsufficientAxesError as e:
            log.info("skipped %s: %s", label, e)

    _try("scaling", lambda: presets.scaling(df, phase=phase, out=out_dir / f"scaling{suffix}"))
    _try("bar-space", lambda: presets.bar_space(df, out=out_dir / f"bar-space{suffix}"))
    _try("tradeoff", lambda: presets.tradeoff(df, phase=phase, out=out_dir / f"tradeoff{suffix}"))
    _try("dist", lambda: presets.dist(df, samples=samples, phase=phase,
                                      out=out_dir / f"dist{suffix}"))

    queries = sorted(df["query"].dropna().unique().tolist()) if "query" in df.columns else []
    for q in queries:
        _try(
            f"bar-time-{q}",
            lambda q=q: presets.bar_time(df, query=q, phase=phase,
                                         out=out_dir / f"bar-time-{q}{suffix}"),
        )

    if {"data_structure", "algorithm"} <= set(df.columns):
        pairs = sorted(
            df.dropna(subset=["data_structure", "algorithm"])
            .groupby(["data_structure", "algorithm"]).groups.keys()
        )
        for ds, algo in pairs:
            _try(
                f"bar-queries-{ds}-{algo}",
                lambda ds=ds, algo=algo: presets.bar_queries(
                    df, ds=[ds], algo=[algo], phase=phase,
                    out=out_dir / f"bar-queries-{ds}-{algo}{suffix}",
                ),
            )

    for axis in _ablation_axes(df):
        _try(
            f"ablation-{axis}",
            lambda axis=axis: presets.ablation(
                df, axis=axis, phase=phase, out=out_dir / f"ablation-{axis}{suffix}"
            ),
        )
```

- [ ] **Step 4: Run to verify pass**

Run: `uv run pytest tests/test_render_all.py -v`
Expected: PASS (2 passed)

- [ ] **Step 5: Commit**

```bash
git add kermit_lab/drivers/render_all.py tests/test_render_all.py
git commit -m "feat(kermit-lab): preset-based render-all with ablation auto-discovery"
```

---

## Task 12: CLI — general `plot` subcommand + preset routing

**Files:**
- Modify: `kermit_lab/drivers/main.py`
- Test: `tests/test_cli.py`

- [ ] **Step 1: Write the failing test**

```python
# tests/test_cli.py
"""CLI smoke: general plot subcommand + a preset subcommand."""
from __future__ import annotations

import matplotlib

matplotlib.use("Agg")

from pathlib import Path

from kermit_lab.drivers.main import main


def test_general_plot_subcommand(fixture_opt_tree, tmp_path: Path) -> None:
    out = tmp_path / "ablation.pdf"
    rc = main([
        "plot", *[str(p) for p in fixture_opt_tree["paths"]],
        "--criterion-root", str(fixture_opt_tree["criterion_root"]),
        "--out", str(out),
        "--kind", "bar", "--y", "time",
        "--x", "ds_config_singleton_pruning",
        "--colour", "data_structure",
    ])
    assert rc == 0
    assert out.exists() and out.stat().st_size > 0


def test_scaling_preset_subcommand(fixture_tree, tmp_path: Path) -> None:
    out = tmp_path / "s.pdf"
    rc = main([
        "scaling", *[str(p) for p in fixture_tree["paths"]],
        "--criterion-root", str(fixture_tree["criterion_root"]),
        "--out", str(out),
    ])
    assert rc == 0
    assert out.exists()
```

- [ ] **Step 2: Run to verify failure**

Run: `uv run pytest tests/test_cli.py -v`
Expected: FAIL — `invalid choice: 'plot'` (subcommand not registered) / import errors.

- [ ] **Step 3: Rewrite `drivers/main.py`**

Replace the module with the version below (drops the `plots.*` imports, adds the `plot` subcommand, routes presets through `presets` and `dist` through `load_samples`):

```python
# kermit_lab/drivers/main.py
"""``kermit-lab`` argparse dispatcher over the general plot engine + presets."""
from __future__ import annotations

import argparse
import logging
import sys
from pathlib import Path

import matplotlib.pyplot as plt

from .. import presets
from ..frame import load, load_samples
from ..loader import load_reports
from ..plot import plot
from ..plots_errors import InsufficientAxesError
from ..styles import apply as apply_style
from . import render_all

log = logging.getLogger("kermit-lab")


def _add_common(p: argparse.ArgumentParser) -> None:
    p.add_argument("reports", nargs="+", type=Path, help="BenchReport JSON file(s)")
    p.add_argument("--out", type=Path, required=True,
                   help="output file path (suffix determines format: pdf, png, svg, pgf)")
    p.add_argument("--criterion-root", type=Path, default=Path("target/criterion"),
                   help="Criterion artefact directory (default: target/criterion)")


def _add_phase(p: argparse.ArgumentParser) -> None:
    p.add_argument("--phase", choices=["insertion", "iteration"], default="iteration",
                   help="time-metric phase to plot (default: iteration)")


def _parse_filter(items: list[str] | None) -> dict[str, str]:
    out: dict[str, str] = {}
    for item in items or []:
        if "=" not in item:
            raise SystemExit(f"--filter expects k=v, got {item!r}")
        k, v = item.split("=", 1)
        out[k] = v
    return out


def _build_parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(prog="kermit-lab",
                                description="Render thesis-quality plots from kermit BenchReport JSON.")
    p.add_argument("-v", "--verbose", action="store_true", help="enable info-level logging")
    sub = p.add_subparsers(dest="command", required=True)

    p_plot = sub.add_parser("plot", help="general plot: bind any axis to any channel")
    _add_common(p_plot)
    _add_phase(p_plot)
    p_plot.add_argument("--kind", required=True, choices=["bar", "line", "scatter", "violin"])
    p_plot.add_argument("--x", default=None, help="x column (or 'space'/'time' for tradeoff)")
    p_plot.add_argument("--y", default="time", choices=["time", "space"])
    p_plot.add_argument("--colour", default=None)
    p_plot.add_argument("--style", default=None)
    p_plot.add_argument("--facet", default=None)
    p_plot.add_argument("--filter", action="append", default=None, metavar="K=V")
    p_plot.add_argument("--logx", action="store_true")
    p_plot.add_argument("--logy", action="store_true")

    p_scaling = sub.add_parser("scaling", help="log-log scaling preset")
    _add_common(p_scaling)
    _add_phase(p_scaling)

    p_bar_time = sub.add_parser("bar-time", help="time across DS for one query")
    _add_common(p_bar_time)
    _add_phase(p_bar_time)
    p_bar_time.add_argument("--query", required=True)

    p_bar_space = sub.add_parser("bar-space", help="space across DS")
    _add_common(p_bar_space)

    p_tradeoff = sub.add_parser("tradeoff", help="space vs time scatter")
    _add_common(p_tradeoff)
    _add_phase(p_tradeoff)

    p_dist = sub.add_parser("dist", help="violin of per-iter samples")
    _add_common(p_dist)
    _add_phase(p_dist)

    p_bar_queries = sub.add_parser("bar-queries", help="bars across queries")
    _add_common(p_bar_queries)
    _add_phase(p_bar_queries)
    p_bar_queries.add_argument("--ds", nargs="+", default=None)
    p_bar_queries.add_argument("--algo", nargs="+", default=None)

    p_ablation = sub.add_parser("ablation", help="time vs an optimization axis")
    _add_common(p_ablation)
    _add_phase(p_ablation)
    p_ablation.add_argument("--axis", required=True, help="optimization axis column name")

    p_render_all = sub.add_parser("render-all", help="render every applicable shape into --out-dir")
    p_render_all.add_argument("reports", nargs="+", type=Path)
    p_render_all.add_argument("--out-dir", type=Path, required=True)
    p_render_all.add_argument("--criterion-root", type=Path, default=Path("target/criterion"))
    p_render_all.add_argument("--format", default="pdf", choices=["pdf", "png", "svg", "pgf"])
    _add_phase(p_render_all)
    return p


def _dispatch(args: argparse.Namespace) -> int:
    df = load(args.reports, args.criterion_root)
    log.info("loaded %d row(s) from %d file(s)", len(df), len(args.reports))

    if args.command == "plot":
        fig = plot(df, kind=args.kind, x=args.x, y=args.y, colour=args.colour,
                   style=args.style, facet=args.facet, filter=_parse_filter(args.filter),
                   phase=args.phase, logx=args.logx, logy=args.logy, out=args.out)
    elif args.command == "scaling":
        fig = presets.scaling(df, phase=args.phase, out=args.out)
    elif args.command == "bar-time":
        fig = presets.bar_time(df, query=args.query, phase=args.phase, out=args.out)
    elif args.command == "bar-space":
        fig = presets.bar_space(df, out=args.out)
    elif args.command == "tradeoff":
        fig = presets.tradeoff(df, phase=args.phase, out=args.out)
    elif args.command == "dist":
        samples = load_samples(args.reports, args.criterion_root)
        fig = presets.dist(df, samples=samples, phase=args.phase, out=args.out)
    elif args.command == "bar-queries":
        fig = presets.bar_queries(df, ds=args.ds, algo=args.algo, phase=args.phase, out=args.out)
    elif args.command == "ablation":
        fig = presets.ablation(df, axis=args.axis, phase=args.phase, out=args.out)
    else:
        log.error("unknown command: %s", args.command)
        return 2
    plt.close(fig)
    return 0


def main(argv: list[str] | None = None) -> int:
    args = _build_parser().parse_args(argv)
    logging.basicConfig(
        level=logging.INFO if args.verbose else logging.WARNING,
        format="%(name)s %(levelname)s: %(message)s",
    )
    apply_style()
    try:
        if args.command == "render-all":
            args.out_dir.mkdir(parents=True, exist_ok=True)
            reports = load_reports(args.reports)
            log.info("loaded %d report(s)", len(reports))
            render_all.render_all(reports, args.out_dir, args.criterion_root,
                                  args.format, phase=args.phase)
            return 0
        rc = _dispatch(args)
        if rc != 0:
            return rc
    except InsufficientAxesError as e:
        log.error("%s: %s", args.command, e)
        return 3
    log.info("wrote %s", args.out)
    return 0


if __name__ == "__main__":
    sys.exit(main())
```

- [ ] **Step 4: Run to verify pass**

Run: `uv run pytest tests/test_cli.py -v`
Expected: PASS (2 passed)

- [ ] **Step 5: Commit**

```bash
git add kermit_lab/drivers/main.py tests/test_cli.py
git commit -m "feat(kermit-lab): general plot subcommand + preset routing in CLI"
```

---

## Task 13: delete `plots/` + obsolete tests

**Files:**
- Delete: `kermit_lab/plots/` (whole directory)
- Delete: `tests/test_scaling.py`, `tests/test_bar_time.py`, `tests/test_bar_queries.py`, `tests/test_bar_space.py`, `tests/test_dist.py`, `tests/test_tradeoff.py`, `tests/test_plots_figure_return.py`
- Modify: `tests/test_public_api.py` (remove the xfail marker if added)

- [ ] **Step 1: Delete the old modules and tests**

```bash
git rm -r kermit_lab/plots
git rm tests/test_scaling.py tests/test_bar_time.py tests/test_bar_queries.py \
       tests/test_bar_space.py tests/test_dist.py tests/test_tradeoff.py \
       tests/test_plots_figure_return.py
```

- [ ] **Step 2: Grep for stragglers**

Run: `grep -rn "kermit_lab.plots\|from ..plots\|from .plots\|InsufficientAxesError" kermit_lab tests | grep -v plots_errors`
Expected: no references to the deleted `plots` package (only `plots_errors` survives).

If any file still imports `from ..plots import InsufficientAxesError`, change it to `from ..plots_errors import InsufficientAxesError` (or `from .plots_errors ...`).

- [ ] **Step 3: Run the whole suite**

Run: `uv run pytest -q`
Expected: PASS (all tests green; no import errors).

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "refactor(kermit-lab): delete legacy plots/ shapes and their tests"
```

---

## Task 14: docs

**Files:**
- Modify: `python/kermit-lab/README.md`
- Modify: `CLAUDE.md` (Gotchas — the `kl.scaling()/kl.bar_time()` line)
- Modify: `docs/specs/2026-05-04-remove-criterion-graphs-design.md` (addendum)

- [ ] **Step 1: Update the kermit-lab README**

Replace the public-API section so it documents `kl.plot(...)` as the primary surface and the presets as configurations. Include this example block:

```markdown
## Public API

```python
import kermit_lab as kl

df = kl.load("bench-runs/*.json")          # tidy summary frame (incl. ds_*/algo_* axes)
samples = kl.load_samples("bench-runs/*.json")

# General engine — bind any axis to any channel:
kl.plot(df, kind="bar", x="ds_config_singleton_pruning",
        y="time", colour="data_structure", facet="query", out="ablation.pdf")

# Presets (configurations of kl.plot):
kl.scaling(df)            # line: time vs tuples, colour=DS, style=algo
kl.bar_time(df, query="triangle")
kl.bar_space(df)
kl.tradeoff(df)
kl.dist(df, samples=samples)
kl.bar_queries(df, ds=["TreeTrie"])
kl.ablation(df, axis="ds_layout_hasher")   # time vs an optimization axis
```

Optimization axes (`ds_layout_*`, `ds_config_*`, `ds_build_mode`) are loaded as
columns automatically; `kl.discover_opt_columns(df)` lists them.
```

- [ ] **Step 2: Update the CLAUDE.md gotcha**

In `CLAUDE.md`, find the "No Criterion auto-plots" gotcha and replace the
`kl.scaling()/kl.bar_time()/etc.` clause with a description of the new surface:

> Analysis and plotting live in `python/kermit-lab/` (uv-managed notebook-first
> library: `kl.load()` returns a pandas DataFrame including the `ds_*`/`algo_*`
> optimization axes; `kl.plot(df, kind=…, x=…, colour=…, facet=…)` is the
> general engine and `kl.scaling()/kl.bar_time()/kl.ablation()/etc.` are presets
> over it, each returning `matplotlib.figure.Figure`; `kl.summary`/`compare`/
> `bootstrap_ratio_ci`/`mannwhitney_u` for pivots/stats).

- [ ] **Step 3: Add a design-doc addendum**

Append to `docs/specs/2026-05-04-remove-criterion-graphs-design.md`:

```markdown
> **2026-06-10 addendum:** the six fixed plot shapes were generalized into a
> single configurable engine (`kl.plot`) plus presets, and the loader now
> surfaces the `ds_*`/`algo_*` optimization axes it previously dropped — see
> `docs/specs/2026-06-10-optimization-axis-graphs-design.md`. Ablation figures
> (time vs an optimization axis) are now drawable and auto-emitted by
> `render-all`.
```

- [ ] **Step 4: Verify the suite once more + commit**

Run: `uv run pytest -q`
Expected: PASS (all).

```bash
git add python/kermit-lab/README.md CLAUDE.md docs/specs/2026-05-04-remove-criterion-graphs-design.md
git commit -m "docs(kermit-lab): document the general plot engine + ablation"
```

---

## Final verification

- [ ] Run the full suite from `python/kermit-lab/`:

Run: `uv run pytest -q`
Expected: all tests pass, no warnings about missing `plots` module.

- [ ] Smoke-test the CLI end to end against a real bench run (optional, needs `target/criterion/`):

Run: `uv run kermit-lab render-all bench-runs/*.json --out-dir /tmp/figs --format png -v`
Expected: log lines for each rendered shape + `ablation-*` for any optimization axis with ≥2 values; skipped shapes logged at info level.
