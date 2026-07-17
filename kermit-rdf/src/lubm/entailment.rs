//! Univ-Bench TBox forward chaining.
//!
//! Reads an N-Triples file emitted by `lubm-uba.jar` and writes a closed
//! version that contains the original ABox plus every triple derivable
//! under the Univ-Bench OWL-Lite axioms relevant to the 14 LUBM queries.
//!
//! ## Scope
//!
//! Hardcoded to Univ-Bench. Not a general OWL reasoner. The rule set
//! supports:
//!
//! - **subClassOf** transitive closure on `rdf:type` triples. `?x rdf:type C ∧
//!   C ⊑ D` → `?x rdf:type D`
//! - **subPropertyOf** duplication. `?x p ?y ∧ p ⊑ q` → `?x q ?y`
//! - **owl:TransitiveProperty** closure on `subOrganizationOf`. `?x p ?y ∧ ?y p
//!   ?z ∧ p transitive` → `?x p ?z`
//! - **owl:inverseOf** duplication. `?x p ?y ∧ p ≡ q⁻¹` → `?y q ?x`
//! - **Realisation** for `Chair`. `?x headOf ?d ∧ ?d rdf:type Department` → `?x
//!   rdf:type Chair`
//!
//! ## Memory model
//!
//! In-memory: the entire ABox is loaded into a `Vec<Triple>`, the closure
//! is computed in a `HashSet`, and the union is written to the output
//! file. At LUBM(50) (~6.9M ABox triples) peak memory is ~1 GB. For
//! larger scales a streaming variant could be added — the rule set
//! tolerates it (most rules are single-pass) — but the LUBM thesis
//! workload doesn't need it.
//!
//! ## Authoritativeness
//!
//! The rule constants below are derived from the LUBM paper §2.1 plus
//! the Univ-Bench class hierarchy in `lubm-uba-rs/Ontology.java`. They
//! are not parsed from `univ-bench.owl` at runtime — that would add a
//! file-format dependency for ~30 axioms that haven't changed since
//! 2005. The LUBM(1, 0) cardinality regression test
//! (`kermit/tests/lubm_cardinalities.rs`, in the `kermit` crate) is the
//! load-bearing correctness check for the rule set: missing rules manifest
//! as result counts below the LUBM paper Table 3 reference values.

use {
    crate::{error::RdfError, ntriples, value::RdfValue},
    std::{
        collections::{HashMap, HashSet},
        io::{BufWriter, Write},
        path::Path,
    },
};

/// Base IRI of the Univ-Bench ontology. Constants below refer to terms
/// relative to this prefix.
pub const UB: &str = "http://www.lehigh.edu/~zhp2/2004/0401/univ-bench.owl#";

/// `rdf:type` IRI used for class assertions.
pub const RDF_TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";

/// Direct subClassOf edges in Univ-Bench. Transitive closure is computed
/// at runtime; one-hop edges are sufficient here.
const SUBCLASS_OF: &[(&str, &str)] = &[
    ("UndergraduateStudent", "Student"),
    ("GraduateStudent", "Student"),
    ("Student", "Person"),
    ("FullProfessor", "Professor"),
    ("AssociateProfessor", "Professor"),
    ("AssistantProfessor", "Professor"),
    ("VisitingProfessor", "Professor"),
    ("Chair", "Professor"),
    ("Dean", "Professor"),
    ("Lecturer", "Faculty"),
    ("PostDoc", "Faculty"),
    ("Professor", "Faculty"),
    ("Faculty", "Employee"),
    ("AdministrativeStaff", "Employee"),
    ("ClericalStaff", "AdministrativeStaff"),
    ("SystemsStaff", "AdministrativeStaff"),
    ("Employee", "Person"),
    ("TeachingAssistant", "Person"),
    ("ResearchAssistant", "Person"),
    ("Director", "Person"),
    ("University", "Organization"),
    ("Department", "Organization"),
    ("College", "Organization"),
    ("Institute", "Organization"),
    ("Program", "Organization"),
    ("ResearchGroup", "Organization"),
    ("GraduateCourse", "Course"),
    ("Article", "Publication"),
    ("Book", "Publication"),
    ("ConferencePaper", "Article"),
    ("JournalArticle", "Article"),
    ("TechnicalReport", "Article"),
    ("Manual", "Publication"),
    ("Software", "Publication"),
    ("Specification", "Publication"),
    ("UnofficialPublication", "Publication"),
];

