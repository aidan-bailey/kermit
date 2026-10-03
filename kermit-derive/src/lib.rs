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
//! A struct generic over a type parameter after `'a`, such as
//! `TreeTrieIter<'a, S: SeekStrategy>`, gets
//! `impl<'a, S: SeekStrategy> IntoIterator for TreeTrieIter<'a, S>`.
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
/// - take the lifetime `'a` as its first generic parameter, optionally followed
///   by type parameters (a Layout such as a seek strategy). Declare their
///   bounds on the struct: the generated impl carries them, which is what lets
///   it name `TrieIteratorWrapper<Self>`.
///
/// The expanded impl wraps `self` in a `kermit_iters::TrieIteratorWrapper`,
/// yielding each root-to-leaf path in the trie as a `Vec<usize>`.
#[proc_macro_derive(IntoTrieIter)]
pub fn derive_into_trie_iter(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let ident = &input.ident;

    let params: Vec<_> = input.generics.params.iter().collect();
    let valid = match params.split_first() {
        | Some((GenericParam::Lifetime(lt), rest)) => {
            lt.lifetime.ident == "a" && rest.iter().all(|p| matches!(p, GenericParam::Type(_)))
        },
        | _ => false,
    };
    if !valid {
        let msg = format!(
            "#[derive(IntoTrieIter)] requires the lifetime `'a` as the first generic parameter, \
             optionally followed by type parameters; found {} parameter(s) on `{}`",
            params.len(),
            ident
        );
        return quote! { compile_error!(#msg); }.into();
    }

    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    let output = quote! {

        impl #impl_generics IntoIterator for #ident #ty_generics #where_clause {
            type Item = Vec<usize>;
            type IntoIter = TrieIteratorWrapper<Self>;

            fn into_iter(self) -> Self::IntoIter {
                TrieIteratorWrapper::new(self)
            }
        }

    };

    output.into()
}
