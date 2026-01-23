use proc_macro::TokenStream;
use quote::{quote, ToTokens};
use syn::{parse_macro_input, ItemFn, LitInt};

#[proc_macro_attribute]
pub fn reducer(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let input_fn = parse_macro_input!(item as ItemFn);
    let fn_name = &input_fn.sig.ident;
    let wrapper_name = syn::Ident::new(&format!("interstice_{}_wrapper", fn_name), fn_name.span());
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

        fn #wrapper_name(interstice_args: interstice_abi::IntersticeValue) -> interstice_abi::IntersticeValue {
            let interstice_args_vec = match interstice_args {
                interstice_abi::IntersticeValue::Vec(v) => v,
                _ => panic!("Expected Vec<IntersticeValue>"),
            };
            if interstice_args_vec.len() != #arg_count { panic!("Expected #arg_count arguments") }

            let res = #fn_name(#(#args),*);
            res.into()
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
            interstice_sdk::interstice_sdk_core::register_reducer(#schema_name);
        }
    };
    gen.into()
}
