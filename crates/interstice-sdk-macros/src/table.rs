use proc_macro::TokenStream;
use quote::{ToTokens, quote};
use syn::Meta;

use crate::index_key::validate_index_key_type;

/// Rows kept by a bare `#[table(event)]`.
const DEFAULT_EVENT_RETENTION: u64 = 256;

/// Parse a `history` duration: `"all"`, or a number with a unit `s`, `m`,
/// `h` or `d`. Returns the retention in ms (`None` for `"all"`).
fn parse_history(text: &str) -> Result<Option<u64>, String> {
    let text = text.trim();
    if text == "all" {
        return Ok(None);
    }
    let unit_ms = match text.chars().last() {
        Some('s') => 1_000,
        Some('m') => 60_000,
        Some('h') => 3_600_000,
        Some('d') => 86_400_000,
        _ => return Err("expected `\"all\"` or a duration such as `\"30s\"`, `\"15m\"`, `\"12h\"`, `\"7d\"`".into()),
    };
    match text[..text.len() - 1].parse::<u64>() {
        Ok(n) if n > 0 => Ok(Some(n * unit_ms)),
        _ => Err("the duration must be a positive integer followed by `s`, `m`, `h` or `d`".into()),
    }
}

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
    let mut explicit_persistence = false;
    let mut retention: Option<u64> = None;
    let mut history: Option<u64> = None;
    let mut explicit_history = false;

    for arg in args.iter() {
        match arg {
            Meta::Path(nv) if nv.is_ident("public") => {
                visibility = quote! { interstice_sdk::TableVisibility::Public };
            }
            Meta::Path(nv) if nv.is_ident("private") => {
                visibility = quote! { interstice_sdk::TableVisibility::Private };
            }
            Meta::Path(nv) if nv.is_ident("ephemeral") => {
                persistence = quote! { interstice_sdk::PersistenceKind::Ephemeral };
                explicit_persistence = true;
            }
            Meta::Path(nv) if nv.is_ident("stateful") => {
                persistence = quote! { interstice_sdk::PersistenceKind::Stateful };
                explicit_persistence = true;
            }
            Meta::Path(nv) if nv.is_ident("logged") => {
                persistence = quote! { interstice_sdk::PersistenceKind::Logged };
                explicit_persistence = true;
            }
            // `event`: an ephemeral notification table keeping its newest rows.
            Meta::Path(nv) if nv.is_ident("event") => {
                retention = Some(DEFAULT_EVENT_RETENTION);
            }
            Meta::NameValue(nv) if nv.path.is_ident("event") => {
                let n = match &nv.value {
                    syn::Expr::Lit(syn::ExprLit { lit: syn::Lit::Int(i), .. }) => i.base10_parse::<u64>(),
                    other => Err(syn::Error::new_spanned(other, "expected an integer: `event = 256`")),
                };
                match n {
                    Ok(n) if n > 0 => retention = Some(n),
                    Ok(_) => {
                        return syn::Error::new_spanned(&nv.value, "event tables must keep at least one row")
                            .into_compile_error()
                            .into();
                    }
                    Err(e) => return e.into_compile_error().into(),
                }
            }
            // `history`: how long a logged table keeps its log (DESIGN §6.7).
            Meta::NameValue(nv) if nv.path.is_ident("history") => {
                let parsed = match &nv.value {
                    syn::Expr::Lit(syn::ExprLit { lit: syn::Lit::Str(text), .. }) => {
                        parse_history(&text.value()).map_err(|e| syn::Error::new_spanned(text, e))
                    }
                    other => Err(syn::Error::new_spanned(other, "expected a string: `history = \"7d\"`")),
                };
                match parsed {
                    Ok(ms) => {
                        history = ms;
                        explicit_history = true;
                    }
                    Err(e) => return e.into_compile_error().into(),
                }
            }
            other => {
                return syn::Error::new_spanned(
                    other,
                    "invalid table attribute: expected `public`, `private`, `ephemeral`, `stateful`, `logged`, `event`, `event = N` or `history = \"7d\"`",
                )
                .into_compile_error()
                .into();
            }
        }
    }
    if retention.is_some() {
        if explicit_persistence {
            return quote! { compile_error!("`event` tables are always ephemeral; drop the persistence attribute"); }.into();
        }
        persistence = quote! { interstice_sdk::PersistenceKind::Ephemeral };
    }
    let retention = match retention {
        Some(n) => quote! { Some(#n) },
        None => quote! { None },
    };
    // Only logged tables keep a log, hence a history.
    // (event tables were made ephemeral above).
    let is_logged =
        persistence.to_string() == quote! { interstice_sdk::PersistenceKind::Logged }.to_string();
    if explicit_history && !is_logged {
        return quote! { compile_error!("`history` only applies to logged tables"); }.into();
    }
    let history = match history {
        Some(n) => quote! { Some(#n) },
        None => quote! { None },
    };

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

        let scan_op = |fn_name: &syn::Ident, variant: proc_macro2::TokenStream| {
            quote! {
                pub fn #fn_name(&self, value: #index_ty_ident) -> Result<Vec<#struct_ident>, String>
                where
                    Caps: interstice_sdk::CanRead<#struct_ident>,
                {
                    interstice_sdk::reads::read_index(
                        interstice_sdk::ModuleSelection::Current,
                        #table_name,
                        #index_name,
                        interstice_sdk::IndexQuery::#variant(interstice_sdk::reads::to_index_key(value)?),
                    )
                }
            }
        };

        index_read_methods.push(scan_op(&fn_eq, quote!(Eq)));

        if *unique {
            index_read_methods.push(quote! {
                pub fn #fn_get(&self, value: #index_ty_ident) -> Result<Option<#struct_ident>, String>
                where
                    Caps: interstice_sdk::CanRead<#struct_ident>,
                {
                    Ok(self.#fn_eq(value)?.into_iter().next())
                }
            });
        }

        if *is_btree {
            index_read_methods.push(scan_op(&fn_lt, quote!(Lt)));
            index_read_methods.push(scan_op(&fn_lte, quote!(Lte)));
            index_read_methods.push(scan_op(&fn_gt, quote!(Gt)));
            index_read_methods.push(scan_op(&fn_gte, quote!(Gte)));
            index_read_methods.push(quote! {
                pub fn #fn_range(
                    &self,
                    min: #index_ty_ident,
                    max: #index_ty_ident,
                    include_min: bool,
                    include_max: bool,
                ) -> Result<Vec<#struct_ident>, String>
                where
                    Caps: interstice_sdk::CanRead<#struct_ident>,
                {
                    interstice_sdk::reads::read_index(
                        interstice_sdk::ModuleSelection::Current,
                        #table_name,
                        #index_name,
                        interstice_sdk::IndexQuery::Range {
                            min: interstice_sdk::reads::to_index_key(min)?,
                            max: interstice_sdk::reads::to_index_key(max)?,
                            include_min,
                            include_max,
                        },
                    )
                }
            });
        }
    }

    let read_table_imp = quote! {
        pub fn scan(&self) -> Result<Vec<#struct_ident>, String>
        where
            Caps: interstice_sdk::CanRead<#struct_ident>,
        {
            interstice_sdk::reads::read_scan(interstice_sdk::ModuleSelection::Current, #table_name)
        }

        pub fn get(&self, primary_key: #pk_type_ident) -> Result<Option<#struct_ident>, String>
        where
            Caps: interstice_sdk::CanRead<#struct_ident>,
        {
            interstice_sdk::reads::read_get(interstice_sdk::ModuleSelection::Current, #table_name, primary_key)
        }

        #(#index_read_methods)*
    };

    // Logged tables: read past states from the log (DESIGN §6.7).
    let history_read_imp = if is_logged {
        quote! {
            /// The newest `limit` commits of this table, newest first.
            pub fn commits(&self, limit: u32) -> Result<Vec<interstice_sdk::HistoryCommit>, String>
            where
                Caps: interstice_sdk::CanRead<#struct_ident>,
            {
                interstice_sdk::host_calls::history_commits(#table_name, limit)
            }

            /// The rows of this table at `point`, by primary key.
            pub fn rows_at(&self, point: interstice_sdk::HistoryPoint) -> Result<Vec<#struct_ident>, String>
            where
                Caps: interstice_sdk::CanRead<#struct_ident>,
            {
                interstice_sdk::host_calls::history_rows_at(#table_name, point)?
                    .into_iter()
                    .map(|row| row.try_into())
                    .collect()
            }
        }
    } else {
        quote! {}
    };
    let history_edit_imp = if is_logged {
        quote! {
            /// Restore this table to its state at `point`, as one ordinary
            /// write of this transaction. Returns the `tx` of the last log
            /// entry the point covers (`0` for an empty start).
            pub fn restore(&self, point: interstice_sdk::HistoryPoint) -> Result<u64, String>
            where
                Caps: interstice_sdk::CanInsert<#struct_ident>
                    + interstice_sdk::CanUpdate<#struct_ident>
                    + interstice_sdk::CanDelete<#struct_ident>,
            {
                interstice_sdk::host_calls::history_restore(#table_name, point)
            }
        }
    } else {
        quote! {}
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

        impl TryFrom<interstice_sdk::Row> for #struct_ident {
            type Error = String;
            fn try_from(row: interstice_sdk::Row) -> Result<#struct_ident, String> {
                #[allow(unused_mut, unused_variables)]
                let mut row_entries = row.entries.into_iter();
                Ok(#struct_ident {
                    #pk_ident: row.primary_key.try_into().map_err(|e| format!("{}.{}: {}", #struct_name, stringify!(#pk_ident), e))?,
                    #(
                        #entry_fields: row_entries
                            .next()
                            .ok_or_else(|| format!("{}: row is missing field {}", #struct_name, stringify!(#entry_fields)))?
                            .try_into()
                            .map_err(|e| format!("{}.{}: {}", #struct_name, stringify!(#entry_fields), e))?,
                    )*
                })
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
                retention: #retention,
                history: #history,
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
                .and_then(|row| row.try_into())
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
                    interstice_sdk::reads::to_index_key(primary_key)?,
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

            #history_edit_imp

        }

        impl<Caps> #table_edit_handle_struct<Caps> {
            #read_table_imp
            #history_read_imp
        }

        pub struct #table_read_handle_struct<Caps>{
            _caps: std::marker::PhantomData<Caps>,
        }

        impl<Caps> #table_read_handle_struct<Caps> {
            #read_table_imp
            #history_read_imp
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
