pub mod bindings;
pub mod macros;

pub use interstice_abi;
pub use interstice_abi::*;
pub use interstice_sdk_core::*;
pub use interstice_sdk_macros::*;
pub use wee_alloc;

pub use std::str::FromStr;

pub fn to_snake_case(name: &str) -> String {
    name.trim().to_lowercase().replace("-", "_")
}

pub fn snake_to_camel_case(name: &str) -> String {
    let node_type_str =
        name.chars().nth(0).unwrap().to_uppercase().to_string() + &name[1..name.len()];
    // Remove "_" and add uppercase to the following character
    node_type_str
        .split('_')
        .map(|s| {
            let mut chars = s.chars();
            match chars.next() {
                None => String::new(),
                Some(f) => f.to_uppercase().to_string() + chars.as_str(),
            }
        })
        .collect::<String>()
}
