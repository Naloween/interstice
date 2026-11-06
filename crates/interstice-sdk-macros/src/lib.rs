mod caps_bounds;
mod context_caps;
mod index_key;
mod init;
mod interstice_type;
mod path_segments;
mod query;
mod reducer;
mod table;
mod types;
mod view;
mod visibility;

use proc_macro::TokenStream;
use syn::visit_mut::VisitMut;

/// A type passed through a `macro_rules!` `$t:ty` fragment reaches a
/// procedural macro wrapped in an invisible group, which the type checks
/// below (matching on `Type::Path`) would not recognise. Unwrap them.
fn ungroup_types(item: TokenStream) -> TokenStream {
    struct Ungroup;
    impl VisitMut for Ungroup {
        fn visit_type_mut(&mut self, ty: &mut syn::Type) {
            while let syn::Type::Group(group) = ty {
                *ty = (*group.elem).clone();
            }
            syn::visit_mut::visit_type_mut(self, ty);
        }
    }
    match syn::parse::<syn::Item>(item.clone()) {
        Ok(mut parsed) => {
            Ungroup.visit_item_mut(&mut parsed);
            quote::ToTokens::into_token_stream(parsed).into()
        }
        Err(_) => item,
    }
}

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
    table_macro(attr, ungroup_types(item))
}

#[proc_macro_attribute]
pub fn reducer(attr: TokenStream, item: TokenStream) -> TokenStream {
    reducer_macro(attr, ungroup_types(item))
}

#[proc_macro_attribute]
pub fn query(attr: TokenStream, item: TokenStream) -> TokenStream {
    query_macro(attr, ungroup_types(item))
}

/// A view (DESIGN §7.5): a filter view `fn name(r: &Table, viewer: &Viewer)
/// -> bool { condition }`, or a function view `#[view(key = id)] fn
/// name<Caps>(ctx: ViewContext<Caps>) -> Vec<Row>`.
#[proc_macro_attribute]
pub fn view(attr: TokenStream, item: TokenStream) -> TokenStream {
    view::view_macro(attr, ungroup_types(item))
}

#[proc_macro_attribute]
pub fn interstice_type(_attr: TokenStream, input: TokenStream) -> TokenStream {
    derive_interstice_type_macro(ungroup_types(input))
}
