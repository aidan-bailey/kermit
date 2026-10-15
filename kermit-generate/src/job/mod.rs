//! The Join Order Benchmark (JOB) generator (#97).
//!
//! JOB (Leis et al., "How Good Are Query Optimizers, Really?", VLDB 2015) is
//! 113 SQL queries, in 33 templates, over the IMDb dataset. This module turns
//! one template into a kermit benchmark: each alias's filters are applied
//! while the relations are generated, so the Datalog rules join JOB's real
//! tables with JOB's real selectivities.
//!
//! - [`translate`]: SQL text to relation files and Datalog rules. Pure Rust
//!   over `sqlparser`; it reads no data.
//! - [`duckdb`]: the DuckDB command-line client every filter, count and load
//!   goes through.
//! - [`pins`]: the mirrors, the query commit and the digests the raw cache is
//!   checked against.
//! - [`raw`]: the shared raw IMDb cache (`imdb.duckdb`, the query files and
//!   `provenance.json`).
//! - [`JobGenerator`]: one template, generated from a raw cache through the
//!   shared `Generator` layer. The filter stage streams DuckDB's filtered rows
//!   into each relation file; the count stage records DuckDB's `COUNT(*)` of
//!   each query as the rule's expected cardinality.
//!
//! The design is in `docs/specs/2026-10-15-job-benchmark-design.md`.

pub mod duckdb;
pub mod pins;
pub mod raw;
pub mod translate;

use {
    crate::{
        job::{
            duckdb::Duckdb,
            raw::RawCache,
            translate::{parse_query, plan_template, RelationPlan},
        },
        Generated, Generator, RelationSink,
    },
    anyhow::{ensure, Context},
    kermit_bench::QueryDefinition,
    kermit_ds::{RelationHeader, Tuples},
    kermit_rdf::generator::Target,
    serde::{Deserialize, Serialize},
    std::{collections::BTreeMap, iter, ops::RangeInclusive},
};

/// The JOB templates, `1` to `33`. A template holds the two to six variants
/// (`6a` … `6f`) of one query shape (Leis et al., "How Good Are Query
/// Optimizers, Really?", VLDB 2015); `bench gen job` and `generator: { kind:
/// job }` name one.
pub const TEMPLATES: RangeInclusive<u8> = 1..=33;

/// The name `schema.sql` has among a raw cache's pinned files; every
/// template's `query_sha256` records its digest beside its own queries'.
const SCHEMA_FILE: &str = "schema.sql";

/// Where one relation file's rows come from.
///
/// `meta.json` records one per relation file, in the order the files were
/// written; `relation` joins it by name to the common `relations` list, which
/// holds each file's columns, row count and SHA-256.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobSource {
    /// The relation's name, also its file stem.
    pub relation: String,
    /// The IMDb table its rows are read from.
    pub table: String,
    /// The filter applied to the table, with the alias stripped from its
    /// columns; empty when the relation is unfiltered.
    pub filter: String,
}

/// The JOB-specific fields of `meta.json`, flattened beside the common ones:
/// what the template's relations were generated from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobExtras {
    /// The template, `1` to `33`.
    pub template: u8,
    /// SHA-256 of the `imdb.tgz` the raw cache was built from.
    pub imdb_sha256: String,
    /// The commit of `gregrahn/join-order-benchmark` the queries were read at.
    pub query_commit: String,
    /// SHA-256 of `schema.sql` and of each of this template's query files, by
    /// file name.
    pub query_sha256: BTreeMap<String, String>,
    /// `duckdb --version` of this generation; the raw cache records its
    /// build's separately.
    pub duckdb_version: String,
    /// Where each relation file's rows come from, in the order the files
    /// were written.
    pub sources: Vec<JobSource>,
}

/// Generates one JOB template from a raw IMDb cache.
///
/// The filter stage writes one relation file per distinct (table, columns,
/// filter) among the template's variants: DuckDB selects the rows that pass
/// the filter and have no NULL join key, in `ORDER BY id` order, and the rows
/// stream to the relation's writer in batches, so no relation is held whole.
/// The count stage runs each variant's `COUNT(*)` as the rule's `expected`
/// cardinality, an oracle independent of kermit's executors. The design is in
/// `docs/specs/2026-10-15-job-benchmark-design.md`.
pub struct JobGenerator<'a> {
    /// The template to generate, one of [`TEMPLATES`].
    pub template: u8,
    /// The raw cache the filters and counts run against.
    pub raw: &'a RawCache,
    /// The DuckDB client.
    pub duckdb: &'a Duckdb,
    /// Output directory, benchmark name, tag and spec hash.
    pub target: Target<'a>,
}

