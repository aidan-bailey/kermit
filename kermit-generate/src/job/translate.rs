//! Translating JOB queries into relation files and Datalog rules.
//!
//! A JOB query is `SELECT MIN(…), … FROM table [AS] alias, … WHERE c1 AND c2
//! …`: a conjunction of conditions over aliased tables. `parse_query` reads one
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

/// One `FROM` entry, `table [AS] alias`.
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
    /// The text is not exactly one plain `SELECT … FROM … WHERE …`: more or
    /// fewer than one statement, a set operation, a missing `WHERE`, a `WITH`,
    /// `DISTINCT`, `TOP`, `INTO`, `GROUP BY`, `HAVING`, `QUALIFY`, a named
    /// `WINDOW`, `ORDER BY`, `LIMIT`, `FETCH` or a locking clause (`FOR
    /// UPDATE`), or a subquery in the `WHERE` clause.
    NotOneSelect {
        /// The query's name.
        query: String,
    },
    /// A `FROM` item that is not `table [AS] alias`, or that samples the table
    /// (`TABLESAMPLE`).
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
                 DISTINCT, TOP, INTO, GROUP BY, HAVING, QUALIFY, WINDOW, ORDER BY, LIMIT, FETCH, \
                 locking clauses, WITH or subqueries"
            ),
            | TranslateError::UnsupportedFrom {
                query,
                item,
            } => {
                write!(
                    f,
                    "query {query}: FROM item `{item}` is not `table [AS] alias`"
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
/// (no `DISTINCT`, `TOP`, `INTO`, `GROUP BY`, `HAVING`, `QUALIFY`, named
/// `WINDOW`, `ORDER BY`, `LIMIT`, `FETCH`, locking clause, `WITH` or subquery
/// in the `WHERE` clause, anywhere inside a conjunct) whose `FROM` items are
/// all `table [AS] alias`, unsampled (no `TABLESAMPLE`).
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
/// `LIMIT`, `FETCH`, locking clause, `DISTINCT`, `TOP`, `INTO`, `GROUP BY`,
/// `HAVING`, `QUALIFY` or named `WINDOW`, and a `FROM` clause. Each would
/// change what `COUNT(*)` counts (or what the statement does), and JOB uses
/// none of them.
fn plain_select<'a>(
    name: &str, statement: &'a mut Statement,
) -> Result<&'a mut Select, TranslateError> {
    let Statement::Query(query) = statement else {
        return Err(not_one_select(name));
    };
    if query.with.is_some()
        || query.order_by.is_some()
        || query.limit_clause.is_some()
        || query.fetch.is_some()
        || !query.locks.is_empty()
    {
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
    if select.from.is_empty()
        || select.distinct.is_some()
        || select.top.is_some()
        || select.into.is_some()
        || select.having.is_some()
        || select.qualify.is_some()
        || !select.named_window.is_empty()
        || !ungrouped
    {
        return Err(not_one_select(name));
    }
    Ok(&mut **select)
}

/// The `FROM` clause as `table [AS] alias` entries. A join, a subquery, a
/// table function, a schema-qualified name, an alias with column names, a
/// missing alias or a `TABLESAMPLE` is `UnsupportedFrom`.
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
            sample: None,
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
        if has_subquery(conjunct) {
            return Err(not_one_select(name));
        }
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

/// Whether `conjunct` contains a subquery (`EXISTS (…)`, `x IN (SELECT …)` or
/// a scalar `(SELECT …)`) at any depth.
fn has_subquery(conjunct: &Expr) -> bool {
    visit_expressions(conjunct, |expr| match expr {
        | Expr::Subquery(_)
        | Expr::Exists {
            ..
        }
        | Expr::InSubquery {
            ..
        } => ControlFlow::Break(()),
        | _ => ControlFlow::Continue(()),
    })
    .is_break()
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

/// One relation file of a template.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelationPlan {
    /// The file and relation name: the table's name when unfiltered, else
    /// `{table}_{alias}_{letters}` (`keyword_k_ace`). The generator's sink
    /// requires `[a-z][a-z0-9_]*`, which holds for JOB's lowercase table and
    /// alias names; the sink rejects anything else.
    pub name: String,
    /// The IMDb table the rows come from.
    pub table: String,
    /// The columns in file order: the join columns in variable order, then
    /// `id` unless it is already a join column.
    pub columns: Vec<String>,
    /// How many leading columns are join columns; rows where one is NULL
    /// cannot join and are left out.
    pub join_columns: usize,
    /// The alias's filters joined by ` AND ` in text order, with the alias
    /// stripped from every column; empty when the relation is unfiltered.
    pub filter: String,
    /// The queries that use this relation, in input order.
    pub queries: Vec<String>,
}

/// One query's rule and its oracle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RulePlan {
    /// The query's name, `6a`.
    pub query: String,
    /// The Datalog rule, `Q(K, T, N) :- …`.
    pub datalog: String,
    /// The query with `COUNT(*)` in place of its projection: the oracle for
    /// the rule's cardinality.
    pub count_sql: String,
}

