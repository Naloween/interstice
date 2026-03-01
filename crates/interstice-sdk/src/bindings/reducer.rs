use interstice_abi::{NodeSelection, ReducerSchema};
use proc_macro2::{Span, TokenStream};
use quote::quote;
use syn::{Ident, LitStr, Type};

pub fn get_reducer_code(
    module_name: &String,
    reducer_schema: ReducerSchema,
    node_selection: &NodeSelection,
) -> TokenStream {
    let span = Span::call_site();
    let method_name = Ident::new(&reducer_schema.name, span);
    let module_name_lit = LitStr::new(module_name, span);
    let reducer_name_lit = LitStr::new(&reducer_schema.name, span);

    let argument_defs: Vec<TokenStream> = reducer_schema
        .arguments
        .iter()
        .map(|arg| {
            let arg_ident = Ident::new(&arg.name, span);
            let arg_type: Type = syn::parse_str(&arg.field_type.to_string())
                .expect("Failed to parse reducer argument type");
            quote! { #arg_ident: #arg_type }
        })
        .collect();

    let argument_values: Vec<TokenStream> = reducer_schema
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
        pub fn #method_name(&self, #(#argument_defs),*) -> Result<(), String> {
            interstice_sdk::host_calls::call_reducer(
                #node_selection_tokens,
                interstice_sdk::ModuleSelection::Other(#module_name_lit.to_string()),
                #reducer_name_lit.to_string(),
                interstice_sdk::IntersticeValue::Vec(vec![#(#argument_values),*]),
            )
        }
    }
}
