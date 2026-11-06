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
7.0, release build. Results on another machine are not comparable with
these.

### Micro matrix (2026-11-06, commit after `ca63955`)

| benchmark | raw/mem | module/mem | module/disk | sqlite/mem | sqlite/disk |
|---|---:|---:|---:|---:|---:|
| `empty` | – | 9.1 µs | – | – | – |
| `insert_bulk/u32_u64_str/btree_each_column/load=2048/count=256` | 128.8 µs | 1.10 ms | 1.36 ms | 265.7 µs | 302.5 µs |
| `insert_bulk/u32_u64_str/unique_0/load=2048/count=256` | 59.5 µs | 929.6 µs | 1.11 ms | 74.9 µs | 76.9 µs |
| `insert_bulk/u32_u64_u64/btree_each_column/load=2048/count=256` | 106.5 µs | 990.5 µs | 1.25 ms | 246.6 µs | 266.9 µs |
| `insert_bulk/u32_u64_u64/unique_0/load=2048/count=256` | 43.2 µs | 963.1 µs | 955.6 µs | 60.6 µs | 65.6 µs |
| `update_bulk/u32_u64_str/unique_0/load=2048/count=256` | 77.9 µs | 1.59 ms | 1.14 ms | 78.9 µs | 91.1 µs |
| `update_bulk/u32_u64_u64/unique_0/load=2048/count=256` | 52.7 µs | 1.18 ms | 1.10 ms | 74.7 µs | 83.6 µs |
| `iterate/u32_u64_str/unique_0/count=256` | 284 ns | 159.1 µs | 163.3 µs | 15.0 µs | 14.2 µs |
| `iterate/u32_u64_u64/unique_0/count=256` | 263 ns | 118.6 µs | 123.1 µs | 14.3 µs | 16.0 µs |
| `filter/u32_u64_str/btree_each_column/load=2048/buckets=8` | 537 ns | 135.3 µs | 178.1 µs | 35.6 µs | 38.5 µs |
| `filter/u32_u64_u64/btree_each_column/load=2048/buckets=8` | 481 ns | 130.1 µs | 104.1 µs | 35.6 µs | 39.2 µs |

**Reading it.** On the raw store, Interstice's tables match or beat SQLite
in memory. The module column is where the time goes:
- inserting or updating 256 rows costs ~1 ms, about 3.5 µs per row against
  0.2 µs raw;
- reading 256 rows costs 120–160 µs, against 15 µs in SQLite.

So each host call and each row crossing the WASM boundary is the bottleneck,
not the tables. That is what to optimise first.

**ABI 17 (values without names, direct encoding, §10.2)**, module/mem, same
machine but busy (load 3), so ±20%:

| benchmark | before | after |
|---|---:|---:|
| `insert_bulk/u32_u64_str/unique_0` | 930 µs | 501 µs |
| `insert_bulk/u32_u64_u64/btree_each_column` | 991 µs | 650 µs |
| `update_bulk/u32_u64_str/unique_0` | 1.59 ms | 777 µs |
| `iterate/u32_u64_str/unique_0` | 159 µs | 61 µs |
| `iterate/u32_u64_u64/unique_0` | 119 µs | 37 µs |
| `filter/u32_u64_str/btree_each_column` | 135 µs | 58 µs |
| `filter/u32_u64_u64/btree_each_column` | 130 µs | 44 µs |

Writes now cost ~2 µs per row, mostly the host call per row (validation,
the overlay, the index updates); reads ~0.15–0.25 µs per row.

### Transfer (2026-11-06)

| Mode | alpha | committed/s | p50 | p99 |
|---|---:|---:|---:|---:|
| in process | 0 | 58 900 ± 2 300 | 43 ms | 61 ms |
| in process | 1.5 | 64 900 ± 6 900 | 35 ms | 91 ms |
| TLS, 64 connections | 0 | 44 500 ± 1 700 | 56 ms | 90 ms |
| TLS, 64 connections | 1.5 | 44 300 ± 2 900 | 57 ms | 87 ms |

Every transfer writes the `account` table, and conflicts are detected per
table (§6.2), so transfers run one at a time, as in SpacetimeDB. With 2 560
calls in flight, the latency is the queue: about 17 µs per transfer times
the queue length. The phase breakdown of the in-process run, per transfer:

| Phase | Mean |
|---|---:|
| waiting for a worker | 1.4 µs |
| prepare (security, frame, encoding) | 0.8 µs |
| WASM, host calls included | 4.9 µs |
| host calls (2 gets + 2 updates) | 2.7 µs |
| commit | 4.8 µs |
| persistence (logged writes) | 4.2 µs |
| reply | 1.0 µs |

**Since then:**
- One log write per commit (§3.2): in process, 83 000 ± 3 100/s
  (alpha 0), p50 29 ms; the commit fell from 4.8 to 2.2 µs.
- Workers complete their own jobs and run the job that releases (§6.1):

  | Mode | alpha | committed/s | p50 | p99 |
  |---|---:|---:|---:|---:|
  | in process | 0 | 140 000 ± 2 300 | 17 ms | 28 ms |
  | in process | 1.5 | 147 400 ± 2 100 | 17 ms | 26 ms |
  | TLS, 64 connections | 0 | 96 300 ± 7 500 | 24 ms | 50 ms |
  | TLS, 64 connections | 1.5 | 100 700 ± 7 900 | 23 ms | 45 ms |

The leads, in order:
1. ~~one log append per transaction~~ (done);
2. ~~values with names~~ (done, ABI 17);
3. row-level conflicts would let uniform transfers run in parallel;
4. the per-host-call cost of writes (see the micro matrix).

