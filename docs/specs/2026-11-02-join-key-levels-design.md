# Join-Key Trie Levels: Payload-Last Plans and a `keys` HashTrie Layout

**Date:** 2026-11-02
**Status:** Design approved in conversation: decision by decision (2026-10-23 to 2026-11-01), then
section by section (2026-11-01 to 2026-11-02).
**Scope:** The two steps of #116. Step 1: every optimiser plans the join keys first and the payload
after them, for every structure. Step 2: `HashTrie` gains a fourth Layout,
`--ds-layout-levels all|keys`. Under `keys`, an index's trie has levels only up to its last
join-key column. The columns after it are payload, kept in the leaf chains, which Hash Triejoin's
leaf product already reads. `all`, the default, is today's trie. The sorted tries keep full tries.
The report schema stays at v6.
**Related:**
- Freitag et al., VLDB 2020:
  - §3.1: one level per join key; the last level holds identifiers that give access to the
    payload;
  - §3.2.1: tries over the join attributes, in the join's order;
  - Algorithm 1 line 3: every relation that holds the join key joins its intersection;
  - Algorithm 3 and §3.2.4: the bag of tuples is expanded after the join keys;
  - §3.3.2: identical hash tries are built once;
  - §4: multi-way joins come only from inner equi-joins, and each attribute order is chosen in
    isolation.
- #116, and its parent #105 (paper convergence). The board row: "Representation: trie levels are
  the join keys only; every other column is payload in the leaf tuples (§3.1, §3.2.1)".
- The per-query index builds (`docs/specs/2026-10-22-per-query-index-build-design.md`), which made
  each query build its own indexes in plan order.
- Report v6. It is on local master (2b4d385) but not yet on origin. Its `plan` axis labels the
  rows whose plans step 1 changes.
- #99, the next sweep, which measures both steps.

## Summary

Today a HashTrie index has one level per column. The paper's hash trie has one level per join
key. A column that no other relation of the join holds is payload: it is kept in the tuples that
the last level's lists point at, and read when Algorithm 3 expands a leaf into result tuples
(§3.1, §3.2.4). kermit's Hash Triejoin already ends the same way. `emit_leaf` crosses every atom's
leaf chain, and `verify_and_construct` binds every variable from the rows. So dropping the payload
levels needs no new join step. It needs two things:
- plans that put every join key before every payload column;
- a trie that stops after its last join key.

**Step 1** makes every optimiser's plan payload-last: join keys first, then the payload. It
changes plans on every structure, and v6's `plan` axis records each row's plan, so the changed
rows can be told apart.

**Step 2** adds the HashTrie Layout `levels`:
- An index spec carries a level count, and the count travels to the structure in the index's
  `RelationHeader`.
- `HashTrie<…, KeyLevels>` builds that many levels.
- `AllLevels`, the default, and the sorted tries ignore the count.
- Hash Triejoin descends, at each plan position, only the atoms whose trie has a level there. It
  skips a position that no trie has a level for, and runs its leaf product unchanged.

Nothing is measured here. Both steps ship with `all` as the default, and #99 measures them.

## Motivation

