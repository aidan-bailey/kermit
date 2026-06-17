# LUBM Reference Comparison Notebook Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A supervisor-ready reference benchmark: a committed declarative benchmark spec (`benchmarks/lubm-reference.yml`) plus an end-to-end Jupyter notebook comparing the three index-structure configurations (TreeTrie+LeapfrogTriejoin, ColumnTrie+LeapfrogTriejoin, HashTrie+HashTriejoin) on LUBM(1, 0) with live correctness verification, a single three-metric sweep, and a six-figure/two-table analysis suite.

**Architecture:** The notebook is pure orchestration + analysis — zero Rust changes. It shells out to the `kermit` CLI via a streamed-subprocess helper (proven in `00_full_timeline.ipynb`), then analyses results with the existing `kermit_lab` Python library. The benchmark spec is declarative: `bench run lubm-reference` materialises LUBM(1, 0) on demand with spec-hash caching (`kermit/src/materialize.rs`). Spec: `docs/specs/2026-06-11-lubm-reference-notebook-design.md`.

**Tech Stack:** Jupyter notebook (nbformat 4.5, committed output-free, JSON `indent=1` — matching `00_full_timeline.ipynb`), `kermit_lab` (pandas/matplotlib/scipy), kermit CLI (`cargo run --release -- bench …`), uv-managed venv at `python/kermit-lab/`.

**Repo deltas (exhaustive):**
- Create: `benchmarks/lubm-reference.yml`
- Create: `python/kermit-lab/notebooks/07_lubm_reference_comparison.ipynb`

Nothing else may be modified (spec "Out of scope"; CLAUDE.md Priorities item 6).

**Key facts the implementer must know (verified against source):**

- `bench fetch` does NOT materialise generator benchmarks — only the `bench run` path calls `materialize::materialize` (`kermit/src/main.rs:1541`). The first sweep run generates the data inline.
- `--metrics` is space-separated, `num_args = 1..`, and one `bench run` invocation handles time AND space Criterion instances (`kermit/src/main.rs:975` and `:1027`). One report JSON for everything — which is exactly what `kl.tradeoff` needs (it merges time/space rows on `source_path`).
- ~~`-i all -a all` skips incompatible pairs via `is_compatible`~~ **CORRECTED during execution review:** the `is_compatible` check (`kermit/src/main.rs:134`) only validates the top-level selector pair; the `all × all` cross-product expansion does NOT skip incompatible pairs and panics at `kermit/src/db.rs:313` on `(TreeTrie, HashTriejoin)`, writing no report. The sweep must enumerate the 3 valid configurations explicitly (see "Execution corrections" below). `(HashTrie, HashTriejoin)` is HashTrie's only valid pairing.
- Report axes carry `data_structure` as the enum Debug name (`"TreeTrie"`, `"ColumnTrie"`, `"HashTrie"`) and `algorithm` as `"LeapfrogTriejoin"` / `"HashTriejoin"`. The hasher is a separate `ds_layout_hasher` axis column (backfilled `"sip"`), NOT part of the name.
- `kl.compare(df, baseline=…, target=…, group_by=…)` inner-joins baseline/target rows on EVERY column except `group_by`, provenance (`source_path`, `criterion_group`, `criterion_function`), and the value family (`mean_*`, `median_*`). Cross-algorithm comparisons therefore need a synthesized `config` column with `data_structure`, `algorithm`, and all opt-axis columns dropped — otherwise the join produces zero rows.
- `kl.compare` output columns: join keys + `baseline_value/baseline_lo/baseline_hi`, `target_value/target_lo/target_hi`, `speedup`, `speedup_lo`, `speedup_hi` (`speedup = baseline / target`, >1 means target faster).
- `kl.bootstrap_ratio_ci(a, b, *, rng=…)` → percentile-bootstrap CI for `mean(a)/mean(b)`. `kl.mannwhitney_u(a, b)` → `(U, p)`.
- Space is benchmarked per relation (`space/{rel_name}` functions, `kermit/src/main.rs:1027-1037`), so space rows repeat per query; presets aggregate.
- LUBM expected cardinalities are only emitted at scale 1 (`kermit/src/main.rs:1745-1749`), as `expected/q<N>.csv` files with header `cardinality` and one value. Cache dir on Linux: `~/.cache/kermit/benchmarks/lubm-reference/`.
- q2's reference cardinality at LUBM(1, 0) is **0** — do not pick it as the deep-dive query. The spec picks q9 (six body atoms, triangle-shaped).
- Existing notebooks are committed output-free (`execution_count: null`, `outputs: []`), nbformat 4.5, cell `id`s present, JSON `indent=1`.
- `cargo` and `java` must be on PATH: run all commands in this plan from inside `nix develop` (or `nix develop --command <cmd>`).

---

### Task 1: Declarative benchmark spec `benchmarks/lubm-reference.yml`

**Files:**
- Create: `benchmarks/lubm-reference.yml`

