//! `#[derive(IntoTrieIter)]` must reject a const parameter after `'a`.
use kermit_derive::IntoTrieIter;

#[derive(IntoTrieIter)]
struct WithConst<'a, const N: usize> {
    data: &'a [usize; N],
}

fn main() {}
