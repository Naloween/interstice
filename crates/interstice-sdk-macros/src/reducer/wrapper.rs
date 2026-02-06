use interstice_abi::get_reducer_wrapper_name;
use quote::quote;
use syn::{Ident, LitInt};

pub fn get_wrapper_function(
    reducer_ident: Ident,
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
    }
}
