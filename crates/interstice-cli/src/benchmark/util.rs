use super::types::{BenchmarkTransport, ThroughputMode, VerifyMode, WorkerMetrics};
use interstice_core::IntersticeError;
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) fn merge_metrics(target: &mut WorkerMetrics, source: WorkerMetrics) {
    target.warmup_sent = target.warmup_sent.saturating_add(source.warmup_sent);
    target.measured_sent = target.measured_sent.saturating_add(source.measured_sent);
    target.failed = target.failed.saturating_add(source.failed);
    target.write_latency_sum_ns = target
        .write_latency_sum_ns
        .saturating_add(source.write_latency_sum_ns);
    target.write_latency_max_ns = target.write_latency_max_ns.max(source.write_latency_max_ns);
    target.latency_samples_ns.extend(source.latency_samples_ns);
    target.summaries.extend(source.summaries);
}

pub(crate) fn percentile_us(samples_ns: &[u64], percentile: f64) -> f64 {
    if samples_ns.is_empty() {
        return 0.0;
    }

    let clamped = percentile.clamp(0.0, 1.0);
    let index = ((samples_ns.len() as f64 - 1.0) * clamped).round() as usize;
    samples_ns[index] as f64 / 1_000.0
}

pub(crate) fn parse_transport(value: &str) -> Result<BenchmarkTransport, IntersticeError> {
    match value {
        "persistent" => Ok(BenchmarkTransport::Persistent),
        "reconnect" => Ok(BenchmarkTransport::Reconnect),
        _ => Err(IntersticeError::Internal(format!(
            "Invalid transport '{}', expected persistent or reconnect",
            value
        ))),
    }
}

pub(crate) fn parse_verify_mode(value: &str) -> Result<VerifyMode, IntersticeError> {
    match value {
        "none" => Ok(VerifyMode::None),
        "query" => Ok(VerifyMode::Query),
        _ => Err(IntersticeError::Internal(format!(
            "Invalid verify mode '{}', expected none or query",
            value
        ))),
    }
}

pub(crate) fn parse_throughput_mode(value: &str) -> Result<ThroughputMode, IntersticeError> {
    match value {
        "dispatch_success" | "dispatch" => Ok(ThroughputMode::DispatchSuccess),
        "query_delta" | "commit" | "committed" => Ok(ThroughputMode::QueryDelta),
        _ => Err(IntersticeError::Internal(format!(
            "Invalid throughput mode '{}', expected dispatch_success or query_delta",
            value
        ))),
    }
}

pub(crate) fn parse_u64(value: &str, name: &str) -> Result<u64, IntersticeError> {
    value.parse::<u64>().map_err(|err| {
        IntersticeError::Internal(format!("Failed to parse {} as u64: {}", name, err))
    })
}

pub(crate) fn parse_usize(value: &str, name: &str) -> Result<usize, IntersticeError> {
    value.parse::<usize>().map_err(|err| {
        IntersticeError::Internal(format!("Failed to parse {} as usize: {}", name, err))
    })
}

pub(crate) fn arg_value(
    args: &[String],
    index: usize,
    option: &str,
) -> Result<String, IntersticeError> {
    args.get(index).cloned().ok_or_else(|| {
        IntersticeError::Internal(format!("Missing value for benchmark option {}", option))
    })
}

pub(crate) fn now_epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

pub(crate) fn slugify(name: &str) -> String {
    name.chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect()
}
