# Benchmarks

The suite measures the engine the way SpacetimeDB measures theirs, so the
numbers sit side by side, and it splits a call's cost into phases so that
optimisation goes to the bottleneck.

| What | Where | Run |
|---|---|---|
| Micro matrix (one transaction per sample) | `crates/interstice-bench/benches/generic.rs` | `cargo bench -p interstice-bench --bench generic [filter]` |
| Transfer load (committed calls per second) | `crates/interstice-bench/src/transfer.rs` | `interstice-bench transfer [--node <addr>] [options]` |
| Phase breakdown | `INTERSTICE_PROFILE=1` on any node | printed by the node on exit and every 10 s |
| Workloads (WASM) | `modules/benchmarks/bench` | built by `scripts/build_modules.sh` |

Build the workload module first:

```bash
cargo build -p bench --target wasm32-unknown-unknown --release
```

## 1. Micro matrix

The matrix of SpacetimeDB's `crates/bench`
(https://github.com/clockworklabs/SpacetimeDB/tree/master/crates/bench):

- **Targets**:
  - `raw`: the engine's table store, with the staging a reducer does
    (validation into an overlay, then apply). No WASM, no scheduler, no
    persistence. This is SpacetimeDB's `stdb_raw`.
  - `module`: the `bench` module on a headless node, called in process
    through `NodeHandle`. Each sample is one reducer call: queued,
    scheduled, run in WASM, committed and (on disk) logged. This is
    SpacetimeDB's `stdb_module`.
  - `sqlite`: rusqlite, in memory, or a file in WAL mode with
    `synchronous = NORMAL`.
- **Schemas**: `u32_u64_str` `{id, age, name}` and `u32_u64_u64` `{id, x, y}`.
  The columns after `id` are drawn as `xorshift % 8`, from seed `0xdeadbeef`.
- **Indexes**:
  - `unique_0`: the id is the primary key.
  - `btree_each_column`: a non-unique btree on every column. Interstice
    needs a primary key, so rows carry a hidden auto-increment one.
- **Persistence**: `mem` (ephemeral tables) and `disk` (logged tables, which
  are flushed every 10 ms, §3.2).
- **Operations**, each one transaction:

  | Operation | Applies to | Setup (not timed) | Timed |
  |---|---|---|---|
  | `empty` | module | – | an empty reducer |
  | `insert_bulk` | all | 2048 rows | 256 new rows |
  | `update_bulk` | `unique_0` | 2048 rows | 256 rows updated by id |
  | `iterate` | `unique_0` | 256 rows | a scan of all of them |
  | `filter` | `btree_each_column` | 2048 rows | an index lookup on column 3 (~256 rows) |

Criterion keeps a baseline per benchmark under `target/criterion`.
`--save-baseline <name>` before a change and `--baseline <name>` after it
reports the difference.

**Comparing with SpacetimeDB on the same machine.** Clone SpacetimeDB and
run `cargo bench -p spacetimedb-bench --bench generic -- 'stdb_module/mem'`.
Its ids have the same shape as ours (`stdb_module` → `module`,
`stdb_raw` → `raw`).

## 2. Transfer load

SpacetimeDB's keynote benchmark (`templates/keynote-2`):

- 100 000 accounts with a balance of 10⁹ each;
- `transfer(from, to, 1)` between pairs picked by a Zipf law: `--alpha 0` is
  uniform, `--alpha 1.5` puts most transfers on a few accounts;
- 64 clients with 40 calls in flight each;
- a call counts only once its `ReducerResult` says committed, at the client;
- throughput is the mean ± standard deviation of the per-second counts
  after the warmup, with p50, p95 and p99 latency;
- the total balance is checked to be unchanged at the end.

```bash
interstice-bench transfer --alpha 0 --seconds 30             # in process
interstice-bench transfer --node 127.0.0.1:8095 --alpha 1.5  # over TLS
```

Their published runs: 300 s with a 30 s warmup, on an i9-14900K. They
report about 280 000 tx/s for SpacetimeDB, about 10 000 for Postgres and
about 3 000 for SQLite.

## 3. Results

Measured on an i7-1360P laptop (4 P-cores + 8 E-cores, 16 threads), Linux
7.0, release build, on 2026-11-07 at commit `895e660` (ABI 17), with the
machine otherwise idle. Results on another machine are not comparable with
these.

### Micro matrix

Criterion medians, 256 rows per operation:

| benchmark | raw/mem | module/mem | module/disk | sqlite/mem | sqlite/disk |
|---|---:|---:|---:|---:|---:|
| `empty` | – | 8.1 µs | – | – | – |
| `insert_bulk/u32_u64_str/btree_each_column/load=2048/count=256` | 122.6 µs | 838 µs | 894 µs | 273.2 µs | 276.2 µs |
| `insert_bulk/u32_u64_str/unique_0/load=2048/count=256` | 50.9 µs | 629 µs | 764 µs | 65.7 µs | 72.6 µs |
| `insert_bulk/u32_u64_u64/btree_each_column/load=2048/count=256` | 93.0 µs | 588 µs | 670 µs | 253.0 µs | 272.5 µs |
| `insert_bulk/u32_u64_u64/unique_0/load=2048/count=256` | 42.6 µs | 655 µs | 738 µs | 63.8 µs | 65.6 µs |
| `update_bulk/u32_u64_str/unique_0/load=2048/count=256` | 74.0 µs | 909 µs | 1.02 ms | 76.8 µs | 87.7 µs |
| `update_bulk/u32_u64_u64/unique_0/load=2048/count=256` | 50.3 µs | 811 µs | 975 µs | 75.9 µs | 82.6 µs |
| `iterate/u32_u64_str/unique_0/count=256` | 287 ns | 56.8 µs | 52.0 µs | 13.6 µs | 14.0 µs |
| `iterate/u32_u64_u64/unique_0/count=256` | 256 ns | 37.2 µs | 26.4 µs | 14.0 µs | 14.6 µs |
| `filter/u32_u64_str/btree_each_column/load=2048/buckets=8` | 444 ns | 69.1 µs | 57.6 µs | 34.6 µs | 40.5 µs |
| `filter/u32_u64_u64/btree_each_column/load=2048/buckets=8` | 419 ns | 46.8 µs | 40.3 µs | 33.7 µs | 38.4 µs |

**Reading it.**
- On the raw store, Interstice's tables match or beat SQLite in memory.
- In a module, a write costs 2.3–3.5 µs per row against 0.2–0.5 µs raw.
  It is mostly the host call per row: validation, the overlay and the index
  updates.
- A read costs 0.1–0.3 µs per row, 1.4–4× SQLite.
- Logging (`disk`) adds 10–20% to writes, and nothing to reads.

### Transfer

30 s after a 5 s warmup, 100 000 accounts, 64 clients with 40 calls in
flight each; the total balance was unchanged after every run:

| Mode | alpha | committed/s | p50 | p99 |
|---|---:|---:|---:|---:|
| in process | 0 | 142 800 ± 6 400 | 17 ms | 28 ms |
| in process | 1.5 | 141 800 ± 6 200 | 17 ms | 27 ms |
| TLS, 64 connections | 0 | 94 500 ± 11 700 | 23 ms | 57 ms |
| TLS, 64 connections | 1.5 | 99 200 ± 8 400 | 23 ms | 45 ms |

Every transfer writes the `account` table, and conflicts are detected per
table (§6.2), so transfers run one at a time, as in SpacetimeDB; alpha
doesn't change the throughput. With 2 560 calls in flight, the latency is
the queue: about 7 µs per transfer times the queue length.

### History

Same machine, module/mem and in-process transfers at alpha 0:

| Change | `insert_bulk` (str, unique) | `iterate` (str) | transfer/s |
|---|---:|---:|---:|
| 2026-11-06, first full run | 930 µs | 159 µs | 58 900 |
| One log write per commit (§3.2) | | | 83 000 |
| Workers run the job they release (§6.1) | | | 140 000 |
| ABI 17: values without names, direct encoding (§10.2) | 629 µs | 57 µs | 142 800 |

The leads, in order:
1. row-level conflicts, so that uniform transfers run in parallel;
2. the per-host-call cost of writes, for example a batched insert.
