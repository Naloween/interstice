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
    let struct_name = struct_ident.to_string().to_lowercase();

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
            return syn::Error::new_spanned(&input, "tables must be structs with named fields")
                .to_compile_error()
                .into();
        }
    };

    let mut primary_key: Option<(String, proc_macro2::TokenStream)> = None;
    let mut entries = Vec::new();
    for field in fields.iter() {
        let field_name = field.ident.as_ref().unwrap().to_string();
        let field_ty_str = field.ty.to_token_stream().to_string();
        let field_ty = quote! { Into::<interstice_sdk::IntersticeType>::into(#field_ty_str)};

        let is_pk = field
            .attrs
            .iter()
            .any(|attr| attr.path().is_ident("primary_key"));

        if is_pk {
            if primary_key.is_some() {
                return syn::Error::new_spanned(&input, "Only one #[primary_key] field is allowed")
                    .to_compile_error()
                    .into();
            }

            primary_key = Some((field_name, field_ty));
        } else {
            entries.push(quote! {
                interstice_sdk::EntrySchema {
                    name: #field_name.to_string(),
                    value_type: #field_ty,
                }
            });
        }
    }
    let (pk_name, pk_type) = match primary_key {
        Some(pk) => pk,
        None => {
            return syn::Error::new_spanned(&input, "A #[primary_key] field is required")
                .to_compile_error()
                .into();
        }
    };

    // Generate the output struct without the primary key attribute
    let mut output_struct = input.clone();
    if let syn::Fields::Named(fields) = &mut output_struct.fields {
        for field in fields.named.iter_mut() {
            field.attrs.retain(|a| !a.path().is_ident("primary_key"));
        }
    }

    // Generate the schema function and registration function
    let schema_fn = syn::Ident::new(
        &format!("interstice_{}_schema", struct_name),
        struct_ident.span(),
    );
    let register_fn = syn::Ident::new(
        &format!("interstice_register_{}_table", struct_name),
        struct_ident.span(),
    );

    quote! {
        #output_struct

        fn #schema_fn() -> interstice_sdk::TableSchema {
            interstice_sdk::TableSchema {
                name: #struct_name.to_string(),
                visibility: #visibility,
                entries: vec![#(#entries),*],
                primary_key: interstice_sdk::EntrySchema {
                    name: #pk_name.to_string(),
                    value_type: #pk_type.into(),
                },
            }
        }
        #[interstice_sdk::init]
        fn #register_fn() {
            interstice_sdk::register_table(#schema_fn);
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

    let args = (0..arg_count).map(|i| {
        let index = LitInt::new(&i.to_string(), proc_macro2::Span::call_site());
        quote! { interstice_args_vec[#index].clone().into() }
    });

    let schema_entries = arg_names.iter().zip(arg_types).map(|(arg_name, arg_type)| {
        let arg_name_str = quote! { #arg_name }.to_string();
        let arg_type_str = quote! { #arg_type }.to_string();
        quote! {
            interstice_sdk::EntrySchema {
                name: #arg_name_str.to_string(),
                value_type: #arg_type_str.to_string().into(),
            }
        }
    });

    let return_type = match &input_fn.sig.output {
        syn::ReturnType::Default => quote! { interstice_sdk::IntersticeType::Void },
        syn::ReturnType::Type(_, ty) => {
            let ty = ty.to_token_stream().to_string();
            quote! {
               #ty.to_string().into()
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
                interstice_sdk::register_subscription(#subscription_schema_fn);
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
            let interstice_args: interstice_sdk::IntersticeValue = interstice_sdk::decode(bytes).unwrap();
            let interstice_args_vec = match interstice_args {
                interstice_sdk::IntersticeValue::Vec(v) => v,
                _ => panic!("Expected Vec<IntersticeValue>"),
            };
            if interstice_args_vec.len() != #arg_count { panic!("Expected {} arguments, got {}", #arg_count, interstice_args_vec.len()) }

            let res: interstice_sdk::IntersticeValue = #reducer_name(#(#args),*).into();

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
            interstice_sdk::register_reducer(#reducer_schema_fn);
        }

        #register_subscription
    }.into()
}
