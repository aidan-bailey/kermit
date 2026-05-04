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
