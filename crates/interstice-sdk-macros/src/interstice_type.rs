use proc_macro::TokenStream;
use quote::quote;
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

    // A named-field enum variant's payload is its own struct type,
    // `Enum::Variant`, so its field names survive into the schema (§10.1).
    let mut variant_structs: Vec<proc_macro2::TokenStream> = Vec::new();
    let fields_of = |fields: &syn::FieldsNamed| -> Result<proc_macro2::TokenStream, syn::Error> {
        let mut defs = Vec::new();
        for field in &fields.named {
            let name = field.ident.as_ref().expect("named field").to_string();
            let ty = crate::types::type_expr(&field.ty)?;
            defs.push(quote! { interstice_sdk::FieldDef { name: #name.to_string(), field_type: #ty } });
        }
        Ok(quote! { vec![#(#defs),*] })
    };

    let type_def = match input.data.clone() {
        syn::Data::Struct(s) => {
            let syn::Fields::Named(named) = &s.fields else {
                return syn::Error::new_spanned(input.ident, "interstice_type on structs requires named fields")
                    .to_compile_error()
                    .into();
            };
            let fields = match fields_of(named) {
                Ok(f) => f,
                Err(e) => return e.to_compile_error().into(),
            };
            quote! {
                interstice_sdk::IntersticeTypeDef::Struct { name: #struct_name_str.to_string(), fields: #fields }
            }
        }
        syn::Data::Enum(en) => {
            let mut variants = Vec::new();
            for variant in &en.variants {
                let variant_name = variant.ident.to_string();
                let ty = match &variant.fields {
                    syn::Fields::Unit => Ok(quote!(interstice_sdk::IntersticeType::Void)),
                    syn::Fields::Unnamed(fields) if fields.unnamed.len() == 1 => {
                        crate::types::type_expr(&fields.unnamed[0].ty)
                    }
                    syn::Fields::Unnamed(fields) => fields
                        .unnamed
                        .iter()
                        .map(|f| crate::types::type_expr(&f.ty))
                        .collect::<syn::Result<Vec<_>>>()
                        .map(|items| quote!(interstice_sdk::IntersticeType::Tuple(vec![#(#items),*]))),
                    syn::Fields::Named(fields) => {
                        let payload = format!("{struct_name_str}::{variant_name}");
                        fields_of(fields).map(|fields| {
                            variant_structs.push(quote! {
                                interstice_sdk::IntersticeTypeDef::Struct { name: #payload.to_string(), fields: #fields }
                            });
                            quote!(interstice_sdk::IntersticeType::Named(#payload.to_string()))
                        })
                    }
                };
                let ty = match ty {
                    Ok(ty) => ty,
                    Err(e) => return e.to_compile_error().into(),
                };
                variants.push(quote! { interstice_sdk::FieldDef { name: #variant_name.to_string(), field_type: #ty } });
            }
            quote! {
                interstice_sdk::IntersticeTypeDef::Enum { name: #struct_name_str.to_string(), variants: vec![#(#variants),*] }
            }
        }
        _ => {
            return syn::Error::new_spanned(input.ident, "interstice_type can only be used on structs or enums")
                .to_compile_error()
                .into();
        }
    };
    let register_variant_structs = variant_structs.iter().enumerate().map(|(i, def)| {
        let get = syn::Ident::new(&format!("{type_def_fn}_variant_{i}"), struct_name.span());
        let register = syn::Ident::new(&format!("{register_type_def_fn}_variant_{i}"), struct_name.span());
        quote! {
            fn #get() -> interstice_sdk::IntersticeTypeDef { #def }
            #[interstice_sdk::init]
            fn #register() {
                interstice_sdk::registry::register_type_def(#get);
            }
        }
    });

    quote! {
        #[derive(interstice_sdk::interstice_abi_macros::IntersticeType, Clone)]
        #input

        fn #type_def_fn() -> interstice_sdk::IntersticeTypeDef {
            #type_def
        }

        #[interstice_sdk::init]
        fn #register_type_def_fn() {
            interstice_sdk::registry::register_type_def(#type_def_fn);
        }

        #(#register_variant_structs)*
    }
    .into()
}