/// Direct subPropertyOf edges. Transitive closure computed at runtime.
const SUBPROPERTY_OF: &[(&str, &str)] = &[
    ("worksFor", "memberOf"),
    ("headOf", "worksFor"),
    ("undergraduateDegreeFrom", "degreeFrom"),
    ("mastersDegreeFrom", "degreeFrom"),
    ("doctoralDegreeFrom", "degreeFrom"),
];

/// Properties declared `owl:TransitiveProperty` in Univ-Bench.
const TRANSITIVE_PROPERTIES: &[&str] = &["subOrganizationOf"];

/// `owl:inverseOf` pairs. The closure is symmetric — for each `(p, q)`
/// here we emit both `p→q` and `q→p` derivations.
const INVERSE_OF: &[(&str, &str)] = &[("hasAlumnus", "degreeFrom")];

/// Realisation rules of the form
/// `?x <prop_iri> ?y ∧ ?y rdf:type <target_class> → ?x rdf:type
/// <derived_class>`.
const REALISATION_RULES: &[(&str, &str, &str)] = &[("headOf", "Department", "Chair")];

/// Statistics surfaced into `meta.json`.
#[derive(Debug, Clone, Copy)]
pub struct EntailmentStats {
    /// Triples read from the input file.
    pub input_triples: usize,
    /// Triples written to the output file (input + non-duplicate derivations).
    pub output_triples: usize,
    /// Number of distinct derivations produced (`output_triples -
    /// input_triples`, modulo any duplicates already present in input).
    pub derived_triples: usize,
    /// Number of fixed-point iterations executed for transitive closures.
    pub iterations: u32,
}

fn ub(local: &str) -> String { format!("{UB}{local}") }

/// Computes the transitive closure of a directed-edge set as a
/// `child → all ancestors` map. Used for both subClassOf and
/// subPropertyOf, which share the same closure semantics. Each edge
/// `(a, b)` is read as "a's parent is b".
///
/// Naive fixed point: |nodes|² edges in the worst case, which for
/// Univ-Bench is ~30² = 900 — trivial.
fn transitive_closure(edges: &[(&str, &str)]) -> HashMap<String, HashSet<String>> {
    // `ancestors` maps each node to the set of all nodes reachable by
    // following parent edges (its transitive ancestors).
    let mut ancestors: HashMap<String, HashSet<String>> = HashMap::new();
    for (child, parent) in edges {
        ancestors.entry(ub(child)).or_default().insert(ub(parent));
        // Ensure the parent appears as a key so iteration sees it.
        ancestors.entry(ub(parent)).or_default();
    }
    let mut changed = true;
    while changed {
        changed = false;
        let snapshot: Vec<(String, Vec<String>)> = ancestors
            .iter()
            .map(|(node, node_ancestors)| (node.clone(), node_ancestors.iter().cloned().collect()))
            .collect();
        for (node, node_ancestors) in snapshot {
            let mut to_add: Vec<String> = Vec::new();
            for parent in &node_ancestors {
                if let Some(ancestors_of_parent) = ancestors.get(parent) {
                    for ancestor in ancestors_of_parent {
                        if !ancestors
                            .get(&node)
                            .map(|s| s.contains(ancestor))
                            .unwrap_or(false)
                        {
                            to_add.push(ancestor.clone());
                        }
                    }
                }
            }
            for ancestor in to_add {
                if ancestors.entry(node.clone()).or_default().insert(ancestor) {
                    changed = true;
                }
            }
        }
    }
    ancestors
}

fn write_triple(
    writer: &mut BufWriter<std::fs::File>, s: &str, p: &str, o: &RdfValue,
) -> std::io::Result<()> {
    writeln!(writer, "<{s}> <{p}> {} .", o.to_canonical())
}

