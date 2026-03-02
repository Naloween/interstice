use interstice_abi::{ModuleSchema, NodeSchema, NodeSelection};
use proc_macro2::{Span, TokenStream};
use quote::quote;
use syn::Ident;

use crate::{bindings::module::get_module_code, snake_to_camel_case, to_snake_case};

pub fn get_node_code(node_schema: NodeSchema) -> TokenStream {
    let span = Span::call_site();
    let node_name = to_snake_case(&node_schema.name);
    let node_type_str = snake_to_camel_case(&node_name);
    let original_node_name = node_schema.name.clone();
    let node_mod_ident = Ident::new(&node_name, span);
    let node_type_ident = Ident::new(&node_type_str, span);
    let node_method_ident = Ident::new(&node_name, span);
    let trait_handle_ident = Ident::new(&("Has".to_string() + &node_type_str + "Handle"), span);

    let module_tokens: Vec<TokenStream> = node_schema
        .modules
        .into_iter()
        .map(|module_schema| {
            get_module_code(
                module_schema,
                NodeSelection::Other(original_node_name.clone()),
            )
        })
        .collect();

    let tokens = quote! {
        pub mod #node_mod_ident {
            pub struct #node_type_ident {}

            #(#module_tokens)*
        }

        pub trait #trait_handle_ident {
            fn #node_method_ident(&self) -> #node_mod_ident::#node_type_ident;
        }

        impl #trait_handle_ident for interstice_sdk::ReducerContext {
            fn #node_method_ident(&self) -> #node_mod_ident::#node_type_ident {
                #node_mod_ident::#node_type_ident {}
            }
        }
    };

    tokens
}

pub fn get_current_node_code(module_schemas: Vec<ModuleSchema>) -> TokenStream {
    let span = Span::call_site();
    let module_tokens: Vec<TokenStream> = module_schemas
        .into_iter()
        .map(|module_schema| {
            let module_name = crate::to_snake_case(&module_schema.name);
            let module_ident = Ident::new(&module_name, span);
            let module_content = get_module_code(module_schema, NodeSelection::Current);
            quote! {
                pub mod #module_ident {
                    #module_content
                }
            }
        })
        .collect();

    quote! {
        #(#module_tokens)*
    }
}
