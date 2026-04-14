use interstice_sdk::*;

interstice_module!(visibility: Public);

#[interstice_type]
#[derive(Debug, Clone)]
pub struct BenchSnapshot {
    pub run_id: String,
    pub table_kind: String,
    pub operation: String,
    pub committed: u64,
    pub target_requests: u64,
    pub completed: bool,
    pub elapsed_ms: u64,
    pub tps: f64,
}

#[table(public, stateful)]
#[derive(Debug, Clone)]
pub struct BenchRun {
    #[primary_key]
    pub key: String,
    pub run_id: String,
    pub table_kind: String,
    pub operation: String,
    pub started_ms: u64,
    pub committed: u64,
    pub target_requests: u64,
    pub completed_ms: u64,
}

#[table(public, ephemeral)]
#[derive(Debug, Clone)]
pub struct BenchEphemeral {
    #[primary_key]
    pub key: String,
    pub value: u64,
    pub payload: String,
}

#[table(public, stateful)]
#[derive(Debug, Clone)]
pub struct BenchStateful {
    #[primary_key]
    pub key: String,
    pub value: u64,
    pub payload: String,
}

#[table(public)]
#[derive(Debug, Clone)]
pub struct BenchLogged {
    #[primary_key]
    pub key: String,
    pub value: u64,
    pub payload: String,
}

#[reducer]
pub fn bench_begin_run<Caps>(
    ctx: ReducerContext<Caps>,
    run_id: String,
    table_kind: String,
    operation: String,
    requests: u64,
    payload_bytes: u64,
) where
    Caps: CanRead<BenchRun>
        + CanInsert<BenchRun>
        + CanUpdate<BenchRun>
        + CanDelete<BenchRun>
        + CanRead<BenchEphemeral>
        + CanInsert<BenchEphemeral>
        + CanUpdate<BenchEphemeral>
        + CanDelete<BenchEphemeral>
        + CanRead<BenchStateful>
        + CanInsert<BenchStateful>
        + CanUpdate<BenchStateful>
        + CanDelete<BenchStateful>
        + CanRead<BenchLogged>
        + CanInsert<BenchLogged>
        + CanUpdate<BenchLogged>
        + CanDelete<BenchLogged>,
{
    let table = table_kind.to_lowercase();
    let op = operation.to_lowercase();
    let payload = "x".repeat(payload_bytes as usize);
    let _ = ctx.current.tables.benchrun().delete("active".to_string());

    match table.as_str() {
        "ephemeral" => {
            let _ = ctx.current.tables.benchephemeral().clear();
            if op == "update" || op == "delete" {
                for seq in 0..requests {
                    let _ = ctx.current.tables.benchephemeral().insert(BenchEphemeral {
                        key: seq.to_string(),
                        value: seq,
                        payload: payload.clone(),
                    });
                }
            }
        }
        "stateful" => {
            let _ = ctx.current.tables.benchstateful().clear();
            if op == "update" || op == "delete" {
                for seq in 0..requests {
                    let _ = ctx.current.tables.benchstateful().insert(BenchStateful {
                        key: seq.to_string(),
                        value: seq,
                        payload: payload.clone(),
                    });
                }
            }
        }
        "logged" => {
            let _ = ctx.current.tables.benchlogged().clear();
            if op == "update" || op == "delete" {
                for seq in 0..requests {
                    let _ = ctx.current.tables.benchlogged().insert(BenchLogged {
                        key: seq.to_string(),
                        value: seq,
                        payload: payload.clone(),
                    });
                }
            }
        }
        _ => {}
    }

    let _ = ctx.current.tables.benchrun().insert(BenchRun {
        key: "active".to_string(),
        run_id,
        table_kind: table,
        operation: op,
        started_ms: now_ms(&ctx),
        committed: 0,
        target_requests: requests,
        completed_ms: 0,
    });
}