1. **Parity (#105).** The paper's trie levels the join keys only (§3.1). kermit's levels every
   kept column.
2. **What a payload level costs.** A payload level costs a table under each bucket of the level
   above, in the build and in `space`. In the join it costs a scan of its values at a position
   that joins nothing. Under `keys` those columns stay in the leaf rows, and the leaf product
   reads them (Algorithm 3 lines 16–19). Whether that is faster is #99's question; this design
   makes no timing claim.
3. **Step 1 is step 2's prerequisite.** A trie reaches a level only through the levels before
   it, so it can drop only trailing columns. Payload must therefore come last in each index's
   column order. `plan_indexes` sorts an index's columns by plan position, so the plan decides
   that order.
4. **Step 1 shrinks cost-based's search** from 2^n bound sets to 2^k, where k counts the join
   keys.

## Decisions

1. **Two steps.** Step 1 covers every optimiser and every structure (the user, at filing,
   2026-10-23).
2. **Step 2 is HashTrie only, as a Layout `all|keys` with `all` the default** (the user,
   2026-10-23). The sorted tries keep full tries, as in Veldhuizen's Leapfrog Triejoin, which
   joins on every attribute. Revisit them only if TreeTrie `build` or `space` evidence from
   payload-heavy queries (JOB, H2O) calls for it, and then as a labelled kermit experiment.
3. **Approach A** (the user, 2026-10-23). The index spec carries a level count, and each structure
   decides whether to use it. Rejected:
   - (B) a HashTrie-only constructor parameter: every family and `build_with` signature would
     change;
   - (C) a separate structure: a fourth structure in every suite, recipe and dispatch.
4. **Plans with payload before a join key: rule (iii)** (the user, 2026-11-01). A `keys` trie
   gives each index levels for its columns up to its last join-key column. That keeps Algorithm
   1 line 3: every relation that holds a join key joins its intersection. So every plan stays
   worst-case optimal and does no more work than under `all`. Every optimiser plan after step 1
   gives exactly the paper's trie. The odd plans arise only from `FixedOrder`, the test optimiser
   that forces each variable order. Rejected:
   - (i) Stop at the first payload column. A join key after that column would leave its
     intersection: the relation would stop constraining that key, and the leaf product would
     cross its whole chain with every binding the other relations allow. Take
     `R(X, Y), S(Y, Z), T(X, Z, P)` under the plan P, X, Y, Z. It would cross every path of
     R ⋈ S with all of T: up to N³ candidates, where N³ is |R ⋈ S| ≤ N² times |T| = N. The output
     is at most N², its AGM bound, since T alone covers P.
   - (ii) Panic under `keys`. This adds a crash path, and `all_orders` could not run `keys`.

   Set aside: give every join key a level wherever it sits in the index. That is the literal
   reading of the paper, but it needs a column lookup at every `row[depth]` read of the build and
   the iterator.
5. **Index sharing: exact** (the user, 2026-11-01). Only identical specs, level count included,
   share a trie, as Umbra builds identical hash tries once (§3.3.2). Deferred and not filed:
   `keys-shared`, a third value of the Layout that would build one trie per column order, at the
   greatest depth any spec asks for. It is wanted only if #99 shows the extra builds matter.
6. **No schema bump** (the user, 2026-11-01). v6 adds the `plan` axis, each row's variable order,
   and that axis labels every row step 1 changes, as the seek-default and allocator changes
   were labelled without a bump. So v6 must be on origin, and merged into this branch, before
   step 1 lands. Pushing it is the owner's call. Rejected: v7, which would cut the queries whose
   plans do not change off every older result.
7. **No measurement in #116** (the user, 2026-11-01). It ships with `all` as the default. #99
   measures step 1 and `keys`, and its harness gains `keys` HashTrie cells. Flipping the default
   is a later amendment, the owner's call. Rejected:
   - a probe prescreen plus a targeted timed A/B;
   - full sweeps before and after, with a flip rule fixed now.
8. **A Layout, not a BuildMode or a Config.** A BuildMode must build an equivalent structure:
   the same contents and capacities, and so the same `space` (Amendment 2 of
   `docs/specs/2026-10-05-parallel-build-design.md`). `keys` changes all three. Nor is it a
   Config, which substitutes a value on a path the code already takes. `keys` changes which
   tables exist, and it adds a root variant that is uninhabited under `all`. By the standard's
   non-user-tax test, that is a shape.

## Background: levels and plans today

- `prepare` builds the access query, plans it, then calls `plan_indexes`. That sorts each
  access's kept columns by the plan positions of their variables, giving an
  `IndexSpec { relation, filter, columns }`. Identical specs merge (`index_plan.rs:108` at
  2607a04), and ids `#k` follow first appearance.
- Every structure gives every column a level. HashTrie's Algorithm 2 recurses to
  `header.arity()`, and the buckets of the last level hold chains.
- Hash Triejoin descends every atom that carries a variable at that variable's plan position
  (`build_variable_to_iter_map`). So after the last position every iterator is on its leaf, and
  `emit_leaf` crosses the chains.
- The optimisers:
  - Lexicographic orders by canonical index, head variables first, so a head's payload often
    comes first.
  - Cardinality orders by the smallest relation holding each variable.
  - CostBased runs a DP over all n variables, with a 64-variable guard, a `state_budget` of
    1 << 14, and Cardinality as its fallback.

## Design

### Join keys (`kermit-algos/src/analysis.rs`)

`QueryAnalysis` gains `join_keys: Vec<bool>`, indexed by canonical variable:
- `join_keys[v]` is true when two or more body atoms carry `v`. An atom that carries it twice
  counts once.
- Every other variable is payload.
- `analyse` computes it from `predicate_variables`.

The planners and `plan_indexes` read the access query. There an atom never repeats a variable,
and an all-constant atom is a check, not an atom, so a join key is a variable that two or more
accesses share. The `triangle()` test helper in `optimiser/plan.rs` builds a `QueryAnalysis` by
hand, and gains the field.

### Step 1: payload-last plans (`kermit-algos/src/optimiser/`)

- **`order_by_rank(analysis: &QueryAnalysis, rank)`** (today `order_by_rank(num_vars, rank)`)
  sorts by `(payload, rank(v), v)`: join keys first, each group by rank, then by canonical index.
  - Lexicographic and Cardinality pass the analysis they already compute.
  - A later optimiser built on `order_by_rank` plans payload-last with no code of its own.
- **CostBased.**
  - `cheapest_order` runs its DP over the k join keys only, renumbered 0..k in canonical order
    for its bitmasks. Its 64-variable guard and its `state_budget` therefore count join keys.
  - The cost model maps each variable to its key number, or to none for a payload variable,
    which is never bound.
  - The payload follows the keys, in canonical order.
  - Every key order ends with all the keys bound, so the estimates of the payload suffix are the
    same for every key order. Leaving them out cannot change which key order is cheapest.
  - Renumbering preserves the lexicographic tie-break, since it keeps canonical order.
  - Its Cardinality fallback is payload-last through `order_by_rank`.
  - Some queries now fit the DP and stop falling back: those with more than 64 variables but at
    most 64 join keys, and those whose key sets fit the budget when all their variables did not.
- **Unchanged:** the `Planner` forwards the optimiser's plan as is, and nothing normalises a plan,
  so `FixedOrder` still forces any order, including the odd ones. `QueryPlan::validate` is
  unchanged.

**What step 1 changes.** These claims go into the optimiser docs.
- **It never visits more key bindings.** For a fixed order of the join keys, moving the payload
  after them never visits more key bindings. A payload-last search reaches each binding of a key
  prefix once; an interleaved search may reach it once per binding of the payload before it.
- **It is never worse under `keys`**, for that key order. The payload is not a level, and the
  leaf product enumerates only result rows.
- **Under `all` and on the sorted tries, it adds a payload walk.** The payload columns are still
  levels, walked once per surviving key binding: a walk proportional to the output, which a
  payload-first plan can partly avoid.
  - Example: `Q(P, K) :- R(P, K), S(K).`, where R holds 1,000 rows under one P value, and S
    holds the same 1,000 K values.
  - Today's plan, P, K: 1 scan step at P, then 1,000 scans and 1,000 lookups at K. 2,001 steps.
  - The payload-last plan, K, P: the same 2,000 steps at K, then 1,000 scans of one-bucket P
    tables. 3,000 steps, about 1.5× more under `all`.
  - Under `keys`, K, P takes 2,000 steps.
- **It can split an index that a self-join shared.** SNAP's `path3`,
  `P(A, B, C, D) :- edge(A, B), edge(B, C), edge(C, D).`, has join keys B and C, and payload A
  and D.
  - Under today's plan, A, B, C, D, all three atoms read `edge` as `[0, 1]`: one index.
  - Under lexicographic's payload-last plan, B, C, A, D, `edge(A, B)` is read destination
    first, as `[1, 0]`, so the query builds two `edge` indexes, on every structure.
  - SNAP's triangle, cycle4 and clique4 have no payload variable, so their plans do not change.
- **Every benchmark query with a payload variable may change plan,** on every structure. v6's
  `plan` axis tells those rows apart (Decision 6).

### Index specs carry a level count (`kermit-iters`, `kermit-algos`, `kermit-ds`, `kermit::db`)

**`TrieLevels`** goes in `kermit-iters/src/trie_levels.rs`, beside `Filter`, so that both
`kermit-algos` and `kermit-ds` can name it; `kermit-ds` re-exports it.
- `enum TrieLevels { All, JoinKeys }`: which of an index's columns its trie gives levels to.
- `All` is the default.
- It derives `Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash`.

**`IndexSpec`** gains `levels: usize`: the trie has levels for the first `levels` columns, and
the rest are payload in the leaf rows. `plan_indexes(accesses, plan, levels: TrieLevels)` sets
it:
- **`All`:** `levels` is the column count.
- **`JoinKeys`** (rule (iii)): one past the last column whose variable is a join key of the
  access query, or 0 when none is. An index's columns follow the plan, so for a payload-last plan
  this is the number of join-key columns.

Identical specs merge as today, with `levels` part of the comparison:
- Under `JoinKeys`, specs that differ only in their level count become separate indexes
  (Decision 5).
- Under `All`, the merge is exactly today's.
- Ids are unchanged.

**`Relation::TRIE_LEVELS: TrieLevels`** (default `All`), beside `KEEPS_DUPLICATES`, is the policy
a structure builds under:
- HashTrie takes it from its Layout (`L::TRIE_LEVELS`).
- `Configured` and `BuiltWith` forward `R::TRIE_LEVELS`.
- The sorted tries keep the default.

**`prepare(catalog, query, planner, levels: TrieLevels)`** hands `levels` to `plan_indexes`.
`Prepared` records it, read through `Prepared::trie_levels()`.
- `lftj_join_for_each`, `lftj_join`, `hash_join_for_each`, `hash_join` and `compute_join` pass
  `R::TRIE_LEVELS`.
- `Workload::prepare` passes the policies of its cells (see the binary, below).
- The test helpers that call `prepare` themselves (in `common/utils.rs`, `all_orders.rs` and
  `result_allocation.rs`) pass `R::TRIE_LEVELS`.
- `build_with::<R>`, which `build` and every family's `build_indexes` reach, panics when
  `prepared.trie_levels() != R::TRIE_LEVELS`, naming both values. A query prepared for one
  policy must not be built under another.

**`RelationHeader`** gains a private `levels` field, like `attrs` and `arity`:
- Every constructor sets it to the arity.
- `with_levels(self, levels)` sets it, and panics above the arity.
- `levels()` reads it.
- `spec_header` sets it from `spec.levels`.

No relation constructor, family or build-mode signature changes: the count travels in the
header, and a structure that ignores it builds as today. Answer headers, `project`'s nameless
header and headers read from files keep the default.

**Example: SNAP `path3`** under lexicographic's payload-last plan, B, C, A, D:

| atom | `JoinKeys` | `All` |
|---|---|---|
| `edge(A, B)` | `#0 edge [1, 0]`, 1 level | `#0 edge [1, 0]` |
| `edge(B, C)` | `#1 edge [0, 1]`, 2 levels | `#1 edge [0, 1]` |
| `edge(C, D)` | `#2 edge [0, 1]`, 1 level | `#1` again |

Three `edge` tries under `keys`, two under `all`.

### The `levels` Layout (`kermit-ds/src/ds/hash_trie/`)

`HashTrie<H, P, E, L: LevelPolicy = AllLevels>`.

`LevelPolicy` lives in a new `levels.rs`, beside `pruning.rs` and `expansion.rs`. It is
`LayoutOption + Copy + Default + 'static` and carries:
- `const TRIE_LEVELS: TrieLevels`;
- `fn levels(header: &RelationHeader) -> usize`;
- `type Rows<C>`, the payload of a zero-level root. It is generic over the chain type, so that
  the public trait never names the crate-private `Chain` (the device `ExpansionPolicy::Pending<N>`
  uses).

Its two values:
- **`AllLevels`** (the default): `TRIE_LEVELS = All`. `levels` returns `header.arity()`, the value
  every site reads today, and ignores the header's count. `Rows<C> = Never`.
- **`KeyLevels`:** `TRIE_LEVELS = JoinKeys`. `levels` returns `header.levels()`. `Rows<C> = C`.

The axis is `ds_layout_levels` (`all` / `keys`), in `HasOptimizationAxes`.

**What reads the level count.** Every site that takes the arity as the leaf depth reads
`L::levels(&header)` instead. By function, at 2607a04:
- in `implementation.rs`:
  - `is_leaf_depth`;
  - `make_root_sized`'s leaf test and its callers;
  - `insert_at`;
  - `child_is_leaf`;
  - `resolve`;
  - `insert`;
  - the bulk and incremental builds;
  - `from_view_partitioned`;
- `bulk.rs`'s `group` and `child`;
- `radix.rs`;
- `parallel.rs`;
- `hash_trie_iter.rs`: the descent bound, the singleton descent and the singleton leaf.

`insert` still checks the row's length against the arity.

**A trie with fewer levels than columns.**
- Algorithm 2 recurses `levels` deep. The buckets of the last level hold chains of whole rows,
  payload columns included.
- The Config values apply to the tables that exist.
- A pruned `Singleton` above the last level is descended level by level until `levels`, then
  lent as its one row.
- Lazy `resolve` builds a child one level deeper, and stops at `levels`.
- At every level count, the build modes build the same contents and capacities, so Amendment
  2's equivalences hold: `bulk` and `incremental` are array-identical, and the radix family is
  equivalent in contents.

**Zero levels.** An index with no join key has no levels: no table, and one chain of its rows in
source order.
- The root becomes `enum Root { Table(HashTrieNode<P, E>), Rows(L::Rows<Chain>) }`.
- Under `AllLevels`, `Rows` holds the uninhabited `Never`. So `Root` is the size of the node, and
  every `Rows` arm compiles away. A size test pins it, as
  `node_does_not_grow_under_the_pruning_policy` pins pruning.
- Every build mode makes the same copy at zero levels: `view.materialize()`, then one chain
  through the rows in order. `radix:K`, `parallel:N` and `presized:N` have no first key to
  partition on, so they build exactly as `bulk` does, array-identical, and push no `test_hooks`
  record.
- `insert` appends to the chain.
- `for_each_tuple`, `heap_size_bytes` and `collect_tuples` match the root's two variants at
  their entry, and otherwise walk as today. A `Rows` root owns no heap of its own.

**The iterator.**
- `HashTrieIterable` gains a required `fn levels(&self) -> usize`. HashTrie returns
  `L::levels(&self.header)`; `Configured` and `BuiltWith` forward it.
- `open` returns `false` at depth `levels` rather than at the arity, so at the pre-root of a
  zero-level trie.
- The contract of `leaf_tuples` becomes: `Some` on an occupied bucket of the last level, or at
  the pre-root of a zero-level trie, where it lends the whole chain, possibly empty; `None`
  otherwise.
- Under `AllLevels` nothing changes: `levels` is the arity, and the pre-root case cannot arise.
- The iterator gains no field. A size test pins it, as `off_frame_is_the_bare_table_pair` does
  today.

**Unchanged:** the node and table types, the hashers, `HeapSize`'s accounting (fewer tables are
simply fewer bytes), `project` (its nameless header has every level), morsel partitioning and the
Config types.

