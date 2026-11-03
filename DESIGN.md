# Interstice — Design Document

This is the reference for **what Interstice is and how it is meant to work**.
`README.md` is the usage guide. This file describes the model, the guarantees
the engine gives (or is meant to give), and where the implementation currently
falls short. Each section separates **Design**, meaning the intended contract,
from **Status**, meaning what the code does today and where it differs.

Paths are relative to the repository root. `core/` means `crates/interstice-core/src/`.

---

## 1. Philosophy

Interstice applies the SpacetimeDB model to a whole machine, clients included:

1. **All data lives in tables.** Durable state, transient state, I/O results and
   UI state are all rows in tables owned by modules. The tables are the program's
   state; nothing else is.
2. **Code is a set of pure-ish functions over tables.**
   - **Reducers** read and write tables.
   - **Queries** only read them.
   - Reducers run either because something called them or because a table event
     they subscribe to fired.
3. **Access is declared, so the engine can parallelise.** Every function states
   which tables it reads, inserts, updates and deletes. The engine uses those
   declarations to run non-conflicting work in parallel, and it enforces them.
   Module code is written at a high level and never deals with threads, locks or
   scheduling.
4. **Machine I/O is an authority.**
   - GPU, input, audio, files, networking and module management are exclusive
     *authorities*, each held by exactly one module per node.
   - That module wraps the raw host API and exposes it as tables and reducers.
   - Other modules talk to hardware only through those tables and reducers. This
     is what lets Interstice serve clients (windows, input, rendering) and not
     just servers.
5. **Nodes form a network.** Modules can call reducers and queries on other
   nodes, subscribe to remote table events and replicate remote public tables
   locally.

**Corollary: module memory is not state.** A WASM instance's linear memory
(statics, `thread_local!`, heap) is an implementation detail of a single call:
- The engine may run a module's reducers on different instances, in parallel,
  and may recreate instances at any time.
- Anything that must survive past one call belongs in a table.
- See §11.1 for the open question of engine-managed caches.

---

## 2. Concepts at a glance

| Concept | What it is |
|---|---|
| **Node** | A running Interstice process. It has a UUID identity, a TCP listener and a data directory. It hosts modules. |
| **Module** | A WASM component. It declares a schema: tables, reducers, queries, subscriptions, authorities, dependencies and replicated tables. |
| **Table** | A typed collection of rows with a primary key, optional indexes, a persistence mode and a visibility. Each table is owned by one module. |
| **Reducer** | A transactional write function. It is triggered by a call, a subscription event or a schedule. |
| **Query** | A read-only function that returns a value. |
| **Subscription** | Binds a reducer to an event: a table insert, update or delete (local or remote), a lifecycle event or an authority event. |
| **Authority** | An exclusive right to a host capability: Gpu, Input, Audio, File, Module or Network. |
| **Replica** | A local, read-only mirror of a public table on another node. |

---

## 3. Tables

### 3.1 Schema

Declared with `#[table]` on a struct (`interstice-sdk-macros/src/table.rs`):

```rust
#[table(public, stateful)]
pub struct Tab {
    #[primary_key(auto_inc)] id: u64,
    #[index(btree)] order: u32,
    #[index(hash, unique)] url: String,
}
```

- **Name.** The struct name is lowercased: `MyTable` becomes `mytable`, not `my_table`.
- **Primary key.**
  - Every table has exactly one primary key.
  - `auto_inc` is allowed only on integer types.
- **Indexes.** An index is `hash` or `btree`, optionally `unique`, and optionally `auto_inc` (which implies unique).
  - Float fields cannot be index keys.
  - Indexes live only in memory and are rebuilt on load.
- **Visibility.**
  - `public`: other modules and other nodes may read the table, subscribe to it and replicate it.
  - `private` (the default): only the owning module may use it.
- **Persistence.** `logged` (the default), `stateful` or `ephemeral`. See §3.2.

### 3.2 Persistence modes

| Mode | Intended use | Storage |
|---|---|---|
| **Logged** | Authoritative data whose history matters, the standard SpacetimeDB model | Append-only WAL (`logs/<table>.log`), with a snapshot every 256 ops (`snapshots/<table>.snap`) after which the log is compacted |
| **Stateful** | Durable current state where history is irrelevant (settings, open tabs) | One file per row: `stateful/<table>/<hex(pk)>.row` |
| **Ephemeral** | Runtime-only state (input state, in-flight requests, frame data) | Memory only; empty after every load |

Everything on disk is postcard-encoded. A node's data directory looks like this:

```
<data_dir>/nodes/<uuid>/
  node.log, peer_tokens.toml
  modules/<module>/
    module.wasm, state.toml (loaded = bool)
    logs/<table>.log  snapshots/<table>.snap  stateful/<table>/*.row
```

