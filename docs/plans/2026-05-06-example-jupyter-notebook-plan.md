# Example Jupyter Notebook (Full Benchmarking Timeline) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a self-executing Jupyter notebook at `python/kermit-lab/notebooks/00_full_timeline.ipynb` that walks a reader from `bench gen lubm` through plot rendering, plus the two README touch-ups that make it discoverable.

**Architecture:** The notebook is constructed programmatically using the `nbformat` library (already a transitive dependency of jupyter via `kermit-lab`'s lockfile). Cells are added incrementally in phase-grouped tasks; each task is verified by running `jupyter nbconvert --execute` headlessly to catch broken cells before commit. No new Rust code, no new `kermit_lab` features — the notebook only consumes existing `bench gen lubm`, `bench run`, and `kl.*` plotting helpers.

**Tech Stack:** Python 3.13, `nbformat` (notebook authoring), `jupyter nbconvert` (headless execution), `subprocess.run` (kermit invocation), `kermit_lab` (plotting), pandas/matplotlib (transitive). Rust nightly + cargo for the kermit binary the notebook will exec.

**Reference:** spec at `docs/specs/2026-05-05-example-jupyter-notebook-design.md`.

---

## File Structure

### Files to Create

```
python/kermit-lab/notebooks/00_full_timeline.ipynb   # the notebook (15 cells)
```

### Files to Modify

```
python/kermit-lab/README.md                          # bullet-list bump (required)
README.md                                            # one-line pointer (optional)
```

### Files Touched at Runtime (Not Edited)

These appear when the notebook executes. The plan does not commit them; they belong in `bench-runs/` (gitignored) and the platform cache.

```
~/.cache/kermit/benchmarks/lubm-1-nb-fulltimeline/   # generator cache
bench-runs/lubm-demo-time.json                       # gitignored
bench-runs/lubm-demo-space.json                      # gitignored
target/criterion/                                    # gitignored
```

---

## Cell Inventory

The notebook has 15 cells across 7 phases. Tasks 1–6 add them incrementally; this section is the canonical reference.

| Cell | Type | Phase | Owner Task |
|---|---|---|---|
| 0.1 | markdown | Title + prereqs | Task 1 |
| 0.2 | code | CWD + prereq check + helper definition | Task 1 |
| 1.1 | markdown | Phase 1 narrative | Task 2 |
| 1.2 | code | `bench gen lubm` invocation | Task 2 |
| 2.1 | markdown | Phase 2 narrative | Task 3 |
| 2.2 | code | `bench run … (time)` | Task 3 |
| 3.1 | markdown | Phase 3 narrative | Task 3 |
| 3.2 | code | `bench run … --metrics space` | Task 3 |
| 4.1 | markdown | Phase 4 narrative | Task 4 |
| 4.2 | code | `kl.load(...)` + `df.head()` | Task 4 |
| 5.1 | markdown | Phase 5 narrative | Task 5 |
| 5.2 | code | `kl.bar_queries(...)` | Task 5 |
| 5.3 | code | `pick_query(df)` + `kl.bar_time(...)` | Task 5 |
| 5.4 | code | `kl.bar_space(...)` | Task 5 |
| 5.5 | code | `kl.tradeoff(...)` | Task 5 |
| 6.1 | markdown | Where to go next | Task 5 |

---

## Build approach

Each notebook-building task uses a heredoc-style `python` invocation that reads the existing `.ipynb`, appends cells via `nbformat.v4`, and writes it back. All Python invocations run via `uv run --directory python/kermit-lab python ...` so they pick up the lockfile-managed `nbformat` install.

**Why nbformat instead of raw JSON authoring:** `nbformat` produces canonical, validator-passing JSON every time. Hand-written `.ipynb` JSON tends to drift on schema-version fields and execution-count nulls. The library is already in the kermit-lab venv (transitively via jupyter).

**Why `jupyter nbconvert --execute` between tasks:** notebooks fail silently if a syntax error lurks in a cell that nobody clicked. Running the notebook headlessly after each task catches that immediately.

---

## Task 1: Scaffold the notebook with Phase 0 (prereqs and CWD)

**Files:**
- Create: `python/kermit-lab/notebooks/00_full_timeline.ipynb`
- Test: run `jupyter nbconvert --execute` on the newly-built notebook

- [ ] **Step 1: Confirm `nbformat` is available in the kermit-lab venv**

Run:

```bash
cd /home/aidanb/Source/UCT/kermit/.loom/worktrees/aidanb/example-jupyter-notebook_18acb35cb8ad6e5a/python/kermit-lab
uv run python -c "import nbformat; print(nbformat.__version__)"
```

Expected: a version string (e.g., `5.10.4`). If it errors with `ModuleNotFoundError`, run `uv sync` and retry.

- [ ] **Step 2: Create the notebook with cells 0.1 (markdown) and 0.2 (code)**

Run from the repo root:

```bash
uv run --directory python/kermit-lab python <<'PYEOF'
import nbformat
from nbformat.v4 import new_notebook, new_markdown_cell, new_code_cell

cell_0_1 = """# Full benchmarking timeline (entry-point notebook)

This notebook walks you through the entire kermit benchmarking pipeline end-to-end:

1. **Generate** a LUBM-1 dataset on demand (`bench gen lubm`).
2. **Run** the time and space benchmarks across both index structures.
3. **Load** the resulting reports into a pandas DataFrame.
4. **Plot** four inline `matplotlib` figures.

Run the cells top-to-bottom (`Run All`) to reproduce. The first invocation triggers a full release build (~3-5 min); subsequent runs short-circuit on the spec-hash cache.

## Prerequisites

1. **Inside `nix develop`** if on NixOS — sets `LD_LIBRARY_PATH` for `libstdc++` and `libz` so numpy/matplotlib wheels load.
2. **JDK 8 on PATH** — the flake provides `pkgs.jdk8`; on Ubuntu, `apt install openjdk-8-jre`.
3. **Kermit-lab venv populated**: from the repo root, `cd python/kermit-lab && uv sync` (one-time).
4. **Launch this notebook** with: `cd python/kermit-lab && uv run --with jupyterlab jupyter lab --notebook-dir=..`, then open `notebooks/00_full_timeline.ipynb`.

This notebook is the entry point. The other five notebooks under `notebooks/` (`01_quick_start`, `02_scaling`, ...) are topical deep-dives that assume `bench-runs/` and `target/criterion/` are already populated by an earlier run."""

cell_0_2 = '''import os, subprocess, shutil, time
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
# error 30 seconds into phase 1.
assert shutil.which("cargo"), "cargo not on PATH — run from inside `nix develop`"
assert shutil.which("java"),  "java not on PATH — needed by `bench gen lubm`"
print("cargo:", shutil.which("cargo"))
print("java: ", shutil.which("java"))


def run_phase(args, label):
    """Run a kermit subprocess with timing, friendly output, and stderr capture on failure.

    Used by phases 1, 2, and 3 — each invokes `cargo run --release -- bench …`
    and may take several minutes on a cold build. We capture stdout/stderr so a
    long-running cell doesn\\'t spam the notebook with raw cargo log output, but
    on failure we surface the last 40 lines of stderr before re-raising.
    """
    print(f"\\u23f3 {label} - this may take several minutes (cold cargo build + dataset generation)...")
    start = time.monotonic()
    try:
        result = subprocess.run(args, check=True, capture_output=True, text=True)
    except subprocess.CalledProcessError as e:
        print("--- STDERR (last 40 lines) ---")
        for line in e.stderr.splitlines()[-40:]:
            print(line)
        raise
    elapsed = time.monotonic() - start
    print(f"\\u2705 {label} - done in {elapsed:.1f}s")
    return result
'''

nb = new_notebook()
nb.cells.append(new_markdown_cell(cell_0_1))
nb.cells.append(new_code_cell(cell_0_2))

# Set kernelspec so jupyter knows which kernel to use; matches the kermit-lab venv.
nb.metadata["kernelspec"] = {
    "display_name": "Python 3",
    "language": "python",
    "name": "python3",
}
nb.metadata["language_info"] = {"name": "python"}

out = Path("python/kermit-lab/notebooks/00_full_timeline.ipynb")
out.parent.mkdir(parents=True, exist_ok=True)
nbformat.write(nb, str(out))
print(f"Wrote {out} ({len(nb.cells)} cells)")
PYEOF
```

Expected: `Wrote python/kermit-lab/notebooks/00_full_timeline.ipynb (2 cells)`

- [ ] **Step 3: Validate the notebook is well-formed JSON**

Run:

```bash
uv run --directory python/kermit-lab python -c "import nbformat; nb = nbformat.read('python/kermit-lab/notebooks/00_full_timeline.ipynb', as_version=4); nbformat.validate(nb); print('valid')"
```

Expected: `valid`

- [ ] **Step 4: Headless-execute Phase 0 to confirm cells run**

From the repo root, with `nix develop` active and `cargo`/`java` on PATH:

```bash
uv run --directory python/kermit-lab jupyter nbconvert \
  --to notebook --execute \
  --output 00_full_timeline.exec.ipynb \
  ../../../python/kermit-lab/notebooks/00_full_timeline.ipynb
```

Wait — that path is wrong because `--directory` shifts CWD. Use this instead:

```bash
cd python/kermit-lab && uv run jupyter nbconvert \
  --to notebook --execute \
  --output 00_full_timeline.exec.ipynb \
  notebooks/00_full_timeline.ipynb && cd ../..
```

Expected: a new file `python/kermit-lab/notebooks/00_full_timeline.exec.ipynb`. If execution failed, the command exits non-zero and the partial file is left behind.

- [ ] **Step 5: Inspect that cell 0.2 produced sensible output**

```bash
uv run --directory python/kermit-lab python -c "
import nbformat
nb = nbformat.read('python/kermit-lab/notebooks/00_full_timeline.exec.ipynb', as_version=4)
for cell in nb.cells:
    if cell.cell_type == 'code':
        for out in cell.outputs:
            if out.output_type == 'stream':
                print(out.text)
"
```

Expected output contains lines beginning with `CWD: `, `cargo: `, `java: `.

- [ ] **Step 6: Delete the executed copy (it's a verification artefact, not committed)**

```bash
rm python/kermit-lab/notebooks/00_full_timeline.exec.ipynb
```

- [ ] **Step 7: Commit**

```bash
git add python/kermit-lab/notebooks/00_full_timeline.ipynb
git commit -m "$(cat <<'EOF'
feat(kermit-lab): scaffold full-timeline notebook with prereq cells

Adds 00_full_timeline.ipynb with Phase 0 (CWD discovery + cargo/java
prereq assertions + run_phase helper for subprocess invocation). The
notebook is the new entry point for python/kermit-lab/notebooks/ and
will grow through subsequent commits to cover bench gen, bench run,
DataFrame loading, and plotting. See
docs/specs/2026-05-05-example-jupyter-notebook-design.md.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

Expected: a single new file in the commit.

---

## Task 2: Add Phase 1 (generate the LUBM-1 dataset)

**Files:**
- Modify: `python/kermit-lab/notebooks/00_full_timeline.ipynb`
- Test: headless-execute and confirm `~/.cache/kermit/benchmarks/lubm-1-nb-fulltimeline/meta.json` exists.

- [ ] **Step 1: Append cells 1.1 and 1.2 via nbformat**

```bash
uv run --directory python/kermit-lab python <<'PYEOF'
import nbformat
from nbformat.v4 import new_markdown_cell, new_code_cell

cell_1_1 = """## Phase 1 — Generate the LUBM-1 dataset

The declarative generator pipeline (`bench gen lubm`) downloads the LUBM Univ-Bench TBox, invokes the committed `lubm-uba.jar` to materialise data for one university, applies forward-chaining entailment, and partitions the resulting N-Triples into per-predicate Parquet files.

The output lands in the platform cache (Linux: `~/.cache/kermit/benchmarks/lubm-1-nb-fulltimeline/`). The cache is keyed by spec hash — re-running with the same `--scale` and `--tag` short-circuits without redoing the work.

The tag `nb-fulltimeline` is private to this notebook so it does not collide with any tag you've used in your own workflow."""

cell_1_2 = '''run_phase(
    ["cargo", "run", "--release", "--",
     "bench", "gen", "lubm",
     "--scale", "1",
     "--tag", "nb-fulltimeline"],
    "Phase 1: generate LUBM-1 dataset",
)
'''

nb = nbformat.read("python/kermit-lab/notebooks/00_full_timeline.ipynb", as_version=4)
nb.cells.append(new_markdown_cell(cell_1_1))
nb.cells.append(new_code_cell(cell_1_2))
nbformat.write(nb, "python/kermit-lab/notebooks/00_full_timeline.ipynb")
print(f"{len(nb.cells)} cells now")
PYEOF
```

Expected: `4 cells now`

- [ ] **Step 2: Headless-execute the notebook (warning: this builds and generates — first run is multi-minute)**

```bash
cd python/kermit-lab && uv run jupyter nbconvert \
  --to notebook --execute \
  --ExecutePreprocessor.timeout=900 \
  --output 00_full_timeline.exec.ipynb \
  notebooks/00_full_timeline.ipynb && cd ../..
```

The 15-minute timeout accommodates a cold cargo release build plus dataset generation. Expected: command exits 0; the cache directory is populated.

- [ ] **Step 3: Confirm the cache directory exists with a meta.json**

```bash
ls ~/.cache/kermit/benchmarks/lubm-1-nb-fulltimeline/meta.json
```

Expected: the path resolves (no error). If on a different OS, the path differs — see `kermit-bench` `discovery.rs` for the platform-specific resolution.

- [ ] **Step 4: Delete the executed copy and commit**

```bash
rm python/kermit-lab/notebooks/00_full_timeline.exec.ipynb
git add python/kermit-lab/notebooks/00_full_timeline.ipynb
git commit -m "$(cat <<'EOF'
feat(kermit-lab): notebook phase 1 — bench gen lubm

Adds the LUBM-1 generation cells. Uses --tag nb-fulltimeline so the
cache subdir (lubm-1-nb-fulltimeline) is private to this notebook and
does not collide with users' own ad-hoc tags.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 3: Add Phases 2 and 3 (bench run with time and space metrics)

**Files:**
- Modify: `python/kermit-lab/notebooks/00_full_timeline.ipynb`
- Test: confirm `bench-runs/lubm-demo-time.json` and `bench-runs/lubm-demo-space.json` both exist.

- [ ] **Step 1: Append cells 2.1, 2.2, 3.1, 3.2 via nbformat**

```bash
uv run --directory python/kermit-lab python <<'PYEOF'
import nbformat
from nbformat.v4 import new_markdown_cell, new_code_cell

cell_2_1 = """## Phase 2 — Run the benchmark (time metric)

`bench run` reads the cached benchmark, builds both index structures (`TreeTrie`, `ColumnTrie`) for each LUBM query, and times the LeapfrogTriejoin algorithm using Criterion. The results land in:

- `bench-runs/lubm-demo-time.json` — the machine-readable `BenchReport` (one entry per query × DS × algo).
- `target/criterion/<group>/<dir>/{base,new}/` — the per-iter Criterion artefacts that `kl.load` will read for plotting.

`-i all` expands to the full `IndexStructure` enum; `-a leapfrog-triejoin` pins the algorithm."""

cell_2_2 = '''run_phase(
    ["cargo", "run", "--release", "--",
     "bench", "run", "lubm-1-nb-fulltimeline",
     "-i", "all",
     "-a", "leapfrog-triejoin",
     "--report-json", "bench-runs/lubm-demo-time.json"],
    "Phase 2: bench run (time)",
)
'''

cell_3_1 = """## Phase 3 — Run the benchmark (space metric)

`--metrics space` swaps Criterion's default time `Measurement` for a custom `SpaceMeasurement` that records `heap_size_bytes()` per iter against the pre-built relation. Because Criterion only supports one `Measurement` per invocation, this must be a separate `bench run` from phase 2.

The reports land in `bench-runs/lubm-demo-space.json` alongside their time-metric counterparts. `kl.load` will discover both via the glob `bench-runs/lubm-demo-*.json` and merge them into a single tidy DataFrame."""

cell_3_2 = '''run_phase(
    ["cargo", "run", "--release", "--",
     "bench", "run", "lubm-1-nb-fulltimeline",
     "-i", "all",
     "-a", "leapfrog-triejoin",
     "--metrics", "space",
     "--report-json", "bench-runs/lubm-demo-space.json"],
    "Phase 3: bench run (space)",
)
'''

nb = nbformat.read("python/kermit-lab/notebooks/00_full_timeline.ipynb", as_version=4)
nb.cells.extend([
    new_markdown_cell(cell_2_1),
    new_code_cell(cell_2_2),
    new_markdown_cell(cell_3_1),
    new_code_cell(cell_3_2),
])
nbformat.write(nb, "python/kermit-lab/notebooks/00_full_timeline.ipynb")
print(f"{len(nb.cells)} cells now")
PYEOF
```

Expected: `8 cells now`

- [ ] **Step 2: Headless-execute the notebook**

```bash
cd python/kermit-lab && uv run jupyter nbconvert \
  --to notebook --execute \
  --ExecutePreprocessor.timeout=1800 \
  --output 00_full_timeline.exec.ipynb \
  notebooks/00_full_timeline.ipynb && cd ../..
```

The 30-minute timeout accommodates two `bench run` invocations on top of the (already-built) cargo release binary. Expected: exit 0.

- [ ] **Step 3: Confirm both report files exist and are non-empty JSON arrays**

```bash
ls -la bench-runs/lubm-demo-time.json bench-runs/lubm-demo-space.json
uv run --directory python/kermit-lab python -c "
import json
for p in ['bench-runs/lubm-demo-time.json', 'bench-runs/lubm-demo-space.json']:
    with open('../../' + p) as f:
        data = json.load(f)
    assert isinstance(data, list) and len(data) > 0, f'{p} is empty or not a list'
    print(f'{p}: {len(data)} report entries')
"
```

Expected: both files exist; both contain non-empty JSON arrays. The exact entry counts depend on how many queries the LUBM example YAML enables — at least 2.

- [ ] **Step 4: Delete the executed copy and commit**

```bash
rm python/kermit-lab/notebooks/00_full_timeline.exec.ipynb
git add python/kermit-lab/notebooks/00_full_timeline.ipynb
git commit -m "$(cat <<'EOF'
feat(kermit-lab): notebook phases 2-3 — bench run time and space

Adds the two bench run invocations: one with the default time
Measurement, one with --metrics space. Both write reports to
bench-runs/lubm-demo-{time,space}.json and Criterion artefacts to
target/criterion/, which kl.load will merge.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 4: Add Phase 4 (load reports into a DataFrame)

**Files:**
- Modify: `python/kermit-lab/notebooks/00_full_timeline.ipynb`
- Test: cell 4.2 prints a non-empty DataFrame head with the expected columns.

- [ ] **Step 1: Append cells 4.1 and 4.2 via nbformat**

```bash
uv run --directory python/kermit-lab python <<'PYEOF'
import nbformat
from nbformat.v4 import new_markdown_cell, new_code_cell

cell_4_1 = """## Phase 4 — Load reports into a pandas DataFrame

`kl.load(glob, criterion_root=…)` parses every `BenchReport` matching the glob, joins it against the Criterion JSON artefacts under `criterion_root/<group>/<dir>/`, and returns a tidy long-form DataFrame: one row per `(query, data_structure, algorithm, phase, sample_index)`.

Column reference: see [`kermit_lab/frame.py`](../kermit_lab/frame.py) for the include-list and [`docs/specs/bench-report-schema.md`](../../../docs/specs/bench-report-schema.md) for the JSON schema."""

cell_4_2 = '''import kermit_lab as kl

kl.apply_style()  # apply once per notebook session
df = kl.load("bench-runs/lubm-demo-*.json", criterion_root="target/criterion")
print(f"{len(df)} rows, {len(df.columns)} columns")
df.head()
'''

nb = nbformat.read("python/kermit-lab/notebooks/00_full_timeline.ipynb", as_version=4)
nb.cells.extend([
    new_markdown_cell(cell_4_1),
    new_code_cell(cell_4_2),
])
nbformat.write(nb, "python/kermit-lab/notebooks/00_full_timeline.ipynb")
print(f"{len(nb.cells)} cells now")
PYEOF
```

Expected: `10 cells now`

- [ ] **Step 2: Headless-execute the notebook**

```bash
cd python/kermit-lab && uv run jupyter nbconvert \
  --to notebook --execute \
  --ExecutePreprocessor.timeout=1800 \
  --output 00_full_timeline.exec.ipynb \
  notebooks/00_full_timeline.ipynb && cd ../..
```

Expected: exit 0. Phase 1 short-circuits on cache hit; phases 2 and 3 re-run (Criterion overwrites previous results — that's fine).

- [ ] **Step 3: Confirm cell 4.2 produced a DataFrame head**

```bash
uv run --directory python/kermit-lab python <<'PYEOF'
import nbformat
nb = nbformat.read("python/kermit-lab/notebooks/00_full_timeline.exec.ipynb", as_version=4)
cell_4_2 = nb.cells[9]  # 0-indexed: 0.1, 0.2, 1.1, 1.2, 2.1, 2.2, 3.1, 3.2, 4.1, 4.2
assert cell_4_2.cell_type == "code"
assert any(out.output_type in ("execute_result", "display_data") for out in cell_4_2.outputs), \
    "cell 4.2 has no DataFrame display output"
# Sniff the streamed output for the row/column count line
streams = [out.text for out in cell_4_2.outputs if out.output_type == "stream"]
print("".join(streams))
PYEOF
```

Expected: a line like `N rows, M columns` where N>0 and M>=8.

- [ ] **Step 4: Delete the executed copy and commit**

```bash
rm python/kermit-lab/notebooks/00_full_timeline.exec.ipynb
git add python/kermit-lab/notebooks/00_full_timeline.ipynb
git commit -m "$(cat <<'EOF'
feat(kermit-lab): notebook phase 4 — load reports into DataFrame

Imports kermit_lab, applies the Wong/Okabe-Ito palette, loads the two
report files via the lubm-demo-*.json glob, and prints a head() so the
reader can confirm the load worked before any plotting.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 5: Add Phases 5 (four plots) and 6 (where to go next)

**Files:**
- Modify: `python/kermit-lab/notebooks/00_full_timeline.ipynb`
- Test: confirm 4 figures rendered as `display_data` outputs.

- [ ] **Step 1: Append cells 5.1, 5.2, 5.3, 5.4, 5.5, 6.1 via nbformat**

```bash
uv run --directory python/kermit-lab python <<'PYEOF'
import nbformat
from nbformat.v4 import new_markdown_cell, new_code_cell

cell_5_1 = """## Phase 5 — Plots

Four inline figures, each answering one question:

- **`bar_queries`** — for a chosen `(DS, algo)`, how do the LUBM queries compare in mean time?
- **`bar_time`** — for one query, how do the index structures compare in mean time? (Pick a non-trivial query programmatically.)
- **`bar_space`** — what's the heap footprint of each index structure?
- **`tradeoff`** — space-vs-time scatter (one point per `(DS, query)`).

Every plot returns a `matplotlib.figure.Figure` for inline display."""

cell_5_2 = '''kl.bar_queries(df, ds="TreeTrie", algo="LeapfrogTriejoin")
'''

cell_5_3 = '''import re

def pick_query(queries):
    """Pick the lowest-numbered LUBM query that isn\\'t Q1 (which is trivial).

    Natural numeric sort (not lexicographic) so q2 wins over q10. Falls back
    to the alphabetic minimum if no query name contains an integer.
    """
    def key(q):
        m = re.search(r"\\d+", q)
        return int(m.group()) if m else -1
    candidates = [q for q in queries if key(q) != 1]
    if not candidates:
        return sorted(queries)[0]
    return sorted(candidates, key=key)[0]


q = pick_query(df["query"].unique())
print(f"Plotting bar_time for query: {q}")
kl.bar_time(df, query=q)
'''

cell_5_4 = '''kl.bar_space(df)
'''

cell_5_5 = '''kl.tradeoff(df)
'''

cell_6_1 = """## Where to go next

You now have working `bench-runs/` and `target/criterion/` artefacts. The other five notebooks pick up from here:

- **`01_quick_start.ipynb`** — terser version of phases 4-5 alone.
- **`02_scaling.ipynb`** — log-log scaling plot across scales (try regenerating with `--scale 2` and re-running).
- **`03_compare_ds.ipynb`** — pairwise DS comparison with `kl.compare()` and bootstrap CI.
- **`04_watdiv_queries.ipynb`** — multi-query bars across WatDiv stress runs.
- **`05_distributions.ipynb`** — violin plot from raw samples + Mann-Whitney U test.

To explore other generator backends, try `bench gen watdiv --scale 100 --tag demo` (requires a locally-built watdiv binary; see `docs/benchmarks/WATDIV.md`).

To compare DS pairs statistically, use `kl.compare(df, baseline="TreeTrie", target="ColumnTrie")` and `kl.bootstrap_ratio_ci(...)`.

**Caveat:** if you `cargo clean` between phases 3 and 4, the Criterion artefacts vanish and `kl.load` will error with a clear "no such directory" message. Re-run phases 2 and 3."""

nb = nbformat.read("python/kermit-lab/notebooks/00_full_timeline.ipynb", as_version=4)
nb.cells.extend([
    new_markdown_cell(cell_5_1),
    new_code_cell(cell_5_2),
    new_code_cell(cell_5_3),
    new_code_cell(cell_5_4),
    new_code_cell(cell_5_5),
    new_markdown_cell(cell_6_1),
])
nbformat.write(nb, "python/kermit-lab/notebooks/00_full_timeline.ipynb")
print(f"{len(nb.cells)} cells now")
PYEOF
```

Expected: `16 cells now`

Cell 5.3 is a single code cell that contains both the `pick_query` helper definition and the `kl.bar_time(...)` call — keeping them together makes the helper's purpose obvious at the call site without adding a separate cell.

- [ ] **Step 2: Headless-execute the notebook**

```bash
cd python/kermit-lab && uv run jupyter nbconvert \
  --to notebook --execute \
  --ExecutePreprocessor.timeout=1800 \
  --output 00_full_timeline.exec.ipynb \
  notebooks/00_full_timeline.ipynb && cd ../..
```

Expected: exit 0.

- [ ] **Step 3: Confirm 4 figure outputs in the four plot cells**

```bash
uv run --directory python/kermit-lab python <<'PYEOF'
import nbformat
nb = nbformat.read("python/kermit-lab/notebooks/00_full_timeline.exec.ipynb", as_version=4)
plot_cell_indices = [11, 12, 13, 14]  # 5.2, 5.3, 5.4, 5.5
for i in plot_cell_indices:
    cell = nb.cells[i]
    has_figure = any(
        out.output_type == "display_data" and "image/png" in out.data
        for out in cell.outputs
    )
    print(f"cell {i}: figure_rendered={has_figure}")
    assert has_figure, f"cell {i} did not render a figure"
print("all 4 plots rendered")
PYEOF
```

Expected: all four cells report `figure_rendered=True` and the script prints `all 4 plots rendered`.

- [ ] **Step 4: Delete the executed copy and commit**

```bash
rm python/kermit-lab/notebooks/00_full_timeline.exec.ipynb
git add python/kermit-lab/notebooks/00_full_timeline.ipynb
git commit -m "$(cat <<'EOF'
feat(kermit-lab): notebook phases 5-6 — plots + outro

Adds the four inline figures (bar_queries, bar_time, bar_space,
tradeoff). bar_time uses a pick_query helper that selects the lowest-
numbered non-trivial LUBM query in natural numeric order (so q2 wins
over q10 lexicographically). Phase 6 points at the other five
notebooks for follow-up.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 6: Update `python/kermit-lab/README.md`

**Files:**
- Modify: `python/kermit-lab/README.md` (lines around 51–58)

- [ ] **Step 1: Read the current bullet list**

Run:

```bash
sed -n '48,60p' python/kermit-lab/README.md
```

Expected output approximately:

```
Five worked examples ship in [`notebooks/`](notebooks/):

- `01_quick_start.ipynb` — load a DataFrame and plot inline.
- `02_scaling.ipynb` — scaling plot + pivot of mean times by DS × scale.
- `03_compare_ds.ipynb` — `compare()` envelope + `bootstrap_ratio_ci()`.
- `04_watdiv_queries.ipynb` — multi-query bars across WatDiv stress runs.
- `05_distributions.ipynb` — violin plot from samples + Mann-Whitney U test.
```

- [ ] **Step 2: Apply the edit**

Use the Edit tool (or `sed -i`) to replace:

```
Five worked examples ship in [`notebooks/`](notebooks/):

- `01_quick_start.ipynb` — load a DataFrame and plot inline.
```

with:

```
Six worked examples ship in [`notebooks/`](notebooks/):

- `00_full_timeline.ipynb` — generate a LUBM-1 dataset, run the bench sweep, then load and plot. **Start here.**
- `01_quick_start.ipynb` — load a DataFrame and plot inline.
```

- [ ] **Step 3: Verify the edit**

Run:

```bash
sed -n '48,62p' python/kermit-lab/README.md
```

Expected: the bullet list now begins with `00_full_timeline.ipynb` and the heading reads "Six worked examples".

- [ ] **Step 4: Commit**

```bash
git add python/kermit-lab/README.md
git commit -m "$(cat <<'EOF'
docs(kermit-lab): point at 00_full_timeline.ipynb as the entry point

Bumps the bullet list from five to six notebooks. The new entry-point
notebook generates its own data via bench gen lubm, so it is the
correct starting point for a reader who has never run kermit before.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 7: Update root `README.md` (optional one-liner)

**Files:**
- Modify: `README.md` near line 133 (the existing `python/kermit-lab/` mention).

- [ ] **Step 1: Read the surrounding context**

```bash
sed -n '130,148p' README.md
```

Expected: a paragraph describing how Criterion's HTML output is disabled and benchmark exploration happens through `python/kermit-lab/`.

- [ ] **Step 2: Append the entry-point pointer**

Use the Edit tool to perform this exact replacement. Find:

```
The CLI is preserved as a thin wrapper.
```

Replace with:

```
The CLI is preserved as a thin wrapper. Start with [`python/kermit-lab/notebooks/00_full_timeline.ipynb`](python/kermit-lab/notebooks/00_full_timeline.ipynb) for an end-to-end walkthrough that generates its own data, runs the bench sweep, and produces inline plots.
```

This appends one sentence to the same paragraph, immediately after the existing summary sentence and before the next paragraph.

- [ ] **Step 3: Verify**

```bash
grep -n "00_full_timeline" README.md
```

Expected: at least one match.

- [ ] **Step 4: Commit**

```bash
git add README.md
git commit -m "$(cat <<'EOF'
docs: point root readme at the full-timeline notebook

Adds a one-line pointer to 00_full_timeline.ipynb in the kermit-lab
section so readers landing on the root README can find the
end-to-end walkthrough.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 8: Final headless verification (clean run on a populated cache)

**Files:**
- No changes; verification only.

- [ ] **Step 1: Re-run the notebook headlessly to confirm a clean second pass**

```bash
cd python/kermit-lab && uv run jupyter nbconvert \
  --to notebook --execute \
  --ExecutePreprocessor.timeout=1800 \
  --output 00_full_timeline.exec.ipynb \
  notebooks/00_full_timeline.ipynb && cd ../..
```

This time the cargo release binary is already built and the spec-hash cache is hot, so phase 1 should short-circuit (no JVM activity, no network), phases 2 and 3 should re-time the bench, and phases 4 and 5 should plot.

Expected: exit 0; total wall-clock under ~5 minutes (no cargo build, no LUBM generation).

- [ ] **Step 2: Confirm the notebook is structurally valid one last time**

```bash
uv run --directory python/kermit-lab python -c "
import nbformat
nb = nbformat.read('python/kermit-lab/notebooks/00_full_timeline.ipynb', as_version=4)
nbformat.validate(nb)
print(f'{len(nb.cells)} cells, valid')
"
```

Expected: `16 cells, valid`.

- [ ] **Step 3: Delete the executed copy**

```bash
rm python/kermit-lab/notebooks/00_full_timeline.exec.ipynb
```

- [ ] **Step 4: No commit needed** — this task is verification-only. If anything in steps 1–3 failed, surface it as a follow-up task and patch before completing the plan.

---

## Risks and known gotchas

These come from the spec (`docs/specs/2026-05-05-example-jupyter-notebook-design.md` § Risks) and CLAUDE.md gotchas. They do not need their own tasks — they are documentation for the implementer.

- **First `cargo run --release` is multi-minute.** Tasks 1–4 use 15-minute timeouts; tasks 5 and 8 use 30-minute timeouts.
- **NixOS `LD_LIBRARY_PATH`.** All Python invocations must run from inside `nix develop`, otherwise numpy/matplotlib wheel imports fail. The launch instructions in cell 0.1 cover this; the implementer must remember it during plan execution.
- **`bench gen lubm` writes to `~/.cache/kermit/`** — outside the repo. The cache survives `cargo clean` but not a `rm -rf ~/.cache/kermit/`. If verification step 3 of Task 2 fails, check the cache directory before assuming the notebook is broken.
- **`cargo clean` between phase 3 and phase 4** invalidates `target/criterion/` and `kl.load` will fail with a clear "no such directory" error. Mentioned in cell 6.1 markdown.
- **Notebook outputs**: every `nbconvert --execute` step writes outputs to a `.exec.ipynb` copy that is deleted before commit. The committed `.ipynb` should remain output-free. If outputs accidentally land in the committed file, run `jupyter nbconvert --clear-output --inplace notebooks/00_full_timeline.ipynb` before re-committing.
