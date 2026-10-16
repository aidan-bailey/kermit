//! The SNAP mini fixture (#96), shared by `snap_generate.rs` (the generator
//! and the joins in-process) and `cli_snap_gen.rs` (the binary against a
//! loopback server): the fixture's text, its gzipped bytes, and the counts
//! both tests pin. `snap_generate.rs`'s module doc describes the fixture and
//! derives the counts.

use {
    flate2::{write::GzEncoder, Compression},
    std::{fs, io::Write, path::Path},
};

/// The fixture's answer counts, in `SnapQuery::ALL` order, derived
/// independently of the generator (see `snap_generate.rs`'s module doc).
pub const MINI_COUNTS: [(&str, u64); 4] = [
    ("triangle", 36),
    ("path3", 482),
    ("cycle4", 322),
    ("clique4", 24),
];

/// The text of `tests/fixtures/snap/mini.ungraph.txt`.
pub fn fixture_text() -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/snap/mini.ungraph.txt");
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()))
}

/// Gzips `text` in memory. The header carries no timestamp, so the bytes
/// (and the digest a test takes of them) depend only on `text`.
pub fn gzip(text: &str) -> Vec<u8> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder
        .write_all(text.as_bytes())
        .expect("gzip the fixture");
    encoder.finish().expect("finish the gzip stream")
}