**On restore** (a module reload):
- logged tables load the snapshot and then replay WAL entries newer than it;
- stateful tables read every row file;
- ephemeral tables start empty.

**Status:**
- **Durability is asynchronous.** Commits are acknowledged before they reach the
  disk. A background thread fdatasyncs the WAL and flushes stateful rows every
  10 ms, so a crash can lose about 10 ms of commits.
- **Stateful row files are written and renamed but never fsynced.**
- **`auto_inc` counters are not persisted.** On restore they are recomputed from
  the largest key present, so the ID of a deleted maximum row can be handed out
  again after a restart.
- **No schema migration.** A changed table schema is not checked against the
  persisted data.
- **Dead code.** `PersistenceConfig` and `LogRotator` are never used.

---

## 4. Functions

### 4.1 Reducers

```rust
#[reducer(on = "input.keystate.insert")]
fn on_key<Caps>(ctx: ReducerContext<Caps>, row: KeyState)
where Caps: CanRead<Tab> + CanUpdate<Tab> + CanInsert<NavRequest> { ... }
```

**Rules:**
- The first argument is the context.
- The return type is `()`.
- The only attribute allowed is `on = "..."`.

**Context forms:**
- **Plain `ReducerContext`:** declares no table access.
- **Generic `ReducerContext<Caps>`:** must carry at least one `Can*` bound. The
  bounds name Rust row types, so a misspelled table is a compile error.

**Design: a reducer is a transaction.**
- Its writes either all commit or none do.
- It aborts by panicking or trapping, and nothing is committed.
- Committed writes produce table events after commit.

**Status** (`core/runtime/reducer.rs`, `transaction.rs`):
- Writes (insert, update, delete, clear) are buffered in the call frame and
  applied in one batch under the module's table lock after the WASM call
  returns. A trap discards the batch.
- **Read-your-writes.** Each call frame keeps a per-table `TableOverlay`
  (`core/runtime/table/overlay.rs`) holding the pending state of every primary
  key the reducer touched. `get`, `scan` and index scans read committed rows
  merged with that overlay, so a row inserted earlier in the reducer is visible
  to a later read. Writes (primary key + unique indexes) are validated against
  the same merged view.
- **Commit is all-or-nothing.** The overlay records each table's `version()`
  at first touch. At commit, under the module's table lock, a table whose
  version moved (someone else committed to it meanwhile) triggers a
  re-validation of the whole batch against current committed state *before*
  anything is applied; a failure rejects the batch as a unit and bumps the
  module's `state_epoch` (§6.4).
- **Not covered:** a *query* called from a reducer runs in its own frame and
  does not see the reducer's pending writes; nested reducer calls commit
  separately (§7.1, §11.3).
- **A host write error returned to the guest can be ignored.** If the guest
  ignores it, the reducer's other writes still commit.
- **Duplicate keys no longer reach commit.** Within a reducer they are caught
  by the overlay; across two parallel Insert/Insert reducers, the second commit
  sees a moved table version, re-validates, and is rejected.
- **Auto-increment values come from atomics** and are not returned on rollback,
  which leaves gaps.

### 4.2 Queries

```rust
#[query]
fn list_apps<Caps>(ctx: QueryContext<Caps>) -> Vec<App> where Caps: CanRead<App> { ... }
```

- A query can only have `CanRead` bounds.
- The engine refuses any write from a query frame.
- Queries cannot call reducers.

**Status:**
- **One query instance per module.** Queries on the same module are serialised;
  queries on different modules run concurrently.
- **Dependency handles are reducer-only.** Generated dependency-module handles
  exist only on `ReducerContext`, so a query cannot read another module's tables
  through bindings.

### 4.3 Subscriptions (events)

`#[reducer(on = "<event>")]` accepts these event strings:

| Event string | Fires when |
|---|---|
| `table.insert`, `table.update`, `table.delete` | A table in the current module changes |
| `module.table.insert`, `module.table.update`, `module.table.delete` | A public table in another module changes |
| `node.module.table.insert`, `node.module.table.update`, `node.module.table.delete` | A remote table changes |
| `init` | The module's very first load, when it has no persisted state |
| `load` | Every load |
| `connect` / `disconnect` | A peer node or the CLI connects or disconnects |
| `input`, `render`, `audio_output`, `audio_input`, `network` | An authority event; delivered only to the module holding that authority |
| `file:<path>` / `file_recursive:<path>` | A file watch fires (File authority) |
| `module_load` / `module_remove` | A load or remove request (Module authority) |
| `sync` (replica) | A replica's initial full sync has completed |

The reducer's parameters follow from the event. Insert and delete pass the row;
update passes the old row and the new row.

**Status:**
- Events are only built when the table has at least one active subscriber.
- The 3- and 4-segment event strings are checked against the bindings TOMLs at
  compile time. The 2-segment form is not checked.

