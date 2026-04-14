mod schema;
mod subscription;
mod wrapper;

use proc_macro::TokenStream;
use quote::quote;
use syn::{ItemFn, Meta, parse_macro_input};

use crate::context_caps::reducer_caps_ty;
use crate::reducer::{
    schema::get_register_schema_function, subscription::get_register_subscription_function,
    wrapper::get_wrapper_function,
};

fn validate_reducer_attrs(attributes: &syn::punctuated::Punctuated<Meta, syn::Token![,]>) -> syn::Result<()> {
    for meta in attributes {
        match meta {
            Meta::NameValue(nv) if nv.path.is_ident("on") => {}
            _ => {
                return Err(syn::Error::new_spanned(
                    meta,
                    "unsupported #[reducer] option; use only `on = \"…\"` for subscriptions (table access is declared via `ReducerContext<Caps>` and `where Caps: CanRead<Row> + …`)",
                ));
            }
        }
    }
    Ok(())
}

pub fn reducer_macro(attr: TokenStream, item: TokenStream) -> TokenStream {
    let mut input_fn = parse_macro_input!(item as ItemFn);
    let reducer_ident = input_fn.sig.ident.clone();

    let arg_count = input_fn.sig.inputs.len();
    if arg_count == 0 {
        return quote! {compile_error!("The reducer should have at least a `ReducerContext` first argument");}.into();
    }

    let caps_kind = match crate::caps_bounds::classify_and_rewrite_context_fn(&mut input_fn, "ReducerContext")
    {
        Ok(k) => k,
        Err(e) => return e.into_compile_error().into(),
    };

    let mut synthetic_defs = proc_macro2::TokenStream::new();
    let mut synthetic_caps_ident: Option<syn::Ident> = None;

    if let crate::caps_bounds::ContextCapsKind::GenericParam { ref bounds, .. } = caps_kind {
        match crate::caps_bounds::emit_reducer_synthetic_caps(&reducer_ident, bounds) {
            Ok((id, defs)) => {
                synthetic_caps_ident = Some(id.clone());
                synthetic_defs = defs;
                if let Some(syn::FnArg::Typed(pat)) = input_fn.sig.inputs.iter_mut().next() {
                    pat.ty = syn::parse_quote! { interstice_sdk::ReducerContext<#id> };
                }
            }
            Err(e) => return e.into_compile_error().into(),
        }
    }

    let first_arg_ty = match input_fn.sig.inputs.first() {
        Some(syn::FnArg::Typed(pat)) => pat.ty.as_ref(),
        _ => {
            return quote! {compile_error!("The reducer should have a typed first argument");}.into();
        }
    };

    let caps_ty = match reducer_caps_ty(first_arg_ty) {
        Ok(t) => t,
        Err(e) => return e.into_compile_error().into(),
    };

    let caps_extend_body = match &caps_kind {
        crate::caps_bounds::ContextCapsKind::GenericParam { .. } => {
            let id = synthetic_caps_ident
                .as_ref()
                .expect("synthetic caps ident set for GenericParam");
            quote! {
                <#id as interstice_sdk::ReducerCaps>::extend_reducer_schema(
                    &mut reads,
                    &mut inserts,
                    &mut updates,
                    &mut deletes,
                );
            }
        }
        crate::caps_bounds::ContextCapsKind::Concrete(ty) => {
            quote! {
                <#ty as interstice_sdk::ReducerCaps>::extend_reducer_schema(
                    &mut reads,
                    &mut inserts,
                    &mut updates,
                    &mut deletes,
                );
            }
        }
        crate::caps_bounds::ContextCapsKind::DefaultEmptyCaps => {
            quote! {
                <() as interstice_sdk::ReducerCaps>::extend_reducer_schema(
                    &mut reads,
                    &mut inserts,
                    &mut updates,
                    &mut deletes,
                );
            }
        }
    };

    let returns_unit = match &input_fn.sig.output {
        syn::ReturnType::Default => true,
        syn::ReturnType::Type(_, ty) => {
            matches!(ty.as_ref(), syn::Type::Tuple(t) if t.elems.is_empty())
        }
    };
    if !returns_unit {
        return quote! {compile_error!("Reducers must not return a value. Use #[query] for read-only return values.");}.into();
    }

    let attributes: syn::punctuated::Punctuated<Meta, syn::Token![,]> = if attr.is_empty() {
        syn::punctuated::Punctuated::new()
    } else {
        parse_macro_input!(attr with syn::punctuated::Punctuated::<Meta, syn::Token![,]>::parse_terminated)
    };

    if let Err(e) = validate_reducer_attrs(&attributes) {
        return e.into_compile_error().into();
    }

    let (register_subscription, use_table_subscription) =
        get_register_subscription_function(reducer_ident.clone(), attributes);

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

    let wrapper_function = get_wrapper_function(
        reducer_ident.clone(),
        caps_ty.clone(),
        arg_count,
        use_table_subscription,
    );

    let register_schema = get_register_schema_function(
        reducer_ident.clone(),
        arg_names,
        arg_types,
        caps_extend_body,
    );

    quote! {
        #synthetic_defs

        #input_fn

        #wrapper_function

        #register_schema

        #register_subscription
    }
    .into()
}
