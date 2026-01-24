// Advanced log inspection and export utilities
// Provides filtering, querying, and exporting transaction logs

use serde::{Deserialize, Serialize};

/// Query filter for transaction logs
#[derive(Debug, Clone, Default)]
pub struct LogQueryFilter {
    pub module_id: Option<String>,
    pub table_name: Option<String>,
    pub operation: Option<String>,
    pub start_index: usize,
    pub limit: Option<usize>,
}

impl LogQueryFilter {
    pub fn new() -> Self {
        LogQueryFilter::default()
    }

    pub fn with_module(mut self, module_id: String) -> Self {
        self.module_id = Some(module_id);
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

    pub fn with_limit(mut self, limit: usize) -> Self {
        self.limit = Some(limit);
        self
    }

    pub fn with_start(mut self, index: usize) -> Self {
        self.start_index = index;
        self
    }

    /// Check if transaction matches filter criteria
    pub fn matches(&self, module: &str, table: &str, op: &str) -> bool {
        if let Some(ref filter_mod) = self.module_id {
            if filter_mod != module {
                return false;
            }
        }

        if let Some(ref filter_tbl) = self.table_name {
            if filter_tbl != table {
                return false;
            }
        }

        if let Some(ref filter_op) = self.operation {
            if filter_op != op {
                return false;
            }
        }

        true
    }
}

/// Log export format options
#[derive(Debug, Clone, Copy)]
pub enum ExportFormat {
    Json,
    Csv,
    Binary,
}

impl ExportFormat {
    pub fn from_str(s: &str) -> Result<Self, String> {
        match s {
            "json" => Ok(ExportFormat::Json),
            "csv" => Ok(ExportFormat::Csv),
            "binary" | "bin" => Ok(ExportFormat::Binary),
            _ => Err(format!("Unknown export format: {}", s)),
        }
    }

    pub fn extension(&self) -> &'static str {
        match self {
            ExportFormat::Json => "json",
            ExportFormat::Csv => "csv",
            ExportFormat::Binary => "bin",
        }
    }
}

/// Filtered log query result
#[derive(Debug, Serialize, Deserialize)]
pub struct FilteredLogResult {
    pub total_transactions: usize,
    pub filtered_count: usize,
    pub transactions: Vec<TransactionRecord>,
    pub filter_applied: String,
}

/// Individual transaction record for export
#[derive(Debug, Serialize, Deserialize)]
pub struct TransactionRecord {
    pub index: usize,
    pub module_id: String,
    pub table_name: String,
    pub operation: String,
    pub timestamp: u64,
    pub data_hash: String,
}

impl FilteredLogResult {
    pub fn new(total: usize, filter_desc: String) -> Self {
        FilteredLogResult {
            total_transactions: total,
            filtered_count: 0,
            transactions: Vec::new(),
            filter_applied: filter_desc,
        }
    }

    pub fn add_transaction(&mut self, record: TransactionRecord) {
        self.filtered_count += 1;
        self.transactions.push(record);
    }

    /// Export as CSV
    pub fn to_csv(&self) -> String {
        let mut csv = String::from("index,module_id,table_name,operation,timestamp,data_hash\n");

        for txn in &self.transactions {
            csv.push_str(&format!(
                "{},{},{},{},{},{}\n",
                txn.index, txn.module_id, txn.table_name, txn.operation, txn.timestamp, txn.data_hash
            ));
        }

        csv
    }

    /// Export as JSON
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_query_filter_matches() {
        let filter = LogQueryFilter::new()
            .with_module("users".to_string())
            .with_table("accounts".to_string());

        assert!(filter.matches("users", "accounts", "insert"));
        assert!(!filter.matches("products", "accounts", "insert"));
        assert!(!filter.matches("users", "orders", "insert"));
    }

    #[test]
    fn test_query_filter_with_operation() {
        let filter = LogQueryFilter::new().with_operation("delete".to_string());

        assert!(filter.matches("any_module", "any_table", "delete"));
        assert!(!filter.matches("any_module", "any_table", "insert"));
    }

    #[test]
    fn test_export_format_from_str() {
        assert!(matches!(
            ExportFormat::from_str("json"),
            Ok(ExportFormat::Json)
        ));
        assert!(matches!(
            ExportFormat::from_str("csv"),
            Ok(ExportFormat::Csv)
        ));
        assert!(ExportFormat::from_str("invalid").is_err());
    }

    #[test]
    fn test_export_format_extension() {
        assert_eq!(ExportFormat::Json.extension(), "json");
        assert_eq!(ExportFormat::Csv.extension(), "csv");
        assert_eq!(ExportFormat::Binary.extension(), "bin");
    }

    #[test]
    fn test_transaction_record_serialization() {
        let record = TransactionRecord {
            index: 1,
            module_id: "test_module".to_string(),
            table_name: "users".to_string(),
            operation: "insert".to_string(),
            timestamp: 1000,
            data_hash: "abc123".to_string(),
        };

        let json = serde_json::to_string(&record);
        assert!(json.is_ok());
    }

    #[test]
    fn test_filtered_log_result_csv_export() {
        let mut result = FilteredLogResult::new(10, "module=users".to_string());

        result.add_transaction(TransactionRecord {
            index: 1,
            module_id: "users".to_string(),
            table_name: "accounts".to_string(),
            operation: "insert".to_string(),
            timestamp: 1000,
            data_hash: "abc".to_string(),
        });

        let csv = result.to_csv();
        assert!(csv.contains("users"));
        assert!(csv.contains("insert"));
        assert!(csv.contains("accounts"));
    }

    #[test]
    fn test_filtered_log_result_json_export() {
        let mut result = FilteredLogResult::new(10, "all".to_string());

        result.add_transaction(TransactionRecord {
            index: 1,
            module_id: "mod".to_string(),
            table_name: "tbl".to_string(),
            operation: "op".to_string(),
            timestamp: 1000,
            data_hash: "hash".to_string(),
        });

        let json = result.to_json();
        assert!(json.is_ok());
    }
}