### 4.4 Scheduling

`ctx.schedule("reducer_name", delay_ms)` runs a **zero-argument** reducer of the
same module after a delay.

**Status:**
- Schedules are registered immediately, not at commit, so a reducer that
  schedules and then traps still fires the schedule.
- Schedules are not persisted.

### 4.5 Frames

There is no fixed tick.
- The GPU authority's `render` reducer runs when winit asks for a redraw.
- The winit thread waits until the whole cascade of subscribers triggered from
  that render has finished.
- The next frame happens only if a module requests a redraw.
- The graphics module exposes this to other modules through its public
  ephemeral `FrameTick` table.

---

## 5. Capabilities and access declarations

`Can*` bounds become schema data. `CanRead<T>`, `CanInsert<T>`, `CanUpdate<T>`
and `CanDelete<T>` are each turned into a `ReducerTableRef { node, module, table }`
in the reducer's `reads` / `inserts` / `updates` / `deletes` lists
(`interstice-sdk-macros/src/caps_bounds.rs`). That one declaration is used three
ways:

1. **Type safety.** The generated table handles require the matching cap trait,
   so code cannot reach an undeclared table.
2. **Runtime enforcement.** Every table host call checks the operation against
   the top call frame's declared access (`core/runtime/host_calls/table.rs`).
   Writes to another module's tables are always refused: a module can only
   *read* others' public tables, and it changes them by calling the owner's
   reducers.
3. **Scheduling.** The access sets drive parallel execution (§6).

**Validation:** at `interstice_describe` time, the module checks that its access
declarations and subscriptions refer to tables that exist and to dependencies it
has declared. A violation panics, so the module fails to load.

**Not capability-checked:** reducer calls, query calls, schedule, time and random.

---

## 6. Execution model and parallelism

### 6.1 Pipeline

```
 events (table/lifecycle/authority) ──► event loop ──► ingress queue (FIFO) ─┐
 timers, remote reducer events ─────────────────────► ingress queue ─────────┤
 network ReducerCall, render ──────────────────────────────────────────────┐ │
                                                                           ▼ ▼
                                          bounded reducer queue (8192)
                                                       │
                                               reducer-scheduler
                                       (conflict check on access sets)
                                                       │ ready
                                     worker pool: min(cores, 8) threads
                                       each takes an instance from the
                                       module's pool of 8 WASM instances
```

### 6.2 Conflict rules

Two jobs **conflict** if they touch the same table and their operations conflict:

|        | Read | Insert | Update | Delete |
|--------|------|--------|--------|--------|
| Read   | –    | ✗      | ✗      | ✗      |
| Insert | ✗    | –      | ✗      | ✗      |
| Update | ✗    | ✗      | ✗      | ✗      |
| Delete | ✗    | ✗      | ✗      | ✗      |

(– = may run in parallel, ✗ = serialised)

How jobs are admitted (`core/runtime/scheduler.rs`):
- A job starts immediately only if it conflicts with nothing that is running
  **and** nothing that is already waiting.
- When a job completes, the waiting queue is scanned in order. A waiting job is
  promoted unless it conflicts with a running job or with an earlier job that is
  still blocked.

**Guarantee:** conflicting reducers execute in arrival order, while
non-conflicting reducers run in parallel. There are no table-level locks inside
the execution; each module's tables sit behind a single mutex, held only for
individual host calls and for commit.

### 6.3 Known holes in the model (Status)

| Hole | Consequence |
|---|---|
| Nested reducer calls run inline on the caller's worker | The callee's access set is never checked by the scheduler |
| Insert/Insert runs in parallel | Correct since commit re-validation (§4.1); the loser is rejected at commit rather than serialised up front |
| Reducers with no declared access never conflict | Nothing serialises them against anything |
| Replica writes and remote queries skip the scheduler | Replica writes are applied on the event-loop thread; remote queries run on `spawn_blocking` |
| No consistent snapshot | Each host read takes the lock separately, so a function with several reads can see commits from non-conflicting jobs between them. Conflicting top-level jobs are already serialised. |
| Network `ReducerCall` and render enqueue directly into the bounded queue | Their order relative to subscription and timer jobs is not guaranteed |
| Access-set cache isn't invalidated on module reload unless explicitly cleared | The scheduler can use stale access sets |
| Event emission happens after the lock is released | Under parallelism, the event order can differ from the commit order |

### 6.4 Instances and memory

- Each module has `REDUCER_POOL_SIZE = 8` reducer instances and 1 query instance
  (`core/runtime/module.rs`), each with its own linear memory.
- **Any instance may serve any call.** Module memory is therefore not state;
  the rule in §1 stands: state goes in tables.
