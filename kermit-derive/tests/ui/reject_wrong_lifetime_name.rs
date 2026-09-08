//! `#[derive(IntoTrieIter)]` must reject a lifetime not named `'a`.
use kermit_derive::IntoTrieIter;

#[derive(IntoTrieIter)]
struct WrongName<'b> {
    data: &'b [usize],
}

fn main() {}
