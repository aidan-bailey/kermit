# Generalize kermit-lab plotting around optimization axes

**Date:** 2026-06-10
**Status:** Design approved; implementation not started
**Sibling specs:** `2026-05-04-remove-criterion-graphs-design.md`,
`bench-report-schema.md`, `optimization-standard.md`

## Problem

`kermit bench` deliberately produces no graphs; rendering lives in
`python/kermit-lab/`, which reads the per-function Criterion JSON plus the
`--report-json` `BenchReport`. Today that tool ships six bespoke plot shapes
(`scaling`, `bar_time`, `bar_queries`, `bar_space`, `tradeoff`, `dist`), each
hardcoding `data_structure` and `algorithm` as its only grouping axes.

This is insufficient for the platform's core research goal. The
optimization-standard (`optimization-standard.md`) makes Layout / Config /
BuildMode the central comparative dimension, emitted into bench reports under
the normative prefixes `ds_layout_*`, `ds_config_*`, `ds_build_mode` (and the
reserved `algo_*` analogues). `bench-report-schema.md` states these prefixes
are normative *because* "kermit-lab tooling relies on them for cross-DS
comparison" and that downstream code "should group on the relevant prefix to
perform ablation analysis."

The implementation does the opposite. `frame.py` carries a hardcoded
include-list of axis keys (`data_structure`, `algorithm`, `query`,
`benchmark`, `relation_path`, `tuples`, `arity`, `relations`,
`relation_bytes`) and **silently drops every other axis key**. The
optimization axes never become DataFrame columns, so no plot can bind them to
any visual channel. The failure is worst for Config and BuildMode: those are
runtime/construction choices on the *same* DS type, so they share one
`data_structure` value and, with the distinguishing axis dropped, collapse
onto each other and overplot silently — a figure that shows two interventions
as one bar.

Secondary gaps: there is no dataset/benchmark faceting (`scaling` collapses
every dataset onto the `tuples` axis; `bar_time` does one query at a time),
and `axis_mapping.py` only knows `TreeTrie`/`ColumnTrie` and
`LeapfrogTriejoin` — `HashTrie` already exists but has no committed colour.

## Decision

Replace the six bespoke plot modules with a single configurable encoding
engine: a thin "aesthetic map" over raw matplotlib that binds visual channels
(`x` / `y` / `colour` / `style` / `facet` / `filter`) to DataFrame columns —
**any** column, conventional or optimization. The six shapes become thin
presets that fill in a map. The loader is changed to surface the optimization
axes as columns so they are bindable.

This is a **clean break**: no backward-compatibility shims are kept. The
existing public functions (`kl.scaling`, `kl.bar_time`, …) are redefined as
presets; all call sites (tests, README, CLI, `render-all`, CLAUDE.md gotcha)
are updated to the new surface.

### Why a custom matplotlib layer, not seaborn/plotnine

1. **The data is already aggregated.** Y-values and 95% CIs come from
   Criterion's `estimates.json` (`mean.point` / `lower` / `upper`), not from
   raw samples. The headline value of seaborn/plotnine — statistical
   aggregation — is moot here; we would use either as a dumb drawer with
   `errorbar=None` and pay the dependency for nothing.
2. **kermit-lab exists to control LaTeX/PGF + the Wong colourblind palette**
   (the documented problem statement in
   `2026-05-04-remove-criterion-graphs-design.md`). A grammar library imposes
   its own aesthetic defaults that we would then fight to override.
3. **It is a thesis** (Priority 2: human-readable implementations). A
   hand-rolled aesthetic-mapping layer matches the existing house style and is
   defensible in a methods chapter; a ggplot DSL is more magic to explain.

The "thin layer" is small because the hard part (statistics) is solved
upstream: it is an aesthetic-map struct, a facet-grid helper, and a set of
dumb per-geom drawers.

## Architecture

### Module layout (`python/kermit-lab/kermit_lab/`)

