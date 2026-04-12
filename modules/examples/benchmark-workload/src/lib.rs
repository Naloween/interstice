//! Example workload for CLI benchmarks (`interstice benchmark …`). Use `total_committed` with
//! `throughput_mode = "query_delta"` to measure committed transactions (target ~100k in long runs).

use interstice_sdk::*;

interstice_module!(visibility: Public);

const FANOUT_KEY: &str = "local";
const TICK_STATE_KEY: &str = "scheduler";

/// Maximum number of unique rows per client per table.
/// After this many inserts, keys wrap around and duplicate-PK inserts
/// become silent no-ops. This bounds memory usage and prevents OOM.
const MAX_BENCH_ROWS: u64 = 100_000;

#[interstice_type]
#[derive(Debug, Clone)]
pub struct BenchStats {
    pub ephemeral_rows: u64,
    pub stateful_rows: u64,
    pub logged_rows: u64,
    pub progress_rows: u64,
    pub fanout_seen: u64,
    pub tick_enabled: bool,
    pub tick_interval_ms: u64,
    pub tick_count: u64,
    pub last_seq: u64,
    pub committed: u64,
}

#[interstice_type]
#[derive(Debug, Clone)]
pub struct BenchGlobalCounts {
    pub ephemeral_rows: u64,
    pub stateful_rows: u64,
    pub logged_rows: u64,
    pub progress_rows: u64,
    pub event_rows: u64,
}

#[table(public, ephemeral)]
#[derive(Debug, Clone)]
pub struct BenchEphemeral {
    #[primary_key]
    pub key: String,
    pub client_id: String,
    pub seq: u64,
    pub payload: String,
    pub committed_ms: u64,
}

#[table(public, stateful)]
#[derive(Debug, Clone)]
pub struct BenchStateful {
    #[primary_key]
    pub key: String,
    pub client_id: String,
    pub seq: u64,
    pub payload: String,
    pub committed_ms: u64,
}

#[table(public)]
#[derive(Debug, Clone)]
pub struct BenchLogged {
    #[primary_key]
    pub key: String,
    pub client_id: String,
    pub seq: u64,
    pub payload: String,
    pub committed_ms: u64,
}

#[table(public, stateful)]
#[derive(Debug, Clone)]
pub struct BenchProgress {
    #[primary_key]
    pub client_id: String,
    pub last_seq: u64,
    pub committed: u64,
    pub updated_ms: u64,
}

#[table(public, ephemeral)]
#[derive(Debug, Clone)]
pub struct BenchEvent {
    #[primary_key(auto_inc)]
    pub id: u64,
    pub client_id: String,
    pub seq: u64,
    pub payload: String,
}

#[table(public, stateful)]
#[derive(Debug, Clone)]
pub struct BenchFanout {
    #[primary_key]
    pub key: String,
    pub seen: u64,
    pub last_client_id: String,
    pub last_seq: u64,
    pub updated_ms: u64,
}

#[table(public, stateful)]
#[derive(Debug, Clone)]
pub struct BenchTickState {
    #[primary_key]
    pub key: String,
    pub enabled: bool,
    pub tick_ms: u64,
    pub ticks: u64,
    pub last_tick_ms: u64,
}

#[reducer(
    on = "load",
    reads = [benchephemeral, benchstateful, benchlogged, benchprogress, benchevent, benchfanout, benchtickstate],
    inserts = [benchephemeral, benchstateful, benchlogged, benchprogress, benchevent, benchfanout, benchtickstate],
    updates = [benchephemeral, benchstateful, benchlogged, benchprogress, benchevent, benchfanout, benchtickstate],
    deletes = [benchephemeral, benchstateful, benchlogged, benchprogress, benchevent, benchfanout, benchtickstate]
)]
pub fn load(ctx: ReducerContext) {
    ensure_fanout_counter(&ctx);
    ensure_tick_state(&ctx);
}

#[reducer]
pub fn noop(_ctx: ReducerContext) {}

