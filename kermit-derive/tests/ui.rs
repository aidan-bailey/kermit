//! Compile-fail (UI) tests for `#[derive(IntoTrieIter)]`.
//!
//! The derive emits a `compile_error!` unless the annotated struct's generics
//! are the lifetime `'a` followed only by type parameters. Each file under
//! `tests/ui/` exercises one rejected shape; the sibling `.stderr` pins the
//! diagnostic text.
//!
//! Ignored under miri: `trybuild` shells out to `cargo`, which miri cannot
//! interpret.

#[test]
#[cfg_attr(miri, ignore = "trybuild invokes cargo, which miri cannot run")]
fn derive_rejects_invalid_generics() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/ui/*.rs");
}