| Module | Change | Responsibility |
| --- | --- | --- |
| `encoding.py` | new | The `AestheticMap` struct + value/CI resolution (filter, group-by). |
| `facet.py` | new | `plt.subplots` grid over the facet column's unique values + shared-legend layout. |
| `geoms.py` | new | `draw_bar` / `draw_line` / `draw_scatter` / `draw_violin` — dumb drawers consuming a resolved series. |
| `plot.py` | new | `kl.plot(...)` entry point: map → facet → geom dispatch → styled `Figure`. |
| `presets.py` | new | `scaling` / `bar_time` / `bar_queries` / `bar_space` / `tradeoff` / `dist` as mapping-constructors. |
| `defaults.py` | new | Documented axis defaults (e.g. `ds_layout_hasher → "sip"`). |
| `axis_mapping.py` | extend | Column-aware colour/marker/linestyle lookups; add `HashTrie`. |
| `frame.py` | change | Wildcard-wide ingest of `ds_*`/`algo_*` axes. |
| `plots/` (six modules) | delete | Logic absorbed into the engine + presets. |
| `drivers/main.py` | change | Add general `plot` subcommand; presets route through `presets.py`. |
| `drivers/render_all.py` | change | Auto-discover optimization axes and emit ablation figures. |

### The aesthetic map

```
AestheticMap:
    x:      column name        e.g. "tuples", "ds_config_singleton_pruning"
    y:      "time" | "space"   selects metric; value = mean_ns, CI = mean_lo/mean_hi
    colour: column | None      e.g. "data_structure"
    style:  column | None      linestyle/marker channel, e.g. "algorithm"
    facet:  column | None      small-multiples, e.g. "query"
    filter: dict[col, value]   hold axes fixed, e.g. {"benchmark": "triangle"}
    phase:  "insertion" | "iteration"   (time only; ignored for space)
```

### Data flow (one `kl.plot` call)

```
df ── apply filter (hold fixed axes) ──► subset
   ── select metric rows for y (+ phase if time) ──► tidy subset
   ── group by facet column ──► one subplot per unique value      (facet.py)
        └─ within each cell: group by colour × style ──► one geom series each (geoms.py)
               └─ y from mean_ns; error bars from mean_lo / mean_hi (precomputed)
   ── shared legend, Wong palette, rcParams, suffix-aware save ──► Figure
```

Geoms perform no statistics; they receive resolved `(positions, values, lo,
hi, label, colour, marker/linestyle)` and draw. `InsufficientAxesError` is
raised when a bound column is absent or under-populated (e.g. `scaling` needs
≥2 distinct `tuples` values) — the same failure mode the current shapes use.

### Public API

```python
# General entry point (1:1 with the CLI --kind flag):
kl.plot(df, kind="bar", x="ds_config_singleton_pruning",
        y="time", colour="data_structure", facet="query")   # the ablation figure

# Presets fill an AestheticMap and call plot():
kl.scaling(df)     # == plot(kind="line", x="tuples", y="time",
                   #         colour="data_structure", style="algorithm")
kl.bar_space(df)   # == plot(kind="bar",  x="data_structure", y="space")
```

`kind ∈ {bar, line, scatter, violin}`. A single `plot(kind=...)` entry point
was chosen over geom-named functions (`kl.bar()`, `kl.line()`) because it maps
1:1 to a CLI `--kind` flag.

## Loader changes (`frame.py`)

Keep today's conventional keys as the fixed **core**; discover the
optimization **tail** dynamically.

```
core (fixed, ordered as today):
    kind, metric, phase,
    data_structure, algorithm, query, benchmark, relation_path,
    tuples, arity, relations, relation_bytes

optimization tail (discovered union across all reports, sorted, appended):
    any axes key matching  ^(ds|algo)_(layout_|config_|build_mode)
    e.g. ds_layout_hasher, ds_config_singleton_pruning, ds_build_mode

then: mean_*, median_*, source_path, criterion_group, criterion_function
```

- **Discovery:** union of matching keys across all reports drives the tail
  columns. A report missing an axis yields NaN in that column (sparsity is
  meaningful — "knob not applicable to this structure").
- **Dtypes:** layout / build_mode values are strings; numeric values are
  `Int64`; **config flags are booleans** and need their own branch — the
  current int path explicitly excludes `bool` (`frame.py:68`), so a boolean
  config flag would otherwise be dropped. Config flags land as a nullable
  boolean column rendering `True`/`False` categories.
- **Core stays fixed:** only the optimization tail is data-dependent, so the
  "consistent schema even when `rows` is empty" property holds for the core.

### Defaults registry (`defaults.py`)

```python
AXIS_DEFAULTS = {"ds_layout_hasher": "sip"}   # schema-mandated for pre-standard reports
```

`load(..., apply_defaults=True)` does a `fillna` from this map after ingest;
the flag lets a notebook opt into the raw sparse view. This is the only place
a default is recorded. Splitting "how to read axes" (generic, `frame.py`)
from "what a missing axis means" (specific, `defaults.py`) keeps the parser
generic and mirrors the Rust side, where `HasOptimizationAxes` impls live
per-structure rather than in the bench reporter. Adding a new optimization
touches `defaults.py` + `axis_mapping.py`, never the loader.