#[reducer]
pub fn bench_tx<Caps>(ctx: ReducerContext<Caps>, run_id: String, seq: u64, payload_bytes: u64)
where
    Caps: CanRead<BenchRun>
        + CanInsert<BenchRun>
        + CanUpdate<BenchRun>
        + CanDelete<BenchRun>
        + CanRead<BenchEphemeral>
        + CanInsert<BenchEphemeral>
        + CanUpdate<BenchEphemeral>
        + CanDelete<BenchEphemeral>
        + CanRead<BenchStateful>
        + CanInsert<BenchStateful>
        + CanUpdate<BenchStateful>
        + CanDelete<BenchStateful>
        + CanRead<BenchLogged>
        + CanInsert<BenchLogged>
        + CanUpdate<BenchLogged>
        + CanDelete<BenchLogged>,
{
    let Some(mut run) = ctx.current.tables.benchrun().get("active".to_string()) else {
        return;
    };
    if run.run_id != run_id {
        return;
    }
    let payload = "x".repeat(payload_bytes as usize);
    let key = seq.to_string();

    match (run.table_kind.as_str(), run.operation.as_str()) {
        ("ephemeral", "insert") => {
            let _ = ctx.current.tables.benchephemeral().insert(BenchEphemeral {
                key,
                value: seq,
                payload,
            });
        }
        ("ephemeral", "update") => {
            if let Some(mut row) = ctx.current.tables.benchephemeral().get(key.clone()) {
                row.value = seq;
                row.payload = payload;
                let _ = ctx.current.tables.benchephemeral().update(row);
            }
        }
        ("ephemeral", "delete") => {
            let _ = ctx.current.tables.benchephemeral().delete(key);
        }
        ("stateful", "insert") => {
            let _ = ctx.current.tables.benchstateful().insert(BenchStateful {
                key,
                value: seq,
                payload,
            });
        }
        ("stateful", "update") => {
            if let Some(mut row) = ctx.current.tables.benchstateful().get(key.clone()) {
                row.value = seq;
                row.payload = payload;
                let _ = ctx.current.tables.benchstateful().update(row);
            }
        }
        ("stateful", "delete") => {
            let _ = ctx.current.tables.benchstateful().delete(key);
        }
        ("logged", "insert") => {
            let _ = ctx.current.tables.benchlogged().insert(BenchLogged {
                key,
                value: seq,
                payload,
            });
        }
        ("logged", "update") => {
            if let Some(mut row) = ctx.current.tables.benchlogged().get(key.clone()) {
                row.value = seq;
                row.payload = payload;
                let _ = ctx.current.tables.benchlogged().update(row);
            }
        }
        ("logged", "delete") => {
            let _ = ctx.current.tables.benchlogged().delete(key);
        }
        _ => return,
    }

    run.committed = run.committed.saturating_add(1);
    if run.completed_ms == 0 && run.committed >= run.target_requests {
        run.completed_ms = now_ms(&ctx);
    }
    let _ = ctx.current.tables.benchrun().update(run);
}

#[query]
pub fn bench_snapshot<Caps: CanRead<BenchRun>>(ctx: QueryContext<Caps>, run_id: String) -> BenchSnapshot {
    let Some(run) = ctx.current.tables.benchrun().get("active".to_string()) else {
        return BenchSnapshot {
            run_id,
            table_kind: String::new(),
            operation: String::new(),
            committed: 0,
            target_requests: 0,
            completed: false,
            elapsed_ms: 0,
            tps: 0.0,
        };
    };
    if run.run_id != run_id {
        return BenchSnapshot {
            run_id,
            table_kind: String::new(),
            operation: String::new(),
            committed: 0,
            target_requests: 0,
            completed: false,
            elapsed_ms: 0,
            tps: 0.0,
        };
    }
    let end_ms = if run.completed_ms > 0 {
        run.completed_ms
    } else {
        ctx.time_now_ms().unwrap_or(run.started_ms)
    };
    let elapsed_ms = end_ms.saturating_sub(run.started_ms);
    let tps = if elapsed_ms > 0 {
        run.committed as f64 / (elapsed_ms as f64 / 1_000.0)
    } else {
        0.0
    };
    BenchSnapshot {
        run_id: run.run_id,
        table_kind: run.table_kind,
        operation: run.operation,
        committed: run.committed,
        target_requests: run.target_requests,
        completed: run.completed_ms > 0,
        elapsed_ms,
        tps,
    }
}

#[query]
pub fn bench_snapshot_active<Caps: CanRead<BenchRun>>(ctx: QueryContext<Caps>) -> BenchSnapshot {
    let Some(run) = ctx.current.tables.benchrun().get("active".to_string()) else {
        return BenchSnapshot {
            run_id: String::new(),
            table_kind: String::new(),
            operation: String::new(),
            committed: 0,
            target_requests: 0,
            completed: false,
            elapsed_ms: 0,
            tps: 0.0,
        };
    };
    let end_ms = if run.completed_ms > 0 {
        run.completed_ms
    } else {
        ctx.time_now_ms().unwrap_or(run.started_ms)
    };
    let elapsed_ms = end_ms.saturating_sub(run.started_ms);
    let tps = if elapsed_ms > 0 {
        run.committed as f64 / (elapsed_ms as f64 / 1_000.0)
    } else {
        0.0
    };
    BenchSnapshot {
        run_id: run.run_id,
        table_kind: run.table_kind,
        operation: run.operation,
        committed: run.committed,
        target_requests: run.target_requests,
        completed: run.completed_ms > 0,
        elapsed_ms,
        tps,
    }
}

fn now_ms<Caps>(ctx: &ReducerContext<Caps>) -> u64 {
    ctx.time_now_ms().unwrap_or(0)
}
