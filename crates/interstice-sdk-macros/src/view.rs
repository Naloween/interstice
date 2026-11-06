//! `#[view]` (DESIGN §7.5).
//!
//! - **Filter view**: `fn name(r: &Row, viewer: &Viewer) -> bool { <condition> }`
//!   (the `viewer` parameter is optional). The condition is a restricted
//!   closure; it is recorded as a `ViewExpr` the kernel evaluates, and the
//!   function is emitted unchanged so rustc type-checks it.
//! - **Function view**: `fn name<Caps>(ctx: ViewContext<Caps>[, viewer: &Viewer])
//!   -> Vec<T>` (or `Result<Vec<T>, E>`) with `key = field`. It becomes an
//!   internal query `view_fn_<name>` the kernel runs.

use proc_macro::TokenStream;
use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::{format_ident, quote, quote_spanned};
use syn::{
    BinOp, Expr, FnArg, ItemFn, Lit, Member, Meta, Pat, ReturnType, Type, UnOp,
    parse_macro_input, punctuated::Punctuated, spanned::Spanned, token::Comma,
};

struct Options {
    visibility: TokenStream2,
    from: Option<syn::Path>,
    key: Option<String>,
}

fn parse_options(attributes: &Punctuated<Meta, Comma>) -> syn::Result<Options> {
    let mut from = None;
    let mut key = None;
    for meta in attributes {
        match meta {
            m if crate::visibility::visibility_option(m).is_some() => {}
            Meta::NameValue(nv) if nv.path.is_ident("from") => match &nv.value {
                Expr::Path(p) => from = Some(p.path.clone()),
                other => return Err(syn::Error::new_spanned(other, "expected a table type: `from = HttpResponse`")),
            },
            Meta::NameValue(nv) if nv.path.is_ident("key") => match &nv.value {
                Expr::Path(p) if p.path.get_ident().is_some() => {
                    key = Some(p.path.get_ident().unwrap().to_string())
                }
                other => return Err(syn::Error::new_spanned(other, "expected a field name: `key = id`")),
            },
            other => {
                return Err(syn::Error::new_spanned(
                    other,
                    "unsupported #[view] option; use `public` / `local` / `internal`, `from = Table` (filter views) and `key = field` (function views)",
                ));
            }
        }
    }
    Ok(Options { visibility: crate::visibility::declared_visibility(attributes)?, from, key })
}

fn param(arg: &FnArg) -> Option<(String, &Type)> {
    let FnArg::Typed(pt) = arg else { return None };
    let Pat::Ident(id) = &*pt.pat else { return None };
    Some((id.ident.to_string(), &*pt.ty))
}

fn last_ident(ty: &Type) -> Option<String> {
    match ty {
        Type::Path(p) => p.path.segments.last().map(|s| s.ident.to_string()),
        Type::Reference(r) => last_ident(&r.elem),
        _ => None,
    }
}

fn returns_bool(output: &ReturnType) -> bool {
    matches!(output, ReturnType::Type(_, ty) if last_ident(ty).as_deref() == Some("bool"))
}

pub fn view_macro(attr: TokenStream, item: TokenStream) -> TokenStream {
    let attributes: Punctuated<Meta, Comma> = if attr.is_empty() {
        Punctuated::new()
    } else {
        parse_macro_input!(attr with Punctuated::<Meta, Comma>::parse_terminated)
    };
    let input_fn = parse_macro_input!(item as ItemFn);
    let options = match parse_options(&attributes) {
        Ok(o) => o,
        Err(e) => return e.into_compile_error().into(),
    };
    let first_is_ref = input_fn.sig.inputs.first().and_then(param).is_some_and(|(_, ty)| matches!(ty, Type::Reference(_)));
    let result = if first_is_ref && returns_bool(&input_fn.sig.output) {
        filter_view(input_fn, options)
    } else {
        function_view(input_fn, options)
    };
    match result {
        Ok(tokens) => tokens.into(),
        Err(e) => e.into_compile_error().into(),
    }
}

// ── Filter views ────────────────────────────────────────────────────────────────

