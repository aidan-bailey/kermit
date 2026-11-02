# Binary Hash Join: a Left-Deep Baseline over a Keyed Hash Index

**Date:** 2026-11-02
**Status:** Design approved in conversation: decision by decision, then section by section
(2026-11-02).
**Scope:** One new `(structure, algorithm)` cell, `-i hash-index -a binary-hash-join`, in a third
iterator family: a left-deep pipelined binary hash join over a hash index keyed on its leading
columns. The join order is derived from the optimiser's variable order by a new index-planning
policy, `TrieLevels::ProbeKeys`. No optimiser, no `QueryPlan` and no existing structure or algorithm
changes. The report schema stays at v6.
**Related:**
- Wang et al., "Free Join: Unifying Worst-Case Optimal and Traditional Joins", SIGMOD 2023
  (Proc. ACM Manag. Data 1(2), Art. 150):
  - §2.2: a left-deep linear plan `[R1, …, Rm]` is evaluated by pipelining; every relation after
    the first is a hash table keyed on the attributes it shares with the relations before it;
  - Example 2.2: the triangle under `[R, S, T]`: `S` keyed on `y`, `T` on `(x, z)`, nested loops
    over `R`, then the `S` probe, then the `T` probe. This is the reference pseudocode for
    `BinaryHashJoin`;
  - §1 and §3: both binary joins and Generic Join are nested loops of iterate-and-probe.
- Freitag et al., VLDB 2020, §1 and §2: binary join plans and the intermediate results that make
  them worse than worst-case optimal joins on cyclic queries. Their baseline is Umbra's binary hash
  joins.
- #110 (hybrid plans; option (a) asks for a binary hash join plus a hybrid planner) and its parent
  #105 (paper convergence). This design delivers the binary hash join half of option (a). The
  hybrid planner stays open.