#[reducer(
    reads = [benchephemeral, benchstateful, benchlogged, benchprogress, benchevent, benchfanout, benchtickstate],
    inserts = [benchephemeral, benchstateful, benchlogged, benchprogress, benchevent, benchfanout, benchtickstate],
    updates = [benchephemeral, benchstateful, benchlogged, benchprogress, benchevent, benchfanout, benchtickstate],
    deletes = [benchephemeral, benchstateful, benchlogged, benchprogress, benchevent, benchfanout, benchtickstate]
)]
pub fn reset(ctx: ReducerContext) {
    let _ = ctx.current.tables.benchephemeral().clear();
    let _ = ctx.current.tables.benchstateful().clear();
    let _ = ctx.current.tables.benchlogged().clear();
    let _ = ctx.current.tables.benchprogress().clear();
    let _ = ctx.current.tables.benchevent().clear();
    let _ = ctx.current.tables.benchfanout().clear();
    let _ = ctx.current.tables.benchtickstate().clear();

    ensure_fanout_counter(&ctx);
    ensure_tick_state(&ctx);
}

#[reducer(
    reads = [benchephemeral, benchstateful, benchlogged, benchprogress, benchevent, benchfanout, benchtickstate],
    inserts = [benchephemeral, benchstateful, benchlogged, benchprogress, benchevent, benchfanout, benchtickstate],
    updates = [benchephemeral, benchstateful, benchlogged, benchprogress, benchevent, benchfanout, benchtickstate],
    deletes = [benchephemeral, benchstateful, benchlogged, benchprogress, benchevent, benchfanout, benchtickstate]
)]
pub fn tx_insert_ephemeral(
    ctx: ReducerContext,
    client_id: String,
    seq: u64,
    payload_bytes: u64,
    track_progress: bool,
) {
    insert_ephemeral(&ctx, &client_id, seq, payload_bytes, track_progress);
}

#[reducer(
    reads = [benchephemeral, benchstateful, benchlogged, benchprogress, benchevent, benchfanout, benchtickstate],
    inserts = [benchephemeral, benchstateful, benchlogged, benchprogress, benchevent, benchfanout, benchtickstate],
    updates = [benchephemeral, benchstateful, benchlogged, benchprogress, benchevent, benchfanout, benchtickstate],
    deletes = [benchephemeral, benchstateful, benchlogged, benchprogress, benchevent, benchfanout, benchtickstate]
)]
pub fn tx_insert_stateful(
    ctx: ReducerContext,
    client_id: String,
    seq: u64,
    payload_bytes: u64,
    track_progress: bool,
) {
    insert_stateful(&ctx, &client_id, seq, payload_bytes, track_progress);
}

#[reducer(
    reads = [benchephemeral, benchstateful, benchlogged, benchprogress, benchevent, benchfanout, benchtickstate],
    inserts = [benchephemeral, benchstateful, benchlogged, benchprogress, benchevent, benchfanout, benchtickstate],
    updates = [benchephemeral, benchstateful, benchlogged, benchprogress, benchevent, benchfanout, benchtickstate],
    deletes = [benchephemeral, benchstateful, benchlogged, benchprogress, benchevent, benchfanout, benchtickstate]
)]
pub fn tx_insert_logged(
    ctx: ReducerContext,
    client_id: String,
    seq: u64,
    payload_bytes: u64,
    track_progress: bool,
) {
    insert_logged(&ctx, &client_id, seq, payload_bytes, track_progress);
}

#[reducer(
    reads = [benchephemeral, benchstateful, benchlogged, benchprogress, benchevent, benchfanout, benchtickstate],
    inserts = [benchephemeral, benchstateful, benchlogged, benchprogress, benchevent, benchfanout, benchtickstate],
    updates = [benchephemeral, benchstateful, benchlogged, benchprogress, benchevent, benchfanout, benchtickstate],
    deletes = [benchephemeral, benchstateful, benchlogged, benchprogress, benchevent, benchfanout, benchtickstate]
)]
pub fn tx_update_ephemeral(
    ctx: ReducerContext,
    client_id: String,
    seq: u64,
    payload_bytes: u64,
    track_progress: bool,
) {
    update_ephemeral(&ctx, &client_id, seq, payload_bytes, track_progress);
}

