# Example Jupyter notebook — full benchmarking timeline

**Date:** 2026-05-05
**Status:** Draft (awaiting user review)
**Owner:** aidanb

## Summary

Add a single self-executing Jupyter notebook,
`python/kermit-lab/notebooks/00_full_timeline.ipynb`, that walks a reader through
the entire kermit benchmarking pipeline end-to-end: generating a LUBM-1
dataset on demand, running the time and space benchmarks across both index
structures, loading the resulting reports into pandas, and producing four
inline `matplotlib` figures. The notebook is the new entry point for the
`python/kermit-lab/notebooks/` directory and supersedes
`01_quick_start.ipynb` as the recommended first read; the existing five
notebooks remain as topical deep-dives.

## Motivation

The five existing kermit-lab notebooks all assume `bench-runs/` and
`target/criterion/` are already populated. A new contributor — or a thesis
reader — has no end-to-end document that connects "I want to benchmark
something" to "I see plots". The recent declarative-generator work (`bench
gen lubm`, `bench gen watdiv`, spec-hash caching) is also undemonstrated in
notebook form. This notebook closes that gap.

## Non-goals

To prevent scope creep:

- No new `kermit_lab` features — the notebook only consumes the existing
  public API (`kl.load`, `kl.apply_style`, `kl.bar_queries`, `kl.bar_time`,
  `kl.bar_space`, `kl.tradeoff`).
- No CI integration. Executing the notebook in CI via `nbmake` or papermill
  would require JDK 8 in runners and a multi-minute build budget; that is a
  separate decision.
- No WatDiv path. We deliberately chose LUBM because the jar is committed.
  The `**/watdiv` gitignore rule for the platform-specific binary stays
  unchanged.
- No new schema fields, no new CLI flags, no new Rust code. Notebook uses
  only what `bench gen lubm`, `bench run`, and `BenchReport` v2 already
  provide.
- No macOS / Windows validation. Linux + JDK 8 is the supported surface.

## Architectural decisions

Five forks were resolved during brainstorming. Each was chosen to align
with infrastructure that already exists, minimising new code.

| Question | Decision | Rationale |
|---|---|---|
| Where does the timeline begin? | Generator-driven (no pre-existing data) | Showcases the most distinctive recent feature (`bench gen` declarative pipeline) and genuinely spans the full timeline. |
| Self-executing or read-only? | Self-executing (`subprocess.run`, not `!cargo`) | "Run All" must reproduce the plots end-to-end; bang-syntax does not propagate non-zero exits. |
| Which generator + benchmark? | LUBM, scale 1 | The jar at `kermit-rdf/vendor/lubm-uba/lubm-uba.jar` is committed — no local build step. 14 reference queries with known cardinalities give a richer plot story. WatDiv would require a locally-built, gitignored binary. |
| Sweep axis? | `-i all -a leapfrog-triejoin` | Two index structures (TreeTrie, ColumnTrie) on a fixed algorithm. Unlocks `bar_queries`, `bar_time`, `bar_space`, `tradeoff` without doubling generation cost. |
| File placement? | `python/kermit-lab/notebooks/00_full_timeline.ipynb` | Re-uses existing path conventions (`'../../../bench-runs/*.json'`) and the `kermit-lab` venv. Numeric prefix `00_` sorts it before the existing `01_quick_start.ipynb`, signalling that it is the new entry point. |

## Notebook structure

Sequential narrative, ~14 cells (6 markdown headers, ~8 code), grouped into
seven phases.

### Phase 0 — Prereqs and CWD

| Cell | Type | Content |
|---|---|---|
| 0.1 | md | Title; "read this first" framing; launch instructions; prereq list. |
| 0.2 | code | Resolve repo root by walking up until a sibling `Cargo.toml` is found; `os.chdir(REPO_ROOT)`; assert `shutil.which("cargo")` and `shutil.which("java")`. |

**Launch instructions** (cell 0.1, prose):

```
1. Be inside `nix develop` if on NixOS (sets LD_LIBRARY_PATH for
   libstdc++/libz so numpy/matplotlib wheels load).
2. Have JDK 8 on PATH (`java -version` reports 1.8). The flake provides
   `pkgs.jdk8`; on Ubuntu, `apt install openjdk-8-jre`.
3. From the repo root: `cd python/kermit-lab && uv sync` (one-time).
4. Launch: `uv run --with jupyterlab jupyter lab --notebook-dir=..`
   then open `notebooks/00_full_timeline.ipynb`.
