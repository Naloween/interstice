use quote::quote;
use syn::{Expr, Ident, Meta, punctuated::Punctuated, token::Comma};

use interstice_sdk_core::module_toml::{module_schema_from_toml_str, node_schema_from_toml_str};
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::Path,
};
use toml::Value as TomlValue;

use crate::path_segments::segments_from_dotted_str;

pub fn get_register_subscription_function(
    reducer_ident: Ident,
    attributes: Punctuated<Meta, Comma>,
) -> (proc_macro2::TokenStream, bool) {
    // load available bindings from src/bindings to validate node/module/table names
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default();
    let mut current_module_name = String::new();
    if let Ok(ct) = fs::read_to_string(Path::new(&manifest_dir).join("Cargo.toml")) {
        if let Ok(v) = toml::from_str::<TomlValue>(&ct) {
            if let Some(pkg) = v.get("package").and_then(|p| p.get("name")).and_then(|n| n.as_str()) {
                current_module_name = pkg.to_lowercase();
            }
        }
    }

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
        let bindings_dir = Path::new(&manifest_dir).join("src").join("bindings");
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

                        let segments = match segments_from_dotted_str(&content, litstr.span()) {
                            Err(e) => return Some(e.into_compile_error()),
                            Ok(s) => s,
                        };

                        let mut node_names: HashSet<String> = HashSet::new();
                        let mut module_names: HashMap<String, HashSet<String>> = HashMap::new();
                        let mut module_tables: HashMap<(String, String), HashSet<String>> = HashMap::new();

                        module_names.insert("".into(), HashSet::new()); // Current node represented with empty name
                        
                        // Bindings
                        if bindings_dir.exists() {
                            if let Ok(entries) = fs::read_dir(bindings_dir) {
                                for entry in entries.flatten() {
                                    let path = entry.path();
                                    if path.extension().and_then(|s| s.to_str()) == Some("toml") {
                                        if let Ok(content) = fs::read_to_string(&path) {
                                            if let Ok(module_schema) = module_schema_from_toml_str(&content) {
                                                let mname = module_schema.name.to_lowercase();
                                                module_names.get_mut("").unwrap().insert(mname.clone());
                                                let mut set = HashSet::new();
                                                for t in module_schema.tables {
                                                    set.insert(t.name.to_lowercase());
                                                }
                                                module_tables.insert(("".into(), mname), set);
                                            } else if let Ok(node_schema) = node_schema_from_toml_str(&content) {
                                                let nname = node_schema.name.to_lowercase();
                                                node_names.insert(nname.clone());
                                                module_names.insert(nname.clone(), HashSet::new());
                                                for module in node_schema.modules {
                                                    let mname = module.name.to_lowercase();
                                                    module_names.get_mut(&nname).unwrap().insert(mname.clone());
                                                    let mut set = HashSet::new();
                                                    for t in module.tables {
                                                        set.insert(t.name.to_lowercase());
                                                    }
                                                    module_tables.insert((nname.clone(), mname), set.clone());
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }

                        // Current module (not working)
                        // module_names.get_mut("").unwrap().insert(current_module_name.clone());
                        // module_tables.insert(("".into(), current_module_name.clone()), HashSet::new());
                        // let current_module_tables = interstice_sdk_core::registry::collect_tables();
                        // for table in current_module_tables {
                        //     module_tables.get_mut(&("".into(), current_module_name.clone())).unwrap().insert(table.name);
                        // }

                        if segments.len() == 2 || segments.len() == 3 || segments.len() == 4 {

                            let (node_selection, module_name, table_name, event_name) = if segments.len() == 2 {
                                let node_selection = quote! {interstice_sdk::NodeSelection::Current};
                                let module_name = &current_module_name;
                                let table_name  = &segments[0];
                                let event_name  = &segments[1];
                                // Current module table's check not implemented
                                // let tbls = module_tables.get(&("".into(), current_module_name.clone())).unwrap();
                                // if !tbls.contains(table_name) {
                                //     let msg = format!(
                                //         "Subscription table '{}' not found in current module '{}'. Available tables: {:?}",
                                //         table_name, current_module_name, &tbls
                                //     );
                                //     return Some(syn::Error::new(litstr.span(), msg).into_compile_error());
                                // }
                                

                                (node_selection, module_name, table_name, event_name)
                            } else if segments.len() == 3 {
                                let node_selection = quote! {interstice_sdk::NodeSelection::Current};
                                let module_name = &segments[0];
                                let table_name  = &segments[1];
                                let event_name  = &segments[2];

                                if module_name != &current_module_name && !module_names.get("").unwrap().contains(module_name) {
                                    let msg = format!(
                                        "Subscription module '{}' not found in src/bindings; add a module schema TOML or correct the name",
                                        module_name
                                    );
                                    return Some(syn::Error::new(litstr.span(), msg).into_compile_error());
                                }
                                if let Some(tbls) = module_tables.get(&("".into(), module_name.clone())) {
                                    if !tbls.contains(table_name) {
                                        let msg = format!(
                                            "Subscription table '{}' not found in module '{}'",
                                            table_name, module_name
                                        );
                                        return Some(syn::Error::new(litstr.span(), msg).into_compile_error());
                                    }
                                }
                                (node_selection, module_name, table_name, event_name)
                            } else {
                                let node_name = &segments[0];
                                let node_selection = quote! {interstice_sdk::NodeSelection::Other(#node_name.to_string())};
                                let module_name = &segments[1];
                                let table_name  = &segments[2];
                                let event_name  = &segments[3];

                                if !node_names.contains(node_name) {
                                    let msg = format!(
                                        "Subscription node '{}' not found in src/bindings; add a node schema TOML or correct the name",
                                        node_name
                                    );
                                    return Some(syn::Error::new(litstr.span(), msg).into_compile_error());
                                }
                                if !module_names.get(node_name).unwrap().contains(module_name) {
                                    let msg = format!(
                                        "Subscription module '{}' not found for node '{}'; check src/bindings",
                                        module_name, node_name
                                    );
                                    return Some(syn::Error::new(litstr.span(), msg).into_compile_error());
                                }
                                if let Some(tbls) = module_tables.get(&(node_name.clone(), module_name.clone())) {
                                    if !tbls.contains(table_name) {
                                        let msg = format!(
                                            "Subscription table '{}' not found in module '{}' on node '{}'",
                                            table_name, module_name, node_name
                                        );
                                        return Some(syn::Error::new(litstr.span(), msg).into_compile_error());
                                    }
                                }
                                (node_selection, module_name, table_name, event_name)
                            };

                            match event_name.as_str() {
                                "insert" => {
                                    use_table_subscription = true;
                                    return Some(
                                            quote! {
                                                interstice_sdk::SubscriptionSchema {
                                                    reducer_name: stringify!(#reducer_ident).to_string(),
                                                    event: interstice_sdk::SubscriptionEventSchema::Insert {
                                                        node_selection: #node_selection,
                                                        module_name: #module_name.to_string(),
                                                        table_name: #table_name.to_string(),
                                                    }
                                                }
                                            })
                                }
                                "update" => {
                                    use_table_subscription = true;
                                    return Some(
                                            quote! {
                                                interstice_sdk::SubscriptionSchema {
                                                    reducer_name: stringify!(#reducer_ident).to_string(),
                                                    event: interstice_sdk::SubscriptionEventSchema::Update {
                                                        node_selection: #node_selection,
                                                        module_name: #module_name.to_string(),
                                                        table_name: #table_name.to_string(),
                                                    }
                                                }
                                            })
                                }
                                "delete" => {
                                    use_table_subscription = true;
                                    return Some(
                                            quote! {
                                                interstice_sdk::SubscriptionSchema {
                                                    reducer_name: stringify!(#reducer_ident).to_string(),
                                                    event: interstice_sdk::SubscriptionEventSchema::Delete {
                                                        node_selection: #node_selection,
                                                        module_name: #module_name.to_string(),
                                                        table_name: #table_name.to_string(),
                                                    }
                                                }
                                            })
                                }
                                "sync" | "table_sync" => {
                                    if segments.len() != 4 {
                                        let msg = "Replica sync event must use '[node].[module].[table].sync'";
                                        return Some(syn::Error::new(litstr.span(), msg).into_compile_error());
                                    }
                                    let node_name = &segments[0];
                                    let module_name = if let Some(module_name) = segments.get(1) {
                                        quote! {interstice_sdk::ModuleSelection::Other(#module_name.to_string())}
                                    } else {
                                        quote! {interstice_sdk::ModuleSelection::Current}
                                    };
                                    return Some(
                                        quote! {
                                            interstice_sdk::SubscriptionSchema {
                                                reducer_name: stringify!(#reducer_ident).to_string(),
                                                event: interstice_sdk::SubscriptionEventSchema::ReplicaSync {
                                                    node_name: #node_name.to_string(),
                                                    module_name: #module_name.to_string(),
                                                    table_name: #table_name.to_string(),
                                                }
                                            }
                                        }
                                    )
                                }
                                other => {
                                    let msg = format!(
                                        "Event name not recognized. Expected 'insert', 'update', 'delete' or 'sync'. Got '{}'",
                                        other
                                    );
                                    return Some(syn::Error::new(litstr.span(), msg).into_compile_error());
                                }
                            }
                        } else if segments.len() == 1 {
                            match segments[0].as_str() {
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
                                "audio_output" => {
                                    return Some(
                                            quote! {
                                                interstice_sdk::SubscriptionSchema {
                                                    reducer_name: stringify!(#reducer_ident).to_string(),
                                                    event: interstice_sdk::SubscriptionEventSchema::AudioOutput
                                                }
                                            }
                                        );
                                },
                                "audio_input" => {
                                    return Some(
                                            quote! {
                                                interstice_sdk::SubscriptionSchema {
                                                    reducer_name: stringify!(#reducer_ident).to_string(),
                                                    event: interstice_sdk::SubscriptionEventSchema::AudioInput
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
                                "module_load" => {
                                    return Some(
                                            quote! {
                                                interstice_sdk::SubscriptionSchema {
                                                    reducer_name: stringify!(#reducer_ident).to_string(),
                                                    event: interstice_sdk::SubscriptionEventSchema::ModuleLoad
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
                                "connect" => {
                                    return Some(
                                            quote! {
                                                interstice_sdk::SubscriptionSchema {
                                                    reducer_name: stringify!(#reducer_ident).to_string(),
                                                    event: interstice_sdk::SubscriptionEventSchema::Connect
                                                }
                                            }
                                        );
                                },
                                "disconnect" => {
                                    return Some(
                                            quote! {
                                                interstice_sdk::SubscriptionSchema {
                                                    reducer_name: stringify!(#reducer_ident).to_string(),
                                                    event: interstice_sdk::SubscriptionEventSchema::Disconnect
                                                }
                                            }
                                        );
                                },
                                _ => {
                                    return Some(
                                        syn::Error::new_spanned(
                                            litstr,
                                            "Expected 'init', 'load', 'input', 'audio_output', 'audio_input', 'render', 'module_load', 'module_remove', 'connect', 'disconnect', 'file:<path>', 'file_recursive:<path>' or formats: '[module].[table].[event]' or '[node].[module].[table].[event]'",
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
                        "Expected 'init', 'load', 'input', 'audio_output', 'audio_input', 'render', 'module_load', 'module_remove', 'connect', 'disconnect', 'file:<path>', 'file_recursive:<path>' or formats: '[module].[table].[event]' or '[node].[module].[table].[event]'",
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