### Hash Triejoin (`kermit-algos/src/hash/hash_triejoin.rs`)

- **Participation by level.** For each plan position, `build_variable_to_iter_map` lists the
  atoms whose first `levels()` columns hold that position's variable. Today it lists every atom
  whose columns hold it.
  - In the executed query an atom's terms follow its index's columns, so these columns are the
    atom's first `levels()` terms.
  - It is computed once per join, from each relation's `levels()`.
- **Skipped positions.** At a position that no atom has a level for, `enumerate` moves on to the
  next position; the check at `:124` used to return.
  - That position's variable is payload, bound later from the leaf rows.
  - Under `all` every variable is a level of every atom that carries it, so every position has a
    participant and the branch never runs. The check runs today and never fires, so `all` pays
    nothing new.
- **The descent.** Each atom is opened exactly `levels()` times on the way down, at its levels'
  positions, in order. After the last position, every iterator stands at its last level, or at
  its pre-root when it has no levels, and `emit_leaf` reads every chain as today.
- **The leaf product is unchanged.** `emit_leaf` and `verify_and_construct` already bind every
  variable from the rows, and reject a candidate whose rows disagree (Algorithm 3 lines 16–19).
  - A zero-level atom lends its whole chain without being opened.
  - An empty chain gives no candidates: `emit_leaf` already returns on one.