There is no new Rust test to write: every `benchmarks/*.yml` is parsed by `kermit-bench` discovery on any `bench list` invocation, so a malformed YAML fails step 2 loudly. Omitting `queries:` under `generator:` selects all 14 LUBM queries (`kermit-bench/src/definition.rs:338` — a *provided* list must be non-empty; an omitted list means "all").

- [ ] **Step 1: Create the YAML**

Write `benchmarks/lubm-reference.yml` with exactly:

```yaml
name: lubm-reference
description: "Reference comparison: LUBM(1,0), all 14 queries, all data structures"
generator:
  kind: lubm
  scale: 1
```

- [ ] **Step 2: Verify discovery and status**

Run: `cargo run --release -- bench list 2>&1 | grep -A1 lubm-reference`

Expected: a `lubm-reference` entry with status `not generated` (or `cached` if a prior run already materialised it — both prove the YAML parses and the generator block is recognised). Any parse error fails the whole `bench list` command instead.

- [ ] **Step 3: Commit**

```bash
git add benchmarks/lubm-reference.yml
git commit -m "feat(bench): add lubm-reference declarative benchmark

LUBM(1,0), all 14 queries, materialised on demand via the declarative
generator path. Anchor workload for the reference comparison notebook
(docs/specs/2026-06-11-lubm-reference-notebook-design.md)."
```

---

### Task 2: Notebook skeleton — title, Phase 0 preflight, profile knob

**Files:**
- Create: `python/kermit-lab/notebooks/07_lubm_reference_comparison.ipynb`

The notebook is built incrementally across Tasks 2–8 by Python heredoc scripts that append cells and re-serialise. Each task's script repeats the same helper block so tasks are independently readable. Cell text is stripped of leading/trailing newlines by the helper, so triple-quoted blocks can start on their own line.

- [ ] **Step 1: Create the notebook with the first five cells**

Run this script (from the repo root):

```bash
python3 - <<'PY'
import json, secrets
from pathlib import Path

NB_PATH = Path("python/kermit-lab/notebooks/07_lubm_reference_comparison.ipynb")

def _cell(cell_type, text):
    cell = {
        "cell_type": cell_type,
        "id": secrets.token_hex(4),
        "metadata": {},
        "source": text.strip("\n").splitlines(keepends=True),
    }
    if cell_type == "code":
        cell["execution_count"] = None
        cell["outputs"] = []
    return cell

nb = {
    "cells": [],
    "metadata": {
        "kernelspec": {"display_name": "Python 3", "language": "python", "name": "python3"},
        "language_info": {"name": "python"},
    },
    "nbformat": 4,
    "nbformat_minor": 5,
}

def md(text): nb["cells"].append(_cell("markdown", text))
def code(text): nb["cells"].append(_cell("code", text))

md('''
# LUBM reference comparison — all three index structures

This is the **reference benchmark** for comparing kermit's three index
structures end-to-end. Every step runs live from this notebook:

1. **Verify** — prove the join engine returns the published LUBM(1, 0)
   cardinalities for all 14 queries *before* timing anything.
2. **Sweep** — one `bench run` across all index structures, algorithms, and
   metrics (insertion time, iteration time, heap space). The LUBM(1, 0)
   dataset is generated on demand and cached.
3. **Analyse** — six figures and two statistical tables.

## The three configurations

`HashTrie` implements `HashTrieIterable`, not `TrieIterable`, so it cannot
run LeapfrogTriejoin — `(HashTrie, HashTriejoin)` is the only valid pairing.
The comparison is therefore between three *system configurations*, not three
structures under one algorithm:

| Configuration | Index structure | Join algorithm |
|---|---|---|
| TreeTrie + LeapfrogTriejoin | pointer-based trie | worst-case-optimal multi-way join |
| ColumnTrie + LeapfrogTriejoin | column-oriented trie | worst-case-optimal multi-way join |
| HashTrie + HashTriejoin | hash-based trie | hash-trie multi-way join |

## Requirements

Run from inside `nix develop` (provides cargo, JDK 8 for the LUBM generator,
and the library path the Python wheels need on NixOS). Launch with:

```sh
cd python/kermit-lab && uv run --with jupyter jupyter lab
```

Design rationale: `docs/specs/2026-06-11-lubm-reference-notebook-design.md`.
Workload reference: `docs/benchmarks/LUBM.md`.
''')

md('''
## Phase 0 — Preflight

Locate the repo root, fail loudly if the toolchain is missing, and define
`run_phase`, the streamed-subprocess helper every phase below uses.
''')

code('''
import os, subprocess, shutil, time
from pathlib import Path

# Resolve the repo root by walking up until we find Cargo.toml.
# This makes the notebook robust to wherever the kernel was launched from.
REPO_ROOT = Path.cwd()
while not (REPO_ROOT / "Cargo.toml").exists():
    if REPO_ROOT.parent == REPO_ROOT:
        raise RuntimeError("Could not locate Kermit repo root (no Cargo.toml found above CWD)")
    REPO_ROOT = REPO_ROOT.parent

os.chdir(REPO_ROOT)
print(f"CWD: {os.getcwd()}")

# Fail loud upfront if the toolchain is missing — better than a confusing
# error 30 seconds into a phase.
assert shutil.which("cargo"), "cargo not on PATH — run from inside `nix develop`"
assert shutil.which("java"),  "java not on PATH — needed by the LUBM generator"
print("cargo:", shutil.which("cargo"))
print("java: ", shutil.which("java"))


def run_phase(args, label):
    """Run a kermit subprocess with timing and streamed output.

    Each phase invokes `cargo run --release -- bench …` (or `cargo test`)
    and may take several minutes on a cold build. Output is streamed live to
    the notebook so the cell does not appear hung. We keep a rolling buffer
    of the last 40 lines and surface it on non-zero exit before raising
    CalledProcessError.
    """
    print(f"⏳ {label} - this may take several minutes...")
    start = time.monotonic()
    proc = subprocess.Popen(
        args,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
        bufsize=1,
    )
    assert proc.stdout is not None
    last_lines: list[str] = []
    for line in proc.stdout:
        line = line.rstrip()
        last_lines.append(line)
        if len(last_lines) > 40:
            last_lines.pop(0)
        print(line)
    rc = proc.wait()
    elapsed = time.monotonic() - start
    if rc != 0:
        raise subprocess.CalledProcessError(rc, args, output="\\n".join(last_lines))
    print(f"✅ {label} - done in {elapsed:.1f}s")
    return proc
''')

md('''
### Sampling profile

`quick` is for live demos (~15–30 min on a cold cache, most of it the one-off
dataset generation and the correctness test). `full` is for thesis-grade
numbers — expect several times longer. Criterion output directories are
shared between profiles, so figures always reflect the most recent sweep;
the report JSON is profile-suffixed so `kl.load` picks up the right run.
''')

code('''
PROFILE = "quick"  # "quick" | "full"

SAMPLING = {
    "quick": ["--sample-size", "10", "--measurement-time", "1", "--warm-up-time", "1"],
    "full":  ["--sample-size", "50", "--measurement-time", "3", "--warm-up-time", "1"],
}[PROFILE]
REPORT_JSON = f"bench-runs/lubm-reference-sweep-{PROFILE}.json"
print(f"profile={PROFILE}")
print(f"sampling flags: {' '.join(SAMPLING)}")
print(f"report → {REPORT_JSON}")
''')

NB_PATH.write_text(json.dumps(nb, indent=1, ensure_ascii=False) + "\n")
print(f"{len(nb['cells'])} cells")
PY
```

