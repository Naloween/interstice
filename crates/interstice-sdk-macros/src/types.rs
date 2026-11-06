//! Rust types to `IntersticeType`, at compile time (DESIGN §10.1).
//!
//! A type that cannot cross the module boundary is a compile error at the
//! type itself, with what to use instead, rather than a panic when the
//! module describes itself.

use proc_macro2::TokenStream;
use quote::{ToTokens, quote};
use syn::{GenericArgument, PathArguments, Type};

/// The `interstice_sdk::IntersticeType` expression for `ty`.
pub fn type_expr(ty: &Type) -> syn::Result<TokenStream> {
    let it = quote!(interstice_sdk::IntersticeType);
    match ty {
        Type::Group(g) => type_expr(&g.elem),
        Type::Paren(p) => type_expr(&p.elem),
        Type::Tuple(t) if t.elems.is_empty() => Ok(quote!(#it::Void)),
        Type::Tuple(t) => {
            let items = t.elems.iter().map(type_expr).collect::<syn::Result<Vec<_>>>()?;
            Ok(quote!(#it::Tuple(vec![#(#items),*])))
        }
        Type::Path(p) if p.qself.is_none() => {
            let seg = p.path.segments.last().expect("a path has a segment");
            let name = seg.ident.to_string();
            let generic = |what: &str| -> syn::Result<&Type> {
                if let PathArguments::AngleBracketed(args) = &seg.arguments {
                    let types: Vec<&Type> = args
                        .args
                        .iter()
                        .filter_map(|a| match a {
                            GenericArgument::Type(t) => Some(t),
                            _ => None,
                        })
                        .collect();
                    if let [inner] = types.as_slice() {
                        return Ok(inner);
                    }
                }
                Err(syn::Error::new_spanned(ty, format!("`{what}` needs one type argument")))
            };
            let plain = |expr: TokenStream| -> syn::Result<TokenStream> {
                if seg.arguments.is_empty() {
                    Ok(expr)
                } else {
                    Err(syn::Error::new_spanned(ty, format!("`{name}` takes no type arguments")))
                }
            };
            match name.as_str() {
                "u8" => plain(quote!(#it::U8)),
                "u32" => plain(quote!(#it::U32)),
                "u64" => plain(quote!(#it::U64)),
                "i32" => plain(quote!(#it::I32)),
                "i64" => plain(quote!(#it::I64)),
                "f32" => plain(quote!(#it::F32)),
                "f64" => plain(quote!(#it::F64)),
                "bool" => plain(quote!(#it::Bool)),
                "String" => plain(quote!(#it::String)),
                "Blob" => plain(quote!(#it::Blob)),
                "Vec" => {
                    let inner = type_expr(generic("Vec")?)?;
                    Ok(quote!(#it::Vec(Box::new(#inner))))
                }
                "Option" => {
                    let inner = type_expr(generic("Option")?)?;
                    Ok(quote!(#it::Option(Box::new(#inner))))
                }
                "u16" | "u128" | "usize" => Err(syn::Error::new_spanned(
                    ty,
                    format!("`{name}` cannot cross the module boundary; use u8, u32 or u64"),
                )),
                "i8" | "i16" | "i128" | "isize" => Err(syn::Error::new_spanned(
                    ty,
                    format!("`{name}` cannot cross the module boundary; use i32 or i64"),
                )),
                "char" | "str" => Err(syn::Error::new_spanned(
                    ty,
                    format!("`{name}` cannot cross the module boundary; use String"),
                )),
                "HashMap" | "BTreeMap" | "HashSet" | "BTreeSet" | "VecDeque" | "Box" | "Rc" | "Arc" => {
                    Err(syn::Error::new_spanned(
                        ty,
                        format!("`{name}` cannot cross the module boundary; use Vec (of tuples for a map) or the value itself"),
                    ))
                }
                _ if !seg.arguments.is_empty() => Err(syn::Error::new_spanned(
                    ty,
                    "generic types other than Vec<T> and Option<T> cannot cross the module boundary",
                )),
                // A struct or enum declared with #[interstice_type] or #[table].
                _ => Ok(quote!(#it::Named(#name.to_string()))),
            }
        }
        Type::Reference(_) => Err(syn::Error::new_spanned(
            ty,
            "references cannot cross the module boundary; take an owned value (String for &str, Vec<T> for &[T])",
        )),
        Type::Array(_) | Type::Slice(_) => Err(syn::Error::new_spanned(ty, "arrays and slices cannot cross the module boundary; use Vec<T>")),
        _ => Err(syn::Error::new_spanned(
            ty,
            format!("`{}` cannot cross the module boundary", ty.to_token_stream()),
        )),
    }
}

/// Type errors in a function's parameters (after the context) and, for a
/// query, its return type: reported alone, with the function unchanged, so
/// the generated glue does not add errors of its own (DESIGN §10.1).
pub fn signature_type_errors(input_fn: &syn::ItemFn, with_return: bool) -> Option<proc_macro2::TokenStream> {
    let mut errors: Option<syn::Error> = None;
    let mut push = |e: syn::Error| match &mut errors {
        Some(all) => all.combine(e),
        None => errors = Some(e),
    };
    for arg in input_fn.sig.inputs.iter().skip(1) {
        if let syn::FnArg::Typed(pat) = arg {
            if let Err(e) = type_expr(&pat.ty) {
                push(e);
            }
        }
    }
    if with_return {
        if let syn::ReturnType::Type(_, ty) = &input_fn.sig.output {
            let ty = crate::query::result_ok_type(ty).unwrap_or(ty);
            if let Err(e) = type_expr(ty) {
                push(e);
            }
        }
    }
    errors.map(|e| {
        let errors = e.to_compile_error();
        quote::quote! { #errors #input_fn }
    })
}
