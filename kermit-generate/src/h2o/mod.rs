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

mod keys;
mod rng;

pub use keys::KeyPlan;
use {
    crate::{
        generator::{Generated, Generator},
        h2o::{keys::KeyPool, rng::KeyColumn},
        sink::RelationSink,
    },
    kermit_bench::{H2oOrder, QueryDefinition},
    kermit_ds::{RelationHeader, Tuples},
    kermit_rdf::generator::Target,
    serde::{Deserialize, Serialize},
};

/// q1, q2 and q5: name, description and Datalog rule. `generate` takes
/// their counts in this order.
const QUERIES: [(&str, &str, &str); 3] = [
    (
        "q1",
        "H2O J1 q1 (small inner on int): few keys, each with many x rows",
        "Q(K, I) :- small(K), x_id1(K, I).",
    ),
    (
        "q2",
        "H2O J1 q2 (medium inner on int): thousands of keys, each with about a thousand x rows",
        "Q(K, I) :- medium(K), x_id2(K, I).",
    ),
    (
        "q5",
        "H2O J1 q5 (big inner on int): two dense key lists that overlap by 90%",
        "Q(K) :- big(K), x_id3(K).",
    ),
];

/// H2O's own `meta.json` fields, flattened beside the common ones.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct H2oExtras {
    /// The seed every stream was drawn from.
    pub seed: u32,
    /// The row order of every relation.
    pub order: H2oOrder,
    /// The cardinalities.
    pub plan: KeyPlan,
}

/// Generates an H2O join benchmark: six relations and the queries q1, q2 and
/// q5, each with the exact count it returns.
#[derive(Debug)]
pub struct H2oGenerator<'a> {
    target: Target<'a>,
    plan: KeyPlan,
    seed: u32,
    order: H2oOrder,
}

impl<'a> H2oGenerator<'a> {
    /// A generator of `plan`'s relations into `target`, drawn from `seed` and
    /// written in `order`.
    pub fn new(target: Target<'a>, plan: KeyPlan, seed: u32, order: H2oOrder) -> Self {
        Self {
            target,
            plan,
            seed,
            order,
        }
    }
}

impl Generator for H2oGenerator<'_> {
    type Extras = H2oExtras;

    const KIND: &'static str = "h2o";

    fn target(&self) -> Target<'_> { self.target }

    /// Writes `x_id3`, `big`, `x_id1`, `small`, `x_id2` and `medium`, in that
    /// order, and returns q1, q2 and q5 with their counts.
    ///
    /// One key column at a time, so that a column's pool and, once it is
    /// interleaved, its `x` column are dropped before the next is built:
    /// the peak is about 4N words (`x3`, `x1` and the 2N interleaved
    /// buffer). The counts are taken from the unsorted columns, and
    /// `x3` is cloned for `x_id3` because sorting that copy must not
    /// reorder the `id3` values that `x_id1` and `x_id2` are paired with.
    fn generate(&self, sink: &mut RelationSink) -> anyhow::Result<Generated<H2oExtras>> {
        let (rows, small, medium) = (self.plan.rows(), self.plan.small(), self.plan.medium());
        let (seed, order) = (self.seed, self.order);
        let pool = |column: KeyColumn| KeyPool::new(self.plan.cardinality(column), seed, column);

        // `id3`: n = rows, so x's column is a permutation. It is kept: it is
        // the second column of `x_id1` and `x_id2`.
        let pool3 = pool(KeyColumn::Id3);
        let x3 = pool3.x_column(rows);
        let q5 = pool3.matches(&x3);
        let big = pool3.right_table();
        drop(pool3);
        write(
            sink,
            order,
            "x_id3",
            &["id3"],
            Tuples::from_flat(1, rows, x3.clone()),
        )?;
        write(
            sink,
            order,
            "big",
            &["id3"],
            Tuples::from_flat(1, rows, big),
        )?;

        // `id1`.
        let pool1 = pool(KeyColumn::Id1);
        let x1 = pool1.x_column(rows);
        let q1 = pool1.matches(&x1);
        let small_table = pool1.right_table();
        drop(pool1);
        let x_id1 = Tuples::from_flat(2, rows, interleave(&x1, &x3));
        drop(x1);
        write(sink, order, "x_id1", &["id1", "id3"], x_id1)?;
        write(
            sink,
            order,
            "small",
            &["id1"],
            Tuples::from_flat(1, small, small_table),
        )?;

        // `id2`; `x3` is not needed after this.
        let pool2 = pool(KeyColumn::Id2);
        let x2 = pool2.x_column(rows);
        let q2 = pool2.matches(&x2);
        let medium_table = pool2.right_table();
        drop(pool2);
        let x_id2 = Tuples::from_flat(2, rows, interleave(&x2, &x3));
        drop(x2);
        drop(x3);
        write(sink, order, "x_id2", &["id2", "id3"], x_id2)?;
        write(
            sink,
            order,
            "medium",
            &["id2"],
            Tuples::from_flat(1, medium, medium_table),
        )?;

        let queries = QUERIES
            .iter()
            .zip([q1, q2, q5])
            .map(|(&(name, description, rule), expected)| QueryDefinition {
                name: name.to_string(),
                description: description.to_string(),
                query: rule.to_string(),
                expected: Some(expected),
            })
            .collect();
        Ok(Generated {
            description: format!(
                "H2O db-benchmark join (J1): N = {rows}, small = {small}, medium = {medium}, seed \
                 {seed}, {order} rows",
                order = order.name()
            ),
            queries,
            extras: H2oExtras {
                seed,
                order,
                plan: self.plan,
            },
        })
    }
}

/// Sorts `tuples` under `H2oOrder::Sorted`, then writes it as the relation
/// `name` with columns `attrs`.
fn write(
    sink: &mut RelationSink, order: H2oOrder, name: &str, attrs: &[&str], mut tuples: Tuples,
) -> anyhow::Result<()> {
    if order == H2oOrder::Sorted {
        tuples.sort();
    }
    let attrs = attrs.iter().map(|attr| (*attr).to_string()).collect();
    sink.write(&RelationHeader::new(name, attrs), &tuples)
}

/// The pairs `[left[i], right[i]]`, row after row, in one buffer sized
/// exactly for them.
///
/// # Panics
///
/// Panics if the slices differ in length.
fn interleave(left: &[usize], right: &[usize]) -> Vec<usize> {
    assert_eq!(
        left.len(),
        right.len(),
        "interleave: the columns differ in length"
    );
    let mut pairs = Vec::with_capacity(2 * left.len());
    for (&l, &r) in left.iter().zip(right) {
        pairs.push(l);
        pairs.push(r);
    }
    pairs
}
