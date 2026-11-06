//! Values shown to a person, with the names from the schema (DESIGN §10.2).

use std::collections::HashMap;
use std::fmt::Write;

use crate::{IntersticeType, IntersticeTypeDef, IntersticeValue};

/// `value` as Rust-like text, with the struct, field and variant names of
/// `ty` and `definitions`: `Point { x: 1, y: 2 }`, `Shape::Circle(3)`.
/// Parts that don't match the type print without names.
pub fn render(value: &IntersticeValue, ty: &IntersticeType, definitions: &HashMap<String, IntersticeTypeDef>) -> String {
    let mut out = String::new();
    write_value(&mut out, value, ty, definitions);
    out
}

fn write_list(out: &mut String, items: &[IntersticeValue], types: &mut dyn FnMut(usize) -> Option<IntersticeType>, open: &str, close: &str, definitions: &HashMap<String, IntersticeTypeDef>) {
    out.push_str(open);
    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        match types(i) {
            Some(ty) => write_value(out, item, &ty, definitions),
            None => {
                let _ = write!(out, "{item}");
            }
        }
    }
    out.push_str(close);
}

fn write_value(out: &mut String, value: &IntersticeValue, ty: &IntersticeType, definitions: &HashMap<String, IntersticeTypeDef>) {
    match (value, ty) {
        (IntersticeValue::Vec(items), IntersticeType::Vec(inner)) => {
            write_list(out, items, &mut |_| Some((**inner).clone()), "[", "]", definitions)
        }
        (IntersticeValue::Option(Some(v)), IntersticeType::Option(inner)) => {
            out.push_str("Some(");
            write_value(out, v, inner, definitions);
            out.push(')');
        }
        (IntersticeValue::Tuple(items), IntersticeType::Tuple(types)) => {
            write_list(out, items, &mut |i| types.get(i).cloned(), "(", ")", definitions)
        }
        (IntersticeValue::Struct(fields), IntersticeType::Named(name)) => match definitions.get(name) {
            Some(IntersticeTypeDef::Struct { fields: defs, .. }) if defs.len() == fields.len() => {
                let _ = write!(out, "{name} {{ ");
                for (i, (field, def)) in fields.iter().zip(defs).enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    let _ = write!(out, "{}: ", def.name);
                    write_value(out, field, &def.field_type, definitions);
                }
                out.push_str(" }");
            }
            _ => {
                let _ = write!(out, "{value}");
            }
        },
        (IntersticeValue::Enum { variant, value: payload }, IntersticeType::Named(name)) => {
            match definitions.get(name) {
                Some(IntersticeTypeDef::Enum { variants, .. }) if (*variant as usize) < variants.len() => {
                    let def = &variants[*variant as usize];
                    let _ = write!(out, "{name}::{}", def.name);
                    match (payload.as_ref(), &def.field_type) {
                        (IntersticeValue::Void, _) => {}
                        // A named-field variant: `Shape::Rect { w: 1, h: 2 }`.
                        (IntersticeValue::Struct(_), IntersticeType::Named(payload_name))
                            if *payload_name == format!("{name}::{}", def.name) =>
                        {
                            let inner = render(payload, &def.field_type, definitions);
                            out.push_str(inner.strip_prefix(payload_name.as_str()).unwrap_or(&inner));
                        }
                        (IntersticeValue::Tuple(_), _) => write_value(out, payload, &def.field_type, definitions),
                        (_, ty) => {
                            out.push('(');
                            write_value(out, payload, ty, definitions);
                            out.push(')');
                        }
                    }
                }
                _ => {
                    let _ = write!(out, "{value}");
                }
            }
        }
        _ => {
            let _ = write!(out, "{value}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::FieldDef;

    #[test]
    fn names_come_from_the_schema() {
        let mut defs = HashMap::new();
        let field = |name: &str, field_type| FieldDef { name: name.into(), field_type };
        defs.insert("Point".into(), IntersticeTypeDef::Struct {
            name: "Point".into(),
            fields: vec![field("x", IntersticeType::U32), field("y", IntersticeType::U32)],
        });
        defs.insert("Shape::Rect".into(), IntersticeTypeDef::Struct {
            name: "Shape::Rect".into(),
            fields: vec![field("w", IntersticeType::U32)],
        });
        defs.insert("Shape".into(), IntersticeTypeDef::Enum {
            name: "Shape".into(),
            variants: vec![
                field("Empty", IntersticeType::Void),
                field("At", IntersticeType::Named("Point".into())),
                field("Rect", IntersticeType::Named("Shape::Rect".into())),
                field("Pair", IntersticeType::Tuple(vec![IntersticeType::U32, IntersticeType::String])),
            ],
        });
        let point = IntersticeValue::Struct(vec![IntersticeValue::U32(1), IntersticeValue::U32(2)]);
        let shape = IntersticeType::Named("Shape".into());
        let en = |variant, value| IntersticeValue::Enum { variant, value: Box::new(value) };
        assert_eq!(render(&point, &IntersticeType::Named("Point".into()), &defs), "Point { x: 1, y: 2 }");
        assert_eq!(render(&en(0, IntersticeValue::Void), &shape, &defs), "Shape::Empty");
        assert_eq!(render(&en(1, point.clone()), &shape, &defs), "Shape::At(Point { x: 1, y: 2 })");
        assert_eq!(render(&en(2, IntersticeValue::Struct(vec![IntersticeValue::U32(3)])), &shape, &defs), "Shape::Rect { w: 3 }");
        assert_eq!(
            render(&en(3, IntersticeValue::Tuple(vec![IntersticeValue::U32(4), IntersticeValue::String("a".into())])), &shape, &defs),
            "Shape::Pair(4, \"a\")"
        );
        assert_eq!(point.to_string(), "{1, 2}");
    }
}
