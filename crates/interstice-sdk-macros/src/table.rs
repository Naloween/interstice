use proc_macro::TokenStream;
use quote::{quote, ToTokens};
use syn::Meta;

pub fn table_macro(attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(item as syn::ItemStruct);
    let struct_ident = &input.ident;
    let struct_name = struct_ident.to_string();
    let table_name = struct_name.to_lowercase();
    let table_handle_struct =
        syn::Ident::new(&format!("{}Handle", struct_name), struct_ident.span());
    let has_table_handle_trait =
        syn::Ident::new(&format!("Has{}Handle", struct_name), struct_ident.span());
    let get_table_handle_fn = syn::Ident::new(&table_name, struct_ident.span());
    let schema_fn = syn::Ident::new(
        &format!("interstice_{}_schema", table_name),
        struct_ident.span(),
    );
    let register_fn = syn::Ident::new(
        &format!("interstice_register_{}_table", table_name),
        struct_ident.span(),
    );

    // Check the visibility of the table
    let args = syn::parse_macro_input!(
        attr with syn::punctuated::Punctuated::<Meta, syn::Token![,]>::parse_terminated
    );

    let visibility = args
        .iter()
        .find_map(|arg| {
            if let Meta::Path(nv) = arg {
                if nv.is_ident("public") {
                    return Some(quote! { interstice_sdk::TableVisibility::Public });
                } else if nv.is_ident("private") {
                    return Some(quote! { interstice_sdk::TableVisibility::Private });
                }
            }
            None
        })
        .unwrap_or_else(|| {
            quote! { interstice_sdk::TableVisibility::Private }
        });

    // Generate the entries and primary key
    let fields = match &input.fields {
        syn::Fields::Named(f) => &f.named,
        _ => {
            return quote! {compile_error!("Tables must be structs with named fields");}.into();
        }
    };

    let mut primary_key: Option<(&syn::Ident, String, proc_macro2::TokenStream)> = None;
    let mut schema_fields = Vec::new();
    let mut entry_fields = Vec::new();
    for field in fields.iter() {
        let field_name = field.ident.as_ref().unwrap().to_string();
        let field_ty_str = field.ty.to_token_stream().to_string();
        let field_ty = quote! { interstice_sdk::IntersticeType::from_str(&#field_ty_str).unwrap()};
        let field_ident = field.ident.as_ref().unwrap();

        let is_pk = field
            .attrs
            .iter()
            .any(|attr| attr.path().is_ident("primary_key"));

        if is_pk {
            if primary_key.is_some() {
                return quote! {compile_error!("Only one #[primary_key] field is allowed");}.into();
            }

            primary_key = Some((field_ident, field_name, field_ty));
        } else {
            entry_fields.push(field_ident.clone());
            schema_fields.push(quote! {
                interstice_sdk::FieldDef {
                    name: #field_name.to_string(),
                    field_type: #field_ty,
                }
            });
        }
    }
    let (pk_ident, pk_name, pk_type) = match primary_key {
        Some(pk) => pk,
        None => {
            return quote! {compile_error!("A #[primary_key] field is required");}.into();
        }
    };

    // Generate the output struct without the primary key attribute
    let mut output_struct = input.clone();
    if let syn::Fields::Named(fields) = &mut output_struct.fields {
        for field in fields.named.iter_mut() {
            field.attrs.retain(|a| !a.path().is_ident("primary_key"));
        }
    }

    quote! {
        #[derive(IntersticeType)]
        #output_struct

        impl Into<interstice_sdk::Row> for #struct_ident {
            fn into(self) -> interstice_sdk::Row{
                Row {
                    primary_key: self.#pk_ident.into(),
                    entries: vec![#(self.#entry_fields.clone().into()),*],
                }
            }
        }

        impl Into<#struct_ident> for interstice_sdk::Row {
            fn into(self) -> #struct_ident{
                let mut row_entries = self.entries.into_iter();
                #struct_ident {
                    #pk_ident: self.primary_key.into(), // convert IntersticeValue → PK type
                    #(
                        #entry_fields: row_entries.next().unwrap().into(), // convert IntersticeValue → field type
                    )*
                }
            }
        }

        fn #schema_fn() -> interstice_sdk::TableSchema {
            interstice_sdk::TableSchema {
                name: #table_name.to_string(),
                visibility: #visibility,
                fields: vec![#(#schema_fields),*],
                primary_key: interstice_sdk::FieldDef {
                    name: #pk_name.to_string(),
                    field_type: #pk_type.into(),
                },
            }
        }
        #[interstice_sdk::init]
        fn #register_fn() {
            interstice_sdk::registry::register_table(#schema_fn);
        }

        pub struct #table_handle_struct{
        }

        impl #table_handle_struct{
            pub fn insert(&self, row: #struct_ident){
                interstice_sdk::host_calls::insert_row(
                    ModuleSelection::Current,
                    #table_name.to_string(),
                    row.into(),
                );
            }

            pub fn scan(&self) -> Vec<#struct_ident>{
                interstice_sdk::host_calls::scan(interstice_sdk::ModuleSelection::Current, #table_name.to_string()).into_iter().map(|x| x.into()).collect()
            }
        }

        pub trait #has_table_handle_trait {
            fn #get_table_handle_fn(&self) -> #table_handle_struct;
        }

        impl #has_table_handle_trait for interstice_sdk::CurrentModuleContext {
            fn #get_table_handle_fn(&self) -> #table_handle_struct {
                return #table_handle_struct {}
            }
        }

    }
    .into()
}