/// A template's relation files and its rules.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplatePlan {
    /// The distinct relation files, in order of first use (query order, then
    /// `FROM` order).
    pub relations: Vec<RelationPlan>,
    /// One rule per query, in input order.
    pub rules: Vec<RulePlan>,
}

/// Plans one template: `queries` are its variants in letter order.
///
/// **Variables.** Within a query, the joins' columns merge into variables
/// (union-find over `alias.column`). A variable's position is that of the
/// first join, in text order, that mentions one of its columns. It is named
/// after the first alias, in text order, whose `id` column it contains
/// (`k.id` gives `K`, `mi_idx1.id` gives `MI_IDX1`), or `ALIAS_COLUMN`
/// uppercased after its first column when it contains no `id`.
///
/// **Relations.** An alias's columns are its join columns in variable order,
/// then `id` unless `id` is already one of them. A relation is shared by
/// every alias, in every query of the template, with the same table, columns
/// and filter, whatever the aliases are called.
///
/// **Rules.** `Q(<every variable, in position order>) :- <one atom per alias,
/// in FROM order>.`, each atom the alias's relation with its variables as
/// terms and `_` for a carried `id`. An empty slice plans nothing.
///
/// # Errors
///
/// `UnjoinedAlias` for an alias in no join condition, and `NameClash` for two
/// variables of one query, or two relations of the template, with one name.
pub fn plan_template(queries: &[JobQuery]) -> Result<TemplatePlan, TranslateError> {
    let mut drafts: Vec<Draft> = Vec::new();
    let mut sketches: Vec<Sketch> = Vec::new();
    for query in queries {
        sketches.push(plan_query(query, &mut drafts)?);
    }
    // Names need every user of a relation, so they come last.
    let names: Vec<String> = drafts.iter().map(relation_name).collect();
    if let Some(name) = first_duplicate(&names) {
        return Err(TranslateError::NameClash {
            name: name.clone(),
        });
    }
    let rules: Vec<RulePlan> = sketches
        .into_iter()
        .map(|sketch| {
            let body: Vec<String> = sketch
                .atoms
                .iter()
                .map(|(relation, terms)| format!("{}({})", names[*relation], terms.join(", ")))
                .collect();
            RulePlan {
                query: sketch.query,
                datalog: format!("Q({}) :- {}.", sketch.head.join(", "), body.join(", ")),
                count_sql: sketch.count_sql,
            }
        })
        .collect();
    let relations = drafts
        .into_iter()
        .zip(names)
        .map(|(draft, name)| RelationPlan {
            name,
            table: draft.table,
            columns: draft.columns,
            join_columns: draft.join_columns,
            filter: draft.filter,
            queries: draft.queries,
        })
        .collect();
    Ok(TemplatePlan {
        relations,
        rules,
    })
}

/// The column every table keys its rows on, carried when it is not joined.
const ID: &str = "id";

/// The term of a carried `id`.
const PLACEHOLDER: &str = "_";

/// A relation file under construction: its sharing key (`table`, `columns`,
/// `filter`) and who uses it.
struct Draft {
    table: String,
    columns: Vec<String>,
    join_columns: usize,
    filter: String,
    /// The alias of the first query's first use of the key; it names a
    /// filtered relation.
    first_alias: String,
    queries: Vec<String>,
}

/// One query's rule before the relations have their names.
struct Sketch {
    query: String,
    count_sql: String,
    /// The variable names in position order.
    head: Vec<String>,
    /// One atom per alias in `FROM` order: the relation's index in the
    /// drafts, and its terms.
    atoms: Vec<(usize, Vec<String>)>,
}

/// The join variables of one query.
struct Variables {
    /// Every joined column once, in the order the joins first mention it,
    /// with the position of its variable.
    columns: Vec<(ColumnRef, usize)>,
    /// The variables' names, by position.
    names: Vec<String>,
}

