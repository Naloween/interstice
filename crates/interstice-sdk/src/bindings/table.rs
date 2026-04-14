use interstice_abi::TableSchema;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::{self, LitStr, Type};

pub fn get_table_code(
    table_schema: TableSchema,
    module_tables_name: &str,
    module_selection_tokens: TokenStream,
    table_name_override: Option<&str>,
    ref_node_name: &str,
    ref_module_name: &str,
    ref_table_name: &str,
) -> TokenStream {
    let span = proc_macro2::Span::call_site();
    let table_name_ident = format_ident!("{}", table_schema.name);
    let effective_table_name = table_name_override
        .unwrap_or(&table_schema.name)
        .to_string();
    let table_name_lit = LitStr::new(&effective_table_name, span);
    // Schema / caps use the logical table name (matches `replicated_tables` in interstice_module!);
    // runtime host calls use `effective_table_name` (e.g. local `__replica__…` shadow table).
    let schema_table_name_lit = LitStr::new(ref_table_name, span);
    let table_struct_name = syn::Ident::new(&table_schema.type_name, span);
    let table_handle_struct_name =
        syn::Ident::new(&(table_schema.type_name.clone() + "Handle"), span);
    let has_table_handle_trait_name = syn::Ident::new(
        &("Has".to_string() + &table_schema.type_name + "Handle"),
        span,
    );
    let module_tables_ident = syn::Ident::new(module_tables_name, span);
    let read_cap = format_ident!("Read{}", table_schema.type_name);
    let module_lit = LitStr::new(ref_module_name, span);
    let node_sel_ts = if ref_node_name == "current" {
        quote! { interstice_sdk::NodeSelection::Current }
    } else {
        let n = LitStr::new(ref_node_name, span);
        quote! { interstice_sdk::NodeSelection::Other(#n.to_string()) }
    };
    let module_sel_ts = quote! { interstice_sdk::ModuleSelection::Other(#module_lit.to_string()) };

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
                    pub fn #fn_get(&self, value: #index_type) -> Option<#table_struct_name>
                    where
                        Caps: interstice_sdk::CanRead<#table_struct_name>,
                    {
                        self.#fn_eq(value).into_iter().next()
                    }
                }
            } else {
                quote! {}
            };

            let btree_methods = if index.index_type == interstice_abi::IndexType::BTree {
                quote! {
                    pub fn #fn_lt(&self, value: #index_type) -> Vec<#table_struct_name>
                    where
                        Caps: interstice_sdk::CanRead<#table_struct_name>,
                    {
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

                        pub fn #fn_lte(&self, value: #index_type) -> Vec<#table_struct_name>
                        where
                            Caps: interstice_sdk::CanRead<#table_struct_name>,
                        {
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

                        pub fn #fn_gt(&self, value: #index_type) -> Vec<#table_struct_name>
                        where
                            Caps: interstice_sdk::CanRead<#table_struct_name>,
                        {
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

                        pub fn #fn_gte(&self, value: #index_type) -> Vec<#table_struct_name>
                        where
                            Caps: interstice_sdk::CanRead<#table_struct_name>,
                        {
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
                    ) -> Vec<#table_struct_name>
                    where
                        Caps: interstice_sdk::CanRead<#table_struct_name>,
                    {
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
                pub fn #fn_eq(&self, value: #index_type) -> Vec<#table_struct_name>
                where
                    Caps: interstice_sdk::CanRead<#table_struct_name>,
                {
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
        #[derive(Clone, Copy, Debug, Default)]
        pub struct #read_cap;

        impl interstice_sdk::CanRead<#table_struct_name> for #read_cap {}

        impl interstice_sdk::TableRow for #table_struct_name {
            const TABLE_NAME: &'static str = #schema_table_name_lit;
            fn table_ref() -> interstice_sdk::ReducerTableRef {
                interstice_sdk::ReducerTableRef {
                    node_selection: #node_sel_ts,
                    module_selection: #module_sel_ts,
                    table_name: #schema_table_name_lit.to_string(),
                }
            }
        }

        impl interstice_sdk::ReducerCapPiece for #read_cap {
            fn extend_reducer_schema(
                reads: &mut Vec<interstice_sdk::ReducerTableRef>,
                _inserts: &mut Vec<interstice_sdk::ReducerTableRef>,
                _updates: &mut Vec<interstice_sdk::ReducerTableRef>,
                _deletes: &mut Vec<interstice_sdk::ReducerTableRef>,
            ) {
                reads.push(interstice_sdk::ReducerTableRef {
                    node_selection: #node_sel_ts,
                    module_selection: #module_sel_ts,
                    table_name: #schema_table_name_lit.to_string(),
                });
            }
        }

        impl interstice_sdk::QueryCapPiece for #read_cap {
            fn extend_query_schema(reads: &mut Vec<interstice_sdk::ReducerTableRef>) {
                reads.push(interstice_sdk::ReducerTableRef {
                    node_selection: #node_sel_ts,
                    module_selection: #module_sel_ts,
                    table_name: #schema_table_name_lit.to_string(),
                });
            }
        }

        pub struct #table_handle_struct_name<Caps> {
            _caps: std::marker::PhantomData<Caps>,
        }

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

        impl<Caps> #table_handle_struct_name<Caps> {
            pub fn scan(&self) -> Vec<#table_struct_name>
            where
                Caps: interstice_sdk::CanRead<#table_struct_name>,
            {
                interstice_sdk::host_calls::scan(
                    #module_selection_tokens,
                    #table_name_lit.to_string(),
                )
                .expect("Table scan failed")
                .into_iter()
                .map(|x| x.try_into().unwrap())
                .collect()
            }

            pub fn get(&self, primary_key: #primary_key_type) -> Option<#table_struct_name>
            where
                Caps: interstice_sdk::CanRead<#table_struct_name>,
            {
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

        impl<Caps> IntoIterator for #table_handle_struct_name<Caps>
        where
            Caps: interstice_sdk::CanRead<#table_struct_name>,
        {
            type Item = #table_struct_name;
            type IntoIter = std::vec::IntoIter<#table_struct_name>;

            fn into_iter(self) -> Self::IntoIter {
                self.scan().into_iter()
            }
        }

        pub trait #has_table_handle_trait_name<Caps> {
            fn #table_name_ident(&self) -> #table_handle_struct_name<Caps>;
        }

        impl<Caps> #has_table_handle_trait_name<Caps> for #module_tables_ident<Caps> {
            fn #table_name_ident(&self) -> #table_handle_struct_name<Caps> {
                #table_handle_struct_name { _caps: std::marker::PhantomData }
            }
        }
    }
}
