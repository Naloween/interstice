use interstice_abi::IntersticeTypeDef;
use proc_macro2::Span;
use quote::quote;
use syn::{Ident, Type};

pub fn get_type_definition_code(type_def: &IntersticeTypeDef) -> String {
    let span = Span::call_site();
    let tokens = match type_def {
        IntersticeTypeDef::Struct { name, fields } => {
            let name_ident = Ident::new(name, span);
            let field_defs = fields.iter().map(|field| {
                let field_ident = Ident::new(&field.name, span);
                let field_type: Type = syn::parse_str(&field.field_type.to_string())
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
                    interstice_abi::IntersticeType::Tuple(interstice_types) => {
                        let tuple_types = interstice_types.iter().map(|t| {
                            syn::parse_str::<Type>(&t.to_string())
                                .expect("Failed to parse tuple variant type")
                        });
                        quote! { #variant_ident(#(#tuple_types),*) }
                    }
                    field_type => {
                        let inner_type: Type = syn::parse_str(&field_type.to_string())
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

    tokens.to_string()
}