#[reducer(
    reads = [benchephemeral, benchstateful, benchlogged, benchprogress, benchevent, benchfanout, benchtickstate],
    inserts = [benchephemeral, benchstateful, benchlogged, benchprogress, benchevent, benchfanout, benchtickstate],
    updates = [benchephemeral, benchstateful, benchlogged, benchprogress, benchevent, benchfanout, benchtickstate],
    deletes = [benchephemeral, benchstateful, benchlogged, benchprogress, benchevent, benchfanout, benchtickstate]
)]
pub fn tx_update_stateful(
    ctx: ReducerContext,
    client_id: String,
    seq: u64,
    payload_bytes: u64,
    track_progress: bool,
) {
    update_stateful(&ctx, &client_id, seq, payload_bytes, track_progress);
}

#[reducer(
    reads = [benchephemeral, benchstateful, benchlogged, benchprogress, benchevent, benchfanout, benchtickstate],
    inserts = [benchephemeral, benchstateful, benchlogged, benchprogress, benchevent, benchfanout, benchtickstate],
    updates = [benchephemeral, benchstateful, benchlogged, benchprogress, benchevent, benchfanout, benchtickstate],
    deletes = [benchephemeral, benchstateful, benchlogged, benchprogress, benchevent, benchfanout, benchtickstate]
)]
pub fn tx_update_logged(
    ctx: ReducerContext,
    client_id: String,
    seq: u64,
    payload_bytes: u64,
    track_progress: bool,
) {
    update_logged(&ctx, &client_id, seq, payload_bytes, track_progress);
}

#[reducer(
    reads = [benchephemeral, benchstateful, benchlogged, benchprogress, benchevent, benchfanout, benchtickstate],
    inserts = [benchephemeral, benchstateful, benchlogged, benchprogress, benchevent, benchfanout, benchtickstate],
    updates = [benchephemeral, benchstateful, benchlogged, benchprogress, benchevent, benchfanout, benchtickstate],
    deletes = [benchephemeral, benchstateful, benchlogged, benchprogress, benchevent, benchfanout, benchtickstate]
)]
pub fn tx_delete_ephemeral(ctx: ReducerContext, client_id: String, seq: u64, track_progress: bool) {
    delete_ephemeral(&ctx, &client_id, seq, track_progress);
}

#[reducer(
    reads = [benchephemeral, benchstateful, benchlogged, benchprogress, benchevent, benchfanout, benchtickstate],
    inserts = [benchephemeral, benchstateful, benchlogged, benchprogress, benchevent, benchfanout, benchtickstate],
    updates = [benchephemeral, benchstateful, benchlogged, benchprogress, benchevent, benchfanout, benchtickstate],
    deletes = [benchephemeral, benchstateful, benchlogged, benchprogress, benchevent, benchfanout, benchtickstate]
)]
pub fn tx_delete_stateful(ctx: ReducerContext, client_id: String, seq: u64, track_progress: bool) {
    delete_stateful(&ctx, &client_id, seq, track_progress);
}

#[reducer(
    reads = [benchephemeral, benchstateful, benchlogged, benchprogress, benchevent, benchfanout, benchtickstate],
    inserts = [benchephemeral, benchstateful, benchlogged, benchprogress, benchevent, benchfanout, benchtickstate],
    updates = [benchephemeral, benchstateful, benchlogged, benchprogress, benchevent, benchfanout, benchtickstate],
    deletes = [benchephemeral, benchstateful, benchlogged, benchprogress, benchevent, benchfanout, benchtickstate]
)]
pub fn tx_delete_logged(ctx: ReducerContext, client_id: String, seq: u64, track_progress: bool) {
    delete_logged(&ctx, &client_id, seq, track_progress);
}

#[reducer(
    reads = [benchephemeral, benchstateful, benchlogged, benchprogress, benchevent, benchfanout, benchtickstate],
    inserts = [benchephemeral, benchstateful, benchlogged, benchprogress, benchevent, benchfanout, benchtickstate],
    updates = [benchephemeral, benchstateful, benchlogged, benchprogress, benchevent, benchfanout, benchtickstate],
    deletes = [benchephemeral, benchstateful, benchlogged, benchprogress, benchevent, benchfanout, benchtickstate]
)]
pub fn tx_mix_ephemeral(
    ctx: ReducerContext,
    client_id: String,
    seq: u64,
    payload_bytes: u64,
    track_progress: bool,
) {
    mix_ephemeral(&ctx, &client_id, seq, payload_bytes, track_progress);
}

