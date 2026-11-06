use quote::quote;
use syn::{Ident, Pat, Type};

pub fn get_register_schema_function(
    reducer_ident: Ident,
    arg_names: Vec<&Box<Pat>>,
    arg_types: Vec<&Box<Type>>,
    caps_extend_body: proc_macro2::TokenStream,
    visibility: proc_macro2::TokenStream,
) -> proc_macro2::TokenStream {
    let reducer_schema_fn = syn::Ident::new(
        &format!("interstice_{}_schema", reducer_ident),
        reducer_ident.span(),
    );
    let register_reducer_schema_fn = syn::Ident::new(
        &format!("interstice_register_{}_schema", reducer_ident),
        reducer_ident.span(),
    );

    let schema_entries = arg_names.iter().skip(1).zip(arg_types.iter().skip(1)).map(
        |(arg_name, arg_type)| {
            // Use the bare identifier: `mut x` / `ref x` patterns must not leak
            // their binding modifiers into the schema (and generated bindings).
            let arg_name_str = match &***arg_name {
                Pat::Ident(pat_ident) => pat_ident.ident.to_string(),
                other => quote! { #other }.to_string(),
            };
            let field_type = crate::types::type_expr(arg_type).unwrap_or_else(|e| e.to_compile_error());
            quote! {
                interstice_sdk::FieldDef {
                    name: #arg_name_str.to_string(),
                    field_type: #field_type,
                }
            }
        },
    );

    quote! {
        fn #reducer_schema_fn() -> interstice_sdk::ReducerSchema {
            let mut reads = Vec::new();
            let mut inserts = Vec::new();
            let mut updates = Vec::new();
            let mut deletes = Vec::new();
            #caps_extend_body
            let mut schema = interstice_sdk::ReducerSchema::new(
                stringify!(#reducer_ident),
                vec![#(#schema_entries),*],
                reads,
                inserts,
                updates,
                deletes,
            );
            schema.visibility = #visibility;
            schema
        }

        #[interstice_sdk::init]
        fn #register_reducer_schema_fn() {
            interstice_sdk::registry::register_reducer(#reducer_schema_fn);
        }
    }
}