/// The `SELECT` that the filter stage streams for one relation.
///
/// Every column is cast to `BIGINT` (`read_parquet` reads only `Int64`); the
/// rows are those that pass the relation's filter and have no NULL in a join
/// column (an SQL equi-join excludes them); and `ORDER BY "id"` fixes their
/// order, which a parallel scan does not promise and which the tries' build
/// cost and byte-identical regeneration depend on. The filter is the text of
/// conjuncts joined by `AND`, each an operand of the query's top-level `AND`,
/// so an `OR` inside one is parenthesised in the source and appending `AND …`
/// cannot change its meaning. Identifiers are quoted; they are IMDb column
/// names, which never contain `"`.
pub fn filter_select(relation: &RelationPlan) -> String {
    let projection = relation
        .columns
        .iter()
        .map(|column| format!("CAST(\"{column}\" AS BIGINT) AS \"{column}\""))
        .collect::<Vec<_>>()
        .join(", ");
    let filter = (!relation.filter.is_empty()).then(|| relation.filter.clone());
    let not_null = relation
        .columns
        .iter()
        .take(relation.join_columns)
        .map(|column| format!("\"{column}\" IS NOT NULL"));
    let conditions: Vec<String> = filter.into_iter().chain(not_null).collect();
    let predicate = if conditions.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", conditions.join(" AND "))
    };
    format!(
        "SELECT {projection} FROM \"{}\"{predicate} ORDER BY \"id\"",
        relation.table
    )
}

