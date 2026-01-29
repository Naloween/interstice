use interstice_abi::get_reducer_wrapper_name;
use proc_macro::TokenStream;
use quote::{quote, ToTokens};
use syn::{parse_macro_input, punctuated::Punctuated, token::Comma, Ident, ItemFn, LitInt, Meta};

pub fn reducer_macro(attr: TokenStream, item: TokenStream) -> TokenStream {
    let input_fn = parse_macro_input!(item as ItemFn);
    let reducer_ident = &input_fn.sig.ident;
    let wrapper_name = syn::Ident::new(
        &get_reducer_wrapper_name(&reducer_ident.to_string()),
        reducer_ident.span(),
    );
    let reducer_schema_fn = syn::Ident::new(
        &format!("interstice_{}_schema", reducer_ident),
        reducer_ident.span(),
    );
    let register_reducer_schema_fn = syn::Ident::new(
        &format!("interstice_register_{}_schema", reducer_ident),
        reducer_ident.span(),
    );
    let subscription_schema_fn = syn::Ident::new(
        &format!("interstice_{}_subscription_schema", reducer_ident),
        reducer_ident.span(),
    );
    let register_subscription_schema_fn = syn::Ident::new(
        &format!("interstice_register_{}_subscription_schema", reducer_ident),
        reducer_ident.span(),
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
    let register_subscription = get_register_subscription_function(
        reducer_ident.clone(),
        attributes,
        subscription_schema_fn,
        register_subscription_schema_fn,
    );

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

            let res: interstice_sdk::IntersticeValue = #reducer_ident(reducer_context, #(#args),*).into();

            let bytes = interstice_sdk::encode(&res).unwrap();
            let out_ptr = alloc(bytes.len() as i32);
            unsafe {
                std::slice::from_raw_parts_mut(out_ptr as *mut u8, bytes.len()).copy_from_slice(&bytes);
            }
            return interstice_sdk::pack_ptr_len(out_ptr, bytes.len() as i32);
        }


        fn #reducer_schema_fn() -> interstice_sdk::ReducerSchema {
            interstice_sdk::ReducerSchema::new(
                stringify!(#reducer_ident),
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

fn get_register_subscription_function(
    reducer_ident: Ident,
    attributes: Punctuated<Meta, Comma>,
    subscription_schema_fn: Ident,
    register_subscription_schema_fn: Ident,
) -> proc_macro2::TokenStream {
    let subscription_error_msg = "You can only subscribe to table events or to the 'init' event. table events are in the formats '[module].[table].[event]' where event can be 'insert', 'update' or 'delete'";

    let subscription = attributes.iter().find_map(|arg| {
        if let Meta::NameValue(nv) = arg {
            if nv.path.is_ident("on") {
                let sub_str = nv.value.to_token_stream().to_string();
                let mut parsed_sub: Vec<&str> = sub_str.split(".").collect();
                if parsed_sub.len() == 3 {
                    let event_name = parsed_sub.pop().unwrap();
                    let table_name = parsed_sub.pop().unwrap();
                    let module_name = parsed_sub.pop().unwrap();
                    match event_name {
                        "insert" => {
                            return Some(
                                quote! {
                                    interstice_sdk::SubscriptionSchema {
                                        reducer_name: stringify!(#reducer_ident).to_string(),
                                        event: interstice_sdk::SubscriptionEventSchema::Insert {
                                            module_name: #module_name.to_string(),
                                            table_name: #table_name.to_string(),
                                        }
                                    }
                                }
                                .into(),
                            )
                        }
                        "update" => {
                            return Some(
                                quote! {
                                    interstice_sdk::SubscriptionSchema {
                                        reducer_name: stringify!(#reducer_ident).to_string(),
                                        event: interstice_sdk::SubscriptionEventSchema::Update {
                                            module_name: #module_name.to_string(),
                                            table_name: #table_name.to_string(),
                                        }
                                    }
                                }
                                .into(),
                            )
                        }
                        "delete" => {
                            return Some(
                                quote! {
                                    interstice_sdk::SubscriptionSchema {
                                        reducer_name: stringify!(#reducer_ident).to_string(),
                                        event: interstice_sdk::SubscriptionEventSchema::Delete {
                                            module_name: #module_name.to_string(),
                                            table_name: #table_name.to_string(),
                                        }
                                    }
                                }
                                .into(),
                            )
                        }
                        _ => return Some(quote! {compile_error!(#subscription_error_msg);}),
                    };
                } else if parsed_sub.len() == 1 {
                    let event_name = parsed_sub.pop().unwrap();
                    if event_name != "init" {
                        return Some(quote! {compile_error!(#subscription_error_msg);});
                    }
                    return Some(quote! {
                            interstice_sdk::SubscriptionSchema {
                            reducer_name: stringify!(#reducer_ident).to_string(),
                            event: interstice_sdk::SubscriptionEventSchema::Init
                        }
                    });
                } else {
                    return Some(quote! {compile_error!(#subscription_error_msg);});
                }
            }
        }
        return None;
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

    return register_subscription;
}