Expected output: `5 cells`

- [ ] **Step 2: Validate the JSON**

Run: `python3 -c "import json; nb=json.load(open('python/kermit-lab/notebooks/07_lubm_reference_comparison.ipynb')); assert nb['nbformat']==4 and len(nb['cells'])==5; print('OK')"`

Expected: `OK`

- [ ] **Step 3: Commit**

```bash
git add python/kermit-lab/notebooks/07_lubm_reference_comparison.ipynb
git commit -m "feat(kermit-lab): reference notebook — preflight and profile knob"
```

---

### Task 3: Phase 1 — correctness verification cells

**Files:**
- Modify: `python/kermit-lab/notebooks/07_lubm_reference_comparison.ipynb` (append 2 cells)

- [ ] **Step 1: Append the cells**

```bash
python3 - <<'PY'
import json, secrets
from pathlib import Path

NB_PATH = Path("python/kermit-lab/notebooks/07_lubm_reference_comparison.ipynb")

def _cell(cell_type, text):
    cell = {
        "cell_type": cell_type,
        "id": secrets.token_hex(4),
        "metadata": {},
        "source": text.strip("\n").splitlines(keepends=True),
    }
    if cell_type == "code":
        cell["execution_count"] = None
        cell["outputs"] = []
    return cell

nb = json.loads(NB_PATH.read_text())

def md(text): nb["cells"].append(_cell("markdown", text))
def code(text): nb["cells"].append(_cell("code", text))

md('''
## Phase 1 — Correctness before timing

Timing numbers from an engine that returns wrong results are worthless, so
the first real step is the cardinality oracle:
`kermit/tests/lubm_cardinalities.rs` generates LUBM(1, 0) end-to-end
(vendored UBA jar → N-Triples → Univ-Bench TBox entailment → partition →
parquet), runs all 14 queries through the join engine, and asserts every
result count matches the published reference cardinalities (LUBM paper,
Table 3).

The test deliberately regenerates into a tempdir rather than reusing the
benchmark cache: it validates the *pipeline*, not a cached snapshot. On
notebook reruns you can skip this cell — it is independent of everything
below.
''')

code('''
run_phase(
    ["cargo", "test", "-p", "kermit", "--test", "lubm_cardinalities",
     "--release", "--", "--nocapture"],
    "Phase 1: verify all 14 LUBM(1,0) cardinalities match the paper",
)
''')

NB_PATH.write_text(json.dumps(nb, indent=1, ensure_ascii=False) + "\n")
print(f"{len(nb['cells'])} cells")
PY
```

