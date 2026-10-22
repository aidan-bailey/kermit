//! The translator on JOB's real 113 queries.
//!
//! `kermit-generate`'s unit tests run the translator on hand-written JOB-shaped
//! SQL, because the query repository carries no licence and its files cannot be
//! committed. This test fetches the pinned originals instead
//! (`gregrahn/join-order-benchmark` at `pins::QUERY_COMMIT`), each checked
//! against its SHA-256 by `download_verified`, parses every variant of every
//! template, plans the template, and checks every emitted rule and relation
//! against what the executors and the generator layer need:
//!
//! - every rule parses as a `JoinQuery` and passes `kermit::db::validate_query`
//!   (the check `bench run` makes) against headers built from its template's
//!   relations: head terms are distinct bound variables, every body atom names
//!   a relation of the template with one term per column, and no term is a
//!   malformed constant. A variable the translator failed to uppercase parses
//!   as a constant, so it is refused as a head term that is not a variable, or
//!   else as a malformed constant;
//! - every relation name matches `[a-z][a-z0-9_]*`, which `RelationSink::open`
//!   enforces;
//! - 113 rules and 465 relation files across the 33 templates.
//!
//! It lives in `kermit` because `validate_query` is in the binary crate's
//! library, which `kermit-generate` does not depend on.
//!
//! Needs network access to `raw.githubusercontent.com`, in CI too. It skips,
//! with a stderr line (run with `--nocapture` to see it), only when the first
//! fetch cannot connect. A digest mismatch, an HTTP error status or an empty
//! answer, or a failure after the first fetch panics, so a stale pin cannot
//! hide behind a skip.

use {
    kermit::db::validate_query,
    kermit_bench::{cache::download_verified, BenchError},
    kermit_ds::RelationHeader,
    kermit_generate::job::{
        pins::{query_url, QUERY_FILES},
        translate::{parse_query, plan_template, JobQuery, TemplatePlan},
    },
    kermit_parser::JoinQuery,
    std::{collections::BTreeMap, fs, sync::OnceLock},
};

/// JOB's 33 templates.
const TEMPLATES: usize = 33;
/// JOB's 113 queries, one rule each.
const RULES: usize = 113;
/// The relation files across all 33 templates (the spec's golden count).
const RELATION_FILES: usize = 465;

/// The pinned queries as `(name, sql)` pairs.
type Queries = Vec<(String, String)>;

/// The 113 pinned queries as `(name, sql)`, `name` being `6a`, in
/// `QUERY_FILES` order (template, then letter). `None` when the first fetch
/// could not connect. Fetched once per test binary: a failure is stored as its
/// message, which every test then panics with, so it costs one fetch round (a
/// panic inside `OnceLock::get_or_init` would leave the lock empty and the
/// next test would fetch all 114 files again).
fn pinned_queries() -> Option<&'static [(String, String)]> {
    static QUERIES: OnceLock<Result<Option<Queries>, String>> = OnceLock::new();
    match QUERIES.get_or_init(fetch_pinned_queries) {
        | Ok(queries) => queries.as_deref(),
        | Err(message) => panic!("{message}"),
    }
}

fn fetch_pinned_queries() -> Result<Option<Queries>, String> {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let mut queries = Vec::new();
    for (index, (file, sha256)) in QUERY_FILES.iter().enumerate() {
        let dest = dir.path().join(file);
        match download_verified(&query_url(file), &dest, sha256) {
            | Ok(()) => {},
            // Only a connection, DNS or timeout failure is "offline". A `Download` carrying
            // an HTTP status (a 404 for a vanished pin) fails the test, as #96's does.
            | Err(BenchError::Download {
                url,
                source,
            }) if index == 0 && source.status().is_none() => {
                eprintln!(
                    "skipping: could not reach {url} ({source}); the pinned JOB queries are \
                     fetched at test time"
                );
                return Ok(None);
            },
            | Err(error) => return Err(format!("fetching {file} failed: {error}")),
        }
        if let Some(name) = file.strip_suffix(".sql").filter(|name| *name != "schema") {
            let sql = fs::read_to_string(&dest).map_err(|e| format!("reading {file}: {e}"))?;
            queries.push((name.to_owned(), sql));
        }
    }
    Ok(Some(queries))
}

/// The template number of a query name: `6a` is 6, `33c` is 33.
fn template_number(name: &str) -> u8 {
    name.trim_end_matches(|c: char| c.is_ascii_lowercase())
        .parse()
        .unwrap_or_else(|_| panic!("{name} does not start with a template number"))
}

/// Every template's plan, and everything that went wrong on the way.
struct Translated {
    plans: BTreeMap<u8, TemplatePlan>,
    problems: Vec<String>,
}