```

**Working-directory contract** (cell 0.2, code skeleton):

```python
import os, subprocess, shutil
from pathlib import Path

REPO_ROOT = Path.cwd()
while not (REPO_ROOT / "Cargo.toml").exists():
    if REPO_ROOT.parent == REPO_ROOT:
        raise RuntimeError("Could not locate Kermit repo root (no Cargo.toml found above CWD)")
    REPO_ROOT = REPO_ROOT.parent

os.chdir(REPO_ROOT)
print(f"CWD: {os.getcwd()}")

assert shutil.which("cargo"), "cargo not on PATH — run from inside `nix develop`"
assert shutil.which("java"),  "java not on PATH — needed by `bench gen lubm`"
print("cargo:", shutil.which("cargo"))
print("java: ", shutil.which("java"))
```

### Phase 1 — Generate the LUBM-1 dataset

| Cell | Type | Content |
|---|---|---|
| 1.1 | md | Explains `bench gen lubm --scale 1 --tag nb-fulltimeline`; describes the cache layout at `~/.cache/kermit/benchmarks/lubm-1-nb-fulltimeline/`; notes the spec-hash short-circuit on re-runs. |
| 1.2 | code | `subprocess.run(['cargo','run','--release','--','bench','gen','lubm','--scale','1','--tag','nb-fulltimeline'], check=True)` with elapsed-time timing and a heads-up message about cold-build duration. |

The resulting benchmark name follows the `lubm-{scale}-{tag}` format from
`kermit/src/main.rs`: scale 1 with tag `nb-fulltimeline` produces
**`lubm-1-nb-fulltimeline`**.

The tag `nb-fulltimeline` is private to this notebook; the user's own
`--tag demo` runs (or any other tag) will not collide with it.

### Phase 2 — Run the benchmark (time metric)

| Cell | Type | Content |
|---|---|---|
| 2.1 | md | Explains the `-i all -a leapfrog-triejoin` sweep; describes `bench-runs/*.json` and `target/criterion/<group>/<dir>/{base,new}/` artefact layout. |
| 2.2 | code | `subprocess.run(['cargo','run','--release','--','bench','run','lubm-1-nb-fulltimeline','-i','all','-a','leapfrog-triejoin','--report-json','bench-runs/lubm-demo-time.json'], check=True)`. |

### Phase 3 — Run the benchmark (space metric)

| Cell | Type | Content |
|---|---|---|
| 3.1 | md | Explains why `--metrics space` is a separate invocation (different Criterion `Measurement`); what `heap_size_bytes()` measures. |
| 3.2 | code | Same as 2.2 plus `--metrics space --report-json bench-runs/lubm-demo-space.json`. |

### Phase 4 — Load into a DataFrame

| Cell | Type | Content |
|---|---|---|
| 4.1 | md | Points at `kl.load()` returning a tidy frame; references the schema doc. |
| 4.2 | code | `import kermit_lab as kl; kl.apply_style(); df = kl.load('bench-runs/lubm-demo-*.json', criterion_root='target/criterion'); df.head()`. |

### Phase 5 — Plots

| Cell | Type | Content |
|---|---|---|
| 5.1 | md | Brief recap of what each plot answers. |
| 5.2 | code | `kl.bar_queries(df, ds='TreeTrie', algo='LeapfrogTriejoin')` — per-query times for TreeTrie. |
| 5.3 | code | Pick the first non-trivial query in **natural order** (so `q2` is preferred over `q10`); the implementation will use `re`-based key extraction (`int(re.search(r'\d+', q).group())`) on `df.query.unique()`, exclude `q1` (trivially small per the LUBM paper's Table 3), and pass the chosen value to `kl.bar_time(df, query=q)`. Print the chosen query so the reader can audit. |
| 5.4 | code | `kl.bar_space(df)` — heap-size bars by DS. |
| 5.5 | code | `kl.tradeoff(df)` — space-vs-time scatter. |

### Phase 6 — Where to go next

| Cell | Type | Content |
|---|---|---|
| 6.1 | md | Pointers to the other five notebooks for follow-up; mentions `bench gen watdiv`, `--scale 2`, and the statistical helpers (`kl.compare`, `kl.bootstrap_ratio_ci`, `kl.mannwhitney_u`). |

## Error handling

The principle: **fail loud, fail early, fail with a pointer to the fix.**

| # | Failure | Where it surfaces | Mitigation |
|---|---|---|---|
| E1 | Wrong JDK / no JDK | Cell 1.2 (`bench gen lubm`) | Cell 0.2 asserts `shutil.which("java")` upfront. Cell 0.1 prose points at `pkgs.jdk8` or `apt install openjdk-8-jre`. The lubm-uba.jar's 1.7 source/target bytecode runs on any later JVM, so we do not parse `java -version`. |
| E2 | Long-running cells appear hung | Cells 1.2 / 2.2 / 3.2 | `subprocess.run(..., check=True)` with a one-line heads-up before each (`⏳ This may take several minutes (cold cargo build + dataset generation)…`). On success, print `✅ Done in {elapsed}s`. On `CalledProcessError`, print the last ~40 lines of stderr before re-raising. |
| E3 | Stale cache from prior `--tag` runs | Cell 1.2 if a previous run used the same tag with a different scale | The private tag `nb-fulltimeline` minimises collision risk. The spec-hash short-circuit (per `kermit-bench`) handles re-runs cleanly when the spec is unchanged; on drift, kermit's own `BenchError::SpecDrift` surfaces and the user is told to `bench run --force` from a terminal. |
| E4 | `cargo clean` between bench and plot phases | Cell 4.2 (`kl.load`) | Documented in phase 6 markdown. No code-level guard — `kl.load` already raises a clear error. |
| E5 | Hardcoded query name does not match LUBM naming | Cell 5.3 (`kl.bar_time`) | Programmatic selection in **natural order**: extract the integer from each query name with a regex, sort numerically, exclude `q1` (trivially small per the LUBM paper), pick the first remaining. Print the chosen query so the reader can audit. Lexicographic sort would put `q10` before `q2` — explicitly avoided. |

A sixth concern, **NixOS LD_LIBRARY_PATH for numpy/matplotlib wheels**, is
handled by the launch contract — the user must launch the kernel from
inside `nix develop`. If they do not, `import kermit_lab as kl` fails at
numpy import. We surface the requirement in the launch instructions; we do
not try to fix it from inside the kernel.

## Documentation touch-ups outside the notebook

| File | Required? | Change |
|---|---|---|
| `python/kermit-lab/README.md` | **Yes** | Section heading "Five worked examples" → "Six worked examples". Add the new notebook *first* in the bullet list as the entry point, marked **Start here.** |
| `README.md` (project root) | Optional | One-line addition near the existing `python/kermit-lab/` mention: `Start with python/kermit-lab/notebooks/00_full_timeline.ipynb for an end-to-end walkthrough.` |
| `USAGE.md` | No | The `kermit-lab` analysis section already covers what the notebook covers; duplicating content invites drift. |

## Verification plan

After implementation:

1. From inside `nix develop`: `cd python/kermit-lab && uv sync && uv run
   --with jupyterlab jupyter lab --notebook-dir=..`. Open the notebook and
   "Run All".
2. Confirm cell 0.2 prints both `cargo` and `java` paths.
3. Confirm phase 1 either generates the cache (~20–60s after build) or
   short-circuits on a cached spec-hash.
4. Confirm phases 2 and 3 produce `bench-runs/lubm-demo-time.json` and
   `bench-runs/lubm-demo-space.json` and write Criterion artefacts under
   `target/criterion/` (the on-disk `directory_name` is derived from the
   `function_id` produced by `kermit bench run` — read it from each
   subdir's `benchmark.json:directory_name` rather than computing it; see
   `docs/specs/bench-report-schema.md`).
5. Confirm phase 4 prints a non-empty DataFrame head with `data_structure`,
   `algorithm`, `query`, and `tuples` columns.
6. Confirm phase 5 renders four inline figures (no `Figure size` text-only
   output).
7. Re-run "Run All" with the cache populated: phase 1 should short-circuit
   (no network, no JVM activity).

## Risks and open questions

- **Cold-build latency.** First `cargo run --release` triggers a full
  release build (~3–5 minutes). The notebook documents this but cannot
  speed it up. Future notebooks could opt into `cargo build --release`
  once at session start, then call already-built binaries directly.
- **JDK 8 is end-of-life.** If the project ever drops JDK 8, the LUBM jar
  will need to be rebuilt under the new JDK, and `vendor/lubm-uba/REGENERATE.md`
  will need updating. Out of scope here.
- **`bench gen lubm` writes to `~/.cache/kermit/`.** The cache lives outside
  the repo, so a fresh clone produces a fresh cache. This is correct
  behaviour, but the notebook should call it out so a reader does not
  expect data inside the working tree.
