use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{parse_macro_input, ItemFn};

pub fn init_macro(item: TokenStream) -> TokenStream {
    let mut input = parse_macro_input!(item as ItemFn);
    let name = &input.sig.ident;

    // 1. Correctly set the ABI to "C" inside the function signature
    // This ensures 'pub extern "C" fn' order is correct automatically
    input.sig.abi = Some(syn::Abi {
        extern_token: syn::token::Extern::default(),
        name: Some(syn::LitStr::new("C", proc_macro2::Span::call_site())),
    });

    let init_static_name = format_ident!("__INTERSTICE_INIT_{}", name.to_string().to_uppercase());

    quote! {
        #[unsafe(no_mangle)]
        #input

        #[used]
        #[link_section = ".init_array"]
        static #init_static_name: extern "C" fn() = #name;
    }
    .into()
}
