use super::network::{
    call_query_value, default_worker_token, handshake_with_worker_identity, invoke_reducer_once,
};
use super::report::{render_report, write_report};
use super::template::{
    query_input_from_config, reducer_input_from_config, throughput_query_input_from_config,
};
use super::types::{
    BenchmarkReport, BenchmarkRunConfig, BenchmarkTransport, TemplateContext, ThroughputMode,
    VerificationReport, VerifyMode, WorkerMetrics, WorkerSummary,
};
use super::util::{merge_metrics, now_epoch_ms, percentile_us};
use crate::node_registry::NodeRegistry;
use interstice_core::{
    IntersticeError, NetworkPacket, interstice_abi::IntersticeValue, packet::write_packet,
};
use std::path::Path;
use std::time::{Duration, Instant};
use tokio::net::TcpStream;
use tokio::time::{Instant as TokioInstant, sleep, sleep_until};
use uuid::Uuid;

pub(crate) async fn run_scenarios(
    configs: Vec<BenchmarkRunConfig>,
) -> Result<Vec<BenchmarkReport>, IntersticeError> {
    let mut reports = Vec::with_capacity(configs.len());
    for config in configs {
        reports.push(run_single(config).await?);
    }
    Ok(reports)
}

pub(crate) async fn run_and_render(
    configs: Vec<BenchmarkRunConfig>,
    output_prefix: Option<&Path>,
) -> Result<Vec<BenchmarkReport>, IntersticeError> {
    let reports = run_scenarios(configs).await?;

    for report in &reports {
        render_report(report);

        if let Some(path) = report_output_path(report, output_prefix) {
            write_report(&path, report)?;
            println!("  report_output: {}", path.display());
        }
    }

    Ok(reports)
}

fn report_output_path(
    report: &BenchmarkReport,
    output_prefix: Option<&Path>,
) -> Option<std::path::PathBuf> {
    if let Some(prefix) = output_prefix {
        let filename = format!("{}_{}.json", report.started_at_ms, report.name);
        return Some(prefix.join(filename));
    }
    None
}