Expected output: `7 cells`

- [ ] **Step 2: Validate the JSON**

Run: `python3 -c "import json; nb=json.load(open('python/kermit-lab/notebooks/07_lubm_reference_comparison.ipynb')); assert len(nb['cells'])==7; print('OK')"`

Expected: `OK`

- [ ] **Step 3: Commit**

```bash
git add python/kermit-lab/notebooks/07_lubm_reference_comparison.ipynb
git commit -m "feat(kermit-lab): reference notebook — correctness phase"
```

---

### Task 4: Phase 2 — benchmark sweep cells (materialises on first run)

> **[Execution correction]** The Phase 2 cell heredoc below is preserved as the
> historical record and still contains the false claim that `-a all` skips
> incompatible pairs (line 376). It does not — see the struck-through key fact
> above and the "Execution corrections (2026-06-11)" section at the end of this
> document for the panic and the per-configuration sweep that replaced it.

**Files:**
- Modify: `python/kermit-lab/notebooks/07_lubm_reference_comparison.ipynb` (append 6 cells)

- [ ] **Step 1: Append the cells**

```bash
python3 - <<'PY'
import json, secrets
from pathlib import Path

NB_PATH = Path("python/kermit-lab/notebooks/07_lubm_reference_comparison.ipynb")

def _cell(cell_type, text):
    cell = {
        "cell_type": cell_type,
        "id": secrets.token_hex(4),
        "metadata": {},
        "source": text.strip("\n").splitlines(keepends=True),
    }
    if cell_type == "code":
        cell["execution_count"] = None
        cell["outputs"] = []
    return cell

nb = json.loads(NB_PATH.read_text())

def md(text): nb["cells"].append(_cell("markdown", text))
def code(text): nb["cells"].append(_cell("code", text))

md('''
## Phase 2 — The benchmark sweep

One invocation covers everything: all index structures (`-i all`), all
algorithms (`-a all` — incompatible pairs are skipped automatically), and
all three metrics:

- **insertion** — time to build each index from tuples,
- **iteration** — time to execute each query (the join itself),
- **space** — heap bytes of the built index (`heap_size_bytes()`, measured
  per relation via a custom Criterion measurement).

On the first run the sweep also **materialises** the dataset:
`benchmarks/lubm-reference.yml` declares `generator: {kind: lubm, scale: 1}`,
and `bench run` drives the vendored jar → entailment → partition → parquet
pipeline into `~/.cache/kermit/benchmarks/lubm-reference/`, recording
provenance (jar SHA-256, spec hash) in `meta.json`. There is deliberately no
standalone "generate" command for declarative specs — the first minutes of
streamed output below *are* the generation pipeline. Reruns hit the
spec-hash cache and skip generation entirely; after editing the YAML, rerun
with `--force` (drift never auto-regenerates).
''')

code('''
run_phase(
    ["cargo", "run", "--release", "--", "bench", "list"],
    "bench list (before) — expect lubm-reference: not generated, or cached on reruns",
)
''')

code('''
run_phase(
    ["cargo", "run", "--release", "--",
     "bench",
     *SAMPLING,
     "--report-json", REPORT_JSON,
     "run", "lubm-reference",
     "-i", "all",
     "-a", "all",
     "--metrics", "insertion", "iteration", "space"],
    "Phase 2: full sweep (3 configurations × 14 queries × 3 metrics)",
)
''')

code('''
run_phase(
    ["cargo", "run", "--release", "--", "bench", "list"],
    "bench list (after) — lubm-reference is now cached",
)
''')

md('''
### Reference cardinalities

The pipeline wrote `expected/q*.csv` next to the generated data (only done
at scale 1, where the published numbers apply — `kermit/src/main.rs:1745`).
These are the counts Phase 1 verified, shown here as the ground truth behind
every figure below. The cache path is the Linux convention; on other
platforms adjust to your platform cache dir.
''')

code('''
import pandas as pd

CACHE = Path.home() / ".cache" / "kermit" / "benchmarks" / "lubm-reference"
files = sorted((CACHE / "expected").glob("q*.csv"), key=lambda p: int(p.stem[1:]))
expected = pd.DataFrame(
    {"query": f.stem, "expected_cardinality": int(f.read_text().splitlines()[1])}
    for f in files
)
assert len(expected) == 14, f"expected 14 reference cardinalities, found {len(expected)}"
expected.set_index("query")
''')

NB_PATH.write_text(json.dumps(nb, indent=1, ensure_ascii=False) + "\n")
print(f"{len(nb['cells'])} cells")
PY
```

Expected output: `13 cells`

- [ ] **Step 2: Validate the JSON**

Run: `python3 -c "import json; nb=json.load(open('python/kermit-lab/notebooks/07_lubm_reference_comparison.ipynb')); assert len(nb['cells'])==13; print('OK')"`

Expected: `OK`

- [ ] **Step 3: Commit**

