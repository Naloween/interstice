use interstice_abi::get_reducer_wrapper_name;
use quote::quote;
use syn::{Ident, Type};

/// The exported wrapper of a reducer: decode the context and the arguments
/// (straight into their types, DESIGN §10.2), call it, and turn an `Err`
/// into a failure with its message.
pub fn get_wrapper_function(
    reducer_ident: Ident,
    caps_ty: Type,
    arg_count: usize,
    table_subscription: bool,
) -> proc_macro2::TokenStream {
    let wrapper_name = syn::Ident::new(
        &get_reducer_wrapper_name(&reducer_ident.to_string()),
        reducer_ident.span(),
    );
    let count = arg_count - 1;
    let names: Vec<String> = (1..=count).map(|i| format!("#{i}")).collect();
    // A table event passes rows (`Row` values, not structs).
    let args = if table_subscription {
        quote! {
            #(
                {
                    let value: interstice_sdk::IntersticeValue = interstice_sdk::wrapper::arg(&mut input, #names);
                    let row: interstice_sdk::Row = value.into();
                    row.try_into().unwrap_or_else(|e| panic!("argument {}: {}", #names, e))
                }
            ),*
        }
    } else {
        quote! { #(interstice_sdk::wrapper::arg(&mut input, #names)),* }
    };

    quote! {
        #[unsafe(no_mangle)]
        pub extern "C" fn #wrapper_name(ptr: i32, len: i32) {
            interstice_sdk::macros::install_panic_hook();
            let bytes = unsafe { std::slice::from_raw_parts(ptr as *const u8, len as usize) };
            #[allow(unused_mut)]
            let (raw_context, mut input) =
                interstice_sdk::wrapper::split_input::<interstice_sdk::RawReducerContext>(bytes, #count);
            let reducer_context: interstice_sdk::ReducerContext<#caps_ty> = raw_context.into();
            // Arguments are evaluated left to right, each taking its bytes.
            if let Err(err) = interstice_sdk::ReducerOutcome::into_outcome(#reducer_ident(reducer_context, #args)) {
                panic!("{}", err);
            }
        }
    }
}