async fn run_single(mut config: BenchmarkRunConfig) -> Result<BenchmarkReport, IntersticeError> {
    config.normalize()?;

    let registry = NodeRegistry::load()?;
    let address = registry
        .resolve_address(&config.node)
        .ok_or_else(|| IntersticeError::Internal(format!("Unknown node '{}'", config.node)))?;

    if config.reset_before {
        let reset_input = IntersticeValue::Vec(vec![]);
        if let Err(err) = invoke_reducer_once(
            &address,
            &config.module_name,
            "reset_benchmark_state",
            reset_input,
        )
        .await
        {
            eprintln!(
                "Warning: failed to reset benchmark state via {}::reset_benchmark_state: {}",
                config.module_name, err
            );
        }
    }

    let started_at_ms = now_epoch_ms();
    let start = Instant::now();
    let warmup_duration = Duration::from_millis(config.warmup_ms);
    let measure_duration = Duration::from_millis(config.duration_ms);

    let throughput_snapshot_start = if config.throughput_mode == ThroughputMode::QueryDelta {
        Some(
            query_throughput_counter_with_retry(
                &address,
                &config,
                &empty_template_context(),
                "start",
            )
            .await
            .map_err(|err| {
                IntersticeError::Internal(format!(
                    "Failed to read throughput start counter in query_delta mode: {}",
                    err
                ))
            })?,
        )
    } else {
        None
    };

    let mut handles = Vec::with_capacity(config.connections);
    for worker_id in 0..config.connections {
        let worker_config = config.clone();
        let worker_address = address.clone();
        handles.push(tokio::spawn(async move {
            run_worker(
                worker_id,
                &worker_address,
                &worker_config,
                start,
                warmup_duration,
                measure_duration,
            )
            .await
        }));
    }

    let mut aggregate = WorkerMetrics::default();

    for handle in handles {
        let metrics = handle.await.map_err(|err| {
            IntersticeError::Internal(format!("Benchmark worker task failed to join: {}", err))
        })??;
        merge_metrics(&mut aggregate, metrics);
    }

    let mut samples = aggregate.latency_samples_ns;
    samples.sort_unstable();

    let sample_count = samples.len() as f64;
    let mean_latency_us = if sample_count > 0.0 {
        (aggregate.write_latency_sum_ns as f64 / sample_count) / 1_000.0
    } else {
        0.0
    };

    let mut summaries = aggregate.summaries;
    summaries.sort_by_key(|summary| summary.worker_id);

    let summary_context = summary_template_context(&summaries);
    let (throughput_kind, throughput_tps, throughput_counter_end, throughput_window_ms) =
        match config.throughput_mode {
            ThroughputMode::DispatchSuccess => (
                "dispatch_success".to_string(),
                aggregate.measured_sent as f64 / (config.duration_ms as f64 / 1_000.0),
                None,
                None,
            ),
            ThroughputMode::QueryDelta => {
                let (start_counter, start_snapshot_time) =
                    throughput_snapshot_start.ok_or_else(|| {
                        IntersticeError::Internal(
                            "query_delta throughput mode is missing a start counter snapshot"
                                .into(),
                        )
                    })?;
                let (end_counter, end_snapshot_time) =
                    query_throughput_counter_with_retry(&address, &config, &summary_context, "end")
                        .await
                        .map_err(|err| {
                            IntersticeError::Internal(format!(
                                "Failed to read throughput end counter in query_delta mode: {}",
                                err
                            ))
                        })?;
                let delta = end_counter.saturating_sub(start_counter);
                let window_ms = end_snapshot_time
                    .saturating_duration_since(start_snapshot_time)
                    .as_millis() as u64;
                let elapsed_secs = (window_ms as f64 / 1_000.0).max(f64::EPSILON);
                (
                    "query_delta".to_string(),
                    delta as f64 / elapsed_secs,
                    Some(end_counter),
                    Some(window_ms),
                )
            }
        };

    let verification = run_verification(&address, &config, &summaries).await;

    let ended_at_ms = now_epoch_ms();

    Ok(BenchmarkReport {
        name: config.display_name(),
        node: config.node,
        module_name: config.module_name,
        reducer_name: config.reducer_name,
        transport: config.transport,
        warmup_ms: config.warmup_ms,
        duration_ms: config.duration_ms,
        connections: config.connections,
        rate: config.rate,
        output: config.output.clone(),
        sent_warmup: aggregate.warmup_sent,
        sent_measured: aggregate.measured_sent,
        failed: aggregate.failed,
        throughput_kind,
        throughput_tps,
        throughput_counter_start: throughput_snapshot_start.map(|(value, _)| value),
        throughput_counter_end,
        throughput_window_ms,
        write_latency_us_mean: mean_latency_us,
        write_latency_us_p50: percentile_us(&samples, 0.50),
        write_latency_us_p95: percentile_us(&samples, 0.95),
        write_latency_us_p99: percentile_us(&samples, 0.99),
        write_latency_us_max: aggregate.write_latency_max_ns as f64 / 1_000.0,
        started_at_ms,
        ended_at_ms,
        worker_summaries: summaries,
        verification,
    })
}

