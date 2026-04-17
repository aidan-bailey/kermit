# kermit-parser

Datalog query parser for the Kermit workspace. Parses rules of the form

```text
Head(…) :- Body1(…), Body2(…), …, BodyN(…).
```

into a [`JoinQuery`](src/join_query.rs) AST. Built on [winnow](https://docs.rs/winnow).

## Syntax

- **Variables** start with an uppercase ASCII letter: `X`, `Name`, `Var1`.
- **Atoms** (ground constants) start with a lowercase ASCII letter: `alice`, `edge`.
- **Placeholders** are the bare underscore `_`; they match any value without binding.

Identifiers after the first character may include ASCII alphanumerics and `_`.

## AST types

- [`JoinQuery`](src/join_query.rs) — `head: Predicate`, `body: Vec<Predicate>`.
- [`Predicate`](src/join_query.rs) — `name: String`, `terms: Vec<Term>`.
- [`Term`](src/join_query.rs) — `Var(String)`, `Atom(String)`, `Placeholder`.

Parse via `std::str::FromStr`:

```rust
use kermit_parser::JoinQuery;
let q: JoinQuery = "path(X, Z) :- edge(X, Y), edge(Y, Z).".parse().unwrap();
```

## Consumers

`kermit-algos` parses queries into variable orderings for Leapfrog Triejoin, and `kermit-bench` embeds queries in benchmark YAML definitions (see [`benchmarks/README.md`](../benchmarks/README.md)).
