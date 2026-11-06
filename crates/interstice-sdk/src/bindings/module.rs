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

pub fn get_module_code(module_schema: ModuleSchema, node_selection: NodeSelection) -> TokenStream {
    let span = Span::call_site();
    let module_name = module_schema.name.clone();
    let snake_module_name = to_snake_case(&module_name);
    let camel_module_name = snake_to_camel_case(&snake_module_name);
    let module_handle_ident = Ident::new(&(camel_module_name.clone() + "ModuleHandle"), span);
    let module_tables_ident = Ident::new(&(camel_module_name.clone() + "Tables"), span);
    let module_reducers_ident = Ident::new(&(camel_module_name.clone() + "Reducers"), span);
    let module_queries_ident = Ident::new(&(camel_module_name.clone() + "Queries"), span);
    let module_views_ident = Ident::new(&(camel_module_name.clone() + "Views"), span);

    // Views read like tables, each with its own row struct (DESIGN §7.5).
    let view_shapes: Vec<_> = module_schema
        .views
        .iter()
        .filter_map(|v| Some((module_schema.view_table_schema(&v.name)?, module_schema.view_type_def(&v.name)?)))
        .collect();
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
        .map(|def| get_type_definition_code(def, &module_schema.type_definitions))
        .collect();

    let module_tables_name = module_tables_ident.to_string();
    let module_views_name = module_views_ident.to_string();
    let module_name_lit = LitStr::new(&module_name, span);
    let view_type_items: Vec<TokenStream> = view_shapes
        .iter()
        .map(|(_, def)| get_type_definition_code(def, &module_schema.type_definitions))
        .collect();
    let view_items: Vec<TokenStream> = view_shapes
        .into_iter()
        .map(|(shape, _)| {
            let view_name = shape.name.clone();
            match &node_selection {
                NodeSelection::Current => get_table_code(
                    shape,
                    &module_views_name,
                    quote! { interstice_sdk::ModuleSelection::Other(#module_name_lit.to_string()) },
                    None,
                    "current",
                    &module_name,
                    &view_name,
                ),
                NodeSelection::Other(node_name) => {
                    let replica_name = format!(
                        "__replica__{}__{}__{}",
                        node_name.replace('-', "_").replace('.', "_"),
                        module_name.replace('-', "_").replace('.', "_"),
                        view_name.replace('-', "_").replace('.', "_"),
                    );
                    get_table_code(
                        shape,
                        &module_views_name,
                        quote! { interstice_sdk::ModuleSelection::Current },
                        Some(&replica_name),
                        node_name,
                        &module_name,
                        &view_name,
                    )
                }
            }
        })
        .collect();
    let table_items: Vec<TokenStream> = module_schema
        .tables
        .into_iter()
        .map(|table| {
            let table_name_for_ref = table.name.clone();
            match &node_selection {
            NodeSelection::Current => get_table_code(
                table,
                &module_tables_name,
                quote! { interstice_sdk::ModuleSelection::Other(#module_name_lit.to_string()) },
                None,
                "current",
                &module_name,
                &table_name_for_ref,
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
                    node_name,
                    &module_name,
                    &table_name_for_ref,
                )
            }
        }})
        .collect();

    let context_type_tokens = match &node_selection {
        NodeSelection::Current => quote! { interstice_sdk::ReducerContext<Caps> },
        NodeSelection::Other(node_name) => {
            let node_ident = Ident::new(&snake_to_camel_case(&to_snake_case(node_name)), span);
            quote! { #node_ident<Caps> }
        }
    };
    let context_generics = match &node_selection {
        NodeSelection::Current => quote! { <Caps> },
        NodeSelection::Other(_) => quote! { <Caps> },
    };

    let needs_inner_module = matches!(node_selection, NodeSelection::Other(_));

    let tokens = if needs_inner_module {
        quote! {
            pub mod #module_inner_ident {
                #(#type_definition_items)*
                #(#view_type_items)*

                pub struct #module_handle_ident<Caps> {
                    pub tables: #module_tables_ident<Caps>,
                    pub views: #module_views_ident<Caps>,
                    pub reducers: #module_reducers_ident,
                    pub queries: #module_queries_ident,
                }

                pub struct #module_tables_ident<Caps> {
                    pub _caps: std::marker::PhantomData<Caps>,
                }
                pub struct #module_views_ident<Caps> {
                    pub _caps: std::marker::PhantomData<Caps>,
                }
                pub struct #module_reducers_ident {}
                pub struct #module_queries_ident {}

                impl #module_reducers_ident {
                    #(#reducer_methods)*
                }

                impl #module_queries_ident {
                    #(#query_methods)*
                }

                #(#table_items)*
                #(#view_items)*
            }

            pub trait #has_module_handle_trait_ident<Caps> {
                fn #module_method_ident(&self) -> #module_inner_ident::#module_handle_ident<Caps>;
            }

            impl #context_generics #has_module_handle_trait_ident<Caps> for #context_type_tokens {
                fn #module_method_ident(&self) -> #module_inner_ident::#module_handle_ident<Caps> {
                    #module_inner_ident::#module_handle_ident {
                        tables: #module_inner_ident::#module_tables_ident { _caps: std::marker::PhantomData },
                        views: #module_inner_ident::#module_views_ident { _caps: std::marker::PhantomData },
                        reducers: #module_inner_ident::#module_reducers_ident {},
                        queries: #module_inner_ident::#module_queries_ident {},
                    }
                }
            }
        }
    } else {
        quote! {
            #(#type_definition_items)*
            #(#view_type_items)*

            pub struct #module_handle_ident<Caps> {
                pub tables: #module_tables_ident<Caps>,
                pub views: #module_views_ident<Caps>,
                pub reducers: #module_reducers_ident,
                pub queries: #module_queries_ident,
            }

            pub struct #module_tables_ident<Caps> {
                pub _caps: std::marker::PhantomData<Caps>,
            }
            pub struct #module_views_ident<Caps> {
                pub _caps: std::marker::PhantomData<Caps>,
            }
            pub struct #module_reducers_ident {}
            pub struct #module_queries_ident {}

            impl #module_reducers_ident {
                #(#reducer_methods)*
            }

            impl #module_queries_ident {
                #(#query_methods)*
            }

            #(#table_items)*
            #(#view_items)*

            pub trait #has_module_handle_trait_ident<Caps> {
                fn #module_method_ident(&self) -> #module_handle_ident<Caps>;
            }

            impl #context_generics #has_module_handle_trait_ident<Caps> for #context_type_tokens {
                fn #module_method_ident(&self) -> #module_handle_ident<Caps> {
                    #module_handle_ident {
                        tables: #module_tables_ident { _caps: std::marker::PhantomData },
                        views: #module_views_ident { _caps: std::marker::PhantomData },
                        reducers: #module_reducers_ident {},
                        queries: #module_queries_ident {},
                    }
                }
            }
        }
    };

    tokens
}
