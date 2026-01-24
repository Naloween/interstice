// Determinism verification tools for Interstice
// Replays logs multiple times and compares results to detect non-deterministic behavior

use crate::persistence::{TransactionLog, ReplayEngine};
use std::path::Path;
use serde::{Deserialize, Serialize};

/// Result of a single replay run
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplaySnapshot {
    pub run_number: u32,
    pub transaction_count: usize,
    pub final_state_hash: u64,
    pub duration_ms: u128,
}

/// Determinism check result
#[derive(Debug, Serialize, Deserialize)]
pub struct DeterminismCheckResult {
    pub is_deterministic: bool,
    pub runs_performed: usize,
    pub snapshots: Vec<ReplaySnapshot>,
    pub divergence_point: Option<usize>,
    pub error_message: Option<String>,
}

impl DeterminismCheckResult {
    pub fn new(runs: usize) -> Self {
        DeterminismCheckResult {
            is_deterministic: true,
            runs_performed: runs,
            snapshots: Vec::with_capacity(runs),
            divergence_point: None,
            error_message: None,
        }
    }

    pub fn add_snapshot(&mut self, snapshot: ReplaySnapshot) {
        self.snapshots.push(snapshot);
    }

    pub fn mark_divergence(&mut self, at_transaction: usize, message: impl Into<String>) {
        self.is_deterministic = false;
        self.divergence_point = Some(at_transaction);
        self.error_message = Some(message.into());
    }
}

/// Verify determinism by replaying log multiple times
/// Note: This is a stub implementation - full implementation requires state snapshot capability
pub fn check_determinism(
    log_path: &Path,
    runs: usize,
) -> Result<DeterminismCheckResult, String> {
    if !log_path.exists() {
        return Err(format!("Log file not found: {}", log_path.display()));
    }

    if runs < 2 {
        return Err("Must perform at least 2 runs to check determinism".to_string());
    }

    let mut result = DeterminismCheckResult::new(runs);

    // Placeholder implementation - actual implementation would:
    // 1. Replay log once, capture state snapshots
    // 2. Replay again with same input, compare snapshots
    // 3. Report any divergence

    for run_num in 1..=runs as u32 {
        let snapshot = ReplaySnapshot {
            run_number: run_num,
            transaction_count: 0,
            final_state_hash: 0,
            duration_ms: 0,
        };
        result.add_snapshot(snapshot);
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_determinism_result_creation() {
        let result = DeterminismCheckResult::new(3);
        assert_eq!(result.runs_performed, 3);
        assert!(result.is_deterministic);
        assert!(result.error_message.is_none());
    }

    #[test]
    fn test_determinism_divergence_marking() {
        let mut result = DeterminismCheckResult::new(2);
        result.mark_divergence(42, "State mismatch at transaction 42");

        assert!(!result.is_deterministic);
        assert_eq!(result.divergence_point, Some(42));
        assert!(result.error_message.is_some());
    }

    #[test]
    fn test_snapshot_serialization() {
        let snapshot = ReplaySnapshot {
            run_number: 1,
            transaction_count: 100,
            final_state_hash: 0xdead_beef,
            duration_ms: 150,
        };

        let json = serde_json::to_string(&snapshot);
        assert!(json.is_ok());
    }
}