#[reducer(
    reads = [benchephemeral, benchstateful, benchlogged, benchprogress, benchevent, benchfanout, benchtickstate],
    inserts = [benchephemeral, benchstateful, benchlogged, benchprogress, benchevent, benchfanout, benchtickstate],
    updates = [benchephemeral, benchstateful, benchlogged, benchprogress, benchevent, benchfanout, benchtickstate],
    deletes = [benchephemeral, benchstateful, benchlogged, benchprogress, benchevent, benchfanout, benchtickstate]
)]
pub fn tx_mix_stateful(
    ctx: ReducerContext,
    client_id: String,
    seq: u64,
    payload_bytes: u64,
    track_progress: bool,
) {
    mix_stateful(&ctx, &client_id, seq, payload_bytes, track_progress);
}

#[reducer(
    reads = [benchephemeral, benchstateful, benchlogged, benchprogress, benchevent, benchfanout, benchtickstate],
    inserts = [benchephemeral, benchstateful, benchlogged, benchprogress, benchevent, benchfanout, benchtickstate],
    updates = [benchephemeral, benchstateful, benchlogged, benchprogress, benchevent, benchfanout, benchtickstate],
    deletes = [benchephemeral, benchstateful, benchlogged, benchprogress, benchevent, benchfanout, benchtickstate]
)]
pub fn tx_mix_logged(
    ctx: ReducerContext,
    client_id: String,
    seq: u64,
    payload_bytes: u64,
    track_progress: bool,
) {
    mix_logged(&ctx, &client_id, seq, payload_bytes, track_progress);
}

#[reducer(
    reads = [benchephemeral, benchstateful, benchlogged, benchprogress, benchevent, benchfanout, benchtickstate],
    inserts = [benchephemeral, benchstateful, benchlogged, benchprogress, benchevent, benchfanout, benchtickstate],
    updates = [benchephemeral, benchstateful, benchlogged, benchprogress, benchevent, benchfanout, benchtickstate],
    deletes = [benchephemeral, benchstateful, benchlogged, benchprogress, benchevent, benchfanout, benchtickstate]
)]
pub fn emit_event(
    ctx: ReducerContext,
    client_id: String,
    seq: u64,
    payload_bytes: u64,
    track_progress: bool,
) {
    let _ = ctx.current.tables.benchevent().insert(BenchEvent {
        id: 0,
        client_id: client_id.clone(),
        seq,
        payload: payload(payload_bytes),
    });

    if track_progress {
        record_progress(&ctx, client_id, seq);
    }
}

#[reducer(
    on = "benchmark-workload.benchevent.insert",
    reads = [benchephemeral, benchstateful, benchlogged, benchprogress, benchevent, benchfanout, benchtickstate],
    inserts = [benchephemeral, benchstateful, benchlogged, benchprogress, benchevent, benchfanout, benchtickstate],
    updates = [benchephemeral, benchstateful, benchlogged, benchprogress, benchevent, benchfanout, benchtickstate],
    deletes = [benchephemeral, benchstateful, benchlogged, benchprogress, benchevent, benchfanout, benchtickstate]
)]
pub fn on_benchevent_insert(ctx: ReducerContext, inserted: BenchEvent) {
    ensure_fanout_counter(&ctx);

    let now = now_ms(&ctx);
    let mut counter = ctx
        .current
        .tables
        .benchfanout()
        .get(FANOUT_KEY.to_string())
        .expect("fanout counter should exist");

    counter.seen = counter.seen.saturating_add(1);
    counter.last_client_id = inserted.client_id;
    counter.last_seq = inserted.seq;
    counter.updated_ms = now;

    let _ = ctx.current.tables.benchfanout().update(counter);
}

#[reducer(
    reads = [benchephemeral, benchstateful, benchlogged, benchprogress, benchevent, benchfanout, benchtickstate],
    inserts = [benchephemeral, benchstateful, benchlogged, benchprogress, benchevent, benchfanout, benchtickstate],
    updates = [benchephemeral, benchstateful, benchlogged, benchprogress, benchevent, benchfanout, benchtickstate],
    deletes = [benchephemeral, benchstateful, benchlogged, benchprogress, benchevent, benchfanout, benchtickstate]
)]
pub fn start_tick(ctx: ReducerContext, tick_ms: u64) {
    let interval = tick_ms;
    let now = now_ms(&ctx);

    if let Some(mut state) = ctx
        .current
        .tables
        .benchtickstate()
        .get(TICK_STATE_KEY.to_string())
    {
        state.enabled = true;
        state.tick_ms = interval;
        state.last_tick_ms = now;
        let _ = ctx.current.tables.benchtickstate().update(state);
    } else {
        let _ = ctx.current.tables.benchtickstate().insert(BenchTickState {
            key: TICK_STATE_KEY.to_string(),
            enabled: true,
            tick_ms: interval,
            ticks: 0,
            last_tick_ms: now,
        });
    }

    let _ = ctx.schedule("tick", interval);
}

