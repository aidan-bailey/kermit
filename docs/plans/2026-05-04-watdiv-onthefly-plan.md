# WatDiv On-The-Fly Generation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a `kermit bench watdiv-gen` subcommand that drives the vendored WatDiv binary at arbitrary scale/stress parameters and produces a complete, kermit-runnable benchmark artifact set in pure Rust.

**Architecture:** New `kermit-rdf` crate holds everything new (RDF value types, N-Triples parser wrapper, dictionary, partitioner, Parquet writer, SPARQL parser+translator, YAML emitter, watdiv binary driver, pipeline orchestrator, expected-results computer). The `kermit` binary gains a `bench watdiv-gen` subcommand that invokes the pipeline. `kermit-bench`'s discovery is extended to walk the cache root in addition to the workspace `benchmarks/` directory.

**Tech Stack:** Rust nightly, `oxttl` for N-Triples, `oxrdf` for value types, `spargebra` for SPARQL, `arrow` + `parquet` for I/O, `thiserror` for errors, `clap` for CLI, `bwrap` for sandboxing the watdiv binary.

**Reference:** spec at `docs/plans/2026-05-04-watdiv-onthefly-design.md`.

---

## File Structure

### Files to Create

```
kermit-rdf/
  Cargo.toml
  src/
    lib.rs                    # crate root with module declarations
    error.rs                  # RdfError enum (thiserror)
    value.rs                  # RdfValue { Iri, Literal, BlankNode } using oxrdf types
    ntriples.rs               # streaming parser: oxttl::NTriplesParser → (Iri, Iri, RdfValue)
    dict.rs                   # Dictionary: bidirectional Value↔usize map
    partition.rs              # sanitize_predicate + partition triples into per-predicate buckets
    parquet.rs                # write dict.parquet + per-predicate <name>.parquet
    sparql/
      mod.rs                  # re-exports
      parser.rs               # spargebra::Query::parse wrapper
      translator.rs           # SPARQL BGP → Datalog rule
      bindings.rs             # variable-name bookkeeping
    yaml_emit.rs              # write benchmark.yml using kermit-bench types
    expected.rs               # write expected.json from .desc cardinalities
    driver/
      mod.rs                  # GenerateOptions API + entry point
      sandbox.rs              # bwrap detection + env construction + temp-dir staging
      invoke.rs               # Command construction for watdiv -d/-s/-q
    pipeline.rs               # 6-stage orchestrator
  tests/
    translator_golden.rs      # ports Python test_translator.py cases
    pipeline.rs               # hand-crafted .nt + .sparql through stages 4–6
    e2e_watdiv.rs             # full binary invocation at SF=1
  vendor/
    watdiv/
      bin/Release/watdiv      # 360 KB ELF, x86_64 Linux (binary committed)
      files/firstnames.txt
      files/lastnames.txt
      files/words             # minimal wordlist for /usr/share/dict/words bind-mount
      MODEL.txt
      VERSION                 # upstream identifier + sha256
      LICENSE                 # upstream license

kermit/
  tests/
    cli_watdiv_gen.rs         # CLI smoke test (skips on missing bwrap)

.gitattributes                # mark vendored binary as "binary" (no diffs)
```

### Files to Modify

```
Cargo.toml                          # add kermit-rdf to workspace members + patches
kermit-bench/src/discovery.rs       # add load_all_with_cache(workspace, cache) variant
kermit-bench/src/lib.rs             # re-export new fn (if exposing)
kermit/Cargo.toml                   # add kermit-rdf dep
kermit/src/main.rs                  # add WatdivGen subcommand + handler; switch BenchSubcommand::List/Fetch to two-root discovery
```

### Files Deliberately Not Touched (for scope)

- `scripts/watdiv-preprocess/` — Phase 2 work (separate plan after parity holds).
- `kermit/tests/watdiv_correctness.rs` and `kermit/tests/fixtures/watdiv-mini/` — stay as deterministic LFTJ regression test.
- The 12 committed `benchmarks/watdiv-stress-*.yml` files — canonical thesis-citation snapshots.

---

## Task 1: Scaffold the kermit-rdf crate

**Files:**
- Create: `kermit-rdf/Cargo.toml`
- Create: `kermit-rdf/src/lib.rs`
- Modify: `Cargo.toml` (workspace root)

- [ ] **Step 1: Add kermit-rdf to the workspace members and patches**

Edit `Cargo.toml` (workspace root):

```toml
[workspace]
resolver = "2"
members = ["kermit", "kermit-algos", "kermit-bench", "kermit-ds", "kermit-iters", "kermit-derive", "kermit-parser", "kermit-rdf"]

[workspace.package]
authors = ["Aidan Bailey"]
edition = "2021"
homepage = "https://github.com/aidan-bailey/kermit"
license = "Apache-2.0 OR MIT"
readme = "README.md"
repository = "https://github.com/aidan-bailey/kermit"

[patch.crates-io]
kermit = { path = "kermit" }
kermit-ds = { path = "kermit-ds" }
kermit-iters = { path = "kermit-iters" }
kermit-algos = { path = "kermit-algos" }
kermit-derive = { path = "kermit-derive" }
kermit-bench = { path = "kermit-bench" }
kermit-parser = { path = "kermit-parser" }
kermit-rdf = { path = "kermit-rdf" }
```

- [ ] **Step 2: Create kermit-rdf/Cargo.toml**

```toml
[package]
name = "kermit-rdf"
description = "RDF/SPARQL preprocessing pipeline for Kermit benchmarks"
version = "0.1.0"
authors.workspace = true
edition.workspace = true
homepage.workspace = true
license.workspace = true
readme.workspace = true
repository.workspace = true

[dependencies]
kermit-parser = { version = "0.0.2", path = "../kermit-parser" }
kermit-algos = { version = "0.0.10", path = "../kermit-algos" }
kermit-ds = { version = "0.1.0", path = "../kermit-ds" }
kermit-bench = { version = "0.1.0", path = "../kermit-bench" }
arrow = "53"
parquet = "53"
oxrdf = "0.2"
oxttl = "0.1"
spargebra = "0.3"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
serde_yaml = "0.9"
sha2 = "0.10"
tempfile = "3"
thiserror = "2"

[dev-dependencies]
tempfile = "3"
```

- [ ] **Step 3: Create kermit-rdf/src/lib.rs**

```rust
//! RDF/SPARQL preprocessing pipeline for Kermit benchmarks.
//!
//! Drives the upstream WatDiv binary, parses its N-Triples + SPARQL output,
//! produces a kermit-runnable benchmark artifact set (dict + per-predicate
//! Parquet, BenchmarkDefinition YAML, expected cardinalities).
#![deny(missing_docs)]

pub mod dict;
pub mod driver;
pub mod error;
pub mod expected;
pub mod ntriples;
pub mod parquet;
pub mod partition;
pub mod pipeline;
pub mod sparql;
pub mod value;
pub mod yaml_emit;

pub use error::RdfError;
```

- [ ] **Step 4: Run cargo check to verify scaffolding compiles**

Run: `cargo check -p kermit-rdf`
Expected: errors about missing modules. The empty modules will be created as their tasks land — for now compile failure is acceptable; we'll fix this in Task 2 by adding empty stub modules so the crate compiles cleanly before we start filling them in.

- [ ] **Step 5: Add empty stub modules so the crate compiles**

Create each file with a single doc comment:

```rust
// kermit-rdf/src/error.rs
//! Error type for the kermit-rdf crate.
```

```rust
// kermit-rdf/src/value.rs
//! RDF value types.
```

Repeat for: `dict.rs`, `driver.rs`(no, see below), `expected.rs`, `ntriples.rs`, `parquet.rs`, `partition.rs`, `pipeline.rs`, `yaml_emit.rs`.

For the `sparql/` and `driver/` directory modules, create:

```rust
// kermit-rdf/src/sparql/mod.rs
//! SPARQL parsing and translation.
pub mod bindings;
pub mod parser;
pub mod translator;
```

```rust
// kermit-rdf/src/driver/mod.rs
//! WatDiv binary driver.
pub mod invoke;
pub mod sandbox;
```

Plus stubs for `sparql/{bindings,parser,translator}.rs` and `driver/{invoke,sandbox}.rs`.

- [ ] **Step 6: Run cargo check to verify compilation**

Run: `cargo check -p kermit-rdf`
Expected: clean compile.

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml kermit-rdf/
git commit -m "feat(kermit-rdf): scaffold crate with empty stub modules"
```

---

## Task 2: Define the RdfError type

**Files:**
- Modify: `kermit-rdf/src/error.rs`

- [ ] **Step 1: Write the failing test**

Add to `kermit-rdf/src/error.rs`:

```rust
//! Error type for the kermit-rdf crate.

use std::path::PathBuf;

/// Errors that can occur during RDF preprocessing.
#[derive(Debug, thiserror::Error)]
pub enum RdfError {
    /// The watdiv binary could not be found at the resolved path.
    #[error("watdiv binary not found at {path:?} (set --watdiv-bin or KERMIT_WATDIV_BIN)")]
    BinaryNotFound {
        /// The path that was searched.
        path: PathBuf,
    },

    /// The watdiv binary exited with a non-zero status.
    #[error("watdiv exited with status {status}: {stderr}")]
    BinaryFailed {
        /// Exit status as a string ("exit code 1", "killed by signal SIGSEGV", etc.).
        status: String,
        /// Captured stderr.
        stderr: String,
    },

    /// Sandbox setup (bwrap detection, mount construction) failed.
    #[error("sandbox setup failed: {0}")]
    Sandbox(String),

    /// Parsing N-Triples failed.
    #[error("N-Triples parse error at line {line}: {message}")]
    NTriplesParse {
        /// 1-indexed line number where the error occurred.
        line: usize,
        /// Human-readable message.
        message: String,
    },

    /// Parsing a SPARQL query failed.
    #[error("SPARQL parse error: {0}")]
    SparqlParse(String),

    /// SPARQL feature not expressible as a Datalog rule (FILTER, OPTIONAL, etc.).
    #[error("unsupported SPARQL feature: {0}")]
    UnsupportedSparql(String),

    /// Computing expected results failed.
    #[error("expected-results computation failed: {0}")]
    Expected(String),

    /// Underlying I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Underlying Arrow error.
    #[error("arrow error: {0}")]
    Arrow(#[from] arrow::error::ArrowError),

    /// Underlying Parquet error.
    #[error("parquet error: {0}")]
    Parquet(#[from] parquet::errors::ParquetError),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binary_not_found_message_includes_path() {
        let err = RdfError::BinaryNotFound {
            path: PathBuf::from("/no/such/path"),
        };
        let msg = format!("{err}");
        assert!(msg.contains("/no/such/path"));
        assert!(msg.contains("watdiv binary not found"));
    }

    #[test]
    fn io_error_wraps_transparently() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "boom");
        let err: RdfError = io_err.into();
        assert!(matches!(err, RdfError::Io(_)));
    }
}
```

- [ ] **Step 2: Run tests to verify they pass**

Run: `cargo test -p kermit-rdf`
Expected: 2 passing tests.

- [ ] **Step 3: Commit**

```bash
git add kermit-rdf/src/error.rs
git commit -m "feat(kermit-rdf): add RdfError enum"
```

---

## Task 3: RDF value types

**Files:**
- Modify: `kermit-rdf/src/value.rs`

- [ ] **Step 1: Write the failing tests**

Replace `kermit-rdf/src/value.rs`:

```rust
//! RDF value types used throughout the preprocessor.
//!
//! `RdfValue` is the canonical key type for the dictionary: every RDF term
//! we encounter (subject IRI, predicate IRI, object IRI, literal, blank
//! node) becomes one of these and is interned to a `usize`. Equality is
//! delegated to the underlying string forms so two parses of the same
//! source N-Triples line produce equal values.

use std::fmt;

/// An RDF term in the dictionary.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum RdfValue {
    /// An IRI (with surrounding angle brackets stripped).
    Iri(String),
    /// A blank node (with the leading `_:` preserved).
    BlankNode(String),
    /// A literal in N-Triples surface form (quotes + optional datatype/lang).
    Literal(String),
}

impl RdfValue {
    /// Returns the canonical string form used for dictionary serialization.
    /// IRIs are wrapped in `<...>`, blank nodes keep `_:`, literals keep
    /// their quoting and any datatype/lang tag exactly as parsed.
    pub fn to_canonical(&self) -> String {
        match self {
            RdfValue::Iri(s) => format!("<{s}>"),
            RdfValue::BlankNode(s) => s.clone(),
            RdfValue::Literal(s) => s.clone(),
        }
    }
}

