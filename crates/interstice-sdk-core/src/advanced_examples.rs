//! Advanced examples of Phase 3.3-3.5 features
//!
//! Demonstrates typed tables, reducers, and event subscriptions.

#[cfg(test)]
mod tests {
    use crate::types::Serialize;
    use crate::table_handle::TableHandle;
    use crate::reducer_signature::ReducerSignature;
    use crate::subscription::{TypedEvent, Subscription, EventHandler};
    use interstice_abi::IntersticeValue;

    // ═══════════════════════════════════════════════════════════
    // Example 1: Typed Table Operations (Phase 3.3)
    // ═══════════════════════════════════════════════════════════

    #[derive(Debug, Clone)]
    struct UserRecord {
        name: String,
        age: u64,
    }

    impl Serialize for UserRecord {
        fn from_value(v: IntersticeValue) -> std::result::Result<Self, String> {
            match v {
                IntersticeValue::String(s) => {
                    let parts: Vec<&str> = s.split('|').collect();
                    if parts.len() == 2 {
                        let age = parts[1].parse::<u64>()
                            .map_err(|e| e.to_string())?;
                        Ok(UserRecord {
                            name: parts[0].to_string(),
                            age,
                        })
                    } else {
                        Err("Invalid format".to_string())
                    }
                }
                _ => Err("Expected string".to_string())
            }
        }

        fn to_value(&self) -> IntersticeValue {
            IntersticeValue::String(
                format!("{}|{}", self.name, self.age)
            )
        }
    }

    #[test]
    fn test_typed_table_operations() {
        // Create a typed table handle
        let users: TableHandle<UserRecord> = TableHandle::new("users");
        assert_eq!(users.name(), "users");

        // Type safety prevents wrong types
        let user = UserRecord {
            name: "Alice".to_string(),
            age: 30,
        };

        // Insert with type checking
        let result = users.insert(1u64, user);
        assert!(result.is_ok());
    }

    // ═══════════════════════════════════════════════════════════
    // Example 2: Typed Reducer Signatures (Phase 3.4)
    // ═══════════════════════════════════════════════════════════

    #[test]
    fn test_typed_reducer_signatures() {
        // Define reducer signatures with full type safety
        let sum_reducer: ReducerSignature<u64, u64> = 
            ReducerSignature::new("math", "sum");
        
        assert_eq!(sum_reducer.module(), "math");
        assert_eq!(sum_reducer.name(), "sum");

        // String to number reducer
        let parse_reducer: ReducerSignature<String, u64> = 
            ReducerSignature::new("parsing", "parse_number");
        
        assert_eq!(parse_reducer.module(), "parsing");
    }

    // ═══════════════════════════════════════════════════════════
    // Example 3: Typed Event Subscriptions (Phase 3.5)
    // ═══════════════════════════════════════════════════════════

    struct UserCreatedEvent;

    impl EventHandler<UserRecord> for UserCreatedEvent {
        fn on_event(&self, user: UserRecord) -> std::result::Result<(), String> {
            println!("User created: {} (age: {})", user.name, user.age);
            Ok(())
        }
    }

    #[test]
    fn test_typed_event_subscriptions() {
        // Create a typed event
        let event: TypedEvent<UserRecord> = 
            TypedEvent::new("user_created", "users");
        
        assert_eq!(event.event_name(), "user_created");
        assert_eq!(event.table_name(), "users");

        // Create a subscription
        let sub = Subscription::new(event, "handler_user_created");
        assert_eq!(sub.handler_id(), "handler_user_created");
        
        // Subscribe (would register with runtime)
        let result = sub.subscribe();
        assert!(result.is_ok());
    }

    #[test]
    fn test_event_handler_with_typed_data() {
        let handler = UserCreatedEvent;
        
        let user = UserRecord {
            name: "Bob".to_string(),
            age: 25,
        };

        let result = handler.on_event(user);
        assert!(result.is_ok());
    }

    // ═══════════════════════════════════════════════════════════
    // Example 4: Complete Module Example
    // ═══════════════════════════════════════════════════════════

    #[test]
    fn test_complete_module_workflow() {
        // 1. Create typed table
        let users: TableHandle<String> = TableHandle::new("users");

        // 2. Create reducer signature
        let count_reducer: ReducerSignature<String, u64> = 
            ReducerSignature::new("analytics", "count_users");

        // 3. Create event
        let user_updated: TypedEvent<String> = 
            TypedEvent::new("user_updated", "users");

        // 4. Verify structure
        assert_eq!(users.name(), "users");
        assert_eq!(count_reducer.module(), "analytics");
        assert_eq!(user_updated.table_name(), "users");

        // All operations are type-safe!
    }

    // ═══════════════════════════════════════════════════════════
    // Example 5: Cross-Module Communication Pattern
    // ═══════════════════════════════════════════════════════════

    #[test]
    fn test_cross_module_patterns() {
        // Module A defines a reducer
        let send_message: ReducerSignature<String, u64> = 
            ReducerSignature::new("messaging", "send");

        // Module B subscribes to events
        let message_received: TypedEvent<String> = 
            TypedEvent::new("message_received", "messages");

        let sub = Subscription::new(message_received, "on_message");

        // Everything is type-checked at compile time
        assert_eq!(send_message.module(), "messaging");
        assert_eq!(sub.event().table_name(), "messages");
    }
}
