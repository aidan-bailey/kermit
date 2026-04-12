//! Star join example: joins three relations through a shared hub variable.
//!
//! ```text
//! star(Person, Dept, Salary, Office) :-
//!   works_in(Person, Dept),
//!   earns(Person, Salary),
//!   located(Person, Office).
//! ```
//!
//! Run with: `cargo run --example star`

use {
    kermit::db::instantiate_database,
    kermit_algos::{JoinAlgorithm, JoinQuery},
    kermit_ds::IndexStructure,
    std::path::Path,
};

fn run_star_join(ds: IndexStructure) {
    let example_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/star");

    let query: JoinQuery = std::fs::read_to_string(example_dir.join("star_query.dl"))
        .expect("failed to read query file")
        .trim()
        .parse()
        .expect("failed to parse query");

    let mut db = instantiate_database(ds, JoinAlgorithm::LeapfrogTriejoin);
    for file in &["works_in.csv", "earns.csv", "located.csv"] {
        db.add_file(&example_dir.join(file))
            .unwrap_or_else(|e| panic!("failed to load {file}: {e}"));
    }

    let result = db.join(query);

    println!("--- {ds:?} ---");
    for tuple in &result {
        let line: String = tuple
            .iter()
            .map(|v| v.to_string())
            .collect::<Vec<_>>()
            .join(",");
        println!("{line}");
    }
    println!();
}

fn main() {
    run_star_join(IndexStructure::TreeTrie);
    run_star_join(IndexStructure::ColumnTrie);
}
