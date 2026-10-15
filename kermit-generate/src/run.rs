//! The sequence every generator goes through.

use {
    crate::{
        generator::Generator,
        meta::{GeneratedMeta, COMMON_FIELDS},
        sink::{RelationRecord, RelationSink},
    },
    anyhow::{bail, Context},
    kermit_bench::{BenchmarkDefinition, QueryDefinition, RelationSource},
    kermit_parser::JoinQuery,
    kermit_rdf::{
        generator::{meta_json_path, write_meta_json, META_SCHEMA_VERSION},
        timestamp::utc_iso8601_now,
    },
    serde::Serialize,
    serde_json::Value,
    std::{collections::HashSet, fs, io::ErrorKind, path::Path},
};

/// Runs `generator` and leaves a complete benchmark cache at its target.
///
/// The steps and their order are the cache contract:
///
/// 1. Create the output directory and delete any existing `meta.json`, so that
///    a failed run can never leave an old commit marker beside new files.
/// 2. Call [`Generator::generate`] with a fresh [`RelationSink`].
/// 3. Check the extras: they must serialize to a JSON object, and none of its
///    keys may be a common `meta.json` field.
/// 4. Check the queries: names are unique, each one parses as a Datalog rule,
///    and each body predicate names a relation the generator wrote. A malformed
///    rule therefore fails here, not at `bench run`.
/// 5. Write `benchmark.yml`: one `file://` URL and one `sha256` per relation,
///    `generator: None`, validated first. A generator that wrote no relation or
///    no query fails that validation.
/// 6. Write `meta.json` last, and return it.
///
/// # Errors
///
/// Any failing step, with the stale `meta.json` already gone. Files the
/// generator wrote before the failure stay in the directory; without a
/// `meta.json` they are not a cache.
pub fn run<G: Generator>(generator: &G) -> anyhow::Result<GeneratedMeta<G::Extras>> {
    let target = generator.target();
    let out_dir = target.out_dir;

    clear_commit_marker(out_dir)?;

    let mut sink = RelationSink::new(out_dir);
    let generated = generator.generate(&mut sink)?;

    check_extras(&generated.extras)?;
    check_queries(&generated.queries, sink.records())?;

    let relation_count =
        u32::try_from(sink.records().len()).context("more relations than fit a u32")?;
    let query_count =
        u32::try_from(generated.queries.len()).context("more queries than fit a u32")?;
    write_benchmark_yaml(
        out_dir,
        target.bench_name,
        generated.description,
        generated.queries,
        sink.records(),
    )?;

    let meta = GeneratedMeta {
        schema_version: META_SCHEMA_VERSION,
        kind: G::KIND.to_string(),
        tag: target.tag.to_string(),
        generated_at_utc: utc_iso8601_now(),
        relation_count,
        query_count,
        spec_hash: target.spec_hash.map(str::to_string),
        relations: sink.into_records(),
        extras: generated.extras,
    };
    write_meta_json(out_dir, &meta)?;
    Ok(meta)
}

/// Step 1: creates `out_dir` and deletes `meta.json` if there is one.
fn clear_commit_marker(out_dir: &Path) -> anyhow::Result<()> {
    fs::create_dir_all(out_dir).with_context(|| format!("creating {}", out_dir.display()))?;
    let marker = meta_json_path(out_dir);
    match fs::remove_file(&marker) {
        | Ok(()) => Ok(()),
        | Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
        | Err(e) => Err(e).with_context(|| format!("removing {}", marker.display())),
    }
}

/// Step 3: the extras must be a JSON object that avoids the common fields.
/// `serde(flatten)` would otherwise let a duplicate key through silently.
fn check_extras<E: Serialize>(extras: &E) -> anyhow::Result<()> {
    let value = serde_json::to_value(extras).context("serializing the generator's extras")?;
    let fields = match value {
        | Value::Object(fields) => fields,
        | other => bail!("a generator's extras must serialize to a JSON object, not {other}"),
    };
    if let Some(key) = COMMON_FIELDS
        .into_iter()
        .find(|key| fields.contains_key(*key))
    {
        bail!(
            "a generator's extras use the key {key:?}, which meta.json already uses for a common \
             field"
        );
    }
    Ok(())
}

