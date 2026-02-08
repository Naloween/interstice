mod schema;
mod wrapper;

use proc_macro::TokenStream;
use quote::{quote, ToTokens};
use syn::{parse_macro_input, ItemFn};

use crate::query::{schema::get_register_schema_function, wrapper::get_wrapper_function};

pub fn query_macro(item: TokenStream) -> TokenStream {
    let input_fn = parse_macro_input!(item as ItemFn);
    let query_ident = &input_fn.sig.ident;

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
        return quote! {compile_error!("The query should have at least a 'QueryContext' first argument");}.into();
    }
    if arg_types[0].to_token_stream().to_string() != "QueryContext" {
        return quote! {compile_error!("The query should have the first argument of type 'QueryContext'");}.into();
    }

    // Wrapper function
    let wrapper_function = get_wrapper_function(query_ident.clone(), arg_count);

    // Schema function
    let register_schema = get_register_schema_function(
        query_ident.clone(),
        input_fn.clone(),
        arg_names,
        arg_types,
    );

    quote! {
        #input_fn

        #wrapper_function

        #register_schema
    }
    .into()
}