impl Variables {
    fn of(query: &JobQuery) -> Variables {
        // Number the joined columns by first mention.
        let mut columns: Vec<&ColumnRef> = Vec::new();
        let mut pairs: Vec<(usize, usize)> = Vec::new();
        for (left, right) in &query.joins {
            let left = intern(&mut columns, left);
            let right = intern(&mut columns, right);
            pairs.push((left, right));
        }
        // Union-find in its plainest form: a join merges the labels of its two
        // columns, so equal labels are one variable.
        let mut label: Vec<usize> = (0..columns.len()).collect();
        for &(left, right) in &pairs {
            let (kept, merged) = (label[left], label[right]);
            for other in &mut label {
                if *other == merged {
                    *other = kept;
                }
            }
        }
        // A variable's position is the rank of the first join that mentions it.
        let mut order: Vec<usize> = Vec::new();
        let mut position = vec![0; columns.len()];
        for &(left, right) in &pairs {
            let rank = intern(&mut order, label[left]);
            position[left] = rank;
            position[right] = rank;
        }
        // A variable is named after the first `id` among its columns, else
        // after its first column.
        let mut names = vec![String::new(); order.len()];
        let mut named_by_id = vec![false; order.len()];
        for (column, &rank) in columns.iter().zip(&position) {
            if column.column == ID && !named_by_id[rank] {
                names[rank] = column.alias.to_uppercase();
                named_by_id[rank] = true;
            } else if names[rank].is_empty() {
                names[rank] = format!("{}_{}", column.alias, column.column).to_uppercase();
            }
        }
        Variables {
            columns: columns.into_iter().cloned().zip(position).collect(),
            names,
        }
    }

    /// The join columns of `alias` with their variables' positions, ordered by
    /// position; two columns in one variable stay in the order of first
    /// mention.
    fn of_alias(&self, alias: &str) -> Vec<(&str, usize)> {
        let mut joined: Vec<(&str, usize)> = self
            .columns
            .iter()
            .filter(|(column, _)| column.alias == alias)
            .map(|(column, rank)| (column.column.as_str(), *rank))
            .collect();
        joined.sort_by_key(|&(_, rank)| rank);
        joined
    }

    fn check_names(&self) -> Result<(), TranslateError> {
        match first_duplicate(&self.names) {
            | Some(name) => Err(TranslateError::NameClash {
                name: name.clone(),
            }),
            | None => Ok(()),
        }
    }
}

/// The index of `item` in `seen`, appending it first when it is new.
fn intern<T: PartialEq>(seen: &mut Vec<T>, item: T) -> usize {
    let found = seen.iter().position(|other| *other == item);
    match found {
        | Some(index) => index,
        | None => {
            seen.push(item);
            seen.len() - 1
        },
    }
}

/// The first name that an earlier entry already has.
fn first_duplicate(names: &[String]) -> Option<&String> {
    names
        .iter()
        .enumerate()
        .find(|&(index, name)| names[..index].contains(name))
        .map(|(_, name)| name)
}

/// Plans one query: its variables, and for each alias the relation it reads
/// (found among `drafts` or added to them).
fn plan_query(query: &JobQuery, drafts: &mut Vec<Draft>) -> Result<Sketch, TranslateError> {
    let variables = Variables::of(query);
    if let Some(alias) = query
        .aliases
        .iter()
        .find(|alias| variables.of_alias(&alias.name).is_empty())
    {
        return Err(TranslateError::UnjoinedAlias {
            query: query.name.clone(),
            alias: alias.name.clone(),
        });
    }
    variables.check_names()?;
    let mut atoms = Vec::new();
    for alias in &query.aliases {
        let joined = variables.of_alias(&alias.name);
        let mut columns: Vec<String> = joined
            .iter()
            .map(|&(column, _)| column.to_owned())
            .collect();
        let mut terms: Vec<String> = joined
            .iter()
            .map(|&(_, rank)| variables.names[rank].clone())
            .collect();
        if !columns.iter().any(|column| column == ID) {
            columns.push(ID.to_owned());
            terms.push(PLACEHOLDER.to_owned());
        }
        let filter = query
            .filters
            .iter()
            .filter(|(owner, _)| *owner == alias.name)
            .map(|(_, text)| text.as_str())
            .collect::<Vec<_>>()
            .join(" AND ");
        let found = drafts.iter().position(|draft| {
            draft.table == alias.table && draft.columns == columns && draft.filter == filter
        });
        let index = match found {
            | Some(index) => index,
            | None => {
                drafts.push(Draft {
                    table: alias.table.clone(),
                    columns,
                    join_columns: joined.len(),
                    filter,
                    first_alias: alias.name.clone(),
                    queries: Vec::new(),
                });
                drafts.len() - 1
            },
        };
        if drafts[index].queries.last() != Some(&query.name) {
            drafts[index].queries.push(query.name.clone());
        }
        atoms.push((index, terms));
    }
    Ok(Sketch {
        query: query.name.clone(),
        count_sql: query.count_sql.clone(),
        head: variables.names,
        atoms,
    })
}

