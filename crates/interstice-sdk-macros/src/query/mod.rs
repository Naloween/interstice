mod schema;
mod wrapper;

use proc_macro::TokenStream;
use quote::ToTokens;
use syn::{ItemFn, Meta, parse_macro_input};

use crate::query::{schema::get_register_schema_function, wrapper::get_wrapper_function};
use crate::table_access_tokens::parse_table_access_list;

pub fn query_macro(attr: TokenStream, item: TokenStream) -> TokenStream {
    let input_fn = parse_macro_input!(item as ItemFn);
    let query_ident = &input_fn.sig.ident;

    let attributes: syn::punctuated::Punctuated<Meta, syn::Token![,]> = if attr.is_empty() {
        syn::punctuated::Punctuated::new()
    } else {
        parse_macro_input!(attr with syn::punctuated::Punctuated::<Meta, syn::Token![,]>::parse_terminated)
    };

    let reads = match parse_table_access_list(&attributes, "reads") {
        Ok(v) => v,
        Err(err) => return err.into_compile_error().into(),
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

    let arg_count = arg_names.len();
    if arg_count == 0 {
        return quote::quote! {compile_error!("The query should have at least a 'QueryContext' first argument");}.into();
    }
    if arg_types[0].to_token_stream().to_string() != "QueryContext" {
        return quote::quote! {compile_error!("The query should have the first argument of type 'QueryContext'");}.into();
    }

    let wrapper_function = get_wrapper_function(query_ident.clone(), arg_count);
    let register_schema = get_register_schema_function(
        query_ident.clone(),
        input_fn.clone(),
        arg_names,
        arg_types,
        reads,
    );

    quote::quote! {
        #input_fn

        #wrapper_function

        #register_schema
    }
    .into()
}