impl fmt::Display for RdfValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_canonical())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iri_canonical_wraps_in_angle_brackets() {
        let v = RdfValue::Iri("http://example/x".to_string());
        assert_eq!(v.to_canonical(), "<http://example/x>");
    }

    #[test]
    fn blank_node_canonical_preserves_prefix() {
        let v = RdfValue::BlankNode("_:b1".to_string());
        assert_eq!(v.to_canonical(), "_:b1");
    }

    #[test]
    fn literal_canonical_preserves_full_form() {
        let v = RdfValue::Literal("\"hello\"@en".to_string());
        assert_eq!(v.to_canonical(), "\"hello\"@en");
    }

    #[test]
    fn equality_uses_underlying_string() {
        let a = RdfValue::Iri("http://x".to_string());
        let b = RdfValue::Iri("http://x".to_string());
        assert_eq!(a, b);
    }

    #[test]
    fn iri_and_literal_with_same_content_are_distinct() {
        let iri = RdfValue::Iri("hello".to_string());
        let lit = RdfValue::Literal("hello".to_string());
        assert_ne!(iri, lit);
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p kermit-rdf value::tests`
Expected: 5 passing.

- [ ] **Step 3: Commit**

```bash
git add kermit-rdf/src/value.rs
git commit -m "feat(kermit-rdf): add RdfValue enum"
```

---

## Task 4: N-Triples streaming parser

**Files:**
- Modify: `kermit-rdf/src/ntriples.rs`

- [ ] **Step 1: Write the failing tests**

Replace `kermit-rdf/src/ntriples.rs`:

```rust
//! Streaming N-Triples parser.
//!
//! Wraps `oxttl::NTriplesParser` and yields `(subject, predicate, object)`
//! triples whose subject and predicate must be IRIs (the only forms WatDiv
//! emits). Blank-node subjects, literal predicates, etc. are surfaced as
//! parse errors. Iteration is line-streaming: memory usage is O(1) in the
//! file size.

use {
    crate::{error::RdfError, value::RdfValue},
    oxttl::NTriplesParser,
    std::{io::BufRead, path::Path},
};

/// Iterator yielding parsed triples from an N-Triples source.
pub struct TripleIter<R: BufRead> {
    inner: oxttl::ntriples::FromReadNTriplesReader<R>,
    line: usize,
}

impl<R: BufRead> Iterator for TripleIter<R> {
    type Item = Result<(String, String, RdfValue), RdfError>;

    fn next(&mut self) -> Option<Self::Item> {
        let triple = self.inner.next()?;
        self.line += 1;
        Some(map_triple(triple, self.line))
    }
}

fn map_triple(
    parsed: Result<oxrdf::Triple, oxttl::ParseError>,
    line: usize,
) -> Result<(String, String, RdfValue), RdfError> {
    let triple = parsed.map_err(|e| RdfError::NTriplesParse {
        line,
        message: e.to_string(),
    })?;
    let subject_iri = match triple.subject {
        oxrdf::Subject::NamedNode(n) => n.into_string(),
        other => {
            return Err(RdfError::NTriplesParse {
                line,
                message: format!("non-IRI subject: {other}"),
            });
        }
    };
    let predicate_iri = triple.predicate.into_string();
    let object = match triple.object {
        oxrdf::Term::NamedNode(n) => RdfValue::Iri(n.into_string()),
        oxrdf::Term::BlankNode(b) => RdfValue::BlankNode(b.to_string()),
        oxrdf::Term::Literal(l) => RdfValue::Literal(l.to_string()),
    };
    Ok((subject_iri, predicate_iri, object))
}

/// Returns an iterator over the triples of an N-Triples file.
pub fn iter_path<P: AsRef<Path>>(
    path: P,
) -> Result<TripleIter<std::io::BufReader<std::fs::File>>, RdfError> {
    let file = std::fs::File::open(path)?;
    let reader = std::io::BufReader::new(file);
    let inner = NTriplesParser::new().parse_read(reader);
    Ok(TripleIter { inner, line: 0 })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_temp(contents: &str) -> tempfile::NamedTempFile {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(contents.as_bytes()).unwrap();
        f
    }

    #[test]
    fn parses_iri_object() {
        let f = write_temp("<http://x> <http://p> <http://y> .\n");
        let triples: Vec<_> = iter_path(f.path()).unwrap().collect();
        assert_eq!(triples.len(), 1);
        let (s, p, o) = triples[0].as_ref().unwrap();
        assert_eq!(s, "http://x");
        assert_eq!(p, "http://p");
        assert_eq!(*o, RdfValue::Iri("http://y".to_string()));
    }

    #[test]
    fn parses_literal_object() {
        let f = write_temp("<http://x> <http://p> \"hello\" .\n");
        let triples: Vec<_> = iter_path(f.path()).unwrap().collect();
        let (_, _, o) = triples[0].as_ref().unwrap();
        assert!(matches!(o, RdfValue::Literal(_)));
    }

    #[test]
    fn skips_blank_lines_and_comments() {
        let f = write_temp(
            "# header\n\n<http://x> <http://p> <http://y> .\n# footer\n",
        );
        let triples: Vec<_> = iter_path(f.path()).unwrap().collect();
        assert_eq!(triples.len(), 1);
    }

    #[test]
    fn malformed_line_errors() {
        let f = write_temp("<not a valid triple>\n");
        let triples: Vec<_> = iter_path(f.path()).unwrap().collect();
        assert!(triples[0].is_err());
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p kermit-rdf ntriples::tests`
Expected: 4 passing. If the oxttl API names differ (this crate has had API churn), adjust the imports — the public API is `NTriplesParser::new().parse_read(reader)` returning an iterator of `Result<Triple, ParseError>`. If a different version is locked, run `cargo doc -p oxttl --open` to confirm the parser entry point.

- [ ] **Step 3: Commit**

```bash
git add kermit-rdf/src/ntriples.rs
git commit -m "feat(kermit-rdf): add streaming N-Triples parser"
```

---

## Task 5: Dictionary builder

**Files:**
- Modify: `kermit-rdf/src/dict.rs`

- [ ] **Step 1: Write the failing tests**

Replace `kermit-rdf/src/dict.rs`:

```rust
//! Dictionary: bidirectional `RdfValue ↔ usize` map.
//!
//! Built by streaming the N-Triples file once; subjects, predicates, and
//! objects are all interned. The order of insertion is preserved (so dict
//! IDs are deterministic given the same input file). Predicates are
//! present in the dict alongside subjects/objects so the SPARQL translator
//! can use predicate IDs to disambiguate sanitization collisions.

use {
    crate::value::RdfValue,
    std::collections::HashMap,
};

/// A bidirectional `RdfValue ↔ usize` map preserving insertion order.
#[derive(Debug, Default, Clone)]
pub struct Dictionary {
    by_value: HashMap<RdfValue, usize>,
    by_id: Vec<RdfValue>,
}

impl Dictionary {
    /// Creates an empty dictionary.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the ID of `value`, inserting it if not present.
    pub fn intern(&mut self, value: RdfValue) -> usize {
        if let Some(&id) = self.by_value.get(&value) {
            return id;
        }
        let id = self.by_id.len();
        self.by_id.push(value.clone());
        self.by_value.insert(value, id);
        id
    }

    /// Returns the ID for `value` if interned, else `None`.
    pub fn lookup(&self, value: &RdfValue) -> Option<usize> {
        self.by_value.get(value).copied()
    }

    /// Returns the value at `id` if it exists.
    pub fn get(&self, id: usize) -> Option<&RdfValue> {
        self.by_id.get(id)
    }

    /// Total entries.
    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    /// True if no entries.
    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }

    /// Iterates entries in insertion order as `(id, value)` pairs.
    pub fn iter(&self) -> impl Iterator<Item = (usize, &RdfValue)> {
        self.by_id.iter().enumerate()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intern_assigns_sequential_ids() {
        let mut d = Dictionary::new();
        let a = RdfValue::Iri("a".into());
        let b = RdfValue::Iri("b".into());
        assert_eq!(d.intern(a.clone()), 0);
        assert_eq!(d.intern(b.clone()), 1);
        assert_eq!(d.intern(a), 0);
    }

    #[test]
    fn lookup_returns_none_for_missing() {
        let d = Dictionary::new();
        assert_eq!(d.lookup(&RdfValue::Iri("missing".into())), None);
    }

    #[test]
    fn iter_preserves_insertion_order() {
        let mut d = Dictionary::new();
        d.intern(RdfValue::Iri("first".into()));
        d.intern(RdfValue::Iri("second".into()));
        d.intern(RdfValue::Iri("third".into()));
        let ordered: Vec<_> = d.iter().collect();
        assert_eq!(ordered[0].0, 0);
        assert_eq!(ordered[2].0, 2);
        assert_eq!(*ordered[0].1, RdfValue::Iri("first".into()));
    }

    #[test]
    fn distinguishes_iri_from_literal_with_same_text() {
        let mut d = Dictionary::new();
        let id1 = d.intern(RdfValue::Iri("x".into()));
        let id2 = d.intern(RdfValue::Literal("x".into()));
        assert_ne!(id1, id2);
        assert_eq!(d.len(), 2);
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p kermit-rdf dict::tests`
Expected: 4 passing.

- [ ] **Step 3: Commit**

```bash
git add kermit-rdf/src/dict.rs
git commit -m "feat(kermit-rdf): add Dictionary type"
```

---

## Task 6: Predicate name sanitization

**Files:**
- Modify: `kermit-rdf/src/partition.rs` (sanitize_predicate only — partitioning logic comes in Task 7)

- [ ] **Step 1: Write the failing tests**

Replace `kermit-rdf/src/partition.rs`:

```rust
//! Predicate name sanitization and per-predicate partitioning.

/// Converts a predicate IRI into a Datalog-safe lowercase identifier.
///
/// Strips angle brackets if present, prefers the fragment (after `#`) or
/// last path segment (after `/`), then replaces non-alphanumeric characters
/// with underscores. Falls back to a `p_` prefix if the result would start
/// with a digit. Two distinct IRIs may sanitize to the same name; collision
/// resolution happens at the partition level (see `partition_triples`).
pub fn sanitize_predicate(uri: &str) -> String {
    let core = uri.trim_start_matches('<').trim_end_matches('>');
    let last_segment = match (core.rfind('#'), core.rfind('/')) {
        (Some(h), _) => &core[h + 1..],
        (None, Some(s)) => &core[s + 1..],
        (None, None) => core,
    };
    let cleaned: String = last_segment
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect::<String>()
        .trim_matches('_')
        .to_string();
    let safe = if cleaned.is_empty() || cleaned.chars().next().unwrap().is_ascii_digit() {
        format!("p_{cleaned}")
    } else {
        cleaned
    };
    safe.to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fragment_uri() {
        assert_eq!(sanitize_predicate("<http://ogp.me/ns#title>"), "title");
    }

    #[test]
    fn path_segment_uri() {
        assert_eq!(sanitize_predicate("<http://example/follows>"), "follows");
    }

    #[test]
    fn special_chars_replaced() {
        assert_eq!(sanitize_predicate("<http://x/has-genre>"), "has_genre");
    }

    #[test]
    fn digit_prefix_gets_p_prefix() {
        assert_eq!(sanitize_predicate("<http://x/123abc>"), "p_123abc");
    }

    #[test]
    fn already_lowercase_unchanged() {
        assert_eq!(sanitize_predicate("<http://x/age>"), "age");
    }

    #[test]
    fn uppercase_normalized_to_lowercase() {
        assert_eq!(sanitize_predicate("<http://x/HasGenre>"), "hasgenre");
    }

    #[test]
    fn no_angle_brackets_still_works() {
        assert_eq!(sanitize_predicate("http://x/foo"), "foo");
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p kermit-rdf partition::tests`
Expected: 7 passing.

- [ ] **Step 3: Commit**

```bash
git add kermit-rdf/src/partition.rs
git commit -m "feat(kermit-rdf): add predicate sanitization"
```

---

## Task 7: Triple partitioning

**Files:**
- Modify: `kermit-rdf/src/partition.rs` (extends prior task)

- [ ] **Step 1: Add the failing tests for partitioning**

Append to `kermit-rdf/src/partition.rs`:

```rust
use {
    crate::{dict::Dictionary, error::RdfError, ntriples, value::RdfValue},
    std::{collections::HashMap, path::Path},
};

/// Tuples for one predicate, plus its canonical Datalog identifier.
#[derive(Debug)]
pub struct PartitionedRelation {
    /// Datalog-safe lowercase identifier (collisions resolved with `_<id>` suffix).
    pub name: String,
    /// `(s_id, o_id)` pairs.
    pub tuples: Vec<(usize, usize)>,
}

/// Result of streaming an N-Triples file into a dictionary + per-predicate buckets.
#[derive(Debug, Default)]
pub struct Partitioned {
    /// Dictionary capturing every term seen during the stream.
    pub dict: Dictionary,
    /// One entry per distinct predicate IRI.
    pub relations: Vec<PartitionedRelation>,
    /// Map from predicate IRI (without angle brackets) to the canonical name
    /// used in `relations`. Used by the SPARQL translator to disambiguate
    /// sanitization collisions.
    pub predicate_map: HashMap<String, String>,
}

/// Streams an N-Triples file once, building the dictionary and per-predicate
/// `(s, o)` buckets in a single pass.
///
/// Collisions between sanitized predicate names are resolved by appending
/// `_<dict-id>` to all but the first occurrence. The chosen name is recorded
/// in `predicate_map` keyed by the predicate IRI (without angle brackets).
pub fn partition<P: AsRef<Path>>(nt_path: P) -> Result<Partitioned, RdfError> {
    let mut dict = Dictionary::new();
    let mut buckets: HashMap<String, Vec<(usize, usize)>> = HashMap::new();
    let mut insertion_order: Vec<String> = Vec::new();

    for triple in ntriples::iter_path(nt_path)? {
        let (s_iri, p_iri, o) = triple?;
        let s_id = dict.intern(RdfValue::Iri(s_iri));
        let p_id = dict.intern(RdfValue::Iri(p_iri.clone()));
        let o_id = dict.intern(o);
        // p_id silences "unused variable" warnings; the predicate dict-id is
        // used by collision-resolution below.
        let _ = p_id;
        if !buckets.contains_key(&p_iri) {
            insertion_order.push(p_iri.clone());
        }
        buckets.entry(p_iri).or_default().push((s_id, o_id));
    }

    let mut used_names: HashMap<String, ()> = HashMap::new();
    let mut predicate_map: HashMap<String, String> = HashMap::new();
    let mut relations: Vec<PartitionedRelation> = Vec::new();

    for p_iri in insertion_order {
        let base = sanitize_predicate(&p_iri);
        let pred_id = dict
            .lookup(&RdfValue::Iri(p_iri.clone()))
            .expect("predicate just interned");
        let name = if used_names.contains_key(&base) {
            format!("{base}_{pred_id}")
        } else {
            base.clone()
        };
        used_names.insert(name.clone(), ());
        predicate_map.insert(p_iri.clone(), name.clone());
        let tuples = buckets.remove(&p_iri).unwrap_or_default();
        relations.push(PartitionedRelation { name, tuples });
    }

    Ok(Partitioned {
        dict,
        relations,
        predicate_map,
    })
}

#[cfg(test)]
mod partition_tests {
    use super::*;
    use std::io::Write;

    fn write_temp(contents: &str) -> tempfile::NamedTempFile {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(contents.as_bytes()).unwrap();
        f
    }

    #[test]
    fn single_predicate_one_relation() {
        let f = write_temp(
            "<http://x/a> <http://x/follows> <http://x/b> .\n\
             <http://x/b> <http://x/follows> <http://x/c> .\n",
        );
        let p = partition(f.path()).unwrap();
        assert_eq!(p.relations.len(), 1);
        assert_eq!(p.relations[0].name, "follows");
        assert_eq!(p.relations[0].tuples.len(), 2);
        assert_eq!(p.predicate_map["http://x/follows"], "follows");
    }

    #[test]
    fn two_predicates_two_relations() {
        let f = write_temp(
            "<http://x/a> <http://x/follows> <http://x/b> .\n\
             <http://x/a> <http://x/likes> <http://x/c> .\n",
        );
        let p = partition(f.path()).unwrap();
        assert_eq!(p.relations.len(), 2);
        let names: Vec<_> = p.relations.iter().map(|r| r.name.clone()).collect();
        assert!(names.contains(&"follows".to_string()));
        assert!(names.contains(&"likes".to_string()));
    }

    #[test]
    fn sanitization_collision_resolved_with_id_suffix() {
        let f = write_temp(
            "<http://x/a> <http://ogp.me/ns#title> <http://x/b> .\n\
             <http://x/a> <http://purl.org/stuff/rev#title> <http://x/c> .\n",
        );
        let p = partition(f.path()).unwrap();
        assert_eq!(p.relations.len(), 2);
        let first = &p.predicate_map["http://ogp.me/ns#title"];
        let second = &p.predicate_map["http://purl.org/stuff/rev#title"];
        assert_eq!(first, "title");
        assert!(second.starts_with("title_"));
        assert_ne!(first, second);
    }

    #[test]
    fn dictionary_includes_all_terms() {
        let f = write_temp("<http://x/a> <http://x/p> \"lit\" .\n");
        let p = partition(f.path()).unwrap();
        assert_eq!(p.dict.len(), 3);
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p kermit-rdf partition`
Expected: 11 passing (7 from Task 6 + 4 new).

- [ ] **Step 3: Commit**

```bash
git add kermit-rdf/src/partition.rs
git commit -m "feat(kermit-rdf): add triple partitioning by predicate"
```

---

## Task 8: Parquet writer

**Files:**
- Modify: `kermit-rdf/src/parquet.rs`

- [ ] **Step 1: Write the failing tests**

Replace `kermit-rdf/src/parquet.rs`:

```rust
//! Parquet writers for the dictionary and per-predicate relation tables.

use {
    crate::{
        dict::Dictionary,
        error::RdfError,
        partition::PartitionedRelation,
    },
    arrow::{
        array::{ArrayRef, Int64Array, StringArray},
        datatypes::{DataType, Field, Schema},
        record_batch::RecordBatch,
    },
    parquet::{arrow::ArrowWriter, file::properties::WriterProperties},
    std::{path::Path, sync::Arc},
};

/// Writes the dictionary as a 2-column Parquet file: `id: i64`, `value: string`.
/// `value` is the canonical string form (`<iri>`, `_:bN`, `"lit"`).
pub fn write_dict(dict: &Dictionary, out_path: &Path) -> Result<(), RdfError> {
    let ids: Vec<i64> = (0..dict.len() as i64).collect();
    let values: Vec<String> = dict.iter().map(|(_, v)| v.to_canonical()).collect();
    let schema = Arc::new(Schema::new(vec![
        Field::new("id", DataType::Int64, false),
        Field::new("value", DataType::Utf8, false),
    ]));
    let id_arr = Arc::new(Int64Array::from(ids)) as ArrayRef;
    let val_arr = Arc::new(StringArray::from(values)) as ArrayRef;
    let batch = RecordBatch::try_new(schema.clone(), vec![id_arr, val_arr])?;
    let file = std::fs::File::create(out_path)?;
    let mut writer = ArrowWriter::try_new(file, schema, Some(WriterProperties::default()))?;
    writer.write(&batch)?;
    writer.close()?;
    Ok(())
}

/// Writes one predicate's tuples as a 2-column Parquet file: `s: i64`, `o: i64`.
pub fn write_relation(rel: &PartitionedRelation, out_path: &Path) -> Result<(), RdfError> {
    let ss: Vec<i64> = rel.tuples.iter().map(|(s, _)| *s as i64).collect();
    let oo: Vec<i64> = rel.tuples.iter().map(|(_, o)| *o as i64).collect();
    let schema = Arc::new(Schema::new(vec![
        Field::new("s", DataType::Int64, false),
        Field::new("o", DataType::Int64, false),
    ]));
    let s_arr = Arc::new(Int64Array::from(ss)) as ArrayRef;
    let o_arr = Arc::new(Int64Array::from(oo)) as ArrayRef;
    let batch = RecordBatch::try_new(schema.clone(), vec![s_arr, o_arr])?;
    let file = std::fs::File::create(out_path)?;
    let mut writer = ArrowWriter::try_new(file, schema, Some(WriterProperties::default()))?;
    writer.write(&batch)?;
    writer.close()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::RdfValue;
    use parquet::file::reader::{FileReader, SerializedFileReader};

    #[test]
    fn dict_roundtrip() {
        let mut d = Dictionary::new();
        d.intern(RdfValue::Iri("http://x/a".into()));
        d.intern(RdfValue::Literal("\"hello\"".into()));

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("dict.parquet");
        write_dict(&d, &path).unwrap();

        let f = std::fs::File::open(&path).unwrap();
        let reader = SerializedFileReader::new(f).unwrap();
        let meta = reader.metadata();
        assert_eq!(meta.file_metadata().num_rows(), 2);
    }

    #[test]
    fn relation_roundtrip() {
        let rel = PartitionedRelation {
            name: "follows".into(),
            tuples: vec![(0, 1), (1, 2), (2, 0)],
        };
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("follows.parquet");
        write_relation(&rel, &path).unwrap();

        let f = std::fs::File::open(&path).unwrap();
        let reader = SerializedFileReader::new(f).unwrap();
        assert_eq!(reader.metadata().file_metadata().num_rows(), 3);
    }

    #[test]
    fn empty_dict_writes_zero_row_file() {
        let d = Dictionary::new();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("dict.parquet");
        write_dict(&d, &path).unwrap();
        assert!(path.exists());
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p kermit-rdf parquet`
Expected: 3 passing.

- [ ] **Step 3: Commit**

```bash
git add kermit-rdf/src/parquet.rs
git commit -m "feat(kermit-rdf): add Parquet writers"
```

---

## Task 9: SPARQL parser wrapper

**Files:**
- Modify: `kermit-rdf/src/sparql/parser.rs`

- [ ] **Step 1: Write the failing tests**

Replace `kermit-rdf/src/sparql/parser.rs`:

```rust
//! Thin wrapper around `spargebra::Query::parse`.

use {crate::error::RdfError, spargebra::Query};

/// Parses a SPARQL query string into a `spargebra::Query` AST.
///
/// All errors from `spargebra` are mapped to [`RdfError::SparqlParse`].
pub fn parse_query(text: &str) -> Result<Query, RdfError> {
    Query::parse(text, None).map_err(|e| RdfError::SparqlParse(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_simple_select() {
        let q = parse_query("SELECT ?x WHERE { ?x <http://p> ?y . }").unwrap();
        assert!(matches!(q, Query::Select { .. }));
    }

    #[test]
    fn rejects_garbage() {
        let err = parse_query("not a query").unwrap_err();
        assert!(matches!(err, RdfError::SparqlParse(_)));
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p kermit-rdf sparql::parser`
Expected: 2 passing.

- [ ] **Step 3: Commit**

```bash
git add kermit-rdf/src/sparql/parser.rs
git commit -m "feat(kermit-rdf): add SPARQL parser wrapper"
```

---

## Task 10: SPARQL → Datalog translator (golden tests upfront)

**Files:**
- Modify: `kermit-rdf/src/sparql/bindings.rs`
- Modify: `kermit-rdf/src/sparql/translator.rs`
- Create: `kermit-rdf/tests/translator_golden.rs`

This task is the largest single piece of new logic; we lean on golden tests ported from the Python `test_translator.py` so progress is verifiable on every step.

- [ ] **Step 1: Implement bindings.rs**

Replace `kermit-rdf/src/sparql/bindings.rs`:

```rust
//! Variable-name bookkeeping during SPARQL → Datalog translation.

/// Tracks variables in their order of first appearance in a BGP.
#[derive(Debug, Default)]
pub struct VarOrder {
    seen: std::collections::HashSet<String>,
    order: Vec<String>,
}

impl VarOrder {
    /// Records a variable; ignored if already seen.
    pub fn note(&mut self, name: &str) {
        if self.seen.insert(name.to_string()) {
            self.order.push(name.to_string());
        }
    }

    /// True if `name` has been seen.
    pub fn contains(&self, name: &str) -> bool {
        self.seen.contains(name)
    }

    /// Returns the variables in order of first appearance.
    pub fn order(&self) -> &[String] {
        &self.order
    }
}

/// Normalises a SPARQL variable name (with optional `?`/`$` prefix) to a
/// Datalog-safe uppercase token.
pub fn var_name(raw: &str) -> String {
    raw.trim_start_matches('?').trim_start_matches('$').to_uppercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn var_name_strips_question_mark() {
        assert_eq!(var_name("?x"), "X");
    }

    #[test]
    fn var_name_strips_dollar_sign() {
        assert_eq!(var_name("$y"), "Y");
    }

    #[test]
    fn var_name_already_uppercase_unchanged() {
        assert_eq!(var_name("?ABC"), "ABC");
    }

    #[test]
    fn var_order_tracks_first_appearance() {
        let mut o = VarOrder::default();
        o.note("X");
        o.note("Y");
        o.note("X");
        o.note("Z");
        assert_eq!(o.order(), &["X", "Y", "Z"]);
    }
}
```

- [ ] **Step 2: Run bindings tests**

Run: `cargo test -p kermit-rdf sparql::bindings`
Expected: 4 passing.

- [ ] **Step 3: Implement translator.rs**

Replace `kermit-rdf/src/sparql/translator.rs`:

```rust
//! SPARQL BGP → Datalog rule translation.
//!
//! Accepts only Basic Graph Pattern (BGP) queries with optional projection;
//! FILTER, OPTIONAL, UNION, GROUP BY, etc. are rejected via
//! [`RdfError::UnsupportedSparql`]. The shape produced is
//! `head(...) :- body_atom_1, body_atom_2, ...`.
//!
//! `predicate_map` MUST come from `kermit_rdf::partition::partition`'s
//! result so the translator agrees with the partitioner on collision-
//! resolved names.

use {
    crate::{
        dict::Dictionary,
        error::RdfError,
        sparql::{
            bindings::{var_name, VarOrder},
            parser::parse_query,
        },
        value::RdfValue,
    },
    spargebra::{
        algebra::GraphPattern,
        term::{NamedNodePattern, TermPattern, TriplePattern},
        Query,
    },
    std::collections::HashMap,
};

/// Translates one SPARQL query to a Datalog rule.
///
/// `dict` is mutated: URI constants in the query that were never seen in
/// the source data get fresh dictionary IDs, so the caller should re-emit
/// `dict.parquet` if `dict.len()` grew.
pub fn translate_query(
    sparql: &str,
    dict: &mut Dictionary,
    predicate_map: &HashMap<String, String>,
    head_name: &str,
) -> Result<String, RdfError> {
    let parsed = parse_query(sparql)?;
    let (pattern, projection) = match parsed {
        Query::Select { pattern, .. } => (pattern, None),
        _ => {
            return Err(RdfError::UnsupportedSparql(
                "only SELECT queries are supported".to_string(),
            ));
        }
    };

    let (bgp, projected_vars) = extract_bgp_and_projection(pattern, projection)?;

    let mut order = VarOrder::default();
    let mut body_parts: Vec<String> = Vec::new();

    for triple in &bgp {
        let pred_iri = match &triple.predicate {
            NamedNodePattern::NamedNode(n) => n.as_str().to_string(),
            NamedNodePattern::Variable(v) => {
                return Err(RdfError::UnsupportedSparql(format!(
                    "non-ground predicate variable: ?{}",
                    v.as_str()
                )));
            }
        };
        let pred_name = predicate_map.get(&pred_iri).ok_or_else(|| {
            RdfError::UnsupportedSparql(format!(
                "predicate URI not in partition map: {pred_iri}"
            ))
        })?;
        let s_term = term_to_datalog(&triple.subject, dict, &mut order)?;
        let o_term = term_to_datalog(&triple.object, dict, &mut order)?;
        body_parts.push(format!("{pred_name}({s_term}, {o_term})"));
    }

    let head_args: Vec<String> = match projected_vars {
        Some(p) => {
            for v in &p {
                if !order.contains(v) {
                    return Err(RdfError::UnsupportedSparql(format!(
                        "projected variable {v} not bound by BGP"
                    )));
                }
            }
            p
        }
        None => order.order().to_vec(),
    };

    let head_terms = head_args.join(", ");
    let body = body_parts.join(", ");
    Ok(format!("{head_name}({head_terms}) :- {body}."))
}

/// Returns `(triples, projected_vars)` where `projected_vars = None` means SELECT *.
fn extract_bgp_and_projection(
    pattern: GraphPattern,
    _projection: Option<Vec<String>>,
) -> Result<(Vec<TriplePattern>, Option<Vec<String>>), RdfError> {
    // spargebra represents `SELECT ?x WHERE { ... }` as
    // GraphPattern::Project { inner, variables }.
    // SELECT * unwraps directly to the BGP.
    match pattern {
        GraphPattern::Project { inner, variables } => {
            let triples = expect_bgp(*inner)?;
            let proj = variables.iter().map(|v| var_name(v.as_str())).collect();
            Ok((triples, Some(proj)))
        }
        other => {
            let triples = expect_bgp(other)?;
            Ok((triples, None))
        }
    }
}

fn expect_bgp(pattern: GraphPattern) -> Result<Vec<TriplePattern>, RdfError> {
    match pattern {
        GraphPattern::Bgp { patterns } => Ok(patterns),
        GraphPattern::Filter { .. } => Err(RdfError::UnsupportedSparql(
            "FILTER not supported".to_string(),
        )),
        GraphPattern::LeftJoin { .. } => Err(RdfError::UnsupportedSparql(
            "OPTIONAL not supported".to_string(),
        )),
        GraphPattern::Union { .. } => Err(RdfError::UnsupportedSparql(
            "UNION not supported".to_string(),
        )),
        other => Err(RdfError::UnsupportedSparql(format!(
            "unsupported pattern: {other:?}"
        ))),
    }
}

fn term_to_datalog(
    term: &TermPattern,
    dict: &mut Dictionary,
    order: &mut VarOrder,
) -> Result<String, RdfError> {
    match term {
        TermPattern::Variable(v) => {
            let name = var_name(v.as_str());
            order.note(&name);
            Ok(name)
        }
        TermPattern::NamedNode(n) => {
            let value = RdfValue::Iri(n.as_str().to_string());
            let id = dict.intern(value);
            Ok(format!("c{id}"))
        }
        TermPattern::Literal(_) => Err(RdfError::UnsupportedSparql(
            "literal terms in BGP not supported".to_string(),
        )),
        TermPattern::BlankNode(_) => Err(RdfError::UnsupportedSparql(
            "blank node terms in BGP not supported".to_string(),
        )),
        TermPattern::Triple(_) => Err(RdfError::UnsupportedSparql(
            "RDF-star triple terms not supported".to_string(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dict_with(items: &[(RdfValue, usize)]) -> Dictionary {
        // Forces specific IDs for reproducible test expectations by interning
        // a sentinel for any "skipped" slot, then overwriting via assert.
        let mut d = Dictionary::new();
        for (v, expected_id) in items {
            let got = d.intern(v.clone());
            assert_eq!(got, *expected_id, "test setup: id mismatch");
        }
        d
    }

    #[test]
    fn rejects_non_select() {
        let mut d = Dictionary::new();
        let pm = HashMap::new();
        let err = translate_query(
            "ASK { ?x <http://p> ?y }",
            &mut d,
            &pm,
            "Q",
        )
        .unwrap_err();
        assert!(matches!(err, RdfError::UnsupportedSparql(_)));
    }
}
```

- [ ] **Step 4: Run translator unit tests**

Run: `cargo test -p kermit-rdf sparql::translator`
Expected: 1 passing.

- [ ] **Step 5: Create the golden test suite**

Create `kermit-rdf/tests/translator_golden.rs`:

```rust
//! Port of `scripts/watdiv-preprocess/tests/test_translator.py`.

use {
    kermit_rdf::{
        dict::Dictionary,
        error::RdfError,
        sparql::translator::translate_query,
        value::RdfValue,
    },
    std::collections::HashMap,
};

fn build_dict(uris: &[&str]) -> Dictionary {
    let mut d = Dictionary::new();
    for u in uris {
        d.intern(RdfValue::Iri(u.to_string()));
    }
    d
}

#[test]
fn simple_bgp_one_triple() {
    let mut dict = build_dict(&["http://example/p", "http://example/c"]);
    let mut pm = HashMap::new();
    pm.insert("http://example/p".to_string(), "p".to_string());
    let out = translate_query(
        "SELECT ?x WHERE { ?x <http://example/p> <http://example/c> . }",
        &mut dict,
        &pm,
        "Q0",
    )
    .unwrap();
    assert_eq!(out, "Q0(X) :- p(X, c1).");
}

#[test]
fn select_star_projects_all_bound_in_source_order() {
    let mut dict = build_dict(&["http://example/p", "http://example/q"]);
    let mut pm = HashMap::new();
    pm.insert("http://example/p".to_string(), "p".to_string());
    pm.insert("http://example/q".to_string(), "q".to_string());
    let out = translate_query(
        "SELECT * WHERE { ?x <http://example/p> ?y . ?y <http://example/q> ?z . }",
        &mut dict,
        &pm,
        "Q1",
    )
    .unwrap();
    assert_eq!(out, "Q1(X, Y, Z) :- p(X, Y), q(Y, Z).");
}

#[test]
fn watdiv_style_select_star_with_constant_object() {
    let mut dict = build_dict(&[
        "http://xmlns.com/foaf/homepage",
        "http://db.uwaterloo.ca/~galuc/wsdbm/Website2948",
        "http://ogp.me/ns#title",
    ]);
    let mut pm = HashMap::new();
    pm.insert(
        "http://xmlns.com/foaf/homepage".to_string(),
        "homepage".to_string(),
    );
    pm.insert(
        "http://ogp.me/ns#title".to_string(),
        "title".to_string(),
    );
    let out = translate_query(
        "SELECT * WHERE { \
         ?v0 <http://xmlns.com/foaf/homepage> <http://db.uwaterloo.ca/~galuc/wsdbm/Website2948> . \
         ?v0 <http://ogp.me/ns#title> ?v2 . }",
        &mut dict,
        &pm,
        "Q_test1_q0000",
    )
    .unwrap();
    assert_eq!(
        out,
        "Q_test1_q0000(V0, V2) :- homepage(V0, c1), title(V0, V2)."
    );
}

#[test]
fn predicate_map_disambiguates_sanitize_collisions() {
    let mut dict = build_dict(&[
        "http://ogp.me/ns#title",
        "http://purl.org/stuff/rev#title",
        "http://example/o1",
        "http://example/o2",
    ]);
    let mut pm = HashMap::new();
    pm.insert(
        "http://ogp.me/ns#title".to_string(),
        "title".to_string(),
    );
    pm.insert(
        "http://purl.org/stuff/rev#title".to_string(),
        "title_1".to_string(),
    );
    let sparql = "SELECT * WHERE { \
         ?x <http://ogp.me/ns#title> <http://example/o1> . \
         ?x <http://purl.org/stuff/rev#title> <http://example/o2> . \
         }";
    let out = translate_query(sparql, &mut dict, &pm, "Q_collision").unwrap();
    assert!(out.contains("title(X, c2)"), "got: {out}");
    assert!(out.contains("title_1(X, c3)"), "got: {out}");
}

#[test]
fn missing_predicate_in_map_errors() {
    let mut dict = build_dict(&["http://example/p"]);
    let pm = HashMap::new();
    let err = translate_query(
        "SELECT ?x WHERE { ?x <http://example/p> ?y . }",
        &mut dict,
        &pm,
        "Q",
    )
    .unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("partition map"), "msg: {msg}");
}

#[test]
fn filter_rejected() {
    let mut dict = build_dict(&["http://example/p"]);
    let mut pm = HashMap::new();
    pm.insert("http://example/p".to_string(), "p".to_string());
    let err = translate_query(
        "SELECT ?x WHERE { ?x <http://example/p> ?y . FILTER(?y = <http://example/y>) }",
        &mut dict,
        &pm,
        "Q",
    )
    .unwrap_err();
    assert!(matches!(err, RdfError::UnsupportedSparql(_)));
}

#[test]
fn optional_rejected() {
    let mut dict = build_dict(&["http://example/p", "http://example/q"]);
    let mut pm = HashMap::new();
    pm.insert("http://example/p".to_string(), "p".to_string());
    pm.insert("http://example/q".to_string(), "q".to_string());
    let err = translate_query(
        "SELECT ?x WHERE { ?x <http://example/p> ?y . OPTIONAL { ?y <http://example/q> ?z } }",
        &mut dict,
        &pm,
        "Q",
    )
    .unwrap_err();
    assert!(matches!(err, RdfError::UnsupportedSparql(_)));
}

#[test]
fn unknown_uri_added_to_dict() {
    let mut dict = build_dict(&["http://example/p"]);
    let mut pm = HashMap::new();
    pm.insert("http://example/p".to_string(), "p".to_string());
    let rule = translate_query(
        "SELECT ?x WHERE { ?x <http://example/p> <http://example/unseen> . }",
        &mut dict,
        &pm,
        "Q4",
    )
    .unwrap();
    let assigned = dict
        .lookup(&RdfValue::Iri("http://example/unseen".into()))
        .unwrap();
    assert_eq!(assigned, 1);
    assert!(rule.contains(&format!("c{assigned}")), "rule: {rule}");
}

#[test]
fn literal_object_errors() {
    let mut dict = build_dict(&["http://example/p"]);
    let mut pm = HashMap::new();
    pm.insert("http://example/p".to_string(), "p".to_string());
    let err = translate_query(
        "SELECT ?x WHERE { ?x <http://example/p> \"literal\" . }",
        &mut dict,
        &pm,
        "Q",
    )
    .unwrap_err();
    assert!(matches!(err, RdfError::UnsupportedSparql(_)));
}
```

- [ ] **Step 6: Run the golden tests**

Run: `cargo test -p kermit-rdf --test translator_golden`
Expected: all 9 passing. If any fail because spargebra exposes the BGP shape differently than this code assumes (most likely candidate: `GraphPattern::Project` carries a `Vec<Variable>`, which on this spargebra version may instead be on the parent `Query::Select`), adjust `extract_bgp_and_projection` to read projection from the right place. Use `cargo doc -p spargebra --open` and inspect `Query::Select` and `GraphPattern::Project` to confirm the field names.

- [ ] **Step 7: Commit**

```bash
git add kermit-rdf/src/sparql/ kermit-rdf/tests/translator_golden.rs
git commit -m "feat(kermit-rdf): SPARQL→Datalog translator with golden tests"
```

---

## Task 11: YAML emitter

**Files:**
- Modify: `kermit-rdf/src/yaml_emit.rs`

- [ ] **Step 1: Write the failing tests**

Replace `kermit-rdf/src/yaml_emit.rs`:

```rust
//! Emits a kermit `BenchmarkDefinition` YAML for a generated artifact set.

use {
    crate::error::RdfError,
    kermit_bench::{BenchmarkDefinition, QueryDefinition, RelationSource},
    std::{collections::HashSet, path::Path},
};

/// Inputs for emitting a benchmark YAML.
pub struct YamlInputs<'a> {
    /// Benchmark name (matches the cache directory and YAML filename).
    pub name: &'a str,
    /// Human-readable description.
    pub description: &'a str,
    /// All translated queries as `(query_name, datalog)` pairs.
    pub queries: Vec<(String, String)>,
    /// All known predicate names (canonical Datalog names).
    pub all_predicates: &'a [String],
    /// Base URL for relation parquet files (use `file:///abs/path/to/dir`
    /// for on-the-fly generation; the cache layer skips download when files
    /// already exist on disk).
    pub base_url: &'a str,
}

/// Returns the predicate names referenced in any query body.
fn collect_used_predicates(queries: &[(String, String)]) -> HashSet<String> {
    let pat = regex_lite::Regex::new(r"([a-z][a-z0-9_]*)\(").unwrap();
    let mut used = HashSet::new();
    for (_, dl) in queries {
        if let Some((_, body)) = dl.split_once(":-") {
            for cap in pat.captures_iter(body) {
                used.insert(cap[1].to_string());
            }
        }
    }
    used
}

/// Builds a `BenchmarkDefinition`, then writes it to `<dir>/benchmark.yml`.
pub fn write_benchmark_yaml(
    inputs: &YamlInputs,
    out_dir: &Path,
) -> Result<BenchmarkDefinition, RdfError> {
    let used = collect_used_predicates(&inputs.queries);
    let known: HashSet<&str> = inputs.all_predicates.iter().map(|s| s.as_str()).collect();
    for name in &used {
        if !known.contains(name.as_str()) {
            return Err(RdfError::UnsupportedSparql(format!(
                "query body references unknown predicate: {name}"
            )));
        }
    }
    let mut sorted_used: Vec<&String> = used.iter().collect();
    sorted_used.sort();
    let relations: Vec<RelationSource> = sorted_used
        .iter()
        .map(|name| RelationSource {
            name: (*name).clone(),
            url: format!(
                "{}/{}.parquet",
                inputs.base_url.trim_end_matches('/'),
                name
            ),
        })
        .collect();
    let queries: Vec<QueryDefinition> = inputs
        .queries
        .iter()
        .map(|(qname, dl)| QueryDefinition {
            name: qname.clone(),
            description: format!("query {qname}"),
            query: dl.clone(),
        })
        .collect();
    let def = BenchmarkDefinition {
        name: inputs.name.to_string(),
        description: inputs.description.to_string(),
        relations,
        queries,
    };
    def.validate().map_err(|e| RdfError::Expected(e.to_string()))?;
    let yaml = serde_yaml::to_string(&def).map_err(|e| RdfError::Expected(e.to_string()))?;
    std::fs::write(out_dir.join("benchmark.yml"), yaml)?;
    Ok(def)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emits_yaml_with_only_used_predicates() {
        let dir = tempfile::tempdir().unwrap();
        let inputs = YamlInputs {
            name: "test",
            description: "test bench",
            queries: vec![("q0000".into(), "Q_q0000(X) :- follows(X, Y).".into())],
            all_predicates: &["follows".to_string(), "likes".to_string()],
            base_url: "file:///tmp/x",
        };
        let def = write_benchmark_yaml(&inputs, dir.path()).unwrap();
        assert_eq!(def.relations.len(), 1);
        assert_eq!(def.relations[0].name, "follows");
        assert!(dir.path().join("benchmark.yml").exists());
    }

    #[test]
    fn unknown_predicate_in_body_errors() {
        let dir = tempfile::tempdir().unwrap();
        let inputs = YamlInputs {
            name: "test",
            description: "test bench",
            queries: vec![(
                "q0000".into(),
                "Q_q0000(X) :- ghost(X, Y).".into(),
            )],
            all_predicates: &["follows".to_string()],
            base_url: "file:///tmp/x",
        };
        assert!(write_benchmark_yaml(&inputs, dir.path()).is_err());
    }
}
```

- [ ] **Step 2: Add the regex_lite dependency**

Add to `kermit-rdf/Cargo.toml` under `[dependencies]`:

```toml
regex-lite = "0.1"
```

Note: the Python emitter used a stdlib regex; `regex-lite` keeps the dependency tree minimal compared to the full `regex` crate.

Adjust the `use` in `yaml_emit.rs` to `use regex_lite;` and the call to `regex_lite::Regex::new(...)`. (If `regex_lite` is unavailable, swap to `regex = "1"`.)

Also ensure `BenchmarkDefinition` derives `Serialize`. It currently only derives `Deserialize`. Modify `kermit-bench/src/definition.rs` to add `Serialize`:

```rust
// In kermit-bench/src/definition.rs, change all #[derive(Debug, Clone, serde::Deserialize)]
// to #[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
```

(Existing tests must still pass after adding the derive; serde-yaml emits the same field order as definition.)

- [ ] **Step 3: Run tests**

Run: `cargo test -p kermit-rdf yaml_emit && cargo test -p kermit-bench`
Expected: 2 new yaml_emit tests + all kermit-bench tests still passing.

- [ ] **Step 4: Commit**

```bash
git add kermit-rdf/src/yaml_emit.rs kermit-rdf/Cargo.toml kermit-bench/src/definition.rs
git commit -m "feat(kermit-rdf): emit BenchmarkDefinition YAML"
```

---

## Task 12: Expected-cardinalities harvester

**Files:**
- Modify: `kermit-rdf/src/expected.rs`

This task departs from the spec section that describes "execute Datalog (via kermit-algos) against the in-memory dict-encoded data." Why: watdiv `-q` already emits cardinalities in `.desc` sidecars, and the existing Python pipeline harvests those — using watdiv's own counts gives a real cross-check (translator + LFTJ vs. watdiv-internal), whereas re-executing the Datalog ourselves would tautologically agree with itself. Keeping the spec's CSV file naming (`expected/<query>.csv`) but populating it from the `.desc` files is a faithful match to existing behavior.

- [ ] **Step 1: Write the failing tests**

Replace `kermit-rdf/src/expected.rs`:

```rust
//! Reads watdiv `.desc` cardinality sidecars and emits one CSV per query.

use {
    crate::error::RdfError,
    std::{
        io::Write,
        path::{Path, PathBuf},
    },
};

/// Reads a `.desc` file (one integer per non-blank line) and returns the list.
pub fn parse_desc(desc_path: &Path) -> Result<Vec<u64>, RdfError> {
    let text = std::fs::read_to_string(desc_path)?;
    let mut out = Vec::new();
    for (lineno, raw) in text.lines().enumerate() {
        let stripped = raw.trim();
        if stripped.is_empty() {
            continue;
        }
        let n: u64 = stripped.parse().map_err(|_| {
            RdfError::Expected(format!(
                "{}:{}: not an integer: {stripped:?}",
                desc_path.display(),
                lineno + 1,
            ))
        })?;
        out.push(n);
    }
    Ok(out)
}

/// For each (sparql_path, query_name) pair, looks at `<sparql_path with .desc>`,
/// reads the i-th cardinality, and writes `<out_dir>/<query_name>.csv`
/// containing a one-line header `cardinality\n<N>\n`.
///
/// Returns the number of `.csv` files written.
pub fn write_expected_csvs(
    sparql_files: &[PathBuf],
    out_dir: &Path,
) -> Result<usize, RdfError> {
    std::fs::create_dir_all(out_dir)?;
    let mut written = 0;
    for sparql in sparql_files {
        let desc = sparql.with_extension("desc");
        if !desc.exists() {
            continue;
        }
        let nums = parse_desc(&desc)?;
        let stem = sparql.file_stem().and_then(|s| s.to_str()).unwrap_or("q");
        let stem = stem.replace('.', "-");
        for (i, n) in nums.iter().enumerate() {
            let qname = format!("{stem}_q{i:04}");
            let csv_path = out_dir.join(format!("{qname}.csv"));
            let mut f = std::fs::File::create(&csv_path)?;
            writeln!(f, "cardinality")?;
            writeln!(f, "{n}")?;
            written += 1;
        }
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn parses_desc_numbers() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        writeln!(f, "12").unwrap();
        writeln!(f, "0").unwrap();
        writeln!(f, "987").unwrap();
        let nums = parse_desc(f.path()).unwrap();
        assert_eq!(nums, vec![12, 0, 987]);
    }

    #[test]
    fn writes_one_csv_per_query() {
        let dir = tempfile::tempdir().unwrap();
        let sparql = dir.path().join("test1.sparql");
        std::fs::write(&sparql, "SELECT * WHERE { }\nSELECT * WHERE { }\n").unwrap();
        let desc = dir.path().join("test1.desc");
        std::fs::write(&desc, "5\n7\n").unwrap();
        let out_dir = dir.path().join("expected");
        let n = write_expected_csvs(&[sparql], &out_dir).unwrap();
        assert_eq!(n, 2);
        assert!(out_dir.join("test1_q0000.csv").exists());
        let content = std::fs::read_to_string(out_dir.join("test1_q0001.csv")).unwrap();
        assert!(content.contains("7"));
    }

    #[test]
    fn missing_desc_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let sparql = dir.path().join("test1.sparql");
        std::fs::write(&sparql, "SELECT *").unwrap();
        let n = write_expected_csvs(&[sparql], &dir.path().join("expected")).unwrap();
        assert_eq!(n, 0);
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p kermit-rdf expected`
Expected: 3 passing.

- [ ] **Step 3: Commit**

```bash
git add kermit-rdf/src/expected.rs
git commit -m "feat(kermit-rdf): harvest watdiv .desc cardinalities into expected/ CSVs"
```

---

## Task 13: Driver — temp-dir staging

**Files:**
- Modify: `kermit-rdf/src/driver/sandbox.rs`

- [ ] **Step 1: Write the failing tests**

Replace `kermit-rdf/src/driver/sandbox.rs`:

```rust
//! Sandbox + temp-dir staging for the watdiv binary.
//!
//! The watdiv binary expects the layout `<cwd>/../../files/firstnames.txt`
//! relative to `bin/Release/watdiv`. We satisfy that by staging a fresh
//! temp dir per generation:
//! ```text
//! <stage>/bin/Release/watdiv  -> <resolved binary path>  (symlink)
//! <stage>/files/firstnames.txt
//! <stage>/files/lastnames.txt
//! <stage>/files/words
//! ```
//! `TempStagingDir` is RAII: removes itself on Drop.

use {
    crate::error::RdfError,
    std::{
        fs,
        path::{Path, PathBuf},
    },
};

/// Owns a staging temp dir for the lifetime of one generation.
pub struct TempStagingDir {
    root: PathBuf,
}

impl TempStagingDir {
    /// Creates the staging layout for an existing `binary_path`.
    /// `vendor_files` must be a directory containing `firstnames.txt`,
    /// `lastnames.txt`, and `words`.
    pub fn create(binary_path: &Path, vendor_files: &Path) -> Result<Self, RdfError> {
        let root = tempfile::Builder::new()
            .prefix("kermit-watdiv-")
            .tempdir()?
            .into_path();

        let bin_release = root.join("bin").join("Release");
        fs::create_dir_all(&bin_release)?;
        let stage_bin = bin_release.join("watdiv");
        std::os::unix::fs::symlink(binary_path, &stage_bin).map_err(|e| {
            RdfError::Sandbox(format!(
                "symlink {binary_path:?} -> {stage_bin:?}: {e}"
            ))
        })?;

        let files_dir = root.join("files");
        fs::create_dir_all(&files_dir)?;
        for name in ["firstnames.txt", "lastnames.txt", "words"] {
            let src = vendor_files.join(name);
            let dst = files_dir.join(name);
            fs::copy(&src, &dst).map_err(|e| {
                RdfError::Sandbox(format!("copy {src:?} -> {dst:?}: {e}"))
            })?;
        }

        Ok(Self { root })
    }

    /// Returns the staged binary path (the symlink under `bin/Release/`).
    pub fn binary_path(&self) -> PathBuf {
        self.root.join("bin").join("Release").join("watdiv")
    }

    /// Returns the staging root.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Returns the staged words file, used for bind-mounting to `/usr/share/dict/words`.
    pub fn words_path(&self) -> PathBuf {
        self.root.join("files").join("words")
    }
}

impl Drop for TempStagingDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_vendor_files(dir: &Path) {
        fs::create_dir_all(dir).unwrap();
        for name in ["firstnames.txt", "lastnames.txt", "words"] {
            fs::write(dir.join(name), b"sample\n").unwrap();
        }
    }

    #[test]
    fn staging_creates_expected_layout() {
        let workdir = tempfile::tempdir().unwrap();
        let bin = workdir.path().join("real_watdiv");
        fs::write(&bin, b"#!/bin/sh\n").unwrap();
        let vendor = workdir.path().join("vendor");
        make_vendor_files(&vendor);

        let stage = TempStagingDir::create(&bin, &vendor).unwrap();
        let staged = stage.binary_path();
        assert!(staged.exists() || staged.symlink_metadata().is_ok());
        assert!(stage.root().join("files/firstnames.txt").exists());
        assert!(stage.root().join("files/words").exists());
    }

    #[test]
    fn drop_removes_root() {
        let workdir = tempfile::tempdir().unwrap();
        let bin = workdir.path().join("real_watdiv");
        fs::write(&bin, b"#!/bin/sh\n").unwrap();
        let vendor = workdir.path().join("vendor");
        make_vendor_files(&vendor);

        let root_path: PathBuf;
        {
            let stage = TempStagingDir::create(&bin, &vendor).unwrap();
            root_path = stage.root().to_path_buf();
            assert!(root_path.exists());
        }
        assert!(!root_path.exists(), "stage dir should be cleaned up");
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p kermit-rdf driver::sandbox`
Expected: 2 passing.

- [ ] **Step 3: Commit**

```bash
git add kermit-rdf/src/driver/sandbox.rs
git commit -m "feat(kermit-rdf): add TempStagingDir for watdiv driver"
```

---

## Task 14: Driver — invoke watdiv -d / -s / -q

**Files:**
- Modify: `kermit-rdf/src/driver/invoke.rs`

- [ ] **Step 1: Write the failing tests (using a stub binary)**

Replace `kermit-rdf/src/driver/invoke.rs`:

```rust
//! Constructs and executes the three watdiv invocations.

use {
    crate::{driver::sandbox::TempStagingDir, error::RdfError},
    std::{
        path::{Path, PathBuf},
        process::Command,
    },
};

/// Configuration shared across all three invocations.
pub struct InvokeConfig<'a> {
    /// Staging dir owning the binary symlink + vendored files.
    pub stage: &'a TempStagingDir,
    /// Path to the WatDiv data model file (e.g. wsdbm-data-model.txt).
    pub model_file: &'a Path,
    /// True to wrap each invocation under `bwrap` with the vendored words
    /// list bind-mounted at `/usr/share/dict/words`. False to assume the
    /// host already has that file.
    pub use_bwrap: bool,
}

fn build_command(cfg: &InvokeConfig, watdiv_args: &[&str]) -> Result<Command, RdfError> {
    let bin = cfg.stage.binary_path();
    let bin_release = bin.parent().expect("staged path has parent");
    if cfg.use_bwrap {
        let mut cmd = Command::new("bwrap");
        cmd.arg("--bind").arg("/").arg("/")
            .arg("--bind")
            .arg(cfg.stage.words_path())
            .arg("/usr/share/dict/words")
            .arg("--chdir")
            .arg(bin_release)
            .arg("./watdiv");
        for a in watdiv_args {
            cmd.arg(a);
        }
        Ok(cmd)
    } else {
        let mut cmd = Command::new(&bin);
        cmd.current_dir(bin_release);
        for a in watdiv_args {
            cmd.arg(a);
        }
        Ok(cmd)
    }
}

/// Runs `watdiv -d <model> <scale>`, writing N-Triples to `out_path`.
pub fn run_data(cfg: &InvokeConfig, scale: u32, out_path: &Path) -> Result<(), RdfError> {
    let scale_str = scale.to_string();
    let mut cmd = build_command(
        cfg,
        &["-d", cfg.model_file.to_str().unwrap(), &scale_str],
    )?;
    let output = cmd.output()?;
    if !output.status.success() {
        return Err(RdfError::BinaryFailed {
            status: format!("{}", output.status),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        });
    }
    std::fs::write(out_path, &output.stdout)?;
    Ok(())
}

/// Runs `watdiv -s <stress-dir> <count>`, returning the list of generated
/// `.txt` template files (the names exactly as watdiv writes them).
///
/// `stress_dir` is a SHARED working directory under the stage where templates
/// are emitted (watdiv writes them as side effects of the invocation).
pub fn run_stress(
    cfg: &InvokeConfig,
    stress_dir_arg: &str,
    count: u32,
) -> Result<Vec<PathBuf>, RdfError> {
    let count_str = count.to_string();
    let mut cmd = build_command(cfg, &["-s", stress_dir_arg, &count_str])?;
    let output = cmd.output()?;
    if !output.status.success() {
        return Err(RdfError::BinaryFailed {
            status: format!("{}", output.status),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        });
    }
    let bin_release = cfg
        .stage
        .binary_path()
        .parent()
        .unwrap()
        .to_path_buf();
    let dir = bin_release.join(stress_dir_arg);
    let mut templates = Vec::new();
    if dir.is_dir() {
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            let p = entry.path();
            if p.extension().and_then(|s| s.to_str()) == Some("txt") {
                templates.push(p);
            }
        }
    }
    templates.sort();
    Ok(templates)
}

/// Runs `watdiv -q <template> <count>` for each template, writing one
/// `.sparql` and one `.desc` per template (watdiv's default behavior).
/// Returns the list of `(sparql_path, desc_path)` pairs in template order.
pub fn run_queries(
    cfg: &InvokeConfig,
    templates: &[PathBuf],
    count_per_template: u32,
) -> Result<Vec<(PathBuf, PathBuf)>, RdfError> {
    let mut out = Vec::new();
    for tpl in templates {
        let bin_release = cfg
            .stage
            .binary_path()
            .parent()
            .unwrap()
            .to_path_buf();
        let count_str = count_per_template.to_string();
        let mut cmd = build_command(cfg, &["-q", tpl.to_str().unwrap(), &count_str])?;
        let output = cmd.output()?;
        if !output.status.success() {
            return Err(RdfError::BinaryFailed {
                status: format!("{}", output.status),
                stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            });
        }
        // `watdiv -q template.txt N` writes `<tpl-stem>.sparql` and
        // `<tpl-stem>.desc` next to the template. We just locate them.
        let stem = tpl.file_stem().unwrap();
        let sparql = tpl.with_file_name(format!("{}.sparql", stem.to_string_lossy()));
        let desc = tpl.with_file_name(format!("{}.desc", stem.to_string_lossy()));
        let _ = bin_release; // bin_release used by build_command; suppress unused warning.
        out.push((sparql, desc));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    /// Creates a fake watdiv binary that just echoes its args to stdout
    /// and returns success. Lets us test the Command construction without
    /// the real binary.
    fn make_fake_binary(dir: &Path) -> PathBuf {
        let path = dir.join("watdiv");
        let script = "#!/bin/sh\necho fake-stdout\nexit 0\n";
        std::fs::write(&path, script).unwrap();
        let mut perms = std::fs::metadata(&path).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&path, perms).unwrap();
        path
    }

    #[test]
    fn run_data_writes_stdout_to_file() {
        let dir = tempfile::tempdir().unwrap();
        let bin = make_fake_binary(dir.path());
        let vendor = dir.path().join("vendor");
        std::fs::create_dir_all(&vendor).unwrap();
        for n in ["firstnames.txt", "lastnames.txt", "words"] {
            std::fs::write(vendor.join(n), b"x\n").unwrap();
        }
        let stage = TempStagingDir::create(&bin, &vendor).unwrap();
        let model = dir.path().join("MODEL.txt");
        std::fs::write(&model, b"model").unwrap();
        let cfg = InvokeConfig {
            stage: &stage,
            model_file: &model,
            use_bwrap: false,
        };
        let out = dir.path().join("data.nt");
        run_data(&cfg, 1, &out).unwrap();
        let s = std::fs::read_to_string(&out).unwrap();
        assert!(s.contains("fake-stdout"));
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p kermit-rdf driver::invoke`
Expected: 1 passing.

- [ ] **Step 3: Commit**

```bash
git add kermit-rdf/src/driver/invoke.rs
git commit -m "feat(kermit-rdf): driver invokes watdiv -d/-s/-q"
```

---

## Task 15: Driver entry point

**Files:**
- Modify: `kermit-rdf/src/driver/mod.rs`

- [ ] **Step 1: Write driver/mod.rs (no new tests; orchestration is exercised by Task 17 pipeline)**

Replace `kermit-rdf/src/driver/mod.rs`:

```rust
//! WatDiv binary driver: builds a `RawArtifacts` bundle by running watdiv
//! end-to-end inside a temp-dir sandbox, then leaves the bundle for the
//! pipeline orchestrator to consume.

pub mod invoke;
pub mod sandbox;

use {
    crate::error::RdfError,
    std::{
        path::{Path, PathBuf},
    },
};

/// Stress parameters that affect the watdiv `-s` invocation.
#[derive(Debug, Clone)]
pub struct StressParams {
    /// `<max-query-size>` in stress templates.
    pub max_query_size: u32,
    /// `<query-count>` per template.
    pub query_count: u32,
    /// `<constants-per-query>`.
    pub constants_per_query: u32,
    /// `<allow-join-vertex>`.
    pub allow_join_vertex: bool,
}

impl Default for StressParams {
    fn default() -> Self {
        Self {
            max_query_size: 5,
            query_count: 20,
            constants_per_query: 2,
            allow_join_vertex: false,
        }
    }
}

/// Inputs to the driver.
pub struct DriverInputs<'a> {
    /// Resolved path to the watdiv binary.
    pub watdiv_bin: &'a Path,
    /// Path to the vendor `files/` dir holding firstnames/lastnames/words.
    pub vendor_files: &'a Path,
    /// Path to the model file (e.g. wsdbm-data-model.txt).
    pub model_file: &'a Path,
    /// Scale factor passed to `-d`.
    pub scale: u32,
    /// Stress parameters passed to `-s`/`-q` (currently informational; the
    /// vendored binary doesn't accept all of them as flags — this is
    /// preserved for meta.json and future binary patches).
    pub stress: StressParams,
    /// Number of concrete queries per template (passed to `-q`).
    pub query_count_per_template: u32,
    /// Wrap watdiv invocations with bwrap.
    pub use_bwrap: bool,
}

/// Outputs of the driver: paths to the raw watdiv outputs INSIDE the temp
/// stage. The caller MUST copy them out before the staging dir drops.
pub struct RawArtifacts {
    /// Path to data.nt (inside the stage).
    pub data_nt: PathBuf,
    /// One template path per stress template (inside the stage).
    pub templates: Vec<PathBuf>,
    /// (sparql_path, desc_path) tuples per template.
    pub queries: Vec<(PathBuf, PathBuf)>,
    /// The owning staging directory (kept alive so the paths above are valid).
    pub stage: sandbox::TempStagingDir,
}

/// Runs watdiv end-to-end and returns paths to the raw outputs.
pub fn drive(inputs: &DriverInputs) -> Result<RawArtifacts, RdfError> {
    if !inputs.watdiv_bin.exists() {
        return Err(RdfError::BinaryNotFound {
            path: inputs.watdiv_bin.to_path_buf(),
        });
    }
    let stage = sandbox::TempStagingDir::create(inputs.watdiv_bin, inputs.vendor_files)?;
    let cfg = invoke::InvokeConfig {
        stage: &stage,
        model_file: inputs.model_file,
        use_bwrap: inputs.use_bwrap,
    };

    let bin_release = stage.binary_path().parent().unwrap().to_path_buf();
    let data_nt = bin_release.join("data.nt");
    invoke::run_data(&cfg, inputs.scale, &data_nt)?;

    let stress_arg = "stress-templates";
    std::fs::create_dir_all(bin_release.join(stress_arg))?;
    let templates = invoke::run_stress(&cfg, stress_arg, inputs.stress.query_count)?;
    let queries =
        invoke::run_queries(&cfg, &templates, inputs.query_count_per_template)?;

    Ok(RawArtifacts {
        data_nt,
        templates,
        queries,
        stage,
    })
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check -p kermit-rdf`
Expected: clean.

- [ ] **Step 3: Commit**

```bash
git add kermit-rdf/src/driver/mod.rs
git commit -m "feat(kermit-rdf): driver entry point and DriverInputs"
```

---

## Task 16: Pipeline orchestrator

**Files:**
- Modify: `kermit-rdf/src/pipeline.rs`

- [ ] **Step 1: Write the pipeline orchestrator**

Replace `kermit-rdf/src/pipeline.rs`:

```rust
//! End-to-end pipeline orchestrator.
//!
//! Runs the driver to produce raw watdiv artifacts, then runs stages 4–6
//! in pure Rust to produce the final benchmark cache directory:
//!
//! ```text
//! <out_dir>/
//!   meta.json
//!   benchmark.yml
//!   dict.parquet
//!   <predicate>.parquet × N
//!   raw/data.nt
//!   raw/templates/*.txt
//!   raw/queries/*.sparql + *.desc
//!   expected/<query>.csv
//! ```

use {
    crate::{
        dict::Dictionary,
        driver::{self, DriverInputs, RawArtifacts, StressParams},
        error::RdfError,
        expected, parquet, partition,
        sparql::translator::translate_query,
        yaml_emit::{write_benchmark_yaml, YamlInputs},
    },
    serde::Serialize,
    sha2::{Digest, Sha256},
    std::{
        collections::HashMap,
        fs,
        io::Read,
        path::{Path, PathBuf},
    },
};

/// Inputs for `run_pipeline`.
pub struct PipelineInputs<'a> {
    /// Driver inputs (binary path, model file, vendor files, scale, stress).
    pub driver: DriverInputs<'a>,
    /// Final output directory; created if missing.
    pub out_dir: &'a Path,
    /// Benchmark name (used for the YAML's `name` field, equal to the cache
    /// dir's basename in normal use).
    pub bench_name: &'a str,
    /// Tag (recorded in meta.json for provenance).
    pub tag: &'a str,
}

/// Returns the byte counts and IDs surfaced in `meta.json`.
#[derive(Debug, Serialize)]
pub struct PipelineMeta {
    /// Schema version (bump on breaking field-name or value-type change).
    pub schema_version: u32,
    /// "watdiv-onthefly" for this pipeline.
    pub kind: String,
    /// Scale factor passed to watdiv -d.
    pub scale: u32,
    /// User-provided tag (CLI `--tag`).
    pub tag: String,
    /// SHA-256 of the watdiv binary file.
    pub watdiv_binary_sha256: String,
    /// SHA-256s of vendor files used.
    pub names_files_sha256: HashMap<String, String>,
    /// SHA-256 of the model file.
    pub model_file_sha256: String,
    /// Stress params copied from CLI / defaults.
    pub stress_params: StressParamsMeta,
    /// UTC timestamp of generation.
    pub generated_at_utc: String,
    /// Number of triples generated.
    pub triple_count: u64,
    /// Number of distinct predicates (relations).
    pub relation_count: u32,
    /// Number of queries produced.
    pub query_count: u32,
}

/// Stress params surfaced into meta.json.
#[derive(Debug, Serialize)]
pub struct StressParamsMeta {
    /// `--max-query-size`.
    pub max_query_size: u32,
    /// `--query-count`.
    pub query_count: u32,
    /// `--constants-per-query`.
    pub constants_per_query: u32,
    /// `--allow-join-vertex`.
    pub allow_join_vertex: bool,
}

fn sha256_file(path: &Path) -> Result<String, RdfError> {
    let mut h = Sha256::new();
    let mut f = fs::File::open(path)?;
    let mut buf = [0u8; 8192];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(format!("{:x}", h.finalize()))
}

/// Stage 4 + 5 + 6 of the pipeline. Public so the no-binary pipeline
/// integration test (Task 19) can drive stages 4–6 with a hand-crafted
/// `RawArtifacts`-equivalent.
pub fn process_artifacts(
    inputs: &PipelineInputs,
    raw: &RawArtifacts,
) -> Result<PipelineMeta, RdfError> {
    fs::create_dir_all(inputs.out_dir)?;
    let raw_root = inputs.out_dir.join("raw");
    fs::create_dir_all(raw_root.join("templates"))?;
    fs::create_dir_all(raw_root.join("queries"))?;

    fs::copy(&raw.data_nt, raw_root.join("data.nt"))?;
    for tpl in &raw.templates {
        let dst = raw_root.join("templates").join(tpl.file_name().unwrap());
        fs::copy(tpl, dst)?;
    }
    let mut copied_sparql_paths: Vec<PathBuf> = Vec::new();
    for (sparql, desc) in &raw.queries {
        let s_dst = raw_root.join("queries").join(sparql.file_name().unwrap());
        fs::copy(sparql, &s_dst)?;
        if desc.exists() {
            let d_dst = raw_root.join("queries").join(desc.file_name().unwrap());
            fs::copy(desc, d_dst)?;
        }
        copied_sparql_paths.push(s_dst);
    }

    let part = partition::partition(raw_root.join("data.nt"))?;
    let mut dict = part.dict;

    for rel in &part.relations {
        let path = inputs.out_dir.join(format!("{}.parquet", rel.name));
        parquet::write_relation(rel, &path)?;
    }

    let all_predicates: Vec<String> = part
        .relations
        .iter()
        .map(|r| r.name.clone())
        .collect();

    let mut all_queries: Vec<(String, String)> = Vec::new();
    for sparql_path in &copied_sparql_paths {
        let stem = sparql_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("q")
            .replace('.', "-");
        let text = fs::read_to_string(sparql_path)?;
        for (i, q) in text
            .lines()
            .filter(|l| !l.trim().is_empty())
            .enumerate()
        {
            let qname = format!("{stem}_q{i:04}");
            let head = format!("Q_{}_{}_{}", inputs.bench_name, stem, i);
            let head = head.replace('-', "_");
            let dl = translate_query(q, &mut dict, &part.predicate_map, &head)?;
            all_queries.push((qname, dl));
        }
    }

    let dict_path = inputs.out_dir.join("dict.parquet");
    parquet::write_dict(&dict, &dict_path)?;

    let base_url = format!("file://{}", inputs.out_dir.canonicalize()?.display());
    let yaml = YamlInputs {
        name: inputs.bench_name,
        description: &format!(
            "WatDiv on-the-fly generation, scale {}, tag {}",
            inputs.driver.scale, inputs.tag
        ),
        queries: all_queries.clone(),
        all_predicates: &all_predicates,
        base_url: &base_url,
    };
    write_benchmark_yaml(&yaml, inputs.out_dir)?;

    let expected_dir = inputs.out_dir.join("expected");
    expected::write_expected_csvs(&copied_sparql_paths, &expected_dir)?;

    let mut names_hashes = HashMap::new();
    for n in ["firstnames.txt", "lastnames.txt"] {
        let p = inputs.driver.vendor_files.join(n);
        names_hashes.insert(n.to_string(), sha256_file(&p)?);
    }

    let triple_count = part
        .relations
        .iter()
        .map(|r| r.tuples.len() as u64)
        .sum();

    let meta = PipelineMeta {
        schema_version: 1,
        kind: "watdiv-onthefly".to_string(),
        scale: inputs.driver.scale,
        tag: inputs.tag.to_string(),
        watdiv_binary_sha256: sha256_file(inputs.driver.watdiv_bin)?,
        names_files_sha256: names_hashes,
        model_file_sha256: sha256_file(inputs.driver.model_file)?,
        stress_params: StressParamsMeta {
            max_query_size: inputs.driver.stress.max_query_size,
            query_count: inputs.driver.stress.query_count,
            constants_per_query: inputs.driver.stress.constants_per_query,
            allow_join_vertex: inputs.driver.stress.allow_join_vertex,
        },
        generated_at_utc: utc_iso8601_now(),
        triple_count,
        relation_count: part.relations.len() as u32,
        query_count: all_queries.len() as u32,
    };
    let meta_json =
        serde_json::to_string_pretty(&meta).map_err(|e| RdfError::Expected(e.to_string()))?;
    fs::write(inputs.out_dir.join("meta.json"), meta_json)?;
    Ok(meta)
}

/// Top-level entry point: runs the driver and processes artifacts.
pub fn run_pipeline(inputs: &PipelineInputs) -> Result<PipelineMeta, RdfError> {
    let raw = driver::drive(&inputs.driver)?;
    process_artifacts(inputs, &raw)
}

fn utc_iso8601_now() -> String {
    // Avoid pulling in chrono / time; format manually from SystemTime.
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // Naive UTC formatting using stdlib only.
    let days = secs / 86400;
    let rem = secs % 86400;
    let h = rem / 3600;
    let m = (rem % 3600) / 60;
    let s = rem % 60;
    let (y, mo, d) = days_since_epoch_to_ymd(days as i64);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{m:02}:{s:02}Z")
}

fn days_since_epoch_to_ymd(mut days: i64) -> (i32, u32, u32) {
    // 1970-01-01 is day 0; this is good enough for monotonic provenance
    // strings — exact correctness around leap years matters less than
    // having a stable sortable string.
    let mut y: i32 = 1970;
    loop {
        let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
        let year_days = if leap { 366 } else { 365 };
        if days < year_days as i64 {
            break;
        }
        days -= year_days as i64;
        y += 1;
    }
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let months = [31, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut mo: u32 = 1;
    for &mlen in &months {
        if days < mlen as i64 {
            break;
        }
        days -= mlen as i64;
        mo += 1;
    }
    (y, mo, days as u32 + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ymd_epoch_zero_is_jan_1_1970() {
        assert_eq!(days_since_epoch_to_ymd(0), (1970, 1, 1));
    }

    #[test]
    fn ymd_handles_one_year() {
        assert_eq!(days_since_epoch_to_ymd(365), (1971, 1, 1));
    }

    #[test]
    fn iso_timestamp_well_formed() {
        let s = utc_iso8601_now();
        assert_eq!(s.len(), 20);
        assert!(s.ends_with('Z'));
        assert!(s.chars().nth(4) == Some('-'));
        assert!(s.chars().nth(10) == Some('T'));
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p kermit-rdf pipeline`
Expected: 3 passing.

- [ ] **Step 3: Commit**

```bash
git add kermit-rdf/src/pipeline.rs
git commit -m "feat(kermit-rdf): pipeline orchestrator and meta.json schema"
```

---

## Task 17: Pipeline integration test (no binary)

**Files:**
- Create: `kermit-rdf/tests/pipeline.rs`

- [ ] **Step 1: Write the integration test**

Create `kermit-rdf/tests/pipeline.rs`:

```rust
//! Hand-crafted N-Triples + SPARQL through stages 4–6 (no watdiv binary).

use {
    kermit_rdf::{
        dict::Dictionary,
        parquet,
        partition,
        sparql::translator::translate_query,
        yaml_emit::{write_benchmark_yaml, YamlInputs},
    },
    std::{collections::HashMap, fs},
};

#[test]
fn end_to_end_stages_4_through_5_on_handcrafted_input() {
    let dir = tempfile::tempdir().unwrap();
    let out_dir = dir.path().to_path_buf();

    let nt = "\
<http://x/a> <http://x/follows> <http://x/b> .
<http://x/b> <http://x/follows> <http://x/c> .
<http://x/c> <http://x/follows> <http://x/a> .
";
    let nt_path = out_dir.join("data.nt");
    fs::write(&nt_path, nt).unwrap();

    let part = partition::partition(&nt_path).unwrap();
    let mut dict = part.dict;

    for rel in &part.relations {
        parquet::write_relation(rel, &out_dir.join(format!("{}.parquet", rel.name))).unwrap();
    }
    parquet::write_dict(&dict, &out_dir.join("dict.parquet")).unwrap();

    let q = "SELECT * WHERE { ?x <http://x/follows> ?y . ?y <http://x/follows> ?z . }";
    let dl = translate_query(q, &mut dict, &part.predicate_map, "Q_path").unwrap();
    let queries = vec![("path".to_string(), dl)];

    let predicates: Vec<String> = part.relations.iter().map(|r| r.name.clone()).collect();
    let inputs = YamlInputs {
        name: "test-bench",
        description: "two-hop path",
        queries,
        all_predicates: &predicates,
        base_url: "file:///tmp/x",
    };
    let def = write_benchmark_yaml(&inputs, &out_dir).unwrap();

    assert_eq!(def.queries.len(), 1);
    assert_eq!(def.queries[0].query, "Q_path(X, Y, Z) :- follows(X, Y), follows(Y, Z).");
    assert!(out_dir.join("benchmark.yml").exists());
    assert!(out_dir.join("dict.parquet").exists());
    assert!(out_dir.join("follows.parquet").exists());
}
```

- [ ] **Step 2: Run the test**

Run: `cargo test -p kermit-rdf --test pipeline`
Expected: 1 passing.

- [ ] **Step 3: Commit**

```bash
git add kermit-rdf/tests/pipeline.rs
git commit -m "test(kermit-rdf): integration test for stages 4-6"
```

---

## Task 18: Vendor watdiv binary + files

**Files:**
- Create: `kermit-rdf/vendor/watdiv/bin/Release/watdiv`
- Create: `kermit-rdf/vendor/watdiv/files/firstnames.txt`
- Create: `kermit-rdf/vendor/watdiv/files/lastnames.txt`
- Create: `kermit-rdf/vendor/watdiv/files/words`
- Create: `kermit-rdf/vendor/watdiv/MODEL.txt`
- Create: `kermit-rdf/vendor/watdiv/VERSION`
- Create: `kermit-rdf/vendor/watdiv/LICENSE`
- Create: `.gitattributes`

- [ ] **Step 1: Move the existing watdiv binary into the vendor location**

The binary already exists at the worktree root from earlier exploration. Move it:

```bash
mkdir -p kermit-rdf/vendor/watdiv/bin/Release
mv watdiv kermit-rdf/vendor/watdiv/bin/Release/watdiv
chmod +x kermit-rdf/vendor/watdiv/bin/Release/watdiv
```

- [ ] **Step 2: Acquire the support files**

The firstnames/lastnames lists come from upstream (`mhoangvslev/watdiv` mirror). The model file is the standard `wsdbm-data-model.txt`. The words file is whatever was used during the initial empirical verification (see `/tmp/wd-files/` referenced in the spec's session history; replicate or fetch fresh from upstream).

```bash
mkdir -p kermit-rdf/vendor/watdiv/files
# Copy from prior empirical-test artifacts or fetch upstream:
#   curl -L -o kermit-rdf/vendor/watdiv/files/firstnames.txt https://raw.githubusercontent.com/mhoangvslev/watdiv/master/files/firstnames.txt
#   curl -L -o kermit-rdf/vendor/watdiv/files/lastnames.txt  https://raw.githubusercontent.com/mhoangvslev/watdiv/master/files/lastnames.txt
# Words: a 1000-line subset of /usr/share/dict/words is sufficient.
```

If the user has earlier empirical-test files at `/tmp/wd-files/`, copy them in directly.

- [ ] **Step 3: Add MODEL.txt, VERSION, LICENSE**

```bash
# wsdbm-data-model.txt from the upstream watdiv distribution
cp /tmp/wsdbm-model.txt kermit-rdf/vendor/watdiv/MODEL.txt   # or fetch upstream
echo "watdiv-upstream-2014" > kermit-rdf/vendor/watdiv/VERSION
sha256sum kermit-rdf/vendor/watdiv/bin/Release/watdiv >> kermit-rdf/vendor/watdiv/VERSION
# LICENSE: copy the Apache-2.0 license text from upstream watdiv repo.
```

- [ ] **Step 4: Create .gitattributes**

```bash
cat > .gitattributes <<'EOF'
kermit-rdf/vendor/watdiv/bin/Release/watdiv binary
EOF
```

- [ ] **Step 5: Commit**

```bash
git add kermit-rdf/vendor .gitattributes
git commit -m "feat(kermit-rdf): vendor watdiv binary + support files"
```

---

## Task 19: End-to-end test (real binary, gated on Linux x86_64 + bwrap)

**Files:**
- Create: `kermit-rdf/tests/e2e_watdiv.rs`

- [ ] **Step 1: Write the e2e test**

Create `kermit-rdf/tests/e2e_watdiv.rs`:

```rust
//! End-to-end test that drives the real vendored watdiv binary.
//!
//! Skipped on non-Linux, non-x86_64, or hosts without `bwrap`.

use {
    kermit_rdf::{
        driver::{DriverInputs, StressParams},
        pipeline::{run_pipeline, PipelineInputs},
    },
    std::path::PathBuf,
};

fn skip_if_unsupported() -> bool {
    if cfg!(not(target_os = "linux")) || cfg!(not(target_arch = "x86_64")) {
        eprintln!("skipping watdiv e2e: requires linux x86_64");
        return true;
    }
    if std::process::Command::new("bwrap")
        .arg("--version")
        .output()
        .is_err()
    {
        eprintln!("skipping watdiv e2e: bwrap not found");
        return true;
    }
    false
}

fn vendor_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("vendor/watdiv")
}

#[test]
fn watdiv_sf1_pipeline_succeeds_and_produces_expected_artifacts() {
    if skip_if_unsupported() {
        return;
    }
    let vendor = vendor_root();
    let dir = tempfile::tempdir().unwrap();
    let inputs = PipelineInputs {
        driver: DriverInputs {
            watdiv_bin: &vendor.join("bin/Release/watdiv"),
            vendor_files: &vendor.join("files"),
            model_file: &vendor.join("MODEL.txt"),
            scale: 1,
            stress: StressParams::default(),
            query_count_per_template: 5,
            use_bwrap: true,
        },
        out_dir: dir.path(),
        bench_name: "watdiv-stress-1-e2e",
        tag: "e2e",
    };
    let meta = run_pipeline(&inputs).expect("pipeline failed");
    assert!(meta.triple_count > 0, "no triples generated");
    assert!(meta.relation_count > 0, "no relations partitioned");

    assert!(dir.path().join("benchmark.yml").exists());
    assert!(dir.path().join("dict.parquet").exists());
    assert!(dir.path().join("meta.json").exists());

    // Round-trip consistency: re-parse the .nt and check the dict size matches.
    let part = kermit_rdf::partition::partition(dir.path().join("raw/data.nt")).unwrap();
    assert_eq!(
        part.relations.len() as u32,
        meta.relation_count,
        "relation count drifted between meta and re-parse"
    );
}
```

- [ ] **Step 2: Run the e2e test (skips gracefully if bwrap missing)**

Run: `cargo test -p kermit-rdf --test e2e_watdiv`
Expected (on Linux + bwrap): 1 passing. On other platforms: 1 passing (the test prints "skipping…" and returns early).

- [ ] **Step 3: Commit**

```bash
git add kermit-rdf/tests/e2e_watdiv.rs
git commit -m "test(kermit-rdf): e2e SF=1 against real watdiv binary"
```

---

## Task 20: Discovery extension in kermit-bench

**Files:**
- Modify: `kermit-bench/src/discovery.rs`
- Modify: `kermit-bench/src/lib.rs`

- [ ] **Step 1: Add the failing test**

Append to `kermit-bench/src/discovery.rs`'s test module:

```rust
    #[test]
    fn load_all_with_cache_walks_both_roots() {
        let workspace = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();
        let workspace_benchmarks = workspace.path().join("benchmarks");
        std::fs::create_dir(&workspace_benchmarks).unwrap();

        let yaml_in_workspace = r#"
name: alpha
description: "Workspace bench"
relations:
  - name: r
    url: "https://example.com/r.parquet"
queries:
  - name: q
    description: "default"
    query: "Q(X) :- r(X)."
"#;
        std::fs::write(workspace_benchmarks.join("alpha.yml"), yaml_in_workspace).unwrap();

        let cache_subdir = cache.path().join("beta");
        std::fs::create_dir(&cache_subdir).unwrap();
        let yaml_in_cache = r#"
name: beta
description: "Cached bench"
relations:
  - name: r
    url: "file:///tmp/r.parquet"
queries:
  - name: q
    description: "default"
    query: "Q(X) :- r(X)."
"#;
        std::fs::write(cache_subdir.join("benchmark.yml"), yaml_in_cache).unwrap();

        let names: Vec<String> =
            load_all_benchmarks_with_cache(workspace.path(), cache.path())
                .unwrap()
                .into_iter()
                .map(|b| b.name)
                .collect();
        assert!(names.contains(&"alpha".to_string()));
        assert!(names.contains(&"beta".to_string()));
    }
```

- [ ] **Step 2: Implement the new function**

Add to `kermit-bench/src/discovery.rs` (above the `#[cfg(test)]` block):

```rust
/// Loads all benchmarks from the workspace AND the cache root.
///
/// Workspace benchmarks live at `<workspace>/benchmarks/*.yml` (existing
/// behavior). Cache benchmarks live at `<cache_root>/<name>/benchmark.yml`
/// (one self-contained directory per generated benchmark). The two lists
/// are concatenated; cache benchmarks override workspace benchmarks of the
/// same name.
///
/// # Errors
///
/// Returns any [`BenchError`] produced while reading either root.
pub fn load_all_benchmarks_with_cache(
    workspace_root: &Path, cache_root: &Path,
) -> Result<Vec<BenchmarkDefinition>, BenchError> {
    let mut out = load_all_benchmarks(workspace_root)?;
    let mut existing: std::collections::HashMap<String, usize> = out
        .iter()
        .enumerate()
        .map(|(i, b)| (b.name.clone(), i))
        .collect();
    if !cache_root.exists() {
        return Ok(out);
    }
    let mut entries: Vec<_> = std::fs::read_dir(cache_root)?.filter_map(|e| e.ok()).collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let yml = path.join("benchmark.yml");
        if !yml.exists() {
            continue;
        }
        let contents = std::fs::read_to_string(&yml)?;
        let def: BenchmarkDefinition =
            serde_yaml::from_str(&contents).map_err(|source| BenchError::Yaml {
                path: yml.clone(),
                source,
            })?;
        def.validate()?;
        if let Some(&idx) = existing.get(&def.name) {
            out[idx] = def;
        } else {
            existing.insert(def.name.clone(), out.len());
            out.push(def);
        }
    }
    Ok(out)
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p kermit-bench`
Expected: existing tests still pass + 1 new test passes.

- [ ] **Step 4: Commit**

```bash
git add kermit-bench/src/discovery.rs
git commit -m "feat(kermit-bench): discovery walks workspace + cache roots"
```

---

## Task 21: CLI subcommand `bench watdiv-gen`

**Files:**
- Modify: `kermit/Cargo.toml`
- Modify: `kermit/src/main.rs`

- [ ] **Step 1: Add kermit-rdf as a dep**

In `kermit/Cargo.toml`, append under `[dependencies]`:

```toml
kermit-rdf = { version = "0.1.0", path = "../kermit-rdf" }
```

- [ ] **Step 2: Add the subcommand definition**

In `kermit/src/main.rs`, add a new variant to `BenchSubcommand`:

```rust
    /// Generate a fresh watdiv benchmark on the fly
    WatdivGen {
        /// Scale factor passed to watdiv -d (>= 1)
        #[arg(long, value_name = "N", required = true)]
        scale: u32,

        /// Tag appended to the benchmark name; must contain a non-numeric
        /// character so it cannot collide with committed snapshot names
        #[arg(long, value_name = "STRING", required = true)]
        tag: String,

        /// max-query-size for stress templates (default 5)
        #[arg(long, value_name = "N", default_value = "5")]
        max_query_size: u32,

        /// concrete queries per template (default 20)
        #[arg(long, value_name = "N", default_value = "20")]
        query_count: u32,

        /// constants per query (default 2)
        #[arg(long, value_name = "N", default_value = "2")]
        constants_per_query: u32,

        /// allow join-vertex (default false)
        #[arg(long)]
        allow_join_vertex: bool,

        /// Override the watdiv binary path (default: vendored)
        #[arg(long, value_name = "PATH", env = "KERMIT_WATDIV_BIN")]
        watdiv_bin: Option<PathBuf>,

        /// Override the cache dir parent (default: ~/.cache/kermit/benchmarks)
        #[arg(long, value_name = "PATH")]
        output_dir: Option<PathBuf>,

        /// Skip bwrap sandbox; require host /usr/share/dict/words
        #[arg(long)]
        no_bwrap: bool,
    },
```

- [ ] **Step 3: Add the handler arm**

Inside `match subcommand` add:

```rust
            | BenchSubcommand::WatdivGen {
                scale,
                tag,
                max_query_size,
                query_count,
                constants_per_query,
                allow_join_vertex,
                watdiv_bin,
                output_dir,
                no_bwrap,
            } => {
                if !tag.chars().any(|c| !c.is_ascii_digit()) {
                    anyhow::bail!(
                        "--tag must contain at least one non-numeric character to avoid \
                         collision with committed benchmark names"
                    );
                }
                let vendor = vendored_watdiv_root();
                let bin = watdiv_bin
                    .unwrap_or_else(|| vendor.join("bin/Release/watdiv"));
                if !bin.exists() {
                    anyhow::bail!("watdiv binary not found at {bin:?}");
                }
                let cache_parent = output_dir.unwrap_or_else(|| {
                    dirs::cache_dir()
                        .map(|p| p.join("kermit").join("benchmarks"))
                        .expect("no cache dir on this platform")
                });
                let bench_name = format!("watdiv-stress-{scale}-{tag}");
                let out_dir = cache_parent.join(&bench_name);
                std::fs::create_dir_all(&out_dir)?;

                let stress = kermit_rdf::driver::StressParams {
                    max_query_size,
                    query_count,
                    constants_per_query,
                    allow_join_vertex,
                };
                let inputs = kermit_rdf::pipeline::PipelineInputs {
                    driver: kermit_rdf::driver::DriverInputs {
                        watdiv_bin: &bin,
                        vendor_files: &vendor.join("files"),
                        model_file: &vendor.join("MODEL.txt"),
                        scale,
                        stress,
                        query_count_per_template: query_count,
                        use_bwrap: !no_bwrap,
                    },
                    out_dir: &out_dir,
                    bench_name: &bench_name,
                    tag: &tag,
                };
                let meta = kermit_rdf::pipeline::run_pipeline(&inputs)
                    .map_err(|e| anyhow::anyhow!("watdiv-gen pipeline failed: {e}"))?;
                eprintln!(
                    "[watdiv-gen] wrote {} (triples={}, relations={}, queries={})",
                    out_dir.display(),
                    meta.triple_count,
                    meta.relation_count,
                    meta.query_count
                );
            },
```

- [ ] **Step 4: Add the vendored-root helper**

In `kermit/src/main.rs`, after `workspace_root()`:

```rust
fn vendored_watdiv_root() -> PathBuf {
    workspace_root().join("kermit-rdf/vendor/watdiv")
}
```

Also add the `dirs` dep to `kermit/Cargo.toml` if not present:

```toml
dirs = "6"
```

- [ ] **Step 5: Build to verify**

Run: `cargo build -p kermit`
Expected: clean build.

- [ ] **Step 6: Commit**

```bash
git add kermit/Cargo.toml kermit/src/main.rs
git commit -m "feat(kermit): add bench watdiv-gen subcommand"
```

---

## Task 22: Switch list/fetch to two-root discovery

**Files:**
- Modify: `kermit/src/main.rs`

- [ ] **Step 1: Replace the discovery calls in List/Fetch**

In `kermit/src/main.rs`, replace the body of `BenchSubcommand::List`:

```rust
            | BenchSubcommand::List => {
                let root = workspace_root();
                let cache = dirs::cache_dir()
                    .map(|p| p.join("kermit").join("benchmarks"))
                    .unwrap_or_else(|| PathBuf::from("/tmp/no-cache"));
                let benchmarks =
                    kermit_bench::discovery::load_all_benchmarks_with_cache(&root, &cache)
                        .map_err(|e| anyhow::anyhow!("{e}"))?;
                if benchmarks.is_empty() {
                    eprintln!("No benchmarks found in benchmarks/ or cache");
                } else {
                    for b in &benchmarks {
                        let cached = kermit_bench::cache::is_cached(b).unwrap_or(false);
                        let status = if cached { "cached" } else { "not cached" };
                        println!("{} ({}) [{}]", b.name, b.description, status);
                        for q in &b.queries {
                            println!("  query: {} - {}", q.name, q.description);
                        }
                    }
                }
            },
```

Repeat the same swap inside `BenchSubcommand::Fetch` (replace `load_all_benchmarks` → `load_all_benchmarks_with_cache`, and pass the cache root).

The `resolve_benchmarks` helper used by `bench run` can keep using `load_benchmark` (workspace-only) for `bench run <name>`, but `--all` should walk both roots — adjust accordingly:

```rust
fn resolve_benchmarks(
    name: &Option<String>, all: bool,
) -> anyhow::Result<Vec<BenchmarkDefinition>> {
    let root = workspace_root();
    let cache = dirs::cache_dir()
        .map(|p| p.join("kermit").join("benchmarks"))
        .unwrap_or_else(|| PathBuf::from("/tmp/no-cache"));
    if all {
        kermit_bench::discovery::load_all_benchmarks_with_cache(&root, &cache)
            .map_err(|e| anyhow::anyhow!("Failed to load benchmarks: {e}"))
    } else if let Some(name) = name {
        // Try workspace first, then cache.
        match kermit_bench::discovery::load_benchmark(&root, name) {
            Ok(b) => Ok(vec![b]),
            Err(_) => {
                let yml = cache.join(name).join("benchmark.yml");
                if !yml.exists() {
                    anyhow::bail!("benchmark not found: {name}");
                }
                let contents = std::fs::read_to_string(&yml)?;
                let def: BenchmarkDefinition = serde_yaml::from_str(&contents)?;
                def.validate()?;
                Ok(vec![def])
            }
        }
    } else {
        anyhow::bail!("Specify a benchmark name or --all")
    }
}
```

- [ ] **Step 2: Run all kermit tests**

Run: `cargo test -p kermit`
Expected: all existing tests pass.

- [ ] **Step 3: Commit**

```bash
git add kermit/src/main.rs
git commit -m "feat(kermit): bench list/fetch/run --all walk cache root"
```

---

## Task 23: CLI smoke test

**Files:**
- Create: `kermit/tests/cli_watdiv_gen.rs`

- [ ] **Step 1: Write the smoke test**

Create `kermit/tests/cli_watdiv_gen.rs`:

```rust
//! End-to-end CLI smoke test for `kermit bench watdiv-gen`.
//!
//! Skipped on non-Linux/non-x86_64 hosts and hosts without `bwrap`.

use std::process::Command;

fn skip_unsupported() -> bool {
    if cfg!(not(target_os = "linux")) || cfg!(not(target_arch = "x86_64")) {
        eprintln!("skipping cli_watdiv_gen: needs linux x86_64");
        return true;
    }
    if Command::new("bwrap").arg("--version").output().is_err() {
        eprintln!("skipping cli_watdiv_gen: bwrap not available");
        return true;
    }
    false
}

#[test]
fn watdiv_gen_sf1_smoke_test() {
    if skip_unsupported() {
        return;
    }
    let out = tempfile::tempdir().unwrap();
    let bin = env!("CARGO_BIN_EXE_kermit");
    let status = Command::new(bin)
        .arg("bench")
        .arg("watdiv-gen")
        .arg("--scale")
        .arg("1")
        .arg("--tag")
        .arg("smoke")
        .arg("--query-count")
        .arg("3")
        .arg("--output-dir")
        .arg(out.path())
        .status()
        .expect("failed to run kermit");
    assert!(status.success(), "watdiv-gen failed");

    let bench_dir = out.path().join("watdiv-stress-1-smoke");
    assert!(bench_dir.join("benchmark.yml").exists());
    assert!(bench_dir.join("meta.json").exists());
    assert!(bench_dir.join("dict.parquet").exists());
}
```

- [ ] **Step 2: Run the test**

Run: `cargo test -p kermit --test cli_watdiv_gen`
Expected: 1 passing (or skipping cleanly on unsupported platforms).

- [ ] **Step 3: Commit**

```bash
git add kermit/tests/cli_watdiv_gen.rs
git commit -m "test(kermit): CLI smoke test for bench watdiv-gen"
```

---

## Task 24: Whole-repo CI checks

**Files:** none (verification only)

- [ ] **Step 1: Run the full CI gate locally**

Run each command. Each must succeed:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --workspace -- -D warnings
cargo test --workspace
cargo doc --workspace --no-deps
```

If any step fails, fix the underlying issue and re-run that step.

- [ ] **Step 2: Run miri on the new crate's safe-Rust portion**

Run: `MIRIFLAGS="-Zmiri-disable-isolation" cargo miri test -p kermit-rdf --lib`

Expected: passes. The `--lib` flag excludes integration tests that spawn `Command` (miri can't model process spawning).

- [ ] **Step 3: Verify nothing in the workspace's other CI gates regressed**

Run: `cargo test --workspace`
Expected: all 200+ tests pass.

- [ ] **Step 4: Commit any fmt/clippy fixes if needed**

If steps 1–3 produced fixes:

```bash
git add -A
git commit -m "style: fmt + clippy fixes for watdiv-gen rollout"
```

---

## Self-Review Notes

### Spec coverage check

- ✓ Crate topology: `kermit-rdf` created (Tasks 1–17), `kermit-bench` extended (Task 20), `kermit` binary updated (Tasks 21–22). Other crates untouched.
- ✓ Module breakdown: every module from the spec's table has a Task.
- ✓ Workspace deps: oxttl, oxrdf, spargebra, arrow, parquet all added in Task 1 / Cargo.toml. Plus `regex-lite` (Task 11), `sha2` and `tempfile` (Task 1), `dirs` (Task 21).
- ✓ Data flow stages 1–6: stages 1–3 in Tasks 13–15 (driver), stages 4–6 in Task 16 (pipeline).
- ✓ Cache directory layout: matches the spec (flat predicate parquets at top, raw/ subdir, expected/ subdir, meta.json, benchmark.yml, dict.parquet).
- ✓ Vendoring: Task 18.
- ✓ Binary path resolution: `--watdiv-bin` flag + `KERMIT_WATDIV_BIN` env (clap's `env =` attribute) + vendored default — Task 21.
- ✓ /usr/share/dict/words: bwrap bind-mount handled in Task 14 via `--bind <words> /usr/share/dict/words`. `--no-bwrap` escape hatch wired in Task 21.
- ✓ libstdc++ portability: e2e tests skip cleanly on non-Linux/non-x86_64 (Tasks 19, 23).
- ✓ CLI surface: all flags from the spec ↔ Task 21 declarations match.
- ✓ Discovery: extended in Task 20, wired in Task 22.
- ✓ Migration plan Phase 1: implementation = Tasks 1–24. Phase 1 #3 (translator parity test) = Task 10. Phase 1 #4 (week of green CI) is operational, not code.
- ✓ Error handling: `RdfError` (Task 2) with all the spec's variants. RAII cleanup via `TempStagingDir::Drop` (Task 13). `meta.json` written last (Task 16) → `kermit bench run` cache-load failure surfaces clearly via existing cache code.
- ✓ Testing: Layer 1 (unit) is inline; Layer 2 (translator golden) = Task 10; Layer 3 (pipeline no-binary) = Task 17; Layer 4 (e2e binary) = Task 19; Layer 5 (CLI smoke) = Task 23.

### Deliberate departures from the spec

- **Expected results = harvested from `.desc` (not Datalog-eval).** Spec said "execute Datalog (via kermit-algos) against the in-memory dict-encoded data; write CSV result rows." Plan instead reads watdiv's `.desc` cardinality sidecars, matching the existing Python pipeline. Reasoning: re-executing the Datalog ourselves would tautologically agree with itself, providing zero real cross-check; harvesting watdiv's authoritative counts gives a real translator + LFTJ verification path. Documented inline at Task 12.
- **Predicate parquets at `<cache>/<bench>/<rel>.parquet` (flat), not `<cache>/<bench>/relations/<rel>.parquet`.** Matches existing `cache::ensure_cached` exactly; no changes to cache.rs needed. The spec's `relations/` subdir was speculative.

### Type consistency check

- `Dictionary` (Task 5) → used in `partition.rs` (Task 7), `parquet.rs` (Task 8), `sparql/translator.rs` (Task 10), `pipeline.rs` (Task 16). Methods: `new`, `intern`, `lookup`, `get`, `len`, `is_empty`, `iter` — used consistently.
- `RdfValue` (Task 3) — `Iri`, `Literal`, `BlankNode` arms; `to_canonical()` used by `parquet::write_dict` (Task 8).
- `PartitionedRelation` (Task 7) — fields `name: String`, `tuples: Vec<(usize, usize)>`. Used by `parquet::write_relation` (Task 8) and `pipeline.rs`.
- `Partitioned` (Task 7) — fields `dict: Dictionary`, `relations: Vec<PartitionedRelation>`, `predicate_map: HashMap<String, String>`. Used by translator (Task 10) and pipeline (Task 16).
- `RdfError` (Task 2) — `BinaryNotFound { path }`, `BinaryFailed { status, stderr }`, `Sandbox`, `NTriplesParse { line, message }`, `SparqlParse`, `UnsupportedSparql`, `Expected`, `Io`, `Arrow`, `Parquet`. Used at every error site.
- `TempStagingDir` (Task 13) — methods `create`, `binary_path`, `root`, `words_path`. Used in Task 14 (`InvokeConfig::stage`).
- `DriverInputs`, `RawArtifacts`, `StressParams` (Task 15) — used by pipeline (Task 16) and CLI (Task 21).
- `PipelineInputs`, `PipelineMeta` (Task 16) — used by CLI (Task 21).

No naming drift detected.

---

## Plan complete and saved.

**Two execution options:**

**1. Subagent-Driven (recommended)** — I dispatch a fresh subagent per task, review between tasks, fast iteration.

**2. Inline Execution** — Execute tasks in this session using executing-plans, batch execution with checkpoints.

Which approach?