/// Rule 1 — subClassOf: `?x rdf:type C ∧ C ⊑ D → ?x rdf:type D`.
/// Reads from the frozen iteration snapshot (`frozen`) while inserting into
/// the working set (`working`).
fn apply_subclass_rule(
    frozen: &[(String, String, RdfValue)], rdf_type: &str,
    superclasses: &HashMap<String, HashSet<String>>,
    working: &mut HashSet<(String, String, RdfValue)>,
) {
    for (s, p, o) in frozen {
        if p != rdf_type {
            continue;
        }
        let RdfValue::Iri(c) = o else {
            continue;
        };
        if let Some(parents) = superclasses.get(c) {
            for d in parents {
                working.insert((s.clone(), rdf_type.to_string(), RdfValue::Iri(d.clone())));
            }
        }
    }
}

/// Rule 2 — subPropertyOf: `?x p ?y ∧ p ⊑ q → ?x q ?y`.
/// Reads from the frozen iteration snapshot (`frozen`) while inserting into
/// the working set (`working`).
fn apply_subproperty_rule(
    frozen: &[(String, String, RdfValue)], superproperties: &HashMap<String, HashSet<String>>,
    working: &mut HashSet<(String, String, RdfValue)>,
) {
    for (s, p, o) in frozen {
        if let Some(parents) = superproperties.get(p) {
            for q in parents {
                working.insert((s.clone(), q.clone(), o.clone()));
            }
        }
    }
}

/// Rule 3 — owl:inverseOf: `?x p ?y → ?y q ?x` for each declared `(p, q)`.
/// Skipped when `?y` is a literal or blank node — inverses on data values
/// are nonsensical, and blank-node objects cannot round-trip through the
/// current triple representation (subjects are untyped strings, so a blank
/// node lifted into subject position would serialise as a malformed IRI).
/// Reads from the frozen iteration snapshot (`frozen`) while inserting into
/// the working set (`working`).
fn apply_inverse_rule(
    frozen: &[(String, String, RdfValue)], inverse_pairs: &[(String, String)],
    working: &mut HashSet<(String, String, RdfValue)>,
) {
    for (s, p, o) in frozen {
        for (a, b) in inverse_pairs {
            if p != a {
                continue;
            }
            let new_subject = match o {
                | RdfValue::Iri(iri) => iri.clone(),
                | RdfValue::BlankNode(_) | RdfValue::Literal(_) => continue,
            };
            working.insert((new_subject, b.clone(), RdfValue::Iri(s.clone())));
        }
    }
}

/// Rule 4 — owl:TransitiveProperty: one-step closure
/// `?x p ?y ∧ ?y p ?z → ?x p ?z` for each transitive `p`. Reads from the
/// working set (`working`, not the iteration snapshot) so that the outer
/// fixed-point loop drives the multi-hop closure across iterations.
fn apply_transitive_rule(
    transitive: &HashSet<String>, working: &mut HashSet<(String, String, RdfValue)>,
) {
    for tp in transitive {
        let edges: Vec<(String, String)> = working
            .iter()
            .filter_map(|(s, p, o)| {
                if p != tp {
                    return None;
                }
                if let RdfValue::Iri(oi) = o {
                    Some((s.clone(), oi.clone()))
                } else {
                    None
                }
            })
            .collect();
        let mut by_src: HashMap<&str, Vec<&str>> = HashMap::new();
        for (a, b) in &edges {
            by_src.entry(a.as_str()).or_default().push(b.as_str());
        }
        for (a, b) in &edges {
            if let Some(nexts) = by_src.get(b.as_str()) {
                for c in nexts {
                    working.insert((a.clone(), tp.clone(), RdfValue::Iri(c.to_string())));
                }
            }
        }
    }
}