## Visual encoding (`axis_mapping.py`)

Generalize the two hardcoded channels into column-aware lookups so any column
can drive colour/style:

```
colour_for(column, value):
    committed map if (column, value) is known   # e.g. data_structure → TreeTrie
    else stable Wong-palette rotation keyed on (column, value)

marker_for(column, value) / linestyle_for(column, value):   # same pattern for `style`
```

Committed maps remain the contract. Add `HashTrie` to
`DATA_STRUCTURE_COLOURS` (currently missing). Unknown categories (e.g.
`ds_layout_hasher` values `fx`/`sip`, boolean config flags) get a stable,
colourblind-safe fallback instead of being unrepresentable.

## CLI (`drivers/main.py`)

Add a general `plot` subcommand mirroring the engine; keep preset subcommands
as conveniences routing through `presets.py`.

```
kermit-lab plot reports.json -o fig.pdf \
    --kind bar --y time \
    --x ds_config_singleton_pruning \
    --colour data_structure --facet query \
    --filter benchmark=triangle

kermit-lab scaling reports.json -o s.pdf      # preset
kermit-lab render-all reports.json -d out/    # now also emits ablation figures
```

Shared args unchanged: out-path suffix → format (pdf/png/svg/pgf),
`--criterion-root`, `--phase`. `--filter k=v` is repeatable and populates the
map's `filter` dict.

## `render-all` auto-discovery (`drivers/render_all.py`)

Keep the existing fixed shapes plus per-query / per-`(ds, algo)` bars. Add one
rule: for every discovered optimization column with **≥2 distinct non-null
values**, emit an ablation figure (`kind=bar, x=<that axis>, y=time,
colour=data_structure`; facet by `query` when >1 query). Reuse the existing
`_try` pattern: `InsufficientAxesError` is demoted to an info log, every other
exception propagates. Per the "no silent caps" house rule, `render-all` logs
each axis emitted *and* each axis skipped for too-few values, so ablation
coverage is never silently truncated.

## Testing

Clean break ⇒ rewrite the suite rather than keep it green.

- **New unit tests:** `encoding` (map resolution, filter, group-by), `facet`
  (grid shape for N facet values + shared legend), `geoms` (each draws a
  non-empty artist), `presets` (each builds the expected map), `defaults`
  (back-fill `fillna`), `frame` (optimization columns appear, correct dtypes
  including boolean config flags, NaN sparsity, default applied).
- **`axis_mapping`:** committed maps honoured (including new `HashTrie`);
  fallback colour/marker stable across calls within a process.
- **Fixtures (`conftest.py`):** add `BenchReport` JSON fixtures that *carry*
  optimization axes — HashTrie `sip` vs `fx`, a config flag on/off — so the
  ablation path is exercised, not just the conventional shapes. This is the
  load-bearing new fixture; without it the feature is untested. It is also
  what converts "the spec says we support ablation" into "ablation is
  verified," closing the spec-vs-implementation gap that motivated this work.
- **Rewrite** `test_scaling.py` / `test_bar_time.py` / `test_bar_space.py` /
  `test_tradeoff.py` / `test_dist.py` / `test_bar_queries.py` to the preset
  API, and `test_render_all.py` for the new auto-discovery. Headless Agg
  backend as today.

## Docs to update

- kermit-lab `README` — new public API surface.
- `CLAUDE.md` Gotchas — the `kl.scaling()/kl.bar_time()` reference.
- An addendum to `2026-05-04-remove-criterion-graphs-design.md` pointing at the
  new engine.

`bench-report-schema.md` and `optimization-standard.md` need no change — they
already promise this behaviour; this work makes the implementation honour it.

## Non-goals / out of scope

- No change to the Rust bench side, the `BenchReport` schema, or
  `schema_version` (the `axes` map is already open; no new keys are minted
  here).
- No new statistical methods; `analysis.py` (`summary`, `compare`,
  `bootstrap_ratio_ci`, `mannwhitney_u`) is untouched.
- `algo_layout_*` / `algo_config_*` / `algo_build_mode` are ingested by the
  generic wildcard rule (they match `^algo_`), but no algorithm emits them
  yet; no algorithm-specific presets are added.
- No interactive/HTML output; PDF/PNG/SVG/PGF only, as today.

## Migration

Clean break, single change. Delete `plots/`, add the engine modules, rewrite
the loader and tests, update the four call-site doc/CLI surfaces. No deprecation
window; no compatibility shims.
