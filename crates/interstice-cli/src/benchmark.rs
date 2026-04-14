use crate::node_client::handshake_with_node;
use crate::node_registry::NodeRegistry;
use interstice_core::{
    IntersticeError, NetworkPacket,
    interstice_abi::{Field, IntersticeValue},
    packet::{read_packet, write_packet},
};
use tokio::time::{Duration, Instant, sleep};
use uuid::Uuid;

pub async fn handle_benchmark_command(args: &[String]) -> Result<(), IntersticeError> {
    if args.len() < 3 || args[2] != "simple" {
        print_benchmark_help();
        return Ok(());
    }
    run_simple(args).await
}

async fn run_simple(args: &[String]) -> Result<(), IntersticeError> {
    let mut node = String::new();
    let mut module = "benchmark-workload".to_string();
    let mut table = "ephemeral".to_string();
    let mut operation = "insert".to_string();
    let mut requests: u64 = 10_000;
    let mut poll_ms: u64 = 1_000;
    let mut payload_bytes: u64 = 64;

    let mut i = 3;
    while i < args.len() {
        match args[i].as_str() {
            "--node" => {
                node = arg_value(args, i + 1, "--node")?;
                i += 2;
            }
            "--module" => {
                module = arg_value(args, i + 1, "--module")?;
                i += 2;
            }
            "--table" => {
                table = arg_value(args, i + 1, "--table")?;
                i += 2;
            }
            "--operation" => {
                operation = arg_value(args, i + 1, "--operation")?;
                i += 2;
            }
            "--requests" => {
                requests = parse_u64(&arg_value(args, i + 1, "--requests")?, "--requests")?;
                i += 2;
            }
            "--poll-ms" => {
                poll_ms = parse_u64(&arg_value(args, i + 1, "--poll-ms")?, "--poll-ms")?;
                i += 2;
            }
            "--payload-bytes" => {
                payload_bytes = parse_u64(
                    &arg_value(args, i + 1, "--payload-bytes")?,
                    "--payload-bytes",
                )?;
                i += 2;
            }
            unknown => {
                return Err(IntersticeError::Internal(format!(
                    "Unknown benchmark option '{}'",
                    unknown
                )));
            }
        }
    }

    if node.is_empty() {
        return Err(IntersticeError::Internal(
            "Missing required option --node".to_string(),
        ));
    }
    if !matches!(table.as_str(), "ephemeral" | "stateful" | "logged") {
        return Err(IntersticeError::Internal(
            "--table must be one of: ephemeral, stateful, logged".to_string(),
        ));
    }
    if !matches!(operation.as_str(), "insert" | "update" | "delete") {
        return Err(IntersticeError::Internal(
            "--operation must be one of: insert, update, delete".to_string(),
        ));
    }

    let registry = NodeRegistry::load()?;
    let address = registry
        .resolve_address(&node)
        .ok_or_else(|| IntersticeError::Internal(format!("Unknown node '{}'", node)))?;
    let run_id = format!("bench-{}", Uuid::new_v4());

    let (mut stream, _) = handshake_with_node(&address).await?;
    write_packet(
        &mut stream,
        &NetworkPacket::ReducerCall {
            module_name: module.clone(),
            reducer_name: "bench_begin_run".to_string(),
            input: IntersticeValue::Vec(vec![
                IntersticeValue::String(run_id.clone()),
                IntersticeValue::String(table.clone()),
                IntersticeValue::String(operation.clone()),
                IntersticeValue::U64(requests),
                IntersticeValue::U64(payload_bytes),
            ]),
        },
    )
    .await?;

    for seq in 0..requests {
        let packet = NetworkPacket::ReducerCall {
            module_name: module.clone(),
            reducer_name: "bench_tx".to_string(),
            input: IntersticeValue::Vec(vec![
                IntersticeValue::String(run_id.clone()),
                IntersticeValue::U64(seq),
                IntersticeValue::U64(payload_bytes),
            ]),
        };
        write_packet(&mut stream, &packet).await?;
    }
    if poll_ms == 0 {
        return Err(IntersticeError::Internal(
            "--poll-ms must be greater than 0".to_string(),
        ));
    }
    let client_start = Instant::now();
    let timeout = Duration::from_secs(30 * 60);
    let snapshot = loop {
        let snapshot = query_snapshot_on_stream(&mut stream, &module, &run_id).await?;
        if snapshot.completed && snapshot.committed >= requests {
            break snapshot;
        }
        if client_start.elapsed() >= timeout {
            return Err(IntersticeError::Internal(format!(
                "Benchmark timed out waiting for completion (committed={} target={})",
                snapshot.committed, requests
            )));
        }
        sleep(Duration::from_millis(poll_ms)).await;
    };
    let _ = write_packet(&mut stream, &NetworkPacket::Close).await;
    println!("Benchmark Simple Report");
    println!("  node: {}", node);
    println!("  module: {}", module);
    println!("  table: {}", table);
    println!("  operation: {}", operation);
    println!("  requests_sent: {}", requests);
    println!("  poll_ms: {}", poll_ms);
    println!("  committed: {}", snapshot.committed);
    println!("  target_requests: {}", snapshot.target_requests);
    println!("  completed: {}", snapshot.completed);
    println!("  elapsed_ms: {}", snapshot.elapsed_ms);
    println!("  committed_tps: {:.2}", snapshot.tps);
    Ok(())
}

