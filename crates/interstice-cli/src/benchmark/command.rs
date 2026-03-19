use super::report::{print_benchmark_help, print_profiles};
use super::runner::{run_and_render, run_scenarios};
use super::template::{parse_interstice_json, parse_json_value};
use super::types::{
    BenchmarkRunConfig, BenchmarkTransport, ScenarioCollection, ThroughputMode, VerifyConfig,
    default_args_json,
};
use super::util::{
    arg_value, parse_throughput_mode, parse_transport, parse_u64, parse_usize, parse_verify_mode,
    slugify,
};
use interstice_core::IntersticeError;
use std::path::{Path, PathBuf};

pub async fn handle_benchmark_command(args: Vec<String>) -> Result<(), IntersticeError> {
    if args.is_empty() {
        print_benchmark_help();
        return Ok(());
    }

    match args[0].as_str() {
        "run" => run_command(args[1..].to_vec()).await,
        "scenario" => scenario_command(args[1..].to_vec()).await,
        "profile" => profile_command(args[1..].to_vec()).await,
        "list-profiles" => {
            print_profiles();
            Ok(())
        }
        _ => {
            print_benchmark_help();
            Ok(())
        }
    }
}

async fn run_command(args: Vec<String>) -> Result<(), IntersticeError> {
    let mut config = BenchmarkRunConfig {
        node: String::new(),
        module_name: String::new(),
        reducer_name: String::new(),
        name: None,
        duration_ms: super::types::DEFAULT_DURATION_MS,
        warmup_ms: super::types::DEFAULT_WARMUP_MS,
        connections: super::types::DEFAULT_CONNECTIONS,
        transport: BenchmarkTransport::Persistent,
        rate: None,
        args_json: default_args_json(),
        args_interstice_json: None,
        verify: VerifyConfig::default(),
        latency_sample_stride: super::types::DEFAULT_LATENCY_SAMPLE_STRIDE,
        throughput_mode: ThroughputMode::DispatchSuccess,
        throughput_query_name: None,
        throughput_query_args_json: default_args_json(),
        throughput_query_args_interstice_json: None,
        throughput_query_field: None,
        output: None,
        reset_before: false,
    };

    let mut reducer_args: Vec<serde_json::Value> = Vec::new();
    let mut verify_args: Vec<serde_json::Value> = Vec::new();
    let mut throughput_query_args: Vec<serde_json::Value> = Vec::new();
    let mut output_prefix: Option<PathBuf> = None;
    let mut scenario_files: Vec<PathBuf> = Vec::new();
    let mut profile_name: Option<String> = None;

    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--node" => {
                config.node = arg_value(&args, index + 1, "--node")?;
                index += 2;
            }
            "--module" => {
                config.module_name = arg_value(&args, index + 1, "--module")?;
                index += 2;
            }
            "--reducer" => {
                config.reducer_name = arg_value(&args, index + 1, "--reducer")?;
                index += 2;
            }
            "--name" => {
                config.name = Some(arg_value(&args, index + 1, "--name")?);
                index += 2;
            }
            "--duration-ms" => {
                config.duration_ms = parse_u64(
                    &arg_value(&args, index + 1, "--duration-ms")?,
                    "duration-ms",
                )?;
                index += 2;
            }
            "--warmup-ms" => {
                config.warmup_ms =
                    parse_u64(&arg_value(&args, index + 1, "--warmup-ms")?, "warmup-ms")?;
                index += 2;
            }
            "--connections" => {
                config.connections = parse_usize(
                    &arg_value(&args, index + 1, "--connections")?,
                    "connections",
                )?;
                index += 2;
            }
            "--transport" => {
                config.transport = parse_transport(&arg_value(&args, index + 1, "--transport")?)?;
                index += 2;
            }
            "--rate" => {
                config.rate = Some(parse_u64(&arg_value(&args, index + 1, "--rate")?, "rate")?);
                index += 2;
            }
            "--latency-sample-stride" => {
                config.latency_sample_stride = parse_u64(
                    &arg_value(&args, index + 1, "--latency-sample-stride")?,
                    "latency-sample-stride",
                )?;
                index += 2;
            }
            "--throughput-mode" => {
                config.throughput_mode =
                    parse_throughput_mode(&arg_value(&args, index + 1, "--throughput-mode")?)?;
                index += 2;
            }
            "--throughput-query" => {
                config.throughput_query_name =
                    Some(arg_value(&args, index + 1, "--throughput-query")?);
                index += 2;
            }
            "--throughput-query-field" => {
                config.throughput_query_field =
                    Some(arg_value(&args, index + 1, "--throughput-query-field")?);
                index += 2;
            }
            "--throughput-query-arg-json" => {
                throughput_query_args.push(parse_json_value(&arg_value(
                    &args,
                    index + 1,
                    "--throughput-query-arg-json",
                )?)?);
                index += 2;
            }
            "--throughput-query-args-json" => {
                config.throughput_query_args_json = parse_json_value(&arg_value(
                    &args,
                    index + 1,
                    "--throughput-query-args-json",
                )?)?;
                index += 2;
            }
            "--throughput-query-args-interstice-json" => {
                config.throughput_query_args_interstice_json = Some(parse_interstice_json(
                    &arg_value(&args, index + 1, "--throughput-query-args-interstice-json")?,
                )?);
                index += 2;
            }
            "--arg-json" => {
                reducer_args.push(parse_json_value(&arg_value(
                    &args,
                    index + 1,
                    "--arg-json",
                )?)?);
                index += 2;
            }
            "--args-json" => {
                let value = parse_json_value(&arg_value(&args, index + 1, "--args-json")?)?;
                config.args_json = value;
                index += 2;
            }
            "--args-interstice-json" => {
                config.args_interstice_json = Some(parse_interstice_json(&arg_value(
                    &args,
                    index + 1,
                    "--args-interstice-json",
                )?)?);
                index += 2;
            }
            "--verify-mode" => {
                config.verify.mode =
                    parse_verify_mode(&arg_value(&args, index + 1, "--verify-mode")?)?;
                index += 2;
            }
            "--verify-query" => {
                config.verify.query_name = Some(arg_value(&args, index + 1, "--verify-query")?);
                index += 2;
            }
            "--verify-arg-json" => {
                verify_args.push(parse_json_value(&arg_value(
                    &args,
                    index + 1,
                    "--verify-arg-json",
                )?)?);
                index += 2;
            }
            "--verify-args-json" => {
                config.verify.args_json =
                    parse_json_value(&arg_value(&args, index + 1, "--verify-args-json")?)?;
                index += 2;
            }
            "--verify-args-interstice-json" => {
                config.verify.args_interstice_json = Some(parse_interstice_json(&arg_value(
                    &args,
                    index + 1,
                    "--verify-args-interstice-json",
                )?)?);
                index += 2;
            }
            "--verify-expect-json" => {
                config.verify.expect_json = Some(parse_json_value(&arg_value(
                    &args,
                    index + 1,
                    "--verify-expect-json",
                )?)?);
                index += 2;
            }
            "--verify-expect-interstice-json" => {
                config.verify.expect_interstice_json = Some(parse_interstice_json(&arg_value(
                    &args,
                    index + 1,
                    "--verify-expect-interstice-json",
                )?)?);
                index += 2;
            }
            "--output" => {
                config.output = Some(arg_value(&args, index + 1, "--output")?);
                index += 2;
            }
            "--output-prefix" => {
                output_prefix = Some(PathBuf::from(arg_value(
                    &args,
                    index + 1,
                    "--output-prefix",
                )?));
                index += 2;
            }
            "--scenario-file" => {
                scenario_files.push(PathBuf::from(arg_value(
                    &args,
                    index + 1,
                    "--scenario-file",
                )?));
                index += 2;
            }
            "--profile" => {
                profile_name = Some(arg_value(&args, index + 1, "--profile")?);
                index += 2;
            }
            "--reset-before" => {
                config.reset_before = true;
                index += 1;
            }
            unknown => {
                return Err(IntersticeError::Internal(format!(
                    "Unknown benchmark option: {}",
                    unknown
                )));
            }
        }
    }

    if !reducer_args.is_empty() {
        config.args_json = serde_json::Value::Array(reducer_args);
    }
    if !verify_args.is_empty() {
        config.verify.args_json = serde_json::Value::Array(verify_args);
    }
    if !throughput_query_args.is_empty() {
        config.throughput_query_args_json = serde_json::Value::Array(throughput_query_args);
    }

    let mut scenarios = Vec::new();

    for file in scenario_files {
        let mut loaded = load_scenario_file(&file)?;
        scenarios.append(&mut loaded);
    }

    if let Some(profile) = profile_name {
        let mut built = built_in_profile(&profile)?;
        scenarios.append(&mut built);
    }

    let config_complete = !config.node.is_empty()
        && !config.module_name.is_empty()
        && !config.reducer_name.is_empty();

    if config_complete {
        config.normalize()?;
        if config.name.is_none() {
            config.name = Some(config.display_name());
        }
        scenarios.insert(0, config);
    } else if scenarios.is_empty() {
        return Err(IntersticeError::Internal(
            "benchmark run requires --node, --module, --reducer unless --scenario-file or --profile is provided"
                .into(),
        ));
    }

    if let Some(prefix) = output_prefix.as_deref() {
        run_and_render(scenarios, Some(prefix)).await?;
    } else {
        let reports = run_scenarios(scenarios).await?;
        for report in reports {
            super::report::render_report(&report);
            if let Some(path) = report.output.as_deref() {
                super::report::write_report(Path::new(path), &report)?;
                println!("  report_output: {}", path);
            }
        }
    }

    Ok(())
}

