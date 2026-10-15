//! Translating JOB queries into relation files and Datalog rules.
//!
//! A JOB query is `SELECT MIN(…), … FROM table AS alias, … WHERE c1 AND c2 …`:
//! a conjunction of conditions over aliased tables. `parse_query` reads one
//! with `sqlparser` and sorts each condition into a *join* (`a.x = b.y`
//! across two aliases) or a *filter* (a condition on one alias); anything
//! else is an error that names the query and the condition. `plan_template`
//! then merges the joins of one template's variants into variables and
//! writes the relation files and the Datalog rule of each query.
//!
//! Filtering one table commutes with an inner join, so no filter reaches a
//! rule: the generator applies a relation's filter while it writes the
//! relation, and the rules join the filtered relations. This module is pure:
//! it reads no data and starts no process.

use {
    sqlparser::{
        ast::{
            visit_expressions, visit_expressions_mut, BinaryOperator, Expr, Function, FunctionArg,
            FunctionArgExpr, FunctionArgumentList, FunctionArguments, GroupByExpr, Ident,
            ObjectName, ObjectNamePart, Select, SelectItem, SetExpr, Statement, TableFactor,
        },
        dialect::PostgreSqlDialect,
        parser::Parser,
    },
    std::{fmt, ops::ControlFlow},
};

/// One `alias.column` reference.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ColumnRef {
    /// The alias, as written in the `FROM` clause.
    pub alias: String,
    /// The column of the alias's table.
    pub column: String,
}

/// One `FROM` entry, `table AS alias`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Alias {
    /// The alias.
    pub name: String,
    /// The table it names.
    pub table: String,
}

/// A JOB query, parsed and its `WHERE` conjuncts classified.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobQuery {
    /// The query's name, `6a`.
    pub name: String,
    /// The `FROM` entries, in `FROM` order.
    pub aliases: Vec<Alias>,
    /// The join conditions in text order, each as written: `(left, right)`.
    pub joins: Vec<(ColumnRef, ColumnRef)>,
    /// The filters in text order: the alias, and the conjunct as `sqlparser`
    /// prints it with every `alias.` stripped from its columns.
    pub filters: Vec<(String, String)>,
    /// The statement with its projection replaced by `COUNT(*)`, printed
    /// without a trailing `;`. DuckDB's answer to it is the query's expected
    /// cardinality.
    pub count_sql: String,
}

/// Why a JOB query could not be translated. Every variant but `NameClash`
/// names the query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TranslateError {
    /// `sqlparser` rejected the text.
    Parse {
        /// The query's name.
        query: String,
        /// `sqlparser`'s error text.
        message: String,
    },
    /// The text is not exactly one plain `SELECT … FROM … WHERE …`.
    NotOneSelect {
        /// The query's name.
        query: String,
    },
    /// A `FROM` item that is not `table AS alias`.
    UnsupportedFrom {
        /// The query's name.
        query: String,
        /// The item as `sqlparser` prints it.
        item: String,
    },
    /// Two `FROM` items share an alias.
    DuplicateAlias {
        /// The query's name.
        query: String,
        /// The repeated alias.
        alias: String,
    },
    /// A condition names `x.column` with `x` not in `FROM`.
    UnknownAlias {
        /// The query's name.
        query: String,
        /// The unknown alias.
        alias: String,
    },
    /// A condition names two or more aliases and is not `a.x = b.y`.
    CrossAliasCondition {
        /// The query's name.
        query: String,
        /// The condition as `sqlparser` prints it.
        condition: String,
    },
    /// A condition names no alias.
    NoAliasCondition {
        /// The query's name.
        query: String,
        /// The condition as `sqlparser` prints it.
        condition: String,
    },
    /// An alias in no join condition (raised by `plan_template`).
    UnjoinedAlias {
        /// The query's name.
        query: String,
        /// The alias.
        alias: String,
    },
    /// Two relations of a template, or two variables of a query, share a name
    /// (raised by `plan_template`).
    NameClash {
        /// The shared name.
        name: String,
    },
}