/// Parses every query, groups the variants by template (they arrive in letter
/// order) and plans each template. A failure is recorded, not raised, so one
/// run reports them all.
fn translate(queries: &[(String, String)]) -> Translated {
    let mut problems = Vec::new();
    let mut variants: BTreeMap<u8, Vec<JobQuery>> = BTreeMap::new();
    for (name, sql) in queries {
        match parse_query(name, sql) {
            | Ok(query) => variants
                .entry(template_number(name))
                .or_default()
                .push(query),
            | Err(error) => problems.push(format!("parse {name}: {error}")),
        }
    }
    let mut plans = BTreeMap::new();
    for (template, queries) in &variants {
        match plan_template(queries) {
            | Ok(plan) => {
                plans.insert(*template, plan);
            },
            | Err(error) => problems.push(format!("plan template {template}: {error}")),
        }
    }
    Translated {
        plans,
        problems,
    }
}

/// Whether `name` matches `[a-z][a-z0-9_]*`, the rule `RelationSink::open`
/// enforces (checked by hand: no regex crate).
fn is_relation_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|first| first.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// What is wrong with one rule, if anything: it must parse and pass
/// `validate_query` against its template's relations, the check `bench run`
/// makes before it loads anything.
fn rule_problems(query: &str, datalog: &str, headers: &[RelationHeader]) -> Vec<String> {
    let parsed: JoinQuery = match datalog.parse() {
        | Ok(parsed) => parsed,
        | Err(error) => return vec![format!("{query}: `{datalog}` does not parse: {error}")],
    };
    match validate_query(&parsed, headers) {
        | Ok(()) => Vec::new(),
        | Err(error) => vec![format!("{query}: `{datalog}` is rejected: {error}")],
    }
}

/// Fails with every problem listed, so one run reports them all.
fn assert_no_problems(problems: &[String]) {
    assert!(
        problems.is_empty(),
        "{} problem(s):\n{}",
        problems.len(),
        problems.join("\n")
    );
}

#[test]
#[cfg_attr(miri, ignore = "miri does not support network access")]
fn pinned_queries_translate_into_valid_rules() {
    let Some(queries) = pinned_queries() else {
        return;
    };
    let Translated {
        plans,
        mut problems,
    } = translate(queries);
    for (template, plan) in &plans {
        for relation in &plan.relations {
            if !is_relation_name(&relation.name) {
                problems.push(format!(
                    "template {template}: relation name {:?} does not match [a-z][a-z0-9_]*",
                    relation.name
                ));
            }
        }
        let headers: Vec<RelationHeader> = plan
            .relations
            .iter()
            .map(|relation| RelationHeader::new(&relation.name, relation.columns.clone()))
            .collect();
        for rule in &plan.rules {
            problems.extend(rule_problems(&rule.query, &rule.datalog, &headers));
        }
    }
    assert_no_problems(&problems);
}

#[test]
#[cfg_attr(miri, ignore = "miri does not support network access")]
fn pinned_templates_give_the_golden_counts() {
    let Some(queries) = pinned_queries() else {
        return;
    };
    let Translated {
        plans,
        problems,
    } = translate(queries);
    assert_no_problems(&problems);

    assert_eq!(plans.len(), TEMPLATES, "templates");
    let rules: usize = plans.values().map(|plan| plan.rules.len()).sum();
    assert_eq!(rules, RULES, "rules, one per query");
    let relation_files: usize = plans.values().map(|plan| plan.relations.len()).sum();
    assert_eq!(
        relation_files,
        RELATION_FILES,
        "relation files across all templates; per template (template:files): {}",
        plans
            .iter()
            .map(|(template, plan)| format!("{template}:{}", plan.relations.len()))
            .collect::<Vec<_>>()
            .join(" ")
    );
}

/// Template 12 reads `company_name` with `country_code = '[us]'` in 12a and 12c
/// but `country_code ='[us]'` in 12b. The sharing key is the filter as
/// `sqlparser` prints it, which normalises that spacing, so all three variants
/// share one relation. Pins that (it is why the total is 465, not 466).
#[test]
#[cfg_attr(miri, ignore = "miri does not support network access")]
fn template_12_shares_one_company_name_relation() {
    let Some(queries) = pinned_queries() else {
        return;
    };
    let Translated {
        plans,
        problems,
    } = translate(queries);
    assert_no_problems(&problems);

    let plan = &plans[&12];
    let names: Vec<&str> = plan
        .relations
        .iter()
        .filter(|relation| relation.table == "company_name")
        .map(|relation| relation.name.as_str())
        .collect();
    assert_eq!(
        names,
        ["company_name_cn_abc"],
        "template 12's company_name relations"
    );
    assert_eq!(plan.rules.len(), 3, "template 12's rules");
    for rule in &plan.rules {
        assert!(
            rule.datalog.contains("company_name_cn_abc("),
            "{} does not read company_name_cn_abc: {}",
            rule.query,
            rule.datalog
        );
    }
}
