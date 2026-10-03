//! `#[derive(IntoTrieIter)]` must reject a second lifetime after `'a`.
use kermit_derive::IntoTrieIter;

#[derive(IntoTrieIter)]
struct TwoLifetimes<'a, 'b> {
    first: &'a [usize],
    second: &'b [usize],
}

fn main() {}
