//! Predicate name sanitization and per-predicate partitioning.

use {
    crate::{dict::Dictionary, error::RdfError, ntriples, value::RdfValue},
    std::{collections::HashMap, path::Path},
};

/// Tuples for one predicate, plus its canonical Datalog identifier.
#[derive(Debug)]
pub struct PartitionedRelation {
    /// Datalog-safe lowercase identifier (collisions resolved with `_<id>`
    /// suffix).
    pub name: String,
    /// `(s_id, o_id)` pairs.
    pub tuples: Vec<(usize, usize)>,
}

/// Result of streaming an N-Triples file into a dictionary + per-predicate
/// buckets.
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
        relations.push(PartitionedRelation {
            name,
            tuples,
        });
    }

    Ok(Partitioned {
        dict,
        relations,
        predicate_map,
    })
}

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
        | (Some(h), _) => &core[h + 1..],
        | (None, Some(s)) => &core[s + 1..],
        | (None, None) => core,
    };
    let cleaned: String = last_segment
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c
            } else {
                '_'
            }
        })
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

#[cfg(test)]
mod partition_tests {
    use {super::*, std::io::Write};

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
