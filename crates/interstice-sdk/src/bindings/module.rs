use interstice_abi::{ModuleSchema, NodeSelection};
use proc_macro2::{Span, TokenStream};
use quote::quote;
use syn::{Ident, LitStr};

use crate::{
    bindings::{
        query::get_query_code, reducer::get_reducer_code, table::get_table_code,
        type_definition::get_type_definition_code,
    },
    snake_to_camel_case, to_snake_case,
};

pub fn get_module_code(module_schema: ModuleSchema, node_selection: NodeSelection) -> String {
    let span = Span::call_site();
    let module_name = module_schema.name;
    let snake_module_name = to_snake_case(&module_name);
    let camel_module_name = snake_to_camel_case(&snake_module_name);
    let module_handle_ident = Ident::new(&(camel_module_name.clone() + "ModuleHandle"), span);
    let module_tables_ident = Ident::new(&(camel_module_name.clone() + "Tables"), span);
    let module_reducers_ident = Ident::new(&(camel_module_name.clone() + "Reducers"), span);
    let module_queries_ident = Ident::new(&(camel_module_name.clone() + "Queries"), span);
    let has_module_handle_trait_ident = Ident::new(
        &("Has".to_string() + &camel_module_name + "ModuleHandle"),
        span,
    );
    let module_method_ident = Ident::new(&snake_module_name, span);
    let module_inner_ident = Ident::new(&snake_module_name, span);

    let reducer_methods: Vec<TokenStream> = module_schema
        .reducers
        .into_iter()
        .map(|schema| get_reducer_code(&module_name, schema, &node_selection))
        .collect();

    let query_methods: Vec<TokenStream> = module_schema
        .queries
        .into_iter()
        .map(|schema| get_query_code(&module_name, schema, &node_selection))
        .collect();

    let type_definition_items: Vec<TokenStream> = module_schema
        .type_definitions
        .values()
        .map(|type_def| {
            get_type_definition_code(type_def)
                .parse::<TokenStream>()
                .expect("Failed to parse generated type definition tokens")
        })
        .collect();

    let module_tables_name = module_tables_ident.to_string();
    let module_name_lit = LitStr::new(&module_name, span);
    let table_items: Vec<TokenStream> = module_schema
        .tables
        .into_iter()
        .map(|table| match &node_selection {
            NodeSelection::Current => get_table_code(
                table,
                &module_tables_name,
                quote! { interstice_sdk::ModuleSelection::Other(#module_name_lit.to_string()) },
                None,
            ),
            NodeSelection::Other(node_name) => {
                let replica_table_name = format!(
                    "__replica__{}__{}__{}",
                    node_name.replace('-', "_").replace('.', "_"),
                    module_name.replace('-', "_").replace('.', "_"),
                    table.name.replace('-', "_").replace('.', "_"),
                );
                get_table_code(
                    table,
                    &module_tables_name,
                    quote! { interstice_sdk::ModuleSelection::Current },
                    Some(&replica_table_name),
                )
            }
        })
        .collect();

    let context_type_tokens = match &node_selection {
        NodeSelection::Current => quote! { interstice_sdk::ReducerContext },
        NodeSelection::Other(node_name) => {
            let node_ident = Ident::new(&snake_to_camel_case(&to_snake_case(node_name)), span);
            quote! { #node_ident }
        }
    };

    let needs_inner_module = matches!(node_selection, NodeSelection::Other(_));

    let tokens = if needs_inner_module {
        quote! {
            pub mod #module_inner_ident {
                #(#type_definition_items)*

                pub struct #module_handle_ident {
                    pub tables: #module_tables_ident,
                    pub reducers: #module_reducers_ident,
                    pub queries: #module_queries_ident,
                }

                pub struct #module_tables_ident {}
                pub struct #module_reducers_ident {}
                pub struct #module_queries_ident {}

                impl #module_reducers_ident {
                    #(#reducer_methods)*
                }

                impl #module_queries_ident {
                    #(#query_methods)*
                }

                #(#table_items)*
            }

            pub trait #has_module_handle_trait_ident {
                fn #module_method_ident(&self) -> #module_inner_ident::#module_handle_ident;
            }

            impl #has_module_handle_trait_ident for #context_type_tokens {
                fn #module_method_ident(&self) -> #module_inner_ident::#module_handle_ident {
                    #module_inner_ident::#module_handle_ident {
                        tables: #module_inner_ident::#module_tables_ident {},
                        reducers: #module_inner_ident::#module_reducers_ident {},
                        queries: #module_inner_ident::#module_queries_ident {},
                    }
                }
            }
        }
    } else {
        quote! {
            #(#type_definition_items)*

            pub struct #module_handle_ident {
                pub tables: #module_tables_ident,
                pub reducers: #module_reducers_ident,
                pub queries: #module_queries_ident,
            }

            pub struct #module_tables_ident {}
            pub struct #module_reducers_ident {}
            pub struct #module_queries_ident {}

            impl #module_reducers_ident {
                #(#reducer_methods)*
            }

            impl #module_queries_ident {
                #(#query_methods)*
            }

            #(#table_items)*

            pub trait #has_module_handle_trait_ident {
                fn #module_method_ident(&self) -> #module_handle_ident;
            }

            impl #has_module_handle_trait_ident for #context_type_tokens {
                fn #module_method_ident(&self) -> #module_handle_ident {
                    #module_handle_ident {
                        tables: #module_tables_ident {},
                        reducers: #module_reducers_ident {},
                        queries: #module_queries_ident {},
                    }
                }
            }
        }
    };

    tokens.to_string()
}