/// A relation's file name: the table for an unfiltered one, else
/// `{table}_{alias}_{letters}` with the variant letters (`6a` gives `a`) of
/// every query that uses it.
fn relation_name(draft: &Draft) -> String {
    if draft.filter.is_empty() {
        return draft.table.clone();
    }
    let letters: String = draft
        .queries
        .iter()
        .map(|query| query.trim_start_matches(|c: char| c.is_ascii_digit()))
        .collect();
    format!("{}_{}_{}", draft.table, draft.first_alias, letters)
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
            "SELECT MIN(a.x) FROM t AS a WHERE a.x = 1 FETCH FIRST 5 ROWS ONLY",
            "SELECT MIN(a.x) FROM t AS a WHERE a.x = 1 FOR UPDATE",
            "SELECT MIN(a.x) FROM t AS a WHERE a.x = 1 QUALIFY a.x > 0",
            "SELECT MIN(a.x) INTO u FROM t AS a WHERE a.x = 1",
            "SELECT TOP 5 MIN(a.x) FROM t AS a WHERE a.x = 1",
            "SELECT MIN(a.x) FROM t AS a WHERE a.x = 1 WINDOW w AS (PARTITION BY a.x)",
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
    fn subqueries_in_where_are_rejected() {
        // Each is a subquery in a conjunct; the last two are nested inside an
        // `OR` and a `NOT`, not at the top of the conjunct.
        let conjuncts = [
            "EXISTS (SELECT 1 FROM u AS b WHERE b.x = a.x)",
            "a.x IN (SELECT b.x FROM u AS b)",
            "a.x = (SELECT MIN(b.x) FROM u AS b)",
            "(a.x = 1 OR a.x IN (SELECT b.x FROM u AS b))",
            "NOT EXISTS (SELECT 1 FROM u AS b)",
        ];
        for conjunct in conjuncts {
            let sql = format!("SELECT MIN(a.x) FROM t AS a WHERE a.y = 2 AND {conjunct}");
            assert_eq!(
                parse_query("2c", &sql),
                Err(TranslateError::NotOneSelect {
                    query: "2c".to_owned(),
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
            (
                "SELECT MIN(a.x) FROM t AS a TABLESAMPLE BERNOULLI (10) WHERE a.x = 1",
                "t AS a TABLESAMPLE",
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

    /// Plans the `(name, sql)` variants of one template.
    fn plan<S: AsRef<str>>(variants: &[(&str, S)]) -> TemplatePlan {
        let queries: Vec<JobQuery> = variants
            .iter()
            .map(|(name, sql)| parse(name, sql.as_ref()))
            .collect();
        plan_template(&queries).unwrap_or_else(|error| panic!("{error}"))
    }

    /// One line per relation: `name: table(columns) join=n filter=[…]
    /// queries=…`.
    fn described(planned: &TemplatePlan) -> Vec<String> {
        planned
            .relations
            .iter()
            .map(|relation| {
                format!(
                    "{}: {}({}) join={} filter=[{}] queries={}",
                    relation.name,
                    relation.table,
                    relation.columns.join(", "),
                    relation.join_columns,
                    relation.filter,
                    relation.queries.join(",")
                )
            })
            .collect()
    }

    fn datalog(planned: &TemplatePlan) -> Vec<String> {
        planned
            .rules
            .iter()
            .map(|rule| rule.datalog.clone())
            .collect()
    }

    /// The six variants of a template-6-shaped query: five aliases, three
    /// variables, and a filter on `k`, `n` and `t` that changes from variant
    /// to variant (`6f` has none on `n`).
    ///
    /// The plan, derived by hand. The joins, in text order, mention these
    /// columns first-to-last: `k.id`, `mk.keyword_id`, `t.id`, `mk.movie_id`,
    /// `ci.movie_id`, `n.id`, `ci.person_id`. The variables, in position
    /// order, are `K` = {`k.id`, `mk.keyword_id`}, `T` = {`t.id`,
    /// `mk.movie_id`, `ci.movie_id`} and `N` = {`n.id`, `ci.person_id`}, at
    /// positions 0, 1 and 2, each named after the `id` it contains. The
    /// drafts, in `FROM` order (`ci`, `k`, `mk`, `n`, `t`), are
    /// `cast_info(movie_id, person_id, id)` read as `(T, N, _)`, `keyword(id)`
    /// as `(K)`, `movie_keyword(keyword_id, movie_id, id)` as `(K, T, _)`,
    /// `name(id)` as `(N)` and `title(id)` as `(T)`.
    fn template_6() -> Vec<JobQuery> {
        let one = "k.keyword = 'kw-one'";
        let two = "k.keyword IN ('kw-two', 'kw-three')";
        let person = "n.name LIKE '%Ab%Cd%'";
        [
            ("6a", one, person, "t.production_year > 2010"),
            ("6b", two, person, "t.production_year > 2014"),
            ("6c", one, person, "t.production_year > 2014"),
            ("6d", two, person, "t.production_year > 2000"),
            ("6e", one, person, "t.production_year > 2000"),
            ("6f", two, "", "t.production_year > 2000"),
        ]
        .into_iter()
        .map(|(name, keyword_filter, person_filter, year_filter)| {
            let filters: String = [keyword_filter, person_filter, year_filter]
                .iter()
                .filter(|condition| !condition.is_empty())
                .map(|condition| format!("{condition} AND "))
                .collect();
            parse(
                name,
                &format!(
                    "SELECT MIN(k.keyword), MIN(n.name), MIN(t.title) FROM cast_info AS ci, \
                     keyword AS k, movie_keyword AS mk, name AS n, title AS t WHERE {filters}k.id \
                     = mk.keyword_id AND t.id = mk.movie_id AND t.id = ci.movie_id AND \
                     ci.movie_id = mk.movie_id AND n.id = ci.person_id"
                ),
            )
        })
        .collect()
    }

    /// Template 6's rule for a variant that uses these relations.
    fn rule_6(keyword: &str, person: &str, title: &str) -> String {
        format!(
            "Q(K, T, N) :- cast_info(T, N, _), {keyword}(K), movie_keyword(K, T, _), {person}(N), \
             {title}(T)."
        )
    }

    #[test]
    fn template_6_shapes() {
        let queries = template_6();
        let planned = plan_template(&queries).unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(described(&planned), [
            "cast_info: cast_info(movie_id, person_id, id) join=2 filter=[] \
             queries=6a,6b,6c,6d,6e,6f",
            "keyword_k_ace: keyword(id) join=1 filter=[keyword = 'kw-one'] queries=6a,6c,6e",
            "movie_keyword: movie_keyword(keyword_id, movie_id, id) join=2 filter=[] \
             queries=6a,6b,6c,6d,6e,6f",
            "name_n_abcde: name(id) join=1 filter=[name LIKE '%Ab%Cd%'] queries=6a,6b,6c,6d,6e",
            "title_t_a: title(id) join=1 filter=[production_year > 2010] queries=6a",
            "keyword_k_bdf: keyword(id) join=1 filter=[keyword IN ('kw-two', 'kw-three')] \
             queries=6b,6d,6f",
            "title_t_bc: title(id) join=1 filter=[production_year > 2014] queries=6b,6c",
            "title_t_def: title(id) join=1 filter=[production_year > 2000] queries=6d,6e,6f",
            "name: name(id) join=1 filter=[] queries=6f",
        ]);
        // The spec's example, verbatim.
        assert_eq!(
            planned.rules[0].datalog,
            "Q(K, T, N) :- cast_info(T, N, _), keyword_k_ace(K), movie_keyword(K, T, _), \
             name_n_abcde(N), title_t_a(T)."
        );
        assert_eq!(datalog(&planned), [
            rule_6("keyword_k_ace", "name_n_abcde", "title_t_a"),
            rule_6("keyword_k_bdf", "name_n_abcde", "title_t_bc"),
            rule_6("keyword_k_ace", "name_n_abcde", "title_t_bc"),
            rule_6("keyword_k_bdf", "name_n_abcde", "title_t_def"),
            rule_6("keyword_k_ace", "name_n_abcde", "title_t_def"),
            rule_6("keyword_k_bdf", "name", "title_t_def"),
        ]);
        assert_eq!(planned.rules.len(), queries.len());
        for (rule, query) in planned.rules.iter().zip(&queries) {
            assert_eq!(rule.query, query.name);
            assert_eq!(rule.count_sql, query.count_sql);
        }
    }

    #[test]
    fn every_rule_parses_as_datalog() {
        use kermit_parser::{JoinQuery, Term};
        let planned = plan_template(&template_6()).unwrap_or_else(|error| panic!("{error}"));
        let names: Vec<&str> = planned
            .relations
            .iter()
            .map(|relation| relation.name.as_str())
            .collect();
        for rule in &planned.rules {
            let parsed: JoinQuery = rule
                .datalog
                .parse()
                .unwrap_or_else(|error| panic!("{}: {error}", rule.datalog));
            assert_eq!(parsed.head.name, "Q");
            assert_eq!(parsed.head.terms, vec![
                Term::Var("K".into()),
                Term::Var("T".into()),
                Term::Var("N".into())
            ]);
            assert_eq!(parsed.body.len(), 5);
            for atom in &parsed.body {
                assert!(
                    names.contains(&atom.name.as_str()),
                    "{} is not planned",
                    atom.name
                );
            }
            assert_eq!(parsed.body[0].terms, vec![
                Term::Var("T".into()),
                Term::Var("N".into()),
                Term::Placeholder
            ]);
        }
    }

    #[test]
    fn variables_follow_text_order() {
        let forward = "SELECT MIN(t.title) FROM movie_keyword AS mk, title AS t, keyword AS k \
                       WHERE t.id = mk.movie_id AND k.id = mk.keyword_id";
        let reversed = "SELECT MIN(t.title) FROM movie_keyword AS mk, title AS t, keyword AS k \
                        WHERE k.id = mk.keyword_id AND t.id = mk.movie_id";
        let first = plan(&[("1a", forward)]);
        let second = plan(&[("1a", reversed)]);
        assert_eq!(datalog(&first), [
            "Q(T, K) :- movie_keyword(T, K, _), title(T), keyword(K)."
        ]);
        assert_eq!(datalog(&second), [
            "Q(K, T) :- movie_keyword(K, T, _), title(T), keyword(K)."
        ]);
        assert_eq!(first.relations[0].columns, ["movie_id", "keyword_id", "id"]);
        assert_eq!(second.relations[0].columns, [
            "keyword_id",
            "movie_id",
            "id"
        ]);
    }

    #[test]
    fn id_is_appended_only_when_not_joined() {
        // `kt` joins on its `id` alone, `t` on `kind_id` and `id`, `ci` on
        // `movie_id` only: its `id` is carried.
        let planned = plan(&[(
            "1a",
            "SELECT MIN(t.title) FROM title AS t, kind_type AS kt, cast_info AS ci WHERE \
             t.kind_id = kt.id AND t.id = ci.movie_id",
        )]);
        assert_eq!(described(&planned), [
            "title: title(kind_id, id) join=2 filter=[] queries=1a",
            "kind_type: kind_type(id) join=1 filter=[] queries=1a",
            "cast_info: cast_info(movie_id, id) join=1 filter=[] queries=1a",
        ]);
        assert_eq!(datalog(&planned), [
            "Q(KT, T) :- title(KT, T), kind_type(KT), cast_info(T, _)."
        ]);
    }

    #[test]
    fn shared_relation_keeps_column_order_in_its_key() {
        // The same filter on `mk` in all three variants, but `1b` reads the
        // joins the other way round, so its `mk` file has its columns the
        // other way round: a second file, not a shared one. `1a` and `1c`
        // agree and share.
        let sql = |joins: &str| {
            format!(
                "SELECT MIN(t.title) FROM movie_keyword AS mk, title AS t, keyword AS k WHERE \
                 mk.keyword_id > 5 AND {joins}"
            )
        };
        let forward = sql("t.id = mk.movie_id AND k.id = mk.keyword_id");
        let reversed = sql("k.id = mk.keyword_id AND t.id = mk.movie_id");
        let planned = plan(&[("1a", &forward), ("1b", &reversed), ("1c", &forward)]);
        assert_eq!(described(&planned), [
            "movie_keyword_mk_ac: movie_keyword(movie_id, keyword_id, id) join=2 \
             filter=[keyword_id > 5] queries=1a,1c",
            "title: title(id) join=1 filter=[] queries=1a,1b,1c",
            "keyword: keyword(id) join=1 filter=[] queries=1a,1b,1c",
            "movie_keyword_mk_b: movie_keyword(keyword_id, movie_id, id) join=2 \
             filter=[keyword_id > 5] queries=1b",
        ]);
        assert_eq!(datalog(&planned), [
            "Q(T, K) :- movie_keyword_mk_ac(T, K, _), title(T), keyword(K).",
            "Q(K, T) :- movie_keyword_mk_b(K, T, _), title(T), keyword(K).",
            "Q(T, K) :- movie_keyword_mk_ac(T, K, _), title(T), keyword(K).",
        ]);
    }

    #[test]
    fn filtered_name_lists_variant_letters() {
        // Multi-digit templates: `16a` contributes `a`, not `16a`.
        let sql = |year: &str| {
            format!(
                "SELECT MIN(t.title) FROM title AS t, movie_keyword AS mk WHERE t.production_year \
                 > {year} AND t.id = mk.movie_id"
            )
        };
        let planned = plan(&[
            ("16a", sql("2000")),
            ("16b", sql("2010")),
            ("16c", sql("2000")),
        ]);
        assert_eq!(described(&planned), [
            "title_t_ac: title(id) join=1 filter=[production_year > 2000] queries=16a,16c",
            "movie_keyword: movie_keyword(movie_id, id) join=1 filter=[] queries=16a,16b,16c",
            "title_t_b: title(id) join=1 filter=[production_year > 2010] queries=16b",
        ]);
        assert_eq!(datalog(&planned), [
            "Q(T) :- title_t_ac(T), movie_keyword(T, _).",
            "Q(T) :- title_t_b(T), movie_keyword(T, _).",
            "Q(T) :- title_t_ac(T), movie_keyword(T, _).",
        ]);
    }

    #[test]
    fn renamed_aliases_still_share() {
        // Template-8 style: the variants rename their aliases (`an1`/`an`,
        // `n1`/`n`), so the variables differ, but the files do not.
        let planned = plan(&[
            (
                "8a",
                "SELECT MIN(an1.name) FROM aka_name AS an1, name AS n1, cast_info AS ci WHERE \
                 an1.name LIKE '%Y%' AND an1.person_id = n1.id AND n1.id = ci.person_id",
            ),
            (
                "8b",
                "SELECT MIN(an.name) FROM aka_name AS an, name AS n, cast_info AS ci WHERE \
                 an.name LIKE '%Y%' AND an.person_id = n.id AND n.id = ci.person_id",
            ),
        ]);
        assert_eq!(described(&planned), [
            "aka_name_an1_ab: aka_name(person_id, id) join=1 filter=[name LIKE '%Y%'] \
             queries=8a,8b",
            "name: name(id) join=1 filter=[] queries=8a,8b",
            "cast_info: cast_info(person_id, id) join=1 filter=[] queries=8a,8b",
        ]);
        assert_eq!(datalog(&planned), [
            "Q(N1) :- aka_name_an1_ab(N1, _), name(N1), cast_info(N1, _).",
            "Q(N) :- aka_name_an1_ab(N, _), name(N), cast_info(N, _).",
        ]);
    }

    #[test]
    fn variable_named_after_id_alias() {
        // The `id` member is on the right of both joins, and its alias has a
        // digit. Both `cct` aliases read one shared file, once each in
        // `queries`.
        let planned = plan(&[(
            "1a",
            "SELECT MIN(cc.id) FROM complete_cast AS cc, comp_cast_type AS cct1, comp_cast_type \
             AS cct2 WHERE cc.subject_id = cct1.id AND cc.status_id = cct2.id",
        )]);
        assert_eq!(described(&planned), [
            "complete_cast: complete_cast(subject_id, status_id, id) join=2 filter=[] queries=1a",
            "comp_cast_type: comp_cast_type(id) join=1 filter=[] queries=1a",
        ]);
        assert_eq!(datalog(&planned), ["Q(CCT1, CCT2) :- complete_cast(CCT1, \
                                        CCT2, _), comp_cast_type(CCT1), \
                                        comp_cast_type(CCT2)."]);
    }

    #[test]
    fn fallback_variable_name() {
        // No member is an `id`: the variable is named after its first member.
        let planned = plan(&[(
            "1a",
            "SELECT MIN(ci.note) FROM cast_info AS ci, movie_keyword AS mk WHERE ci.movie_id = \
             mk.movie_id",
        )]);
        assert_eq!(datalog(&planned), ["Q(CI_MOVIE_ID) :- \
                                        cast_info(CI_MOVIE_ID, _), \
                                        movie_keyword(CI_MOVIE_ID, _)."]);
    }

    #[test]
    fn two_columns_of_one_alias_in_one_variable_keep_text_order() {
        // JOB has no such query; the translator keeps the columns in the order
        // the joins first mention them, and the variable repeats in the atom.
        let planned = plan(&[(
            "1a",
            "SELECT MIN(t.title) FROM movie_link AS ml, title AS t WHERE ml.movie_id = t.id AND \
             ml.linked_movie_id = t.id",
        )]);
        assert_eq!(described(&planned), [
            "movie_link: movie_link(movie_id, linked_movie_id, id) join=2 filter=[] queries=1a",
            "title: title(id) join=1 filter=[] queries=1a",
        ]);
        assert_eq!(
            datalog(&planned),
            ["Q(T) :- movie_link(T, T, _), title(T)."]
        );
    }

    #[test]
    fn unjoined_alias_is_an_error() {
        // `k` appears in a filter, which does not join it.
        let query = parse(
            "9z",
            "SELECT MIN(t.title) FROM title AS t, keyword AS k, movie_keyword AS mk WHERE \
             k.keyword = 'kw' AND mk.movie_id = t.id",
        );
        assert_eq!(
            plan_template(&[query]),
            Err(TranslateError::UnjoinedAlias {
                query: "9z".to_owned(),
                alias: "k".to_owned(),
            })
        );
    }

    #[test]
    fn relation_name_clash_is_an_error() {
        // Unfiltered, so both `mk` files would be called `movie_keyword`, but
        // their columns differ (see `variables_follow_text_order`).
        let forward = parse(
            "1a",
            "SELECT MIN(t.title) FROM movie_keyword AS mk, title AS t, keyword AS k WHERE t.id = \
             mk.movie_id AND k.id = mk.keyword_id",
        );
        let reversed = parse(
            "1b",
            "SELECT MIN(t.title) FROM movie_keyword AS mk, title AS t, keyword AS k WHERE k.id = \
             mk.keyword_id AND t.id = mk.movie_id",
        );
        assert_eq!(
            plan_template(&[forward, reversed]),
            Err(TranslateError::NameClash {
                name: "movie_keyword".to_owned(),
            })
        );
    }

    #[test]
    fn variable_name_clash_is_an_error() {
        // `mi.x` has no `id` in its variable: its name falls back to `MI_X`,
        // which is also the name `mi_x.id` gives the other variable.
        let query = parse(
            "1a",
            "SELECT MIN(mi.x) FROM movie_info AS mi, aka_name AS a, title AS mi_x, keyword AS b \
             WHERE mi.x = a.y AND mi_x.id = b.z",
        );
        assert_eq!(
            plan_template(&[query]),
            Err(TranslateError::NameClash {
                name: "MI_X".to_owned(),
            })
        );
    }

    #[test]
    fn names_survive_digits_and_underscores() {
        // A two-variant template 16 whose aliases carry digits and underscores
        // (`it1`, `mi_idx1`) and whose number has two digits: the names keep
        // the digits and underscores, and the letters drop the `16`. (Template
        // 6's names are pinned in `template_6_shapes`.)
        let rank = |year: &str| {
            format!(
                "SELECT MIN(mi_idx1.info) FROM info_type AS it1, movie_info_idx AS mi_idx1, title \
                 AS t WHERE it1.info = 'rank-info' AND t.production_year > {year} AND \
                 mi_idx1.info_type_id = it1.id AND mi_idx1.movie_id = t.id"
            )
        };
        let sixteen = plan(&[("16a", rank("2000")), ("16b", rank("2005"))]);
        assert_eq!(
            sixteen
                .relations
                .iter()
                .map(|relation| relation.name.as_str())
                .collect::<Vec<_>>(),
            [
                "info_type_it1_ab",
                "movie_info_idx",
                "title_t_a",
                "title_t_b"
            ]
        );
    }
}