- **The contract comment.** `enumerate`'s contract comment becomes: each participant at position
  `i` stands at the level just before its level for `i`'s variable.
- **The answers** are the same multiset as under `all`. Only their order may differ: chains are
  in input order, and under `all` the payload values came in bucket order. The join suite sorts
  both sides before it compares them (`kermit/tests/common/utils.rs:220-228`).
- LFTJ is untouched.

**Worked example.** The query is `Q(X, P, Y) :- R(X, P), S(X, Y), T(Y).` The relations are
R = {(1, 10), (1, 11), (2, 20)}, S = {(1, 5), (2, 6)} and T = {(5), (7)}.

X and Y are join keys and P is payload. Under `keys`:
- R is `[X, P]` with 1 level;
- S is `[X, Y]` with 2 levels;
- T is `[Y]` with 1 level.

Take today's lexicographic plan, X, P, Y, which puts the payload before a key:
- **Position X:** R and S participate. X = 1 is in both.
  - **Position P:** no atom has a level for P, so it is skipped.
  - **Position Y:** S, below X = 1, holds {5}, and T holds {5, 7}, so Y = 5.
    - **Leaf:** R's chain at X = 1 holds (1, 10) and (1, 11), S's holds (1, 5), and T's holds
      (5). That gives two candidates, and both verify: (1, 10, 5) and (1, 11, 5).