impl fmt::Display for TranslateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            | TranslateError::Parse {
                query,
                message,
            } => write!(f, "query {query}: {message}"),
            | TranslateError::NotOneSelect {
                query,
            } => write!(
                f,
                "query {query}: expected one SELECT ... FROM ... WHERE ... statement without \
                 DISTINCT, GROUP BY, HAVING, ORDER BY, LIMIT or WITH"
            ),
            | TranslateError::UnsupportedFrom {
                query,
                item,
            } => {
                write!(
                    f,
                    "query {query}: FROM item `{item}` is not `table AS alias`"
                )
            },
            | TranslateError::DuplicateAlias {
                query,
                alias,
            } => {
                write!(f, "query {query}: alias `{alias}` appears twice in FROM")
            },
            | TranslateError::UnknownAlias {
                query,
                alias,
            } => write!(
                f,
                "query {query}: a condition names alias `{alias}`, which is not in FROM"
            ),
            | TranslateError::CrossAliasCondition {
                query,
                condition,
            } => write!(
                f,
                "query {query}: condition `{condition}` names several aliases but is not `a.x = \
                 b.y`"
            ),
            | TranslateError::NoAliasCondition {
                query,
                condition,
            } => {
                write!(f, "query {query}: condition `{condition}` names no alias")
            },
            | TranslateError::UnjoinedAlias {
                query,
                alias,
            } => {
                write!(f, "query {query}: alias `{alias}` is in no join condition")
            },
            | TranslateError::NameClash {
                name,
            } => {
                write!(f, "two relations or variables are both named `{name}`")
            },
        }
    }
}

impl std::error::Error for TranslateError {}

/// Parses one JOB query and sorts the conjuncts of its `WHERE` clause.
///
/// `name` is the query's name (`6a`); it is carried into the result and into
/// every error. The text must be exactly one plain `SELECT … FROM … WHERE …`
/// (no `DISTINCT`, `GROUP BY`, `HAVING`, `ORDER BY`, `LIMIT` or `WITH`) whose
/// `FROM` items are all `table AS alias`.
///
/// The `WHERE` clause is flattened through its top-level `AND`s, left before
/// right. A parenthesised conjunct stays whole (`(a OR b)` is one conjunct)
/// and so does `x BETWEEN lo AND hi`, which `sqlparser` parses as one node. A
/// conjunct is classified by the aliases of its `alias.column` references:
/// two different aliases compared by `=` is a join; exactly one alias is a
/// filter on it; anything else is an error.
///
/// # Errors
///
/// A [`TranslateError`] for text `sqlparser` rejects, for any shape above that
/// is not supported, for an alias used twice or not declared, and for a
/// conjunct that is neither a join nor a filter.
pub fn parse_query(name: &str, sql: &str) -> Result<JobQuery, TranslateError> {
    let statements =
        Parser::parse_sql(&PostgreSqlDialect {}, sql).map_err(|error| TranslateError::Parse {
            query: name.to_owned(),
            message: error.to_string(),
        })?;
    let Ok([mut statement]) = <[Statement; 1]>::try_from(statements) else {
        return Err(not_one_select(name));
    };
    let select = plain_select(name, &mut statement)?;
    let aliases = read_aliases(name, select)?;
    let Some(selection) = select.selection.as_ref() else {
        return Err(not_one_select(name));
    };
    let Conditions {
        joins,
        filters,
    } = classify(name, &aliases, selection)?;
    select.projection = vec![count_star()];
    Ok(JobQuery {
        name: name.to_owned(),
        aliases,
        joins,
        filters,
        count_sql: statement.to_string(),
    })
}

fn not_one_select(name: &str) -> TranslateError {
    TranslateError::NotOneSelect {
        query: name.to_owned(),
    }
}

/// The statement's one `SELECT`, if it is a plain one: no `WITH`, `ORDER BY`,
/// `LIMIT`, `DISTINCT`, `GROUP BY` or `HAVING`, and a `FROM` clause. Each
/// would change what `COUNT(*)` counts, and JOB uses none of them.
fn plain_select<'a>(
    name: &str, statement: &'a mut Statement,
) -> Result<&'a mut Select, TranslateError> {
    let Statement::Query(query) = statement else {
        return Err(not_one_select(name));
    };
    if query.with.is_some() || query.order_by.is_some() || query.limit_clause.is_some() {
        return Err(not_one_select(name));
    }
    let SetExpr::Select(select) = &mut *query.body else {
        return Err(not_one_select(name));
    };
    let ungrouped = matches!(
        &select.group_by,
        GroupByExpr::Expressions(expressions, modifiers)
            if expressions.is_empty() && modifiers.is_empty()
    );
    if select.from.is_empty() || select.distinct.is_some() || select.having.is_some() || !ungrouped
    {
        return Err(not_one_select(name));
    }
    Ok(&mut **select)
}

