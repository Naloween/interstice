//! `interstice-bench`: the load benchmarks (benchmarks/README.md). The
//! micro-benchmarks run with `cargo bench -p interstice-bench`.

use interstice_bench::node::{BenchNode, MODULE};
use interstice_bench::transfer::{self, CallFuture, Caller, TransferConfig};
use interstice_client::Client;
use interstice_core::{KnownPeers, LocalIdentity, NodeHandle};
use interstice_core::interstice_abi::IntersticeValue;
use std::sync::Arc;
use std::time::Duration;

const USAGE: &str = "\
usage: interstice-bench breakdown [--iters n]
  runs each micro operation through the bench module with the profiler on
  and prints its phase breakdown (DESIGN §6.8)
usage: interstice-bench transfer [options]
  --node <addr>    a running node with the bench module (host:port);
                   without it, a headless node runs in process
  --profile        print the node's phase breakdown (in process only)
  --accounts <n>   accounts (100000)
  --alpha <a>      Zipf exponent for account picks; 0 = uniform (0)
  --clients <n>    concurrent clients (64)
  --inflight <n>   calls in flight per client (40)
  --warmup <s>     seconds before measuring (5)
  --seconds <s>    seconds measured (30)";

struct InProcess(NodeHandle);

impl Caller for InProcess {
    fn transfer(&self, from: u32, to: u32) -> CallFuture {
        let rx = self.0.submit_reducer(MODULE, "transfer", transfer::transfer_args(from, to));
        Box::pin(async move {
            match rx {
                Ok(rx) => rx.await.unwrap_or_else(|_| Err("dropped".into())),
                Err(e) => Err(e.to_string()),
            }
        })
    }
}

struct Remote(Client);

impl Caller for Remote {
    fn transfer(&self, from: u32, to: u32) -> CallFuture {
        Box::pin(self.0.submit_reducer(MODULE, "transfer", transfer::transfer_args(from, to)))
    }
}

#[derive(Default)]
struct Options {
    node: Option<String>,
    profile: bool,
}

fn parse(args: &[String]) -> Result<(TransferConfig, Options), String> {
    let mut c = TransferConfig::default();
    let mut o = Options::default();
    let mut it = args.iter();
    while let Some(flag) = it.next() {
        if flag == "--profile" {
            o.profile = true;
            continue;
        }
        let mut value = || it.next().ok_or(format!("{flag} needs a value"));
        let bad = |e: &dyn std::fmt::Display| format!("{flag}: {e}");
        match flag.as_str() {
            "--accounts" => c.accounts = value()?.parse().map_err(|e| bad(&e))?,
            "--alpha" => c.alpha = value()?.parse().map_err(|e| bad(&e))?,
            "--clients" => c.clients = value()?.parse().map_err(|e| bad(&e))?,
            "--inflight" => c.inflight = value()?.parse().map_err(|e| bad(&e))?,
            "--warmup" => c.warmup = Duration::from_secs(value()?.parse().map_err(|e| bad(&e))?),
            "--seconds" => c.duration = Duration::from_secs(value()?.parse().map_err(|e| bad(&e))?),
            "--node" => o.node = Some(value()?.clone()),
            other => return Err(format!("unknown option {other}")),
        }
    }
    Ok((c, o))
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let rest = &args[1.min(args.len())..];
    match args.first().map(String::as_str) {
        Some("transfer") => {}
        Some("breakdown") => {
            let iters = rest
                .iter()
                .position(|a| a == "--iters")
                .and_then(|i| rest.get(i + 1))
                .and_then(|v| v.parse().ok())
                .unwrap_or(200);
            return breakdown(iters);
        }
        _ => {
            eprintln!("{USAGE}");
            std::process::exit(2);
        }
    }
    let (config, options) = parse(rest).unwrap_or_else(|e| {
        eprintln!("{e}\n{USAGE}");
        std::process::exit(2);
    });
    match &options.node {
        Some(address) => over_network(config, address),
        None => in_process(config, options.profile),
    }
}