- **X = 2:** S holds {6} below X = 2, and T has no 6.

Lexicographic's payload-last plan, X, Y, P, builds the same three indexes and skips P last. If
the plan were P, X, Y instead, R would be `[P, X]`. Its last join key is its second column, so by
rule (iii) it keeps both levels, and P is scanned at position 0, as under `all`.

### The binary (`kermit/src`, on v6)

- **The nested macro first.** `with_hash_trie_layout!` becomes a nested macro that expands one
  dimension at a time, as its own doc asks for before a fourth dimension. That is its own commit,
  with no change in behaviour. The fourth dimension then takes it to 16 cells.
- **The new types and fields:**
  - `LevelsChoice` (`all` | `keys`, a clap `ValueEnum`);
  - `levels_of::<L>()`, the label derived from the type, like `pruning_of`;
  - `LayoutChoices::hash_trie_levels`, for `--ds-layout-levels`;
  - `DsFlag::LayoutLevels`, with its row in `DsFlag::structures` (hash-trie only);
  - `Execution::HashHtj { levels, .. }`;
  - `HashTrieFamily<H, P, E, L>` and `HashHtj<H, P, E, L>`, whose `execution()` re-derives
    `levels` from `L`, the two-roles rule that `hasher`, `pruning` and `expansion` follow;
  - `Execution::trie_levels()`: `All` for `TrieLftj`, and the cell's choice for `HashHtj`.
- **One preparation per policy.**
  - `Workload::prepare(self, planner, levels: &[TrieLevels])` prepares each query once per
    distinct policy, untimed.
  - `PreparedWorkload::prepared(levels)` returns that policy's queries, and panics on a policy
    that was not prepared.
  - `bench run` and `bench probe` pass the policies of their cells. That is at most two, since
    each Layout flag takes one value: `All` for the sorted cells, and the HashTrie cell's own.
  - `bench join` and `kermit join` pass their one cell's policy.
  - `run_benchmark` reads `workload.prepared(<F::Rel as Relation>::TRIE_LEVELS)`.
- **`bench ds` refuses `--ds-layout-levels`.** Its relation is built from a whole file, with no
  query, so it has no join keys. The message follows the one for `--ds-layout-seek`.
- **The `index` line.** `describe_index` appends ` levels {n}` after the columns when `n` is
  below the column count, as in `#0 edge filter - columns [1, 0] levels 1: … rows, … bytes`.
  Under `all` no line gains the suffix, so the format is today's.

## Parity with the paper