impl Generator for JobGenerator<'_> {
    type Extras = JobExtras;

    const KIND: &'static str = "job";

    fn target(&self) -> Target<'_> { self.target }

    fn generate(&self, sink: &mut RelationSink) -> anyhow::Result<Generated<JobExtras>> {
        ensure!(
            TEMPLATES.contains(&self.template),
            "JOB has templates {}..={}, not {}",
            TEMPLATES.start(),
            TEMPLATES.end(),
            self.template
        );
        let files = self.raw.template_queries(self.template)?;
        ensure!(
            !files.is_empty(),
            "the raw cache at {} holds no query files for JOB template {}",
            self.raw.dir().display(),
            self.template
        );
        let queries = files
            .iter()
            .map(|(name, sql)| parse_query(name, sql))
            .collect::<Result<Vec<_>, _>>()?;
        let plan = plan_template(&queries)?;

        // The pins the cache was built from, narrowed to this template's files.
        // Checked before any DuckDB process starts: a cache whose
        // provenance lacks one of the digests fails here, with no
        // relation written.
        let provenance = self.raw.provenance()?;
        let mut query_sha256 = BTreeMap::new();
        let pinned_files = iter::once(SCHEMA_FILE.to_owned())
            .chain(files.iter().map(|(name, _)| format!("{name}.sql")));
        for file in pinned_files {
            let digest = provenance.query_sha256.get(&file).with_context(|| {
                format!(
                    "the raw cache at {} records no SHA-256 for {file}",
                    self.raw.dir().display()
                )
            })?;
            query_sha256.insert(file, digest.clone());
        }

        let database = self.raw.database();
        let temp_dir = self.raw.temp_dir();

        // Filter stage: one DuckDB process per relation, streamed into its
        // writer. The writer is finished only after DuckDB exited 0
        // with an empty stderr (`stream_rows` fails otherwise), so a
        // failed filter leaves no file for it.
        for relation in &plan.relations {
            let header = RelationHeader::new(relation.name.clone(), relation.columns.clone());
            let mut writer = sink.open(&header)?;
            self.duckdb
                .stream_rows(
                    &database,
                    &temp_dir,
                    &filter_select(relation),
                    relation.columns.len(),
                    &mut |batch: &Tuples| writer.write_batch(batch),
                )
                .with_context(|| {
                    format!(
                        "filtering relation `{}` from table `{}`",
                        relation.name, relation.table
                    )
                })?;
            writer.finish()?;
        }

        // Count stage: DuckDB's COUNT(*) of the original query is the rule's
        // expected cardinality.
        let mut definitions = Vec::with_capacity(plan.rules.len());
        for rule in &plan.rules {
            let expected = self
                .duckdb
                .query_u64(&database, &temp_dir, &rule.count_sql)
                .with_context(|| format!("counting JOB query {}", rule.query))?;
            definitions.push(QueryDefinition {
                name: rule.query.clone(),
                description: format!("JOB {}, filters applied at generation", rule.query),
                query: rule.datalog.clone(),
                expected: Some(expected),
            });
        }

        let sources = plan
            .relations
            .iter()
            .map(|relation| JobSource {
                relation: relation.name.clone(),
                table: relation.table.clone(),
                filter: relation.filter.clone(),
            })
            .collect();

        Ok(Generated {
            description: format!(
                "Join Order Benchmark template {} (Leis et al., VLDB 2015) over IMDb, filters \
                 applied at generation",
                self.template
            ),
            queries: definitions,
            extras: JobExtras {
                template: self.template,
                imdb_sha256: provenance.imdb_sha256,
                query_commit: provenance.query_commit,
                query_sha256,
                duckdb_version: self.duckdb.version().to_owned(),
                sources,
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A relation plan of the given shape; `queries` plays no part in the
    /// `SELECT`.
    fn relation(
        name: &str, table: &str, columns: &[&str], join_columns: usize, filter: &str,
    ) -> RelationPlan {
        RelationPlan {
            name: name.to_owned(),
            table: table.to_owned(),
            columns: columns.iter().map(|column| (*column).to_owned()).collect(),
            join_columns,
            filter: filter.to_owned(),
            queries: vec!["1a".to_owned()],
        }
    }

    #[test]
    fn filter_select_shapes() {
        // Filtered, two join columns, then a carried id.
        let filtered = relation(
            "movie_info_mi1_a",
            "movie_info",
            &["info_type_id", "movie_id", "id"],
            2,
            "info IN ('Action', 'Drama')",
        );
        assert_eq!(
            filter_select(&filtered),
            concat!(
                r#"SELECT CAST("info_type_id" AS BIGINT) AS "info_type_id", "#,
                r#"CAST("movie_id" AS BIGINT) AS "movie_id", CAST("id" AS BIGINT) AS "id" "#,
                r#"FROM "movie_info" WHERE info IN ('Action', 'Drama') "#,
                r#"AND "info_type_id" IS NOT NULL AND "movie_id" IS NOT NULL ORDER BY "id""#,
            )
        );

        // Unfiltered: no filter conjunct, only the NULL checks.
        let unfiltered = relation(
            "movie_keyword",
            "movie_keyword",
            &["movie_id", "keyword_id", "id"],
            2,
            "",
        );
        assert_eq!(
            filter_select(&unfiltered),
            concat!(
                r#"SELECT CAST("movie_id" AS BIGINT) AS "movie_id", "#,
                r#"CAST("keyword_id" AS BIGINT) AS "keyword_id", CAST("id" AS BIGINT) AS "id" "#,
                r#"FROM "movie_keyword" WHERE "movie_id" IS NOT NULL "#,
                r#"AND "keyword_id" IS NOT NULL ORDER BY "id""#,
            )
        );

        // Id only: the id is the one join column and is checked like any other.
        let id_only = relation("title_t_a", "title", &["id"], 1, "production_year > 1990");
        assert_eq!(
            filter_select(&id_only),
            concat!(
                r#"SELECT CAST("id" AS BIGINT) AS "id" FROM "title" "#,
                r#"WHERE production_year > 1990 AND "id" IS NOT NULL ORDER BY "id""#,
            )
        );

        // The id first and a second join column after it: both are checked, in
        // column order, and a filter text holding double quotes passes
        // through untouched.
        let id_first = relation(
            "title_t_a",
            "title",
            &["id", "kind_id"],
            2,
            r#"title LIKE '%"Hero"%'"#,
        );
        assert_eq!(
            filter_select(&id_first),
            concat!(
                r#"SELECT CAST("id" AS BIGINT) AS "id", CAST("kind_id" AS BIGINT) AS "kind_id" "#,
                r#"FROM "title" WHERE title LIKE '%"Hero"%' "#,
                r#"AND "id" IS NOT NULL AND "kind_id" IS NOT NULL ORDER BY "id""#,
            )
        );

        // No join column and no filter (the planner never produces one): no
        // WHERE at all, never a dangling `WHERE`.
        let bare = relation("title", "title", &["id"], 0, "");
        assert_eq!(
            filter_select(&bare),
            r#"SELECT CAST("id" AS BIGINT) AS "id" FROM "title" ORDER BY "id""#
        );
    }
}
