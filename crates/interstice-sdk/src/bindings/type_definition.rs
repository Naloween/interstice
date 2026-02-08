use interstice_abi::IntersticeTypeDef;

pub fn get_type_definition_code(type_def: &IntersticeTypeDef) -> String {
    match type_def {
        IntersticeTypeDef::Struct { name, fields } => {
            let mut result =
                "#[derive(interstice_sdk::interstice_abi_macros::IntersticeType)]\npub struct "
                    .to_string()
                    + &name
                    + "{\n";
            for field in fields {
                result += &("   pub ".to_string()
                    + &field.name
                    + ": "
                    + &field.field_type.to_string()
                    + ",\n");
            }
            result += "}\n";
            return result;
        }
        IntersticeTypeDef::Enum { name, variants } => {
            let mut result =
                "#[derive(interstice_sdk::interstice_abi_macros::IntersticeType)]\npub enum "
                    .to_string()
                    + &name
                    + "{\n";
            for variant in variants {
                match &variant.field_type {
                    interstice_abi::IntersticeType::Void => {
                        result += &(variant.name.clone() + ",\n");
                    }
                    interstice_abi::IntersticeType::Tuple(interstice_types) => {
                        let mut inners = String::new();
                        for field_type in interstice_types {
                            inners += &(field_type.to_string() + ", ");
                        }

                        result += &(variant.name.clone() + "(" + &inners + ")" + ",\n");
                    }
                    field_type => {
                        result +=
                            &(variant.name.clone() + "(" + &field_type.to_string() + ")" + ",\n");
                    }
                }
            }
            result += "}\n";
            return result;
        }
    };
}
