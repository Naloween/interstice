//! `#[derive(IntersticeType)]`: conversions between a Rust struct or enum
//! and its positional `IntersticeValue` (DESIGN §10.2): a struct is its
//! fields in declaration order, an enum its variant index and payload.

use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::{DeriveInput, Fields};

use crate::abi_path;

pub fn derive_interstice_type_macro(input: DeriveInput) -> TokenStream {
    let name = input.ident.clone();
    match &input.data {
        syn::Data::Struct(s) => match &s.fields {
            Fields::Named(_) => derive_struct(&name, &s.fields),
            _ => syn::Error::new_spanned(&name, "IntersticeType structs need named fields").to_compile_error(),
        },
        syn::Data::Enum(e) => derive_enum(&name, e.variants.iter().collect()),
        _ => syn::Error::new_spanned(&name, "IntersticeType can only be derived for a struct or an enum").to_compile_error(),
    }
}

/// `let a = ..; let b = ..;` from the values of `fields` (an iterator
/// named `items`), with errors that name the field.
fn take_fields(context: &str, idents: &[syn::Ident], types: &[&syn::Type]) -> TokenStream {
    let abi = abi_path();
    let names: Vec<String> = idents.iter().map(|i| i.to_string()).collect();
    quote! {
        #(
            let #idents = <#types as TryFrom<#abi::IntersticeValue>>::try_from(items.next().unwrap())
                .map_err(|e| #abi::IntersticeAbiError::ConversionError(format!("{}.{}: {}", #context, #names, e)))?;
        )*
    }
}

fn derive_struct(name: &syn::Ident, fields: &Fields) -> TokenStream {
    let abi = abi_path();
    let idents: Vec<syn::Ident> = fields.iter().map(|f| f.ident.clone().unwrap()).collect();
    let types: Vec<&syn::Type> = fields.iter().map(|f| &f.ty).collect();
    let count = idents.len();
    let context = name.to_string();
    let take = take_fields(&context, &idents, &types);
    quote! {
        impl From<#name> for #abi::IntersticeValue {
            fn from(value: #name) -> Self {
                #abi::IntersticeValue::Struct(vec![#(value.#idents.into()),*])
            }
        }

        impl TryFrom<#abi::IntersticeValue> for #name {
            type Error = #abi::IntersticeAbiError;

            fn try_from(value: #abi::IntersticeValue) -> Result<Self, Self::Error> {
                match value {
                    #abi::IntersticeValue::Struct(fields) if fields.len() == #count => {
                        #[allow(unused_mut, unused_variables)]
                        let mut items = fields.into_iter();
                        #take
                        Ok(Self { #(#idents),* })
                    }
                    other => Err(#abi::IntersticeAbiError::ConversionError(format!(
                        "expected {} ({} fields), got {}", #context, #count, other
                    ))),
                }
            }
        }
    }
}

fn derive_enum(name: &syn::Ident, variants: Vec<&syn::Variant>) -> TokenStream {
    let abi = abi_path();
    let mut to_value = Vec::new();
    let mut from_value = Vec::new();
    for (index, variant) in variants.iter().enumerate() {
        let index = index as u32;
        let v = &variant.ident;
        let context = format!("{name}::{v}");
        let wrong = quote! {
            other => Err(#abi::IntersticeAbiError::ConversionError(format!("{}: unexpected payload {}", #context, other)))
        };
        match &variant.fields {
            Fields::Unit => {
                to_value.push(quote! {
                    #name::#v => #abi::IntersticeValue::Enum { variant: #index, value: Box::new(#abi::IntersticeValue::Void) }
                });
                from_value.push(quote! {
                    #index => match *value {
                        #abi::IntersticeValue::Void => Ok(#name::#v),
                        #wrong,
                    }
                });
            }
            Fields::Unnamed(fields) if fields.unnamed.len() == 1 => {
                let ty = &fields.unnamed[0].ty;
                to_value.push(quote! {
                    #name::#v(x) => #abi::IntersticeValue::Enum { variant: #index, value: Box::new(x.into()) }
                });
                from_value.push(quote! {
                    #index => <#ty as TryFrom<#abi::IntersticeValue>>::try_from(*value)
                        .map(#name::#v)
                        .map_err(|e| #abi::IntersticeAbiError::ConversionError(format!("{}: {}", #context, e)))
                });
            }
            Fields::Unnamed(fields) => {
                let idents: Vec<syn::Ident> = (0..fields.unnamed.len()).map(|i| format_ident!("v{i}")).collect();
                let types: Vec<&syn::Type> = fields.unnamed.iter().map(|f| &f.ty).collect();
                let count = idents.len();
                let take = take_fields(&context, &idents, &types);
                to_value.push(quote! {
                    #name::#v(#(#idents),*) => #abi::IntersticeValue::Enum {
                        variant: #index,
                        value: Box::new(#abi::IntersticeValue::Tuple(vec![#(#idents.into()),*])),
                    }
                });
                from_value.push(quote! {
                    #index => match *value {
                        #abi::IntersticeValue::Tuple(fields) if fields.len() == #count => {
                            let mut items = fields.into_iter();
                            #take
                            Ok(#name::#v(#(#idents),*))
                        }
                        #wrong,
                    }
                });
            }
            Fields::Named(fields) => {
                // The payload is the struct `Enum::Variant` (DESIGN §10.1).
                let idents: Vec<syn::Ident> = fields.named.iter().map(|f| f.ident.clone().unwrap()).collect();
                let types: Vec<&syn::Type> = fields.named.iter().map(|f| &f.ty).collect();
                let count = idents.len();
                let take = take_fields(&context, &idents, &types);
                to_value.push(quote! {
                    #name::#v { #(#idents),* } => #abi::IntersticeValue::Enum {
                        variant: #index,
                        value: Box::new(#abi::IntersticeValue::Struct(vec![#(#idents.into()),*])),
                    }
                });
                from_value.push(quote! {
                    #index => match *value {
                        #abi::IntersticeValue::Struct(fields) if fields.len() == #count => {
                            #[allow(unused_mut)]
                            let mut items = fields.into_iter();
                            #take
                            Ok(#name::#v { #(#idents),* })
                        }
                        #wrong,
                    }
                });
            }
        }
    }
    let context = name.to_string();
    quote! {
        impl From<#name> for #abi::IntersticeValue {
            fn from(value: #name) -> Self {
                match value {
                    #(#to_value,)*
                }
            }
        }

        impl TryFrom<#abi::IntersticeValue> for #name {
            type Error = #abi::IntersticeAbiError;

            fn try_from(value: #abi::IntersticeValue) -> Result<Self, Self::Error> {
                let #abi::IntersticeValue::Enum { variant, value } = value else {
                    return Err(#abi::IntersticeAbiError::ConversionError(format!("expected {}, got {}", #context, value)));
                };
                match variant {
                    #(#from_value,)*
                    other => Err(#abi::IntersticeAbiError::ConversionError(format!("{} has no variant #{}", #context, other))),
                }
            }
        }
    }
}
