//! Parse `ReducerContext<Caps>` / `QueryContext<Caps>` and extract the `Caps` type.

use syn::{GenericArgument, PathArguments, Type};

pub(crate) fn unwrap_type(ty: &Type) -> &Type {
    match ty {
        Type::Group(g) => unwrap_type(&g.elem),
        Type::Paren(p) => unwrap_type(&p.elem),
        _ => ty,
    }
}

fn extract_caps(ty: &Type, expected: &str) -> syn::Result<Type> {
    let ty = unwrap_type(ty);
    let Type::Path(tp) = ty else {
        return Err(syn::Error::new_spanned(
            ty,
            format!("expected `{expected}<Caps>` (or `{expected}` for no table access, `Caps = ()`)"),
        ));
    };
    let Some(seg) = tp.path.segments.last() else {
        return Err(syn::Error::new_spanned(ty, "empty path"));
    };
    if seg.ident != expected {
        return Err(syn::Error::new_spanned(
            &seg.ident,
            format!("expected `{expected}` as the context type"),
        ));
    }
    match &seg.arguments {
        PathArguments::None => Ok(syn::parse_quote!(())),
        PathArguments::AngleBracketed(ab) => {
            let mut types = ab.args.iter().filter_map(|a| {
                if let GenericArgument::Type(t) = a {
                    Some(t.clone())
                } else {
                    None
                }
            });
            let caps = types.next().ok_or_else(|| {
                syn::Error::new_spanned(ab, "missing `Caps` type argument; use e.g. `ReducerContext<(ReadFoo,)>`")
            })?;
            if types.next().is_some() {
                return Err(syn::Error::new_spanned(
                    ab,
                    "too many generic arguments on context type",
                ));
            }
            Ok(caps)
        }
        PathArguments::Parenthesized(_) => Err(syn::Error::new_spanned(
            ty,
            "unexpected parenthesized path arguments",
        )),
    }
}

pub fn reducer_caps_ty(ty: &Type) -> syn::Result<Type> {
    extract_caps(ty, "ReducerContext")
}

pub fn query_caps_ty(ty: &Type) -> syn::Result<Type> {
    extract_caps(ty, "QueryContext")
}
