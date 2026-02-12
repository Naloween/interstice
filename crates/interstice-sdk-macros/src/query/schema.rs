use quote::{ToTokens, quote};
use syn::{Ident, ItemFn, Pat, Type};

pub fn get_register_schema_function(
    query_ident: Ident,
    input_fn: ItemFn,
    arg_names: Vec<&Box<Pat>>,
    arg_types: Vec<&Box<Type>>,
) -> proc_macro2::TokenStream {
    let query_schema_fn = syn::Ident::new(
        &format!("interstice_{}_query_schema", query_ident),
        query_ident.span(),
    );
    let register_query_schema_fn = syn::Ident::new(
        &format!("interstice_register_{}_query_schema", query_ident),
        query_ident.span(),
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

    let return_type = match &input_fn.sig.output {
        syn::ReturnType::Default => quote! { interstice_sdk::IntersticeType::Void },
        syn::ReturnType::Type(_, ty) => {
            let ty = ty.to_token_stream().to_string();
            quote! {
               interstice_sdk::IntersticeType::from_str(#ty).unwrap()
            }
        }
    };

    quote! {
        fn #query_schema_fn() -> interstice_sdk::QuerySchema {
            interstice_sdk::QuerySchema::new(
                stringify!(#query_ident),
                vec![#(#schema_entries),*],
                #return_type,
            )
        }

        #[interstice_sdk::init]
        fn #register_query_schema_fn() {
            interstice_sdk::registry::register_query(#query_schema_fn);
        }
    }
}