async fn run_worker(
    worker_id: usize,
    address: &str,
    config: &BenchmarkRunConfig,
    start: Instant,
    warmup_duration: Duration,
    measure_duration: Duration,
) -> Result<WorkerMetrics, IntersticeError> {
    let warmup_end = start + warmup_duration;
    let measure_end = warmup_end + measure_duration;

    let mut op: u64 = 0;
    let mut metrics = WorkerMetrics::default();
    let mut stream: Option<TcpStream> = None;

    let worker_peer_id = Uuid::new_v4();
    let worker_token = default_worker_token();
    let client_id = format!("bench-worker-{}-{}", worker_id, worker_peer_id);

    let target_interval = config
        .rate_per_worker()
        .map(|rate| Duration::from_secs_f64(1.0 / rate));
    let mut next_deadline = Instant::now();

    loop {
        let now = Instant::now();
        if now >= measure_end {
            break;
        }

        if let Some(interval) = target_interval {
            next_deadline += interval;
            let sleep_target = TokioInstant::from_std(next_deadline);
            if TokioInstant::now() < sleep_target {
                sleep_until(sleep_target).await;
            }
        }

        let in_measured_window = now >= warmup_end;
        op = op.saturating_add(1);
        let seq = metrics.warmup_sent + metrics.measured_sent + 1;

        let template_context = TemplateContext {
            seq,
            worker: worker_id,
            op,
            client: client_id.clone(),
            now_ms: now_epoch_ms(),
            max_seq: 0,
            max_client: String::new(),
            total_sent: seq,
        };

        let input = match reducer_input_from_config(config, &template_context) {
            Ok(input) => input,
            Err(_) => {
                metrics.failed = metrics.failed.saturating_add(1);
                continue;
            }
        };

        let write_start = Instant::now();

        let write_result = match config.transport {
            BenchmarkTransport::Persistent => {
                if stream.is_none() {
                    stream = Some(
                        handshake_with_worker_identity(address, &worker_peer_id, &worker_token)
                            .await?,
                    );
                }

                if let Some(active_stream) = stream.as_mut() {
                    let packet = NetworkPacket::ReducerCall {
                        module_name: config.module_name.clone(),
                        reducer_name: config.reducer_name.clone(),
                        input,
                    };
                    write_packet(active_stream, &packet).await
                } else {
                    continue;
                }
            }
            BenchmarkTransport::Reconnect => {
                let mut transient =
                    handshake_with_worker_identity(address, &worker_peer_id, &worker_token).await?;
                let packet = NetworkPacket::ReducerCall {
                    module_name: config.module_name.clone(),
                    reducer_name: config.reducer_name.clone(),
                    input,
                };
                let result = write_packet(&mut transient, &packet).await;
                let _ = write_packet(&mut transient, &NetworkPacket::Close).await;
                result
            }
        };

        let write_ns = write_start.elapsed().as_nanos() as u64;
        if write_result.is_ok() {
            if in_measured_window {
                metrics.measured_sent = metrics.measured_sent.saturating_add(1);
                if metrics.measured_sent % config.latency_sample_stride == 0 {
                    metrics.write_latency_sum_ns = metrics
                        .write_latency_sum_ns
                        .saturating_add(write_ns as u128);
                    metrics.write_latency_max_ns = metrics.write_latency_max_ns.max(write_ns);
                    metrics.latency_samples_ns.push(write_ns);
                }
            } else {
                metrics.warmup_sent = metrics.warmup_sent.saturating_add(1);
            }
        } else {
            metrics.failed = metrics.failed.saturating_add(1);
            if config.transport == BenchmarkTransport::Persistent {
                stream = None;
            }
        }
    }

    if let Some(mut active_stream) = stream {
        let _ = write_packet(&mut active_stream, &NetworkPacket::Close).await;
    }

    let attempted = metrics.warmup_sent + metrics.measured_sent + metrics.failed;
    let last_seq = metrics.warmup_sent + metrics.measured_sent;
    metrics.summaries.push(WorkerSummary {
        worker_id,
        client_id,
        attempted,
        measured_sent: metrics.measured_sent,
        last_seq,
    });

    Ok(metrics)
}

fn throughput_query_retry_timeout(config: &BenchmarkRunConfig) -> Duration {
    Duration::from_millis(config.duration_ms.saturating_mul(4).max(5_000).min(120_000))
}

async fn query_throughput_counter_with_retry(
    address: &str,
    config: &BenchmarkRunConfig,
    context: &TemplateContext,
    phase: &str,
) -> Result<(u64, Instant), IntersticeError> {
    let timeout = throughput_query_retry_timeout(config);
    let started = Instant::now();
    let mut attempts: u64 = 0;

    loop {
        attempts = attempts.saturating_add(1);
        match query_throughput_counter(address, config, context).await {
            Ok(counter) => return Ok((counter, Instant::now())),
            Err(err) => {
                let reason = err.to_string();
                if started.elapsed() >= timeout {
                    return Err(IntersticeError::Internal(format!(
                        "timed out reading throughput {} counter after {} attempts over {} ms: {}",
                        phase,
                        attempts,
                        started.elapsed().as_millis(),
                        reason
                    )));
                }
                sleep(Duration::from_millis(50)).await;
            }
        }
    }
}

async fn run_verification(
    address: &str,
    config: &BenchmarkRunConfig,
    summaries: &[WorkerSummary],
) -> Option<VerificationReport> {
    match config.verify.mode {
        VerifyMode::None => None,
        VerifyMode::Query => {
            let query_name = config.verify.query_name.clone();
            let Some(query_name_value) = query_name.clone() else {
                return Some(VerificationReport {
                    mode: VerifyMode::Query,
                    query_name: None,
                    result: None,
                    expected: None,
                    matched: None,
                    error: Some("verify.query_name is required when verify.mode=query".into()),
                });
            };

            let context = summary_template_context(summaries);

            let input = match query_input_from_config(config, &context) {
                Ok(value) => value,
                Err(err) => {
                    return Some(VerificationReport {
                        mode: VerifyMode::Query,
                        query_name,
                        result: None,
                        expected: None,
                        matched: None,
                        error: Some(format!(
                            "failed to prepare verification query input: {}",
                            err
                        )),
                    });
                }
            };

            match call_query_value(address, &config.module_name, &query_name_value, input).await {
                Ok(result) => {
                    let expected = config.verify.expect_interstice_json.clone().or_else(|| {
                        config
                            .verify
                            .expect_json
                            .clone()
                            .map(|value| IntersticeValue::String(value.to_string()))
                    });

                    let matched = expected
                        .as_ref()
                        .map(|expected_value| expected_value == &result);

                    Some(VerificationReport {
                        mode: VerifyMode::Query,
                        query_name,
                        result: Some(result),
                        expected,
                        matched,
                        error: None,
                    })
                }
                Err(err) => Some(VerificationReport {
                    mode: VerifyMode::Query,
                    query_name,
                    result: None,
                    expected: None,
                    matched: None,
                    error: Some(err.to_string()),
                }),
            }
        }
    }
}

