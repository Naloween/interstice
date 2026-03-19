use interstice_core::{IntersticeError, interstice_abi::IntersticeValue};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub(crate) const DEFAULT_DURATION_MS: u64 = 30_000;
pub(crate) const DEFAULT_WARMUP_MS: u64 = 5_000;
pub(crate) const DEFAULT_CONNECTIONS: usize = 1;
pub(crate) const DEFAULT_LATENCY_SAMPLE_STRIDE: u64 = 64;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ThroughputMode {
    DispatchSuccess,
    QueryDelta,
}

impl Default for ThroughputMode {
    fn default() -> Self {
        Self::DispatchSuccess
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum BenchmarkTransport {
    Persistent,
    Reconnect,
}

impl Default for BenchmarkTransport {
    fn default() -> Self {
        Self::Persistent
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum VerifyMode {
    None,
    Query,
}

impl Default for VerifyMode {
    fn default() -> Self {
        Self::None
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifyConfig {
    #[serde(default)]
    pub mode: VerifyMode,
    #[serde(default)]
    pub query_name: Option<String>,
    #[serde(default = "default_args_json")]
    pub args_json: Value,
    #[serde(default)]
    pub args_interstice_json: Option<IntersticeValue>,
    #[serde(default)]
    pub expect_json: Option<Value>,
    #[serde(default)]
    pub expect_interstice_json: Option<IntersticeValue>,
}

impl Default for VerifyConfig {
    fn default() -> Self {
        Self {
            mode: VerifyMode::None,
            query_name: None,
            args_json: default_args_json(),
            args_interstice_json: None,
            expect_json: None,
            expect_interstice_json: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkRunConfig {
    pub node: String,
    pub module_name: String,
    pub reducer_name: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default = "default_duration_ms")]
    pub duration_ms: u64,
    #[serde(default = "default_warmup_ms")]
    pub warmup_ms: u64,
    #[serde(default = "default_connections")]
    pub connections: usize,
    #[serde(default)]
    pub transport: BenchmarkTransport,
    #[serde(default)]
    pub rate: Option<u64>,
    #[serde(default = "default_args_json")]
    pub args_json: Value,
    #[serde(default)]
    pub args_interstice_json: Option<IntersticeValue>,
    #[serde(default)]
    pub verify: VerifyConfig,
    #[serde(default = "default_latency_sample_stride")]
    pub latency_sample_stride: u64,
    #[serde(default)]
    pub throughput_mode: ThroughputMode,
    #[serde(default)]
    pub throughput_query_name: Option<String>,
    #[serde(default = "default_args_json")]
    pub throughput_query_args_json: Value,
    #[serde(default)]
    pub throughput_query_args_interstice_json: Option<IntersticeValue>,
    #[serde(default)]
    pub throughput_query_field: Option<String>,
    #[serde(default)]
    pub output: Option<String>,
    #[serde(default)]
    pub reset_before: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ScenarioCollection {
    #[serde(default)]
    pub(crate) scenarios: Vec<BenchmarkRunConfig>,
    #[serde(default)]
    pub(crate) scenario: Vec<BenchmarkRunConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationReport {
    pub mode: VerifyMode,
    pub query_name: Option<String>,
    pub result: Option<IntersticeValue>,
    pub expected: Option<IntersticeValue>,
    pub matched: Option<bool>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkReport {
    pub name: String,
    pub node: String,
    pub module_name: String,
    pub reducer_name: String,
    pub transport: BenchmarkTransport,
    pub warmup_ms: u64,
    pub duration_ms: u64,
    pub connections: usize,
    pub rate: Option<u64>,
    pub output: Option<String>,
    pub sent_warmup: u64,
    pub sent_measured: u64,
    pub failed: u64,
    pub throughput_kind: String,
    pub throughput_tps: f64,
    pub throughput_counter_start: Option<u64>,
    pub throughput_counter_end: Option<u64>,
    pub throughput_window_ms: Option<u64>,
    pub write_latency_us_mean: f64,
    pub write_latency_us_p50: f64,
    pub write_latency_us_p95: f64,
    pub write_latency_us_p99: f64,
    pub write_latency_us_max: f64,
    pub started_at_ms: u64,
    pub ended_at_ms: u64,
    pub worker_summaries: Vec<WorkerSummary>,
    pub verification: Option<VerificationReport>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerSummary {
    pub worker_id: usize,
    pub client_id: String,
    pub attempted: u64,
    pub measured_sent: u64,
    pub last_seq: u64,
}

#[derive(Debug, Default)]
pub(crate) struct WorkerMetrics {
    pub(crate) warmup_sent: u64,
    pub(crate) measured_sent: u64,
    pub(crate) failed: u64,
    pub(crate) write_latency_sum_ns: u128,
    pub(crate) write_latency_max_ns: u64,
    pub(crate) latency_samples_ns: Vec<u64>,
    pub(crate) summaries: Vec<WorkerSummary>,
}

#[derive(Debug, Clone)]
pub(crate) struct TemplateContext {
    pub(crate) seq: u64,
    pub(crate) worker: usize,
    pub(crate) op: u64,
    pub(crate) client: String,
    pub(crate) now_ms: u64,
    pub(crate) max_seq: u64,
    pub(crate) max_client: String,
    pub(crate) total_sent: u64,
}

impl BenchmarkRunConfig {
    pub(crate) fn normalize(&mut self) -> Result<(), IntersticeError> {
        if self.duration_ms == 0 {
            return Err(IntersticeError::Internal(
                "duration_ms must be greater than 0".into(),
            ));
        }
        if self.connections == 0 {
            return Err(IntersticeError::Internal(
                "connections must be greater than 0".into(),
            ));
        }
        if self.latency_sample_stride == 0 {
            self.latency_sample_stride = 1;
        }
        if self.throughput_mode == ThroughputMode::QueryDelta
            && self.throughput_query_name.is_none()
            && self.verify.query_name.is_none()
        {
            return Err(IntersticeError::Internal(
                "throughput_mode=query_delta requires --throughput-query or --verify-query".into(),
            ));
        }
        // QueryDelta now supports warmup: the start snapshot is taken after warmup completes.
        Ok(())
    }

    pub(crate) fn display_name(&self) -> String {
        self.name.clone().unwrap_or_else(|| {
            format!(
                "{}:{}:{}",
                self.node.clone(),
                self.module_name.clone(),
                self.reducer_name.clone()
            )
        })
    }

    pub(crate) fn rate_per_worker(&self) -> Option<f64> {
        self.rate.and_then(|total_rate| {
            if total_rate == 0 {
                None
            } else {
                Some(total_rate as f64 / self.connections as f64)
            }
        })
    }
}

fn default_duration_ms() -> u64 {
    DEFAULT_DURATION_MS
}

fn default_warmup_ms() -> u64 {
    DEFAULT_WARMUP_MS
}

fn default_connections() -> usize {
    DEFAULT_CONNECTIONS
}

pub(crate) fn default_args_json() -> Value {
    Value::Array(vec![])
}

fn default_latency_sample_stride() -> u64 {
    DEFAULT_LATENCY_SAMPLE_STRIDE
}
