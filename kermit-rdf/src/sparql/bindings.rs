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

/// Normalises a SPARQL variable name (with optional `?`/`$` prefix) to a
/// Datalog-safe uppercase token.
pub fn var_name(raw: &str) -> String {
    raw.trim_start_matches('?')
        .trim_start_matches('$')
        .to_uppercase()
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
