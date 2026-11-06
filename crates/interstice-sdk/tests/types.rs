//! Schema of `#[interstice_type]` enums with named-field variants (§10.1):
//! the payload is the struct `Enum::Variant`, and values validate.

use interstice_sdk::*;

#[interstice_type]
#[derive(Debug, PartialEq)]
pub enum Shape {
    Empty,
    Circle(f64),
    Rect { width: u32, height: u32 },
}

#[test]
fn named_variant_fields_are_in_the_schema() {
    let defs = interstice_sdk::registry::collect_type_definitions();
    let Some(IntersticeTypeDef::Enum { variants, .. }) = defs.get("Shape") else {
        panic!("Shape not registered: {:?}", defs.keys().collect::<Vec<_>>());
    };
    let rect = variants.iter().find(|v| v.name == "Rect").unwrap();
    assert!(matches!(&rect.field_type, IntersticeType::Named(n) if n == "Shape::Rect"), "{:?}", rect.field_type);
    let Some(IntersticeTypeDef::Struct { fields, .. }) = defs.get("Shape::Rect") else {
        panic!("Shape::Rect not registered");
    };
    let names: Vec<_> = fields.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, ["width", "height"]);

    for shape in [Shape::Empty, Shape::Circle(1.5), Shape::Rect { width: 2, height: 3 }] {
        let value: IntersticeValue = shape.clone().into();
        validate_value_detailed(&value, &IntersticeType::Named("Shape".into()), &defs)
            .unwrap_or_else(|e| panic!("{shape:?}: {e}"));
        let back: Shape = value.try_into().unwrap();
        assert_eq!(back, shape);
    }
}

#[test]
fn compile_errors() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/ui/*.rs");
}