async fn scenario_command(args: Vec<String>) -> Result<(), IntersticeError> {
    let path = args.first().ok_or_else(|| {
        IntersticeError::Internal("benchmark scenario requires a path to a TOML file".into())
    })?;

    let scenarios = load_scenario_file(Path::new(path))?;
    let reports = run_scenarios(scenarios).await?;
    for report in reports {
        super::report::render_report(&report);
        if let Some(path) = report.output.as_deref() {
            super::report::write_report(Path::new(path), &report)?;
            println!("  report_output: {}", path);
        }
    }

    Ok(())
}

async fn profile_command(args: Vec<String>) -> Result<(), IntersticeError> {
    let profile = args.first().ok_or_else(|| {
        IntersticeError::Internal("benchmark profile requires a profile name".into())
    })?;

    let scenarios = built_in_profile(profile)?;
    let reports = run_scenarios(scenarios).await?;
    for report in reports {
        super::report::render_report(&report);
        if let Some(path) = report.output.as_deref() {
            super::report::write_report(Path::new(path), &report)?;
            println!("  report_output: {}", path);
        }
    }

    Ok(())
}

fn load_scenario_file(path: &Path) -> Result<Vec<BenchmarkRunConfig>, IntersticeError> {
    let contents = std::fs::read_to_string(path).map_err(|err| {
        IntersticeError::Internal(format!(
            "Failed to read benchmark scenario {:?}: {}",
            path, err
        ))
    })?;

    let collection: ScenarioCollection = toml::from_str(&contents).map_err(|err| {
        IntersticeError::Internal(format!(
            "Failed to parse benchmark scenario TOML {:?}: {}",
            path, err
        ))
    })?;

    let mut scenarios = Vec::new();
    scenarios.extend(collection.scenarios);
    scenarios.extend(collection.scenario);

    if scenarios.is_empty() {
        return Err(IntersticeError::Internal(format!(
            "Benchmark scenario {:?} does not define [[scenarios]] or [[scenario]] entries",
            path
        )));
    }

    for scenario in &mut scenarios {
        scenario.normalize()?;
        if scenario.name.is_none() {
            scenario.name = Some(scenario.display_name());
        }
    }

    Ok(scenarios)
}