| Element (Freitag et al., VLDB 2020) | kermit after this design |
|---|---|
| One trie level per join key attribute (§3.1, §3.2.1) | ✓ under `keys`. ✗ under `all`, the default, until #99 measures `keys`; the flip is the owner's call |
| The other columns are payload, reached through the last level's identifiers (§3.1) | ✓ under `keys`: the last level's chains name whole rows by row id |
| Levels in the join's attribute order (§3.2.1; Algorithm 2 line 8) | ✓ an index's columns follow the plan (unchanged) |
| An attribute order over the join keys only (§3.2.1, §4) | — kermit's plans order every variable: the join keys first, then the payload, which no `keys` trie has a level for |
| Every relation holding a join key joins its intersection (Algorithm 1 line 3) | ✓ participation by level. Skipping a position that no trie has a level for is kermit's, since the paper's orders have no such position |
| The bag of tuples is expanded after the join keys, with collisions checked before output (Algorithm 3 lines 16–19, §3.2.4) | ✓ `emit_leaf` and `verify_and_construct`, unchanged |
| Identical hash tries are built once (§3.3.2) | ✓ exact sharing. A trie that differs only in its level count is built separately (`keys-shared` is deferred) |
| A plan with payload before a join key | — rule (iii), kermit's. Such a trie keeps levels up to its last join key, so every join key stays a level and the join stays worst-case optimal. No optimiser plans this after step 1 |
| A relation that holds no join key | — kermit's: no levels, one chain. The paper is silent: its multi-way joins come from inner equi-joins (§4), and each of their relations shares a join key |
| How the join keys are ordered | Tributary Join's cost model (§4) against kermit's optimisers, unchanged |

The closest-to-paper configuration becomes:
```
--ds-layout-levels keys --ds-layout-pruning on --ds-layout-expansion lazy
--ds-config root-capacity=tuples,child-capacity=tuples,load-factor=0.8
--ds-build hash-trie=presized:N
```

## Placement under the standard

- **Step 1 is not an optimisation with an axis.** It changes every optimiser's plans
  unconditionally, by the user's decision at filing, as #93 made the planner free. v6's `plan`
  axis labels each row's plan, and `FixedOrder` still forces any order in the tests.
- **Step 2 is a Layout** (Decision 8): the `L` parameter of `HashTrie`, the axis
  `ds_layout_levels`, and aliases with a `Keys` suffix.
  - It costs a non-user nothing. `levels` is the arity read today. The `Rows` root is
    uninhabited under `AllLevels`. And the skip in `enumerate` sits on a check that already runs
    and never fires under `all`.
  - It composes with every hasher, pruning value, expansion value, config and build mode, and
    declares no prerequisite.
- **Priority 6.**
  - Step 1 changes the plans of all three structures. It is a declared cross-cutting change.
  - Step 2 changes HashTrie, Hash Triejoin and the lifecycle's spec-to-header plumbing, which the
    sorted tries ignore. Their builds and joins are unchanged.
- **Priority 1.** The HashTrie aliases go from 8 to 16. Each of today's eight gets a twin with a
  `Keys` suffix: `HashTrieSipKeys`, `HashTrieFxKeys`, `HashTrieSipPrunedKeys`,
  `HashTrieFxPrunedKeys`, `HashTrieSipLazyKeys`, `HashTrieFxLazyKeys`,
  `HashTrieSipPrunedLazyKeys` and `HashTrieFxPrunedLazyKeys`.

## CLI, reports and kermit-lab

- **The flag.** `--ds-layout-levels all|keys` (default `all`) is accepted by `bench run`,
  `bench probe`, `bench join` and `kermit join` wherever a HashTrie cell runs. `bench ds` refuses
  it, as do selections with no HashTrie cell (through `DsFlag::structures`).
- **The axis.** `ds_layout_levels` (`"all"` / `"keys"`) appears on every HashTrie report.
- **The `index` metadata line** gains ` levels n` when the trie has fewer levels than columns.
- **The schema.** No schema bump. The version table gains two no-bump rows:
  - step 1: plans change for every structure, told apart by v6's `plan` axis;
  - step 2: the `ds_layout_levels` axis and the ` levels n` suffix.
- **kermit-lab** gets a `SCOPED_AXIS_DEFAULTS` entry `("ds_layout_levels", "HashTrie"): "all"`,
  which back-fills the rows that predate the axis. There is no `AXIS_PHASES` entry, since the
  level count changes `build`, `space` and `iteration` alike.

## Testing

**Step 1:**
- **New.** `every_optimiser_plans_payload_last`, in `kermit/tests/all_orders.rs`, beside the
  shapes. It prepares every shape the file holds under every `Optimiser` variant, and again under
  cost-based with a `state_budget` of 1, which forces its Cardinality fallback. It asserts that,
  in `Prepared`'s plan, every join key of the executed query precedes every payload variable.
- **New unit tests:**
  - `order_by_rank` puts the join keys first whatever their rank;
  - `analyse`'s `join_keys`: a repeated variable counts once, and a single atom has no join key;
  - cost-based's DP over join keys: the guard and the budget count keys, and the payload follows
    in canonical order.
- **Updated:**
  - the plans pinned in the unit tests of `lexicographic.rs`, `cardinality.rs`, `planner.rs` and
    `cost_based.rs`;
  - the five WatDiv pins in `kermit-algos/tests/cost_based_watdiv_plans.rs` (q0020, q0034,
    q0008, q0264, q0085), and its budget-fit checks;
  - cost-based's brute-force reference, which searches payload-last orders only;
  - the guard and budget tests (`more_than_64_variables_get_cardinalitys_plan`,
    `the_budget_counts_every_set_reached`), which count join keys;
  - the tests that pin `prepared.indexes()`: the lifecycle tests, `db.rs`'s and
    `subject_position_constant.rs`'s. A test that writes an `IndexSpec` literal gains its
    `levels`.

