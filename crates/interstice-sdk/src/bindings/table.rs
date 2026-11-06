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

    let entry_idents: Vec<syn::Ident> =
        table_schema.fields.iter().map(|f| syn::Ident::new(&f.name, span)).collect();
    let entry_types: Vec<Type> = table_schema
        .fields
        .iter()
        .map(|f| syn::parse_str(&f.field_type.to_string()).expect("a field type parses"))
        .collect();
    let entry_count = entry_idents.len();

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
            quote! {
                #field: row_entries
                    .next()
                    .ok_or_else(|| interstice_sdk::interstice_abi::IntersticeAbiError::ConversionError(
                        format!("row is missing field {}", stringify!(#field)),
                    ))?
                    .try_into()?
            }
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

            let scan_op = |fn_name: &syn::Ident, variant: TokenStream| {
                quote! {
                    pub fn #fn_name(&self, value: #index_type) -> Result<Vec<#table_struct_name>, String>
                    where
                        Caps: interstice_sdk::CanRead<#table_struct_name>,
                    {
                        interstice_sdk::reads::read_index(
                            #module_selection_tokens,
                            #table_name_lit,
                            #index_name_lit,
                            interstice_sdk::IndexQuery::#variant(interstice_sdk::reads::to_index_key(value)?),
                        )
                    }
                }
            };

            let unique_method = if index.unique {
                quote! {
                    pub fn #fn_get(&self, value: #index_type) -> Result<Option<#table_struct_name>, String>
                    where
                        Caps: interstice_sdk::CanRead<#table_struct_name>,
                    {
                        Ok(self.#fn_eq(value)?.into_iter().next())
                    }
                }
            } else {
                quote! {}
            };

            let btree_methods = if index.index_type == interstice_abi::IndexType::BTree {
                let lt = scan_op(&fn_lt, quote!(Lt));
                let lte = scan_op(&fn_lte, quote!(Lte));
                let gt = scan_op(&fn_gt, quote!(Gt));
                let gte = scan_op(&fn_gte, quote!(Gte));
                quote! {
                    #lt
                    #lte
                    #gt
                    #gte

                    pub fn #fn_range(
                        &self,
                        min: #index_type,
                        max: #index_type,
                        include_min: bool,
                        include_max: bool,
                    ) -> Result<Vec<#table_struct_name>, String>
                    where
                        Caps: interstice_sdk::CanRead<#table_struct_name>,
                    {
                        interstice_sdk::reads::read_index(
                            #module_selection_tokens,
                            #table_name_lit,
                            #index_name_lit,
                            interstice_sdk::IndexQuery::Range {
                                min: interstice_sdk::reads::to_index_key(min)?,
                                max: interstice_sdk::reads::to_index_key(max)?,
                                include_min,
                                include_max,
                            },
                        )
                    }
                }
            } else {
                quote! {}
            };

            let eq = scan_op(&fn_eq, quote!(Eq));
            quote! {
                #eq
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

        impl interstice_sdk::RowCodec for #table_struct_name {
            fn encode_row(&self, out: &mut Vec<u8>) {
                interstice_sdk::ValueCodec::encode(&self.#primary_key, out);
                interstice_sdk::wire::put_len(out, #entry_count);
                #(interstice_sdk::ValueCodec::encode(&self.#entry_idents, out);)*
            }

            fn decode_row(input: &mut &[u8]) -> Result<Self, interstice_sdk::IntersticeAbiError> {
                let #primary_key = <#primary_key_type as interstice_sdk::ValueCodec>::decode(input)?;
                // A newer version of the table may have appended columns.
                let count = interstice_sdk::table_row::row_wire::take_columns(input, #entry_count, #table_name_lit)?;
                #(
                    let #entry_idents = <#entry_types as interstice_sdk::ValueCodec>::decode(input)?;
                )*
                interstice_sdk::table_row::row_wire::skip_columns(input, count - #entry_count)?;
                Ok(Self { #primary_key, #(#entry_idents),* })
            }
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
                #[allow(unused_mut, unused_variables)]
                let mut row_entries = row.entries.into_iter();
                Ok(Self {
                    #primary_key: row.primary_key.try_into()?,
                    #(#into_struct_entries),*
                })
            }
        }

        impl<Caps> #table_handle_struct_name<Caps> {
            pub fn scan(&self) -> Result<Vec<#table_struct_name>, String>
            where
                Caps: interstice_sdk::CanRead<#table_struct_name>,
            {
                interstice_sdk::reads::read_scan(#module_selection_tokens, #table_name_lit)
            }

            pub fn get(&self, primary_key: #primary_key_type) -> Result<Option<#table_struct_name>, String>
            where
                Caps: interstice_sdk::CanRead<#table_struct_name>,
            {
                interstice_sdk::reads::read_get(#module_selection_tokens, #table_name_lit, primary_key)
            }

            #(#index_methods)*
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
