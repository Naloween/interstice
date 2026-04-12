use super::types::{BenchmarkReport, VerifyMode};
use interstice_core::IntersticeError;
use std::path::Path;

pub(crate) fn render_report(report: &BenchmarkReport) {
    println!("Benchmark Report: {}", report.name);
    println!("  node: {}", report.node);
    println!("  module: {}", report.module_name);
    println!("  reducer: {}", report.reducer_name);
    println!("  transport: {:?}", report.transport);
    println!("  duration_ms: {}", report.duration_ms);
    println!("  warmup_ms: {}", report.warmup_ms);
    println!("  connections: {}", report.connections);
    if let Some(rate) = report.rate {
        println!("  configured_rate_tps: {}", rate);
    }
    println!("  sent_warmup: {}", report.sent_warmup);
    println!("  sent_measured: {}", report.sent_measured);
    println!("  failed: {}", report.failed);
    println!(
        "  throughput_tps ({}): {:.2}",
        report.throughput_kind, report.throughput_tps
    );
    if let Some(start) = report.throughput_counter_start {
        println!("  throughput_counter_start: {}", start);
    }
    if let Some(end) = report.throughput_counter_end {
        println!("  throughput_counter_end: {}", end);
    }
    if let Some(window_ms) = report.throughput_window_ms {
        println!("  throughput_window_ms: {}", window_ms);
    }
    let latency_label = if report.throughput_kind == "dispatch_rate" {
        "dispatch_latency_us"
    } else {
        "write_latency_us"
    };
    println!(
        "  {}_mean: {:.2}",
        latency_label, report.write_latency_us_mean
    );
    println!("  {}_p50: {:.2}", latency_label, report.write_latency_us_p50);
    println!("  {}_p95: {:.2}", latency_label, report.write_latency_us_p95);
    println!("  {}_p99: {:.2}", latency_label, report.write_latency_us_p99);
    println!("  {}_max: {:.2}", latency_label, report.write_latency_us_max);

    if let Some(verification) = &report.verification {
        println!("  verification_mode: {:?}", verification.mode);
        match verification.mode {
            VerifyMode::None => {}
            VerifyMode::Query => {
                if let Some(query_name) = &verification.query_name {
                    println!("  verification_query: {}", query_name);
                }
                if let Some(matched) = verification.matched {
                    println!("  verification_matched: {}", matched);
                }
                if let Some(error) = &verification.error {
                    println!("  verification_error: {}", error);
                }
            }
        }
    }
}

pub(crate) fn write_report(path: &Path, report: &BenchmarkReport) -> Result<(), IntersticeError> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent).map_err(|err| {
            IntersticeError::Internal(format!(
                "Failed to create benchmark output directory {:?}: {}",
                parent, err
            ))
        })?;
    }

    let contents = serde_json::to_string_pretty(report)
        .map_err(|err| IntersticeError::Internal(format!("Failed to serialize report: {}", err)))?;

    std::fs::write(path, contents).map_err(|err| {
        IntersticeError::Internal(format!(
            "Failed to write benchmark report {:?}: {}",
            path, err
        ))
    })
}

pub(crate) fn print_benchmark_help() {
    println!("Benchmark commands:");
    println!("  benchmark run [options]");
    println!("    Required:");
    println!("      --node <name|id>");
    println!("      --module <module_name>");
    println!("      --reducer <reducer_name>");
    println!("    Optional:");
    println!("      --name <run_name>");
    println!("      --duration-ms <ms> (default 30000)");
    println!("      --warmup-ms <ms> (default 5000)");
    println!("      --connections <n> (default 1)");
    println!("      --transport <persistent|reconnect> (default persistent)");
    println!("      --rate <tps_total>");
    println!("      --latency-sample-stride <n> (default 64)");
    println!("      --throughput-mode <dispatch_success|query_delta> (default dispatch_success)");
    println!("        query_delta: measures real committed tx/s via server-side counter");
    println!("          (the throughput query must declare #[query(reads = [...])] for tables it reads)");
    println!("      --throughput-query <query_name>");
    println!("      --throughput-query-field <field_name>");
    println!("      --throughput-query-arg-json '<json>' (repeatable)");
    println!("      --throughput-query-args-json '<json_array_or_value>'");
    println!("      --throughput-query-args-interstice-json '<IntersticeValue json>'");
    println!("      --arg-json '<json>' (repeatable)");
    println!("      --args-json '<json_array_or_value>'");
    println!("      --args-interstice-json '<IntersticeValue json>'");
    println!("      --reset-before");
    println!("      --verify-mode <none|query>");
    println!("      --verify-query <query_name>");
    println!("      --verify-arg-json '<json>' (repeatable)");
    println!("      --verify-args-json '<json_array_or_value>'");
    println!("      --verify-args-interstice-json '<IntersticeValue json>'");
    println!("      --verify-expect-json '<json>'");
    println!("      --verify-expect-interstice-json '<IntersticeValue json>'");
    println!("      --output <path/to/report.json>");
    println!("      --output-prefix <path/prefix>");
    println!("      --scenario-file <path.toml> (repeatable)");
    println!("      --profile <vm-noop|insert-ephemeral|durability|fanout>");
    println!();
    println!("  benchmark scenario <path.toml>");
    println!("    Runs all scenarios in a TOML file.");
    println!();
    println!("  benchmark profile <name>");
    println!("    Runs one of the built-in benchmark profiles.");
    println!();
    println!("  benchmark list-profiles");
}

pub(crate) fn print_profiles() {
    println!("Built-in benchmark profiles (node registry name `benchmark-example`, see `interstice example benchmark`):");
    println!("  vm-noop           — pure WASM call overhead (noop reducer, dispatch_success)");
    println!("  insert-ephemeral  — ephemeral inserts + progress tracking; query_delta toward ~100k commits");
    println!("  durability        — logged/persisted insert throughput (query_delta)");
    println!("  fanout            — subscription fanout throughput");
}