/// Rule 5 — Realisation: `?x prop ?y ∧ ?y rdf:type T → ?x rdf:type D`.
/// Reads from the working set (`working`) so derivations from earlier rules
/// in the same iteration (e.g. a `Department` that was just derived via
/// subClassOf) participate.
fn apply_realisation_rule(
    realisation: &[(String, String, String)], rdf_type: &str,
    working: &mut HashSet<(String, String, RdfValue)>,
) {
    for (prop_iri, target_class, derived_class) in realisation {
        let target_subjects: HashSet<&str> = working
            .iter()
            .filter_map(|(s, p, o)| {
                if p != rdf_type {
                    return None;
                }
                if let RdfValue::Iri(c) = o {
                    if c == target_class {
                        return Some(s.as_str());
                    }
                }
                None
            })
            .collect();
        let new_classifications: Vec<String> = working
            .iter()
            .filter_map(|(s, p, o)| {
                if p != prop_iri {
                    return None;
                }
                if let RdfValue::Iri(y) = o {
                    if target_subjects.contains(y.as_str()) {
                        return Some(s.clone());
                    }
                }
                None
            })
            .collect();
        for x in new_classifications {
            working.insert((
                x,
                rdf_type.to_string(),
                RdfValue::Iri(derived_class.clone()),
            ));
        }
    }
}

/// Forward-chains the Univ-Bench rules over `input_path` and writes the
/// closed N-Triples to `output_path`.
///
/// Errors out if the fixed-point iteration count exceeds
/// `MAX_ITERATIONS` — a buggy rule that re-triggers itself must not
/// hang silently.
pub fn entail(input_path: &Path, output_path: &Path) -> Result<EntailmentStats, RdfError> {
    const MAX_ITERATIONS: u32 = 64;

    let superclasses = transitive_closure(SUBCLASS_OF);
    let superproperties = transitive_closure(SUBPROPERTY_OF);
    let inverse_pairs: Vec<(String, String)> = INVERSE_OF
        .iter()
        .flat_map(|(a, b)| [(ub(a), ub(b)), (ub(b), ub(a))])
        .collect();
    let transitive: HashSet<String> = TRANSITIVE_PROPERTIES.iter().map(|p| ub(p)).collect();
    let realisation: Vec<(String, String, String)> = REALISATION_RULES
        .iter()
        .map(|(p, t, d)| (ub(p), ub(t), ub(d)))
        .collect();
    let rdf_type = RDF_TYPE.to_string();

    // `working` is the growing set of triples the fixed point mutates in place.
    let mut working: HashSet<(String, String, RdfValue)> = HashSet::new();
    let mut input_count: usize = 0;
    for triple in ntriples::iter_path(input_path)? {
        let (s, p, o) = triple?;
        input_count += 1;
        // Defense in depth: `lubm/driver::gunzip` already line-filters
        // `<>`-subject document-self triples. This guard catches the
        // direct-invocation case (entail() called without going through the
        // driver, e.g. on a hand-crafted N-Triples file). Empty IRIs do not
        // round-trip through N-Triples — strict parsers reject them.
        if s.is_empty() {
            continue;
        }
        working.insert((s, p, o));
    }

    let original_size = working.len();
    let mut iterations: u32 = 0;
    loop {
        iterations += 1;
        if iterations > MAX_ITERATIONS {
            return Err(RdfError::Expected(format!(
                "entailment failed to converge after {MAX_ITERATIONS} iterations — rule set \
                 probably triggers a cycle"
            )));
        }
        let before = working.len();

        // Two read conventions are in play, and the asymmetry is deliberate:
        //
        // - `frozen` is a snapshot of `working` taken at the top of each iteration.
        //   Rules 1-3 (`apply_subclass_rule`, `apply_subproperty_rule`,
        //   `apply_inverse_rule`) derive from this fixed view while inserting into
        //   `working`. Reading a stable snapshot keeps their output independent of
        //   intra-iteration ordering — they never observe a triple another rule derived
        //   in the same pass.
        // - Rules 4-5 (`apply_transitive_rule`, `apply_realisation_rule`) instead
        //   re-scan the live `working` set, because they must see derivations produced
        //   earlier in the same iteration (e.g. a `Department` typing just derived via
        //   subClassOf feeds realisation of `Chair`). The outer fixed-point loop still
        //   guarantees eventual convergence either way.
        let frozen: Vec<(String, String, RdfValue)> = working.iter().cloned().collect();

        apply_subclass_rule(&frozen, &rdf_type, &superclasses, &mut working);
        apply_subproperty_rule(&frozen, &superproperties, &mut working);
        apply_inverse_rule(&frozen, &inverse_pairs, &mut working);
        apply_transitive_rule(&transitive, &mut working);
        apply_realisation_rule(&realisation, &rdf_type, &mut working);

        if working.len() == before {
            break;
        }
    }

    let out = std::fs::File::create(output_path)?;
    let mut writer = BufWriter::new(out);
    let mut output_count = 0;
    for (s, p, o) in &working {
        write_triple(&mut writer, s, p, o)?;
        output_count += 1;
    }
    writer.flush()?;

    Ok(EntailmentStats {
        input_triples: input_count,
        output_triples: output_count,
        derived_triples: output_count.saturating_sub(original_size),
        iterations,
    })
}