#[reducer(
    reads = [benchephemeral, benchstateful, benchlogged, benchprogress, benchevent, benchfanout, benchtickstate],
    inserts = [benchephemeral, benchstateful, benchlogged, benchprogress, benchevent, benchfanout, benchtickstate],
    updates = [benchephemeral, benchstateful, benchlogged, benchprogress, benchevent, benchfanout, benchtickstate],
    deletes = [benchephemeral, benchstateful, benchlogged, benchprogress, benchevent, benchfanout, benchtickstate]
)]
pub fn stop_tick(ctx: ReducerContext) {
    if let Some(mut state) = ctx
        .current
        .tables
        .benchtickstate()
        .get(TICK_STATE_KEY.to_string())
    {
        state.enabled = false;
        let _ = ctx.current.tables.benchtickstate().update(state);
    }
}

#[reducer(
    reads = [benchephemeral, benchstateful, benchlogged, benchprogress, benchevent, benchfanout, benchtickstate],
    inserts = [benchephemeral, benchstateful, benchlogged, benchprogress, benchevent, benchfanout, benchtickstate],
    updates = [benchephemeral, benchstateful, benchlogged, benchprogress, benchevent, benchfanout, benchtickstate],
    deletes = [benchephemeral, benchstateful, benchlogged, benchprogress, benchevent, benchfanout, benchtickstate]
)]
pub fn tick(ctx: ReducerContext) {
    let Some(mut state) = ctx
        .current
        .tables
        .benchtickstate()
        .get(TICK_STATE_KEY.to_string())
    else {
        return;
    };

    if !state.enabled {
        return;
    }

    state.ticks = state.ticks.saturating_add(1);
    state.last_tick_ms = now_ms(&ctx);
    let next_tick_ms = state.tick_ms;

    let _ = ctx.current.tables.benchtickstate().update(state);
    let _ = ctx.schedule("tick", next_tick_ms);
}

#[query]
pub fn health(_ctx: QueryContext) -> String {
    "ok".to_string()
}

#[query]
pub fn has_progress(ctx: QueryContext, client_id: String, min_seq: u64) -> bool {
    ctx.current
        .tables
        .benchprogress()
        .get(client_id)
        .map(|progress| progress.last_seq >= min_seq)
        .unwrap_or(false)
}

#[query]
pub fn has_committed(ctx: QueryContext, client_id: String, min_committed: u64) -> bool {
    ctx.current
        .tables
        .benchprogress()
        .get(client_id)
        .map(|progress| progress.committed >= min_committed)
        .unwrap_or(false)
}

#[query]
pub fn stats(ctx: QueryContext, client_id: String) -> BenchStats {
    let progress = ctx.current.tables.benchprogress().get(client_id);
    let fanout = ctx.current.tables.benchfanout().get(FANOUT_KEY.to_string());
    let tick = ctx
        .current
        .tables
        .benchtickstate()
        .get(TICK_STATE_KEY.to_string());

    BenchStats {
        ephemeral_rows: ctx.current.tables.benchephemeral().scan().len() as u64,
        stateful_rows: ctx.current.tables.benchstateful().scan().len() as u64,
        logged_rows: ctx.current.tables.benchlogged().scan().len() as u64,
        progress_rows: ctx.current.tables.benchprogress().scan().len() as u64,
        fanout_seen: fanout.map(|row| row.seen).unwrap_or(0),
        tick_enabled: tick.as_ref().map(|row| row.enabled).unwrap_or(false),
        tick_interval_ms: tick.as_ref().map(|row| row.tick_ms).unwrap_or(0),
        tick_count: tick.as_ref().map(|row| row.ticks).unwrap_or(0),
        last_seq: progress.as_ref().map(|row| row.last_seq).unwrap_or(0),
        committed: progress.as_ref().map(|row| row.committed).unwrap_or(0),
    }
}

