// Structured logging infrastructure for Interstice runtime and modules
// Provides context-aware logging with support for tracing reducer calls and mutations

use std::fmt;

/// Log level for filtering output
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LogLevel {
    Trace = 0,
    Debug = 1,
    Info = 2,
    Warn = 3,
    Error = 4,
}

impl fmt::Display for LogLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LogLevel::Trace => write!(f, "TRACE"),
            LogLevel::Debug => write!(f, "DEBUG"),
            LogLevel::Info => write!(f, "INFO"),
            LogLevel::Warn => write!(f, "WARN"),
            LogLevel::Error => write!(f, "ERROR"),
        }
    }
}

/// Context for a log event - includes module, reducer, and operation info
#[derive(Debug, Clone)]
pub struct LogContext {
    pub module_id: Option<String>,
    pub reducer_name: Option<String>,
    pub table_name: Option<String>,
    pub operation: Option<String>,
}

impl LogContext {
    pub fn new() -> Self {
        LogContext {
            module_id: None,
            reducer_name: None,
            table_name: None,
            operation: None,
        }
    }

    pub fn with_module(mut self, module_id: String) -> Self {
        self.module_id = Some(module_id);
        self
    }

    pub fn with_reducer(mut self, reducer_name: String) -> Self {
        self.reducer_name = Some(reducer_name);
        self
    }

    pub fn with_table(mut self, table_name: String) -> Self {
        self.table_name = Some(table_name);
        self
    }

    pub fn with_operation(mut self, operation: String) -> Self {
        self.operation = Some(operation);
        self
    }
}

impl Default for LogContext {
    fn default() -> Self {
        Self::new()
    }
}

/// Structured log event
#[derive(Debug, Clone)]
pub struct LogEvent {
    pub level: LogLevel,
    pub timestamp: String,
    pub message: String,
    pub context: LogContext,
    pub fields: Vec<(String, String)>,
}

impl LogEvent {
    pub fn new(level: LogLevel, message: impl Into<String>) -> Self {
        LogEvent {
            level,
            timestamp: chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f").to_string(),
            message: message.into(),
            context: LogContext::new(),
            fields: Vec::new(),
        }
    }

    pub fn with_context(mut self, context: LogContext) -> Self {
        self.context = context;
        self
    }

    pub fn with_field(mut self, key: String, value: String) -> Self {
        self.fields.push((key, value));
        self
    }

    /// Format as human-readable text with context
    pub fn format_text(&self) -> String {
        let mut output = format!(
            "[{}] {} - {}",
            self.level, self.timestamp, self.message
        );

        if let Some(module) = &self.context.module_id {
            output.push_str(&format!(" [module: {}]", module));
        }

        if let Some(reducer) = &self.context.reducer_name {
            output.push_str(&format!(" [reducer: {}]", reducer));
        }

        if let Some(table) = &self.context.table_name {
            output.push_str(&format!(" [table: {}]", table));
        }

        if let Some(op) = &self.context.operation {
            output.push_str(&format!(" [op: {}]", op));
        }

        for (key, value) in &self.fields {
            output.push_str(&format!(" {}={}", key, value));
        }

        output
    }

    /// Format as JSON for structured parsing
    pub fn format_json(&self) -> serde_json::Result<String> {
        use serde_json::json;

        let mut obj = json!({
            "level": self.level.to_string(),
            "timestamp": self.timestamp,
            "message": self.message,
        });

        let obj_map = obj.as_object_mut().unwrap();

        if let Some(module) = &self.context.module_id {
            obj_map.insert("module_id".to_string(), json!(module));
        }

        if let Some(reducer) = &self.context.reducer_name {
            obj_map.insert("reducer".to_string(), json!(reducer));
        }

        if let Some(table) = &self.context.table_name {
            obj_map.insert("table".to_string(), json!(table));
        }

        if let Some(op) = &self.context.operation {
            obj_map.insert("operation".to_string(), json!(op));
        }

        if !self.fields.is_empty() {
            let mut fields = serde_json::Map::new();
            for (key, value) in &self.fields {
                fields.insert(key.clone(), json!(value));
            }
            obj_map.insert("fields".to_string(), json!(fields));
        }

        serde_json::to_string(&obj)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_log_level_ordering() {
        assert!(LogLevel::Trace < LogLevel::Debug);
        assert!(LogLevel::Error > LogLevel::Warn);
    }

    #[test]
    fn test_log_context_builder() {
        let ctx = LogContext::new()
            .with_module("test_module".to_string())
            .with_reducer("my_reducer".to_string())
            .with_table("users".to_string());

        assert_eq!(ctx.module_id, Some("test_module".to_string()));
        assert_eq!(ctx.reducer_name, Some("my_reducer".to_string()));
        assert_eq!(ctx.table_name, Some("users".to_string()));
    }

    #[test]
    fn test_log_event_text_format() {
        let event = LogEvent::new(LogLevel::Info, "User inserted")
            .with_context(
                LogContext::new()
                    .with_table("users".to_string())
                    .with_operation("insert".to_string()),
            )
            .with_field("row_id".to_string(), "123".to_string());

        let text = event.format_text();
        assert!(text.contains("INFO"));
        assert!(text.contains("User inserted"));
        assert!(text.contains("[table: users]"));
        assert!(text.contains("[op: insert]"));
        assert!(text.contains("row_id=123"));
    }

    #[test]
    fn test_log_event_json_format() {
        let event = LogEvent::new(LogLevel::Warn, "High latency")
            .with_context(LogContext::new().with_module("analytics".to_string()))
            .with_field("latency_ms".to_string(), "500".to_string());

        let json = event.format_json();
        assert!(json.is_ok());
        let text = json.unwrap();
        assert!(text.contains("WARN"));
        assert!(text.contains("High latency"));
        assert!(text.contains("analytics"));
    }
}
