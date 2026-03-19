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
        IntersticeValue::Struct { name, fields } => {
            if let IntersticeType::Named(type_name) = ty {
                if type_name != name {
                    return Err(format!("struct name mismatch: expected '{}', got '{}'", type_name, name));
                }
                if let Some(IntersticeTypeDef::Struct {
                    name: _name_def,
                    fields: fields_def,
                }) = type_definitions.get(type_name)
                {
                    // TODO: here the order of the field definition matter, make it not
                    for (field, field_def) in fields.iter().zip(fields_def) {
                        if field.name != field_def.name {
                            return Err(format!(
                                "struct '{}' field name mismatch: expected '{}', got '{}'",
                                type_name, field_def.name, field.name
                            ));
                        }
                        validate_value_detailed(&field.value, &field_def.field_type, type_definitions)
                            .map_err(|e| format!("struct '{}' field '{}': {}", type_name, field.name, e))?;
                    }
                    Ok(())
                } else {
                    Err(format!(
                        "type '{}' not found in type_definitions (available: [{}])",
                        type_name,
                        type_definitions.keys().cloned().collect::<Vec<_>>().join(", ")
                    ))
                }
            } else {
                Err(format!("expected Named type for struct '{}', got {:?}", name, ty))
            }
        }
        IntersticeValue::Enum {
            name,
            variant,
            value,
        } => {
            if let IntersticeType::Named(type_name) = ty {
                if type_name != name {
                    return Err(format!("enum name mismatch: expected '{}', got '{}'", type_name, name));
                }
                if let Some(IntersticeTypeDef::Enum {
                    name: _name_def,
                    variants,
                }) = type_definitions.get(type_name)
                {
                    for variant_def in variants {
                        if &variant_def.name == variant {
                            return validate_value_detailed(
                                value,
                                &variant_def.field_type,
                                type_definitions,
                            )
                            .map_err(|e| format!("enum '{}' variant '{}': {}", type_name, variant, e));
                        }
                    }
                    Err(format!("enum '{}' has no variant '{}'", type_name, variant))
                } else {
                    Err(format!(
                        "type '{}' not found in type_definitions (available: [{}])",
                        type_name,
                        type_definitions.keys().cloned().collect::<Vec<_>>().join(", ")
                    ))
                }
            } else {
                Err(format!("expected Named type for enum '{}', got {:?}", name, ty))
            }
        }
    }
}
