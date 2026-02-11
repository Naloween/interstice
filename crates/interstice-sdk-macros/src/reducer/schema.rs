use quote::quote;
use syn::{Ident, Pat, Type};

pub fn get_register_schema_function(
    reducer_ident: Ident,
    arg_names: Vec<&Box<Pat>>,
    arg_types: Vec<&Box<Type>>,
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
            let arg_name_str = quote! { #arg_name }.to_string();
            let arg_type_str = quote! { #arg_type }.to_string();
            quote! {
                interstice_sdk::FieldDef {
                    name: #arg_name_str.to_string(),
                    field_type: interstice_sdk::IntersticeType::from_str(#arg_type_str).unwrap(),
                }
            }
        },
    );

    quote! {
        fn #reducer_schema_fn() -> interstice_sdk::ReducerSchema {
            interstice_sdk::ReducerSchema::new(
                stringify!(#reducer_ident),
                vec![#(#schema_entries),*],
            )
        }

        #[interstice_sdk::init]
        fn #register_reducer_schema_fn() {
            interstice_sdk::registry::register_reducer(#reducer_schema_fn);
        }
    }
}
