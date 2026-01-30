use quote::quote;
use syn::{DeriveInput, Fields, Ident, Variant};

use crate::abi_path;

pub fn derive_interstice_type_macro(input: DeriveInput) -> proc_macro2::TokenStream {
    let struct_name = input.ident.clone();

    return match input.data.clone() {
        syn::Data::Struct(s) => derive_interstice_type_macro_struct(struct_name, s.fields),
        syn::Data::Enum(s) => {
            let variants: Vec<Variant> = s.variants.into_iter().map(|f| f).collect();
            derive_interstice_type_macro_enum(struct_name, variants)
        }
        _ => syn::Error::new_spanned(
            struct_name,
            "IntersticeType can only be derived for struct or enum",
        )
        .to_compile_error()
        .into(),
    };
}

fn derive_interstice_type_macro_struct(
    struct_name: Ident,
    fields: Fields,
) -> proc_macro2::TokenStream {
    let abi = abi_path();

    let struct_name_str = struct_name.to_string();

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

    let field_name_strings: Vec<String> = field_names.iter().map(|f| f.to_string()).collect();

    quote! {
        impl Into<#abi::IntersticeValue> for #struct_name {
            fn into(self) -> #abi::IntersticeValue {
                #abi::IntersticeValue::Struct {
                    name: #struct_name_str.to_string(),
                    fields: vec![
                        #(
                            #abi::Field {
                                name: #field_name_strings.to_string(),
                                value: self.#field_names.into(),
                            }
                        ),*
                    ],
                }
            }
        }

        impl Into<#struct_name> for #abi::IntersticeValue {
            fn into(self) -> #struct_name {
                match self {
                    #abi::IntersticeValue::Struct { name, fields } if name == #struct_name_str => {
                        let mut map = std::collections::HashMap::new();
                        for field in fields {
                            map.insert(field.name, field.value);
                        }

                        #struct_name {
                            #(
                                #field_names: map.remove(#field_name_strings).unwrap().into(),
                            )*
                        }
                    }
                    _ => panic!("Expected struct {}", #struct_name_str),
                }
            }
        }
    }
}

