use interstice_abi::get_query_wrapper_name;
use quote::quote;
use syn::{Ident, Type};

/// The exported wrapper of a query: decode the context and the arguments,
/// call it, and return its encoded result (DESIGN §10.2).
pub fn get_wrapper_function(
    query_ident: Ident,
    caps_ty: Type,
    arg_count: usize,
    returns_result: bool,
) -> proc_macro2::TokenStream {
    let wrapper_name = syn::Ident::new(
        &get_query_wrapper_name(&query_ident.to_string()),
        query_ident.span(),
    );
    let count = arg_count - 1;
    let names: Vec<String> = (1..=count).map(|i| format!("#{i}")).collect();
    let call = quote! { #query_ident(query_context, #(interstice_sdk::wrapper::arg(&mut input, #names)),*) };
    let value = if returns_result {
        quote! {
            match #call {
                Ok(value) => value,
                Err(err) => panic!("{}", err),
            }
        }
    } else {
        call
    };

    quote! {
        #[unsafe(no_mangle)]
        pub extern "C" fn #wrapper_name(ptr: i32, len: i32) -> i64 {
            interstice_sdk::macros::install_panic_hook();
            let bytes = unsafe { std::slice::from_raw_parts(ptr as *const u8, len as usize) };
            #[allow(unused_mut)]
            let (raw_context, mut input) =
                interstice_sdk::wrapper::split_input::<interstice_sdk::RawQueryContext>(bytes, #count);
            let query_context: interstice_sdk::QueryContext<#caps_ty> = raw_context.into();
            let value = #value;
            interstice_sdk::wrapper::return_value(&value, crate::alloc)
        }
    }
}
