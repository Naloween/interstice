use std::collections::HashMap;

use crate::{IntersticeType, IntersticeTypeDef, IntersticeValue};

pub fn validate_value(
    value: &IntersticeValue,
    ty: &IntersticeType,
    type_definitions: &HashMap<String, IntersticeTypeDef>,
) -> bool {
    validate_value_detailed(value, ty, type_definitions).is_ok()
}

pub fn validate_value_detailed(
    value: &IntersticeValue,
    ty: &IntersticeType,
    type_definitions: &HashMap<String, IntersticeTypeDef>,
) -> Result<(), String> {
    match value {
        IntersticeValue::Void => {
            if let IntersticeType::Void = ty { Ok(()) } else { Err(format!("expected Void, got {:?}", ty)) }
        }
        IntersticeValue::Bool(_) => {
            if let IntersticeType::Bool = ty { Ok(()) } else { Err(format!("expected Bool, got {:?}", ty)) }
        }
        IntersticeValue::U32(_) => {
            if let IntersticeType::U32 = ty { Ok(()) } else { Err(format!("expected U32, got {:?}", ty)) }
        }
        IntersticeValue::U8(_) => {
            if let IntersticeType::U8 = ty { Ok(()) } else { Err(format!("expected U8, got {:?}", ty)) }
        }
        IntersticeValue::U64(_) => {
            if let IntersticeType::U64 = ty { Ok(()) } else { Err(format!("expected U64, got {:?}", ty)) }
        }
        IntersticeValue::I32(_) => {
            if let IntersticeType::I32 = ty { Ok(()) } else { Err(format!("expected I32, got {:?}", ty)) }
        }
        IntersticeValue::I64(_) => {
            if let IntersticeType::I64 = ty { Ok(()) } else { Err(format!("expected I64, got {:?}", ty)) }
        }
        IntersticeValue::F32(_) => {
            if let IntersticeType::F32 = ty { Ok(()) } else { Err(format!("expected F32, got {:?}", ty)) }
        }
        IntersticeValue::F64(_) => {
            if let IntersticeType::F64 = ty { Ok(()) } else { Err(format!("expected F64, got {:?}", ty)) }
        }
        IntersticeValue::String(_) => {
            if let IntersticeType::String = ty { Ok(()) } else { Err(format!("expected String, got {:?}", ty)) }
        }
        IntersticeValue::Bytes(_) => match ty {
            IntersticeType::Vec(inner) if matches!(**inner, IntersticeType::U8) => Ok(()),
            // Inline bytes for a blob parameter (e.g. arriving from another
            // node); the runtime registers them as a blob before the call.
            IntersticeType::Blob => Ok(()),
            _ => Err(format!("expected Vec<u8>, got {:?}", ty)),
        },
        IntersticeValue::Blob(_) => match ty {
            IntersticeType::Blob => Ok(()),
            _ => Err(format!("expected Blob, got {:?}", ty)),
        },
        IntersticeValue::Vec(v) => {
            if let IntersticeType::Vec(inner) = ty {
                for (i, x) in v.iter().enumerate() {
                    validate_value_detailed(x, inner, type_definitions)
                        .map_err(|e| format!("Vec[{}]: {}", i, e))?;
                }
                Ok(())
            } else {
                Err(format!("expected Vec, got {:?}", ty))
            }
        }
        IntersticeValue::Option(None) => {
            if let IntersticeType::Option(_) = ty { Ok(()) } else { Err(format!("expected Option, got {:?}", ty)) }
        }
        IntersticeValue::Option(Some(v)) => {
            if let IntersticeType::Option(inner) = ty {
                validate_value_detailed(v, inner, type_definitions)
                    .map_err(|e| format!("Option<Some>: {}", e))
            } else {
                Err(format!("expected Option, got {:?}", ty))
            }
        }
        IntersticeValue::Tuple(interstice_values) => {
            if let IntersticeType::Tuple(inner) = ty {
                for (i, (inner_ty, inner_val)) in inner.iter().zip(interstice_values).enumerate() {
                    validate_value_detailed(inner_val, inner_ty, type_definitions)
                        .map_err(|e| format!("Tuple[{}]: {}", i, e))?;
                }
                Ok(())
            } else {
                Err(format!("expected Tuple, got {:?}", ty))
            }
        }
        IntersticeValue::Struct(fields) => {
            let IntersticeType::Named(type_name) = ty else {
                return Err(format!("expected {ty}, got a struct"));
            };
            match type_definitions.get(type_name) {
                Some(IntersticeTypeDef::Struct { fields: defs, .. }) => {
                    if fields.len() != defs.len() {
                        return Err(format!("struct '{type_name}' has {} fields, got {}", defs.len(), fields.len()));
                    }
                    for (field, def) in fields.iter().zip(defs) {
                        validate_value_detailed(field, &def.field_type, type_definitions)
                            .map_err(|e| format!("{type_name}.{}: {e}", def.name))?;
                    }
                    Ok(())
                }
                Some(_) => Err(format!("'{type_name}' is an enum, got a struct")),
                None => Err(format!("type '{type_name}' is not defined")),
            }
        }
        IntersticeValue::Enum { variant, value } => {
            let IntersticeType::Named(type_name) = ty else {
                return Err(format!("expected {ty}, got an enum"));
            };
            match type_definitions.get(type_name) {
                Some(IntersticeTypeDef::Enum { variants, .. }) => {
                    let def = variants
                        .get(*variant as usize)
                        .ok_or_else(|| format!("enum '{type_name}' has no variant #{variant}"))?;
                    validate_value_detailed(value, &def.field_type, type_definitions)
                        .map_err(|e| format!("{type_name}::{}: {e}", def.name))
                }
                Some(_) => Err(format!("'{type_name}' is a struct, got an enum")),
                None => Err(format!("type '{type_name}' is not defined")),
            }
        }
    }
}
