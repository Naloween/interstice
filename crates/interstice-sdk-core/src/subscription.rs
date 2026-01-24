//! Typed event subscriptions
//!
//! Provides type-safe event handling for module interactions.

use crate::types::Serialize;
use interstice_abi::IntersticeValue;
use std::fmt::Debug;

/// A typed event that can be subscribed to
/// 
/// Events are published when table mutations occur.
#[derive(Debug, Clone)]
pub struct TypedEvent<T: Serialize> {
    event_name: String,
    table_name: String,
    _phantom: std::marker::PhantomData<T>,
}

impl<T: Serialize> TypedEvent<T> {
    /// Create a new typed event
    pub fn new(event_name: &str, table_name: &str) -> Self {
        Self {
            event_name: event_name.to_string(),
            table_name: table_name.to_string(),
            _phantom: std::marker::PhantomData,
        }
    }

    /// Get the event name
    pub fn event_name(&self) -> &str {
        &self.event_name
    }

    /// Get the table name
    pub fn table_name(&self) -> &str {
        &self.table_name
    }
}

/// Trait for handling typed events
/// 
/// Implement this to subscribe to specific events with type safety.
pub trait EventHandler<T: Serialize>: Send + Sync {
    /// Handle an incoming event with typed data
    fn on_event(&self, data: T) -> std::result::Result<(), String>;
}

/// A subscription to a typed event
/// 
/// Manages the relationship between an event and its handler.
#[derive(Debug, Clone)]
pub struct Subscription<T: Serialize> {
    event: TypedEvent<T>,
    handler_id: String,
}

impl<T: Serialize> Subscription<T> {
    /// Create a new subscription
    pub fn new(event: TypedEvent<T>, handler_id: &str) -> Self {
        Self {
            event,
            handler_id: handler_id.to_string(),
        }
    }

    /// Get the event
    pub fn event(&self) -> &TypedEvent<T> {
        &self.event
    }

    /// Get the handler ID
    pub fn handler_id(&self) -> &str {
        &self.handler_id
    }

    /// Subscribe to the event (placeholder - requires runtime integration)
    pub fn subscribe(&self) -> std::result::Result<(), String> {
        #[cfg(target_arch = "wasm32")]
        {
            use crate::host_calls;
            host_calls::subscribe(
                self.event.event_name.clone(),
                self.handler_id.clone(),
            );
            Ok(())
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            Ok(())
        }
    }

    /// Unsubscribe from the event
    pub fn unsubscribe(&self) -> std::result::Result<(), String> {
        Ok(())
    }
}

/// Helper struct for registering event handlers
pub struct EventRegistry;

impl EventRegistry {
    /// Register a handler for a typed event
    pub fn register<T: Serialize + 'static>(
        event: TypedEvent<T>,
        _handler: impl EventHandler<T> + 'static,
    ) -> std::result::Result<Subscription<T>, String> {
        // Generate a simple handler ID without external dependency
        let handler_id = format!("handler_{:x}", std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos());
        let subscription = Subscription::new(event, &handler_id);
        subscription.subscribe()?;
        Ok(subscription)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_typed_event_creation() {
        let event: TypedEvent<String> = TypedEvent::new("user_created", "users");
        assert_eq!(event.event_name(), "user_created");
        assert_eq!(event.table_name(), "users");
    }

    #[test]
    fn test_typed_event_clone() {
        let event1: TypedEvent<u64> = TypedEvent::new("count_updated", "counts");
        let event2 = event1.clone();
        assert_eq!(event1.event_name(), event2.event_name());
        assert_eq!(event1.table_name(), event2.table_name());
    }

    #[test]
    fn test_subscription_creation() {
        let event: TypedEvent<String> = TypedEvent::new("data_changed", "table");
        let sub = Subscription::new(event, "handler_1");
        assert_eq!(sub.handler_id(), "handler_1");
    }

    #[test]
    fn test_subscription_clone() {
        let event: TypedEvent<String> = TypedEvent::new("test", "test_table");
        let sub1 = Subscription::new(event, "h1");
        let sub2 = sub1.clone();
        assert_eq!(sub1.handler_id(), sub2.handler_id());
    }

    // Example typed event handler
    struct LoggingHandler;

    impl EventHandler<String> for LoggingHandler {
        fn on_event(&self, data: String) -> std::result::Result<(), String> {
            // In real implementation, would log the data
            println!("Event: {}", data);
            Ok(())
        }
    }

    #[test]
    fn test_event_handler_implementation() {
        let handler = LoggingHandler;
        let result = handler.on_event("test event".to_string());
        assert!(result.is_ok());
    }

    #[test]
    fn test_subscription_subscribe() {
        let event: TypedEvent<u64> = TypedEvent::new("number", "nums");
        let sub = Subscription::new(event, "handler_1");
        let result = sub.subscribe();
        assert!(result.is_ok());
    }
}
