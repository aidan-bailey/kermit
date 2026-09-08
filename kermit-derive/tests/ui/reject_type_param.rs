//! `#[derive(IntoTrieIter)]` must reject a type parameter alongside `'a`.
use kermit_derive::IntoTrieIter;

#[derive(IntoTrieIter)]
struct WithTypeParam<'a, T> {
    data: &'a [T],
}

fn main() {}
