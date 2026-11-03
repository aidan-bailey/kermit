//! The SQL translator that every SQL engine shares.
//!
//! It reads the executed query that `prepare` leaves and its index specs.
//! `extract_accesses` and `plan_indexes` guarantee the shape: every atom names
//! an index id `#k`, holds one variable per column in index-column order, and
//! holds no constant, placeholder or repeated variable. Those became filters on
//! the specs.
//!
//! The query becomes two kinds of statement:
//! - [`create_spec_table`], one per spec: a temporary table
//!   `idx_{index}_{build}` of the spec's filtered, projected rows, its columns
//!   named `c0…` in output order. The base relations are tables
//!   `rel_{relation}` with columns `c0…`, which the engine loads before it
//!   builds.
//! - [`join_select`], one per query: the join over the spec tables. Atom `i`
//!   becomes the alias `a{i}` and its `j`-th variable the column `c{j}` of it.
//!   Every variable is selected once, at its first binding, in the executed
//!   query's canonical order (`canonical_names(prepared.query())`, the order
//!   kermit's executors emit); every later binding is an equality with the
//!   first; an atom that shares no variable with the atoms before it joins by
//!   `CROSS JOIN`.
//!
//! The build suffix keeps the tables of one build apart from another's, so the
//! describe step's set and a timed build can coexist in one database.
//! Identifiers are double-quoted. DuckDB (like SQL generally) compares even
//! quoted identifiers without regard to case, so two relations whose names
//! differ only in case would share a table: the load then fails with the
//! database's "already exists" error rather than reading wrong rows.

use {
    crate::db::Prepared,
    kermit_algos::{canonical_names, IndexSpec},
    kermit_parser::Term,
    std::collections::HashMap,
};

/// `name` as a double-quoted SQL identifier, any `"` inside it doubled.
pub fn quote_identifier(name: &str) -> String { format!("\"{}\"", name.replace('"', "\"\"")) }

/// The unquoted name of the base table that holds `relation`: `rel_{relation}`.
/// An appender takes a table name raw, so it needs this form; a statement needs
/// [`relation_table`].
pub fn relation_table_name(relation: &str) -> String { format!("rel_{relation}") }

/// The quoted base table that holds `relation`.
pub fn relation_table(relation: &str) -> String { quote_identifier(&relation_table_name(relation)) }

/// The quoted table of spec `index` in build `build`: `idx_{index}_{build}`.
pub fn spec_table(index: usize, build: u64) -> String {
    quote_identifier(&format!("idx_{index}_{build}"))
}

/// The quoted name of positional column `index`: `c{index}`. The base tables,
/// the spec tables and the join all name their columns this way.
pub fn column(index: usize) -> String { quote_identifier(&format!("c{index}")) }

/// `CREATE TEMP TABLE` for spec number `index` of build `build`: the spec's
/// columns, renamed to `c0…` in output order, of the rows of its relation that
/// its filter admits. Constants become `c{c} = {value}` and equalities `c{a} =
/// c{b}`, joined by `AND`; an empty filter has no `WHERE`. Every source column
/// is qualified by its table.
///
/// `spec.levels` is ignored: engines prepare under `TrieLevels::All`, where it
/// equals the number of columns.
pub fn create_spec_table(spec: &IndexSpec, index: usize, build: u64) -> String {
    // A source column is qualified by its table, since an output name can equal
    // another source column's name (`"c1" AS "c0"`): the reference then never
    // depends on an engine's alias rules.
    let table = relation_table(&spec.relation);
    let source = |position: usize| format!("{table}.{}", column(position));
    let columns: Vec<String> = spec
        .columns
        .iter()
        .enumerate()
        .map(|(output, &from)| format!("{} AS {}", source(from), column(output)))
        .collect();
    let constants = spec
        .filter
        .constants()
        .iter()
        .map(|&(from, value)| format!("{} = {value}", source(from)));
    let equalities = spec
        .filter
        .equalities()
        .iter()
        .map(|&(earlier, later)| format!("{} = {}", source(earlier), source(later)));
    let conditions: Vec<String> = constants.chain(equalities).collect();
    let filter = if conditions.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", conditions.join(" AND "))
    };
    format!(
        "CREATE TEMP TABLE {} AS SELECT {} FROM {table}{filter}",
        spec_table(index, build),
        columns.join(", "),
    )
}

