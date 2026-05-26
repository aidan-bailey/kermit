//! Procedural macros for the Kermit workspace.
//!
//! Provides `#[derive(IntoTrieIter)]` to automatically implement
//! [`IntoIterator`] for trie-iterator types, wrapping them in
//! `TrieIteratorWrapper` so that `for tuple in iter { ... }` yields
//! `Vec<usize>` tuples directly.
//!
//! # Example
//!
//! ```ignore
//! use kermit_iters::{LinearIterator, TrieIterator, TrieIteratorWrapper};
//! use kermit_derive::IntoTrieIter;
//!
//! #[derive(IntoTrieIter)]
//! struct MyTrieIter<'a> {
//!     // ... fields ...
//! }
//!
//! impl LinearIterator for MyTrieIter<'_> { /* ... */ }
//! impl TrieIterator   for MyTrieIter<'_> { /* ... */ }
//!
//! // The derive generates:
//! //   impl<'a> IntoIterator for MyTrieIter<'a> {
//! //       type Item = Vec<usize>;
//! //       type IntoIter = TrieIteratorWrapper<Self>;
//! //       fn into_iter(self) -> Self::IntoIter { TrieIteratorWrapper::new(self) }
//! //   }
//! ```
//!
//! See `kermit-derive/tests/derive_into_trie_iter.rs` for a runnable example
//! with a minimal mock trie, and `kermit-ds` for two production uses
//! (`TreeTrieIter`, `ColumnTrieIter`).

#![deny(missing_docs)]

use {
    proc_macro::TokenStream,
    quote::quote,
    syn::{parse_macro_input, DeriveInput, GenericParam},
};

/// Derives [`IntoIterator`] for a trie-iterator struct.
///
/// The annotated struct must:
/// - implement `kermit_iters::TrieIterator` (and therefore
///   `kermit_iters::LinearIterator`),
/// - have exactly one generic parameter, and it must be a lifetime named `'a`.
///
/// The expanded impl wraps `self` in a `kermit_iters::TrieIteratorWrapper`,
/// yielding each root-to-leaf path in the trie as a `Vec<usize>`.
#[proc_macro_derive(IntoTrieIter)]
pub fn derive_into_trie_iter(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let ident = &input.ident;

    let params: Vec<_> = input.generics.params.iter().collect();
    let valid =
        matches!(params.as_slice(), [GenericParam::Lifetime(lt)] if lt.lifetime.ident == "a");
    if !valid {
        let msg = format!(
            "#[derive(IntoTrieIter)] requires exactly one generic parameter, a lifetime named \
             `'a`; found {} parameter(s) on `{}`",
            params.len(),
            ident
        );
        return quote! { compile_error!(#msg); }.into();
    }

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
