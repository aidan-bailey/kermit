//! H2O: the join task ("J1") of H2O.ai's db-benchmark as a generated
//! benchmark (#95).
//!
//! db-benchmark is an open benchmark of dataframe and database engines. Its
//! join task builds a fact table `x` of N rows with three integer key columns
//! (`id1`, `id2`, `id3`) and three right-hand tables, `small` (N / 10⁶ keys),
//! `medium` (N / 10³ keys) and `big` (N keys), each sharing 90% of its keys
//! with the matching column of `x`. The data comes from the benchmark's
//! `_data/join-datagen.R`, which is distributed under the Mozilla Public
//! License 2.0. This module is a reimplementation of that generator's
//! structure, not a copy of its code: it keeps the key pools, their 90%
//! overlap and the way `x`'s column covers them, but draws from an in-repo
//! random number generator (so a dependency bump cannot change the data) and
//! splits the pools with integer arithmetic.
//!
//! Kermit joins integer relations, so the benchmark is six relations, each a
//! projection that one query needs (a query over a shared `x(id1, id2, id3)`
//! would descend its unused columns, because a `_` is still a trie level):
//!
//! | relation | columns | rows |
//! |---|---|---|
//! | `x_id3` | `id3` | N |
//! | `big` | `id3` | N |
//! | `x_id1` | `id1`, `id3` | N |
//! | `small` | `id1` | N / 10⁶ |
//! | `x_id2` | `id2`, `id3` | N |
//! | `medium` | `id2` | N / 10³ |
//!
//! and three of upstream's five join queries, each with its exact answer
//! count:
//!
//! | query | Datalog |
//! |---|---|
//! | `q1` | `Q(K, I) :- small(K), x_id1(K, I).` |
//! | `q2` | `Q(K, I) :- medium(K), x_id2(K, I).` |
//! | `q5` | `Q(K) :- big(K), x_id3(K).` |
//!
//! `x` carries its unique `id3` into `x_id1` and `x_id2`, so every result row
//! is distinct and each count is the same for every structure.

// Story 3 uses every item of these modules; until then they are dead outside
// the tests.
#[allow(dead_code)]
mod keys;
#[allow(dead_code)]
mod rng;

pub use keys::KeyPlan;
