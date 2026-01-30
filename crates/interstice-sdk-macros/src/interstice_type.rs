use proc_macro::TokenStream;
use quote::{quote, ToTokens};
use syn::parse_macro_input;

pub fn derive_interstice_type_macro(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as syn::DeriveInput);

    let struct_name = input.ident.clone();
    let struct_name_str = struct_name.to_string();
    let type_def_fn = syn::Ident::new(
        &format!("get_type_def_{}", &struct_name_str.to_lowercase()),
        struct_name.span(),
    );
    let register_type_def_fn = syn::Ident::new(
        &format!("register_get_type_def_{}", &struct_name_str.to_lowercase()),
        struct_name.span(),
    );

    let fields = match input.data.clone() {
        syn::Data::Struct(s) => s.fields,
        _ => panic!("IntersticeType can only be derived for structs"),
    };

    let named_fields = match fields {
        syn::Fields::Named(f) => f.named,
        _ => panic!("IntersticeType requires named fields"),
    };

    let mut field_names = Vec::new();
    let mut field_types = Vec::new();

    for field in named_fields {
        let ident = field.ident.unwrap();
        let ty = field.ty;

        field_names.push(ident);
        field_types.push(ty);
    }

    // Convert Rust type → IntersticeType expression
    let interstice_type_exprs = field_types
        .iter()
        .map(|ty| ty.to_token_stream().to_string())
        .collect();

    let field_name_strings: Vec<String> = field_names.iter().map(|f| f.to_string()).collect();

    let type_def = get_struct_type_def(struct_name_str, field_name_strings, interstice_type_exprs);

    quote! {
        #[derive(interstice_sdk::interstice_abi_macros::IntersticeType)]
        #input

        fn #type_def_fn() -> interstice_sdk::IntersticeTypeDef {
            #type_def
        }

        #[interstice_sdk::init]
        fn #register_type_def_fn() {
            interstice_sdk::registry::register_type_def(#type_def_fn);
        }
    }
    .into()
}

fn get_struct_type_def(
    struct_name_str: String,
    field_name_strings: Vec<String>,
    interstice_type_exprs: Vec<String>,
) -> proc_macro2::TokenStream {
    quote! {
        interstice_sdk::IntersticeTypeDef::Struct {
            name: #struct_name_str.to_string(),
            fields: vec![
                #(
                    FieldDef {
                        name: #field_name_strings.to_string(),
                        field_type: interstice_sdk::IntersticeType::from_str(#interstice_type_exprs).expect("Couldn't convert field type string to IntersticeType"),
                    }
                ),*
            ],
        }
    }
}
