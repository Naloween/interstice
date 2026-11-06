# Changelog

## Unreleased (0.6.0)

Everything since 0.5.0 (2026-06-05). Module ABI 16, loads modules built for
ABI 15 and up.

### Engine
- **Isolation and ordering** (DESIGN §6.6):
  - reducer calls are deferred messages released on commit;
  - events are emitted in commit order;
  - queries read consistent snapshots;
  - blob scopes.
- **Deterministic transactions**: a clock and a seed per transaction.
- **Time travel on logged tables**: the log is the history (`commits`,
  `rows_at`, `restore`).
- **Event tables**: `#[table(event = N)]`.
- **Schema versioning and automatic migrations**, SpacetimeDB-style.
- **Views**: filter and function views, per viewer or anonymous. Private tables
  are closed to other modules.
- **Scheduler**:
  - per-key wait queues, instead of O(n²) admission;
  - workers complete their own jobs and run the job that releases.
- **Persistence**:
  - one log write per commit;
  - stateful rows and their directories are synced;
  - `auto_inc` counters persist, so ids are never reused.
- **Phase profiler**: `INTERSTICE_PROFILE=1`.
- **Scans encode rows without copying them.**

### Security and distribution
- **Kernel and module manager**: the kernel enforces, and the holder of the
  Module authority decides on loads and grants.
- **Call visibility**: `public`, `local` and `internal`.
- **Owners**, and nodes listen on localhost by default.
- **Key identities and mutual TLS 1.3**; peers are admitted by the
  `connect` handler.
- **Bundles**: a whole node in one `.itb` file, or as a program.

### Protocol, clients and CLI
- **Reducer results**: a network caller can ask for the commit or the error.
- **Query errors** carry their request id, and remote queries time out after
  30 s.
- **`interstice-client`**, a new crate: pipelined reducer calls with results,
  queries, schemas and module administration.
- **Headless embedding**: `Node::start_headless` returns a `NodeHandle`.
- **CLI**:
  - `call_reducer` and `call_query` type their arguments from the schema and
    print the call's error message;
  - `interstice init` uses the matching SDK version;
  - errors print as `error: …`.

### SDK
- **Capability shorthands**: `CanAll<T>`, tuple rows, `caps!`.
- **Reads return `Result`**, and reducers and queries may return `Result`;
  the caller gets the `Err` message (ABI 16).
- **Compile-time types**: types are checked at compile time, and an
  unsupported type is an error at the type itself.
- **Named-field enum variants** keep their field names.
- **`.sync` subscriptions** compile.
- **`macro_rules!`**: types passed through a macro are accepted.
- **Call-scoped `Blob` handles** for bulk data.

### Examples
- **Desktop**: compositor, launcher, services, notes with time travel.
- **Browser**: Blitz, a JavaScript runtime and tests.
- **Default network module**: TCP, DNS and HTTP.
- **Brand**: the brand board's look, applied everywhere.

### Tooling
- **Benchmarks** (`benchmarks/README.md`): SpacetimeDB's micro matrix and the
  keynote transfer load.
- **CI**: tests, the benchmark comparison workflow, and browser tests on Xvfb.
- **Scripts** moved to `scripts/`.