#[query]
pub fn global_counts(ctx: QueryContext) -> BenchGlobalCounts {
    BenchGlobalCounts {
        ephemeral_rows: ctx.current.tables.benchephemeral().scan().len() as u64,
        stateful_rows: ctx.current.tables.benchstateful().scan().len() as u64,
        logged_rows: ctx.current.tables.benchlogged().scan().len() as u64,
        progress_rows: ctx.current.tables.benchprogress().scan().len() as u64,
        event_rows: ctx.current.tables.benchevent().scan().len() as u64,
    }
}

#[query]
pub fn total_committed(ctx: QueryContext) -> u64 {
    ctx.current
        .tables
        .benchprogress()
        .scan()
        .iter()
        .map(|row| row.committed)
        .sum()
}

fn mix_ephemeral(
    ctx: &ReducerContext,
    client_id: &str,
    seq: u64,
    payload_bytes: u64,
    track_progress: bool,
) {
    match seq % 10 {
        0 => delete_ephemeral(ctx, client_id, seq, track_progress),
        1 | 2 | 3 => update_ephemeral(ctx, client_id, seq, payload_bytes, track_progress),
        _ => insert_ephemeral(ctx, client_id, seq, payload_bytes, track_progress),
    }
}

fn mix_stateful(
    ctx: &ReducerContext,
    client_id: &str,
    seq: u64,
    payload_bytes: u64,
    track_progress: bool,
) {
    match seq % 10 {
        0 => delete_stateful(ctx, client_id, seq, track_progress),
        1 | 2 | 3 => update_stateful(ctx, client_id, seq, payload_bytes, track_progress),
        _ => insert_stateful(ctx, client_id, seq, payload_bytes, track_progress),
    }
}

fn mix_logged(
    ctx: &ReducerContext,
    client_id: &str,
    seq: u64,
    payload_bytes: u64,
    track_progress: bool,
) {
    match seq % 10 {
        0 => delete_logged(ctx, client_id, seq, track_progress),
        1 | 2 | 3 => update_logged(ctx, client_id, seq, payload_bytes, track_progress),
        _ => insert_logged(ctx, client_id, seq, payload_bytes, track_progress),
    }
}

fn insert_ephemeral(
    ctx: &ReducerContext,
    client_id: &str,
    seq: u64,
    payload_bytes: u64,
    track_progress: bool,
) {
    let _ = ctx.current.tables.benchephemeral().insert(BenchEphemeral {
        key: key(client_id, seq),
        client_id: client_id.to_string(),
        seq,
        payload: payload(payload_bytes),
        committed_ms: now_ms(ctx),
    });

    if track_progress {
        record_progress(ctx, client_id.to_string(), seq);
    }
}

fn insert_stateful(
    ctx: &ReducerContext,
    client_id: &str,
    seq: u64,
    payload_bytes: u64,
    track_progress: bool,
) {
    let _ = ctx.current.tables.benchstateful().insert(BenchStateful {
        key: key(client_id, seq),
        client_id: client_id.to_string(),
        seq,
        payload: payload(payload_bytes),
        committed_ms: now_ms(ctx),
    });

    if track_progress {
        record_progress(ctx, client_id.to_string(), seq);
    }
}

fn insert_logged(
    ctx: &ReducerContext,
    client_id: &str,
    seq: u64,
    payload_bytes: u64,
    track_progress: bool,
) {
    let _ = ctx.current.tables.benchlogged().insert(BenchLogged {
        key: key(client_id, seq),
        client_id: client_id.to_string(),
        seq,
        payload: payload(payload_bytes),
        committed_ms: now_ms(ctx),
    });

    if track_progress {
        record_progress(ctx, client_id.to_string(), seq);
    }
}

fn update_ephemeral(
    ctx: &ReducerContext,
    client_id: &str,
    seq: u64,
    payload_bytes: u64,
    track_progress: bool,
) {
    let row_key = key(client_id, seq);

    if let Some(mut row) = ctx.current.tables.benchephemeral().get(row_key.clone()) {
        row.seq = seq;
        row.payload = payload(payload_bytes);
        row.committed_ms = now_ms(ctx);
        let _ = ctx.current.tables.benchephemeral().update(row);
    } else {
        insert_ephemeral(ctx, client_id, seq, payload_bytes, false);
    }

    if track_progress {
        record_progress(ctx, client_id.to_string(), seq);
    }
}