**Step 2:**
- **`kermit-algos`, `index_plan.rs`:**
  - under `All`, every spec's `levels` is its column count;
  - under `JoinKeys`, rule (iii) on payload-last plans and on plans with payload before a join
    key;
  - zero levels when there is no join key;
  - every column a join key;
  - `path3`: three specs under `JoinKeys`, two under `All`.
- **`kermit-ds`, a new `kermit-ds/tests/hash_trie_levels.rs`.** For headers at every level count
  from 0 to the arity, across every build mode, pruning value and expansion value:
  - the same contents as under `all` (`for_each_tuple`, `collect_tuples`);
  - leaf chains of whole rows;
  - `open` stops at `levels`;
  - at zero levels, `leaf_tuples` lends the whole chain at the pre-root, including an empty one;
  - Amendment 2 at partial levels: `bulk` and `incremental` array-identical, the radix family by
    `identity::assert_same_contents`, and `presized:N` by `assert_equivalent_trie`;
  - every mode array-identical at zero levels;
  - `insert` at partial and zero levels gives the bulk build's trie;
  - `KeyLevels` over a full-depth header builds exactly the `AllLevels` trie;
  - `levels()` through `Configured` and `BuiltWith`.

  In the unit tests, a size test checks that `Root` under `AllLevels` is the size of
  `HashTrieNode`, and another that the iterator does not grow under `AllLevels`.
- **The join suite, `kermit/tests/join_tests.rs`: 51 new invocations.**
  - The 8 `Keys` aliases, each running the 16 patterns under the 3 optimisers: 24 invocations.
  - One `Keys` alias per Config and BuildMode group, under the 3 optimisers, following the lazy
    expansion precedent: 27 invocations, as follows:
    - `HashTrieSipKeys` in HalfFull, PresizedRoot and SizedChildren;
    - `HashTrieSipPrunedLazyKeys` in PaperSizing, Radix2, HashIncremental and HashParallel2;
    - `Configured<HashTrieSipPrunedLazyKeys, PresizedRoot>` in HashPresized2;
    - `Configured<HashTrieSipPrunedLazyKeys, PaperSizing>` in the stacked group, under the same
      build mode as `HashTrieSipPrunedLazyPaper`. This is the closest-to-paper stack with `keys`
      added.
- **`all_orders.rs`.** Every shape runs under every order, under both `all` and `keys`: a
  `KeyLevels` cell sits beside the HashTrie one in `run_shapes`, `run_pinned` and
  `run_duplicated_constant_atom`. This amends #116's acceptance. The odd orders exercise rule
  (iii) and the skipped positions.
- **Real data.**
  - Today `watdiv_correctness.rs` and `lubm_mini_oracle.rs` (both run in CI) and
    `lubm_cardinalities.rs` (needs `java`) run only the sorted tries.
  - Each gains three HashTrie cells, run under every optimiser: `HashTrieSip`,
    `HashTrieSipKeys` and `HashTrieSipPrunedLazyKeys`.
  - Their helpers become generic over the `JoinEntry` they join through.
  - The reason: the suite's fixtures of 3 to 5 tuples rarely build long chains or deep pruning,
    and real data does. That is the LFTJ lesson in CLAUDE.md's optimiser recipe, step 5.
- **Allocation.**
  - `result_allocation.rs`'s blocks of the 8 aliases list the 8 `Keys` aliases too. So the
    zero-level and skipped-position paths are shown to stream without allocating per row.
  - `tuple_allocation.rs`'s HashTrie build cells gain `KeyLevels` twins. Each is built from a
    header with fewer levels than columns, and again at zero levels.
- **`kermit::db`:**
  - rule (iii) level counts through `prepare`;
  - exact sharing: `path3` gives 3 specs under `JoinKeys` and 2 under `All`;
  - `build` panics on a policy mismatch, in both directions;
  - an all-constant atom beside a zero-level index;
  - the entry points pass `R::TRIE_LEVELS`.
- **The binary.**
  - A new `kermit/tests/cli_hash_trie_layout_levels.rs`, beside
    `cli_hash_trie_layout_expansion.rs`:
    - `--ds-layout-levels keys` runs and reports `ds_layout_levels: "keys"`, and the default is
      `"all"`;
    - a payload query's `index` lines carry ` levels n` under `keys`, and none under `all`;
    - `bench ds --ds-layout-levels keys` and `-i tree-trie --ds-layout-levels keys` are refused;
    - `kermit join` writes the same rows under both, once sorted.
  - The `options.rs` round-trip tests cover `LevelsChoice` and `levels_of`.
  - `workload.rs`: a sweep with a sorted cell and a `keys` cell prepares each query twice, and
    `prepared(levels)` returns each policy's queries.
