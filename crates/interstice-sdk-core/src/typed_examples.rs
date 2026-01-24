//! Guide and examples for using the typed SDK
//!
//! This module demonstrates how to use the Serialize trait
//! for strongly-typed module development.

#[cfg(test)]
mod tests {
    use crate::types::Serialize;
    use interstice_abi::IntersticeValue;

    // Example 1: Manual Serialize implementation
    #[derive(Debug, Clone)]
    struct User {
        id: u64,
        name: String,
    }

    impl Serialize for User {
        fn from_value(v: IntersticeValue) -> Result<Self, String> {
            match v {
                IntersticeValue::String(s) => {
                    // Simple: assume "id:name" format
                    let parts: Vec<&str> = s.split(':').collect();
                    if parts.len() == 2 {
                        let id = parts[0].parse::<u64>()
                            .map_err(|e| e.to_string())?;
                        Ok(User { id, name: parts[1].to_string() })
                    } else {
                        Err("Invalid user format".to_string())
                    }
                }
                _ => Err(format!("Cannot convert {:?} to User", v))
            }
        }

        fn to_value(&self) -> IntersticeValue {
            IntersticeValue::String(
                format!("{}:{}", self.id, self.name)
            )
        }
    }

    #[test]
    fn test_custom_type_conversion() {
        let user = User {
            id: 1,
            name: "Alice".to_string(),
        };

        let value = user.to_value();
        let user2 = User::from_value(value).unwrap();

        assert_eq!(user2.id, user.id);
        assert_eq!(user2.name, user.name);
    }

    #[test]
    fn test_built_in_types() {
        let s = "hello".to_string();
        let v = s.to_value();
        let s2 = String::from_value(v).unwrap();
        assert_eq!(s, s2);

        let n = 42u64;
        let v = n.to_value();
        let n2 = u64::from_value(v).unwrap();
        assert_eq!(n, n2);
    }

    #[test]
    fn test_f32_conversion() {
        let f = 3.14f32;
        let v = f.to_value();
        let f2 = f32::from_value(v).unwrap();
        assert!((f - f2).abs() < 0.001);
    }

    #[test]
    fn test_all_numeric_types() {
        // Test u32
        let v = 42u32;
        assert_eq!(u32::from_value(v.to_value()).unwrap(), v);

        // Test i64
        let v = -42i64;
        assert_eq!(i64::from_value(v.to_value()).unwrap(), v);

        // Test f64
        let v = 3.14159f64;
        let v2 = f64::from_value(v.to_value()).unwrap();
        assert!((v - v2).abs() < 0.00001);
    }
}

