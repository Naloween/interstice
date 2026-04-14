use proc_macro::TokenStream;
use quote::{ToTokens, quote};
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

    let type_def = match input.data.clone() {
        syn::Data::Struct(s) => {
            let named_fields = match s.fields {
                syn::Fields::Named(f) => f.named,
                _ => {
                    return syn::Error::new_spanned(
                        input.ident,
                        "interstice_type on structs requires named fields",
                    )
                    .to_compile_error()
                    .into();
                }
            };
            let mut field_names = Vec::new();
            let mut field_types = Vec::new();
            for field in named_fields {
                let ident = match field.ident {
                    Some(id) => id,
                    None => {
                        return syn::Error::new_spanned(
                            input.ident,
                            "interstice_type struct field must be named",
                        )
                        .to_compile_error()
                        .into();
                    }
                };
                field_names.push(ident.to_string());
                field_types.push(field.ty.to_token_stream().to_string());
            }
            get_struct_type_def(struct_name_str, field_names, field_types)
        }
        syn::Data::Enum(en) => {
            let mut variants = Vec::new();
            for variant in en.variants {
                let variant_name = variant.ident.to_string();
                let variant_ty = match variant.fields {
                    syn::Fields::Unit => "()".to_string(),
                    syn::Fields::Unnamed(fields) => {
                        if fields.unnamed.len() == 1 {
                            fields
                                .unnamed
                                .first()
                                .expect("single unnamed field must exist")
                                .ty
                                .to_token_stream()
                                .to_string()
                        } else {
                            let parts: Vec<String> = fields
                                .unnamed
                                .iter()
                                .map(|f| f.ty.to_token_stream().to_string())
                                .collect();
                            format!("({})", parts.join(", "))
                        }
                    }
                    syn::Fields::Named(fields) => {
                        // IntersticeTypeDef stores one IntersticeType per variant.
                        // Represent named variants as tuple payloads to preserve field order/types.
                        let parts: Vec<String> = fields
                            .named
                            .iter()
                            .map(|f| f.ty.to_token_stream().to_string())
                            .collect();
                        format!("({})", parts.join(", "))
                    }
                };
                variants.push((variant_name, variant_ty));
            }
            get_enum_type_def(struct_name_str, variants)
        }
        _ => {
            return syn::Error::new_spanned(
                input.ident,
                "interstice_type can only be used on structs or enums",
            )
            .to_compile_error()
            .into();
        }
    };

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

fn get_enum_type_def(
    enum_name_str: String,
    variants: Vec<(String, String)>,
) -> proc_macro2::TokenStream {
    let variant_names: Vec<String> = variants.iter().map(|(name, _)| name.clone()).collect();
    let variant_types: Vec<String> = variants.iter().map(|(_, ty)| ty.clone()).collect();
    quote! {
        interstice_sdk::IntersticeTypeDef::Enum {
            name: #enum_name_str.to_string(),
            variants: vec![
                #(
                    FieldDef {
                        name: #variant_names.to_string(),
                        field_type: interstice_sdk::IntersticeType::from_str(#variant_types).expect("Couldn't convert enum variant type string to IntersticeType"),
                    }
                ),*
            ],
        }
    }
}