/// The join of `prepared`'s executed query over the spec tables of build
/// `build`, as one `SELECT` (clauses on separate lines, no trailing semicolon).
///
/// Every variable of the executed query is selected once, in its canonical
/// order (`canonical_names(prepared.query())`), as
/// `a{alias}."c{column}" AS "{variable}"` at its first binding, so a row is one
/// full assignment. Each later binding of a variable is an equality with the
/// first, and an atom with no equality to the atoms before it is a `CROSS
/// JOIN`.
///
/// # Panics
///
/// If the executed query has no atom (a query of checks alone never reaches an
/// engine), or an atom holds anything but distinct variables (`prepare`
/// guarantees it does not).
pub fn join_select(prepared: &Prepared, build: u64) -> String {
    let query = prepared.query();
    assert!(
        !query.body.is_empty(),
        "join_select: a query of checks alone has no join to translate"
    );

    // Where each variable is first bound: (alias, column).
    let mut first: HashMap<&str, (usize, usize)> = HashMap::new();
    let mut clauses: Vec<String> = Vec::with_capacity(query.body.len());
    let atom_indexes = prepared.executed().atom_indexes();
    for (alias, atom) in query.body.iter().enumerate() {
        let table = spec_table(atom_indexes[alias], build);
        let mut conditions: Vec<String> = Vec::new();
        for (position, term) in atom.terms.iter().enumerate() {
            let Term::Var(name) = term else {
                panic!("join_select: `{atom}` holds a term that is not a variable");
            };
            match first.get(name.as_str()) {
                | Some(&(earlier, source)) => {
                    // An earlier binding in this very atom has `earlier ==
                    // alias`.
                    assert!(earlier < alias, "join_select: `{atom}` repeats a variable");
                    conditions.push(format!(
                        "a{alias}.{} = a{earlier}.{}",
                        column(position),
                        column(source)
                    ));
                },
                | None => {
                    first.insert(name.as_str(), (alias, position));
                },
            }
        }
        clauses.push(match (alias, conditions.is_empty()) {
            | (0, _) => format!("FROM {table} AS a0"),
            | (_, true) => format!("CROSS JOIN {table} AS a{alias}"),
            | (_, false) => format!("JOIN {table} AS a{alias} ON {}", conditions.join(" AND ")),
        });
    }

    let select: Vec<String> = canonical_names(query)
        .iter()
        .map(|name| {
            let &(alias, position) = first
                .get(name.as_str())
                .expect("every variable of the executed query is bound by an atom");
            format!(
                "a{alias}.{} AS {}",
                column(position),
                quote_identifier(name)
            )
        })
        .collect();
    format!("SELECT {}\n{}", select.join(", "), clauses.join("\n"))
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::db::{prepare, Catalog},
        kermit_algos::{JoinQuery, LexicographicOptimiser, Planner, StatisticsLevel},
        kermit_ds::{Filter, RelationHeader, TrieLevels, Tuples},
    };

    /// A catalog of empty relations, each `(name, arity)`.
    fn catalog(relations: &[(&str, usize)]) -> Catalog {
        Catalog::new(
            relations
                .iter()
                .map(|&(name, arity)| {
                    (
                        RelationHeader::new_positional(name, arity),
                        Tuples::new(arity),
                    )
                })
                .collect(),
            StatisticsLevel::TupleCounts,
        )
    }

    fn prepared(catalog: &Catalog, text: &str) -> Prepared {
        let query: JoinQuery = text.parse().expect("a well-formed query");
        let planner = Planner::new(Box::new(LexicographicOptimiser));
        prepare(catalog, &query, &planner, TrieLevels::All).expect("a valid query")
    }

    #[test]
    fn identifiers_are_double_quoted_with_embedded_quotes_doubled() {
        assert_eq!(quote_identifier("edge"), r#""edge""#);
        assert_eq!(quote_identifier(r#"a"b"#), r#""a""b""#);
        assert_eq!(quote_identifier(""), r#""""#);
    }

    #[test]
    fn tables_and_columns_have_fixed_names() {
        assert_eq!(relation_table_name("edge"), "rel_edge");
        assert_eq!(relation_table("edge"), r#""rel_edge""#);
        assert_eq!(spec_table(3, 12), r#""idx_3_12""#);
        assert_eq!(column(2), r#""c2""#);
    }

    #[test]
    fn an_unfiltered_spec_copies_its_columns_under_positional_names() {
        let spec = IndexSpec {
            relation: "e".to_string(),
            filter: Filter::none(),
            columns: vec![0, 1],
            levels: 2,
        };
        assert_eq!(
            create_spec_table(&spec, 0, 9),
            r#"CREATE TEMP TABLE "idx_0_9" AS SELECT "rel_e"."c0" AS "c0", "rel_e"."c1" AS "c1" FROM "rel_e""#
        );
    }

    /// `r(Y, X, c42)` under the plan `X, Y`: the index keeps columns 1 and 0,
    /// in that order, under the names `c0` and `c1`, and only the rows whose
    /// column 2 holds 42.
    #[test]
    fn a_reordered_spec_with_a_constant_renames_its_columns_into_output_order() {
        let catalog = catalog(&[("r", 3)]);
        let prepared = prepared(&catalog, "Q(X, Y) :- r(Y, X, c42).");
        assert_eq!(prepared.query().to_string(), "Q(X, Y) :- #0(X, Y).");
        assert_eq!(
            create_spec_table(&prepared.indexes()[0], 0, 5),
            r#"CREATE TEMP TABLE "idx_0_5" AS SELECT "rel_r"."c1" AS "c0", "rel_r"."c0" AS "c1" FROM "rel_r" WHERE "rel_r"."c2" = 42"#
        );
    }

    #[test]
    fn a_repeated_variable_is_an_equality_and_beside_a_constant_follows_it() {
        let catalog = catalog(&[("r", 2), ("t", 3)]);
        let diagonal = prepared(&catalog, "Q(X) :- r(X, X).");
        assert_eq!(diagonal.query().to_string(), "Q(X) :- #0(X).");
        assert_eq!(
            create_spec_table(&diagonal.indexes()[0], 0, 1),
            r#"CREATE TEMP TABLE "idx_0_1" AS SELECT "rel_r"."c0" AS "c0" FROM "rel_r" WHERE "rel_r"."c0" = "rel_r"."c1""#
        );
        let both = prepared(&catalog, "Q(X) :- t(X, c5, X).");
        assert_eq!(both.query().to_string(), "Q(X) :- #0(X).");
        assert_eq!(
            create_spec_table(&both.indexes()[0], 0, 1),
            r#"CREATE TEMP TABLE "idx_0_1" AS SELECT "rel_t"."c0" AS "c0" FROM "rel_t" WHERE "rel_t"."c1" = 5 AND "rel_t"."c0" = "rel_t"."c2""#
        );
    }

    #[test]
    fn every_condition_of_a_filter_is_joined_with_and() {
        let spec = IndexSpec {
            relation: "t".to_string(),
            filter: Filter::new(vec![(1, 5), (3, 7)], vec![(0, 2)]),
            columns: vec![2, 0],
            levels: 2,
        };
        assert_eq!(
            create_spec_table(&spec, 4, 1),
            r#"CREATE TEMP TABLE "idx_4_1" AS SELECT "rel_t"."c2" AS "c0", "rel_t"."c0" AS "c1" FROM "rel_t" WHERE "rel_t"."c1" = 5 AND "rel_t"."c3" = 7 AND "rel_t"."c0" = "rel_t"."c2""#
        );
    }

    /// All three atoms read one merged spec, through three aliases. Each later
    /// binding of a variable is an equality with its first.
    #[test]
    fn a_self_join_reads_one_merged_spec_through_several_aliases() {
        let catalog = catalog(&[("e", 2)]);
        let prepared = prepared(&catalog, "Q(X, Y, Z) :- e(X, Y), e(Y, Z), e(X, Z).");
        assert_eq!(
            prepared.query().to_string(),
            "Q(X, Y, Z) :- #0(X, Y), #0(Y, Z), #0(X, Z)."
        );
        assert_eq!(prepared.indexes().len(), 1);
        assert_eq!(
            join_select(&prepared, 7),
            [
                r#"SELECT a0."c0" AS "X", a0."c1" AS "Y", a1."c1" AS "Z""#,
                r#"FROM "idx_0_7" AS a0"#,
                r#"JOIN "idx_0_7" AS a1 ON a1."c0" = a0."c1""#,
                r#"JOIN "idx_0_7" AS a2 ON a2."c0" = a0."c0" AND a2."c1" = a1."c1""#,
            ]
            .join("\n")
        );
    }

    #[test]
    fn an_atom_that_shares_no_variable_cross_joins() {
        let catalog = catalog(&[("r", 1), ("s", 1)]);
        let prepared = prepared(&catalog, "Q(X, Y) :- r(X), s(Y).");
        assert_eq!(prepared.query().to_string(), "Q(X, Y) :- #0(X), #1(Y).");
        assert_eq!(
            join_select(&prepared, 3),
            [
                r#"SELECT a0."c0" AS "X", a1."c0" AS "Y""#,
                r#"FROM "idx_0_3" AS a0"#,
                r#"CROSS JOIN "idx_1_3" AS a1"#,
            ]
            .join("\n")
        );
    }

    /// The third atom shares a variable with each side of the earlier `CROSS
    /// JOIN`, so its `ON` clause references both aliases.
    #[test]
    fn a_later_atom_joins_both_sides_of_an_earlier_cross_join() {
        let catalog = catalog(&[("r", 1), ("s", 1), ("t", 2)]);
        let prepared = prepared(&catalog, "Q(X, Y) :- r(X), s(Y), t(X, Y).");
        assert_eq!(
            prepared.query().to_string(),
            "Q(X, Y) :- #0(X), #1(Y), #2(X, Y)."
        );
        assert_eq!(
            join_select(&prepared, 4),
            [
                r#"SELECT a0."c0" AS "X", a1."c0" AS "Y""#,
                r#"FROM "idx_0_4" AS a0"#,
                r#"CROSS JOIN "idx_1_4" AS a1"#,
                r#"JOIN "idx_2_4" AS a2 ON a2."c0" = a0."c0" AND a2."c1" = a1."c0""#,
            ]
            .join("\n")
        );
    }

    /// `Y` is carried by both atoms and `X` by one, so the plan binds `Y` first
    /// and the first index reads its columns reversed. The select list still
    /// follows the canonical order of the executed query
    /// (`canonical_names(prepared.query())`): the head's `X`, then the
    /// body-only `Y`. The constant is a filter on the second spec.
    #[test]
    fn a_body_only_variable_is_selected_after_the_head_in_canonical_order() {
        let catalog = catalog(&[("r", 2), ("s", 2)]);
        let prepared = prepared(&catalog, "Q(X) :- r(X, Y), s(Y, c5).");
        assert_eq!(prepared.query().to_string(), "Q(X) :- #0(Y, X), #1(Y).");
        assert_eq!(
            create_spec_table(&prepared.indexes()[0], 0, 2),
            r#"CREATE TEMP TABLE "idx_0_2" AS SELECT "rel_r"."c1" AS "c0", "rel_r"."c0" AS "c1" FROM "rel_r""#
        );
        assert_eq!(
            create_spec_table(&prepared.indexes()[1], 1, 2),
            r#"CREATE TEMP TABLE "idx_1_2" AS SELECT "rel_s"."c0" AS "c0" FROM "rel_s" WHERE "rel_s"."c1" = 5"#
        );
        assert_eq!(
            join_select(&prepared, 2),
            [
                r#"SELECT a0."c1" AS "X", a0."c0" AS "Y""#,
                r#"FROM "idx_0_2" AS a0"#,
                r#"JOIN "idx_1_2" AS a1 ON a1."c0" = a0."c0""#,
            ]
            .join("\n")
        );
    }

    /// The join must read exactly the tables the specs create, under the same
    /// build suffix.
    #[test]
    fn the_join_reads_the_tables_the_specs_create() {
        let catalog = catalog(&[("r", 2), ("s", 2)]);
        let prepared = prepared(&catalog, "Q(X) :- r(X, Y), s(Y, c5).");
        let join = join_select(&prepared, 11);
        assert_eq!(prepared.indexes().len(), 2);
        for (index, spec) in prepared.indexes().iter().enumerate() {
            let table = spec_table(index, 11);
            assert!(join.contains(&table), "{join} does not read {table}");
            assert!(
                create_spec_table(spec, index, 11)
                    .starts_with(&format!("CREATE TEMP TABLE {table} AS ")),
                "spec {index} creates another table than {table}"
            );
        }
    }

    #[test]
    #[should_panic(expected = "a query of checks alone has no join to translate")]
    fn a_query_of_checks_alone_has_nothing_to_translate() {
        let catalog = catalog(&[("s", 2)]);
        let mut query: JoinQuery = "Q(X) :- s(c7, c8).".parse().expect("a well-formed query");
        query.head.terms.clear();
        let planner = Planner::new(Box::new(LexicographicOptimiser));
        let prepared =
            prepare(&catalog, &query, &planner, TrieLevels::All).expect("a query of checks");
        join_select(&prepared, 0);
    }
}