```bash
git add python/kermit-lab/notebooks/07_lubm_reference_comparison.ipynb
git commit -m "feat(kermit-lab): reference notebook — benchmark sweep phase"
```

---

### Task 5: Phase 3 — load into pandas + completeness check

**Files:**
- Modify: `python/kermit-lab/notebooks/07_lubm_reference_comparison.ipynb` (append 3 cells)

- [ ] **Step 1: Append the cells**

```bash
python3 - <<'PY'
import json, secrets
from pathlib import Path

NB_PATH = Path("python/kermit-lab/notebooks/07_lubm_reference_comparison.ipynb")

def _cell(cell_type, text):
    cell = {
        "cell_type": cell_type,
        "id": secrets.token_hex(4),
        "metadata": {},
        "source": text.strip("\n").splitlines(keepends=True),
    }
    if cell_type == "code":
        cell["execution_count"] = None
        cell["outputs"] = []
    return cell

nb = json.loads(NB_PATH.read_text())

def md(text): nb["cells"].append(_cell("markdown", text))
def code(text): nb["cells"].append(_cell("code", text))

md('''
## Phase 3 — Load into pandas

`kl.load` joins the bench report JSON with the Criterion estimates under
`target/criterion/` into one tidy DataFrame: one row per (configuration,
query, metric, phase / relation). The completeness check below makes a
missing combination fail loudly here rather than silently shrinking a
figure later.
''')

code('''
import kermit_lab as kl

kl.apply_style()  # Wong/Okabe-Ito palette; call once per session
df = kl.load(REPORT_JSON, criterion_root="target/criterion")
print(f"{len(df)} rows × {len(df.columns)} columns")
df.head()
''')

code('''
EXPECTED_CONFIGS = {
    "TreeTrie+LeapfrogTriejoin",
    "ColumnTrie+LeapfrogTriejoin",
    "HashTrie+HashTriejoin",
}

coverage = (
    df.assign(config=df["data_structure"] + "+" + df["algorithm"])
      .groupby("config")["query"]
      .nunique()
)
print(coverage)
assert set(coverage.index) == EXPECTED_CONFIGS, (
    f"unexpected configurations: {sorted(coverage.index)}"
)
assert (coverage == 14).all(), "every configuration must cover all 14 queries"
print("✅ 3 configurations × 14 queries, all present")
''')

NB_PATH.write_text(json.dumps(nb, indent=1, ensure_ascii=False) + "\n")
print(f"{len(nb['cells'])} cells")
PY
```

Expected output: `16 cells`

- [ ] **Step 2: Validate the JSON**

Run: `python3 -c "import json; nb=json.load(open('python/kermit-lab/notebooks/07_lubm_reference_comparison.ipynb')); assert len(nb['cells'])==16; print('OK')"`

Expected: `OK`

- [ ] **Step 3: Commit**

```bash
git add python/kermit-lab/notebooks/07_lubm_reference_comparison.ipynb
git commit -m "feat(kermit-lab): reference notebook — load phase"
```

---

### Task 6: Phase 4 — comparison figures F1–F4

**Files:**
- Modify: `python/kermit-lab/notebooks/07_lubm_reference_comparison.ipynb` (append 8 cells)

- [ ] **Step 1: Append the cells**

```bash
python3 - <<'PY'
import json, secrets
from pathlib import Path

NB_PATH = Path("python/kermit-lab/notebooks/07_lubm_reference_comparison.ipynb")

def _cell(cell_type, text):
    cell = {
        "cell_type": cell_type,
        "id": secrets.token_hex(4),
        "metadata": {},
        "source": text.strip("\n").splitlines(keepends=True),
    }
    if cell_type == "code":
        cell["execution_count"] = None
        cell["outputs"] = []
    return cell

nb = json.loads(NB_PATH.read_text())

def md(text): nb["cells"].append(_cell("markdown", text))
def code(text): nb["cells"].append(_cell("code", text))

md('''
## Phase 4 — Analysis

Each figure answers one question, stated in the heading above it.

### F1 — Which configuration answers each query fastest?

Iteration (query-execution) time per LUBM query, log scale — the 14 queries
span orders of magnitude in selectivity and join width.
''')

code('''
kl.plot(
    df, kind="bar", x="query", y="time",
    colour="data_structure", style="algorithm",
    phase="iteration", logy=True,
    title="Iteration time per LUBM query (log scale)",
);
''')

md('''
### F2 — What does building each index cost?

Insertion time: constructing the index from tuples. This matters when data
is loaded once and queried many times (amortised) versus loaded per query
(dominant).
''')

code('''
kl.plot(
    df, kind="bar", x="data_structure", y="time",
    colour="data_structure", style="algorithm",
    phase="insertion",
    title="Index build (insertion) time",
);
''')

md('''
### F3 — What does each index cost in memory?

Heap bytes of the built indexes (`heap_size_bytes()` — heap-allocated bytes
only, not `size_of::<Self>`), measured per relation and aggregated.
''')

code('''
kl.bar_space(df);
''')

md('''
### F4 — Is any configuration Pareto-dominant?

Space versus iteration time; down and left is better. `kl.tradeoff` merges
the time and space rows on `source_path`, which works here because the
single sweep invocation put both metrics in one report.
''')

code('''
kl.tradeoff(df);
''')

NB_PATH.write_text(json.dumps(nb, indent=1, ensure_ascii=False) + "\n")
print(f"{len(nb['cells'])} cells")
PY
```