#[cfg(test)]
mod tests {
    use {super::*, std::io::Write};

    fn write_temp(contents: &str) -> tempfile::NamedTempFile {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(contents.as_bytes()).unwrap();
        f.flush().unwrap();
        f
    }

    fn run_entailment(input: &str) -> (HashSet<(String, String, RdfValue)>, EntailmentStats) {
        let in_file = write_temp(input);
        let out_file = tempfile::NamedTempFile::new().unwrap();
        let stats = entail(in_file.path(), out_file.path()).unwrap();
        let triples: HashSet<_> = ntriples::iter_path(out_file.path())
            .unwrap()
            .map(|t| t.unwrap())
            .collect();
        (triples, stats)
    }

    fn type_of(s: &str, c: &str) -> (String, String, RdfValue) {
        (
            s.to_string(),
            RDF_TYPE.to_string(),
            RdfValue::Iri(format!("{UB}{c}")),
        )
    }

    #[test]
    fn subclass_propagates_one_hop() {
        // GraduateStudent ⊑ Student
        let nt = format!("<http://x/jane> <{RDF_TYPE}> <{UB}GraduateStudent> .\n");
        let (triples, _) = run_entailment(&nt);
        assert!(triples.contains(&type_of("http://x/jane", "GraduateStudent")));
        assert!(triples.contains(&type_of("http://x/jane", "Student")));
    }

    #[test]
    fn subclass_propagates_transitively_to_person() {
        // GraduateStudent ⊑ Student ⊑ Person
        let nt = format!("<http://x/jane> <{RDF_TYPE}> <{UB}GraduateStudent> .\n");
        let (triples, _) = run_entailment(&nt);
        assert!(triples.contains(&type_of("http://x/jane", "Person")));
    }

    #[test]
    fn full_professor_classified_as_faculty_and_employee_and_person() {
        let nt = format!("<http://x/p> <{RDF_TYPE}> <{UB}FullProfessor> .\n");
        let (triples, _) = run_entailment(&nt);
        for c in [
            "FullProfessor",
            "Professor",
            "Faculty",
            "Employee",
            "Person",
        ] {
            assert!(
                triples.contains(&type_of("http://x/p", c)),
                "missing classification {c}"
            );
        }
    }

    #[test]
    fn subproperty_works_for_implies_member_of() {
        let nt = format!("<http://x/p> <{UB}worksFor> <http://x/d> .\n");
        let (triples, _) = run_entailment(&nt);
        assert!(triples.contains(&(
            "http://x/p".to_string(),
            format!("{UB}worksFor"),
            RdfValue::Iri("http://x/d".to_string())
        )));
        assert!(triples.contains(&(
            "http://x/p".to_string(),
            format!("{UB}memberOf"),
            RdfValue::Iri("http://x/d".to_string())
        )));
    }

    #[test]
    fn head_of_implies_works_for_and_member_of() {
        // headOf ⊑ worksFor ⊑ memberOf
        let nt = format!("<http://x/p> <{UB}headOf> <http://x/d> .\n");
        let (triples, _) = run_entailment(&nt);
        for prop in ["headOf", "worksFor", "memberOf"] {
            assert!(
                triples.contains(&(
                    "http://x/p".to_string(),
                    format!("{UB}{prop}"),
                    RdfValue::Iri("http://x/d".to_string())
                )),
                "missing super-property {prop}"
            );
        }
    }

    #[test]
    fn realisation_chair_from_head_of_department() {
        let nt = format!(
            "<http://x/p> <{UB}headOf> <http://x/d> .\n<http://x/d> <{RDF_TYPE}> <{UB}Department> \
             .\n"
        );
        let (triples, _) = run_entailment(&nt);
        assert!(triples.contains(&type_of("http://x/p", "Chair")));
        // Chair ⊑ Professor ⊑ Faculty ⊑ Employee ⊑ Person
        assert!(triples.contains(&type_of("http://x/p", "Professor")));
        assert!(triples.contains(&type_of("http://x/p", "Person")));
    }