fn filter_view(input_fn: ItemFn, options: Options) -> syn::Result<TokenStream2> {
    let name = input_fn.sig.ident.to_string();
    let inputs: Vec<_> = input_fn.sig.inputs.iter().collect();
    if inputs.is_empty() || inputs.len() > 2 {
        return Err(syn::Error::new_spanned(
            &input_fn.sig.inputs,
            "a filter view takes `(row: &Table)` or `(row: &Table, viewer: &Viewer)`",
        ));
    }
    let (row_var, row_ty) = param(inputs[0]).ok_or_else(|| syn::Error::new_spanned(inputs[0], "expected `row: &Table`"))?;
    let table_type = last_ident(row_ty).ok_or_else(|| syn::Error::new_spanned(row_ty, "expected `&Table`"))?;
    if let Some(from) = &options.from {
        let from_name = from.segments.last().map(|s| s.ident.to_string()).unwrap_or_default();
        if from_name != table_type {
            return Err(syn::Error::new_spanned(from, format!("`from = {from_name}` but the row parameter is `&{table_type}`")));
        }
    }
    let viewer_var = match inputs.get(1) {
        Some(arg) => {
            let (var, ty) = param(arg).ok_or_else(|| syn::Error::new_spanned(arg, "expected `viewer: &Viewer`"))?;
            if last_ident(ty).as_deref() != Some("Viewer") {
                return Err(syn::Error::new_spanned(ty, "the second parameter of a filter view is `&Viewer`"));
            }
            Some(var)
        }
        None => None,
    };
    let body = match input_fn.block.stmts.as_slice() {
        [syn::Stmt::Expr(expr, None)] => expr,
        _ => {
            return Err(syn::Error::new_spanned(
                &input_fn.block,
                "a filter view's body is one condition, e.g. `r.owner == viewer.module`",
            ))
        }
    };
    let ctx = Translate { row: row_var, viewer: viewer_var.clone() };
    let predicate = ctx.expr(body)?;
    let per_viewer = viewer_var.is_some();
    let table_name = table_type.to_lowercase();
    let visibility = options.visibility;
    let register = format_ident!("__interstice_register_view_{}", name);
    let ident = &input_fn.sig.ident;
    Ok(quote! {
        #[allow(dead_code)]
        #input_fn

        #[interstice_sdk::init]
        fn #register() {
            interstice_sdk::registry::register_view(|| interstice_sdk::ViewSchema {
                name: stringify!(#ident).to_string(),
                visibility: #visibility,
                row_type: String::new(),
                primary_key: String::new(),
                per_viewer: #per_viewer,
                kind: interstice_sdk::ViewKind::Filter {
                    table: #table_name.to_string(),
                    predicate: #predicate,
                },
            });
        }
    })
}

/// Translates the restricted closure into `ViewExpr` constructors.
struct Translate {
    row: String,
    viewer: Option<String>,
}

impl Translate {
    fn reject(&self, span: Span, what: &str) -> syn::Error {
        syn::Error::new(
            span,
            format!(
                "{what} is not allowed in a filter view; use `{}.<column>`, `viewer.node` / `viewer.module`, literals, `== != < <= > >=`, `&& || !` and parentheses",
                self.row
            ),
        )
    }

