//! Shared `ReducerTableRef` token generation for `#[reducer]` and `#[query(reads = …)]`.

use crate::path_segments::segments_from_dotted_str;
use proc_macro2::Span;
use quote::quote;
use syn::spanned::Spanned;
use syn::{Expr, LitStr, Meta};

pub(crate) fn parse_table_access_list(
    attributes: &syn::punctuated::Punctuated<Meta, syn::Token![,]>,
    field_name: &str,
) -> syn::Result<Vec<proc_macro2::TokenStream>> {
    let mut values = Vec::new();
    for meta in attributes {
        let Meta::NameValue(nv) = meta else {
            continue;
        };
        if !nv.path.is_ident(field_name) {
            continue;
        }
        let Expr::Array(arr) = &nv.value else {
            return Err(syn::Error::new_spanned(
                &nv.value,
                format!("Expected {} = [..] array", field_name),
            ));
        };
        for expr in &arr.elems {
            values.push(entry_to_reducer_table_ref(expr)?);
        }
    }
    Ok(values)
}

pub(crate) fn entry_to_reducer_table_ref(expr: &Expr) -> syn::Result<proc_macro2::TokenStream> {
    let span = expr.span();
    let segments: Vec<String> = match expr {
        Expr::Path(path) => {
            if path.path.segments.len() != 1 {
                return Err(syn::Error::new_spanned(
                    path,
                    "use a string literal for dotted paths, e.g. reads = [\"module.table\"] (same as subscription `on = \"module.table.event\"`)",
                ));
            }
            let seg = path.path.segments.last().unwrap();
            vec![seg.ident.to_string()]
        }
        Expr::Lit(expr_lit) => {
            if let syn::Lit::Str(s) = &expr_lit.lit {
                segments_from_dotted_str(&s.value(), s.span())?
            } else {
                return Err(syn::Error::new_spanned(
                    expr_lit,
                    "expected a string literal for table access (same form as subscription `on`)",
                ));
            }
        }
        other => {
            return Err(syn::Error::new_spanned(
                other,
                "expected a string literal or a single identifier for table access (subscription `on` uses string literals only for dotted paths)",
            ));
        }
    };
    reducer_table_ref_from_segments(&segments, span)
}

pub(crate) fn reducer_table_ref_from_segments(
    segments: &[String],
    span: Span,
) -> syn::Result<proc_macro2::TokenStream> {
    if segments.is_empty() {
        return Err(syn::Error::new(span, "empty table access entry"));
    }
    match segments.len() {
        1 => {
            let t = LitStr::new(&segments[0].to_lowercase(), span);
            Ok(quote! {
                interstice_sdk::ReducerTableRef {
                    node_selection: interstice_sdk::NodeSelection::Current,
                    module_selection: interstice_sdk::ModuleSelection::Current,
                    table_name: #t.to_string(),
                }
            })
        }
        2 => {
            let m = LitStr::new(&segments[0].to_lowercase(), span);
            let t = LitStr::new(&segments[1].to_lowercase(), span);
            Ok(quote! {
                interstice_sdk::ReducerTableRef {
                    node_selection: interstice_sdk::NodeSelection::Current,
                    module_selection: interstice_sdk::ModuleSelection::Other(#m.to_string()),
                    table_name: #t.to_string(),
                }
            })
        }
        3 => {
            let n = LitStr::new(&segments[0].to_lowercase(), span);
            let m = LitStr::new(&segments[1].to_lowercase(), span);
            let t = LitStr::new(&segments[2].to_lowercase(), span);
            Ok(quote! {
                interstice_sdk::ReducerTableRef {
                    node_selection: interstice_sdk::NodeSelection::Other(#n.to_string()),
                    module_selection: interstice_sdk::ModuleSelection::Other(#m.to_string()),
                    table_name: #t.to_string(),
                }
            })
        }
        _ => Err(syn::Error::new(
            span,
            "expected `table`, `module.table`, or `node.module.table` (same segment rules as subscription `on` without the trailing event)",
        )),
    }
}
