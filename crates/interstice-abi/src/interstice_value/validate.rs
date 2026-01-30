use std::collections::HashMap;

use crate::{IntersticeType, IntersticeTypeDef, IntersticeValue};

pub fn validate_value(
    value: &IntersticeValue,
    ty: &IntersticeType,
    type_definitions: &HashMap<String, IntersticeTypeDef>,
) -> bool {
    match value {
        IntersticeValue::Void => {
            if let IntersticeType::Void = ty {
                true
            } else {
                false
            }
        }
        IntersticeValue::Bool(_) => {
            if let IntersticeType::Bool = ty {
                true
            } else {
                false
            }
        }
        IntersticeValue::U32(_) => {
            if let IntersticeType::U32 = ty {
                true
            } else {
                false
            }
        }
        IntersticeValue::U64(_) => {
            if let IntersticeType::U64 = ty {
                true
            } else {
                false
            }
        }
        IntersticeValue::I32(_) => {
            if let IntersticeType::I32 = ty {
                true
            } else {
                false
            }
        }
        IntersticeValue::I64(_) => {
            if let IntersticeType::I64 = ty {
                true
            } else {
                false
            }
        }
        IntersticeValue::F32(_) => {
            if let IntersticeType::F32 = ty {
                true
            } else {
                false
            }
        }
        IntersticeValue::F64(_) => {
            if let IntersticeType::F64 = ty {
                true
            } else {
                false
            }
        }
        IntersticeValue::String(_) => {
            if let IntersticeType::String = ty {
                true
            } else {
                false
            }
        }
        IntersticeValue::Vec(v) => {
            if let IntersticeType::Vec(inner) = ty {
                v.iter().all(|x| validate_value(x, inner, type_definitions))
            } else {
                false
            }
        }
        IntersticeValue::Option(None) => {
            if let IntersticeType::Option(_) = ty {
                true
            } else {
                false
            }
        }
        IntersticeValue::Option(Some(v)) => {
            if let IntersticeType::Option(inner) = ty {
                validate_value(v, inner, type_definitions)
            } else {
                false
            }
        }
        IntersticeValue::Tuple(interstice_values) => {
            if let IntersticeType::Tuple(inner) = ty {
                inner
                    .iter()
                    .zip(interstice_values)
                    .all(|(inner_ty, inner_val)| {
                        validate_value(inner_val, inner_ty, type_definitions)
                    })
            } else {
                false
            }
        }
        IntersticeValue::Struct { name, fields } => {
            if let IntersticeType::Named(type_name) = ty {
                if type_name != name {
                    return false;
                }
                if let Some(IntersticeTypeDef::Struct {
                    name: _name_def,
                    fields: fields_def,
                }) = type_definitions.get(type_name)
                {
                    // TODO: here the order of the field definition matter, make it not
                    fields.iter().zip(fields_def).all(|(field, field_def)| {
                        field.name == field_def.name
                            && validate_value(&field.value, &field_def.field_type, type_definitions)
                    })
                } else {
                    false
                }
            } else {
                false
            }
        }
        IntersticeValue::Enum {
            name,
            variant,
            value,
        } => {
            if let IntersticeType::Named(type_name) = ty {
                if type_name != name {
                    return false;
                }
                if let Some(IntersticeTypeDef::Enum {
                    name: _name_def,
                    variants,
                }) = type_definitions.get(type_name)
                {
                    for variant_def in variants {
                        if &variant_def.name == variant {
                            return validate_value(
                                value,
                                &variant_def.field_type,
                                type_definitions,
                            );
                        }
                    }
                    false
                } else {
                    false
                }
            } else {
                false
            }
        }
    }
}
