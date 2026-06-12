use interstice_sdk::*;

#[interstice_type]
#[derive(Debug, PartialEq)]
pub enum LayoutDirection {
    Row,
    Column,
}

#[interstice_type]
#[derive(Debug, PartialEq)]
pub enum Size {
    Fixed(f32),
    Grow,
    Fit,
}

#[interstice_type]
#[derive(Debug, PartialEq)]
pub enum TextWrap {
    None,
    Words,
    Newlines,
}