fn built_in_profile(name: &str) -> Result<Vec<BenchmarkRunConfig>, IntersticeError> {
    let scenarios = match name {
        "vm-noop" => vec![BenchmarkRunConfig {
            node: "bench-node".to_string(),
            module_name: "benchmark-workload".to_string(),
            reducer_name: "noop".to_string(),
            name: Some("vm-noop".to_string()),
            duration_ms: 10_000,
            warmup_ms: 2_000,
            connections: 4,
            transport: BenchmarkTransport::Persistent,
            rate: None,
            args_json: serde_json::Value::Array(vec![]),
            args_interstice_json: None,
            verify: VerifyConfig::default(),
            latency_sample_stride: super::types::DEFAULT_LATENCY_SAMPLE_STRIDE,
            throughput_mode: ThroughputMode::DispatchSuccess,
            throughput_query_name: None,
            throughput_query_args_json: default_args_json(),
            throughput_query_args_interstice_json: None,
            throughput_query_field: None,
            output: Some(format!(
                "benchmarks/results/profile_vm_noop_{}.json",
                slugify(name)
            )),
            reset_before: true,
        }],
        "insert-ephemeral" => vec![BenchmarkRunConfig {
            node: "bench-node".to_string(),
            module_name: "benchmark-workload".to_string(),
            reducer_name: "tx_insert_ephemeral".to_string(),
            name: Some("insert-ephemeral".to_string()),
            duration_ms: 15_000,
            warmup_ms: 5_000,
            connections: 4,
            transport: BenchmarkTransport::Persistent,
            rate: None,
            args_json: serde_json::Value::Array(vec![
                serde_json::json!("$client"),
                serde_json::json!("$seq"),
                serde_json::json!(64),
                serde_json::json!(true),
            ]),
            args_interstice_json: None,
            verify: VerifyConfig::default(),
            latency_sample_stride: super::types::DEFAULT_LATENCY_SAMPLE_STRIDE,
            throughput_mode: ThroughputMode::QueryDelta,
            throughput_query_name: Some("total_committed".to_string()),
            throughput_query_args_json: default_args_json(),
            throughput_query_args_interstice_json: None,
            throughput_query_field: None,
            output: Some(format!(
                "benchmarks/results/profile_insert_ephemeral_{}.json",
                slugify(name)
            )),
            reset_before: true,
        }],
        "durability" => vec![BenchmarkRunConfig {
            node: "bench-node".to_string(),
            module_name: "benchmark-workload".to_string(),
            reducer_name: "tx_insert_logged".to_string(),
            name: Some("durability".to_string()),
            duration_ms: 10_000,
            warmup_ms: 0,
            connections: 4,
            transport: BenchmarkTransport::Persistent,
            rate: None,
            args_json: serde_json::Value::Array(vec![
                serde_json::json!("$client"),
                serde_json::json!("$seq"),
                serde_json::json!(64),
                serde_json::json!(true),
            ]),
            args_interstice_json: None,
            verify: VerifyConfig::default(),
            latency_sample_stride: super::types::DEFAULT_LATENCY_SAMPLE_STRIDE,
            throughput_mode: ThroughputMode::QueryDelta,
            throughput_query_name: Some("total_committed".to_string()),
            throughput_query_args_json: default_args_json(),
            throughput_query_args_interstice_json: None,
            throughput_query_field: None,
            output: Some(format!(
                "benchmarks/results/profile_durability_{}.json",
                slugify(name)
            )),
            reset_before: true,
        }],
        "fanout" => vec![
            BenchmarkRunConfig {
                node: "bench-node".to_string(),
                module_name: "benchmark-workload".to_string(),
                reducer_name: "start_tick".to_string(),
                name: Some("fanout-start".to_string()),
                duration_ms: 1_000,
                warmup_ms: 0,
                connections: 1,
                transport: BenchmarkTransport::Persistent,
                rate: Some(1),
                args_json: serde_json::json!([16]),
                args_interstice_json: None,
                verify: VerifyConfig::default(),
                latency_sample_stride: super::types::DEFAULT_LATENCY_SAMPLE_STRIDE,
                throughput_mode: ThroughputMode::DispatchSuccess,
                throughput_query_name: None,
                throughput_query_args_json: default_args_json(),
                throughput_query_args_interstice_json: None,
                throughput_query_field: None,
                output: Some("benchmarks/results/profile_fanout_start.json".to_string()),
                reset_before: false,
            },
            BenchmarkRunConfig {
                node: "bench-node".to_string(),
                module_name: "benchmark-workload".to_string(),
                reducer_name: "emit_event".to_string(),
                name: Some("fanout".to_string()),
                duration_ms: 10_000,
                warmup_ms: 2_000,
                connections: 4,
                transport: BenchmarkTransport::Persistent,
                rate: None,
                args_json: serde_json::json!(["$seq", "$client", {"event":"fanout","worker":"$worker","op":"$op"}]),
                args_interstice_json: None,
                verify: VerifyConfig {
                    mode: super::types::VerifyMode::Query,
                    query_name: Some("stats".to_string()),
                    args_json: default_args_json(),
                    args_interstice_json: None,
                    expect_json: None,
                    expect_interstice_json: None,
                },
                latency_sample_stride: super::types::DEFAULT_LATENCY_SAMPLE_STRIDE,
                throughput_mode: ThroughputMode::DispatchSuccess,
                throughput_query_name: None,
                throughput_query_args_json: default_args_json(),
                throughput_query_args_interstice_json: None,
                throughput_query_field: None,
                output: Some("benchmarks/results/profile_fanout.json".to_string()),
                reset_before: false,
            },
            BenchmarkRunConfig {
                node: "bench-node".to_string(),
                module_name: "benchmark-workload".to_string(),
                reducer_name: "stop_tick".to_string(),
                name: Some("fanout-stop".to_string()),
                duration_ms: 1_000,
                warmup_ms: 0,
                connections: 1,
                transport: BenchmarkTransport::Persistent,
                rate: Some(1),
                args_json: default_args_json(),
                args_interstice_json: None,
                verify: VerifyConfig::default(),
                latency_sample_stride: super::types::DEFAULT_LATENCY_SAMPLE_STRIDE,
                throughput_mode: ThroughputMode::DispatchSuccess,
                throughput_query_name: None,
                throughput_query_args_json: default_args_json(),
                throughput_query_args_interstice_json: None,
                throughput_query_field: None,
                output: Some("benchmarks/results/profile_fanout_stop.json".to_string()),
                reset_before: false,
            },
        ],
        other => {
            return Err(IntersticeError::Internal(format!(
                "Unknown benchmark profile '{}'",
                other
            )));
        }
    };

    let mut normalized = Vec::with_capacity(scenarios.len());
    for mut scenario in scenarios {
        scenario.normalize()?;
        normalized.push(scenario);
    }

    Ok(normalized)
}
