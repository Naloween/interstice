use proc_macro::TokenStream;
use quote::{ToTokens, quote};
use syn::Meta;

use crate::index_key::validate_index_key_type;

pub fn table_macro(attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(item as syn::ItemStruct);
    let struct_ident = &input.ident;
    let struct_name = struct_ident.to_string();
    let table_name = struct_name.to_lowercase();
    let table_name_lit = syn::LitStr::new(&table_name, struct_ident.span());
    let read_cap = syn::Ident::new(&format!("Read{}", struct_name), struct_ident.span());
    let insert_cap = syn::Ident::new(&format!("Insert{}", struct_name), struct_ident.span());
    let update_cap = syn::Ident::new(&format!("Update{}", struct_name), struct_ident.span());
    let delete_cap = syn::Ident::new(&format!("Delete{}", struct_name), struct_ident.span());
    let table_edit_handle_struct =
        syn::Ident::new(&format!("{}EditHandle", struct_name), struct_ident.span());
    let table_read_handle_struct =
        syn::Ident::new(&format!("{}ReadHandle", struct_name), struct_ident.span());
    let has_table_edit_handle_trait = syn::Ident::new(
        &format!("Has{}EditHandle", struct_name),
        struct_ident.span(),
    );
    let get_table_edit_handle_fn = syn::Ident::new(&table_name, struct_ident.span());
    let has_table_read_handle_trait = syn::Ident::new(
        &format!("Has{}ReadHandle", struct_name),
        struct_ident.span(),
    );
    let get_table_read_handle_fn = syn::Ident::new(&table_name, struct_ident.span());

    let schema_fn = syn::Ident::new(
        &format!("interstice_{}_schema", table_name),
        struct_ident.span(),
    );
    let register_fn = syn::Ident::new(
        &format!("interstice_register_{}_table", table_name),
        struct_ident.span(),
    );

    // Parse table attributes (visibility + persistence)
    let args = syn::parse_macro_input!(
        attr with syn::punctuated::Punctuated::<Meta, syn::Token![,]>::parse_terminated
    );

    let mut visibility = quote! { interstice_sdk::TableVisibility::Private };
    let mut persistence = quote! { interstice_sdk::PersistenceKind::Logged };

    for arg in args.iter() {
        if let Meta::Path(nv) = arg {
            if nv.is_ident("public") {
                visibility = quote! { interstice_sdk::TableVisibility::Public };
                continue;
            } else if nv.is_ident("private") {
                visibility = quote! { interstice_sdk::TableVisibility::Private };
                continue;
            } else if nv.is_ident("ephemeral") {
                persistence = quote! { interstice_sdk::PersistenceKind::Ephemeral };
                continue;
            } else if nv.is_ident("stateful") {
                persistence = quote! { interstice_sdk::PersistenceKind::Stateful };
                continue;
            } else if nv.is_ident("logged") {
                persistence = quote! { interstice_sdk::PersistenceKind::Logged };
                continue;
            } else {
                return quote! { compile_error!("Invalid table attribute. Expected 'public', 'private', 'ephemeral', or 'stateful'") } .into();
            }
        } else {
            return quote! { compile_error!("Invalid table attribute syntax") }.into();
        }
    }

    // Generate the entries and primary key
    let fields = match &input.fields {
        syn::Fields::Named(f) => &f.named,
        _ => {
            return quote! {compile_error!("Tables must be structs with named fields");}.into();
        }
    };

    let mut primary_key: Option<(
        &syn::Ident,
        String,
        proc_macro2::TokenStream,
        syn::Type,
        bool,
    )> = None;
    let mut schema_fields = Vec::new();
    let mut entry_fields = Vec::new();
    let mut index_schemas = Vec::new();
    let mut indexed_fields: Vec<(
        syn::Ident,
        String,
        syn::Type,
        proc_macro2::TokenStream,
        bool,
        bool,
    )> = Vec::new();
    for field in fields.iter() {
        let field_name = field.ident.as_ref().unwrap().to_string();
        let field_ty_ident = field.ty.clone();
        let field_ty_str = field_ty_ident.to_token_stream().to_string();
        let field_ty = quote! { interstice_sdk::IntersticeType::from_str(&#field_ty_str).unwrap()};
        let field_ident = field.ident.as_ref().unwrap();

        let is_pk = field
            .attrs
            .iter()
            .any(|attr| attr.path().is_ident("primary_key"));

        let mut pk_auto_inc = false;
        if is_pk {
            if let Some(attr) = field
                .attrs
                .iter()
                .find(|attr| attr.path().is_ident("primary_key"))
            {
                let args = attr.parse_args_with(
                    syn::punctuated::Punctuated::<Meta, syn::Token![,]>::parse_terminated,
                );
                if let Ok(args) = args {
                    for arg in args {
                        match arg {
                            Meta::Path(path) if path.is_ident("auto_inc") => {
                                pk_auto_inc = true;
                            }
                            _ => {
                                return quote! {compile_error!("Invalid #[primary_key] argument. Use auto_inc");}.into();
                            }
                        }
                    }
                }
            }
        }

        let index_attr = field
            .attrs
            .iter()
            .find(|attr| attr.path().is_ident("index"));
        if let Some(attr) = index_attr {
            if is_pk {
                return quote! {compile_error!("#[index] cannot be used on #[primary_key] fields");}.into();
            }

            let args = attr.parse_args_with(
                syn::punctuated::Punctuated::<Meta, syn::Token![,]>::parse_terminated,
            );
            let args = match args {
                Ok(args) => args,
                Err(_) => {
                    return quote! {compile_error!("Invalid #[index(...)] syntax");}.into();
                }
            };

            let mut index_type: Option<proc_macro2::TokenStream> = None;
            let mut is_btree = false;
            let mut unique = false;
            let mut auto_inc = false;

            for arg in args {
                match arg {
                    Meta::Path(path) if path.is_ident("hash") => {
                        index_type = Some(quote! { interstice_sdk::IndexType::Hash });
                        is_btree = false;
                    }
                    Meta::Path(path) if path.is_ident("btree") => {
                        index_type = Some(quote! { interstice_sdk::IndexType::BTree });
                        is_btree = true;
                    }
                    Meta::Path(path) if path.is_ident("unique") => {
                        unique = true;
                    }
                    Meta::Path(path) if path.is_ident("auto_inc") => {
                        auto_inc = true;
                    }
                    _ => {
                        return quote! {compile_error!("Invalid #[index] argument. Use hash, btree, unique, and/or auto_inc");}.into();
                    }
                }
            }

            if index_type.is_none() {
                return quote! {compile_error!("#[index] requires an index type (hash or btree)");}
                    .into();
            }

            if let Err(message) = validate_index_key_type(&field_ty_ident) {
                return quote! {compile_error!(#message);}.into();
            }

            if auto_inc {
                if !matches!(field_ty_ident, syn::Type::Path(_)) {
                    return quote! {compile_error!("#[index(auto_inc)] requires an integer field type");}.into();
                }
                let type_name = field_ty_ident.to_token_stream().to_string();
                match type_name.as_str() {
                    "u8" | "u32" | "u64" | "i32" | "i64" => {}
                    _ => {
                        return quote! {compile_error!("#[index(auto_inc)] is only supported for integer field types (u8, u32, u64, i32, i64)");}.into();
                    }
                }
                unique = true;
            }

            let index_type = index_type.unwrap();
            index_schemas.push(quote! {
                interstice_sdk::IndexSchema {
                    field_name: #field_name.to_string(),
                    index_type: #index_type,
                    unique: #unique,
                    auto_inc: #auto_inc,
                }
            });

            indexed_fields.push((
                field_ident.clone(),
                field_name.clone(),
                field_ty_ident.clone(),
                index_type,
                unique,
                is_btree,
            ));
        }

        if is_pk {
            if primary_key.is_some() {
                return quote! {compile_error!("Only one #[primary_key] field is allowed");}.into();
            }

            primary_key = Some((
                field_ident,
                field_name,
                field_ty,
                field_ty_ident,
                pk_auto_inc,
            ));
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
    let (pk_ident, pk_name, pk_type, pk_type_ident, pk_auto_inc) = match primary_key {
        Some(pk) => pk,
        None => {
            return quote! {compile_error!("A #[primary_key] field is required");}.into();
        }
    };

    if let Err(message) = validate_index_key_type(&pk_type_ident) {
        return quote! {compile_error!(#message);}.into();
    }

    if pk_auto_inc {
        let type_name = pk_type_ident.to_token_stream().to_string();
        match type_name.as_str() {
            "u8" | "u32" | "u64" | "i32" | "i64" => {}
            _ => {
                return quote! {compile_error!("#[primary_key(auto_inc)] is only supported for integer field types (u8, u32, u64, i32, i64)");}.into();
            }
        }
    }

    // Generate the output struct without the primary key attribute
    let mut output_struct = input.clone();
    if let syn::Fields::Named(fields) = &mut output_struct.fields {
        for field in fields.named.iter_mut() {
            field.attrs.retain(|a| !a.path().is_ident("primary_key"));
            field.attrs.retain(|a| !a.path().is_ident("index"));
        }
    }

    let mut index_read_methods = Vec::new();
    for (_index_ident, index_name, index_ty_ident, _index_type, unique, is_btree) in &indexed_fields
    {
        let fn_eq = syn::Ident::new(&format!("scan_by_{}_eq", index_name), struct_ident.span());
        let fn_lt = syn::Ident::new(&format!("scan_by_{}_lt", index_name), struct_ident.span());
        let fn_lte = syn::Ident::new(&format!("scan_by_{}_lte", index_name), struct_ident.span());
        let fn_gt = syn::Ident::new(&format!("scan_by_{}_gt", index_name), struct_ident.span());
        let fn_gte = syn::Ident::new(&format!("scan_by_{}_gte", index_name), struct_ident.span());
        let fn_range = syn::Ident::new(
            &format!("scan_by_{}_range", index_name),
            struct_ident.span(),
        );
        let fn_get = syn::Ident::new(&format!("get_by_{}", index_name), struct_ident.span());

        index_read_methods.push(quote! {
            pub fn #fn_eq(&self, value: #index_ty_ident) -> Vec<#struct_ident>
            where
                Caps: interstice_sdk::CanRead<#struct_ident>,
            {
                interstice_sdk::host_calls::scan_index(
                    interstice_sdk::ModuleSelection::Current,
                    #table_name.to_string(),
                    #index_name.to_string(),
                    interstice_sdk::IndexQuery::Eq(TryInto::<interstice_sdk::IndexKey>::try_into(Into::<interstice_sdk::IntersticeValue>::into(value)).expect("Failed to convert IntersticeValue to IndexKey")),
                )
                .expect("Index scan failed")
                .into_iter()
                .map(|x| x.into())
                .collect()
            }
        });

        if *unique {
            index_read_methods.push(quote! {
                pub fn #fn_get(&self, value: #index_ty_ident) -> Option<#struct_ident>
                where
                    Caps: interstice_sdk::CanRead<#struct_ident>,
                {
                    self.#fn_eq(value).into_iter().next()
                }
            });
        }

        if *is_btree {
            index_read_methods.push(quote! {
                pub fn #fn_lt(&self, value: #index_ty_ident) -> Vec<#struct_ident>
                where
                    Caps: interstice_sdk::CanRead<#struct_ident>,
                {
                    interstice_sdk::host_calls::scan_index(
                        interstice_sdk::ModuleSelection::Current,
                        #table_name.to_string(),
                        #index_name.to_string(),
                        interstice_sdk::IndexQuery::Lt(TryInto::<interstice_sdk::IndexKey>::try_into(Into::<interstice_sdk::IntersticeValue>::into(value)).expect("Failed to convert IntersticeValue to IndexKey")),
                    )
                    .expect("Index scan failed")
                    .into_iter()
                    .map(|x| x.into())
                    .collect()
                }

                pub fn #fn_lte(&self, value: #index_ty_ident) -> Vec<#struct_ident>
                where
                    Caps: interstice_sdk::CanRead<#struct_ident>,
                {
                    interstice_sdk::host_calls::scan_index(
                        interstice_sdk::ModuleSelection::Current,
                        #table_name.to_string(),
                        #index_name.to_string(),
                        interstice_sdk::IndexQuery::Lte(TryInto::<interstice_sdk::IndexKey>::try_into(Into::<interstice_sdk::IntersticeValue>::into(value)).expect("Failed to convert IntersticeValue to IndexKey")),
                    )
                    .expect("Index scan failed")
                    .into_iter()
                    .map(|x| x.into())
                    .collect()
                }

                pub fn #fn_gt(&self, value: #index_ty_ident) -> Vec<#struct_ident>
                where
                    Caps: interstice_sdk::CanRead<#struct_ident>,
                {
                    interstice_sdk::host_calls::scan_index(
                        interstice_sdk::ModuleSelection::Current,
                        #table_name.to_string(),
                        #index_name.to_string(),
                        interstice_sdk::IndexQuery::Gt(TryInto::<interstice_sdk::IndexKey>::try_into(Into::<interstice_sdk::IntersticeValue>::into(value)).expect("Failed to convert IntersticeValue to IndexKey")),
                    )
                    .expect("Index scan failed")
                    .into_iter()
                    .map(|x| x.into())
                    .collect()
                }

                pub fn #fn_gte(&self, value: #index_ty_ident) -> Vec<#struct_ident>
                where
                    Caps: interstice_sdk::CanRead<#struct_ident>,
                {
                    interstice_sdk::host_calls::scan_index(
                        interstice_sdk::ModuleSelection::Current,
                        #table_name.to_string(),
                        #index_name.to_string(),
                        interstice_sdk::IndexQuery::Gte(TryInto::<interstice_sdk::IndexKey>::try_into(Into::<interstice_sdk::IntersticeValue>::into(value)).expect("Failed to convert IntersticeValue to IndexKey")),
                    )
                    .expect("Index scan failed")
                    .into_iter()
                    .map(|x| x.into())
                    .collect()
                }

                pub fn #fn_range(
                    &self,
                    min: #index_ty_ident,
                    max: #index_ty_ident,
                    include_min: bool,
                    include_max: bool,
                ) -> Vec<#struct_ident>
                where
                    Caps: interstice_sdk::CanRead<#struct_ident>,
                {
                    interstice_sdk::host_calls::scan_index(
                        interstice_sdk::ModuleSelection::Current,
                        #table_name.to_string(),
                        #index_name.to_string(),
                        interstice_sdk::IndexQuery::Range {
                            min: TryInto::<interstice_sdk::IndexKey>::try_into(Into::<interstice_sdk::IntersticeValue>::into(min)).expect("Failed to convert IntersticeValue to IndexKey"),
                            max: TryInto::<interstice_sdk::IndexKey>::try_into(Into::<interstice_sdk::IntersticeValue>::into(max)).expect("Failed to convert IntersticeValue to IndexKey"),
                            include_min,
                            include_max,
                        },
                    )
                    .expect("Index scan failed")
                    .into_iter()
                    .map(|x| x.into())
                    .collect()
                }
            });
        }
    }

    let read_table_imp = quote! {
        pub fn scan(&self) -> Vec<#struct_ident>
        where
            Caps: interstice_sdk::CanRead<#struct_ident>,
        {
            interstice_sdk::host_calls::scan(interstice_sdk::ModuleSelection::Current, #table_name.to_string())
                .expect("Table scan failed")
                .into_iter()
                .map(|x| x.into())
                .collect()
        }

        pub fn get(&self, primary_key: #pk_type_ident) -> Option<#struct_ident>
        where
            Caps: interstice_sdk::CanRead<#struct_ident>,
        {
            interstice_sdk::host_calls::get_by_primary_key(
                interstice_sdk::ModuleSelection::Current,
                #table_name,
                TryInto::<interstice_sdk::IndexKey>::try_into(Into::<interstice_sdk::IntersticeValue>::into(primary_key)).expect("Failed to convert IntersticeValue to IndexKey"),
            )
            .expect("Table get_by_primary_key failed")
            .map(|row| row.into())
        }

        #(#index_read_methods)*
    };

    quote! {
        #[interstice_type]
        #output_struct

        #[derive(Clone, Copy, Debug, Default)]
        pub struct #read_cap;

        #[derive(Clone, Copy, Debug, Default)]
        pub struct #insert_cap;

        #[derive(Clone, Copy, Debug, Default)]
        pub struct #update_cap;

        #[derive(Clone, Copy, Debug, Default)]
        pub struct #delete_cap;

        impl interstice_sdk::CanRead<#struct_ident> for #read_cap {}

        impl interstice_sdk::CanInsert<#struct_ident> for #insert_cap {}

        impl interstice_sdk::CanUpdate<#struct_ident> for #update_cap {}

        impl interstice_sdk::CanDelete<#struct_ident> for #delete_cap {}

        impl interstice_sdk::TableRow for #struct_ident {
            const TABLE_NAME: &'static str = #table_name_lit;
        }

        impl interstice_sdk::ReducerCapPiece for #read_cap {
            fn extend_reducer_schema(
                reads: &mut Vec<interstice_sdk::ReducerTableRef>,
                _inserts: &mut Vec<interstice_sdk::ReducerTableRef>,
                _updates: &mut Vec<interstice_sdk::ReducerTableRef>,
                _deletes: &mut Vec<interstice_sdk::ReducerTableRef>,
            ) {
                reads.push(interstice_sdk::ReducerTableRef {
                    node_selection: interstice_sdk::NodeSelection::Current,
                    module_selection: interstice_sdk::ModuleSelection::Current,
                    table_name: #table_name.to_string(),
                });
            }
        }

        impl interstice_sdk::ReducerCapPiece for #insert_cap {
            fn extend_reducer_schema(
                _reads: &mut Vec<interstice_sdk::ReducerTableRef>,
                inserts: &mut Vec<interstice_sdk::ReducerTableRef>,
                _updates: &mut Vec<interstice_sdk::ReducerTableRef>,
                _deletes: &mut Vec<interstice_sdk::ReducerTableRef>,
            ) {
                inserts.push(interstice_sdk::ReducerTableRef {
                    node_selection: interstice_sdk::NodeSelection::Current,
                    module_selection: interstice_sdk::ModuleSelection::Current,
                    table_name: #table_name.to_string(),
                });
            }
        }

        impl interstice_sdk::ReducerCapPiece for #update_cap {
            fn extend_reducer_schema(
                _reads: &mut Vec<interstice_sdk::ReducerTableRef>,
                _inserts: &mut Vec<interstice_sdk::ReducerTableRef>,
                updates: &mut Vec<interstice_sdk::ReducerTableRef>,
                _deletes: &mut Vec<interstice_sdk::ReducerTableRef>,
            ) {
                updates.push(interstice_sdk::ReducerTableRef {
                    node_selection: interstice_sdk::NodeSelection::Current,
                    module_selection: interstice_sdk::ModuleSelection::Current,
                    table_name: #table_name.to_string(),
                });
            }
        }

        impl interstice_sdk::ReducerCapPiece for #delete_cap {
            fn extend_reducer_schema(
                _reads: &mut Vec<interstice_sdk::ReducerTableRef>,
                _inserts: &mut Vec<interstice_sdk::ReducerTableRef>,
                _updates: &mut Vec<interstice_sdk::ReducerTableRef>,
                deletes: &mut Vec<interstice_sdk::ReducerTableRef>,
            ) {
                deletes.push(interstice_sdk::ReducerTableRef {
                    node_selection: interstice_sdk::NodeSelection::Current,
                    module_selection: interstice_sdk::ModuleSelection::Current,
                    table_name: #table_name.to_string(),
                });
            }
        }

        impl interstice_sdk::QueryCapPiece for #read_cap {
            fn extend_query_schema(reads: &mut Vec<interstice_sdk::ReducerTableRef>) {
                reads.push(interstice_sdk::ReducerTableRef {
                    node_selection: interstice_sdk::NodeSelection::Current,
                    module_selection: interstice_sdk::ModuleSelection::Current,
                    table_name: #table_name.to_string(),
                });
            }
        }

        impl Into<interstice_sdk::Row> for #struct_ident {
            fn into(self) -> interstice_sdk::Row{
                Row {
                    primary_key: self.#pk_ident.into(),
                    entries: vec![#(self.#entry_fields.clone().into()),*],
                }
            }
        }

        impl From<interstice_sdk::Row> for #struct_ident {
            fn from(row: interstice_sdk::Row) -> #struct_ident{
                let mut row_entries = row.entries.into_iter();
                #struct_ident {
                    #pk_ident: row.primary_key.try_into().unwrap(),
                    #(
                        #entry_fields: row_entries.next().unwrap().try_into().unwrap(),
                    )*
                }
            }
        }

        fn #schema_fn() -> interstice_sdk::TableSchema {
            interstice_sdk::TableSchema {
                name: #table_name.to_string(),
                type_name: #struct_name.to_string(),
                visibility: #visibility,
                fields: vec![#(#schema_fields),*],
                primary_key: interstice_sdk::FieldDef {
                    name: #pk_name.to_string(),
                    field_type: #pk_type.into(),
                },
                primary_key_auto_inc: #pk_auto_inc,
                indexes: vec![#(#index_schemas),*],
                persistence: #persistence,
            }
        }
        #[interstice_sdk::init]
        fn #register_fn() {
            interstice_sdk::registry::register_table(#schema_fn);
        }

        pub struct #table_edit_handle_struct<Caps>{
            _caps: std::marker::PhantomData<Caps>,
        }

        impl<Caps> #table_edit_handle_struct<Caps>{
            pub fn insert(&self, row: #struct_ident) -> Result<#struct_ident, String>
            where
                Caps: interstice_sdk::CanInsert<#struct_ident>,
            {
                interstice_sdk::host_calls::insert_row(
                    #table_name,
                    row.into(),
                )
                .map(|row| row.into())
            }

            pub fn update(&self, row: #struct_ident) -> Result<(), String>
            where
                Caps: interstice_sdk::CanUpdate<#struct_ident>,
            {
                interstice_sdk::host_calls::update_row(
                    #table_name,
                    row.into(),
                )
            }

            pub fn delete(&self, primary_key: #pk_type_ident) -> Result<(), String>
            where
                Caps: interstice_sdk::CanDelete<#struct_ident>,
            {
                interstice_sdk::host_calls::delete_row(
                    #table_name,
                    TryInto::<interstice_sdk::IndexKey>::try_into(Into::<interstice_sdk::IntersticeValue>::into(primary_key)).expect("Failed to convert IntersticeValue to IndexKey"),
                )
            }

            pub fn clear(&self) -> Result<(), String>
            where
                Caps: interstice_sdk::CanDelete<#struct_ident>,
            {
                interstice_sdk::host_calls::clear_table(
                    interstice_sdk::ModuleSelection::Current,
                    #table_name,
                )
            }

        }

        impl<Caps> #table_edit_handle_struct<Caps> {
            #read_table_imp
        }

        pub struct #table_read_handle_struct<Caps>{
            _caps: std::marker::PhantomData<Caps>,
        }

        impl<Caps> #table_read_handle_struct<Caps> {
            #read_table_imp
        }

        impl<Caps> IntoIterator for #table_edit_handle_struct<Caps>
        where
            Caps: interstice_sdk::CanRead<#struct_ident>,
        {
            type Item = #struct_ident;
            type IntoIter = std::vec::IntoIter<#struct_ident>;

            fn into_iter(self) -> Self::IntoIter {
                self.scan().into_iter()
            }
        }

        impl<Caps> IntoIterator for #table_read_handle_struct<Caps>
        where
            Caps: interstice_sdk::CanRead<#struct_ident>,
        {
            type Item = #struct_ident;
            type IntoIter = std::vec::IntoIter<#struct_ident>;

            fn into_iter(self) -> Self::IntoIter {
                self.scan().into_iter()
            }
        }


        pub trait #has_table_edit_handle_trait<Caps> {
            fn #get_table_edit_handle_fn(&self) -> #table_edit_handle_struct<Caps>;
        }

        impl<Caps> #has_table_edit_handle_trait<Caps> for interstice_sdk::ReducerContextCurrentModuleTables<Caps> {
            fn #get_table_edit_handle_fn(&self) -> #table_edit_handle_struct<Caps> {
                return #table_edit_handle_struct { _caps: std::marker::PhantomData }
            }
        }


        pub trait #has_table_read_handle_trait<Caps> {
            fn #get_table_read_handle_fn(&self) -> #table_read_handle_struct<Caps>;
        }

        impl<Caps> #has_table_read_handle_trait<Caps> for interstice_sdk::QueryContextCurrentModuleTables<Caps> {
            fn #get_table_read_handle_fn(&self) -> #table_read_handle_struct<Caps> {
                return #table_read_handle_struct { _caps: std::marker::PhantomData }
            }
        }


    }
    .into()
}
