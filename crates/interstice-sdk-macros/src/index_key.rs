use quote::ToTokens as _;
use syn::Type;

pub fn validate_index_key_type(ty: &Type) -> Result<(), String> {
    match ty {
        Type::Path(type_path) => {
            let last_segment = type_path
                .path
                .segments
                .last()
                .ok_or_else(|| "Unsupported index key type".to_string())?;
            let ident = last_segment.ident.to_string();
            match ident.as_str() {
                "u8" | "u32" | "u64" | "i32" | "i64" | "bool" | "String" => Ok(()),
                "f32" | "f64" => Err(
                    "Float types are not supported as index keys. Use an integer, bool, String, Option, or tuple."
                        .to_string(),
                ),
                "Option" => {
                    if let syn::PathArguments::AngleBracketed(args) = &last_segment.arguments {
                        let mut inner_ty = None;
                        for arg in &args.args {
                            if let syn::GenericArgument::Type(t) = arg {
                                inner_ty = Some(t);
                                break;
                            }
                        }
                        if let Some(inner) = inner_ty {
                            validate_index_key_type(inner)
                        } else {
                            Err(
                                "Option must have a concrete type to be used as an index key"
                                    .to_string(),
                            )
                        }
                    } else {
                        Err(
                            "Option must have a concrete type to be used as an index key"
                                .to_string(),
                        )
                    }
                }
                _ => Err(format!(
                    "Type `{}` is not supported as an index key. Use an integer, bool, String, Option, or tuple.",
                    ty.to_token_stream()
                )),
            }
        }
        Type::Tuple(tuple) => {
            for elem in &tuple.elems {
                validate_index_key_type(elem)?;
            }
            Ok(())
        }
        _ => Err(format!(
            "Type `{}` is not supported as an index key. Use an integer, bool, String, Option, or tuple.",
            ty.to_token_stream()
        )),
    }
}
