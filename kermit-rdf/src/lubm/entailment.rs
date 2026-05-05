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
//! (`tests/lubm_cardinalities.rs`) is the load-bearing correctness check
//! for the rule set: missing rules manifest as result counts below the
//! LUBM paper Table 3 reference values.

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
    let mut sup: HashMap<String, HashSet<String>> = HashMap::new();
    for (child, parent) in edges {
        sup.entry(ub(child)).or_default().insert(ub(parent));
        // Ensure the parent appears as a key so iteration sees it.
        sup.entry(ub(parent)).or_default();
    }
    let mut changed = true;
    while changed {
        changed = false;
        let snap: Vec<(String, Vec<String>)> = sup
            .iter()
            .map(|(k, v)| (k.clone(), v.iter().cloned().collect()))
            .collect();
        for (node, parents) in snap {
            let mut to_add: Vec<String> = Vec::new();
            for p in &parents {
                if let Some(grand) = sup.get(p) {
                    for g in grand {
                        if !sup.get(&node).map(|s| s.contains(g)).unwrap_or(false) {
                            to_add.push(g.clone());
                        }
                    }
                }
            }
            for g in to_add {
                if sup.entry(node.clone()).or_default().insert(g) {
                    changed = true;
                }
            }
        }
    }
    sup
}

fn write_triple(
    writer: &mut BufWriter<std::fs::File>, s: &str, p: &str, o: &RdfValue,
) -> std::io::Result<()> {
    writeln!(writer, "<{s}> <{p}> {} .", o.to_canonical())
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

    let mut all: HashSet<(String, String, RdfValue)> = HashSet::new();
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
        all.insert((s, p, o));
    }

    let original_size = all.len();
    let mut iterations: u32 = 0;
    loop {
        iterations += 1;
        if iterations > MAX_ITERATIONS {
            return Err(RdfError::Expected(format!(
                "entailment failed to converge after {MAX_ITERATIONS} iterations — rule set \
                 probably triggers a cycle"
            )));
        }
        let before = all.len();

        // Snapshot current triples for derivation; we mutate `all`.
        let snap: Vec<(String, String, RdfValue)> = all.iter().cloned().collect();

        // subClassOf: ?x rdf:type C → ?x rdf:type D for each D ∈ superclasses(C)
        for (s, p, o) in &snap {
            if p == &rdf_type {
                if let RdfValue::Iri(c) = o {
                    if let Some(parents) = superclasses.get(c) {
                        for d in parents {
                            all.insert((s.clone(), rdf_type.clone(), RdfValue::Iri(d.clone())));
                        }
                    }
                }
            }
        }

        // subPropertyOf: ?x p ?y → ?x q ?y for each q ∈ superproperties(p)
        for (s, p, o) in &snap {
            if let Some(parents) = superproperties.get(p) {
                for q in parents {
                    all.insert((s.clone(), q.clone(), o.clone()));
                }
            }
        }

        // owl:inverseOf: ?x p ?y → ?y q ?x  (only when ?y is an IRI/blank, not a
        // literal)
        for (s, p, o) in &snap {
            for (a, b) in &inverse_pairs {
                if p == a {
                    let new_subject = match o {
                        | RdfValue::Iri(iri) => iri.clone(),
                        | RdfValue::BlankNode(b) => b.clone(),
                        | RdfValue::Literal(_) => continue,
                    };
                    all.insert((new_subject, b.clone(), RdfValue::Iri(s.clone())));
                }
            }
        }

        // owl:TransitiveProperty: gather all (s, o) for each transitive p,
        // compute join `(s, o) ⋈ (o, z) → (s, z)`. Stored back into `all`.
        for tp in &transitive {
            // Collect current edges for this property.
            let edges: Vec<(String, String)> = all
                .iter()
                .filter_map(|(s, p, o)| {
                    if p == tp {
                        if let RdfValue::Iri(oi) = o {
                            Some((s.clone(), oi.clone()))
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                })
                .collect();
            // Build adjacency for one-step join.
            let mut by_src: HashMap<&str, Vec<&str>> = HashMap::new();
            for (a, b) in &edges {
                by_src.entry(a.as_str()).or_default().push(b.as_str());
            }
            for (a, b) in &edges {
                if let Some(nexts) = by_src.get(b.as_str()) {
                    for c in nexts {
                        all.insert((a.clone(), tp.clone(), RdfValue::Iri(c.to_string())));
                    }
                }
            }
        }

        // Realisation: ?x p ?y ∧ ?y rdf:type T → ?x rdf:type D
        for (prop_iri, target_class, derived_class) in &realisation {
            // Index ?y rdf:type target_class.
            let target_subjects: HashSet<&str> = all
                .iter()
                .filter_map(|(s, p, o)| {
                    if p == &rdf_type {
                        if let RdfValue::Iri(c) = o {
                            if c == target_class {
                                return Some(s.as_str());
                            }
                        }
                    }
                    None
                })
                .collect();
            // Find ?x prop_iri ?y where ?y in target_subjects.
            let new_classifications: Vec<String> = all
                .iter()
                .filter_map(|(s, p, o)| {
                    if p == prop_iri {
                        if let RdfValue::Iri(y) = o {
                            if target_subjects.contains(y.as_str()) {
                                return Some(s.clone());
                            }
                        }
                    }
                    None
                })
                .collect();
            for x in new_classifications {
                all.insert((x, rdf_type.clone(), RdfValue::Iri(derived_class.clone())));
            }
        }

        if all.len() == before {
            break;
        }
    }

    let out = std::fs::File::create(output_path)?;
    let mut writer = BufWriter::new(out);
    let mut output_count = 0;
    for (s, p, o) in &all {
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
