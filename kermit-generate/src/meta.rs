//! The `meta.json` every generator writes.

use {
    crate::sink::RelationRecord,
    kermit_rdf::generator::GeneratorMeta,
    serde::{de::DeserializeOwned, Deserialize, Serialize},
};

/// A generator's `meta.json`: the fields every kind shares, then the
/// generator's own `Extras` flattened beside them.
///
/// `kermit_rdf::generator::MetaHeader` reads the shared fields back and
/// ignores the rest, so spec-drift detection, the outdated-cache floors and
/// `bench list` treat every kind alike. An `Extras` type must not reuse the
/// name of a shared field, because flattening would collide; `run` checks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GeneratedMeta<E> {
    /// Always `kermit_rdf::generator::META_SCHEMA_VERSION`.
    pub schema_version: u32,
    /// The generator's `KIND`.
    pub kind: String,
    /// The user-provided tag.
    pub tag: String,
    /// UTC timestamp of generation.
    pub generated_at_utc: String,
    /// The number of relations written.
    pub relation_count: u32,
    /// The number of queries in `benchmark.yml`.
    pub query_count: u32,
    /// The declarative spec's hash; `None` for an imperative run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spec_hash: Option<String>,
    /// Every relation written, in write order.
    pub relations: Vec<RelationRecord>,
    /// The generator's own fields.
    #[serde(flatten)]
    pub extras: E,
}

impl<E: Serialize + DeserializeOwned> GeneratorMeta for GeneratedMeta<E> {
    fn schema_version(&self) -> u32 { self.schema_version }

    fn kind(&self) -> &str { &self.kind }

    fn tag(&self) -> &str { &self.tag }

    fn generated_at_utc(&self) -> &str { &self.generated_at_utc }

    fn relation_count(&self) -> u32 { self.relation_count }

    fn query_count(&self) -> u32 { self.query_count }

    fn spec_hash(&self) -> Option<&str> { self.spec_hash.as_deref() }
}

/// The keys `GeneratedMeta` writes itself; `Extras` may use none of them.
pub(crate) const COMMON_FIELDS: [&str; 8] = [
    "schema_version",
    "kind",
    "tag",
    "generated_at_utc",
    "relation_count",
    "query_count",
    "spec_hash",
    "relations",
];

#[cfg(test)]
mod tests {
    use {super::*, std::collections::BTreeSet};

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Empty {}

    fn meta(spec_hash: Option<&str>) -> GeneratedMeta<Empty> {
        GeneratedMeta {
            schema_version: 4,
            kind: "toy".to_string(),
            tag: "t1".to_string(),
            generated_at_utc: "1970-01-01T00:00:00Z".to_string(),
            relation_count: 2,
            query_count: 1,
            spec_hash: spec_hash.map(str::to_string),
            relations: vec![],
            extras: Empty {},
        }
    }

    #[test]
    fn common_fields_match_serialized_keys() {
        let value = serde_json::to_value(meta(Some("h"))).unwrap();
        let keys: BTreeSet<&str> = value
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        let listed: BTreeSet<&str> = COMMON_FIELDS.into_iter().collect();
        assert_eq!(keys, listed);
        assert_eq!(
            listed.len(),
            COMMON_FIELDS.len(),
            "COMMON_FIELDS repeats a key"
        );
    }

    #[test]
    fn accessors_return_the_common_fields() {
        let with = meta(Some("h"));
        assert_eq!(with.schema_version(), 4);
        assert_eq!(with.kind(), "toy");
        assert_eq!(with.tag(), "t1");
        assert_eq!(with.generated_at_utc(), "1970-01-01T00:00:00Z");
        assert_eq!(with.relation_count(), 2);
        assert_eq!(with.query_count(), 1);
        assert_eq!(with.spec_hash(), Some("h"));
        assert_eq!(meta(None).spec_hash(), None);
    }
}
