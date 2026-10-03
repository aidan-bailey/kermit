//! Predicate name sanitization and per-predicate partitioning.

use {
    crate::{dict::Dictionary, error::RdfError, ntriples, value::RdfValue},
    std::{
        collections::{HashMap, HashSet},
        path::Path,
    },
};

/// Tuples for one predicate, plus its canonical Datalog identifier.
#[derive(Debug)]
pub struct PartitionedRelation {
    /// Datalog-safe lowercase identifier, unique among the relations of one
    /// partition (collisions resolved by [`unique_relation_name`]).
    pub name: String,
    /// `(s_id, o_id)` pairs, in the order their triples appear in the input.
    /// The order is a contract: relations loaded from the Parquet files are
    /// built in this order and build time depends on it (issue #66), so
    /// reordering here would shift benchmark measurements.
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
/// Each relation's tuples keep the order of their triples in the input file
/// (see [`PartitionedRelation::tuples`]): relation build time is sensitive to
/// insertion order (issue #66), so the partitioner must not reorder them.
///
/// Collisions between sanitized predicate names are resolved by
/// [`unique_relation_name`]: the first occurrence keeps the bare name, a
/// later one gets `_<dict-id>`, or a further `_<k>` if that is taken too.
/// The chosen name is recorded in `predicate_map` keyed by the predicate IRI
/// (without angle brackets).
pub fn partition<P: AsRef<Path>>(nt_path: P) -> Result<Partitioned, RdfError> {
    let mut dict = Dictionary::new();
    let mut buckets: HashMap<String, Vec<(usize, usize)>> = HashMap::new();
    let mut insertion_order: Vec<String> = Vec::new();

    for triple in ntriples::iter_path(nt_path)? {
        let (s_iri, p_iri, o) = triple?;
        let s_id = dict.intern(RdfValue::Iri(s_iri));
        // Intern the predicate here for its dictionary side effect; the ID is
        // retrieved again during naming.
        dict.intern(RdfValue::Iri(p_iri.clone()));
        let o_id = dict.intern(o);
        if !buckets.contains_key(&p_iri) {
            insertion_order.push(p_iri.clone());
        }
        buckets.entry(p_iri).or_default().push((s_id, o_id));
    }

    let mut used_names: HashSet<String> = HashSet::new();
    let mut predicate_map: HashMap<String, String> = HashMap::new();
    let mut relations: Vec<PartitionedRelation> = Vec::new();

    for p_iri in insertion_order {
        let base = sanitize_predicate(&p_iri);
        let pred_id = dict
            .lookup(&RdfValue::Iri(p_iri.clone()))
            .expect("predicate just interned");
        let name = unique_relation_name(&base, pred_id, &used_names);
        used_names.insert(name.clone());
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

/// The relation name for a predicate whose sanitized name is `base` and whose
/// dictionary id is `pred_id`, given the names already taken: `base` if it
/// is free, else `base_<pred_id>`, else `base_<pred_id>_<k>` for the
/// smallest `k >= 2` that is free.
///
/// Total, so two predicates never share a name and one Parquet file can
/// never overwrite another's: `base_<pred_id>` alone is not enough, because
/// an earlier predicate's own sanitized name can be exactly that (#76).
/// Shared by [`partition`] and the WatDiv pipeline's seeding of predicates
/// the data lacks, so both follow one rule; a `Generator::seed_relations`
/// hook that adds relations should name them with it too. Existing names never
/// change: the third step only applies where the old rule produced a duplicate.
pub fn unique_relation_name(base: &str, pred_id: usize, used: &HashSet<String>) -> String {
    if !used.contains(base) {
        return base.to_string();
    }
    let suffixed = format!("{base}_{pred_id}");
    if !used.contains(&suffixed) {
        return suffixed;
    }
    (2..)
        .map(|k| format!("{suffixed}_{k}"))
        .find(|name| !used.contains(name))
        .expect("finitely many names are taken")
}

/// Converts a predicate IRI into a Datalog-safe lowercase identifier.
///
/// Strips angle brackets if present, prefers the fragment (after `#`) or
/// last path segment (after `/`), then replaces non-alphanumeric characters
/// with underscores. Falls back to a `p_` prefix if the result would start
/// with a digit. Two distinct IRIs may sanitize to the same name; collision
/// resolution happens at the partition level (see [`partition`]).
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

    /// Naming is total: the bare name, then the dictionary-id suffix, then
    /// a counter on top of it until the name is free (#76).
    #[test]
    fn unique_relation_name_never_reuses_a_taken_name() {
        let mut used = HashSet::new();
        for want in ["title", "title_6", "title_6_2", "title_6_3"] {
            let name = unique_relation_name("title", 6, &used);
            assert_eq!(name, want);
            used.insert(name);
        }
    }

    /// The #76 reproduction: an earlier predicate's own name is literally
    /// `title_6`, which is what the later colliding `title` predicate (id
    /// 6) would have been suffixed to. Each must keep its own relation.
    #[test]
    fn a_suffixed_name_that_is_already_taken_is_not_reused() {
        let f = write_temp(
            "<http://x/a> <http://x/title_6> <http://x/b> .\n\
             <http://x/a> <http://ogp.me/ns#title> <http://x/c> .\n\
             <http://x/d> <http://purl.org/stuff/rev#title> <http://x/e> .\n",
        );
        let p = partition(f.path()).unwrap();
        assert_eq!(p.predicate_map["http://x/title_6"], "title_6");
        assert_eq!(p.predicate_map["http://ogp.me/ns#title"], "title");
        assert_eq!(
            p.predicate_map["http://purl.org/stuff/rev#title"],
            "title_6_2"
        );
        let names: HashSet<&str> = p.relations.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names.len(), 3);
    }

    #[test]
    fn dictionary_includes_all_terms() {
        let f = write_temp("<http://x/a> <http://x/p> \"lit\" .\n");
        let p = partition(f.path()).unwrap();
        assert_eq!(p.dict.len(), 3);
    }
}
