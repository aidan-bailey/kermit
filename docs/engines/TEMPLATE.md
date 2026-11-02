# `<EngineName>`

> **Status:** [stable | experimental] · **CLI:** `--engine <name>` · **Feature:** `engine-<name>` · **Implementation:** [`kermit::engine::<module>`](../../kermit/src/engine/<module>.rs)

One paragraph: what the engine is, what it is compared against and why (the thesis question it serves,
RQ1.4: a bounded sanity baseline against an established peer engine, not a head-to-head claim).

## Phase mapping

How the engine fills kermit's phases. Every engine answers the full conjunctive query (every
variable of the executed query), from the same `Prepared` the structure cells run.

| Phase | What the engine does | Timed | Notes |
|---|---|---|---|
| `load` | … | no | … |
| `build` | … | yes | what is copied, and how many times (kermit's cells copy once) |
| `iteration` | … or "not offered" | yes | … |
| `materialize` | … | yes | … |
| `space` | … or "not reported" | exact | … |

State the `Engine` constants: `NAME` (the `system` axis), `STREAMS`, `KEEPS_DUPLICATES`,
`MATERIALIZE_MUTATES`, `HAS_SPACE`.

## Translation

How `Prepared` (the executed query over index ids `#k`, each index's `IndexSpec`, the plan) becomes
the engine's input: one statement or plan, with a worked example for the triangle under the plan
`X, Y, Z`.

## Fairness caveats

Where the engine's work sits differently from kermit's, so that a reader knows which pairs compare
like with like and which do not (see `BENCHMARKING.md`, "Comparing against external engines"):
- which phase holds the engine's index work;
- set or bag semantics, and what that does to counts on inputs with duplicate rows;
- who chooses the join order;
- threads, memory limits and anything the engine does outside kermit's timers;
- a query with an all-constant atom whose count is 0: the engine cell skips its build entirely there
  (the result is known to be empty) while kermit's `build` still builds every index, so say how
  that makes the engine's `build` not like-for-like;
- the `plan` axis: engine rows carry kermit's variable order, so say whether the engine runs it.

## Version pin

The version that runs, where it is pinned, what `engine_version` records, and **how to move the
pin** (every file and constant to change, and the commands that prove the move).

## Settings and limits

The engine's `Settings`, its CLI flags and their bounds, and the memory or disk limits kermit sets
on it.

## Tests

The engine's cells in `kermit/tests/engine_oracle.rs`, its unit tests, and the CI job that runs
them.

## Acceptance probes

One table per probed query, from `bench probe … --engine … --materialize`: the engine's single-run
times.

## See also

- Sibling pages in `docs/engines/`.
- The design: [`docs/specs/2026-11-02-external-engines-design.md`](../specs/2026-11-02-external-engines-design.md).
- The report schema's v8 axes: [`docs/specs/bench-report-schema.md`](../specs/bench-report-schema.md).
