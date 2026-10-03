//! Variable-name bookkeeping during SPARQL → Datalog translation.

/// Tracks variables in their order of first appearance in a BGP.
///
/// **Load-bearing invariant.** This first-appearance order is what the
/// emitted Datalog rule's head uses (e.g. `Q(X, Y, Z) :- …` where `X, Y,
/// Z` are the first three distinct variables seen). Two BGPs that mention
/// the same variables in different orders therefore produce different
/// head-argument orders, so the order is stable for a given input but is
/// **not** semantically meaningful — callers that need a specific
/// projection must pass `projected_vars` to the translator instead of
/// relying on first-appearance order.
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
    pub fn contains(&self, name: &str) -> bool { self.seen.contains(name) }

    /// Returns the variables in order of first appearance.
    pub fn order(&self) -> &[String] { &self.order }
}

/// Maps a SPARQL variable name (with optional `?`/`$` prefix) to the
/// Datalog variable `V_<escaped name>`.
///
/// SPARQL variable names are case-sensitive and may begin with `_` or a
/// digit or contain non-ASCII characters, while a `kermit-parser` variable
/// is an ASCII uppercase letter followed by ASCII letters, digits and `_`.
/// Uppercasing the name, as this function used to, merged `?x` and `?X`
/// into one join variable and rejected `?_x` at parse time (#75). The
/// fixed `V_` prefix makes every result a parser variable, and the escape
/// keeps the mapping injective: ASCII letters and digits pass through, `_`
/// becomes `__`, and any other character becomes `_x<hex code point>_`.
/// Read left to right, an escaped `_` is always followed by `_` (a literal
/// underscore) or `x` (a code point running to the next `_`), so distinct
/// names never map to one variable.
pub fn var_name(raw: &str) -> String {
    let name = raw.trim_start_matches('?').trim_start_matches('$');
    let mut out = String::with_capacity(name.len() + 2);
    out.push_str("V_");
    for c in name.chars() {
        match c {
            | c if c.is_ascii_alphanumeric() => out.push(c),
            | '_' => out.push_str("__"),
            | c => out.push_str(&format!("_x{:x}_", u32::from(c))),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn var_name_strips_question_mark() {
        assert_eq!(var_name("?x"), "V_x");
    }

    #[test]
    fn var_name_strips_dollar_sign() {
        assert_eq!(var_name("$y"), "V_y");
    }

    #[test]
    fn var_name_keeps_letters_and_digits_as_written() {
        assert_eq!(var_name("?ABC"), "V_ABC");
        assert_eq!(var_name("?v0"), "V_v0");
        assert_eq!(var_name("?1a"), "V_1a");
    }

    /// SPARQL names are case-sensitive; uppercasing merged `?x` and `?X`
    /// into one Datalog variable (#75).
    #[test]
    fn case_distinct_names_stay_distinct() {
        assert_eq!(var_name("?x"), "V_x");
        assert_eq!(var_name("?X"), "V_X");
    }

    #[test]
    fn underscores_and_other_characters_are_escaped() {
        assert_eq!(var_name("?_x"), "V___x");
        assert_eq!(var_name("?a_b"), "V_a__b");
        assert_eq!(var_name("?straße"), "V_stra_xdf_e");
    }

    /// Names that collided under uppercasing, or that an escape scheme
    /// could confuse, all map apart.
    #[test]
    fn mapping_is_injective() {
        let names = [
            "x", "X", "a_b", "a__b", "ab", "aß", "a_xdf_", "a_xdf", "straße", "strasse", "STRASSE",
            "_x", "__x", "x_", "1a", "v0", "V0",
        ];
        let mapped: std::collections::HashSet<String> = names.iter().map(|n| var_name(n)).collect();
        assert_eq!(mapped.len(), names.len(), "{mapped:?}");
    }

    /// Every mapped name is a kermit-parser variable: an ASCII uppercase
    /// letter, then ASCII letters, digits or `_`. (This crate cannot depend
    /// on the parser; `kermit/tests/translator_round_trip.rs` parses the
    /// emitted rules.)
    #[test]
    fn every_mapped_name_is_a_datalog_variable() {
        for name in ["x", "_x", "1a", "straße", "x·y", "é", "a‿b"] {
            let mapped = var_name(name);
            let mut chars = mapped.chars();
            assert!(
                chars.next().is_some_and(|c| c.is_ascii_uppercase()),
                "{mapped}"
            );
            assert!(
                chars.all(|c| c.is_ascii_alphanumeric() || c == '_'),
                "{mapped}"
            );
        }
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
