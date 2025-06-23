use {
    proc_macro::TokenStream,
    quote::quote,
    syn::{parse_macro_input, DeriveInput},
};

#[proc_macro_derive(IntoTrieIter)]
pub fn derive_into_trie_iter(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let ident = &input.ident;

    let output = quote! {

        impl<'a, KT> IntoIterator for #ident<'a, KT>
        where
            KT: KeyType
        {
            type Item = Vec<&'a KT>;
            type IntoIter = TrieIteratorWrapper<'a, Self>;

            fn into_iter(self) -> Self::IntoIter {
                TrieIteratorWrapper::new(self)
            }
        }

    };

    output.into()
}