- **kermit-lab:** a back-fill test for `ds_layout_levels`.

## Measurement

None in #116 (Decision 7).
- The next sweep, #99, measures step 1, which every structure picks up, and `keys`, through new
  HashTrie cells in its harness.
- A v6 sweep run before #116 lands doubles as the before picture. One is prepared, but not
  launched, in `kermit-bench-runs/full-sweep-2026-10-27`, with its binary built at 2b4d385.
- Flipping the default to `keys` is a later amendment, the owner's call.

## Out of scope

- **Payload columns in the sorted tries.** They keep full tries (Decision 2).
- **`keys-shared`**, which is deferred and not filed (Decision 5).
- **Measurement, and the default flip** (Decision 7).
- **Projection pushdown:** dropping body-only payload columns that the head does not need. Under
  bag semantics they set the multiplicity, so they stay in the leaf rows.
- **H2O's per-query projections** (`x_id1`, …). They become unnecessary under `keys`, but they
  stay.

## Sequencing

0. **The prerequisite, which is the owner's.** v6 goes to origin, and this branch merges
   origin/master.
1. **Step 1:**
   - `join_keys`, `order_by_rank` and the cost-based DP;
   - the plan tests;
   - the optimiser docs;
   - the no-bump row for the plan change.
2. **`with_hash_trie_layout!` becomes a nested macro.** No behaviour changes.
3. **Level counts through the lifecycle:**
   - `TrieLevels`, `IndexSpec.levels`, `Relation::TRIE_LEVELS`;
   - `prepare`, `Prepared`, `RelationHeader`;
   - the mismatch panic and the `index` line.

   Every structure is still `All`, so nothing changes yet.
4. **HashTrie's `L` parameter:** the zero-level root, every build mode, the iterator and
   `levels()`.
5. **Hash Triejoin:** participation by level, and the skip.
6. **The binary and the joined tests:**
   - the flag, the cell, the families, and preparation per policy;
   - the aliases, and the tests that join under `keys`;
   - kermit-lab.
7. **Documentation** (below).

Each phase leaves the CI gate green.

## Acceptance

1. **Plans.** Every optimiser plans payload-last on every shape, its Cardinality fallback
   included (the new test), and `FixedOrder` still runs every order.
2. **Index specs.** Under `keys`, `prepare` gives each index its levels by rule (iii), and
   merges only identical specs. Under `all`, every spec's level count is its column count. So for
   a given plan, the specs, `index` lines, structures and footprints under `all` are those before
   step 2, which the size tests and the unchanged `index` lines show.
3. **Answers are unchanged:**
   - the join suite, with its 51 new invocations;
   - `all_orders`, under both policies;
   - the WatDiv and LUBM oracles, with their new HashTrie cells.
4. **Allocation.** The allocation tests pass with their `keys` cells.
5. **The flag.** `--ds-layout-levels` is accepted only where a HashTrie cell runs a join, and
   `bench ds` refuses it.
6. **The schema.**
   - `REPORT_SCHEMA_VERSION` stays 6.
   - `bench-report-schema.md` has the two no-bump rows and the axis row.
   - kermit-lab back-fills `all`.
7. **The CI gate passes:** test, clippy (`-Dwarnings`), fmt, doc (`-Dwarnings`) and python. So
   does the binary's rustdoc,
   `cargo rustdoc -p kermit --bin kermit -- --document-private-items -D warnings`.
8. **Documentation.** The documents below are updated.

## Documentation

- **`docs/data-structures/hash-trie.md`:**
  - Representation: the level count and the zero-level root;
  - Invariants: a trie's depth is its level count, and its leaf chains hold whole rows;
  - Complexity;
  - a "Join-key levels" subsection under Layout options: the flag, the axis, the aliases, rule
    (iii), zero levels and exact sharing;
  - the parity rows above.
- **`docs/algorithms/hash-triejoin.md`:** participation by level, skipped positions, the leaf
  product binding the payload, and the worked example.
- **`docs/optimisers/lexicographic.md`, `cardinality.md` and `cost-based.md`:** payload-last
  plans, step 1's claims, and cost-based's DP over join keys.
- **`docs/specs/bench-report-schema.md`:**
  - the `ds_layout_levels` axis row;
  - the `index` line's ` levels n`;
  - the two no-bump rows.
- **`docs/specs/optimization-standard.md`:** HashTrie's fourth Layout and its 16 aliases.
- **`CLAUDE.md`:**
  - Priorities item 1: 16 aliases;
  - Build Commands: a `--ds-layout-levels keys` line;
  - Key Trait Hierarchy:
    - `LevelPolicy`;
    - `Relation::TRIE_LEVELS`;
    - `HashTrieIterable::levels`;
    - the zero-level case of `leaf_tuples`;
    - payload-last plans under `QueryOptimiser`;
  - Testing Patterns;
  - the Gotchas on singleton pruning (the macro is nested, with 16 cells) and on index planning
    (`levels`, and exact sharing).
- **`ARCHITECTURE.md`:** planning, and the HashTrie and Hash Triejoin sections.
