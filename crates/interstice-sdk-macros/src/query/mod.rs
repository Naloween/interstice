mod schema;
mod wrapper;

use proc_macro::TokenStream;
use quote::quote;
use syn::{ItemFn, Meta, parse_macro_input};

use crate::context_caps::query_caps_ty;
use crate::query::{schema::get_register_schema_function, wrapper::get_wrapper_function};

/// `Some(T)` when `ty` is spelled `Result<T, E>` (any path ending in `Result`).
pub(crate) fn result_ok_type(ty: &syn::Type) -> Option<&syn::Type> {
    let syn::Type::Path(p) = ty else { return None };
    let seg = p.path.segments.last()?;
    if seg.ident != "Result" {
        return None;
    }
    let syn::PathArguments::AngleBracketed(args) = &seg.arguments else { return None };
    match args.args.first()? {
        syn::GenericArgument::Type(t) => Some(t),
        _ => None,
    }
}

fn validate_query_attrs(attributes: &syn::punctuated::Punctuated<Meta, syn::Token![,]>) -> syn::Result<()> {
    for meta in attributes {
        if crate::visibility::visibility_option(meta).is_some() {
            continue;
        }
        return Err(syn::Error::new_spanned(
            meta,
            "unsupported #[query] option; use `public` / `local` / `internal` for who may call it, and declare reads via `QueryContext<Caps>` and `where Caps: CanRead<Row>`",
        ));
    }
    Ok(())
}

pub fn query_macro(attr: TokenStream, item: TokenStream) -> TokenStream {
    let mut input_fn = parse_macro_input!(item as ItemFn);
    let query_ident = input_fn.sig.ident.clone();

    if let Some(errors) = crate::types::signature_type_errors(&input_fn, true) {
        return errors.into();
    }

    let arg_count = input_fn.sig.inputs.len();
    if arg_count == 0 {
        return quote! {compile_error!("The query should have at least a `QueryContext` first argument");}.into();
    }

    let caps_kind = match crate::caps_bounds::classify_and_rewrite_context_fn(&mut input_fn, "QueryContext") {
        Ok(k) => k,
        Err(e) => return e.into_compile_error().into(),
    };

    let mut synthetic_defs = proc_macro2::TokenStream::new();
    let mut synthetic_caps_ident: Option<syn::Ident> = None;

    if let crate::caps_bounds::ContextCapsKind::GenericParam { ref bounds, .. } = caps_kind {
        if let Err(e) = crate::caps_bounds::validate_query_can_bounds(bounds) {
            return e.into_compile_error().into();
        }
        match crate::caps_bounds::emit_query_synthetic_caps(&query_ident, bounds) {
            Ok((id, defs)) => {
                synthetic_caps_ident = Some(id.clone());
                synthetic_defs = defs;
                if let Some(syn::FnArg::Typed(pat)) = input_fn.sig.inputs.iter_mut().next() {
                    pat.ty = syn::parse_quote! { interstice_sdk::QueryContext<#id> };
                }
            }
            Err(e) => return e.into_compile_error().into(),
        }
    }

    let first_arg_ty = match input_fn.sig.inputs.first() {
        Some(syn::FnArg::Typed(pat)) => pat.ty.as_ref(),
        _ => {
            return quote! {compile_error!("The query should have a typed first argument");}.into();
        }
    };

    let caps_ty = match query_caps_ty(first_arg_ty) {
        Ok(t) => t,
        Err(e) => return e.into_compile_error().into(),
    };

    let caps_extend_body = match &caps_kind {
        crate::caps_bounds::ContextCapsKind::GenericParam { .. } => {
            let id = synthetic_caps_ident
                .as_ref()
                .expect("synthetic caps ident set for GenericParam");
            quote! {
                <#id as interstice_sdk::QueryCaps>::extend_query_schema(&mut reads);
            }
        }
        crate::caps_bounds::ContextCapsKind::Concrete(ty) => {
            quote! {
                <#ty as interstice_sdk::QueryCaps>::extend_query_schema(&mut reads);
            }
        }
        crate::caps_bounds::ContextCapsKind::DefaultEmptyCaps => {
            quote! {
                <() as interstice_sdk::QueryCaps>::extend_query_schema(&mut reads);
            }
        }
    };

    let attributes: syn::punctuated::Punctuated<Meta, syn::Token![,]> = if attr.is_empty() {
        syn::punctuated::Punctuated::new()
    } else {
        parse_macro_input!(attr with syn::punctuated::Punctuated::<Meta, syn::Token![,]>::parse_terminated)
    };

    if let Err(e) = validate_query_attrs(&attributes) {
        return e.into_compile_error().into();
    }
    let visibility = match crate::visibility::declared_visibility(&attributes) {
        Ok(v) => v,
        Err(e) => return e.into_compile_error().into(),
    };

    let arg_names: Vec<_> = input_fn
        .sig
        .inputs
        .iter()
        .map(|arg| match arg {
            syn::FnArg::Typed(pat) => &pat.pat,
            _ => panic!("Unexpected argument type"),
        })
        .collect();

    let arg_types: Vec<_> = input_fn
        .sig
        .inputs
        .iter()
        .map(|arg| match arg {
            syn::FnArg::Typed(pat) => &pat.ty,
            _ => panic!("Unexpected argument type"),
        })
        .collect();

    let returns_result = match &input_fn.sig.output {
        syn::ReturnType::Type(_, ty) => result_ok_type(ty).is_some(),
        syn::ReturnType::Default => false,
    };
    let wrapper_function =
        get_wrapper_function(query_ident.clone(), caps_ty.clone(), arg_count, returns_result);
    let register_schema = get_register_schema_function(
        query_ident.clone(),
        input_fn.clone(),
        arg_names,
        arg_types,
        caps_extend_body,
        visibility,
    );

    quote! {
        #synthetic_defs

        #input_fn

        #wrapper_function

        #register_schema
    }
    .into()
}
