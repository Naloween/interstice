use quote::quote;
use syn::{punctuated::Punctuated, token::Comma, Expr, Ident, Meta};

pub fn get_register_subscription_function(
    reducer_ident: Ident,
    attributes: Punctuated<Meta, Comma>,
) -> (proc_macro2::TokenStream, bool) {
    let subscription_schema_fn = syn::Ident::new(
        &format!("interstice_{}_subscription_schema", reducer_ident),
        reducer_ident.span(),
    );
    let register_subscription_schema_fn = syn::Ident::new(
        &format!("interstice_register_{}_subscription_schema", reducer_ident),
        reducer_ident.span(),
    );

    let mut use_table_subscription = false;

    let subscription = attributes.iter().find_map(|arg| {

        if let Meta::NameValue(nv) = arg {
            if nv.path.is_ident("on") {
                if let Expr::Lit(expr_lit) = &nv.value {
                    if let syn::Lit::Str(litstr) = &expr_lit.lit {
                        let content = litstr.value();
                        let segments: Vec<_> = content.split('.').collect();

                        if segments.len() == 3 {
                            use_table_subscription = true;
                            let module_name = &segments[0];
                            let table_name  = &segments[1];
                            let event_name  = &segments[2];

                            match event_name.to_string().as_str() {
                                "insert" => { return Some(
                                            quote! {
                                                interstice_sdk::SubscriptionSchema {
                                                    reducer_name: stringify!(#reducer_ident).to_string(),
                                                    event: interstice_sdk::SubscriptionEventSchema::Insert {
                                                        module_name: #module_name.to_string(),
                                                        table_name: #table_name.to_string(),
                                                    }
                                                }
                                            })}
                                "update" => { return Some(
                                            quote! {
                                                interstice_sdk::SubscriptionSchema {
                                                    reducer_name: stringify!(#reducer_ident).to_string(),
                                                    event: interstice_sdk::SubscriptionEventSchema::Update {
                                                        module_name: #module_name.to_string(),
                                                        table_name: #table_name.to_string(),
                                                    }
                                                }
                                            }) }
                                "delete" => { return Some(
                                            quote! {
                                                interstice_sdk::SubscriptionSchema {
                                                    reducer_name: stringify!(#reducer_ident).to_string(),
                                                    event: interstice_sdk::SubscriptionEventSchema::Delete {
                                                        module_name: #module_name.to_string(),
                                                        table_name: #table_name.to_string(),
                                                    }
                                                }
                                            }) }
                                other => {
                                    let msg = format!(
                                        "Event name not recognized. Expected 'insert', 'update' or 'delete'. Got '{}'",
                                        other
                                    );
                                    return Some(syn::Error::new_spanned(event_name, msg).to_compile_error());
                                }
                            }
                        } else if segments.len() == 1 {
                            match segments[0]{
                                "init" => {
                                    return Some(
                                            quote! {
                                                interstice_sdk::SubscriptionSchema {
                                                    reducer_name: stringify!(#reducer_ident).to_string(),
                                                    event: interstice_sdk::SubscriptionEventSchema::Init
                                                }
                                            }
                                        );
                                },
                                "input" => {
                                    return Some(
                                            quote! {
                                                interstice_sdk::SubscriptionSchema {
                                                    reducer_name: stringify!(#reducer_ident).to_string(),
                                                    event: interstice_sdk::SubscriptionEventSchema::Input
                                                }
                                            }
                                        );
                                },
                                "render" => {
                                    return Some(
                                            quote! {
                                                interstice_sdk::SubscriptionSchema {
                                                    reducer_name: stringify!(#reducer_ident).to_string(),
                                                    event: interstice_sdk::SubscriptionEventSchema::Render
                                                }
                                            }
                                        );
                                },
                                _ => {
                                    return Some(
                                        syn::Error::new_spanned(
                                            litstr,
                                            "Expected 'init', 'input', 'render' or format: '[module].[table].[event]'",
                                        )
                                        .to_compile_error()
                                        .into(),
                                    );
                                }
                            }
                        }
                    }
                }
                return Some(
                    syn::Error::new_spanned(
                        &nv.value,
                        "Expected 'init', 'input', 'render' or format: '[module].[table].[event]'",
                    )
                    .to_compile_error()
                    .into(),
                );
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

    return (register_subscription, use_table_subscription);
}