Expected output: `24 cells`

- [ ] **Step 2: Validate the JSON**

Run: `python3 -c "import json; nb=json.load(open('python/kermit-lab/notebooks/07_lubm_reference_comparison.ipynb')); assert len(nb['cells'])==24; print('OK')"`

Expected: `OK`

- [ ] **Step 3: Commit**

```bash
git add python/kermit-lab/notebooks/07_lubm_reference_comparison.ipynb
git commit -m "feat(kermit-lab): reference notebook — comparison figures"
```

---

### Task 7: Phase 4 — statistics: ratio table (T1), q9 deep dive (F5, T2)

**Files:**
- Modify: `python/kermit-lab/notebooks/07_lubm_reference_comparison.ipynb` (append 6 cells)

- [ ] **Step 1: Append the cells**

```bash
python3 - <<'PY'
import json, secrets
from pathlib import Path

NB_PATH = Path("python/kermit-lab/notebooks/07_lubm_reference_comparison.ipynb")

def _cell(cell_type, text):
    cell = {
        "cell_type": cell_type,
        "id": secrets.token_hex(4),
        "metadata": {},
        "source": text.strip("\n").splitlines(keepends=True),
    }
    if cell_type == "code":
        cell["execution_count"] = None
        cell["outputs"] = []
    return cell

nb = json.loads(NB_PATH.read_text())

def md(text): nb["cells"].append(_cell("markdown", text))
def code(text): nb["cells"].append(_cell("code", text))

md('''
### T1 — How much faster, with uncertainty?

Per-query speedup ratios against the TreeTrie+LeapfrogTriejoin baseline
(`speedup > 1` means the target configuration is faster). `kl.compare`
pairs rows on every non-grouped column — including `algorithm` — so to
compare across configurations that differ in *both* structure and algorithm
we collapse `(data_structure, algorithm)` into one `config` column and drop
the originals plus the optimization-axis columns. The `speedup_lo/hi`
envelope is deliberately conservative (built from Criterion's stored CIs);
the bootstrap below tightens it for the headline query.
''')

code('''
ITER = df[(df["metric"] == "time") & (df["phase"] == "iteration")]

def as_configs(frame):
    """Collapse (data_structure, algorithm) into one `config` column so
    kl.compare can pair rows across configurations that differ in both."""
    drop = ["data_structure", "algorithm"] + kl.discover_opt_columns(frame)
    return (
        frame.assign(config=frame["data_structure"] + "+" + frame["algorithm"])
             .drop(columns=drop)
    )

BASELINE = "TreeTrie+LeapfrogTriejoin"
cmp_frame = as_configs(ITER)
tables = []
for target in ["ColumnTrie+LeapfrogTriejoin", "HashTrie+HashTriejoin"]:
    t = kl.compare(cmp_frame, baseline=BASELINE, target=target, group_by="config")
    t["target"] = target
    tables.append(t)
ratios = pd.concat(tables, ignore_index=True)
ratios["qnum"] = ratios["query"].str.extract(r"(\\d+)").astype(int)
ratios = ratios.sort_values(["qnum", "target"]).drop(columns="qnum")
ratios[["query", "target", "baseline_value", "target_value",
        "speedup", "speedup_lo", "speedup_hi"]].reset_index(drop=True)
''')

md('''
### Deep dive: q9

q9 is the most join-heavy of the 14 queries — a triangle between students,
their advisors, and courses (`?X advisor ?Y . ?Y teacherOf ?Z .
?X takesCourse ?Z`) plus three `rdf:type` constraints: six body atoms.
Structural differences between the tries should be most visible here.

First the raw per-iteration distributions (violins), then a percentile
bootstrap CI on the mean ratio, then a Mann-Whitney U test — three
independent ways of asking "is the q9 difference real or noise?".
''')

code('''
samples = kl.load_samples(REPORT_JSON, criterion_root="target/criterion")
kl.dist(df[df["query"] == "q9"], samples=samples);
''')

code('''
def q9_samples(ds, algo):
    keys = df[
        (df["metric"] == "time") & (df["phase"] == "iteration")
        & (df["query"] == "q9")
        & (df["data_structure"] == ds) & (df["algorithm"] == algo)
    ][["criterion_group", "criterion_function"]]
    merged = samples.merge(keys, on=["criterion_group", "criterion_function"])
    return merged["per_iter_ns"].to_numpy()

q9_tree = q9_samples("TreeTrie", "LeapfrogTriejoin")
q9_col  = q9_samples("ColumnTrie", "LeapfrogTriejoin")
q9_hash = q9_samples("HashTrie", "HashTriejoin")

for label, arr in [("TreeTrie+LFTJ", q9_tree), ("ColumnTrie+LFTJ", q9_col),
                   ("HashTrie+HTJ", q9_hash)]:
    print(f"{label:18s} n={len(arr):3d} mean={arr.mean()/1e6:8.3f} ms")

for label, other in [("ColumnTrie+LFTJ", q9_col), ("HashTrie+HTJ", q9_hash)]:
    lo, hi = kl.bootstrap_ratio_ci(q9_tree, other, rng=0)
    print(f"mean(TreeTrie)/mean({label}) 95% CI: [{lo:.3f}, {hi:.3f}]")
''')

code('''
pairs = [
    ("TreeTrie+LFTJ", q9_tree, "ColumnTrie+LFTJ", q9_col),
    ("TreeTrie+LFTJ", q9_tree, "HashTrie+HTJ", q9_hash),
    ("ColumnTrie+LFTJ", q9_col, "HashTrie+HTJ", q9_hash),
]
for name_a, a, name_b, b in pairs:
    u, p = kl.mannwhitney_u(a, b)
    print(f"{name_a:18s} vs {name_b:18s} (q9): U={u:8.0f}  p={p:.2e}")
''')

NB_PATH.write_text(json.dumps(nb, indent=1, ensure_ascii=False) + "\n")
print(f"{len(nb['cells'])} cells")
PY
```

