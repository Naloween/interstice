use interstice_abi::{NodeSelection, QuerySchema};
use proc_macro2::{Span, TokenStream};
use quote::quote;
use syn::{Ident, LitStr, Type};

pub fn get_query_code(
    module_name: &String,
    query_schema: QuerySchema,
    node_selection: &NodeSelection,
) -> TokenStream {
    let span = Span::call_site();
    let method_name = Ident::new(&query_schema.name, span);
    let module_name_lit = LitStr::new(module_name, span);
    let query_name_lit = LitStr::new(&query_schema.name, span);
    let return_type: Type = syn::parse_str(&query_schema.return_type.to_string())
        .expect("Failed to parse query return type");

    let argument_defs: Vec<TokenStream> = query_schema
        .arguments
        .iter()
        .map(|arg| {
            let arg_ident = Ident::new(&arg.name, span);
            let arg_type: Type = syn::parse_str(&arg.field_type.to_string())
                .expect("Failed to parse query argument type");
            quote! { #arg_ident: #arg_type }
        })
        .collect();

    let argument_values: Vec<TokenStream> = query_schema
        .arguments
        .iter()
        .map(|arg| {
            let arg_ident = Ident::new(&arg.name, span);
            quote! { #arg_ident.into() }
        })
        .collect();

    let node_selection_tokens = match node_selection {
        NodeSelection::Current => quote! { interstice_sdk::NodeSelection::Current },
        NodeSelection::Other(node_name) => {
            let node_name_lit = LitStr::new(node_name, span);
            quote! { interstice_sdk::NodeSelection::Other(#node_name_lit.to_string()) }
        }
    };

    quote! {
        pub fn #method_name(&self, #(#argument_defs),*) -> Result<#return_type, String> {
            let res = interstice_sdk::host_calls::call_query(
                #node_selection_tokens,
                interstice_sdk::ModuleSelection::Other(#module_name_lit.to_string()),
                #query_name_lit.to_string(),
                interstice_sdk::IntersticeValue::Vec(vec![#(#argument_values),*]),
            )?;
            Ok(res.try_into().unwrap())
        }
    }
}
