//! The query shapes the oracle tests share: the 16 standard join patterns'
//! shapes and the constant cases, the seeded random relations they run over,
//! and the catalog helpers. `all_orders.rs` runs them under every variable
//! order, `materialize_oracle.rs` and `engine_oracle.rs` under each
//! optimiser, so every test reads the same cases.

use {
    super::oracle::{Lcg, Relations},
    kermit::db::{Catalog, Prepared},
    kermit_algos::{canonical_names, rewrite_placeholders, JoinQuery, StatisticsLevel},
    kermit_ds::{RelationHeader, Tuples},
    kermit_parser::Term,
};

/// The values in the random relations, and so the constants `c0`..`c2` the
/// shapes use.
pub const DOMAIN: usize = 3;

/// Seeds per shape. Each seed gives the shape's relations other tuples,
/// duplicates included.
pub const SEEDS: std::ops::Range<u64> = 0..6;

/// A query over relations `r0`, `r1`, …, with the arity of each.
pub struct Shape {
    pub name: &'static str,
    pub query: &'static str,
    pub arities: &'static [usize],
}

fn shape(name: &'static str, query: &'static str, arities: &'static [usize]) -> Shape {
    Shape {
        name,
        query,
        arities,
    }
}

/// The 16 standard patterns' shapes (those whose fixtures differ only in
/// data share a shape and keep their names), then the constant cases.
pub fn shapes() -> Vec<Shape> {
    vec![
        shape("unary", "Q(V0) :- r0(V0).", &[1]),
        shape(
            "triangle",
            "Q(V0, V1, V2) :- r0(V0, V1), r1(V1, V2), r2(V0, V2).",
            &[2, 2, 2],
        ),
        shape(
            "chain",
            "Q(V0, V1, V2, V3) :- r0(V0, V1), r1(V1, V2), r2(V2, V3).",
            &[2, 2, 2],
        ),
        shape("star", "Q(V0, V1, V2) :- r0(V0, V1), r1(V0, V2).", &[2, 2]),
        shape("selfjoin", "Q(V0, V1, V2) :- r0(V0, V1), r0(V1, V2).", &[2]),
        shape("existential", "Q(V0) :- r0(V0), r1(V0).", &[1, 1]),
        shape(
            "empty-result",
            "Q(V0, V1, V2) :- r0(V0, V1), r1(V1, V2).",
            &[2, 2],
        ),
        shape("single-relation", "Q(V0, V1) :- r0(V0, V1).", &[2]),
        shape(
            "four-way-chain",
            "Q(V0, V1, V2, V3, V4) :- r0(V0, V1), r1(V1, V2), r2(V2, V3), r3(V3, V4).",
            &[2, 2, 2, 2],
        ),
        shape(
            "wide-fanout",
            "Q(V0, V1, V2) :- r0(V0, V1), r1(V0, V2).",
            &[2, 2],
        ),
        shape("dead-end", "Q(V0, V1, V2) :- r0(V0, V1), r1(V1, V2).", &[
            2, 2,
        ]),
        shape(
            "deep-dead-end",
            "Q(V0, V1, V2, V3) :- r0(V0, V1), r1(V1, V2), r2(V2, V3).",
            &[2, 2, 2],
        ),
        shape("diagonal", "Q(V0) :- r0(V0, V0).", &[2]),
        shape(
            "repeated-nonadjacent",
            "Q(V0, V1) :- r0(V0, V1, V0), r1(V1).",
            &[3, 1],
        ),
        shape("trailing-placeholder", "Q(V0) :- r0(V0, _), r1(V0).", &[
            2, 1,
        ]),
        shape(
            "middle-placeholder",
            "Q(V0, V2) :- r0(V0, _, V2), r1(V2).",
            &[3, 1],
        ),
        shape("constant-object", "Q(X) :- r0(X, c1).", &[2]),
        shape("constant-with-repeat", "Q(X) :- r0(X, X, c1).", &[3]),
        shape(
            "constant-in-a-join",
            "Q(X, Z) :- r0(X, Y), r1(Y, c2), r2(Y, Z).",
            &[2, 2, 2],
        ),
        shape("constant-subject", "Q(Y) :- r0(c1, Y), r1(Y).", &[2, 1]),
        shape("atom-of-only-constants", "Q(X) :- r0(X), r1(c0, c1).", &[
            1, 2,
        ]),
        shape(
            "mixed-constants-and-repeats",
            "Q(X, Z) :- r0(X, c1, Y), r1(Y, Z, Z), r2(c0, c1).",
            &[3, 3, 2],
        ),
    ]
}

/// One relation per arity, `r0`, `r1`, …, each with 8–12 tuples drawn from
/// `0..DOMAIN`, so duplicates are likely and joins are dense.
pub fn random_data(rng: &mut Lcg, arities: &[usize]) -> Relations {
    arities
        .iter()
        .enumerate()
        .map(|(i, &arity)| {
            let tuples = (0..8 + rng.below(5))
                .map(|_| (0..arity).map(|_| rng.below(DOMAIN)).collect())
                .collect();
            (format!("r{i}"), arity, tuples)
        })
        .collect()
}

/// `relations` as a catalog at the tuple-count level, which is all
/// `FixedOrder` reads. Each relation is built row by row, so an empty one
/// still has its arity.
pub fn catalog_for(relations: &Relations) -> Catalog {
    catalog_at(relations, StatisticsLevel::TupleCounts)
}

/// A catalog of `relations` with the statistics `level` asks for.
pub fn catalog_at(relations: &Relations, level: StatisticsLevel) -> Catalog {
    Catalog::new(
        relations
            .iter()
            .map(|(name, arity, tuples)| {
                let mut flat = Tuples::new(*arity);
                for tuple in tuples {
                    flat.push(tuple);
                }
                (RelationHeader::new_positional(name.as_str(), *arity), flat)
            })
            .collect(),
        level,
    )
}

/// `query` after `rewrite_placeholders`, with every variable in the head, in
/// `prepared`'s canonical order (`canonical_names(prepared.query())`): the
/// full conjunctive query, whose oracle rows compare with
/// `execute_assignments`' and every engine's rows as they come, with no
/// permutation. The oracle's rows for it have bag semantics; their distinct
/// rows are the same set under set and bag semantics.
///
/// The head rests on one premise: `rewrite_placeholders` is deterministic
/// (`rewrite_placeholders` in `kermit-algos/src/placeholder_rewrite.rs` numbers
/// each `_` from one counter, past the query's highest `K<n>`), so a `_` gets
/// the same `K<n>` here as in the call `prepare` made, and the names in the
/// executed query's canonical order are the rewritten body's own.
pub fn full_query(query: &JoinQuery, prepared: &Prepared) -> JoinQuery {
    let mut full = rewrite_placeholders(query.clone());
    full.head.terms = canonical_names(prepared.query())
        .into_iter()
        .map(Term::Var)
        .collect();
    full
}