    fn expr(&self, expr: &Expr) -> syn::Result<TokenStream2> {
        let span = expr.span();
        match expr {
            Expr::Paren(p) => self.expr(&p.expr),
            Expr::Group(g) => self.expr(&g.expr),
            Expr::Binary(b) => {
                let (l, r) = (self.expr(&b.left)?, self.expr(&b.right)?);
                let cmp = |op: TokenStream2| {
                    quote_spanned! {span=> interstice_sdk::ViewExpr::Compare {
                        op: interstice_sdk::CompareOp::#op,
                        left: Box::new(#l),
                        right: Box::new(#r),
                    }}
                };
                Ok(match b.op {
                    BinOp::Eq(_) => cmp(quote!(Eq)),
                    BinOp::Ne(_) => cmp(quote!(Ne)),
                    BinOp::Lt(_) => cmp(quote!(Lt)),
                    BinOp::Le(_) => cmp(quote!(Le)),
                    BinOp::Gt(_) => cmp(quote!(Gt)),
                    BinOp::Ge(_) => cmp(quote!(Ge)),
                    BinOp::And(_) => quote! { interstice_sdk::ViewExpr::And(Box::new(#l), Box::new(#r)) },
                    BinOp::Or(_) => quote! { interstice_sdk::ViewExpr::Or(Box::new(#l), Box::new(#r)) },
                    _ => return Err(self.reject(b.op.span(), "this operator")),
                })
            }
            Expr::Unary(u) => match (&u.op, &*u.expr) {
                (UnOp::Not(_), inner) => {
                    let inner = self.expr(inner)?;
                    Ok(quote! { interstice_sdk::ViewExpr::Not(Box::new(#inner)) })
                }
                (UnOp::Neg(_), Expr::Lit(lit)) => match &lit.lit {
                    Lit::Int(i) => {
                        let n: i64 = i.base10_parse::<i64>()?.checked_neg().ok_or_else(|| syn::Error::new(span, "out of range"))?;
                        Ok(quote! { interstice_sdk::ViewExpr::Literal(interstice_sdk::ViewLiteral::Int(#n)) })
                    }
                    Lit::Float(f) => {
                        let n: f64 = -f.base10_parse::<f64>()?;
                        Ok(quote! { interstice_sdk::ViewExpr::Literal(interstice_sdk::ViewLiteral::Float(#n)) })
                    }
                    _ => Err(self.reject(span, "this literal")),
                },
                _ => Err(self.reject(span, "this expression")),
            },
            Expr::Field(f) => {
                let Expr::Path(base) = &*f.base else { return Err(self.reject(span, "a nested field")) };
                let Some(base) = base.path.get_ident().map(|i| i.to_string()) else {
                    return Err(self.reject(span, "this path"));
                };
                let Member::Named(member) = &f.member else { return Err(self.reject(span, "a tuple field")) };
                let member = member.to_string();
                if base == self.row {
                    Ok(quote! { interstice_sdk::ViewExpr::Column(#member.to_string()) })
                } else if Some(&base) == self.viewer.as_ref() {
                    match member.as_str() {
                        "node" => Ok(quote! { interstice_sdk::ViewExpr::Viewer(interstice_sdk::ViewerField::Node) }),
                        "module" => Ok(quote! { interstice_sdk::ViewExpr::Viewer(interstice_sdk::ViewerField::Module) }),
                        _ => Err(syn::Error::new(span, "a viewer has `node` and `module`")),
                    }
                } else {
                    Err(self.reject(span, &format!("`{base}`")))
                }
            }
            Expr::Lit(lit) => match &lit.lit {
                Lit::Str(s) => {
                    let v = s.value();
                    Ok(quote! { interstice_sdk::ViewExpr::Literal(interstice_sdk::ViewLiteral::Str(#v.to_string())) })
                }
                Lit::Int(i) => {
                    let n: i64 = i.base10_parse()?;
                    Ok(quote! { interstice_sdk::ViewExpr::Literal(interstice_sdk::ViewLiteral::Int(#n)) })
                }
                Lit::Float(f) => {
                    let n: f64 = f.base10_parse()?;
                    Ok(quote! { interstice_sdk::ViewExpr::Literal(interstice_sdk::ViewLiteral::Float(#n)) })
                }
                Lit::Bool(b) => {
                    let v = b.value;
                    Ok(quote! { interstice_sdk::ViewExpr::Literal(interstice_sdk::ViewLiteral::Bool(#v)) })
                }
                _ => Err(self.reject(span, "this literal")),
            },
            Expr::Path(p) if p.path.get_ident().is_some() => {
                Err(self.reject(span, &format!("`{}` on its own", p.path.get_ident().unwrap())))
            }
            Expr::MethodCall(_) => Err(self.reject(span, "a method call")),
            Expr::Call(_) => Err(self.reject(span, "a function call")),
            _ => Err(self.reject(span, "this expression")),
        }
    }
}

// ── Function views ──────────────────────────────────────────────────────────────

/// `T` of `Vec<T>` or `Result<Vec<T>, E>`.
fn row_type_of(output: &ReturnType) -> Option<String> {
    let ReturnType::Type(_, ty) = output else { return None };
    let mut ty: &Type = ty;
    if let Some(ok) = crate::query::result_ok_type(ty) {
        ty = ok;
    }
    let Type::Path(p) = ty else { return None };
    let seg = p.path.segments.last()?;
    if seg.ident != "Vec" {
        return None;
    }
    let syn::PathArguments::AngleBracketed(args) = &seg.arguments else { return None };
    match args.args.first()? {
        syn::GenericArgument::Type(inner) => last_ident(inner),
        _ => None,
    }
}

fn function_view(input_fn: ItemFn, options: Options) -> syn::Result<TokenStream2> {
    let name = input_fn.sig.ident.clone();
    let key = options.key.clone().ok_or_else(|| {
        syn::Error::new_spanned(&input_fn.sig.ident, "a function view needs its rows' primary key: `#[view(key = id)]`")
    })?;
    if options.from.is_some() {
        return Err(syn::Error::new_spanned(
            &input_fn.sig.ident,
            "`from` is for filter views: `fn name(r: &Table, viewer: &Viewer) -> bool`",
        ));
    }
    let row_type = row_type_of(&input_fn.sig.output).ok_or_else(|| {
        syn::Error::new_spanned(&input_fn.sig.output, "a function view returns `Vec<Row>` or `Result<Vec<Row>, E>`")
    })?;
    let inputs: Vec<_> = input_fn.sig.inputs.iter().collect();
    let (_, ctx_ty) = inputs
        .first()
        .and_then(|a| param(a))
        .ok_or_else(|| syn::Error::new_spanned(&input_fn.sig, "a function view takes `ctx: ViewContext<Caps>` first"))?;
    if last_ident(ctx_ty).as_deref() != Some("ViewContext") {
        return Err(syn::Error::new_spanned(ctx_ty, "the first parameter of a function view is `ViewContext<Caps>`"));
    }
    let per_viewer = match inputs.get(1) {
        Some(arg) => {
            let (_, ty) = param(arg).ok_or_else(|| syn::Error::new_spanned(arg, "expected `viewer: &Viewer`"))?;
            if last_ident(ty).as_deref() != Some("Viewer") || inputs.len() > 2 {
                return Err(syn::Error::new_spanned(arg, "a function view takes `(ctx: ViewContext<Caps>)` or `(ctx, viewer: &Viewer)`"));
            }
            true
        }
        None => false,
    };

    // The query's context type: the user's `ViewContext<…>` as `QueryContext<…>`.
    let mut query_ctx_ty = ctx_ty.clone();
    if let Type::Path(p) = &mut query_ctx_ty {
        if let Some(last) = p.path.segments.last_mut() {
            last.ident = syn::Ident::new("QueryContext", last.ident.span());
        }
    }
    let impl_ident = format_ident!("__interstice_view_impl_{}", name);
    let query_ident = format_ident!("view_fn_{}", name);
    let query_name = query_ident.to_string();
    let mut impl_fn = input_fn.clone();
    impl_fn.sig.ident = impl_ident.clone();
    let generics = &input_fn.sig.generics;
    let where_clause = &input_fn.sig.generics.where_clause;
    let output = &input_fn.sig.output;
    let call = if per_viewer {
        quote! { #impl_ident(ctx, &__viewer) }
    } else {
        quote! { #impl_ident(ctx) }
    };
    let visibility = options.visibility;
    let register = format_ident!("__interstice_register_view_{}", name);
    Ok(quote! {
        #[allow(dead_code)]
        #impl_fn

        #[interstice_sdk::query(internal)]
        fn #query_ident #generics (ctx: #query_ctx_ty, viewer_node: String, viewer_module: String) #output #where_clause {
            #[allow(unused_variables)]
            let __viewer = interstice_sdk::Viewer { node: viewer_node, module: viewer_module };
            #call
        }

        #[interstice_sdk::init]
        fn #register() {
            interstice_sdk::registry::register_view(|| interstice_sdk::ViewSchema {
                name: stringify!(#name).to_string(),
                visibility: #visibility,
                row_type: #row_type.to_string(),
                primary_key: #key.to_string(),
                per_viewer: #per_viewer,
                kind: interstice_sdk::ViewKind::Function {
                    query: #query_name.to_string(),
                    reads: Vec::new(),
                },
            });
        }
    })
}
