// CLI command implementations for Interstice schema inspection and validation

use interstice_core::persistence::LogValidator;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Output format for CLI commands
#[derive(Debug, Clone, Copy)]
pub enum OutputFormat {
    Json,
    Yaml,
    Text,
}

impl OutputFormat {
    pub fn from_str(s: &str) -> Result<Self, String> {
        match s {
            "json" => Ok(OutputFormat::Json),
            "yaml" | "yml" => Ok(OutputFormat::Yaml),
            "text" | "plain" => Ok(OutputFormat::Text),
            _ => Err(format!("Unknown format: {}", s)),
        }
    }
}

/// Schema inspection result
#[derive(Debug, Serialize, Deserialize)]
pub struct SchemaInfo {
    pub module_name: String,
    pub abi_version: u16,
    pub table_count: usize,
    pub reducer_count: usize,
    pub subscription_count: usize,
}

/// Log inspection result
#[derive(Debug, Serialize, Deserialize)]
pub struct LogInspectionResult {
    pub file_path: String,
    pub transaction_count: usize,
    pub valid_transactions: usize,
    pub invalid_transactions: usize,
    pub modules_involved: Vec<String>,
    pub tables_involved: Vec<String>,
    pub log_size_bytes: u64,
    pub is_valid: bool,
}

/// Validation result
#[derive(Debug, Serialize, Deserialize)]
pub struct ValidationResult {
    pub is_valid: bool,
    pub issues: Vec<String>,
    pub warnings: Vec<String>,
}

/// Inspect a transaction log
pub fn inspect_log(log_path: &Path) -> Result<LogInspectionResult, String> {
    if !log_path.exists() {
        return Err(format!("Log file not found: {}", log_path.display()));
    }

    let file_size = std::fs::metadata(log_path)
        .map_err(|e| format!("Failed to read log file: {}", e))?
        .len();

    let validator_result = LogValidator::validate(log_path)
        .map_err(|e| format!("Failed to validate log: {}", e))?;

    let info = LogValidator::inspect(log_path)
        .map_err(|e| format!("Failed to inspect log: {}", e))?;

    Ok(LogInspectionResult {
        file_path: log_path.to_string_lossy().to_string(),
        transaction_count: info.transaction_count,
        valid_transactions: validator_result.valid_transactions,
        invalid_transactions: validator_result.errors.len(),
        modules_involved: info.modules.clone(),
        tables_involved: info.tables.clone(),
        log_size_bytes: file_size,
        is_valid: validator_result.is_valid(),
    })
}

/// Format output according to specified format
pub fn format_output<T: Serialize + std::fmt::Debug>(data: &T, format: OutputFormat) -> Result<String, String> {
    match format {
        OutputFormat::Json => {
            serde_json::to_string_pretty(data)
                .map_err(|e| format!("Failed to serialize to JSON: {}", e))
        }
        OutputFormat::Yaml => {
            // For now, fallback to JSON; YAML support can be added with serde_yaml
            serde_json::to_string_pretty(data)
                .map_err(|e| format!("Failed to serialize: {}", e))
        }
        OutputFormat::Text => {
            Ok(format!("{:#?}", data))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_output_format_from_str() {
        assert!(matches!(OutputFormat::from_str("json"), Ok(OutputFormat::Json)));
        assert!(matches!(OutputFormat::from_str("yaml"), Ok(OutputFormat::Yaml)));
        assert!(matches!(OutputFormat::from_str("text"), Ok(OutputFormat::Text)));
        assert!(OutputFormat::from_str("invalid").is_err());
    }

    #[test]
    fn test_schema_info_serialize() {
        let info = SchemaInfo {
            module_name: "test_module".to_string(),
            abi_version: 1,
            table_count: 3,
            reducer_count: 2,
            subscription_count: 1,
        };
        let json = serde_json::to_string(&info);
        assert!(json.is_ok());
    }

    #[test]
    fn test_validation_result_serialize() {
        let result = ValidationResult {
            is_valid: true,
            issues: vec![],
            warnings: vec!["Warning 1".to_string()],
        };
        let json = serde_json::to_string(&result);
        assert!(json.is_ok());
    }

    #[test]
    fn test_format_output_json() {
        let info = SchemaInfo {
            module_name: "test".to_string(),
            abi_version: 1,
            table_count: 1,
            reducer_count: 1,
            subscription_count: 0,
        };
        let formatted = format_output(&info, OutputFormat::Json);
        assert!(formatted.is_ok());
        let output = formatted.unwrap();
        assert!(output.contains("test") || output.contains("module_name"));
    }

    #[test]
    fn test_log_inspection_result_serialize() {
        let result = LogInspectionResult {
            file_path: "/tmp/txn.log".to_string(),
            transaction_count: 100,
            valid_transactions: 100,
            invalid_transactions: 0,
            modules_involved: vec!["module1".to_string()],
            tables_involved: vec!["table1".to_string()],
            log_size_bytes: 4096,
            is_valid: true,
        };
        let json = serde_json::to_string(&result);
        assert!(json.is_ok());
    }
}