Expected output: `30 cells`

- [ ] **Step 2: Validate the JSON**

Run: `python3 -c "import json; nb=json.load(open('python/kermit-lab/notebooks/07_lubm_reference_comparison.ipynb')); assert len(nb['cells'])==30; print('OK')"`

Expected: `OK`

- [ ] **Step 3: Commit**

```bash
git add python/kermit-lab/notebooks/07_lubm_reference_comparison.ipynb
git commit -m "feat(kermit-lab): reference notebook — statistical analysis"
```

---

### Task 8: Phase 4 — summary pivot (F6) and closing narrative

**Files:**
- Modify: `python/kermit-lab/notebooks/07_lubm_reference_comparison.ipynb` (append 3 cells)

- [ ] **Step 1: Append the cells**

```bash
python3 - <<'PY'
import json, secrets
from pathlib import Path

NB_PATH = Path("python/kermit-lab/notebooks/07_lubm_reference_comparison.ipynb")

def _cell(cell_type, text):
    cell = {
        "cell_type": cell_type,
        "id": secrets.token_hex(4),
        "metadata": {},
        "source": text.strip("\n").splitlines(keepends=True),
    }
    if cell_type == "code":
        cell["execution_count"] = None
        cell["outputs"] = []
    return cell

nb = json.loads(NB_PATH.read_text())

def md(text): nb["cells"].append(_cell("markdown", text))
def code(text): nb["cells"].append(_cell("code", text))

md('''
### F6 — One-glance overview

Mean iteration time in milliseconds, query × configuration.
''')

code('''
pivot = kl.summary(
    ITER.assign(config=ITER["data_structure"] + "+" + ITER["algorithm"]),
    rows="query", cols="config",
)
pivot.index = pd.CategoricalIndex(
    pivot.index,
    categories=sorted(pivot.index, key=lambda q: int(q[1:])),
    ordered=True,
)
(pivot.sort_index() / 1e6).round(3)
''')

md('''
## Reading the results

What each artefact answers:

- **F1 (per-query bars)** — which configuration answers each query fastest;
  log scale because the queries span orders of magnitude.
- **F2 (insertion bars)** — what building each index costs.
- **F3 (space bars)** — resident heap cost of each index.
- **F4 (tradeoff scatter)** — whether any configuration is Pareto-dominant
  (down and left is better).
- **T1 (ratio table)** — per-query speedups vs the TreeTrie baseline, with
  a conservative CI envelope.
- **F5 + bootstrap + T2 (q9)** — whether the headline difference is real:
  separated violins, a ratio CI excluding 1.0, and a small Mann-Whitney p.

Caveats to carry into any write-up:

- **Single scale.** LUBM(1, 0) only — the reference cardinalities that make
  this run *verifiable* do not transfer to other scales, so scaling claims
  need a different (separately verified) workload.
- **Single machine, wall-clock estimates.** Rerun with `PROFILE = "full"`
  for thesis-grade sample counts before quoting numbers.
- **Configuration-vs-configuration framing** (see intro): structure and
  algorithm change together for HashTrie.
- **Dictionary-encoded `usize` keys** — string handling is out of scope by
  design.

Component docs: `docs/data-structures/{tree-trie,column-trie,hash-trie}.md`,
`docs/algorithms/{leapfrog-triejoin,hash-triejoin}.md`, and
`docs/benchmarks/LUBM.md`.
''')

NB_PATH.write_text(json.dumps(nb, indent=1, ensure_ascii=False) + "\n")
print(f"{len(nb['cells'])} cells")
PY
```

Expected output: `33 cells`

- [ ] **Step 2: Validate the JSON**

