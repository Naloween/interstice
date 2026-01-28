use interstice_abi::get_reducer_wrapper_name;
use proc_macro::TokenStream;
use quote::{format_ident, quote, ToTokens};
use syn::{parse_macro_input, ItemFn, LitInt, Meta};

#[proc_macro_attribute]
pub fn init(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let mut input = parse_macro_input!(item as ItemFn);
    let name = &input.sig.ident;

    // 1. Correctly set the ABI to "C" inside the function signature
    // This ensures 'pub extern "C" fn' order is correct automatically
    input.sig.abi = Some(syn::Abi {
        extern_token: syn::token::Extern::default(),
        name: Some(syn::LitStr::new("C", proc_macro2::Span::call_site())),
    });

    let init_static_name = format_ident!("__INTERSTICE_INIT_{}", name.to_string().to_uppercase());

    quote! {
        #[unsafe(no_mangle)]
        #input

        #[used]
        #[link_section = ".init_array"]
        static #init_static_name: extern "C" fn() = #name;
    }
    .into()
}

#[proc_macro_attribute]
pub fn table(attr: TokenStream, item: TokenStream) -> TokenStream {
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

#[proc_macro_attribute]
pub fn reducer(attr: TokenStream, item: TokenStream) -> TokenStream {
    let input_fn = parse_macro_input!(item as ItemFn);
    let reducer_name = &input_fn.sig.ident;
    let wrapper_name = syn::Ident::new(
        &get_reducer_wrapper_name(&reducer_name.to_string()),
        reducer_name.span(),
    );
    let reducer_schema_fn = syn::Ident::new(
        &format!("interstice_{}_schema", reducer_name),
        reducer_name.span(),
    );
    let register_reducer_schema_fn = syn::Ident::new(
        &format!("interstice_register_{}_schema", reducer_name),
        reducer_name.span(),
    );
    let subscription_schema_fn = syn::Ident::new(
        &format!("interstice_{}_subscription_schema", reducer_name),
        reducer_name.span(),
    );
    let register_subscription_schema_fn = syn::Ident::new(
        &format!("interstice_register_{}_subscription_schema", reducer_name),
        reducer_name.span(),
    );

    let arg_names: Vec<_> = input_fn
        .sig
        .inputs
        .iter()
        .map(|arg| match arg {
            syn::FnArg::Typed(pat) => &pat.pat,
            _ => panic!("Unexpected argument type"),
        })
        .collect();

    let arg_types: Vec<_> = input_fn
        .sig
        .inputs
        .iter()
        .map(|arg| match arg {
            syn::FnArg::Typed(pat) => &pat.ty,
            _ => panic!("Unexpected argument type"),
        })
        .collect();

    let arg_count = arg_names.len();
    if arg_count == 0 {
        return quote! {compile_error!("The reducer should have at least a 'ReducerContext' first argument");}.into();
    }
    if arg_types[0].to_token_stream().to_string() != "ReducerContext" {
        return quote! {compile_error!("The reducer should have the first argument of type 'ReducerContext'");}.into();
    }

    let args = (0..arg_count - 1).map(|i| {
        let index = LitInt::new(&i.to_string(), proc_macro2::Span::call_site());
        quote! { interstice_args_vec[#index].clone().into() }
    });

    let schema_entries = arg_names.iter().skip(1).zip(arg_types.iter().skip(1)).map(
        |(arg_name, arg_type)| {
            let arg_name_str = quote! { #arg_name }.to_string();
            let arg_type_str = quote! { #arg_type }.to_string();
            quote! {
                interstice_sdk::FieldDef {
                    name: #arg_name_str.to_string(),
                    field_type: interstice_sdk::IntersticeType::from_str(#arg_type_str).unwrap(),
                }
            }
        },
    );

    let return_type = match &input_fn.sig.output {
        syn::ReturnType::Default => quote! { interstice_sdk::IntersticeType::Void },
        syn::ReturnType::Type(_, ty) => {
            let ty = ty.to_token_stream().to_string();
            quote! {
               interstice_sdk::IntersticeType::from_str(#ty).unwrap()
            }
        }
    };

    // Add potential subscription
    let attributes = syn::parse_macro_input!(
        attr with syn::punctuated::Punctuated::<Meta, syn::Token![,]>::parse_terminated
    );
    let subscription = attributes.iter().find_map(|arg| {
        if let Meta::NameValue(nv) = arg {
            if nv.path.is_ident("on") {
                let sub_str = nv.value.to_token_stream().to_string();
                let mut parsed_sub: Vec<&str> = sub_str.split(".").collect();
                if parsed_sub.len() == 3 {
                    let event_name = parsed_sub.pop().unwrap();
                    let table_name = parsed_sub.pop().unwrap();
                    let module_name = parsed_sub.pop().unwrap();
                    return Some(quote! {
                            interstice_sdk::SubscriptionSchema {
                            module_name: #module_name.to_string(),
                            table_name: #table_name.to_string(),
                            reducer_name: stringify!(#reducer_name).to_string(),
                            event: #event_name.into(),
                        }
                    });
                }
            }
        }
        None
    });

    let register_subscription = if let Some(subscription_schema) = subscription {
        quote! {
            fn #subscription_schema_fn() -> interstice_sdk::SubscriptionSchema {
                #subscription_schema
            }

            #[interstice_sdk::init]
            fn #register_subscription_schema_fn() {
                interstice_sdk::registry::register_subscription(#subscription_schema_fn);
            }
        }
    } else {
        quote! {}
    };

    // Generate wrapper and schema
    quote! {
        #input_fn

        #[no_mangle]
        pub extern "C" fn #wrapper_name(ptr: i32, len: i32) -> i64 {
            let bytes = unsafe { std::slice::from_raw_parts(ptr as *const u8, len as usize) };
            let (reducer_context, interstice_args): (interstice_sdk::ReducerContext, interstice_sdk::IntersticeValue) = interstice_sdk::decode(bytes).unwrap();
            let interstice_args_vec = match interstice_args {
                interstice_sdk::IntersticeValue::Vec(v) => v,
                _ => panic!("Expected Vec<IntersticeValue> as reducer_wrapper input, got {:?}", interstice_args),
            };
            if interstice_args_vec.len() != #arg_count - 1 { panic!("Expected {} reducer arguments, got {}", #arg_count-1, interstice_args_vec.len()) }

            let res: interstice_sdk::IntersticeValue = #reducer_name(reducer_context, #(#args),*).into();

            let bytes = interstice_sdk::encode(&res).unwrap();
            let out_ptr = alloc(bytes.len() as i32);
            unsafe {
                std::slice::from_raw_parts_mut(out_ptr as *mut u8, bytes.len()).copy_from_slice(&bytes);
            }
            return interstice_sdk::pack_ptr_len(out_ptr, bytes.len() as i32);
        }


        fn #reducer_schema_fn() -> interstice_sdk::ReducerSchema {
            interstice_sdk::ReducerSchema::new(
                stringify!(#reducer_name),
                vec![#(#schema_entries),*],
                #return_type,
            )
        }

        #[interstice_sdk::init]
        fn #register_reducer_schema_fn() {
            interstice_sdk::registry::register_reducer(#reducer_schema_fn);
        }

        #register_subscription
    }
    .into()
}

#[proc_macro_derive(IntersticeType)]
pub fn derive_interstice_type(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as syn::DeriveInput);

    let struct_name = input.ident;
    let struct_name_str = struct_name.to_string();
    let type_def_fn = syn::Ident::new(
        &format!("get_type_def_{}", &struct_name_str.to_lowercase()),
        struct_name.span(),
    );
    let register_type_def_fn = syn::Ident::new(
        &format!("register_get_type_def_{}", &struct_name_str.to_lowercase()),
        struct_name.span(),
    );

    let fields = match input.data {
        syn::Data::Struct(s) => s.fields,
        _ => panic!("IntersticeType can only be derived for structs"),
    };

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

    // Convert Rust type → IntersticeType expression
    let interstice_type_exprs: Vec<_> = field_types
        .iter()
        .map(|ty| ty.to_token_stream().to_string())
        .collect();

    let field_name_strings: Vec<String> = field_names.iter().map(|f| f.to_string()).collect();

    quote! {
        impl Into<interstice_sdk::IntersticeValue> for #struct_name {
            fn into(self) -> interstice_sdk::IntersticeValue {
                interstice_sdk::IntersticeValue::Struct {
                    name: #struct_name_str.to_string(),
                    fields: vec![
                        #(
                            interstice_sdk::Field {
                                name: #field_name_strings.to_string(),
                                value: self.#field_names.into(),
                            }
                        ),*
                    ],
                }
            }
        }

        impl Into<#struct_name> for interstice_sdk::IntersticeValue {

            fn into(self) -> #struct_name {
                match self {
                    interstice_sdk::IntersticeValue::Struct { name, fields } if name == #struct_name_str => {
                        let mut map = std::collections::HashMap::new();
                        for field in fields {
                            map.insert(field.name, field.value);
                        }

                        #struct_name {
                            #(
                                #field_names: map.remove(#field_name_strings).unwrap().into(),
                                    // .ok_or_else(|| interstice_sdk::IntersticeAbiError::ConversionError(
                                    //     format!("Missing field {}", #field_name_strings)
                                    // ))?
                                    // .try_into().map_err(|err| interstice_sdk::IntersticeAbiError::ConversionError(
                                    //     format!("Bad field {}", #field_name_strings)
                                    // ))?,
                            )*
                        }
                    }
                    _ => panic!("Expected struct {}", #struct_name_str),
                }
            }
        }

        fn #type_def_fn() -> interstice_sdk::IntersticeTypeDef {
            interstice_sdk::IntersticeTypeDef::Struct {
                name: #struct_name_str.to_string(),
                fields: vec![
                    #(
                        FieldDef {
                            name: #field_name_strings.to_string(),
                            field_type: interstice_sdk::IntersticeType::from_str(#interstice_type_exprs).expect("Couldn't convert field type string to IntersticeType"),
                        }
                    ),*
                ],
            }
        }

        #[interstice_sdk::init]
        fn #register_type_def_fn() {
            interstice_sdk::registry::register_type_def(#type_def_fn);
        }
    }
    .into()
}