- **Affinity (performance only).** An idle instance remembers the reducer it
  last ran; a call prefers the most recently returned instance that last ran
  the same reducer, otherwise the least recently used one. A hot reducer such
  as a frame handler keeps landing on the same warm instance, so its
  instance caches (§11.1) stay built. Nothing may depend on this: under
  contention, or after a trap, the call goes elsewhere.
- **Trap ⇒ fresh instance.** A trapped instance is discarded and replaced by a
  freshly instantiated one (its memory may be mid-update), so a trap never
  leaks corrupted module memory into later calls.
- **`state_epoch` (ABI v7).** Every reducer context carries the module's state
  epoch. It is bumped when a reducer ran to completion but its commit was
  rejected (§4.1): every instance may have cached writes that never happened.
  The SDK's `InstanceCache` drops any value built under an older epoch.

### 6.5 Determinism

`ctx.deterministic_random_u64()` uses SplitMix64, seeded by
`(caller_node_id, module, function, kind, call_sequence)`.

**Status:** `call_sequence` is a global counter taken at execution time, so with
parallel workers the seeds are not reproducible across runs. Time is wall-clock
time. Interstice is not currently a deterministic-replay system.

---

## 7. Cross-module and cross-node interaction

### 7.1 Calls

| From → To | Semantics |
|---|---|
| Reducer → local reducer | Synchronous and inline. The callee is a **separate transaction**: it commits and emits its events before the caller commits. If the caller later traps, the callee's writes remain. |
| Reducer → remote reducer | Fire-and-forget over the network. |
| Reducer or query → local query | Synchronous, on the target module's query instance. |
| Reducer or query → remote query | Synchronous and blocking. ⚠ No timeout, and an error reply carries no request id, so the caller can hang forever. |
| Query → reducer | Refused. |

**Re-entrancy:** a call into any module already on the call stack is refused,
even when it targets a different reducer.

**Caller identity:** every context carries `caller_node_id` and `caller_module_name`.
- `caller_module_name` is empty for jobs the runtime originates (events, timers,
  render) and for network calls.
- Co-located modules share a `caller_node_id`, so `caller_module_name` is the
  routing key.

### 7.2 Dependencies and bindings

- A module that depends on another module or node has that dependency's
  **public schema** stored as TOML in `src/bindings/`. The CLI writes it with
  `interstice bindings add module|node`.
- The consumer's `build.rs` calls `generate_bindings()`, which produces typed
  handles:
  - `ctx.<module>().tables.<t>()`
  - `ctx.<module>().reducers.<r>(..)`
  - `ctx.<module>().queries.<q>(..)`
  - and the same for remote nodes under `ctx.<node>().<module>()`.
- Module dependencies are checked at load time and require an **exact version**.
- Node dependencies are `{name, address}`, and the node connects to them when
  the module loads.

### 7.3 Replicated tables

`interstice_module!(replicated_tables: ["node.module.table"])`:
- At load, the node fetches the remote schema and creates a local table
  `__replica__<node>__<module>__<table>` with the remote's schema and
  persistence.
- It subscribes to the remote table's insert, update and delete events and
  requests a full sync.
- The replica is read-only for the consuming module. Bindings route reads to
  it transparently.
- A `sync` event fires once, after the initial full sync.

### 7.4 Visibility and security

**Design:**
- Private modules and private tables are invisible and inaccessible from other
  nodes.
- Peers authenticate with trust-on-first-use tokens.

**Status:**
- `to_public()` filtering is applied only when the CLI generates bindings.
- A node's `SchemaRequest` reply contains every module, private ones included.
- Remote reducer calls, queries and subscriptions perform **no visibility
  check**; only table sync and replica setup require a public table.
- Transport is plain TCP with length-prefixed postcard frames and no
  encryption.
- `connect_to_peer` unwraps, so loading a module whose node dependency is
  offline panics. There is no reconnection.

---

## 8. Authorities and default modules

Each authority is held by at most one module per node, claimed at load time.

