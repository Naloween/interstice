use quote::quote;
use syn::{Expr, Ident, Meta, punctuated::Punctuated, token::Comma};

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
                        if let Some(path) = content.strip_prefix("file_recursive:") {
                            return Some(
                                quote! {
                                    interstice_sdk::SubscriptionSchema {
                                        reducer_name: stringify!(#reducer_ident).to_string(),
                                        event: interstice_sdk::SubscriptionEventSchema::File {
                                            path: #path.to_string(),
                                            recursive: true,
                                        }
                                    }
                                }
                            );
                        }
                        if let Some(path) = content.strip_prefix("file:") {
                            return Some(
                                quote! {
                                    interstice_sdk::SubscriptionSchema {
                                        reducer_name: stringify!(#reducer_ident).to_string(),
                                        event: interstice_sdk::SubscriptionEventSchema::File {
                                            path: #path.to_string(),
                                            recursive: false,
                                        }
                                    }
                                }
                            );
                        }

                        let segments: Vec<_> = content.split('.').collect();

                        if segments.len() == 3 || segments.len() == 4 {
                            use_table_subscription = true;

                            let (node_selection, module_name, table_name, event_name) = if segments.len() == 3 {
                                let node_selection = quote! {interstice_sdk::NodeSelection::Current};
                                let module_name = segments[0];
                                let table_name  = segments[1];
                                let event_name  = segments[2];
                                (node_selection, module_name, table_name, event_name)
                            } else {
                                let node_name = segments[0];
                                let node_selection = quote! {interstice_sdk::NodeSelection::Other(#node_name.to_string())};
                                let module_name = segments[1];
                                let table_name  = segments[2];
                                let event_name  = segments[3];
                                (node_selection, module_name, table_name, event_name)
                            };

                            match event_name.to_string().as_str() {
                                "insert" => { return Some(
                                            quote! {
                                                interstice_sdk::SubscriptionSchema {
                                                    reducer_name: stringify!(#reducer_ident).to_string(),
                                                    event: interstice_sdk::SubscriptionEventSchema::Insert {
                                                        node_selection: #node_selection,
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
                                                        node_selection: #node_selection,
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
                                                        node_selection: #node_selection,
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
                                "load" => {
                                    return Some(
                                            quote! {
                                                interstice_sdk::SubscriptionSchema {
                                                    reducer_name: stringify!(#reducer_ident).to_string(),
                                                    event: interstice_sdk::SubscriptionEventSchema::Load
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
                                "module_publish" => {
                                    return Some(
                                            quote! {
                                                interstice_sdk::SubscriptionSchema {
                                                    reducer_name: stringify!(#reducer_ident).to_string(),
                                                    event: interstice_sdk::SubscriptionEventSchema::ModulePublish
                                                }
                                            }
                                        );
                                },
                                "module_remove" => {
                                    return Some(
                                            quote! {
                                                interstice_sdk::SubscriptionSchema {
                                                    reducer_name: stringify!(#reducer_ident).to_string(),
                                                    event: interstice_sdk::SubscriptionEventSchema::ModuleRemove
                                                }
                                            }
                                        );
                                },
                                _ => {
                                    return Some(
                                        syn::Error::new_spanned(
                                            litstr,
                                            "Expected 'init', 'load', 'input', 'render', 'module_publish', 'module_remove', 'file:<path>', 'file_recursive:<path>' or formats: '[module].[table].[event]' or '[node].[module].[table].[event]'",
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
                        "Expected 'init', 'load', 'input', 'render', 'module_publish', 'module_remove', 'file:<path>', 'file_recursive:<path>' or formats: '[module].[table].[event]' or '[node].[module].[table].[event]'",
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