fn seed_args(config: &TransferConfig) -> IntersticeValue {
    IntersticeValue::Vec(vec![IntersticeValue::U32(config.accounts), IntersticeValue::I64(1_000_000_000)])
}

fn check(before: IntersticeValue, after: IntersticeValue) {
    if before != after {
        eprintln!("  INVARIANT BROKEN: total balance {before:?} -> {after:?}");
        std::process::exit(1);
    }
    println!("  total balance unchanged: {after:?}");
}

fn in_process(config: TransferConfig, profile: bool) {
    let node = BenchNode::start();
    let IntersticeValue::Vec(seed) = seed_args(&config) else { unreachable!() };
    node.call("seed_accounts", seed).expect("seed accounts");
    let before = node.query("total_balance", vec![]).expect("total");
    if profile {
        interstice_core::profile::enable();
    }
    let callers: Vec<Arc<dyn Caller>> = vec![Arc::new(InProcess(node.handle.clone()))];
    let mut report = node.rt.block_on(transfer::run(config, callers));
    report.print();
    if profile {
        println!("{}", interstice_core::profile::report());
    }
    check(before, node.query("total_balance", vec![]).expect("total"));
}

/// One TLS connection, with its own key, per client.
fn over_network(config: TransferConfig, address: &str) {
    let rt = tokio::runtime::Runtime::new().expect("tokio");
    rt.block_on(async {
        let known = KnownPeers::load(None);
        let mut clients = Vec::new();
        for _ in 0..config.clients {
            let (identity, _) = LocalIdentity::generate().expect("key");
            clients.push(Client::connect(address, &identity, &known).await.expect("connect"));
        }
        let first = &clients[0];
        first.call_reducer(MODULE, "seed_accounts", seed_args(&config)).await.expect("seed accounts");
        let before = first.call_query(MODULE, "total_balance", IntersticeValue::Vec(vec![])).await.expect("total");
        let probe = Client::connect(address, &LocalIdentity::generate().expect("key").0, &known)
            .await
            .expect("connect");
        let callers: Vec<Arc<dyn Caller>> =
            clients.into_iter().map(|c| Arc::new(Remote(c)) as Arc<dyn Caller>).collect();
        let mut report = transfer::run(config, callers).await;
        report.print();
        let after = probe.call_query(MODULE, "total_balance", IntersticeValue::Vec(vec![])).await.expect("total");
        check(before, after);
    });
}

/// The phase breakdown of each micro operation, through the module.
fn breakdown(iters: u32) {
    use interstice_bench::data::{self, IndexStrategy, Schema, TableId, XorShift};
    use interstice_bench::targets::{BenchDatabase, ModuleTarget};
    let mut target = ModuleTarget::new();
    let unique = TableId { schema: Schema::U32U64Str, index: IndexStrategy::Unique0, disk: false };
    let btree = TableId { schema: Schema::U32U64Str, index: IndexStrategy::BtreeEachColumn, disk: false };
    let mut rng = XorShift::new();
    let rows = data::rows(&mut rng, 0, 256, 8);

    let run = |name: &str, target: &mut ModuleTarget, f: &mut dyn FnMut(&mut ModuleTarget)| {
        interstice_core::profile::enable();
        let start = std::time::Instant::now();
        for _ in 0..iters {
            f(target);
        }
        let per = start.elapsed() / iters;
        println!("== {name}: {per:?} per call ({iters} calls)");
        println!("{}", interstice_core::profile::report());
    };
    run("empty", &mut target, &mut |t| t.empty());
    run("insert_bulk 256 rows (unique_0, includes a clear call)", &mut target, &mut |t| {
        t.clear(unique);
        t.insert_bulk(unique, &rows);
    });
    run("update_bulk 256 rows", &mut target, &mut |t| t.update_bulk(unique, 256));
    run("iterate 256 rows", &mut target, &mut |t| t.iterate(unique));
    target.clear(btree);
    target.insert_bulk(btree, &data::rows(&mut rng, 0, 2048, 8));
    run("filter ~256 of 2048 rows", &mut target, &mut |t| t.filter(btree, 3));
}