Run: `python3 -c "import json; nb=json.load(open('python/kermit-lab/notebooks/07_lubm_reference_comparison.ipynb')); assert len(nb['cells'])==33; print('OK')"`

Expected: `OK`

- [ ] **Step 3: Commit**

```bash
git add python/kermit-lab/notebooks/07_lubm_reference_comparison.ipynb
git commit -m "feat(kermit-lab): reference notebook — summary and outro"
```

---

### Task 9: End-to-end verification (quick profile)

**Files:** none (verification only — `jupyter execute` does not write outputs back to the file, so the committed notebook stays output-free).

This validates the spec's acceptance criteria. The full run takes ~15–35 minutes on a cold cache (cargo release build + LUBM generation + cardinality test + 126 benchmark functions), which exceeds the 10-minute foreground command limit — run it in the background and poll.

- [ ] **Step 1: Execute the notebook end-to-end**

Run (in the background; from the repo root, inside `nix develop` or via `nix develop --command`):

```bash
cd python/kermit-lab && uv run --with jupyter jupyter execute --timeout=-1 notebooks/07_lubm_reference_comparison.ipynb
```

Expected: exits 0. If `jupyter execute` rejects `--timeout=-1`, drop the flag and instead confirm no cell hits the default timeout (nbclient's default is no timeout). If a `kermit_lab` API call fails (e.g. a preset signature drifted), fix the offending notebook cell via a `python3` heredoc that edits `nb["cells"][i]["source"]`, re-run, and amend the relevant task's commit message convention with a follow-up `fix(kermit-lab):` commit.

- [ ] **Step 2: Check the artefacts (acceptance criteria 1, 4, 6)**

```bash
ls bench-runs/lubm-reference-sweep-quick-*.json   # expect 3 per-config reports
ls target/criterion/ | grep -c "run_lubm-reference" # flattened group dirs: / → _
cargo run --release -- bench list 2>&1 | grep -A1 lubm-reference
```

Expected: three per-config report JSONs exist (`…-tree-trie-leapfrog-triejoin`, `…-column-trie-leapfrog-triejoin`, `…-hash-trie-hash-triejoin`); the criterion count covers 3 configurations × 14 queries (42 group directories); `bench list` shows `lubm-reference` as `cached`.

- [ ] **Step 3: Verify rerun behaviour (acceptance criterion 5)**

Re-run the Phase 2 sweep command alone (not the whole notebook) and confirm the streamed output shows a spec-hash cache hit (no jar invocation, no entailment stage) before benchmarking starts:

```bash
cargo run --release -- bench --sample-size 10 --measurement-time 1 --warm-up-time 1 \
  --report-json bench-runs/lubm-reference-rerun-check.json \
  run lubm-reference -i tree-trie -a leapfrog-triejoin --metrics iteration
rm bench-runs/lubm-reference-rerun-check.json
```

Expected: starts benchmarking within seconds of the build finishing — no generation stages in the output.

- [ ] **Step 4: Confirm the committed notebook is still output-free**

Run: `python3 -c "import json; nb=json.load(open('python/kermit-lab/notebooks/07_lubm_reference_comparison.ipynb')); assert not any(c.get('outputs') for c in nb['cells'] if c['cell_type']=='code'); print('clean')"`

Expected: `clean` (and `git status` shows no unstaged change to the notebook).

---

## Execution corrections (2026-06-11)

Code-quality review of the executed Tasks 2–8 reproduced a panic in the
planned sweep: `bench run lubm-reference -i all -a all` aborts at
`kermit/src/db.rs:313` on the first incompatible expanded pair and writes no
report (see the corrected key-fact bullet above). The Task 2/4/5/7 heredocs
in this document are preserved as the historical record of commits
`722b7ab..97396a9`; a follow-up `fix(kermit-lab):` commit replaced the
affected cells with: a `CONFIGS` list + `REPORT_GLOB` in the profile cell,
a per-configuration sweep loop, glob-based `kl.load`/`kl.load_samples`,
corrected Phase 2 / F4 markdown, explicit F5/T2 labels on the q9 deep dive,
and two citation/wording fixes. Task 9's verification steps above were
updated to match. The latent CLI panic itself is out of scope (spec: zero
Rust changes) and filed separately.

## Self-review notes

- Spec coverage: YAML (Task 1), Phases 0–4 (Tasks 2–8 map one-to-one to the spec's phase list), all six figures + two tables (Tasks 6–8: F1–F4, T1, F5, T2, F6), acceptance criteria (Task 9). Criterion 3 (cardinality verification passes) is exercised inside Task 9 step 1 since Phase 1 is a notebook cell.
- The spec's F5/T2 query is q9, justified in the notebook markdown as required ("named and justified").
- `pd` is imported in the Phase 2 expected-cardinality cell and used again in Tasks 7–8 (`pd.concat`, `pd.CategoricalIndex`); `ITER` is defined in Task 7's T1 cell and reused in Task 8's F6 cell — execution order is linear, so this is safe.
