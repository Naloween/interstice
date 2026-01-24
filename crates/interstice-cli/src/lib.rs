// CLI command implementations for Interstice schema inspection and validation

use interstice_abi::ModuleSchema;
use serde::{Deserialize, Serialize};
use std::path::Path;

pub mod advanced_log;
pub mod tracer;

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

/// Schema compatibility change
#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum CompatibilityChange {
    TableAdded(String),
    TableRemoved(String),
    ReducerAdded(String),
    ReducerRemoved(String),
    FieldAdded {
        table: String,
        field: String,
    },
    FieldRemoved {
        table: String,
        field: String,
    },
    FieldTypeChanged {
        table: String,
        field: String,
        from: String,
        to: String,
    },
}

/// Schema diff result
#[derive(Debug, Serialize, Deserialize)]
pub struct SchemaDiffResult {
    pub old_module: String,
    pub new_module: String,
    pub is_compatible: bool,
    pub breaking_changes: Vec<CompatibilityChange>,
    pub compatible_additions: Vec<CompatibilityChange>,
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

/// Dry-run load result
#[derive(Debug, Serialize, Deserialize)]
pub struct DryRunResult {
    pub is_loadable: bool,
    pub abi_version: u16,
    pub missing_dependencies: Vec<String>,
    pub schema_errors: Vec<String>,
    pub warnings: Vec<String>,
}

/// Inspect a transaction log
// pub fn inspect_log(log_path: &Path) -> Result<LogInspectionResult, String> {
//     if !log_path.exists() {
//         return Err(format!("Log file not found: {}", log_path.display()));
//     }

//     let file_size = std::fs::metadata(log_path)
//         .map_err(|e| format!("Failed to read log file: {}", e))?
//         .len();

//     Ok(LogInspectionResult {
//         file_path: log_path.to_string_lossy().to_string(),
//         transaction_count: info.transaction_count,
//         valid_transactions: validator_result.valid_transactions,
//         invalid_transactions: validator_result.errors.len(),
//         modules_involved: info.modules.clone(),
//         tables_involved: info.tables.clone(),
//         log_size_bytes: file_size,
//         is_valid: validator_result.is_valid(),
//     })
// }

/// Compare two module schemas for compatibility
pub fn diff_schemas(old_path: &Path, new_path: &Path) -> Result<SchemaDiffResult, String> {
    // Read old schema
    let old_schema = std::fs::read_to_string(old_path)
        .map_err(|e| format!("Failed to read old schema: {}", e))?;
    let old: ModuleSchema = serde_json::from_str(&old_schema)
        .map_err(|e| format!("Failed to parse old schema: {}", e))?;

    // Read new schema
    let new_schema = std::fs::read_to_string(new_path)
        .map_err(|e| format!("Failed to read new schema: {}", e))?;
    let new: ModuleSchema = serde_json::from_str(&new_schema)
        .map_err(|e| format!("Failed to parse new schema: {}", e))?;

    let mut breaking_changes = Vec::new();
    let mut compatible_additions = Vec::new();

    // Check for removed/changed tables
    for old_table in &old.tables {
        if !new.tables.iter().any(|t| t.name == old_table.name) {
            breaking_changes.push(CompatibilityChange::TableRemoved(old_table.name.clone()));
        }
    }

    // Check for removed/changed reducers
    for old_reducer in &old.reducers {
        if !new.reducers.iter().any(|r| r.name == old_reducer.name) {
            breaking_changes.push(CompatibilityChange::ReducerRemoved(
                old_reducer.name.clone(),
            ));
        }
    }

    // Check for added tables/reducers (non-breaking)
    for new_table in &new.tables {
        if !old.tables.iter().any(|t| t.name == new_table.name) {
            compatible_additions.push(CompatibilityChange::TableAdded(new_table.name.clone()));
        }
    }

    for new_reducer in &new.reducers {
        if !old.reducers.iter().any(|r| r.name == new_reducer.name) {
            compatible_additions.push(CompatibilityChange::ReducerAdded(new_reducer.name.clone()));
        }
    }

    let is_compatible = breaking_changes.is_empty();

    Ok(SchemaDiffResult {
        old_module: old.name.clone(),
        new_module: new.name.clone(),
        is_compatible,
        breaking_changes,
        compatible_additions,
    })
}

/// Perform a dry-run module load (schema validation only, no execution)
pub fn dry_run_module_load(module_path: &Path) -> Result<DryRunResult, String> {
    if !module_path.exists() {
        return Err(format!("Module not found: {}", module_path.display()));
    }

    let schema_str = std::fs::read_to_string(module_path)
        .map_err(|e| format!("Failed to read module: {}", e))?;

    let interface: Result<ModuleSchema, _> = serde_json::from_str(&schema_str);

    match interface {
        Ok(iface) => {
            let mut warnings = Vec::new();

            // Basic validation checks
            if iface.tables.is_empty() {
                warnings.push("Module has no tables defined".to_string());
            }

            if iface.reducers.is_empty() {
                warnings.push("Module has no reducers defined".to_string());
            }

            Ok(DryRunResult {
                is_loadable: true,
                abi_version: iface.version.major,
                missing_dependencies: Vec::new(),
                schema_errors: Vec::new(),
                warnings,
            })
        }
        Err(e) => Ok(DryRunResult {
            is_loadable: false,
            abi_version: 0,
            missing_dependencies: Vec::new(),
            schema_errors: vec![format!("Schema parse error: {}", e)],
            warnings: Vec::new(),
        }),
    }
}

/// Format output according to specified format
pub fn format_output<T: Serialize + std::fmt::Debug>(
    data: &T,
    format: OutputFormat,
) -> Result<String, String> {
    match format {
        OutputFormat::Json => serde_json::to_string_pretty(data)
            .map_err(|e| format!("Failed to serialize to JSON: {}", e)),
        OutputFormat::Yaml => {
            // For now, fallback to JSON; YAML support can be added with serde_yaml
            serde_json::to_string_pretty(data).map_err(|e| format!("Failed to serialize: {}", e))
        }
        OutputFormat::Text => Ok(format!("{:#?}", data)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_output_format_from_str() {
        assert!(matches!(
            OutputFormat::from_str("json"),
            Ok(OutputFormat::Json)
        ));
        assert!(matches!(
            OutputFormat::from_str("yaml"),
            Ok(OutputFormat::Yaml)
        ));
        assert!(matches!(
            OutputFormat::from_str("text"),
            Ok(OutputFormat::Text)
        ));
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

    #[test]
    fn test_dry_run_result_serialize() {
        let result = DryRunResult {
            is_loadable: true,
            abi_version: 1,
            missing_dependencies: vec![],
            schema_errors: vec![],
            warnings: vec!["No tables".to_string()],
        };
        let json = serde_json::to_string(&result);
        assert!(json.is_ok());
    }

    #[test]
    fn test_schema_diff_result_serialize() {
        let result = SchemaDiffResult {
            old_module: "module_v1".to_string(),
            new_module: "module_v2".to_string(),
            is_compatible: true,
            breaking_changes: vec![],
            compatible_additions: vec![CompatibilityChange::TableAdded("new_table".to_string())],
        };
        let json = serde_json::to_string(&result);
        assert!(json.is_ok());
    }
}
