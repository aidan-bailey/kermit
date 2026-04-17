# WatDiv Benchmark Integration — Design

**Date:** 2026-04-17
**Status:** Approved, awaiting implementation plan

## Context

Kermit currently benchmarks the Leapfrog Triejoin (LFTJ) algorithm against
synthetic and small graph workloads (`triangle`, `oxford-*`). To strengthen
the thesis' empirical basis we want to integrate the Waterloo SPARQL
Diversity Test Suite (WatDiv) — specifically the `.nt` dataset at
`watdiv-data/watdiv.10M.nt` (1.5 GB, ~10.9M triples, 86 distinct
predicates) and the two stress-test suites
(`watdiv-stress-{100,1000}/test.{1..5}.sparql` plus `warmup.sparql`,
each ~12.4K queries, ~124K total).

### Blockers in the existing codebase

1. **Query scale.** 124K queries cannot reasonably live inline in a single
   benchmark YAML.
2. **Constant filtering is broken.** `kermit-algos/src/leapfrog_triejoin.rs`
   silently drops `Term::Atom` values in `build_variable_index` (see the
   comment at line 249), so any query with a literal filter produces
   wrong answers. WatDiv queries rely heavily on constant filtering.
3. **No data pipeline.** WatDiv ships N-Triples; kermit consumes
   dictionary-encoded Parquet.

## Decisions (locked before writing this doc)

1. **Scope C — full integration.** Data ingestion, workload translation,
   and a correct fix for constant filtering all land together.
2. **Option 4 for filtering — paper-canonical Const-view rewrite.**
   Follows Veldhuizen 2014 §3.4 point 4: a subformula `A(x, 2)` is
   rewritten to `A(x, y), Const_2(y)` where `Const_2 = {2}` is a
   nonmaterialized singleton TrieIterator. Zero changes to the LFTJ
   engine itself.
3. **Translation architecture A — offline precompute.** A one-time tool
   converts the `.nt` + `.sparql` inputs into Parquet + dictionary + YAML
   artifacts. The runtime path is unchanged.
4. **Grouping A3 — one benchmark per SPARQL file.** Each of the ~12
   SPARQL files becomes its own YAML benchmark with per-query entries,
   preserving per-query granularity without a monolithic artifact.
5. **Preprocessor location P3 — external Python script.** Lives outside
   the Rust workspace (`scripts/watdiv-preprocess/`). Uses mature RDF
   and SPARQL tooling (`rdflib`, `pyarrow`, `pyyaml`). Keeps SPARQL and
   RDF deps out of the pure-Rust codebase.

## Architecture

```
[offline, one time]
 watdiv.10M.nt  ─┐
                 ├─►  scripts/watdiv-preprocess/  ─►  artifacts/
 *.sparql files ─┘       (Python 3 + rdflib)          ├── dict.json        (URI → usize)
                                                      ├── <predicate>.parquet (×86)
                                                      └── watdiv-stress-{100,1000}-{test-N,warmup}.yml (×12)

[upload]
 artifacts/*.parquet  ─►  ZivaHub (or equivalent HTTPS host)

[runtime]
 kermit bench run <benchmark-name> -i tree-trie -a leapfrog-triejoin
    └─► kermit-bench downloads referenced Parquet files (existing path)
    └─► kermit parses generated Datalog query (parser unchanged)
    └─► kermit-algos rewrites atoms → Const_ predicates (new code)
    └─► existing LFTJ runs unchanged
```

Key properties:

- **No new runtime benchmark kind**: generated YAMLs use the existing
  schema; runtime sees "just another benchmark."
- **Const rewrite is runtime**: the preprocessor emits
  `pred(V0, c12345)` style Datalog; rewrite to synthetic
  `Const_c12345` predicates happens in `kermit-algos` just before LFTJ.
- **Dictionary is global across all WatDiv artifacts**: one `dict.json`
  shared by all 86 Parquet files and all 12 YAMLs.

