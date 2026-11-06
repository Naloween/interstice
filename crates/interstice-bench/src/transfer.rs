//! The keynote transfer load (benchmarks/README.md): accounts, transfers
//! between Zipf-picked pairs, many calls in flight, and only committed
//! calls counted, at the client, per second.

use crate::stats::{Latencies, mean_std};
use crate::zipf::{Rng, Zipf};
use interstice_core::interstice_abi::IntersticeValue;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

#[derive(Clone, Debug)]
pub struct TransferConfig {
    pub accounts: u32,
    pub alpha: f64,
    /// Concurrent clients.
    pub clients: usize,
    /// Calls each client keeps in flight.
    pub inflight: usize,
    pub warmup: Duration,
    pub duration: Duration,
}

impl Default for TransferConfig {
    /// SpacetimeDB's keynote setup, with a shorter run.
    fn default() -> Self {
        Self {
            accounts: 100_000,
            alpha: 0.0,
            clients: 64,
            inflight: 40,
            warmup: Duration::from_secs(5),
            duration: Duration::from_secs(30),
        }
    }
}

pub struct TransferReport {
    pub config: TransferConfig,
    /// Committed transfers per second, one entry per measured second.
    pub per_second: Vec<f64>,
    pub committed: u64,
    /// Calls the reducer rejected (none expected with these balances).
    pub failed: u64,
    pub latencies: Latencies,
}

impl TransferReport {
    pub fn print(&mut self) {
        let (mean, std) = mean_std(&self.per_second);
        let c = &self.config;
        println!(
            "transfer accounts={} alpha={} clients={} inflight={} seconds={}",
            c.accounts,
            c.alpha,
            c.clients,
            c.inflight,
            c.duration.as_secs()
        );
        println!("  committed/s : {mean:.0} ± {std:.0}");
        println!("  committed   : {}   failed: {}", self.committed, self.failed);
        println!(
            "  latency µs  : p50 {}  p95 {}  p99 {}  max {}",
            self.latencies.quantile(0.50),
            self.latencies.quantile(0.95),
            self.latencies.quantile(0.99),
            self.latencies.quantile(1.0),
        );
    }
}

/// One call: `transfer(from, to, 1)`, resolving to its outcome.
pub type CallFuture = Pin<Box<dyn Future<Output = Result<(), String>> + Send>>;

/// How the load reaches the node: in process, or over the network.
pub trait Caller: Send + Sync + 'static {
    fn transfer(&self, from: u32, to: u32) -> CallFuture;
}

pub fn transfer_args(from: u32, to: u32) -> IntersticeValue {
    IntersticeValue::Vec(vec![
        IntersticeValue::U32(from),
        IntersticeValue::U32(to),
        IntersticeValue::I64(1),
    ])
}

/// Drive `callers[i % len]` for every in-flight slot of every client.
pub async fn run(config: TransferConfig, callers: Vec<Arc<dyn Caller>>) -> TransferReport {
    let zipf = Arc::new(Zipf::new(config.accounts, config.alpha));
    let committed = Arc::new(AtomicU64::new(0));
    let failed = Arc::new(AtomicU64::new(0));
    let measuring = Arc::new(AtomicBool::new(false));
    let stop = Arc::new(AtomicBool::new(false));

    let mut tasks = Vec::new();
    for client in 0..config.clients {
        let caller = callers[client % callers.len()].clone();
        for slot in 0..config.inflight {
            let (caller, zipf) = (caller.clone(), zipf.clone());
            let (committed, failed) = (committed.clone(), failed.clone());
            let (measuring, stop) = (measuring.clone(), stop.clone());
            let seed = (client as u64) << 32 | slot as u64 | 1;
            tasks.push(tokio::spawn(async move {
                let mut rng = Rng::new(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15));
                let mut latencies = Latencies::default();
                while !stop.load(Ordering::Relaxed) {
                    let from = zipf.sample(rng.next_f64());
                    let mut to = zipf.sample(rng.next_f64());
                    while to == from {
                        to = zipf.sample(rng.next_f64());
                    }
                    let start = Instant::now();
                    let outcome = caller.transfer(from, to).await;
                    if measuring.load(Ordering::Relaxed) {
                        match outcome {
                            Ok(()) => {
                                committed.fetch_add(1, Ordering::Relaxed);
                                latencies.record(start.elapsed());
                            }
                            Err(_) => {
                                failed.fetch_add(1, Ordering::Relaxed);
                            }
                        }
                    }
                }
                latencies
            }));
        }
    }

    tokio::time::sleep(config.warmup).await;
    measuring.store(true, Ordering::Relaxed);
    let mut per_second = Vec::new();
    let mut last = committed.load(Ordering::Relaxed);
    let mut tick = tokio::time::interval(Duration::from_secs(1));
    tick.tick().await;
    for _ in 0..config.duration.as_secs() {
        tick.tick().await;
        let now = committed.load(Ordering::Relaxed);
        per_second.push((now - last) as f64);
        last = now;
    }
    measuring.store(false, Ordering::Relaxed);
    stop.store(true, Ordering::Relaxed);

    let mut latencies = Latencies::default();
    for task in tasks {
        if let Ok(l) = task.await {
            latencies.merge(l);
        }
    }
    TransferReport {
        config,
        per_second,
        committed: last,
        failed: failed.load(Ordering::Relaxed),
        latencies,
    }
}