async fn query_throughput_counter(
    address: &str,
    config: &BenchmarkRunConfig,
    context: &TemplateContext,
) -> Result<u64, IntersticeError> {
    let query_name = config
        .throughput_query_name
        .clone()
        .or_else(|| config.verify.query_name.clone())
        .ok_or_else(|| {
            IntersticeError::Internal(
                "throughput query name is required when throughput_mode=query_delta (set --throughput-query or --verify-query)"
                    .into(),
            )
        })?;

    let use_verify_query_args = config.throughput_query_args_interstice_json.is_none()
        && matches!(
            &config.throughput_query_args_json,
            serde_json::Value::Array(values) if values.is_empty()
        );

    let input = if use_verify_query_args {
        query_input_from_config(config, context)?
    } else {
        throughput_query_input_from_config(config, context)?
    };

    let result = call_query_value(address, &config.module_name, &query_name, input).await?;
    extract_counter_from_value(&result, config.throughput_query_field.as_deref()).ok_or_else(|| {
        IntersticeError::Internal(format!(
            "throughput query '{}' did not return a numeric counter value{} (got: {})",
            query_name,
            config
                .throughput_query_field
                .as_ref()
                .map(|field| format!(" in field '{}'", field))
                .unwrap_or_default(),
            result
        ))
    })
}

fn extract_counter_from_value(value: &IntersticeValue, field: Option<&str>) -> Option<u64> {
    if let Some(field_name) = field {
        return extract_named_field(value, field_name).and_then(interstice_number_to_u64);
    }

    interstice_number_to_u64(value)
        .or_else(|| extract_named_field(value, "committed").and_then(interstice_number_to_u64))
        .or_else(|| extract_named_field(value, "count").and_then(interstice_number_to_u64))
        .or_else(|| extract_named_field(value, "total").and_then(interstice_number_to_u64))
}

fn extract_named_field<'a>(
    value: &'a IntersticeValue,
    field_name: &str,
) -> Option<&'a IntersticeValue> {
    match value {
        IntersticeValue::Struct { fields, .. } => fields
            .iter()
            .find(|field| field.name == field_name)
            .map(|field| &field.value),
        _ => None,
    }
}

fn interstice_number_to_u64(value: &IntersticeValue) -> Option<u64> {
    match value {
        IntersticeValue::U8(value) => Some(*value as u64),
        IntersticeValue::U32(value) => Some(*value as u64),
        IntersticeValue::U64(value) => Some(*value),
        IntersticeValue::I32(value) if *value >= 0 => Some(*value as u64),
        IntersticeValue::I64(value) if *value >= 0 => Some(*value as u64),
        IntersticeValue::F32(value) if *value >= 0.0 => Some(*value as u64),
        IntersticeValue::F64(value) if *value >= 0.0 => Some(*value as u64),
        IntersticeValue::Option(Some(inner)) => interstice_number_to_u64(inner),
        IntersticeValue::Vec(values) => values.first().and_then(interstice_number_to_u64),
        IntersticeValue::Tuple(values) => values.first().and_then(interstice_number_to_u64),
        IntersticeValue::Enum { value, .. } => interstice_number_to_u64(value),
        _ => None,
    }
}

fn empty_template_context() -> TemplateContext {
    TemplateContext {
        seq: 0,
        worker: 0,
        op: 0,
        client: String::new(),
        now_ms: now_epoch_ms(),
        max_seq: 0,
        max_client: String::new(),
        total_sent: 0,
    }
}

fn summary_template_context(summaries: &[WorkerSummary]) -> TemplateContext {
    let total_sent: u64 = summaries.iter().map(|summary| summary.measured_sent).sum();
    let max_seq = summaries
        .iter()
        .map(|summary| summary.last_seq)
        .max()
        .unwrap_or(0);
    let max_client = summaries
        .iter()
        .max_by_key(|summary| summary.last_seq)
        .map(|summary| summary.client_id.clone())
        .unwrap_or_default();

    TemplateContext {
        seq: max_seq,
        worker: 0,
        op: 0,
        client: max_client.clone(),
        now_ms: now_epoch_ms(),
        max_seq,
        max_client,
        total_sent,
    }
}
