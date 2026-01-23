use interstice_abi::get_reducer_wrapper_name;
use proc_macro::TokenStream;
use quote::{quote, ToTokens};
use syn::{parse_macro_input, ItemFn, LitInt};

#[proc_macro_attribute]
pub fn reducer(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let input_fn = parse_macro_input!(item as ItemFn);
    let fn_name = &input_fn.sig.ident;
    let wrapper_name = syn::Ident::new(
        &get_reducer_wrapper_name(&fn_name.to_string()),
        fn_name.span(),
    );
    let schema_name = syn::Ident::new(&format!("interstice_{}_schema", fn_name), fn_name.span());
    let register_schema_name = syn::Ident::new(
        &format!("interstice_register_{}_schema", fn_name),
        fn_name.span(),
    );

    let arg_names: Vec<_> = input_fn
        .sig
        .inputs
        .iter()
        .map(|arg| match arg {
            syn::FnArg::Typed(pat) => &pat.pat,
            _ => panic!("Unexpected argument type"),
        })
        .collect();

    let arg_types: Vec<_> = input_fn
        .sig
        .inputs
        .iter()
        .map(|arg| match arg {
            syn::FnArg::Typed(pat) => &pat.ty,
            _ => panic!("Unexpected argument type"),
        })
        .collect();

    let arg_count = arg_names.len();

    let args = (0..arg_count).map(|i| {
        let index = LitInt::new(&i.to_string(), proc_macro2::Span::call_site());
        quote! { interstice_args_vec[#index].clone().into() }
    });

    let schema_entries = arg_names.iter().zip(arg_types).map(|(arg_name, arg_type)| {
        let arg_name_str = quote! { #arg_name }.to_string();
        let arg_type_str = quote! { #arg_type }.to_string();
        quote! {
            interstice_abi::EntrySchema {
                name: #arg_name_str.to_string(),
                value_type: #arg_type_str.to_string().into(),
            }
        }
    });

    let return_type = match &input_fn.sig.output {
        syn::ReturnType::Default => quote! { interstice_abi::IntersticeType::Void },
        syn::ReturnType::Type(_, ty) => {
            let ty = ty.to_token_stream().to_string();
            quote! {
               #ty.to_string().into()
            }
        }
    };

    // Generate wrapper and schema
    let gen = quote! {
        #input_fn

        #[no_mangle]
        pub extern "C" fn #wrapper_name(ptr: i32, len: i32) -> i64 {
            let bytes = unsafe { std::slice::from_raw_parts(ptr as *const u8, len as usize) };
            let interstice_args: interstice_abi::IntersticeValue = interstice_abi::decode(bytes).unwrap();
            let interstice_args_vec = match interstice_args {
                interstice_abi::IntersticeValue::Vec(v) => v,
                _ => panic!("Expected Vec<IntersticeValue>"),
            };
            if interstice_args_vec.len() != #arg_count { panic!("Expected #arg_count arguments") }

            let res: interstice_abi::IntersticeValue = #fn_name(#(#args),*).into();

            let bytes = interstice_abi::encode(&res).unwrap();
            let out_ptr = alloc(bytes.len() as i32);
            unsafe {
                std::slice::from_raw_parts_mut(out_ptr as *mut u8, bytes.len()).copy_from_slice(&bytes);
            }
            return interstice_abi::pack_ptr_len(out_ptr, bytes.len() as i32);
        }


        fn #schema_name() -> interstice_abi::ReducerSchema {
            interstice_abi::ReducerSchema::new(
                stringify!(#fn_name),
                vec![#(#schema_entries),*],
                #return_type,
            )
        }

        #[ctor::ctor]
        fn #register_schema_name() {
            interstice_sdk::register_reducer(#schema_name);
        }
    };
    gen.into()
}
