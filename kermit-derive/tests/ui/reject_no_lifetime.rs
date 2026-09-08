//! `#[derive(IntoTrieIter)]` must reject a struct with no generic parameters.
use kermit_derive::IntoTrieIter;

#[derive(IntoTrieIter)]
struct NoLifetime {
    pos: usize,
}

fn main() {}
