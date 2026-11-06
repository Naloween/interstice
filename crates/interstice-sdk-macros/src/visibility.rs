//! `public` / `local` / `internal` on `#[reducer]` and `#[query]` (DESIGN §7.4).

use quote::quote;
use syn::Meta;

/// The `CallVisibility` a bare-word option names, if it is one.
pub fn visibility_option(meta: &Meta) -> Option<proc_macro2::TokenStream> {
    let Meta::Path(path) = meta else { return None };
    let ident = path.get_ident()?.to_string();
    match ident.as_str() {
        "public" => Some(quote! { interstice_sdk::CallVisibility::Public }),
        "local" => Some(quote! { interstice_sdk::CallVisibility::Local }),
        "internal" => Some(quote! { interstice_sdk::CallVisibility::Internal }),
        _ => None,
    }
}

/// The declared visibility among `attributes` (`Default` when none); an
/// error if more than one is given.
pub fn declared_visibility(
    attributes: &syn::punctuated::Punctuated<Meta, syn::Token![,]>,
) -> syn::Result<proc_macro2::TokenStream> {
    let mut found = None;
    for meta in attributes {
        if let Some(v) = visibility_option(meta) {
            if found.is_some() {
                return Err(syn::Error::new_spanned(meta, "give at most one of `public`, `local`, `internal`"));
            }
            found = Some(v);
        }
    }
    Ok(found.unwrap_or_else(|| quote! { interstice_sdk::CallVisibility::Default }))
}
