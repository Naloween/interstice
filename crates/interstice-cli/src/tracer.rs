// Execution tracing for reducer calls and transactions
// Enables detailed visibility into module execution flow and timing

use serde::{Deserialize, Serialize};
use std::time::SystemTime;
use std::collections::HashMap;

/// A single span in an execution trace (e.g., a reducer call)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceSpan {
    pub span_id: u64,
    pub parent_span_id: Option<u64>,
    pub name: String,
    pub module_id: String,
    pub reducer_name: Option<String>,
    pub start_time_us: u64,
    pub end_time_us: Option<u64>,
    pub duration_us: Option<u64>,
    pub status: SpanStatus,
    pub attributes: HashMap<String, String>,
}

/// Status of a trace span
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SpanStatus {
    Running,
    Completed,
    Failed(String),
}

impl TraceSpan {
    pub fn new(span_id: u64, name: impl Into<String>, module_id: impl Into<String>) -> Self {
        TraceSpan {
            span_id,
            parent_span_id: None,
            name: name.into(),
            module_id: module_id.into(),
            reducer_name: None,
            start_time_us: current_time_us(),
            end_time_us: None,
            duration_us: None,
            status: SpanStatus::Running,
            attributes: HashMap::new(),
        }
    }

    pub fn with_parent(mut self, parent_id: u64) -> Self {
        self.parent_span_id = Some(parent_id);
        self
    }

    pub fn with_reducer(mut self, reducer_name: impl Into<String>) -> Self {
        self.reducer_name = Some(reducer_name.into());
        self
    }

    pub fn add_attribute(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.attributes.insert(key.into(), value.into());
    }

    pub fn complete(&mut self) {
        self.end_time_us = Some(current_time_us());
        if self.start_time_us != 0 {
            if let Some(end) = self.end_time_us {
                self.duration_us = Some(end - self.start_time_us);
            }
        }
        self.status = SpanStatus::Completed;
    }

    pub fn fail(&mut self, error: impl Into<String>) {
        self.end_time_us = Some(current_time_us());
        if let Some(end) = self.end_time_us {
            self.duration_us = Some(end - self.start_time_us);
        }
        self.status = SpanStatus::Failed(error.into());
    }
}

/// Complete execution trace for an operation
#[derive(Debug, Serialize, Deserialize)]
pub struct ExecutionTrace {
    pub trace_id: String,
    pub root_name: String,
    pub start_time_us: u64,
    pub end_time_us: Option<u64>,
    pub total_duration_us: Option<u64>,
    pub spans: Vec<TraceSpan>,
}

impl ExecutionTrace {
    pub fn new(trace_id: impl Into<String>, root_name: impl Into<String>) -> Self {
        ExecutionTrace {
            trace_id: trace_id.into(),
            root_name: root_name.into(),
            start_time_us: current_time_us(),
            end_time_us: None,
            total_duration_us: None,
            spans: Vec::new(),
        }
    }

    pub fn add_span(&mut self, span: TraceSpan) {
        self.spans.push(span);
    }

    pub fn finish(&mut self) {
        self.end_time_us = Some(current_time_us());
        if let Some(end) = self.end_time_us {
            self.total_duration_us = Some(end - self.start_time_us);
        }
    }

    /// Generate a summary of the trace
    pub fn summary(&self) -> TraceSummary {
        let mut module_call_count: HashMap<String, usize> = HashMap::new();
        let mut reducer_call_count: HashMap<String, usize> = HashMap::new();
        let mut total_time_by_reducer: HashMap<String, u64> = HashMap::new();

        for span in &self.spans {
            *module_call_count.entry(span.module_id.clone()).or_insert(0) += 1;

            if let Some(ref reducer) = span.reducer_name {
                *reducer_call_count.entry(reducer.clone()).or_insert(0) += 1;
                if let Some(duration) = span.duration_us {
                    *total_time_by_reducer.entry(reducer.clone()).or_insert(0) += duration;
                }
            }
        }

        TraceSummary {
            trace_id: self.trace_id.clone(),
            total_duration_us: self.total_duration_us.unwrap_or(0),
            span_count: self.spans.len(),
            module_call_count,
            reducer_call_count,
            total_time_by_reducer,
        }
    }
}

/// Summary statistics from an execution trace
#[derive(Debug, Serialize, Deserialize)]
pub struct TraceSummary {
    pub trace_id: String,
    pub total_duration_us: u64,
    pub span_count: usize,
    pub module_call_count: HashMap<String, usize>,
    pub reducer_call_count: HashMap<String, usize>,
    pub total_time_by_reducer: HashMap<String, u64>,
}

/// Get current time in microseconds
fn current_time_us() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trace_span_creation() {
        let span = TraceSpan::new(1, "test_span", "test_module");
        assert_eq!(span.span_id, 1);
        assert_eq!(span.name, "test_span");
        assert_eq!(span.module_id, "test_module");
        assert!(matches!(span.status, SpanStatus::Running));
    }

    #[test]
    fn test_trace_span_with_reducer() {
        let span = TraceSpan::new(1, "call", "module")
            .with_reducer("my_reducer");
        assert_eq!(span.reducer_name, Some("my_reducer".to_string()));
    }

    #[test]
    fn test_trace_span_completion() {
        let mut span = TraceSpan::new(1, "test", "module");
        assert!(matches!(span.status, SpanStatus::Running));
        
        span.complete();
        assert!(matches!(span.status, SpanStatus::Completed));
        assert!(span.duration_us.is_some());
    }

    #[test]
    fn test_trace_span_failure() {
        let mut span = TraceSpan::new(1, "test", "module");
        span.fail("Something went wrong");
        
        assert!(matches!(span.status, SpanStatus::Failed(_)));
        assert!(span.duration_us.is_some());
    }

    #[test]
    fn test_execution_trace_creation() {
        let trace = ExecutionTrace::new("trace-123", "root_op");
        assert_eq!(trace.trace_id, "trace-123");
        assert_eq!(trace.root_name, "root_op");
        assert!(trace.spans.is_empty());
    }

    #[test]
    fn test_execution_trace_summary() {
        let mut trace = ExecutionTrace::new("trace-1", "root");

        let mut span1 = TraceSpan::new(1, "op1", "module_a");
        span1.reducer_name = Some("reducer_x".to_string());
        span1.complete();

        let mut span2 = TraceSpan::new(2, "op2", "module_a");
        span2.reducer_name = Some("reducer_y".to_string());
        span2.complete();

        trace.add_span(span1);
        trace.add_span(span2);
        trace.finish();

        let summary = trace.summary();
        assert_eq!(summary.span_count, 2);
        assert_eq!(summary.module_call_count.get("module_a"), Some(&2));
        assert_eq!(summary.reducer_call_count.get("reducer_x"), Some(&1));
        assert_eq!(summary.reducer_call_count.get("reducer_y"), Some(&1));
    }

    #[test]
    fn test_trace_span_serialization() {
        let span = TraceSpan::new(1, "test", "module")
            .with_reducer("reducer");
        
        let json = serde_json::to_string(&span);
        assert!(json.is_ok());
    }

    #[test]
    fn test_execution_trace_serialization() {
        let trace = ExecutionTrace::new("t1", "root");
        let json = serde_json::to_string(&trace);
        assert!(json.is_ok());
    }
}
