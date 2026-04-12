mod schema;
mod subscription;
mod wrapper;

use proc_macro::TokenStream;
use quote::{ToTokens, quote};
use syn::{Expr, ItemFn, Meta, parse_macro_input};

use crate::reducer::{
    schema::get_register_schema_function, subscription::get_register_subscription_function,
    wrapper::get_wrapper_function,
};

pub fn reducer_macro(attr: TokenStream, item: TokenStream) -> TokenStream {
    let input_fn = parse_macro_input!(item as ItemFn);
    let reducer_ident = &input_fn.sig.ident;

    let arg_count = input_fn.sig.inputs.len();
    if arg_count == 0 {
        return quote! {compile_error!("The reducer should have at least a 'ReducerContext' first argument");}.into();
    }
    let first_arg_ty = match input_fn.sig.inputs.first() {
        Some(syn::FnArg::Typed(pat)) => pat.ty.to_token_stream().to_string(),
        _ => String::new(),
    };
    if first_arg_ty != "ReducerContext" {
        return quote! {compile_error!("The reducer should have the first argument of type 'ReducerContext'");}.into();
    }

    let returns_unit = match &input_fn.sig.output {
        syn::ReturnType::Default => true,
        syn::ReturnType::Type(_, ty) => {
            matches!(ty.as_ref(), syn::Type::Tuple(t) if t.elems.is_empty())
        }
    };
    if !returns_unit {
        return quote! {compile_error!("Reducers must not return a value. Use #[query] for read-only return values.");}.into();
    }

    // Add potential subscription
    let attributes = syn::parse_macro_input!(
        attr with syn::punctuated::Punctuated::<Meta, syn::Token![,]>::parse_terminated
    );
    let reads = match parse_table_access_list(&attributes, "reads") {
        Ok(v) => v,
        Err(err) => return err.into_compile_error().into(),
    };
    let inserts = match parse_table_access_list(&attributes, "inserts") {
        Ok(v) => v,
        Err(err) => return err.into_compile_error().into(),
    };
    let updates = match parse_table_access_list(&attributes, "updates") {
        Ok(v) => v,
        Err(err) => return err.into_compile_error().into(),
    };
    let deletes = match parse_table_access_list(&attributes, "deletes") {
        Ok(v) => v,
        Err(err) => return err.into_compile_error().into(),
    };
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

    // Wrapper function
    let wrapper_function =
        get_wrapper_function(reducer_ident.clone(), arg_count, use_table_subscription);

    // Schema function
    let register_schema = get_register_schema_function(
        reducer_ident.clone(),
        arg_names,
        arg_types,
        reads,
        inserts,
        updates,
        deletes,
    );

    // Generate wrapper and schema
    quote! {
        #input_fn

        #wrapper_function

        #register_schema

        #register_subscription
    }
    .into()
}

fn parse_table_access_list(
    attributes: &syn::punctuated::Punctuated<Meta, syn::Token![,]>,
    field_name: &str,
) -> syn::Result<Vec<String>> {
    let mut values = Vec::new();
    for meta in attributes {
        let Meta::NameValue(nv) = meta else {
            continue;
        };
        if !nv.path.is_ident(field_name) {
            continue;
        }
        let Expr::Array(arr) = &nv.value else {
            return Err(syn::Error::new_spanned(
                &nv.value,
                format!("Expected {} = [..] array", field_name),
            ));
        };
        for expr in &arr.elems {
            match expr {
                Expr::Path(path) => {
                    let Some(seg) = path.path.segments.last() else {
                        return Err(syn::Error::new_spanned(path, "Expected table identifier"));
                    };
                    values.push(seg.ident.to_string().to_lowercase());
                }
                Expr::Lit(expr_lit) => {
                    if let syn::Lit::Str(s) = &expr_lit.lit {
                        values.push(s.value().to_lowercase());
                    } else {
                        return Err(syn::Error::new_spanned(
                            expr_lit,
                            "Expected table identifier or string literal",
                        ));
                    }
                }
                other => {
                    return Err(syn::Error::new_spanned(
                        other,
                        "Expected table identifier or string literal",
                    ));
                }
            }
        }
    }
    Ok(values)
}