/// Step 4: unique names, well-formed rules, and body predicates that name
/// written relations.
fn check_queries(queries: &[QueryDefinition], records: &[RelationRecord]) -> anyhow::Result<()> {
    let mut names = HashSet::new();
    for query in queries {
        if !names.insert(query.name.as_str()) {
            bail!("the query name {:?} is used twice", query.name);
        }
        let rule: JoinQuery = query
            .query
            .parse()
            .with_context(|| format!("the query {:?} is not a Datalog rule", query.name))?;
        for predicate in &rule.body {
            if !records.iter().any(|record| record.name == predicate.name) {
                bail!(
                    "the query {:?} names the relation {:?}, which the generator did not write",
                    query.name,
                    predicate.name
                );
            }
        }
    }
    Ok(())
}

/// Step 5: writes `<out_dir>/benchmark.yml`, a validated
/// `BenchmarkDefinition` over the finished files. As the RDF pipelines do,
/// it points at each file by an absolute `file://` URL.
fn write_benchmark_yaml(
    out_dir: &Path, bench_name: &str, description: String, queries: Vec<QueryDefinition>,
    records: &[RelationRecord],
) -> anyhow::Result<()> {
    let base = out_dir
        .canonicalize()
        .with_context(|| format!("resolving {}", out_dir.display()))?;
    let relations = records
        .iter()
        .map(|record| RelationSource {
            name: record.name.clone(),
            url: Some(format!("file://{}/{}.parquet", base.display(), record.name)),
            path: None,
            sha256: Some(record.sha256.clone()),
        })
        .collect();
    let definition = BenchmarkDefinition {
        name: bench_name.to_string(),
        description,
        relations,
        queries,
        generator: None,
    };
    definition.validate()?;
    let path = out_dir.join("benchmark.yml");
    fs::write(&path, serde_yaml::to_string(&definition)?)
        .with_context(|| format!("writing {}", path.display()))
}

#[cfg(test)]
mod tests {
    use {super::*, serde_json::json};

    fn record(name: &str) -> RelationRecord {
        RelationRecord {
            name: name.to_string(),
            columns: vec!["a".to_string()],
            rows: 1,
            sha256: "0".repeat(64),
        }
    }

    fn query(name: &str, rule: &str) -> QueryDefinition {
        QueryDefinition {
            name: name.to_string(),
            description: String::new(),
            query: rule.to_string(),
            expected: None,
        }
    }

    #[test]
    fn duplicate_query_names_are_rejected() {
        let queries = [
            query("q1", "Q(X) :- edge(X)."),
            query("q1", "Q(X) :- edge(X)."),
        ];
        let err = check_queries(&queries, &[record("edge")]).unwrap_err();
        assert!(err.to_string().contains("\"q1\""), "{err}");
        assert!(err.to_string().contains("twice"), "{err}");
    }

    #[test]
    fn every_body_predicate_must_be_a_written_relation() {
        let records = [record("edge"), record("node")];
        let both = [query("q1", "Q(X) :- edge(X), node(X).")];
        check_queries(&both, &records).unwrap();
        let missing = [query("q2", "Q(X) :- edge(X), vertex(X).")];
        let err = check_queries(&missing, &records).unwrap_err();
        assert!(err.to_string().contains("\"vertex\""), "{err}");
    }

    #[test]
    fn a_malformed_rule_names_its_query() {
        let queries = [query("q9", "Q(X) :- edge(X")];
        let err = check_queries(&queries, &[record("edge")]).unwrap_err();
        assert!(format!("{err:#}").contains("\"q9\""), "{err:#}");
    }

    #[test]
    fn extras_must_be_an_object() {
        check_extras(&json!({ "seed": 1 })).unwrap();
        check_extras(&json!({})).unwrap();
        for not_an_object in [json!(42), json!("text"), json!([1, 2]), Value::Null] {
            let err = check_extras(&not_an_object).unwrap_err();
            assert!(err.to_string().contains("JSON object"), "{err}");
        }
    }

    #[test]
    fn every_common_field_is_refused_as_an_extra() {
        for key in COMMON_FIELDS {
            let mut fields = serde_json::Map::new();
            fields.insert(key.to_string(), json!(1));
            let err = check_extras(&Value::Object(fields)).unwrap_err();
            assert!(
                err.to_string().contains(&format!("{key:?}")),
                "{key}: {err}"
            );
        }
    }
}