async fn query_snapshot(
    address: &str,
    module_name: &str,
    run_id: &str,
) -> Result<Snapshot, IntersticeError> {
    let (mut stream, _) = handshake_with_node(address).await?;
    let request_id = Uuid::new_v4().to_string();
    let packet = NetworkPacket::QueryCall {
        request_id: request_id.clone(),
        module_name: module_name.to_string(),
        query_name: "bench_snapshot".to_string(),
        input: IntersticeValue::Vec(vec![IntersticeValue::String(run_id.to_string())]),
    };
    write_packet(&mut stream, &packet).await?;
    loop {
        let packet = read_packet(&mut stream).await?;
        match packet {
            NetworkPacket::QueryResponse {
                request_id: response_id,
                result,
            } if response_id == request_id => {
                let _ = write_packet(&mut stream, &NetworkPacket::Close).await;
                return parse_snapshot(result);
            }
            NetworkPacket::Error(err) => {
                return Err(IntersticeError::Internal(format!(
                    "Query failed with error: {}",
                    err
                )));
            }
            _ => {}
        }
    }
}

async fn query_snapshot_on_stream(
    stream: &mut tokio::net::TcpStream,
    module_name: &str,
    run_id: &str,
) -> Result<Snapshot, IntersticeError> {
    let request_id = Uuid::new_v4().to_string();
    let packet = NetworkPacket::QueryCall {
        request_id: request_id.clone(),
        module_name: module_name.to_string(),
        query_name: "bench_snapshot".to_string(),
        input: IntersticeValue::Vec(vec![IntersticeValue::String(run_id.to_string())]),
    };
    write_packet(stream, &packet).await?;
    loop {
        let packet = read_packet(stream).await?;
        match packet {
            NetworkPacket::QueryResponse {
                request_id: response_id,
                result,
            } if response_id == request_id => {
                return parse_snapshot(result);
            }
            NetworkPacket::Error(err) => {
                return Err(IntersticeError::Internal(format!(
                    "Query failed with error: {}",
                    err
                )));
            }
            _ => {}
        }
    }
}

#[derive(Default)]
struct Snapshot {
    committed: u64,
    target_requests: u64,
    completed: bool,
    elapsed_ms: u64,
    tps: f64,
}

fn parse_snapshot(value: IntersticeValue) -> Result<Snapshot, IntersticeError> {
    let fields = match value {
        IntersticeValue::Struct { fields, .. } => fields,
        other => {
            return Err(IntersticeError::Internal(format!(
                "Unexpected snapshot value: {}",
                other
            )));
        }
    };
    Ok(Snapshot {
        committed: field_u64(&fields, "committed"),
        target_requests: field_u64(&fields, "target_requests"),
        completed: field_bool(&fields, "completed"),
        elapsed_ms: field_u64(&fields, "elapsed_ms"),
        tps: field_f64(&fields, "tps"),
    })
}

fn field_bool(fields: &[Field], name: &str) -> bool {
    fields
        .iter()
        .find(|field| field.name == name)
        .and_then(|field| match field.value {
            IntersticeValue::Bool(v) => Some(v),
            _ => None,
        })
        .unwrap_or(false)
}

fn field_u64(fields: &[Field], name: &str) -> u64 {
    fields
        .iter()
        .find(|field| field.name == name)
        .and_then(|field| match field.value {
            IntersticeValue::U8(v) => Some(v as u64),
            IntersticeValue::U32(v) => Some(v as u64),
            IntersticeValue::U64(v) => Some(v),
            IntersticeValue::I32(v) if v >= 0 => Some(v as u64),
            IntersticeValue::I64(v) if v >= 0 => Some(v as u64),
            _ => None,
        })
        .unwrap_or(0)
}

fn field_f64(fields: &[Field], name: &str) -> f64 {
    fields
        .iter()
        .find(|field| field.name == name)
        .and_then(|field| match field.value {
            IntersticeValue::F32(v) => Some(v as f64),
            IntersticeValue::F64(v) => Some(v),
            IntersticeValue::U64(v) => Some(v as f64),
            IntersticeValue::U32(v) => Some(v as f64),
            _ => None,
        })
        .unwrap_or(0.0)
}

fn arg_value(args: &[String], idx: usize, flag: &str) -> Result<String, IntersticeError> {
    args.get(idx).cloned().ok_or_else(|| {
        IntersticeError::Internal(format!("Missing value for option '{}'", flag))
    })
}

fn parse_u64(input: &str, flag: &str) -> Result<u64, IntersticeError> {
    input
        .parse::<u64>()
        .map_err(|_| IntersticeError::Internal(format!("Invalid value for {}: {}", flag, input)))
}

pub fn print_benchmark_help() {
    println!("Benchmark commands:");
    println!("  interstice benchmark simple --node <node> [options]");
    println!("Options:");
    println!("  --module <name>           Module name (default: benchmark-workload)");
    println!("  --table <kind>            ephemeral|stateful|logged (default: ephemeral)");
    println!("  --operation <kind>        insert|update|delete (default: insert)");
    println!("  --requests <n>            Number of reducer calls sent (default: 10000)");
    println!("  --poll-ms <ms>            Snapshot polling interval (default: 1000)");
    println!("  --payload-bytes <n>       Payload size in bytes (default: 64)");
}
