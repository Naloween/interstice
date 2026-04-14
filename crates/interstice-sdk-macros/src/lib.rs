mod caps_bounds;
mod context_caps;
mod index_key;
mod init;
mod interstice_type;
mod path_segments;
mod query;
mod reducer;
mod table;

use proc_macro::TokenStream;

use crate::{
    init::init_macro, interstice_type::derive_interstice_type_macro, query::query_macro,
    reducer::reducer_macro, table::table_macro,
};

#[proc_macro_attribute]
pub fn init(_attr: TokenStream, item: TokenStream) -> TokenStream {
    init_macro(item)
}

#[proc_macro_attribute]
pub fn table(attr: TokenStream, item: TokenStream) -> TokenStream {
    table_macro(attr, item)
}

#[proc_macro_attribute]
pub fn reducer(attr: TokenStream, item: TokenStream) -> TokenStream {
    reducer_macro(attr, item)
}

#[proc_macro_attribute]
pub fn query(attr: TokenStream, item: TokenStream) -> TokenStream {
    query_macro(attr, item)
}

#[proc_macro_attribute]
pub fn interstice_type(_attr: TokenStream, input: TokenStream) -> TokenStream {
    derive_interstice_type_macro(input)
}