## Components

### 1. Preprocessor — `scripts/watdiv-preprocess/`

Python 3. Deps: `rdflib`, `pyarrow`, `pyyaml`.

**CLI:**
```
watdiv-preprocess.py --input <dir> --output <dir> --base-url <url>
```

**Processing stages:**

1. **Build dictionary.** Stream `watdiv.10M.nt` once. For each triple
   `(s, p, o)`, assign fresh `usize` IDs to unseen subjects, predicates,
   and objects. Write `dict.json` (two-way: `uri → id` and `id → uri`)
   and `dict.parquet`.

2. **Partition triples by predicate.** Second pass over
   `watdiv.10M.nt`. For each triple `(s, p, o)` append `(dict[s],
   dict[o])` to an in-memory buffer keyed by `p`. Flush each buffer to
   `<sanitized_predicate>.parquet` (schema: two `UInt64` columns `s`,
   `o`). Sanitize predicate URIs to valid relation identifiers (strip
   prefix; replace non-alphanumerics with `_`; e.g.
   `http://xmlns.com/foaf/homepage` → `foaf_homepage`).

3. **Translate SPARQL files.** For each of the 12 SPARQL files:
   - Parse each query with `rdflib`'s SPARQL parser.
   - Require BGP-only SELECT; fail loudly on OPTIONAL, FILTER, UNION,
     subqueries. (WatDiv stress tests are all BGPs.)
   - Map each triple pattern `(?s, <pred>, ?o)` to
     `<sanitized_pred>(S, O)`; variables become uppercase Datalog vars
     (`?x` → `X0`); constants become `c<dict_id>` atoms.
   - Generate a head `Q_<file>_q<N>(V0, V1, ...)` from the SELECT
     projection.

4. **Emit benchmark YAMLs.** One `watdiv-stress-<size>-<stem>.yml` per
   SPARQL file:
   - `name`: filename stem
   - `description`: `"WatDiv stress test <size>, file <stem>, <N> queries"`
   - `relations`: one entry per predicate **actually referenced** by any
     query in that file. Each entry is `{name, url: "<base-url>/<name>.parquet"}`.
   - `queries`: one `QueryDefinition` per SPARQL query. Name like
     `q0042`, description from the SPARQL comment or index, `query`
     field holds the translated Datalog string.

5. **Sidecar `expected.json`** captured from `.desc` files for
   correctness verification.

~400–600 LOC Python. Not unit-tested as a Python project — its
correctness is covered by the runtime integration tests (Layer 2 below).

### 2. Parser — no change

The existing parser accepts `c12345` as `Term::Atom("c12345")` (lowercase
alphabetic prefix). The constant-encoding convention is enforced in the
rewrite step, not the grammar. Validation of the `c\d+` shape lives in
`rewrite_atoms` and produces a clear error on malformed input.

### 3. Const view rewrite — `kermit-algos/`

Three new pieces. Zero changes to `LeapfrogTriejoinIter`,
`update_iters`, `triejoin_open`, `triejoin_up`, or
`build_variable_index`.

**Piece 1: `SingletonTrieIter`** — `kermit-algos/src/singleton.rs`
(~40 LOC). Minimal unary TrieIterator holding one `usize`. State:
root (depth 0) → `open()` moves to value (depth 1) → `key()` returns
the value → `next()` / `seek(k >= value)` sets `at_end` → `up()` returns
to root. Implements `TrieIterator`.

**Piece 2: `TrieIterKind<IT>`** — `kermit-algos/src/trie_iter_kind.rs`
(~50 LOC).

```rust
enum TrieIterKind<IT: TrieIterator> {
    Relation(IT),
    Singleton(SingletonTrieIter),
}
```

Dispatches all `TrieIterator` methods to the inner variant. Lets LFTJ
hold a heterogeneous iterator set.

