use interstice_abi::TableSchema;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::{self, LitStr, Type};

pub fn get_table_code(
    table_schema: TableSchema,
    module_tables_name: &str,
    module_selection_tokens: TokenStream,
    table_name_override: Option<&str>,
) -> TokenStream {
    let span = proc_macro2::Span::call_site();
    let table_name_ident = format_ident!("{}", table_schema.name);
    let effective_table_name = table_name_override
        .unwrap_or(&table_schema.name)
        .to_string();
    let table_name_lit = LitStr::new(&effective_table_name, span);
    let table_struct_name = syn::Ident::new(&table_schema.type_name, span);
    let table_handle_struct_name =
        syn::Ident::new(&(table_schema.type_name.clone() + "Handle"), span);
    let has_table_handle_trait_name = syn::Ident::new(
        &("Has".to_string() + &table_schema.type_name + "Handle"),
        span,
    );
    let module_tables_ident = syn::Ident::new(module_tables_name, span);

    let into_row_entries: Vec<TokenStream> = table_schema
        .fields
        .iter()
        .map(|entry| {
            let field = syn::Ident::new(&entry.name, span);
            quote! { self.#field.into() }
        })
        .collect();

    let into_struct_entries: Vec<TokenStream> = table_schema
        .fields
        .iter()
        .map(|entry| {
            let field = syn::Ident::new(&entry.name, span);
            quote! { #field: row_entries.next().unwrap().try_into()? }
        })
        .collect();

    let primary_key = syn::Ident::new(&table_schema.primary_key.name, span);
    let primary_key_type: Type = syn::parse_str(&table_schema.primary_key.field_type.to_string())
        .expect("Failed to parse primary key type");

    let index_methods: Vec<TokenStream> = table_schema
        .indexes
        .iter()
        .map(|index| {
            let index_name = &index.field_name;
            let index_name_lit = LitStr::new(index_name, span);
            let index_field_type_str = table_schema
                .fields
                .iter()
                .find(|f| f.name == *index_name)
                .map(|f| f.field_type.to_string())
                .expect("Index field type not found in table fields");
            let index_type: Type =
                syn::parse_str(&index_field_type_str).expect("Failed to parse index field type");

            let fn_eq = format_ident!("scan_by_{}_eq", index_name);
            let fn_get = format_ident!("get_by_{}", index_name);
            let fn_lt = format_ident!("scan_by_{}_lt", index_name);
            let fn_lte = format_ident!("scan_by_{}_lte", index_name);
            let fn_gt = format_ident!("scan_by_{}_gt", index_name);
            let fn_gte = format_ident!("scan_by_{}_gte", index_name);
            let fn_range = format_ident!("scan_by_{}_range", index_name);

            let unique_method = if index.unique {
                quote! {
                    pub fn #fn_get(&self, value: #index_type) -> Option<#table_struct_name> {
                        self.#fn_eq(value).into_iter().next()
                    }
                }
            } else {
                quote! {}
            };

            let btree_methods = if index.index_type == interstice_abi::IndexType::BTree {
                quote! {
                    pub fn #fn_lt(&self, value: #index_type) -> Vec<#table_struct_name> {
                        interstice_sdk::host_calls::scan_index(
                            #module_selection_tokens,
                            #table_name_lit.to_string(),
                            #index_name_lit.to_string(),
                            interstice_sdk::IndexQuery::Lt(
                                TryInto::<interstice_sdk::IndexKey>::try_into(Into::<interstice_sdk::IntersticeValue>::into(value))
                                    .expect("Failed to convert IntersticeValue to IndexKey"),
                            ),
                        )
                            .expect("Index scan failed")
                            .into_iter()
                            .map(|x| x.try_into().unwrap())
                            .collect()
                    }

                        pub fn #fn_lte(&self, value: #index_type) -> Vec<#table_struct_name> {
                        interstice_sdk::host_calls::scan_index(
                            #module_selection_tokens,
                            #table_name_lit.to_string(),
                            #index_name_lit.to_string(),
                            interstice_sdk::IndexQuery::Lte(
                                TryInto::<interstice_sdk::IndexKey>::try_into(Into::<interstice_sdk::IntersticeValue>::into(value))
                                    .expect("Failed to convert IntersticeValue to IndexKey"),
                            ),
                        )
                            .expect("Index scan failed")
                            .into_iter()
                            .map(|x| x.try_into().unwrap())
                            .collect()
                    }

                        pub fn #fn_gt(&self, value: #index_type) -> Vec<#table_struct_name> {
                        interstice_sdk::host_calls::scan_index(
                            #module_selection_tokens,
                            #table_name_lit.to_string(),
                            #index_name_lit.to_string(),
                            interstice_sdk::IndexQuery::Gt(
                                TryInto::<interstice_sdk::IndexKey>::try_into(Into::<interstice_sdk::IntersticeValue>::into(value))
                                    .expect("Failed to convert IntersticeValue to IndexKey"),
                            ),
                        )
                            .expect("Index scan failed")
                            .into_iter()
                            .map(|x| x.try_into().unwrap())
                            .collect()
                    }

                        pub fn #fn_gte(&self, value: #index_type) -> Vec<#table_struct_name> {
                        interstice_sdk::host_calls::scan_index(
                            #module_selection_tokens,
                            #table_name_lit.to_string(),
                            #index_name_lit.to_string(),
                            interstice_sdk::IndexQuery::Gte(
                                TryInto::<interstice_sdk::IndexKey>::try_into(Into::<interstice_sdk::IntersticeValue>::into(value))
                                    .expect("Failed to convert IntersticeValue to IndexKey"),
                            ),
                        )
                            .expect("Index scan failed")
                            .into_iter()
                            .map(|x| x.try_into().unwrap())
                            .collect()
                    }

                    pub fn #fn_range(
                        &self,
                        min: #index_type,
                        max: #index_type,
                        include_min: bool,
                        include_max: bool,
                    ) -> Vec<#table_struct_name> {
                        interstice_sdk::host_calls::scan_index(
                            #module_selection_tokens,
                            #table_name_lit.to_string(),
                            #index_name_lit.to_string(),
                            interstice_sdk::IndexQuery::Range {
                                min: TryInto::<interstice_sdk::IndexKey>::try_into(Into::<interstice_sdk::IntersticeValue>::into(min))
                                    .expect("Failed to convert IntersticeValue to IndexKey"),
                                max: TryInto::<interstice_sdk::IndexKey>::try_into(Into::<interstice_sdk::IntersticeValue>::into(max))
                                    .expect("Failed to convert IntersticeValue to IndexKey"),
                                include_min,
                                include_max,
                            },
                        )
                        .expect("Index scan failed")
                        .into_iter()
                        .map(|x| x.try_into().unwrap())
                        .collect()
                    }
                }
            } else {
                quote! {}
            };

            quote! {
                pub fn #fn_eq(&self, value: #index_type) -> Vec<#table_struct_name> {
                    interstice_sdk::host_calls::scan_index(
                        #module_selection_tokens,
                        #table_name_lit.to_string(),
                        #index_name_lit.to_string(),
                        interstice_sdk::IndexQuery::Eq(
                            TryInto::<interstice_sdk::IndexKey>::try_into(Into::<interstice_sdk::IntersticeValue>::into(value))
                                .expect("Failed to convert IntersticeValue to IndexKey"),
                        ),
                    )
                    .expect("Index scan failed")
                    .into_iter()
                    .map(|x| x.try_into().unwrap())
                    .collect()
                }

                #unique_method
                #btree_methods
            }
        })
        .collect();

    quote! {
        pub struct #table_handle_struct_name {}

        impl Into<interstice_sdk::Row> for #table_struct_name {
            fn into(self) -> interstice_sdk::Row {
                interstice_sdk::Row {
                    primary_key: self.#primary_key.into(),
                    entries: vec![ #(#into_row_entries),* ],
                }
            }
        }

        impl TryFrom<interstice_sdk::Row> for #table_struct_name {
            type Error = interstice_sdk::interstice_abi::IntersticeAbiError;

            fn try_from(row: interstice_sdk::Row) -> Result<Self, Self::Error> {
                let mut row_entries = row.entries.into_iter();
                Ok(Self {
                    #primary_key: row.primary_key.try_into()?,
                    #(#into_struct_entries),*
                })
            }
        }

        impl #table_handle_struct_name {
            pub fn scan(&self) -> Vec<#table_struct_name> {
                interstice_sdk::host_calls::scan(
                    #module_selection_tokens,
                    #table_name_lit.to_string(),
                )
                .expect("Table scan failed")
                .into_iter()
                .map(|x| x.try_into().unwrap())
                .collect()
            }

            pub fn get(&self, primary_key: #primary_key_type) -> Option<#table_struct_name> {
                interstice_sdk::host_calls::get_by_primary_key(
                    #module_selection_tokens,
                    #table_name_lit,
                    TryInto::<interstice_sdk::IndexKey>::try_into(Into::<interstice_sdk::IntersticeValue>::into(primary_key))
                        .expect("Failed to convert IntersticeValue to IndexKey"),
                )
                .expect("Table get_by_primary_key failed")
                .map(|row| row.try_into().unwrap())
            }

            #(#index_methods)*
        }

        pub trait #has_table_handle_trait_name {
            fn #table_name_ident(&self) -> #table_handle_struct_name;
        }

        impl #has_table_handle_trait_name for #module_tables_ident {
            fn #table_name_ident(&self) -> #table_handle_struct_name {
                #table_handle_struct_name {}
            }
        }
    }
}