- `docs/specs/2026-11-02-join-key-levels-design.md` (#116), which made each `IndexSpec` carry a
  level count and each structure declare the policy it is planned under (`Relation::TRIE_LEVELS`).
  This design adds a third policy.
- `docs/specs/2026-11-02-external-engines-design.md` (a8f7cc62, on `aidanb/state-of-kermit`, not on
  master), whose `Cell` refactor of `run_benchmark` wraps every `ExecutionFamily` in a `FamilyCell`.
  See "Interaction with the external-engines work".
- #99, the next sweep, which measures the new cell.

## Summary

kermit runs every query as a worst-case optimal join: LFTJ over the sorted tries, Hash Triejoin
over `HashTrie`. The thesis has no in-kermit measure of what that buys over the conventional
alternative, a binary join plan. This design adds that baseline as one more cell, built and timed
on the same boundaries as the other three (`build`, `iteration`, `end_to_end`, `space`), planned by
the same optimisers and checked by the same tests.

- **The algorithm** is the left-deep pipelined hash join of Free Join §2.2. The executed query's
  first atom is scanned, and every later atom is probed on the variables earlier atoms bound. Each
  match binds the atom's other columns and descends. Nothing intermediate is stored. The join does
  work proportional to the sum of its intermediate results: on the triangle, `|R ⋈ S|`. That is
  the gap to the AGM bound that worst-case optimal joins close.
- **The structure** is a textbook chained hash table. The view's rows are copied once, alongside a
  power-of-two directory of bucket heads and one link per row. It is keyed on the index's first
  `levels()` columns.
- **The join order** comes from the plan's variable order. Read left to right, each unbound
  variable joins the atom that binds it, and an atom whose variables are then all bound is probed
  at once. Index planning under `TrieLevels::ProbeKeys` applies that rule. It reorders the plan so
  each atom's probe key leads its columns, sets each spec's `levels` to that key's length, and
  lists the executed query's atoms in join order. The executor reads the body left to right and
  derives nothing.

## Decisions

1. **A left-deep pipelined hash join.** Rejected:
   - a materialising binary hash join: its intermediate batches allocate inside `iteration`, which
     breaks `result_allocation.rs`'s invariant, and `space` would not count them;
   - a sort-merge plan over the sorted tries: on one pair of atoms LFTJ already is a merge join, so
     it would measure "LFTJ, restricted", and it would couple the baseline to the sorted tries'
     seek Layout.

   The pipelined join is the baseline Freitag et al. measure against. It fits `join_for_each`'s
   streaming contract unchanged.
2. **The join order is derived from the optimiser's variable order.** Rejected:
   - an explicit join-order field on `QueryPlan` filled by a binary-join optimiser: this is #110(a)'s
     scope, and it touches every optimiser;
   - the body order as written: it is arbitrary, and `--optimiser` would do nothing for this cell.

   `QueryPlan` stays the only type that crosses from planner to executor. Every optimiser reaches
   the new cell unchanged. A later binary-join optimiser can be an ordinary `QueryOptimiser` whose
   variable order encodes its join order.

   Accepted cost: the binary plan is only as good as a variable order chosen for WCOJ execution. A
   dedicated binary optimiser might choose better orders.
3. **The join order travels in the executed query, not in the plan.** Under `ProbeKeys`,
   `plan_indexes` lists the executed body in join order, so the executor never re-derives it. The
   rule therefore lives in one place, and it can change later (for example, to read statistics)
   without touching the algorithm.
4. **The policy is a third `TrieLevels` variant, `ProbeKeys`.** A hash index's "levels" are its key
   columns: one composite level, then the payload in the rows. `IndexSpec::levels` and
   `RelationHeader::levels()` carry the key length with no new field. `build_with`'s
   policy check already refuses a query prepared under another policy. The name `TrieLevels` is
   kept; renaming it is out of scope.
5. **`HashIndex<H: HashStrategy = SipHashStrategy>`, with the hasher reusing
   `--ds-layout-hasher`.** HashTrie-against-HashIndex comparisons can hold the hasher fixed. The
   default is HashTrie's default.
6. **No Config and no BuildMode axes.** The baseline has one build, with a fixed sizing rule.
7. **`KEEPS_DUPLICATES = true`.** Each duplicate row is lent separately, which is bag semantics, so
   the cell's counts equal HashTrie's on every query, the all-constant multiplicity included.
8. **No statistics-aware driver choice.** The rule reads only the query and the plan. The
   optimiser's influence is the variable order. Umbra-style "scan the largest, hash the rest" is
   deferred.

## Background: index planning today

`kermit::db::prepare` plans one query for one policy (`kermit-algos/src/index_plan.rs`,
`plan_indexes(accesses, plan, levels)`):
- each access's kept columns are sorted by the plan position of their variables;
- `levels` decides each spec's level count (`All`: every column; `JoinKeys`: up to the last
  join-key column, #116);
- identical specs merge into one index, and specs get ids `#k` by first appearance;
- the executed query names those ids in access order, each atom's variables in index-column order,
  and the plan is translated to its numbering by variable name.

`QueryPlan::validate` (`kermit-algos/src/optimiser/plan.rs`) checks that the ordering is a
permutation and that every atom's variables appear in plan order. `build_with`
(`kermit/src/db/lifecycle.rs`) asserts `prepared.levels == R::TRIE_LEVELS`. `bench run` prepares
once per policy its cells use (`Workload::prepare(planner, levels)`).

## Design

### The join order (`kermit-algos/src/join_order.rs`, new)

A family-agnostic module, beside `index_plan.rs`. It reads only the access query (each atom's
variables) and the plan `V`, never data.

```
bound ← ∅;  order ← []
while some atom is unjoined:
    u ← the first variable of V not in bound
    a ← among the unjoined atoms containing u, the one with the most variables in bound
        (ties: the earliest in the body)
    join a;  bound ← bound ∪ vars(a)
    join, in body order, every unjoined atom whose variables are all in bound
        (pure filters, probed as soon as their key is complete)
V′ ← for each atom in order, its variables not bound before it joined, each group in V order
```

- An atom's **key** is its variables bound before it joins. In `V′` every key variable precedes
  the atom's new variables, so an index whose columns follow `V′` has its key first.
- When atoms remain, a variable remains unbound: every atom has a variable (checks were removed by
  access extraction), and an atom whose variables are all bound is joined by the filter step. So
  `u` always exists.
- The first atom's key is empty, and so is the key of an atom joined across a cross product. Both
  are the same case, an index with an empty key, probed with an empty key, which lends all of its
  rows.

The function returns the join order (atom indices into the access query) and `V′` (a `QueryPlan`
over the access query's numbering). Its doc reads the rule as "a variable order as a sequence of
binding events".

Worked examples:

| Query, plan `V` | Join order (key) | `V′` |
|---|---|---|
| `R(a,b), S(b,c), T(a,c)`, `[a,b,c]` | R (none), S (b), T (a,c) | `[a,b,c]` |
| `A(x,z), B(y,z)`, `[x,y,z]` | A (none), B (z) | `[x,z,y]`; B's columns become `(z,y)` |
| `R(a,b), S(c,d)`, `[a,c,b,d]` | R (none), S (none): a cross product | `[a,b,c,d]` |

### Index planning under `ProbeKeys` (`kermit-iters/src/trie_levels.rs`, `kermit-algos/src/index_plan.rs`)

`TrieLevels` gains a third variant:

```rust
/// Levels for the columns earlier atoms of the binary join order bind: the key each index is
/// probed on. Planning under it also reorders the plan and the executed query's body into join
/// order (`plan_indexes`). `HashIndex` is planned under it.
ProbeKeys,
```

`plan_indexes` under `ProbeKeys`:
1. computes the join order and `V′` from `accesses.query` and `plan`;
2. sorts each access's kept columns by `V′` position (the same code as today, given `V′`);
3. sets each spec's `levels` to the access's key length;
4. visits the accesses in join order, so spec ids, and the executed body, follow the join order,
   and `#0` is the driver;
5. returns `V′`, translated by name as today, as `IndexPlan.plan`.

`IndexSpec::levels`'s doc ("fewer only under `JoinKeys`") gains `ProbeKeys`: the access's key
length, `0` for the driver.

It passes `QueryPlan::validate` unchanged, since every atom lists its variables in `V′` order.
Identical specs still merge. A spec's `levels` is part of the comparison, so two reads of one
relation with different key lengths are two indexes.

Only under `ProbeKeys` does anything change:
- `IndexPlan.query`'s doc contract gains "under `ProbeKeys`, the body is in join order";
- the cell's report `plan` axis (`Prepared::variable_order`) shows `V′`, the order it ran.

`All` and `JoinKeys` plan exactly as today. `bench run` prepares up to three policies.

### `KeyedIterable` (`kermit-iters/src/keyed.rs`, new)

The family's trait. It is a parallel family, neither a `TrieIterable` nor a `HashTrieIterable`:
an index keyed on a column prefix, probed by exact match. Its doc follows the
`kermit-iters/src/hash_trie.rs` rationale.

```rust
pub trait KeyedIterable: JoinIterable {
    /// How many leading columns form the key (`RelationHeader::levels()` at build).
    fn key_len(&self) -> usize;
    /// Every stored row, in source order.
    fn scan(&self) -> impl Iterator<Item = &[usize]>;
    /// The stored rows whose first `key_len()` values equal `key`, in source order.
    /// `key.len() == key_len()`; with `key_len() == 0`, every row.
    fn probe<'a>(&'a self, key: &'a [usize]) -> impl Iterator<Item = &'a [usize]> + 'a;
}
```

Both iterators borrow the index and allocate nothing. The exact signatures (return-position
`impl Trait` or a named iterator type) are the plan's choice. The contract is the doc comments
above.

### `HashIndex<H>` (`kermit-ds/src/ds/hash_index/`, new)

`mod.rs`, `implementation.rs` and `hash_index_iter.rs`, per the recipe. It is the chained-directory
join hash table (Free Join §2.2's "hash table keyed on y"), without tag bits:

```
header:    RelationHeader
rows:      Tuples      // view.materialize(): one copy, in source order
key_len:   usize       // header.levels()
directory: Vec<RowId>  // `slots` bucket heads, NIL when empty; empty when key_len == 0
next:      Vec<RowId>  // per row, the next row of its bucket's chain; empty when key_len == 0
```

- **Sizing.** `slots = max(2, rows.next_power_of_two())`, so the load factor is at most 1.
- **Hashing.** The key hash folds `H::hash` over the key columns with FxHash's combine step,
  `h = (h.rotate_left(5) ^ H::hash(v)).wrapping_mul(0x517c_c1b7_2722_0a95)`, from `h = 0`. The
  bucket is the top `log2(slots)` bits of `h.wrapping_mul(0x9E37_79B9_7F4A_7C15)` (Fibonacci
  hashing). `HashTrie` uses the same multiply-then-take-top-bits idea in its own code. Neither
  structure depends on the other's (Priority 6).
- **Build (`from_view`).** Rows are linked from last to first, each prepended to its bucket's
  chain, so every chain is in source order. That is three allocations (`rows`, `directory`,
  `next`) and none per row (`tuple_allocation.rs`).
- **Probe.** It hashes `key`, walks the bucket's chain, and lends each row whose first `key_len`
  values equal `key`. Rows of other keys in the same bucket are skipped by that comparison. With
  `key_len == 0`, it lends every row.
- **`Relation`.** `TRIE_LEVELS = ProbeKeys`, `KEEPS_DUPLICATES = true`. `new` is empty. `insert`
  appends a row and links it, and doubles the directory with a rehash once rows outnumber slots.
  `from_view` is the only build that is benchmarked.
- **Other traits.** `Projectable` (rebuild from `scan`, keyed on every projected column),
  `HeapSize` (`rows + directory + next`), `Cardinality` (the row count, duplicates included) and
  `HasOptimizationAxes` (`ds_layout_hasher`).
- **Without a query.** `bench ds` builds from a header whose `levels()` is the arity, so the index
  is keyed on every column.

Register it in `kermit-ds/src/ds/mod.rs` with an `IndexStructure::HashIndex` variant.

### `BinaryHashJoin` (`kermit-algos/src/keyed/binary_hash_join.rs`, new)

The family directory `kermit-algos/src/keyed/` has a `mod.rs` that documents the family contract,
as `sorted/` and `hash/` do. Nothing in it refers to either of them. It implements
`JoinAlgo<DS: KeyedIterable>`:

```
join_for_each(plan, query, indexes, emit):
    validate plan against analyse(query)
    one step per body atom j, in body order (= join order):
        index_j, vars_j (its variables, in column order), k_j = index_j.key_len()
    assert: vars_j[..k_j] are bound by steps before j, and vars_j[k_j..] are not
    row ← scratch[num_vars];  key_j ← scratch[k_j]          // allocated once per join
    descend(0)

descend(j):
    if j = steps: emit(row); return
    key_j ← row[vars_j[..k_j]]
    for t in index_j.probe(key_j):
        row[vars_j[i]] ← t[i]   for i in k_j..arity_j
        descend(j + 1)
```

- The assertion is an invariant named in the doc, "probe keys lead the columns", in the role
  LFTJ's ascent assertion plays. It catches a query not planned under `ProbeKeys`, which
  `build_with`'s policy check also refuses one layer up.
- It emits every variable in canonical order. `kermit::db::execute` lends the head prefix as for
  every family.
- The recursion is as deep as there are atoms. Scratch is allocated once per join, never per
  result row (`result_allocation.rs`).
- Constants, repeated variables and all-constant atoms were handled by access extraction and
  `execute`'s multiplicity before the join, so it sees none of them.

Register it in `kermit-algos/src/keyed/mod.rs`, re-export it flat from `lib.rs`, and add
`JoinAlgorithm::BinaryHashJoin` (axis value `BinaryHashJoin`, CLI `binary-hash-join`).

### Entry points (`kermit/src/db.rs`)

`binary_join_for_each<R: Relation + KeyedIterable>` and `binary_join<R>`, thin wrappers over
`prepare` (under `R::TRIE_LEVELS`) → `build` → `execute::<R, BinaryHashJoin>`, beside
`hash_join_for_each` / `hash_join`.

### The binary (`kermit/src`)

- **`main.rs`.** `IndexStructureSelector::HashIndex` (`hash-index`) and
  `JoinAlgorithmSelector::BinaryHashJoin` (`binary-hash-join`), in their `expand()`, and a
  `write_query_csv` arm.
- **`execution.rs`.**
  - `Execution::IndexBhj { hasher }`, with `for_pair`, `for_structure`, `index_structure`,
    `algorithm` and `trie_levels() = ProbeKeys`.
  - `HashIndexFamily<H>` (`RelationFamily`, for `bench ds`) and `IndexBhj<H>` (`ExecutionFamily`,
    `JOIN_MUTATES = false`), delegating to the marker as the other families do.
  - `build_mode_axes` is empty.
  - `-i all -a all` runs four cells. The new incompatible pairs (`hash-index` with LFTJ or HTJ,
    and every other structure with `binary-hash-join`) are skipped with the existing stderr note,
    and a single explicit one is a usage error.
- **`options.rs`.**
  - `DsFlag::LayoutHasher`'s row in `DsFlag::structures` gains `HashIndex`; every other `--ds-*`
    flag on `hash-index` is refused by the existing validators.
  - A one-dimension `with_hash_index_layout!` sits beside `with_hash_trie_layout!` and
    `with_sorted_trie_layout!`.
- **`bench/run.rs`, `bench/ds.rs`.** `dispatch_run_bench` and `dispatch_ds_bench` arms.
- **Reports.** The report's `data_structure` / `algorithm` axes come from `F::execution()`, as
  for every cell.

### Interaction with the external-engines work

The external-engines spec (a8f7cc62, unmerged) wraps every `ExecutionFamily` in a `FamilyCell`, so
this cell needs no engine-side code. Its step 1 adds `execute_assignments` / `materialize` to the
existing cells. Whichever change lands second adds the other's piece: `materialize` on `IndexBhj`
if the engines work lands first, or `FamilyCell` coverage of `IndexBhj` if this one does. The two
baselines answer different questions. This one runs on kermit's planner and build boundaries;
DuckDB is a production engine with its own optimiser.

## CLI, reports and kermit-lab

- No new flags. `--ds-layout-hasher` now also reaches `hash-index`.
- No schema bump: the cell adds new axis values (`HashIndex`, `BinaryHashJoin`, and
  `ds_layout_hasher` on the new structure), and no metric changes meaning.
- Criterion groups: `{name}/{workload}/{query}/HashIndex/BinaryHashJoin`. The existing 64-byte
  collision guard (`check_sweep_group_directories`) covers it.
- kermit-lab must load a report whose structure has no `ds_build_mode` axis. The plan checks
  `BASELINE_BUILD_MODES` (`python/kermit-lab/kermit_lab/analysis.py`) and adds a case to its
  tests if one is missing.

## Testing

| Where | What |
|---|---|
| `kermit-algos` unit (`join_order.rs`) | On the 16 suite shapes and on every order of each: `V′` is a permutation; each atom joins once; each key is a prefix of the atom's `V′`-ordered variables; no atom joins before its key is bound; the chosen atom has the most bound variables among the unjoined atoms containing `u`. Plus the three worked examples, and a pinned case where a connected atom not containing `u` loses to an unconnected one that does (the rule follows `V`, so `V` can force a cross product, as it does for LFTJ). |
| `kermit-algos` unit (`index_plan.rs`) | Under `ProbeKeys`: each spec's `levels` equals its key length; the body is in join order with `#0` the driver; the returned plan validates; specs differing only in `levels` stay apart; `All` and `JoinKeys` output is unchanged. |
| `kermit-algos` unit (`binary_hash_join.rs`) | The invariant assertion panics when an index's key is not bound by the atoms before it (for example, indexes keyed on every column, as `bench ds` builds them). The triangle micro-example from the doc. |
| `kermit-ds` (`tests/hash_index_tests.rs`, new) | A `keyed_index_test_suite!(Type)`: `probe` lends exactly a key's rows, in source order; `key_len == 0` lends every row; an empty index; insert and rehash; duplicates are kept. A test-only `HashStrategy` that maps every value to one hash puts every row in one bucket, so the key comparison alone must filter. Plus `parquet_test_suite!(Type, collector)` and the `identity::` check that a build from a filtered, permuted view equals the build from its materialised batch. Invoked for `HashIndexSip` and `HashIndexFx`. |
| `kermit/tests/join_tests.rs` | `HashIndexSip` and `HashIndexFx` × `BinaryHashJoin` × Lexicographic, Cardinality and CostBased (six invocations), and a `JoinEntry` impl in `common/utils.rs`. |
| `kermit/tests/all_orders.rs` | A `HashIndex` cell. Every variable order, and therefore every join order the rule produces, is checked against the nested-loop oracle. |
| `kermit/tests/result_allocation.rs`, `tuple_allocation.rs` | One cell each. |
| `kermit::db` unit | All-constant multiplicity on `HashIndex` (it keeps duplicates); building a `ProbeKeys` query with a structure of another policy panics. |
| `kermit/tests/lubm_mini_oracle.rs`, `lubm_cardinalities.rs` | A `HashIndex` cell in each. |
| CLI | `cli_query_errors.rs` gains the cell; `cli_bench_run_sweep.rs` asserts four cells; a new `cli_hash_index_hasher_choice.rs` covers the hasher reaching the report and every other `--ds-*` flag refused; `execution.rs` unit tests (`for_structure` agrees with `for_pair`, the sweep's skipped pairs). |

## Measurement

None in this change, as with #116. The baseline joins the next sweep (#99) as its fourth cell. On
cyclic queries (SNAP triangle, cycle4, clique4; the cyclic WatDiv templates) a binary plan can be
orders of magnitude slower than the WCOJ cells. That is the baseline's point, and the sweep's
`bench probe` prescreen already guards against it.

## Out of scope

- Hybrid planning, binary and WCOJ joins in one plan (#110).
- A statistics-aware driver choice, and a dedicated binary-join optimiser.
- Bushy plans and materialised intermediates.
- Config and BuildMode axes for `HashIndex` (load factor, parallel build), and tag bits.
- Renaming `TrieLevels`.
- Running `BinaryHashJoin` over any other structure.

## Premises to verify first

1. Nothing downstream of `prepare` assumes the executed body lists atoms in access order (index
   ids, the report's `index` metadata lines, `Prepared::relations`, the CSV header). Only
   `ProbeKeys` reorders it.
2. `RelationHeader::with_levels(0)` is accepted (HashTrie's zero-level tries rely on it).
3. kermit-lab loads a structure with no `ds_build_mode` axis.
4. `lubm_mini_oracle.rs` and `lubm_cardinalities.rs` enumerate cells per structure, so a new cell
   is a new row.

## Sequencing

Each step leaves the gate green.

1. `join_order.rs`, `TrieLevels::ProbeKeys` (every exhaustive match updated), and `plan_indexes`
   under it, with their unit tests. No consumer yet.
2. `KeyedIterable`, `HashIndex`, its suite and `docs/data-structures/hash-index.md`.
3. `BinaryHashJoin`, the `db` entry points, and the join-suite, all-orders, allocation, LUBM and
   `db` cells, with `docs/algorithms/binary-hash-join.md`.
4. The binary's wiring: selectors, `Execution`, families, options, dispatch arms, and the CLI tests.
5. The docs sweep (`ARCHITECTURE.md`, `CLAUDE.md`) and the smoke.

## Acceptance

- The gate is green: `cargo test`, `cargo clippy --all-targets` under `-Dwarnings`,
  `cargo fmt --all --check` (inside `nix develop`), `cargo doc --workspace` under `-Dwarnings`,
  `cargo rustdoc -p kermit --bin kermit -- --document-private-items -D warnings`, and the
  `python` job.
- `cargo run -- bench run triangle -i all -a all -m iteration --verify` runs four cells, all
  verified.
- A smoke run, `bench probe --all -i all -a all` over the committed small benchmarks: every count
  with an `expected` is verified, and every `HashIndex` count equals `HashTrie`'s.
- A status comment for #110 and #105 is drafted for the user to post.

## Documentation

- `docs/algorithms/binary-hash-join.md` (from `TEMPLATE.md`): Free Join §2.2's pseudocode, the
  join-order rule, the "probe keys lead the columns" invariant, the complexity of `join_for_each`
  (the sum of intermediate results), and the triangle as a worked micro-example, with `|R ⋈ S|`
  against the AGM bound.
- `docs/data-structures/hash-index.md` (from `TEMPLATE.md`): the representation, the sizing and
  hashing rules, the invariants (chains in source order; `key_len == levels()`), the complexity of
  `from_view`, `probe` and `scan`, and the same triangle's `S` and `T` indexes as the
  micro-example.
- `ARCHITECTURE.md`: three iterator families, and `ProbeKeys` planning.
- `CLAUDE.md`:
  - "exactly 3 valid pairs" becomes 4;
  - the family lists in the workspace architecture and the trait hierarchy;
  - the `TrieLevels` entry;
  - the Testing Patterns aliases;
  - the reference-doc list;
  - a build command (`cargo run -- bench run triangle -i hash-index -a binary-hash-join`).