/// The `FROM` clause as `table AS alias` entries. A join, a subquery, a
/// table function, a schema-qualified name, an alias with column names or a
/// missing alias is `UnsupportedFrom`.
fn read_aliases(name: &str, select: &Select) -> Result<Vec<Alias>, TranslateError> {
    let mut aliases: Vec<Alias> = Vec::new();
    for item in &select.from {
        let unsupported = || TranslateError::UnsupportedFrom {
            query: name.to_owned(),
            item: item.to_string(),
        };
        if !item.joins.is_empty() {
            return Err(unsupported());
        }
        let TableFactor::Table {
            name: table_name,
            alias: Some(alias),
            args: None,
            ..
        } = &item.relation
        else {
            return Err(unsupported());
        };
        let [ObjectNamePart::Identifier(table)] = table_name.0.as_slice() else {
            return Err(unsupported());
        };
        if !alias.columns.is_empty() {
            return Err(unsupported());
        }
        if aliases.iter().any(|seen| seen.name == alias.name.value) {
            return Err(TranslateError::DuplicateAlias {
                query: name.to_owned(),
                alias: alias.name.value.clone(),
            });
        }
        aliases.push(Alias {
            name: alias.name.value.clone(),
            table: table.value.clone(),
        });
    }
    Ok(aliases)
}

/// The `WHERE` clause, sorted.
struct Conditions {
    joins: Vec<(ColumnRef, ColumnRef)>,
    filters: Vec<(String, String)>,
}

/// Sorts each conjunct of `selection` into a join or a filter on one alias.
fn classify(name: &str, aliases: &[Alias], selection: &Expr) -> Result<Conditions, TranslateError> {
    let mut conditions = Conditions {
        joins: Vec::new(),
        filters: Vec::new(),
    };
    for conjunct in conjuncts(selection) {
        let columns = column_refs(conjunct);
        if let Some(unknown) = columns
            .iter()
            .find(|column| !aliases.iter().any(|alias| alias.name == column.alias))
        {
            return Err(TranslateError::UnknownAlias {
                query: name.to_owned(),
                alias: unknown.alias.clone(),
            });
        }
        // The distinct aliases the conjunct names, in order of mention.
        let mut named: Vec<&str> = Vec::new();
        for column in &columns {
            if !named.contains(&column.alias.as_str()) {
                named.push(&column.alias);
            }
        }
        match named.as_slice() {
            | [] => {
                return Err(TranslateError::NoAliasCondition {
                    query: name.to_owned(),
                    condition: conjunct.to_string(),
                });
            },
            | [alias] => conditions
                .filters
                .push(((*alias).to_owned(), without_alias(conjunct))),
            | _ => match as_join(conjunct) {
                | Some(join) => conditions.joins.push(join),
                | None => {
                    return Err(TranslateError::CrossAliasCondition {
                        query: name.to_owned(),
                        condition: conjunct.to_string(),
                    });
                },
            },
        }
    }
    Ok(conditions)
}

/// The top-level `AND` tree of `expr` as a list, left before right. Only
/// `AND` is flattened: `Nested` (`(a OR b)`) and `Between` stay whole.
fn conjuncts(expr: &Expr) -> Vec<&Expr> {
    fn walk<'a>(expr: &'a Expr, out: &mut Vec<&'a Expr>) {
        match expr {
            | Expr::BinaryOp {
                left,
                op: BinaryOperator::And,
                right,
            } => {
                walk(left, out);
                walk(right, out);
            },
            | other => out.push(other),
        }
    }
    let mut out = Vec::new();
    walk(expr, &mut out);
    out
}

