//! `#[interstice_type]`: the schema of enums with named-field variants
//! (§10.1), and the direct encoding of structs and enums (§10.2).

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

#[interstice_type]
#[derive(Debug, PartialEq)]
pub struct Drawing {
    pub name: String,
    pub shapes: Vec<Shape>,
    pub pair: (u8, Option<i64>),
    pub data: Vec<u8>,
}

#[test]
fn direct_encoding_matches_the_tree() {
    let drawing = Drawing {
        name: "d".into(),
        shapes: vec![Shape::Empty, Shape::Circle(1.5), Shape::Rect { width: 2, height: 3 }],
        pair: (7, Some(-9)),
        data: vec![1, 2, 3],
    };
    let direct = encode_value(&drawing);
    let tree: IntersticeValue = drawing.clone().into();
    assert_eq!(direct, encode(&tree).unwrap());
    assert_eq!(decode_value::<Drawing>(&direct).unwrap(), drawing);

    let defs = interstice_sdk::registry::collect_type_definitions();
    validate_value_detailed(&tree, &IntersticeType::Named("Drawing".into()), &defs).unwrap();
    assert!(decode_value::<Shape>(&encode_value(&drawing)).is_err());
}

#[test]
fn compile_errors() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/ui/*.rs");
}
