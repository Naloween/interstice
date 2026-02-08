use interstice_abi::get_query_wrapper_name;
use quote::quote;
use syn::{Ident, LitInt};

pub fn get_wrapper_function(query_ident: Ident, arg_count: usize) -> proc_macro2::TokenStream {
    let wrapper_name = syn::Ident::new(
        &get_query_wrapper_name(&query_ident.to_string()),
        query_ident.span(),
    );
    let args = (0..arg_count - 1).map(|i| {
        let index = LitInt::new(&i.to_string(), proc_macro2::Span::call_site());
        quote! { interstice_args_vec[#index].clone().try_into().unwrap() }
    });

    quote! {
        #[unsafe(no_mangle)]
        pub extern "C" fn #wrapper_name(ptr: i32, len: i32) -> i64 {
            let bytes = unsafe { std::slice::from_raw_parts(ptr as *const u8, len as usize) };
            let (query_context, interstice_args): (interstice_sdk::QueryContext, interstice_sdk::IntersticeValue) = interstice_sdk::decode(bytes).unwrap();
            let interstice_args_vec = match interstice_args {
                interstice_sdk::IntersticeValue::Vec(v) => v,
                _ => panic!("Expected Vec<IntersticeValue> as query_wrapper input, got {:?}", interstice_args),
            };
            if interstice_args_vec.len() != #arg_count - 1 { panic!("Expected {} query arguments, got {}", #arg_count-1, interstice_args_vec.len()) }

            let res: interstice_sdk::IntersticeValue = #query_ident(query_context, #(#args),*).into();

            let bytes = interstice_sdk::encode(&res).unwrap();
            let out_ptr = alloc(bytes.len() as i32);
            unsafe {
                std::slice::from_raw_parts_mut(out_ptr as *mut u8, bytes.len()).copy_from_slice(&bytes);
            }
            return interstice_sdk::pack_ptr_len(out_ptr, bytes.len() as i32);
        }
    }
}