/// Every `alias.column` in `conjunct`, in text order. A reference with any
/// other number of parts (`column`, `schema.table.column`) is not one.
fn column_refs(conjunct: &Expr) -> Vec<ColumnRef> {
    let mut columns = Vec::new();
    let _ = visit_expressions(conjunct, |expr| {
        if let Expr::CompoundIdentifier(parts) = expr {
            if let [alias, column] = parts.as_slice() {
                columns.push(ColumnRef {
                    alias: alias.value.clone(),
                    column: column.value.clone(),
                });
            }
        }
        ControlFlow::<()>::Continue(())
    });
    columns
}

/// `a.x = b.y` as `(a.x, b.y)`, when `conjunct` is exactly that.
fn as_join(conjunct: &Expr) -> Option<(ColumnRef, ColumnRef)> {
    let Expr::BinaryOp {
        left,
        op: BinaryOperator::Eq,
        right,
    } = conjunct
    else {
        return None;
    };
    let (Expr::CompoundIdentifier(left_parts), Expr::CompoundIdentifier(right_parts)) =
        (&**left, &**right)
    else {
        return None;
    };
    let ([left_alias, left_column], [right_alias, right_column]) =
        (left_parts.as_slice(), right_parts.as_slice())
    else {
        return None;
    };
    Some((
        ColumnRef {
            alias: left_alias.value.clone(),
            column: left_column.value.clone(),
        },
        ColumnRef {
            alias: right_alias.value.clone(),
            column: right_column.value.clone(),
        },
    ))
}

/// `conjunct` as `sqlparser` prints it, with every `alias.column` rewritten
/// to `column` in the syntax tree (so a string literal is never touched).
fn without_alias(conjunct: &Expr) -> String {
    let mut stripped = conjunct.clone();
    let _ = visit_expressions_mut(&mut stripped, |expr| {
        let column = match expr {
            | Expr::CompoundIdentifier(parts) if parts.len() == 2 => parts.pop(),
            | _ => None,
        };
        if let Some(column) = column {
            *expr = Expr::Identifier(column);
        }
        ControlFlow::<()>::Continue(())
    });
    stripped.to_string()
}

