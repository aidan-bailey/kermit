//! Procedural macros for the Kermit crate.
//!
//! Provides `#[derive(IntoTrieIter)]` to automatically implement
//! `IntoIterator` for trie iterator types, converting them into a
//! `TrieIteratorWrapper` that yields `Vec<usize>` tuples.

use {
    proc_macro::TokenStream,
    quote::quote,
    syn::{parse_macro_input, DeriveInput},
};

/// Derives `IntoIterator` for a trie iterator struct, wrapping it in a
/// `TrieIteratorWrapper` so that `for tuple in iter { ... }` works directly.
///
/// The annotated struct must implement `TrieIterator` and have a lifetime
/// parameter `'a`.
#[proc_macro_derive(IntoTrieIter)]
pub fn derive_into_trie_iter(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let ident = &input.ident;

    let output = quote! {

        impl<'a> IntoIterator for #ident<'a> {
            type Item = Vec<usize>;
            type IntoIter = TrieIteratorWrapper<Self>;

            fn into_iter(self) -> Self::IntoIter {
                TrieIteratorWrapper::new(self)
            }
        }

    };

    output.into()
}