fn derive_interstice_type_macro_enum(
    enum_name: Ident,
    variants: Vec<Variant>,
) -> proc_macro2::TokenStream {
    let abi = abi_path();

    let enum_name_str = enum_name.to_string();

    let match_arms = variants.clone().into_iter().map(|variant| {
        let variant_ident = variant.ident;
        let variant_name_str = variant_ident.to_string();

        match variant.fields {
            // ---------------- UNIT ----------------
            Fields::Unit => {
                quote! {
                    #enum_name::#variant_ident => #abi::IntersticeValue::Enum {
                        name: #enum_name_str.to_string(),
                        variant: #variant_name_str.to_string(),
                        value: Box::new(#abi::IntersticeValue::Void),
                    }
                }
            }

            // ---------------- TUPLE VARIANT ----------------
            Fields::Unnamed(fields) => {
                let bindings: Vec<_> = (0..fields.unnamed.len())
                    .map(|i| syn::Ident::new(&format!("v{i}"), variant_ident.span()))
                    .collect();

                if bindings.len() == 1 {
                    let v0 = &bindings[0];
                    quote! {
                        #enum_name::#variant_ident(#v0) => #abi::IntersticeValue::Enum {
                            name: #enum_name_str.to_string(),
                            variant: #variant_name_str.to_string(),
                            value: Box::new(#v0.into()),
                        }
                    }
                } else {
                    quote! {
                        #enum_name::#variant_ident(#(#bindings),*) => #abi::IntersticeValue::Enum {
                            name: #enum_name_str.to_string(),
                            variant: #variant_name_str.to_string(),
                            value: Box::new(#abi::IntersticeValue::Tuple(vec![
                                #(#bindings.into()),*
                            ])),
                        }
                    }
                }
            }

            // ---------------- STRUCT VARIANT ----------------
            Fields::Named(fields) => {
                let field_idents: Vec<_> = fields
                    .named
                    .iter()
                    .map(|f| f.ident.clone().unwrap())
                    .collect();

                let field_names: Vec<String> =
                    field_idents.iter().map(|id| id.to_string()).collect();

                quote! {
                    #enum_name::#variant_ident { #(#field_idents),* } => #abi::IntersticeValue::Enum {
                        name: #enum_name_str.to_string(),
                        variant: #variant_name_str.to_string(),
                        value: Box::new(#abi::IntersticeValue::Struct {
                            name: #variant_name_str.to_string(),
                            fields: vec![
                                #(
                                    #abi::Field {
                                        name: #field_names.to_string(),
                                        value: #field_idents.into(),
                                    }
                                ),*
                            ],
                        }),
                    }
                }
            }
        }
    });

    let reverse_match_arms = variants.iter().map(|variant| {
        let variant_ident = &variant.ident;
        let variant_name_str = variant_ident.to_string();

        match &variant.fields {
            // -------- UNIT --------
            Fields::Unit => {
                quote! {
                    #variant_name_str => {
                        match *value {
                            #abi::IntersticeValue::Void => Ok(#enum_name::#variant_ident),
                            other => Err(format!("Expected Void for variant {}::{}, got {:?}", #enum_name_str, #variant_name_str, other)),
                        }
                    }
                }
            }

            // -------- TUPLE --------
            Fields::Unnamed(fields) => {
                let arity = fields.unnamed.len();

                if arity == 1 {
                    let ty = &fields.unnamed.first().unwrap().ty;
                    quote! {
                        #variant_name_str => {
                            let inner: #ty = (*value).try_into()
                                .map_err(|_| format!("Failed to convert payload for {}::{}", #enum_name_str, #variant_name_str))?;
                            Ok(#enum_name::#variant_ident(inner))
                        }
                    }
                } else {
                    let indices: Vec<syn::Index> = (0..arity).map(|i| syn::Index::from(i)).collect();
                    let types: Vec<_> = fields.unnamed.iter().map(|f| &f.ty).collect();

                    quote! {
                        #variant_name_str => {
                            match *value {
                                #abi::IntersticeValue::Tuple(vec) => {
                                    if vec.len() != #arity {
                                        return Err(format!("Wrong tuple arity for {}::{}", #enum_name_str, #variant_name_str));
                                    }
                                    Ok(#enum_name::#variant_ident(
                                        #(
                                            <#types as TryFrom<#abi::IntersticeValue>>::try_from(vec[#indices].clone())
                                                .map_err(|_| format!("Failed to convert tuple element for {}::{}", #enum_name_str, #variant_name_str))?
                                        ),*
                                    ))
                                }
                                other => Err(format!("Expected Tuple for {}::{}, got {:?}", #enum_name_str, #variant_name_str, other)),
                            }
                        }
                    }
                }
            }

            // -------- STRUCT --------
            Fields::Named(fields) => {
                let field_idents: Vec<_> = fields.named.iter().map(|f| f.ident.clone().unwrap()).collect();
                let field_names: Vec<String> = field_idents.iter().map(|id| id.to_string()).collect();
                let field_types: Vec<_> = fields.named.iter().map(|f| &f.ty).collect();

                quote! {
                    #variant_name_str => {
                        match *value {
                            #abi::IntersticeValue::Struct { name: struct_name, fields } => {
                                if struct_name != #variant_name_str {
                                    return Err(format!("Struct name mismatch for {}::{}", #enum_name_str, #variant_name_str));
                                }
                                let mut map = std::collections::HashMap::new();
                                for field in fields {
                                    map.insert(field.name, field.value);
                                }

                                Ok(#enum_name::#variant_ident {
                                    #(
                                        #field_idents: <#field_types as TryFrom<#abi::IntersticeValue>>::try_from(
                                            map.remove(#field_names)
                                                .ok_or_else(|| format!("Missing field {} in {}::{}", #field_names, #enum_name_str, #variant_name_str))?
                                        ).map_err(|_| format!("Failed to convert field {} in {}::{}", #field_names, #enum_name_str, #variant_name_str))?
                                    ),*
                                })
                            }
                            other => Err(format!("Expected Struct for {}::{}, got {:?}", #enum_name_str, #variant_name_str, other)),
                        }
                    }
                }
            }
        }
    });

    quote! {
        impl Into<#abi::IntersticeValue> for #enum_name {
            fn into(self) -> #abi::IntersticeValue {
                match self {
                    #(#match_arms,)*
                }
            }
        }

        impl TryFrom<#abi::IntersticeValue> for #enum_name {
            type Error = String;

            fn try_from(value: #abi::IntersticeValue) -> Result<Self, Self::Error> {
                match value {
                    #abi::IntersticeValue::Enum { name, variant, value } if name == #enum_name_str => {
                        match variant.as_str() {
                            #(#reverse_match_arms,)*
                            _ => Err(format!("Unknown variant '{}' for enum {}", variant, #enum_name_str)),
                        }
                    }
                    _ => Err(format!("Expected IntersticeValue::Enum for {}", #enum_name_str)),
                }
            }
        }

    }
}