/// The projection `COUNT(*)`.
fn count_star() -> SelectItem {
    SelectItem::UnnamedExpr(Expr::Function(Function {
        name: ObjectName::from(vec![Ident::new("COUNT")]),
        uses_odbc_syntax: false,
        parameters: FunctionArguments::None,
        args: FunctionArguments::List(FunctionArgumentList {
            duplicate_treatment: None,
            args: vec![FunctionArg::Unnamed(FunctionArgExpr::Wildcard)],
            clauses: vec![],
        }),
        filter: None,
        null_treatment: None,
        over: None,
        within_group: vec![],
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Parses `sql`, failing the test with the translator's own message.
    fn parse(name: &str, sql: &str) -> JobQuery {
        parse_query(name, sql).unwrap_or_else(|error| panic!("{error}"))
    }

    fn col(alias: &str, name: &str) -> ColumnRef {
        ColumnRef {
            alias: alias.to_owned(),
            column: name.to_owned(),
        }
    }

    fn alias(name: &str, table: &str) -> Alias {
        Alias {
            name: name.to_owned(),
            table: table.to_owned(),
        }
    }

    fn filter(alias: &str, text: &str) -> (String, String) { (alias.to_owned(), text.to_owned()) }

    #[test]
    fn between_is_one_filter() {
        let query = parse(
            "1a",
            "SELECT MIN(t.title) FROM title AS t, movie_keyword AS mk WHERE t.production_year \
             BETWEEN 1990 AND 2000 AND t.id = mk.movie_id",
        );
        assert_eq!(query.filters, vec![filter(
            "t",
            "production_year BETWEEN 1990 AND 2000"
        )]);
        assert_eq!(query.joins, vec![(col("t", "id"), col("mk", "movie_id"))]);
    }

    #[test]
    fn or_in_parentheses_is_one_filter() {
        let query = parse(
            "1a",
            "SELECT MIN(k.keyword) FROM keyword AS k, movie_keyword AS mk WHERE (k.keyword = 'a' \
             OR k.keyword = 'b') AND k.id = mk.keyword_id",
        );
        assert_eq!(query.filters, vec![filter(
            "k",
            "(keyword = 'a' OR keyword = 'b')"
        )]);
        assert_eq!(query.joins, vec![(col("k", "id"), col("mk", "keyword_id"))]);
    }

    #[test]
    fn quoted_and_and_dots_do_not_split() {
        // The second literal looks like a join and like a qualified column;
        // inside quotes it is neither, and it keeps its text.
        let query = parse(
            "1a",
            "SELECT MIN(mi.info) FROM movie_info AS mi, title AS t WHERE mi.info LIKE '%a.b AND \
             c%' AND mi.note LIKE '%t.id = mi.movie_id%' AND mi.movie_id = t.id",
        );
        assert_eq!(query.filters, vec![
            filter("mi", "info LIKE '%a.b AND c%'"),
            filter("mi", "note LIKE '%t.id = mi.movie_id%'"),
        ]);
        assert_eq!(query.joins, vec![(col("mi", "movie_id"), col("t", "id"))]);
    }

    #[test]
    fn like_not_like_in_is_null() {
        let query = parse(
            "1a",
            "SELECT MIN(n.name) FROM name AS n, cast_info AS ci WHERE n.name LIKE '%Ab%' AND \
             n.name NOT LIKE '%Cd%' AND ci.note IN ('(voice)', '(uncredited)') AND ci.note IS NOT \
             NULL AND n.gender IS NULL AND n.id = ci.person_id",
        );
        assert_eq!(query.filters, vec![
            filter("n", "name LIKE '%Ab%'"),
            filter("n", "name NOT LIKE '%Cd%'"),
            filter("ci", "note IN ('(voice)', '(uncredited)')"),
            filter("ci", "note IS NOT NULL"),
            filter("n", "gender IS NULL"),
        ]);
        assert_eq!(query.joins, vec![(col("n", "id"), col("ci", "person_id"))]);
    }

    #[test]
    fn self_join_aliases_are_distinct() {
        let query = parse(
            "1a",
            "SELECT MIN(mi1.info) FROM info_type AS it1, info_type AS it2, movie_info AS mi1, \
             movie_info AS mi2 WHERE it1.info = 'rating' AND it2.info = 'votes' AND \
             mi1.info_type_id = it1.id AND mi2.info_type_id = it2.id AND mi1.movie_id = \
             mi2.movie_id",
        );
        assert_eq!(query.aliases, vec![
            alias("it1", "info_type"),
            alias("it2", "info_type"),
            alias("mi1", "movie_info"),
            alias("mi2", "movie_info"),
        ]);
        assert_eq!(query.filters, vec![
            filter("it1", "info = 'rating'"),
            filter("it2", "info = 'votes'")
        ]);
        assert_eq!(query.joins, vec![
            (col("mi1", "info_type_id"), col("it1", "id")),
            (col("mi2", "info_type_id"), col("it2", "id")),
            (col("mi1", "movie_id"), col("mi2", "movie_id")),
        ]);
    }

    #[test]
    fn join_chain_is_three_joins() {
        // Conditions stay as written, whichever side the alias is on; merging
        // them into one variable is `plan_template`'s job.
        let query = parse(
            "1a",
            "SELECT MIN(t.title) FROM title AS t, movie_keyword AS mk, cast_info AS ci, \
             movie_info AS mi WHERE t.id = mk.movie_id AND mk.movie_id = ci.movie_id AND \
             mi.movie_id = ci.movie_id",
        );
        assert_eq!(query.joins, vec![
            (col("t", "id"), col("mk", "movie_id")),
            (col("mk", "movie_id"), col("ci", "movie_id")),
            (col("mi", "movie_id"), col("ci", "movie_id")),
        ]);
        assert!(query.filters.is_empty());
    }

    #[test]
    fn filter_text_strips_the_alias() {
        // `sqlparser` prints the conjunct, so whitespace is normalised and
        // `!=` becomes `<>`; the alias goes from every column, and only from
        // columns, never from a string literal.
        let query = parse(
            "1a",
            "SELECT MIN(t.title) FROM title AS t, movie_keyword AS mk WHERE t.title   !=\n    ''  \
             AND t.season_nr < t.episode_nr AND t.title LIKE 't.title%' AND t.id = mk.movie_id",
        );
        assert_eq!(query.filters, vec![
            filter("t", "title <> ''"),
            filter("t", "season_nr < episode_nr"),
            filter("t", "title LIKE 't.title%'"),
        ]);
    }

    #[test]
    fn count_sql_replaces_min() {
        let query = parse(
            "1a",
            "SELECT MIN(t.title) AS movie_title, MIN(n.name) AS actor FROM title AS t, name AS n, \
             cast_info AS ci WHERE t.production_year > 2000 AND t.id = ci.movie_id AND n.id = \
             ci.person_id;",
        );
        assert_eq!(
            query.count_sql,
            "SELECT COUNT(*) FROM title AS t, name AS n, cast_info AS ci WHERE t.production_year \
             > 2000 AND t.id = ci.movie_id AND n.id = ci.person_id"
        );
    }

    #[test]
    fn syntax_error_is_a_parse_error() {
        let error =
            parse_query("7b", "SELECT MIN(t.title) FROM title AS t WHERE (t.id = ").unwrap_err();
        match error {
            | TranslateError::Parse {
                query,
                message,
            } => {
                assert_eq!(query, "7b");
                assert!(!message.is_empty());
            },
            | other => panic!("expected a parse error, got {other:?}"),
        }
    }

    #[test]
    fn not_a_single_select_is_rejected() {
        let shapes = [
            "",
            "SELECT MIN(a.x) FROM t AS a WHERE a.x = 1; SELECT MIN(a.x) FROM t AS a WHERE a.x = 1",
            "SELECT MIN(a.x) FROM t AS a WHERE a.x = 1 UNION SELECT MIN(b.x) FROM t AS b WHERE \
             b.x = 1",
            "SELECT MIN(a.x) FROM t AS a",
            "SELECT 1 WHERE 1 = 1",
            "SELECT DISTINCT a.x FROM t AS a WHERE a.x = 1",
            "SELECT a.x FROM t AS a WHERE a.x = 1 GROUP BY a.x",
            "SELECT a.x FROM t AS a WHERE a.x = 1 ORDER BY a.x",
            "SELECT a.x FROM t AS a WHERE a.x = 1 LIMIT 5",
            "WITH c AS (SELECT 1) SELECT MIN(a.x) FROM t AS a WHERE a.x = 1",
            "SELECT MIN(a.x) FROM t AS a WHERE a.x = 1 HAVING MIN(a.x) > 0",
            "DROP TABLE t",
        ];
        for sql in shapes {
            assert_eq!(
                parse_query("2b", sql),
                Err(TranslateError::NotOneSelect {
                    query: "2b".to_owned(),
                }),
                "{sql}"
            );
        }
    }

    #[test]
    fn unsupported_from_items_are_rejected() {
        let cases = [
            ("SELECT MIN(a.x) FROM t, u AS a WHERE a.x = 1", "t"),
            (
                "SELECT MIN(a.x) FROM public.t AS a WHERE a.x = 1",
                "public.t AS a",
            ),
            (
                "SELECT MIN(a.x) FROM t AS a JOIN u AS b ON a.x = b.x WHERE a.x = 1",
                "t AS a",
            ),
            (
                "SELECT MIN(a.x) FROM (SELECT b.x FROM u AS b) AS a WHERE a.x = 1",
                "(SELECT",
            ),
            (
                "SELECT MIN(a.x) FROM t AS a (x, y) WHERE a.x = 1",
                "t AS a (x, y)",
            ),
            (
                "SELECT MIN(a.x) FROM generate_series(1, 3) AS a WHERE a.x = 1",
                "generate_series",
            ),
        ];
        for (sql, prefix) in cases {
            match parse_query("3c", sql) {
                | Err(TranslateError::UnsupportedFrom {
                    query,
                    item,
                }) => {
                    assert_eq!(query, "3c");
                    assert!(item.starts_with(prefix), "{sql}: item was {item:?}");
                },
                | other => panic!("{sql}: expected UnsupportedFrom, got {other:?}"),
            }
        }
    }

    #[test]
    fn duplicate_alias_is_rejected() {
        assert_eq!(
            parse_query(
                "4a",
                "SELECT MIN(t.x) FROM title AS t, name AS t WHERE t.x = 1"
            ),
            Err(TranslateError::DuplicateAlias {
                query: "4a".to_owned(),
                alias: "t".to_owned(),
            })
        );
    }

    #[test]
    fn unknown_alias_is_rejected() {
        let cases = [
            (
                "SELECT MIN(t.title) FROM title AS t, movie_keyword AS mk WHERE t.id = \
                 mk.movie_id AND zz.keyword = 'x'",
                "zz",
            ),
            (
                "SELECT MIN(t.title) FROM title AS t, movie_keyword AS mk WHERE t.id = \
                 ghost.movie_id",
                "ghost",
            ),
        ];
        for (sql, unknown) in cases {
            assert_eq!(
                parse_query("5a", sql),
                Err(TranslateError::UnknownAlias {
                    query: "5a".to_owned(),
                    alias: unknown.to_owned(),
                }),
                "{sql}"
            );
        }
    }

    #[test]
    fn cross_alias_condition_is_rejected() {
        // Each is printed back exactly as written. The last is a plain join in
        // parentheses: a `Nested` is never flattened, so it is not a join.
        let conditions = [
            "t.production_year > mk.movie_id",
            "(t.id = mk.movie_id OR t.id = ci.movie_id)",
            "t.id = mk.movie_id + 1",
            "(t.id = mk.movie_id)",
        ];
        for condition in conditions {
            let sql = format!(
                "SELECT MIN(t.title) FROM title AS t, movie_keyword AS mk, cast_info AS ci WHERE \
                 {condition}"
            );
            assert_eq!(
                parse_query("5c", &sql),
                Err(TranslateError::CrossAliasCondition {
                    query: "5c".to_owned(),
                    condition: condition.to_owned(),
                }),
                "{sql}"
            );
        }
    }

    #[test]
    fn condition_without_alias_is_rejected() {
        for condition in ["production_year > 2000", "1 = 1"] {
            let sql = format!(
                "SELECT MIN(t.title) FROM title AS t, movie_keyword AS mk WHERE t.id = \
                 mk.movie_id AND {condition}"
            );
            assert_eq!(
                parse_query("5d", &sql),
                Err(TranslateError::NoAliasCondition {
                    query: "5d".to_owned(),
                    condition: condition.to_owned(),
                }),
                "{sql}"
            );
        }
    }

    #[test]
    fn error_messages_name_the_query_and_the_item() {
        let cross = parse_query(
            "6a",
            "SELECT MIN(t.title) FROM title AS t, movie_keyword AS mk WHERE t.production_year > \
             mk.movie_id",
        )
        .unwrap_err();
        let errors = vec![
            (
                TranslateError::Parse {
                    query: "6a".to_owned(),
                    message: "boom".to_owned(),
                },
                vec!["6a", "boom"],
            ),
            (
                TranslateError::NotOneSelect {
                    query: "6a".to_owned(),
                },
                vec!["6a"],
            ),
            (
                TranslateError::UnsupportedFrom {
                    query: "6a".to_owned(),
                    item: "title".to_owned(),
                },
                vec!["6a", "`title`"],
            ),
            (
                TranslateError::DuplicateAlias {
                    query: "6a".to_owned(),
                    alias: "t".to_owned(),
                },
                vec!["6a", "`t`"],
            ),
            (
                TranslateError::UnknownAlias {
                    query: "6a".to_owned(),
                    alias: "zz".to_owned(),
                },
                vec!["6a", "`zz`"],
            ),
            (cross, vec!["6a", "`t.production_year > mk.movie_id`"]),
            (
                TranslateError::NoAliasCondition {
                    query: "6a".to_owned(),
                    condition: "1 = 1".to_owned(),
                },
                vec!["6a", "`1 = 1`"],
            ),
            (
                TranslateError::UnjoinedAlias {
                    query: "6a".to_owned(),
                    alias: "k".to_owned(),
                },
                vec!["6a", "`k`"],
            ),
            (
                TranslateError::NameClash {
                    name: "movie_keyword".to_owned(),
                },
                vec!["`movie_keyword`"],
            ),
        ];
        for (error, needles) in errors {
            let text = error.to_string();
            for needle in needles {
                assert!(text.contains(needle), "{text:?} lacks {needle:?}");
            }
            let _: &dyn std::error::Error = &error;
        }
    }
}