| Authority | Grants | Default module | Surface exposed to other modules |
|---|---|---|---|
| Gpu | ~40 wgpu-like host calls, the `render` event and window creation | `graphics` | Layers, textures, meshes, pipelines and 2D drawing reducers (`draw_rect`, `draw_text`, `draw_image`, …); public `FrameTick` and `SurfaceInfo` tables |
| Input | The `input` event | `input` | Public ephemeral tables `KeyState` (physical code + the layout's logical `key` and typed `text`), `MouseState`, `MouseButton`, `TextInputBuffer`; stateful `KeyboardLayout` (`system` default, `us`, `fr`; reducer `set_keyboard_layout`) |
| Network | TCP, UDP, host-side TLS and system DNS | `network` | Reducers `connect`, `send`, `close`, `resolve`, `http_get`; result tables `Connection`, `Inbound`, `ConnClosed`, `Resolved`, `HttpResponse`, each stamped with an `owner` |
| File | File-system host calls and file-watch events | – (claimed directly, e.g. by the browser) | – |
| Module | Load, unload and remove of modules, and routing of remote load/remove requests | `module_manager` | `App` table and app lifecycle reducers |
| Audio | Output and input streams | `audio` (stub) | – |

`crates/interstice-ui` is a library that modules link against (`ui_subsystem!`);
it is not a default module.

**Status:**
- **Authority leak on failed load.** Authorities are registered before the
  other load checks run and are not released if a later check fails.
- **Network result tables use the default `logged` persistence**, so inbound
  traffic is persisted. They should be `ephemeral`.
- **`module_manager` drops requests.** Its `module_load` and `module_remove`
  hooks are no-ops, so remote load and remove requests are silently dropped
  while it is loaded.
- **Without a Module holder, `load` of an existing module removes it**, which
  deletes its data, and then loads the new one.
- **Unload is never routed through the authority.**
- **File watchers are never removed.**
- **Keyboard layout comes from the host.** The engine forwards winit's
  `logical_key` and `text` with each `InputEvent::Key` (empty when it can't
  tell); with `KeyboardLayout = "system"` the input module trusts them, and
  only uses its built-in tables when they are empty or a layout is forced.
  A different host may resolve layouts differently (or not at all), so this
  part of the ABI may still change. Dead keys / IME composition are not
  forwarded.

---

## 9. Module lifecycle

1. **Build.**
   - `interstice_module!` exports `interstice_describe`, `alloc` and `dealloc`,
     plus one wrapper per reducer and query.
   - Registration uses `.init_array` statics.
2. **Load** (`Runtime::load_module`), in this order:
   1. ABI version check
   2. Claim authorities
   3. Name uniqueness check
   4. Module-dependency version check
   5. Connect to node dependencies
   6. Configure remote subscriptions and replicas
   7. Write `module.wasm` and `state.toml`
   8. Restore tables (on reload)
   9. Set up file watches
   10. Emit `init` (first load only), then emit `load`
3. **Unload.** Tear down subscriptions and replicas, release authorities and set
   `loaded = false`. The data is kept.
4. **Remove.** Unload, then delete the module directory.
5. **Node start.** Loads every persisted module with `loaded = true`, in
   dependency order.

---

## 10. ABI, SDK and CLI

**ABI** (`crates/interstice-abi`, `ABI_VERSION = 8`):
- Everything is postcard-encoded.
- Byte vectors travel as `IntersticeValue::Bytes(Vec<u8>)` (v8), a single
  length + raw bytes, not one `U8` value per byte. It validates as
  `Vec(U8)`, and `Vec<u8>` converts to and from both forms, so old stored
  values still load. A 4 MB texture upload took ~250 ms before and ~12 ms after.
- Pointer and length are packed into an `i64` (ptr high, len low).

| Item | Detail |
|---|---|
| Guest exports | `interstice_describe() -> i64`, `alloc`, `dealloc`, `__interstice_reducer_wrapper_<name>(ptr, len)`, `__interstice_query_wrapper_<name>(ptr, len) -> i64` |
| Host import `interstice_host_call` | One generic import carrying a `HostCall` enum: CallReducer, Schedule, CallQuery, TableScan, TableGetByPrimaryKey, TableIndexScan, Gpu, Audio, File, Module, Network, CurrentNodeId |
| Host imports, fast path | `interstice_log`, `interstice_time`, `interstice_random`, `interstice_insert_row`, `interstice_update_row`, `interstice_delete_row`, `interstice_clear_table`, `interstice_get_by_pk` |
| Arguments in | `(RawReducerContext or RawQueryContext, IntersticeValue::Vec(args))` |
| Types | `IntersticeType`: Void, U8, U32, U64, I32, I64, F32, F64, Bool, String, Vec, Option, Tuple, Named. Structs and enums appear as `Named` with type definitions in the schema. |

**SDK** (`interstice-sdk`, `-sdk-core`, `-sdk-macros`):
- Attribute macros: `#[table]`, `#[reducer]`, `#[query]`, `#[interstice_type]`, `#[init]`.
- Module macro: `interstice_module!(visibility, authorities, replicated_tables)`.
- Context helpers: `log`, `time_now_ms`, `deterministic_random_u64`,
  `current_node_id`, `schedule`, and authority accessors such as `ctx.gpu()`.
- Instance caches: `instance_cache! { static X: T; }` declares a per-instance
  `InstanceCache<T>` (`with`, `with_valid`, `take`/`put`, `invalidate`) that is
  dropped automatically on a `state_epoch` change (§6.4, §11.1).

**CLI** (`crates/interstice-cli`):
- Node commands: `node create|add|list|start|schema|…`.
- Modules: `load` builds the module and sends it to the node; `unload` and
  `remove`.
- Calls: `call_reducer` and `call_query`.
- `bindings add module|node`.
- `example <name>` starts a fresh node with embedded default and example WASM
  files; `build_modules.bash` refreshes those.

### 10.1 SDK bugs to fix

- **Unsupported argument types fail late.** Argument and return types are parsed
  from strings at describe time:
  - `u16`, `usize` and `i8` silently become `Named(..)`;
  - paths and references panic.
- **Enum field names are lost.** Enum variants with named fields lose their
  field names in bindings.
- **Read methods panic.** Generated table read methods `expect` on host errors.
- **`sync` arm likely fails to compile.** The `sync` subscription arm in
  `reducer/subscription.rs` calls `to_string` on a token stream of a type with
  no `Display`.

---

## 11. Open design questions

### 11.1 Module-local caches vs. "everything in tables"

**Problem.** Some modules have large in-memory structures that are expensive to
rebuild from rows on every call: a browser's parsed DOM and layout tree, a
physics broadphase, a font atlas. Today the only correct option is to
re-materialise them from tables on each call. Keeping them in module memory
"works" only by accident, because the instance pool is not part of any contract.

**Options considered:**

**A. Explicit derived caches.** Allow module memory as a cache *derived from*
tables, never a source of truth.
- The engine guarantees nothing except that a cache may disappear at any time.
- The module tags the cache with the table versions it was built from (for
  example, the max seq of a logged table). On each call it checks those versions
  and rebuilds or catches up by replaying newer rows.
- Correctness never depends on the cache.
- Cost: work is duplicated across instances, and every instance warms up its own
  copy.

**B. Owner-resident tables (lazy materialisation).**
- Tables stay the only abstraction, but a table's *primary copy* may live in the
  owner module's memory, as native Rust structures.
- The engine pulls a copy, or syncs a delta, only when it actually needs one:
  - another module reads it or subscribes to it;
  - it is replicated to a remote node;
  - persistence flushes it (logged or stateful).
- This removes the host-call and serialisation round-trip for private hot
  tables.

What B requires:
- **Single writer, or instance affinity.** If the data lives in one instance's
  memory, every reducer touching that table must run on that instance. That
  serialises those reducers, which the conflict rules already do for writers,
  but it also pins *readers* to one instance, giving up parallel reads.
- **Change capture.** The engine must still see the writes, for events,
  persistence and replication. Either the module emits a change log (row-level
  diffs) at commit, or the engine diffs on demand.
  - If the module emits a change log, it is effectively a table with a smarter
    storage backend inside the guest.
  - Diffing on demand needs a guest-side table ABI.
- **Rollback.** A trap must undo in-memory mutations. That needs a
  copy-on-write or undo log inside the guest, which is where most of the
  complexity lives.
- **Trust.** The engine can no longer enforce declared access on reads served
  from guest memory. That is fine for private tables only.
- **Persistence ordering.** Durable modes need the change log at commit time,
  not lazily, so the lazy part only helps *ephemeral* (and possibly stateful)
  tables.

**C. A new persistence kind, `local` / `instance`.**
- A private, non-durable, non-shareable table whose storage lives in the
  module's own memory, with the engine's guarantees reduced accordingly.
- It is declared in the schema, so the scheduler knows that reducers touching it
  need instance affinity.

**Recommendation (to be decided):**
1. **First, guarantee A.** Document that module memory may be used only as a
   cache that is rebuildable from tables. Have the SDK offer a small helper
   (`cached(|ctx| build(..), versions)`) so this is easy and correct.
2. **Then, as an optimisation, implement B restricted to private ephemeral
   tables** through instance affinity. Equivalently, implement C as a schema
   flag that the engine is free to implement either way.
   - This is safe: no durability, no cross-module visibility, and the conflict
     rules already serialise writers.
   - The engine materialises the table only if something (debug tooling, a
     future visibility change) asks for it.
3. **Extending B to public or durable tables** should wait until commit
   atomicity (§4.1) and read-your-writes are fixed, because B relies on both.

**Decision (implemented):** option A, made easy and made fast.
- `InstanceCache<T>` in the SDK (`sdk-core/src/cache.rs`) is the only blessed
  use of module memory: per instance, rebuilt on demand, dropped on a
  `state_epoch` change, with a caller-supplied `valid` check against the
  tables for staleness.
- Pool affinity (§6.4) makes the duplicated-work cost of A mostly theoretical
  for hot reducers: the browser's frame reducer parses each document exactly
  once in practice.
- B and C stay on the table as optimisations, not semantics. They are worth
  revisiting only if host-call/serialisation overhead on private hot tables
  shows up in benchmarks.

How a module uses it (the browser, §12): the cache stores *derived* objects
plus the table version they reflect (e.g. "last `DocEvent.seq` applied");
on each call it compares that with the rows, catches up by replaying newer
rows, or rebuilds. Writes always go to tables first.

### 11.2 Read-your-writes inside a reducer

**Done** (§4.1): reads inside a reducer see its own pending writes, and commit
is atomic. Remaining gap: queries called from a reducer do not see the
caller's pending writes (they run in a separate frame).

### 11.3 Atomic multi-module transactions

A nested reducer call commits independently of its caller. The open choice is
between two models:
- **Nested transaction:** the callee's writes join the caller's commit and need
  combined access sets for scheduling.
- **Message passing:** document the current behaviour as asynchronous-style
  semantics.

### 11.4 Determinism and replay

The engine could make execution reproducible by:
- deriving random seeds from a per-transaction id assigned at admission;
- providing time as a per-transaction timestamp.

That would allow replay, auditing, and logged-table reconstruction by
re-running reducers.

### 11.5 Schema evolution

Today nothing checks persisted rows against a new table schema. A migration
story is needed: versioned table schemas plus a `migrate` hook or automatic
additive changes.

---

## 12. Example: designing a module the Interstice way (browser)

`modules/examples/browser` is built exactly this way (`src/model.rs`):

| Table | Kind | Role |
|---|---|---|
| `Session` | stateful | singleton: active tab, id counters, homepage, resource-cache accounting |
| `Tab` | stateful | open tabs + history; restored (and reloaded) after a restart |
| `Ui` | ephemeral | singleton: pointer, buttons/modifiers, focus, URL-field text, texture ids |
| `Document` | ephemeral | the HTML a tab shows; immutable once inserted |
| `DocEvent` | ephemeral | input, script ticks and fetch results applied to a document, in order; compacted past 2048 rows |
| `PendingInput` | ephemeral | raw key/button input queued by the input reducers |
| `Request` | ephemeral | HTTP requests (main frame, sub-resource, favicon), sent or not yet sent |
| `Resource` / `ResourceMeta` | ephemeral | sub-resource cache, FIFO-evicted past 64 MB |
| `WebStorage` | stateful | `localStorage`, per origin |
| `ScriptedInput` | ephemeral | headless test input (`browser_script` file) |

Reducers:
- **Input reducers** only insert `PendingInput` rows.
- **`on_http`** (subscription to `network.HttpResponse`) matches the row to its
  `Request`, then commits a `Document` (main frame) or stores a `Resource`.
  It never parses HTML.
- **`on_frame`** (frame tick) loads `Session`/`Ui`/`Tab` rows, makes sure the
  active document is built (instance cache), applies pending input, lets
  documents request sub-resources (served from `Resource` or turned into
  `Request` rows), paints what changed, sends unsent `Request`s to the
  network broker, and writes back the rows that changed.

Instance caches (`src/cache.rs`): the parsed Blitz document per `Document`,
plus the chrome (toolbar) document.
- A document cache is built from its `Document` row and the `Resource` table,
  then caught up by replaying `DocEvent`s newer than its own `ev_seq`, up to
  `Tab.doc_seq`. Live input is logged first, then applied through the same
  function as replay, so both produce the same state. Side effects
  (navigations) are taken only from live input.
- The chrome is an idempotent function of the rows: the cache keeps the view
  it last applied and only pushes differences.
- Paint identity: each cache gets a `stamp`; `Ui` records which stamp each
  texture shows, so a different instance (or a rebuilt cache) repaints.
- Damage repaint: the cache keeps the last recorded display list; a new
  frame is diffed against it (common prefix/suffix) and only the bounding box
  of the changed commands is rasterised and uploaded (`update_texture` with a
  sub-rect). An identical list skips raster and upload entirely.
- JavaScript (`src/js/`, Boa): one realm per document cache, booted when the
  cache is built. Anything nondeterministic enters through the log: timer and
  animation-frame clocks as `EV_TICK`, fetch/XHR results as `EV_FETCH`. Side
  effects (navigation, network requests, `localStorage` writes) are taken only
  from live input, never during replay.
- Compaction: past 2048 `DocEvent`s a document's log is replaced by a single
  `EV_SCROLL_TO`; a cache rebuilt afterwards reloads the page at that scroll
  position, losing form and script state, the way a browser treats a
  discarded tab.
- Tests: `modules/examples/browser/tests/headless.sh` writes a `data:` page to
  `browser_home` and a `<frame> <command>` script to `browser_script`
  (`click`, `move`, `wheel`, `key Control+KeyL`, `type`, `keyas KeyQ a a`,
  `dump`, `js EXPR`), runs the example and checks the logged `browser-test`
  state lines (scroll, focus, form values, URL bar, tabs, script events,
  keyboard layouts).
- Keyboard: the physical code becomes the DOM `code`, the input module's
  logical key the DOM `key`, its `text` the typed text (both are logged in
  `DocEvent` so replay doesn't depend on the current layout). Ctrl
  shortcuts match the logical letter (Ctrl+W is the key labelled W), digits
  and named keys the physical key.
- Blitz is vendored (`vendor/blitz-dom`) for one layout fix: a table's
  specified width is a minimum (CSS 2.1 §17.5.2), so content wider than it
  grows the table instead of overflowing (Wikipedia infoboxes). Grep
  "interstice patch".

Known limitations: replay is best-effort where layout depends on resource
arrival order; there is no back/forward cache (history traversal reloads);
DOM nodes detached by scripts are only freed with their document (no GC
bridge); the network module cannot reach IP literals or non-standard
ports.

---

## 13. Roadmap / TODO

What remains for a finished Interstice: an engine you can trust as a
**portable kernel** (it already runs as a normal app, so it is trivially
virtualisable), with modules on top acting as an OS (authorities as drivers,
capabilities as permissions), and examples that show how little code that
takes. Running the engine itself inside a web browser is possible in principle
but explicitly out of scope for now.

### 13.1 Engine semantics
- [ ] **View tables**: derived, read-only tables defined by a query over other
      tables, maintained by the engine. Not needed by the browser; next big
      feature after the items below.
- [ ] **Nested / cross-module transactions** (§11.3): pick nested-commit or
      documented message-passing semantics; today a callee commits first and
      survives a caller trap.
- [ ] **Queries from reducers see pending writes** (§11.2 remaining gap).
- [ ] **Scheduler sees nested calls**: the callee's access set is not checked
      (§6.3).
- [ ] **Consistent snapshot per call** (§6.3): reads in one call can observe
      commits of non-conflicting jobs between them.
- [ ] **Event order = commit order** under parallelism (§6.3).
- [ ] **First-class event tables**: notification tables with a declared
      retention window, instead of each module trimming by hand (`network`'s
      `emit!`), and a retention policy for logs such as the browser's
      `DocEvent` (compaction into a snapshot row).
- [ ] **Determinism** (§11.4): per-transaction seeds and timestamps, enabling
      replay and auditing.
- [ ] **Time travel**: timelines/branches of selected tables (README
      roadmap), building on determinism.
- [ ] **Iterator-based scans** and cheaper index access (no full `Vec`
      materialisation per scan).

### 13.2 Storage
- [ ] Persist `auto_inc` counters (ids of deleted maximum rows are reused
      after a restart, §3.2).
- [ ] fsync stateful row files; document the ~10 ms durability window or make
      it configurable.
- [ ] Schema evolution / migrations (§11.5).
- [ ] Remove dead `PersistenceConfig` / `LogRotator`.

### 13.3 Security and distribution
- [ ] Visibility and capability checks on reducer/query calls, schedule, and
      remote access (§5, §7.4).
- [ ] Authority model for the "OS" use case: which module may grant which
      authority to whom; per-user identities.
- [ ] Replica writes and remote queries through the scheduler (§6.3).
- [ ] Row-level visibility (e.g. per-app filtering of `network` results,
      which today rely on an `owner` column filtered client-side).
- [ ] Bundles: ship a node (modules + data) as one program.

### 13.4 SDK / tooling
- [ ] The remaining SDK bugs in §10.1.
- [ ] Generated read methods return `Result` instead of panicking.
- [ ] A shorthand for "full access to these tables" in reducer `where`
      clauses (the browser spells out 40 bounds per reducer).
- [ ] Benchmarks for the overlay / re-validation path and pool affinity.
- [ ] Large payloads between modules are still copied ~4 times (guest →
      host → callee guest → host GPU call): ~12 ms for a 4 MB texture.
      Candidates: a shared-buffer/handle host call for bulk data, or letting
      a module upload straight to a resource another module owns.

### 13.5 Examples
- [x] **Browser**: dead files of the old engine removed; JavaScript (Boa)
      on top of Blitz; `DocEvent` compaction; damage-region repaint;
      scroll/keyboard/chrome/script-event regression tests
      (`tests/headless.sh`).
- [ ] **Browser follow-ups**: a ~0.7 s hitch while Wikipedia's scripts
      boot; freeing script-detached DOM nodes; IP-literal / custom-port
      URLs in the network module; missing web APIs that modern apps need
      (Speedtest by Ookla: `WebSocket` to `ws://…:8080`, `Worker`,
      `performance`, `crypto`, ES-module scripts).
- [ ] **Desktop**: finish the surface-compositor plan (window manager as a
      module, apps as modules).
- [ ] Polish agar, ui, audio and graphics examples into short, readable
      showcases of one feature each.
