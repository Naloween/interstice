use interstice_abi::get_reducer_wrapper_name;
use quote::quote;
use syn::{Ident, LitInt, Type};

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
    let args = (0..arg_count - 1).map(|i| {
        let index = LitInt::new(&i.to_string(), proc_macro2::Span::call_site());
        if table_subscription {
            quote! { {let row: interstice_sdk::Row = interstice_args_vec[#index].clone().try_into().unwrap(); row.try_into().unwrap()} }
        } else {
            quote! { interstice_args_vec[#index].clone().try_into().unwrap() }
        }
    });

    quote! {
        #[unsafe(no_mangle)]
        pub extern "C" fn #wrapper_name(ptr: i32, len: i32) {
            let bytes = unsafe { std::slice::from_raw_parts(ptr as *const u8, len as usize) };
            let (raw_context, interstice_args): (interstice_sdk::RawReducerContext, interstice_sdk::IntersticeValue) = interstice_sdk::decode(bytes).unwrap();
            let reducer_context: interstice_sdk::ReducerContext<#caps_ty> = raw_context.into();
            let interstice_args_vec = match interstice_args {
                interstice_sdk::IntersticeValue::Vec(v) => v,
                _ => panic!("Expected Vec<IntersticeValue> as reducer_wrapper input, got {:?}", interstice_args),
            };
            if interstice_args_vec.len() != #arg_count - 1 { panic!("Expected {} reducer arguments, got {}", #arg_count-1, interstice_args_vec.len()) }

            #reducer_ident(reducer_context, #(#args),*);
        }
    }
}
