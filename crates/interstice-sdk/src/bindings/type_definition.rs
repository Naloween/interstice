use interstice_abi::IntersticeTypeDef;
use proc_macro2::Span;
use quote::quote;
use syn::{Ident, Type};

/// The Rust item for `type_def`. `defs` are the module's type definitions:
/// a named-field enum variant's payload is the struct `Enum::Variant` there,
/// which becomes the variant's fields rather than an item of its own.
pub fn get_type_definition_code(
    type_def: &IntersticeTypeDef,
    defs: &std::collections::HashMap<String, IntersticeTypeDef>,
) -> proc_macro2::TokenStream {
    let span = Span::call_site();
    let tokens = match type_def {
        IntersticeTypeDef::Struct { name, .. } if name.contains("::") => quote! {},
        IntersticeTypeDef::Struct { name, fields } => {
            let name_ident = Ident::new(name, span);
            let field_defs = fields.iter().map(|field| {
                let field_ident = Ident::new(&field.name, span);
                let field_type: Type = syn::parse_str(&super::rust_type_string(&field.field_type))
                    .expect("Failed to parse struct field type");
                quote! { pub #field_ident: #field_type }
            });

            quote! {
                #[derive(interstice_sdk::interstice_abi_macros::IntersticeType)]
                pub struct #name_ident {
                    #(#field_defs,)*
                }
            }
        }
        IntersticeTypeDef::Enum { name, variants } => {
            let name_ident = Ident::new(name, span);
            let variant_defs = variants.iter().map(|variant| {
                let variant_ident = Ident::new(&variant.name, span);
                match &variant.field_type {
                    interstice_abi::IntersticeType::Void => quote! { #variant_ident },
                    interstice_abi::IntersticeType::Named(payload) if payload.contains("::") => {
                        let Some(IntersticeTypeDef::Struct { fields, .. }) = defs.get(payload) else {
                            panic!("bindings: no definition for enum variant payload {payload}");
                        };
                        let fields = fields.iter().map(|field| {
                            let field_ident = Ident::new(&field.name, span);
                            let field_type: Type = syn::parse_str(&super::rust_type_string(&field.field_type))
                                .expect("Failed to parse variant field type");
                            quote! { #field_ident: #field_type }
                        });
                        quote! { #variant_ident { #(#fields),* } }
                    }
                    interstice_abi::IntersticeType::Tuple(interstice_types) => {
                        let tuple_types = interstice_types.iter().map(|t| {
                            syn::parse_str::<Type>(&super::rust_type_string(t))
                                .expect("Failed to parse tuple variant type")
                        });
                        quote! { #variant_ident(#(#tuple_types),*) }
                    }
                    field_type => {
                        let inner_type: Type = syn::parse_str(&super::rust_type_string(field_type))
                            .expect("Failed to parse enum variant type");
                        quote! { #variant_ident(#inner_type) }
                    }
                }
            });

            quote! {
                #[derive(interstice_sdk::interstice_abi_macros::IntersticeType)]
                pub enum #name_ident {
                    #(#variant_defs,)*
                }
            }
        }
    };

    tokens
}