**Piece 3: `rewrite_atoms`** — `kermit-algos/src/const_rewrite.rs`
(~80 LOC).

```rust
fn rewrite_atoms(query: JoinQuery) -> Result<(JoinQuery, Vec<(String, usize)>), RewriteError>
```

For each `Term::Atom(s)` in `query.body`:
1. Validate `s` matches `c\d+`; parse suffix as `usize` → `id`.
2. Allocate a fresh variable (`K0`, `K1`, ..., chosen to not collide).
3. Replace the atom with `Term::Var(fresh)`.
4. Append predicate `Const_c<id>(fresh)` to the body.
5. Record `(name, id)` in the side-list.

Returns the rewritten query plus the list of `(const_predicate_name,
dict_id)` pairs.

**Example:**
```
Input:  Q(V0, V2) :- foaf_homepage(V0, c2948), ogp_title(V0, V2).
Output: Q(V0, V2) :- foaf_homepage(V0, K0), ogp_title(V0, V2), Const_c2948(K0).
Side-list: [("Const_c2948", 2948)]
```

### 4. Runner glue — `kermit/src/main.rs`

`run_benchmark` changes (~30 LOC):

1. Parse Datalog string to `JoinQuery` (unchanged).
2. Call `rewrite_atoms(query)` → `(rewritten, const_specs)`.
3. Build real relations from Parquet (unchanged).
4. Wrap each real relation iterator in `TrieIterKind::Relation(it)`.
5. For each `(const_name, id)` in `const_specs`, construct
   `TrieIterKind::Singleton(SingletonTrieIter::new(id))`.
6. Build the `HashMap<String, &TrieIterKind<IT>>` the algorithm expects.
7. Call `join_iter(rewritten, iters_map)`.

**Signature change in `kermit-algos`:** `JoinAlgo::join` / `join_iter`
currently take `HashMap<String, &DS>` where `DS: TrieIterable`. Cleanest
fix: make `TrieIterKind` itself implement `TrieIterable` so it slots in
where `DS` does. Type-level change only — no algorithmic impact.

**Unfiltered queries:** empty `const_specs` → code path collapses to
today's behavior with one extra `Vec` allocation.

## Testing

**Layer 1 — unit tests** (in `kermit-algos`):
- `rewrite_atoms` on zero-atom query → identity.
- Single atom → one fresh var, one Const_ predicate, correct ID.
- Multiple atoms → each gets its own fresh var and Const_.
- Same atom value twice (e.g. `p(X, c5), q(Y, c5)`) → two distinct fresh
  vars both filtered by `Const_c5`.
- Malformed atom (`foo`, `c`, `c1x`) → clear rewrite error.
- `SingletonTrieIter` state machine: all five open/key/next/seek/up
  transitions covered.

**Layer 2 — end-to-end correctness** (new
`kermit/tests/watdiv_correctness.rs`):
- Small fixture: 10–20 queries over a trimmed `.nt` (a few thousand
  triples), inline.
- Expected result counts come from `expected.json` emitted by the
  preprocessor.
- Not gated on full dataset download; full-dataset validation is a
  manual `bench run` when the user needs it.

**Layer 3 — regression:** existing benchmarks (`triangle`, `oxford-*`)
produce identical results. The rewrite is a no-op on queries with no
atoms; existing tests catch any `TrieIterKind` dispatch regression.

**Explicitly not tested:** the Python preprocessor. Layer 2 covers its
output quality; no pytest scaffolding needed for a ~500-line generation
script.

## Out of scope

- Extending the parser with a formal numeric-literal grammar.
- Non-BGP SPARQL features (FILTER, OPTIONAL, UNION, subqueries).
- Alternative partitioning schemes (triple-store in one Parquet with a
  predicate column).
- Hosting infrastructure (ZivaHub upload step is manual).
- Full Python test suite for the preprocessor.

## Next step

Invoke `superpowers:writing-plans` to produce a step-by-step
implementation plan from this design.