    #[test]
    fn no_realisation_without_department_typing() {
        let nt = format!("<http://x/p> <{UB}headOf> <http://x/d> .\n");
        let (triples, _) = run_entailment(&nt);
        assert!(!triples.contains(&type_of("http://x/p", "Chair")));
    }

    #[test]
    fn inverse_of_has_alumnus_and_degree_from() {
        let nt = format!("<http://x/u> <{UB}hasAlumnus> <http://x/p> .\n");
        let (triples, _) = run_entailment(&nt);
        assert!(triples.contains(&(
            "http://x/p".to_string(),
            format!("{UB}degreeFrom"),
            RdfValue::Iri("http://x/u".to_string())
        )));
    }

    #[test]
    fn inverse_rule_skips_blank_node_objects() {
        // Regression: a blank-node object previously had its raw `_:b…` string
        // promoted to subject position, where it would serialise as a
        // malformed IRI and round-trip back as a different RdfValue variant
        // than the original blank node.
        let nt = format!("<http://x/u> <{UB}hasAlumnus> _:b42 .\n");
        let (triples, _) = run_entailment(&nt);
        // No inverse triple should be derived for the blank-node alumnus.
        for (_, p, _) in &triples {
            assert_ne!(
                p,
                &format!("{UB}degreeFrom"),
                "blank-node alumnus must not produce a degreeFrom triple"
            );
        }
    }

    #[test]
    fn doctoral_degree_from_implies_degree_from_implies_inverse_has_alumnus() {
        // doctoralDegreeFrom ⊑ degreeFrom; degreeFrom ↔ hasAlumnus
        let nt = format!("<http://x/p> <{UB}doctoralDegreeFrom> <http://x/u> .\n");
        let (triples, _) = run_entailment(&nt);
        assert!(triples.contains(&(
            "http://x/p".to_string(),
            format!("{UB}degreeFrom"),
            RdfValue::Iri("http://x/u".to_string())
        )));
        assert!(triples.contains(&(
            "http://x/u".to_string(),
            format!("{UB}hasAlumnus"),
            RdfValue::Iri("http://x/p".to_string())
        )));
    }

    #[test]
    fn transitive_sub_organization_of_chains() {
        // ResearchGroup ⊑ Department ⊑ University via subOrganizationOf
        let nt = format!(
            "<http://x/rg> <{UB}subOrganizationOf> <http://x/d> .\n\
             <http://x/d> <{UB}subOrganizationOf> <http://x/u> .\n"
        );
        let (triples, _) = run_entailment(&nt);
        assert!(triples.contains(&(
            "http://x/rg".to_string(),
            format!("{UB}subOrganizationOf"),
            RdfValue::Iri("http://x/u".to_string())
        )));
    }

    #[test]
    fn empty_input_produces_empty_output() {
        let (triples, stats) = run_entailment("");
        assert!(triples.is_empty());
        assert_eq!(stats.input_triples, 0);
        assert_eq!(stats.output_triples, 0);
        assert_eq!(stats.derived_triples, 0);
    }

    #[test]
    fn literals_pass_through_unchanged() {
        let nt = format!("<http://x/p> <{UB}name> \"Alice\" .\n");
        let (triples, _) = run_entailment(&nt);
        assert!(triples
            .iter()
            .any(|(_, p, o)| { p.ends_with("#name") && matches!(o, RdfValue::Literal(_)) }));
    }

    #[test]
    fn statistics_count_input_and_derived() {
        let nt = format!("<http://x/p> <{RDF_TYPE}> <{UB}FullProfessor> .\n");
        let (_, stats) = run_entailment(&nt);
        assert_eq!(stats.input_triples, 1);
        // FullProfessor → Professor → Faculty → Employee → Person, so we
        // expect 4 derived rdf:type triples on top of the 1 input.
        assert!(stats.output_triples >= 5);
        assert!(stats.derived_triples >= 4);
    }
}
