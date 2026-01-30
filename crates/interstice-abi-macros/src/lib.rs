mod interstice_type;

use crate::interstice_type::derive_interstice_type_macro;
use proc_macro::TokenStream;
use proc_macro2::Span;
use proc_macro_crate::{crate_name, FoundCrate};
use quote::quote;
use syn::parse_macro_input;
use syn::Ident;

#[proc_macro_derive(IntersticeType)]
pub fn derive_interstice_type(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as syn::DeriveInput);
    derive_interstice_type_macro(input).into()
}

fn abi_path() -> proc_macro2::TokenStream {
    let using_sdk = match crate_name("interstice-sdk") {
        Ok(FoundCrate::Name(_)) => true,
        _ => false,
    };

    if using_sdk {
        quote!(interstice_sdk)
    } else {
        match crate_name("interstice-abi") {
            Ok(FoundCrate::Itself) => quote!(crate),
            Ok(FoundCrate::Name(name)) => {
                let ident = Ident::new(&name, Span::call_site());
                quote!(::#ident)
            }
            Err(_) => quote!(::interstice_abi), // fallback
        }
    }
}
