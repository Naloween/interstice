//! Parse `where Caps: CanRead<Row> + CanInsert<Row> + …` for schema registration and FFI lowering.

use quote::{format_ident, quote};

use syn::punctuated::Punctuated;
use syn::{
    GenericParam, Ident, ItemFn, TraitBound, Type, TypeParamBound, WherePredicate,
};
use syn::PredicateType;

use crate::context_caps::unwrap_type;

fn pascal_case_from_ident(ident: &Ident) -> String {
    ident
        .to_string()
        .split('_')
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut c = w.chars();
            match (c.next(), c.as_str()) {
                (Some(f), rest) => f.to_uppercase().to_string() + rest,
                _ => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .concat()
}

fn synthetic_caps_ident(owner: &Ident) -> Ident {
    let pascal = pascal_case_from_ident(owner);
    format_ident!("Interstice{}Caps", pascal)
}

#[derive(Clone, Default)]
pub struct ParsedCanBounds {
    pub reads: Vec<Type>,
    pub inserts: Vec<Type>,
    pub updates: Vec<Type>,
    pub deletes: Vec<Type>,
}

pub enum ContextCapsKind {
    /// `ReducerContext` / `QueryContext` without `<…>` — no table access in schema (`Caps = ()`).
    DefaultEmptyCaps,
    /// `ReducerContext<MyConcrete>` — schema via `ReducerCaps` / `QueryCaps`.
    Concrete(Type),
    /// `ReducerContext<Caps>` where `Caps` is a type parameter: schema from `Can*` where-clauses;
    /// the macro emits `Interstice{PascalCaseFn}Caps` implementing those `Can*` + `ReducerCaps`.
    GenericParam {
        _param: Ident,
        bounds: ParsedCanBounds,
    },
}

fn is_can_trait_path(path: &syn::Path) -> bool {
    let Some(seg) = path.segments.last() else {
        return false;
    };
    matches!(
        seg.ident.to_string().as_str(),
        "CanRead" | "CanInsert" | "CanUpdate" | "CanDelete"
    )
}

fn row_ty_from_can_trait(b: &TraitBound) -> Option<(Ident, Type)> {
    let path = &b.path;
    if !is_can_trait_path(path) {
        return None;
    }
    let seg = path.segments.last()?;
    let op = seg.ident.clone();
    let syn::PathArguments::AngleBracketed(ab) = &seg.arguments else {
        return None;
    };
    let arg = ab.args.first()?;
    let syn::GenericArgument::Type(row_ty) = arg else {
        return None;
    };
    Some((op, row_ty.clone()))
}

fn merge_bounds(summary: &mut ParsedCanBounds, op: &Ident, row_ty: Type) {
    match op.to_string().as_str() {
        "CanRead" => summary.reads.push(row_ty),
        "CanInsert" => summary.inserts.push(row_ty),
        "CanUpdate" => summary.updates.push(row_ty),
        "CanDelete" => summary.deletes.push(row_ty),
        _ => {}
    }
}

fn strip_can_bounds_from_type_param(tp: &mut syn::TypeParam) -> ParsedCanBounds {
    let mut summary = ParsedCanBounds::default();
    let mut kept: Punctuated<TypeParamBound, syn::token::Plus> = Punctuated::new();
    for bound in &tp.bounds {
        let TypeParamBound::Trait(tb) = bound else {
            kept.push(bound.clone());
            continue;
        };
        if let Some((op, row_ty)) = row_ty_from_can_trait(tb) {
            merge_bounds(&mut summary, &op, row_ty);
        } else {
            kept.push(bound.clone());
        }
    }
    tp.bounds = kept;
    summary
}

/// Strip `CanRead` / `CanInsert` / `CanUpdate` / `CanDelete` trait bounds from `where Caps: …`.
fn strip_can_bounds_from_where(
    where_clause: &mut syn::WhereClause,
    caps_param: &Ident,
) -> syn::Result<ParsedCanBounds> {
    let mut summary = ParsedCanBounds::default();
    let mut out: Punctuated<WherePredicate, syn::Token![,]> = Punctuated::new();
    for pred in where_clause.predicates.iter() {
        let WherePredicate::Type(pt) = pred else {
            out.push(pred.clone());
            continue;
        };
        let Type::Path(bounded_path) = &pt.bounded_ty else {
            out.push(pred.clone());
            continue;
        };
        if bounded_path.path.segments.len() != 1
            || bounded_path.path.segments.first().unwrap().ident != *caps_param
        {
            out.push(pred.clone());
            continue;
        }

        let mut kept: Punctuated<TypeParamBound, syn::token::Plus> = Punctuated::new();
        for bound in &pt.bounds {
            let TypeParamBound::Trait(tb) = bound else {
                kept.push(bound.clone());
                continue;
            };
            if let Some((op, row_ty)) = row_ty_from_can_trait(tb) {
                merge_bounds(&mut summary, &op, row_ty);
            } else {
                kept.push(bound.clone());
            }
        }

        if kept.is_empty() {
            continue;
        }
        out.push(WherePredicate::Type(PredicateType {
            lifetimes: pt.lifetimes.clone(),
            bounded_ty: pt.bounded_ty.clone(),
            colon_token: pt.colon_token,
            bounds: kept,
        }));
    }

    where_clause.predicates = out;
    Ok(summary)
}

fn type_contains_ident(ty: &Type, id: &Ident) -> bool {
    match ty {
        Type::Path(p) => p.path.is_ident(id) || p.path.segments.iter().any(|s| {
            if let syn::PathArguments::AngleBracketed(ab) = &s.arguments {
                ab.args.iter().any(|a| {
                    if let syn::GenericArgument::Type(t) = a {
                        type_contains_ident(t, id)
                    } else {
                        false
                    }
                })
            } else {
                false
            }
        }),
        Type::Reference(r) => type_contains_ident(&r.elem, id),
        Type::Tuple(t) => t.elems.iter().any(|e| type_contains_ident(e, id)),
        Type::Group(g) => type_contains_ident(&g.elem, id),
        Type::Paren(p) => type_contains_ident(&p.elem, id),
        _ => false,
    }
}

fn sig_mentions_ident_except_first_arg(sig: &syn::Signature, caps: &Ident) -> bool {
    let mut iter = sig.inputs.iter();
    let _ = iter.next(); // skip first (context)
    for arg in iter {
        let syn::FnArg::Typed(t) = arg else {
            continue;
        };
        if type_contains_ident(&t.ty, caps) {
            return true;
        }
    }
    if let syn::ReturnType::Type(_, ty) = &sig.output {
        if type_contains_ident(ty, caps) {
            return true;
        }
    }
    false
}

/// Inspect `fn …(ctx: ReducerContext<…>, …) where …` and classify caps + optionally rewrite for FFI.
pub fn classify_and_rewrite_context_fn(
    item: &mut ItemFn,
    context_name: &str,
) -> syn::Result<ContextCapsKind> {
    let first_ty = match item.sig.inputs.first() {
        Some(syn::FnArg::Typed(pat)) => pat.ty.as_ref(),
        _ => {
            return Err(syn::Error::new_spanned(
                &item.sig,
                "expected typed first argument for context",
            ));
        }
    };
    let first_ty = unwrap_type(first_ty);
    let Type::Path(tp) = first_ty else {
        return Err(syn::Error::new_spanned(
            first_ty,
            format!("expected `{context_name}<…>`"),
        ));
    };
    let Some(seg) = tp.path.segments.last() else {
        return Err(syn::Error::new_spanned(first_ty, "empty path"));
    };
    if seg.ident.to_string() != context_name {
        return Err(syn::Error::new_spanned(
            &seg.ident,
            format!("expected `{context_name}`"),
        ));
    }

    match &seg.arguments {
        syn::PathArguments::None => return Ok(ContextCapsKind::DefaultEmptyCaps),
        syn::PathArguments::AngleBracketed(ab) => {
            let mut types = ab.args.iter().filter_map(|a| {
                if let syn::GenericArgument::Type(t) = a {
                    Some(t.clone())
                } else {
                    None
                }
            });
            let inner = types.next().ok_or_else(|| {
                syn::Error::new_spanned(ab, format!("missing `{context_name}` type parameter"))
            })?;
            if types.next().is_some() {
                return Err(syn::Error::new_spanned(
                    ab,
                    "too many generic arguments on context type",
                ));
            }

            if let Type::Path(ip) = &inner {
                if ip.path.segments.len() == 1 {
                    let id = &ip.path.segments[0].ident;
                    if item
                        .sig
                        .generics
                        .type_params()
                        .any(|p| p.ident == *id)
                    {
                        if sig_mentions_ident_except_first_arg(&item.sig, id) {
                            return Err(syn::Error::new_spanned(
                                ip,
                                "this type parameter must only appear on the context argument; split the function or use a concrete caps type",
                            ));
                        }

                        let mut bounds = ParsedCanBounds::default();

                        if let Some(GenericParam::Type(tp)) = item
                            .sig
                            .generics
                            .params
                            .iter_mut()
                            .find(|p| matches!(p, GenericParam::Type(t) if t.ident == *id))
                        {
                            let from_tp = strip_can_bounds_from_type_param(tp);
                            bounds.reads.extend(from_tp.reads);
                            bounds.inserts.extend(from_tp.inserts);
                            bounds.updates.extend(from_tp.updates);
                            bounds.deletes.extend(from_tp.deletes);
                        }

                        if let Some(ref mut wc) = item.sig.generics.where_clause {
                            let from_where = strip_can_bounds_from_where(wc, id)?;
                            bounds.reads.extend(from_where.reads);
                            bounds.inserts.extend(from_where.inserts);
                            bounds.updates.extend(from_where.updates);
                            bounds.deletes.extend(from_where.deletes);
                        }

                        if bounds.reads.is_empty()
                            && bounds.inserts.is_empty()
                            && bounds.updates.is_empty()
                            && bounds.deletes.is_empty()
                        {
                            return Err(syn::Error::new_spanned(
                                id,
                                "add `CanRead` / `CanInsert` / `CanUpdate` / `CanDelete` bounds on the context type parameter (for example `Caps: CanRead<Food>` or `where Caps: CanRead<Food>`)",
                            ));
                        }

                        // Remove the type parameter from generics.
                        item.sig.generics.params = item
                            .sig
                            .generics
                            .params
                            .iter()
                            .filter(|p| {
                                if let GenericParam::Type(tp) = p {
                                    tp.ident != *id
                                } else {
                                    true
                                }
                            })
                            .cloned()
                            .collect();

                        // First arg is still `ReducerContext<Caps>` with `Caps` removed from generics;
                        // the #[reducer]/#[query] macro replaces it with `Interstice_<fn>_Caps`.

                        // Drop empty where clause.
                        if let Some(ref wc) = item.sig.generics.where_clause {
                            if wc.predicates.is_empty() {
                                item.sig.generics.where_clause = None;
                            }
                        }

                        // Drop empty generics.
                        if item.sig.generics.params.is_empty()
                            && item.sig.generics.where_clause.is_none()
                        {
                            item.sig.generics = syn::Generics::default();
                        }

                        return Ok(ContextCapsKind::GenericParam {
                            _param: id.clone(),
                            bounds,
                        });
                    }
                }
            }

            Ok(ContextCapsKind::Concrete(inner))
        }
        syn::PathArguments::Parenthesized(p) => Err(syn::Error::new_spanned(
            p,
            "unexpected parenthesized path arguments",
        )),
    }
}

pub fn emit_reducer_schema_fill_from_bounds(bounds: &ParsedCanBounds) -> proc_macro2::TokenStream {
    use quote::quote;
    let read = bounds.reads.iter().map(|row_ty| {
        quote! {
            reads.push(<#row_ty as interstice_sdk::TableRow>::table_ref());
        }
    });
    let ins = bounds.inserts.iter().map(|row_ty| {
        quote! {
            inserts.push(<#row_ty as interstice_sdk::TableRow>::table_ref());
        }
    });
    let upd = bounds.updates.iter().map(|row_ty| {
        quote! {
            updates.push(<#row_ty as interstice_sdk::TableRow>::table_ref());
        }
    });
    let del = bounds.deletes.iter().map(|row_ty| {
        quote! {
            deletes.push(<#row_ty as interstice_sdk::TableRow>::table_ref());
        }
    });
    quote! {
        #(#read)*
        #(#ins)*
        #(#upd)*
        #(#del)*
    }
}

pub fn validate_query_can_bounds(bounds: &ParsedCanBounds) -> syn::Result<()> {
    if !bounds.inserts.is_empty() || !bounds.updates.is_empty() || !bounds.deletes.is_empty() {
        return Err(syn::Error::new(
            proc_macro2::Span::call_site(),
            "queries only support `CanRead<Row>` bounds on the context type parameter",
        ));
    }
    Ok(())
}

pub fn emit_query_schema_fill_from_bounds(bounds: &ParsedCanBounds) -> proc_macro2::TokenStream {
    use quote::quote;
    let read = bounds.reads.iter().map(|row_ty| {
        quote! {
            reads.push(<#row_ty as interstice_sdk::TableRow>::table_ref());
        }
    });
    quote! {
        #(#read)*
    }
}

/// Per-reducer synthetic caps for generic `where Caps: CanRead<…> + …` (replaces the type param).
pub fn emit_reducer_synthetic_caps(
    owner_ident: &Ident,
    bounds: &ParsedCanBounds,
) -> syn::Result<(Ident, proc_macro2::TokenStream)> {
    let caps_ident = synthetic_caps_ident(owner_ident);
    let read_impls = bounds.reads.iter().map(|row_ty| {
        quote! {
            impl interstice_sdk::CanRead<#row_ty> for #caps_ident {}
        }
    });
    let ins_impls = bounds.inserts.iter().map(|row_ty| {
        quote! {
            impl interstice_sdk::CanInsert<#row_ty> for #caps_ident {}
        }
    });
    let upd_impls = bounds.updates.iter().map(|row_ty| {
        quote! {
            impl interstice_sdk::CanUpdate<#row_ty> for #caps_ident {}
        }
    });
    let del_impls = bounds.deletes.iter().map(|row_ty| {
        quote! {
            impl interstice_sdk::CanDelete<#row_ty> for #caps_ident {}
        }
    });
    let fill = emit_reducer_schema_fill_from_bounds(bounds);
    let defs = quote! {
        #[derive(Copy, Clone, Default)]
        pub struct #caps_ident {}
        #( #read_impls )*
        #( #ins_impls )*
        #( #upd_impls )*
        #( #del_impls )*
        impl interstice_sdk::ReducerCaps for #caps_ident {
            fn extend_reducer_schema(
                reads: &mut Vec<interstice_sdk::ReducerTableRef>,
                inserts: &mut Vec<interstice_sdk::ReducerTableRef>,
                updates: &mut Vec<interstice_sdk::ReducerTableRef>,
                deletes: &mut Vec<interstice_sdk::ReducerTableRef>,
            ) {
                #fill
            }
        }
    };
    Ok((caps_ident, defs))
}

pub fn emit_query_synthetic_caps(
    owner_ident: &Ident,
    bounds: &ParsedCanBounds,
) -> syn::Result<(Ident, proc_macro2::TokenStream)> {
    let caps_ident = synthetic_caps_ident(owner_ident);
    let read_impls = bounds.reads.iter().map(|row_ty| {
        quote! {
            impl interstice_sdk::CanRead<#row_ty> for #caps_ident {}
        }
    });
    let fill = emit_query_schema_fill_from_bounds(bounds);
    let defs = quote! {
        #[derive(Copy, Clone, Default)]
        pub struct #caps_ident {}
        #( #read_impls )*
        impl interstice_sdk::QueryCaps for #caps_ident {
            fn extend_query_schema(reads: &mut Vec<interstice_sdk::ReducerTableRef>) {
                #fill
            }
        }
    };
    Ok((caps_ident, defs))
}