fn update_stateful(
    ctx: &ReducerContext,
    client_id: &str,
    seq: u64,
    payload_bytes: u64,
    track_progress: bool,
) {
    let row_key = key(client_id, seq);

    if let Some(mut row) = ctx.current.tables.benchstateful().get(row_key.clone()) {
        row.seq = seq;
        row.payload = payload(payload_bytes);
        row.committed_ms = now_ms(ctx);
        let _ = ctx.current.tables.benchstateful().update(row);
    } else {
        insert_stateful(ctx, client_id, seq, payload_bytes, false);
    }

    if track_progress {
        record_progress(ctx, client_id.to_string(), seq);
    }
}

fn update_logged(
    ctx: &ReducerContext,
    client_id: &str,
    seq: u64,
    payload_bytes: u64,
    track_progress: bool,
) {
    let row_key = key(client_id, seq);

    if let Some(mut row) = ctx.current.tables.benchlogged().get(row_key.clone()) {
        row.seq = seq;
        row.payload = payload(payload_bytes);
        row.committed_ms = now_ms(ctx);
        let _ = ctx.current.tables.benchlogged().update(row);
    } else {
        insert_logged(ctx, client_id, seq, payload_bytes, false);
    }

    if track_progress {
        record_progress(ctx, client_id.to_string(), seq);
    }
}

fn delete_ephemeral(ctx: &ReducerContext, client_id: &str, seq: u64, track_progress: bool) {
    let _ = ctx
        .current
        .tables
        .benchephemeral()
        .delete(key(client_id, seq));

    if track_progress {
        record_progress(ctx, client_id.to_string(), seq);
    }
}

fn delete_stateful(ctx: &ReducerContext, client_id: &str, seq: u64, track_progress: bool) {
    let _ = ctx
        .current
        .tables
        .benchstateful()
        .delete(key(client_id, seq));

    if track_progress {
        record_progress(ctx, client_id.to_string(), seq);
    }
}

fn delete_logged(ctx: &ReducerContext, client_id: &str, seq: u64, track_progress: bool) {
    let _ = ctx.current.tables.benchlogged().delete(key(client_id, seq));

    if track_progress {
        record_progress(ctx, client_id.to_string(), seq);
    }
}

fn ensure_fanout_counter(ctx: &ReducerContext) {
    if ctx
        .current
        .tables
        .benchfanout()
        .get(FANOUT_KEY.to_string())
        .is_none()
    {
        let _ = ctx.current.tables.benchfanout().insert(BenchFanout {
            key: FANOUT_KEY.to_string(),
            seen: 0,
            last_client_id: String::new(),
            last_seq: 0,
            updated_ms: 0,
        });
    }
}

fn ensure_tick_state(ctx: &ReducerContext) {
    if ctx
        .current
        .tables
        .benchtickstate()
        .get(TICK_STATE_KEY.to_string())
        .is_none()
    {
        let _ = ctx.current.tables.benchtickstate().insert(BenchTickState {
            key: TICK_STATE_KEY.to_string(),
            enabled: false,
            tick_ms: 0,
            ticks: 0,
            last_tick_ms: 0,
        });
    }
}

fn record_progress(ctx: &ReducerContext, client_id: String, seq: u64) {
    let now = now_ms(ctx);

    if let Some(mut progress) = ctx.current.tables.benchprogress().get(client_id.clone()) {
        progress.last_seq = progress.last_seq.max(seq);
        progress.committed = progress.committed.saturating_add(1);
        progress.updated_ms = now;
        let _ = ctx.current.tables.benchprogress().update(progress);
    } else {
        let _ = ctx.current.tables.benchprogress().insert(BenchProgress {
            client_id,
            last_seq: seq,
            committed: 1,
            updated_ms: now,
        });
    }
}

fn key(client_id: &str, seq: u64) -> String {
    format!("{}:{}", client_id, seq % MAX_BENCH_ROWS)
}

fn payload(payload_bytes: u64) -> String {
    "x".repeat(payload_bytes as usize)
}

fn now_ms(ctx: &ReducerContext) -> u64 {
    ctx.time_now_ms().unwrap_or(0)
}
