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
- **Event tables.** `event = N` (bare `event`: 256) makes an ephemeral table
  that keeps only its `N` newest rows (`TableSchema.retention`). Subscribers
  get every row in the insert event; after each commit `Table::enforce_retention`
  evicts the oldest rows past `N`, in insertion order, without delete events.
  Eviction runs after the whole batch, so a reducer that inserts then updates
  an event row never sees it vanish mid-commit. `network`'s result tables are
  event tables.
- **Timelines.** `timeline = N` (bare `timeline`: 1024) keeps the table's last
  `N` commits as a tree, so a reducer can travel the table back and forth in
  time and branch from the past. See §6.8. Not allowed on event tables.

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
- It aborts by returning `Err` (reducers may return `Result<(), E>`), panicking
  or trapping, and nothing is committed.
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
- **Queries see pending writes.** A query called from a reducer reads the
  calling reducer's module through that reducer's overlay (§6.6).
- **Calls are part of the transaction.** Reducer calls issued by a reducer
  are deferred: they leave only if it commits, and each callee then runs as
  its own transaction (§6.6, §7.1).
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
| `module_load_result` | A local `load` this module requested has finished; receives `ModuleEvent::LoadResult{module_name, ok, message}`. `module_name` is empty when the bytes could not be compiled |
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

Schedules go into the transaction's outbox (§6.6) and are released when it
commits; the delay counts from the commit. A reducer that schedules and then
fails schedules nothing, so a self-rescheduling loop stops at its first
failed run. Only reducers may schedule; a query that tries gets
an error.

**Status:** schedules are not persisted.

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
(`interstice-sdk-macros/src/caps_bounds.rs`). Shorthands: `CanAll<T>` (all four;
a real supertrait, so helpers can use it too), tuples of rows in reducer `where`
clauses (`CanRead<(A, B)>`, expanded and de-duplicated by the macro), and the
`caps!` macro, which defines a named concrete caps struct (`all` / `read` /
`write` / `insert` / `update` / `delete` lists) taken as `ReducerContext<Name>`.
The tuple form only means something to the macro: as a helper bound it is not
implied. That one declaration is used three ways:

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
 commits (events + call batches, │
   emitted under the table lock) ┴► event loop ──► ingress queue (FIFO) ─┐
 lifecycle/authority events, replica jobs ──► event loop ──► ingress ───┤
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

The rows marked *closed* are closed by the isolation and ordering design (§6.6).

| Hole | Consequence | Status |
|---|---|---|
| Nested reducer calls run inline on the caller's worker | The callee's access set is never checked by the scheduler | *closed*: calls are deferred jobs (§6.6) |
| Insert/Insert runs in parallel | Correct since commit re-validation (§4.1); the loser is rejected at commit rather than serialised up front | by design |
| Reducers with no declared access never conflict | Harmless: such a reducer can neither read nor write a table | by design |
| Replica writes skip the scheduler | Applied on the event-loop thread, under reducers that read the replica | *closed*: replica jobs (§6.6) |
| Remote queries skip the scheduler | They run on `spawn_blocking` | *closed*: query snapshots (§6.6) make that safe |
| No consistent snapshot | Each host read takes the lock separately, so a function with several reads can see commits from non-conflicting jobs between them | *closed* for reducers and for each query (§6.6) |
| Network `ReducerCall` and render enqueue directly into the bounded queue | Their order relative to subscription and timer jobs is not guaranteed | open (backpressure for network calls; render is a lone job) |
| Access-set cache isn't invalidated on module reload unless explicitly cleared | The scheduler can use stale access sets | *closed*: cleared on load and on teardown |
| Access keys weren't normalised | A module's own `Current.t` and another module's `m.t` were different keys, so a writer and a foreign reader of one table never conflicted | *closed*: keys use the owner's name, lower-cased |
| Event emission happens after the lock is released | Under parallelism, the event order can differ from the commit order | *closed*: commit-ordered emission (§6.6) |
| `send_packet` spawns one task per packet | Two packets to the same peer may overtake each other | open (§7.4 network work) |

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

**Transaction inputs.** A reducer transaction gets a `Tx` when it starts
executing on its worker (`call_reducer`):

| Field | Value |
|---|---|
| `id` | node-wide counter |
| `timestamp_ms` | wall-clock ms, clamped so it never decreases on this node |
| `seed` | hash of (node id, module, reducer, `id`) |

Every call in the transaction sees the same `Tx`:
- `ctx.time_now_ms()` returns `timestamp_ms` for the whole transaction,
  including the queries it calls. Time does not advance inside a reducer.
- `ctx.deterministic_random_u64()` draws from one SplitMix64 stream seeded
  with `seed`. The stream lives on the root frame, so a query's draws advance
  the reducer's stream in call order.
- An outermost query (from the host, a remote node, or the app) is its own
  read-only transaction with its own `Tx`.

**The claim.** A reducer transaction is a deterministic function of:
- the module's code;
- the state of the tables it reads (rows *and* their order: scans return rows
  in storage order, which is a function of the op history);
- its arguments, caller identity and `state_epoch`;
- its `Tx`;
- its *external results*: the responses of authority host calls (GPU, audio,
  file, network, module manager) and remote queries, in call order.

Its outputs are its table writes and its outbox: local calls, remote packets
and schedules (§4.4), all released at commit.

Why reads are reproducible:
- No conflicting commit can land while it runs (§6.6), and queries pin
  their snapshot.
- Conflicting transactions start in commit order, because the scheduler
  admits a job only after every conflicting earlier job has committed. So
  along any one table's history `Tx` ids and timestamps are increasing and
  non-decreasing respectively, and commit order is a valid serial order.

Not covered by the claim:
- Instance memory. The engine cannot enforce that module memory is only a
  cache of table state (§11.1); a module that keeps authoritative state in
  memory is outside the claim.
- Guest-side nondeterminism, such as iterating a guest `HashMap` with a
  random hasher.

**Consequences for modules:**
- Measuring elapsed time *inside* one reducer gives 0. Measure across
  transactions (store a start timestamp) or read the log timestamps instead.
- A reducer that busy-waits on the clock never finishes. Use
  `ctx.schedule`.

`Tx`s and every other input of a module can be recorded and replayed
(§6.7).

### 6.6 Isolation and ordering

**One rule:** the scheduler is the isolation mechanism. Everything that writes
a table is a scheduled job, and every reducer transaction runs exactly once,
at top level, on one worker. Reads that the scheduler cannot protect (queries)
protect themselves with read locks.

**Jobs.** The ingress and the bounded queue carry `Job`:

| Job | Origin | Access set |
|---|---|---|
| `Reducer` | events, timers, render, network calls, the app | the reducer's declared set |
| `Calls` (a *call batch*) | the deferred local calls of one committed transaction | union of the callees' declared sets, plus one channel key per callee module |
| `Replica` | a remote table change for a local replica | `Update` on the replica's table, in the remote form consumers declare (`node.module.table`) |

A *channel key* `caller → callee` is a pseudo-resource that conflicts only
with itself. Two batches from the same caller module to the same callee
module are therefore serialised in arrival order. Batches from different
callers, or to different callees, still run in parallel when their tables
allow it.

**Calls are deferred messages.** This is the §11.3 decision.
- **Issuing a call records it.** `ctx.<m>().reducers.<r>(..)` validates the
  target (module and reducer exist) and appends the call to the calling
  transaction's *outbox*. It returns `Err` only if that validation fails. It
  never runs the callee and never reports the callee's own result.
- **Calls leave only if the caller commits.** If the commit succeeds, the
  outbox is released. If the caller returns `Err`, traps, or its commit is
  rejected, the outbox is dropped together with the writes.
- **One batch per commit.** The local calls of one commit form one batch,
  executed in issue order on one worker. Each callee is its own transaction:
  it commits, emits, and releases its own outbox. A failing callee is logged
  and the rest of the batch continues.
- **Remote calls are deferred too.** A remote call's blobs are inlined when it
  is issued, and it is sent at release.
- **Cycles are allowed.** Since nothing runs inline there is no re-entrancy:
  a module may call itself, and the cycle check now applies to queries only.
  An unbounded ping-pong is possible, exactly as with events.
- **What this buys.**
  - The scheduler sees every callee's access set.
  - A caller's writes and its calls are atomic together.
  - A callee sees its caller's committed writes, not a half-done state.

**Commit-ordered emission.** A commit pushes its table events and then its
outbox (as an `EventInstance::Calls` carrying the batch) into the event
channel *inside* the module's table write lock.
- The channel order is a linearisation of commit order: per module it is
  exactly commit order, and causally related commits keep their order.
- The event loop handles the channel FIFO, and the ingress forwarder is FIFO.
  So a commit's subscriber jobs reach the scheduler before its call batch,
  and both reach it before anything emitted by a later commit.
- A transaction with no writes takes no lock and emits its outbox directly. It
  has no commit to order against.

**Snapshots.**
- **Reducers.** Every reducer read targets a declared table. Every writer is
  scheduled, replicas included. So no conflicting job runs while the reducer
  executes, and its reads see one stable committed state plus its own pending
  writes (the overlay, §4.1).
- **Queries.** A query is not admitted by the scheduler, so it pins instead:
  - `Module::tables` is a `RwLock`. On a query's first read of a module, the
    outermost query frame takes a recursive read lock on that module and keeps
    it until the outermost query returns.
  - Commits take the write lock, so they wait for queries pinning their module.
  - The query sees, for every module it reads, the state as of its last pin,
    which is a consistent cut (two-phase locking on reads).
  - This holds for nested queries, for queries called from a reducer, and for
    remote queries, which stay on `spawn_blocking`.
- **Queries called from a reducer see its pending writes.** For tables of the
  calling reducer's module, reads in the query merge the root reducer frame's
  overlay (§11.2).
- **Limit.** Two separate queries in one reducer, or a query and a later read
  of the same table, are each consistent but not necessarily with each other,
  unless the reducer also declares `Read` on that table. The pins end with the
  query, not with the reducer: holding them for the reducer's whole run would
  stall every commit to e.g. `graphics` behind a 30 ms browser layout.
- **Limit.** A query that calls a query on another node keeps its pins while
  it waits for the answer, so commits to the modules it already read wait too
  (bounded by the remote-query timeout). Read remote data first, or through a
  replica, in hot paths.
- **No deadlock.**
  - A write-lock holder never waits for anything: a commit holds one module's
    write lock, applies, emits into unbounded channels and releases it.
  - Readers use `read_recursive`, which never waits while another reader holds
    the lock. A reader therefore only waits for a running writer, which
    finishes.
  - A thread drops all its pins before it commits: queries never commit, and a
    reducer's pins end with its queries.

**Blobs (BlobScope).**
- A call's blobs live in an `Arc<BlobScope>` holding a map of blobs and an
  optional parent scope. A frame resolves an id through its scope chain.
- **Reducer frame.** It creates a scope whose parent is the scope its job
  inherited: none for a top-level job, the caller's root scope for a batch
  callee.
- **Query frame.** It shares the caller's scope, so blobs a query returns
  simply stay alive for the caller. An outermost query, such as a host or
  remote request, gets a fresh scope and inlines blobs in its result as bytes.
- **Call batch.** It holds the caller's scope, so a `Blob` argument stays
  valid until the batch has run, on whatever worker that is. A callee
  forwarding a parent's blob in its own calls keeps the chain alive.
- **Memory** is freed when the last batch referencing a scope completes.

**Regression walk-through (verified against the code before implementing).**
- **Draw order.** A frame handler's `draw_*` calls form one batch, executed in
  issue order, so `Draw2DCommand` auto-inc ids ascend in call order as before.
  The next frame's batch is serialised behind it by the channel key.
- **Create, then use.** A batch is sequential, so a later call sees what an
  earlier one committed. Examples: `create_texture` then `draw_image`;
  `create_layer` then `draw_rect` (browser, agar, ui); `load_app` then
  `create_surface` / `assign_module_surface` (desktop).
- **Render-done.** A batch carries a fork of the active completion token, like
  any event, so the GPU frame still waits for the whole cascade, calls
  included.
- **Reading a callee's effect in the same reducer.** Behaviour change. No
  module does it: desktop, browser, agar, ui and http-get only issue calls,
  and their only cross-module read is the `graphics.surface_info` query.
- **Callee errors.** They are no longer returned to the caller. Every current
  call site ignores the result (`let _ =`) or only logs it. Dispatch errors
  (unknown module or reducer) are still returned.
- **Blob uploads** (browser frames, desktop icons, cursor atlas). The batch
  keeps the caller scope, the callee resolves through its parent chain, and
  `WriteTextureBlob` is unchanged.
- **Replica consumers** (the `caller` example). The replica job is enqueued
  before the subscriber jobs of the same remote event and conflicts with their
  declared `Read`, so they see the row.
- **Cost.** Per call it is the same: one callee transaction. Per level of call
  depth it adds one scheduler hop. For one commit, all its calls share a
  single job.


### 6.7 Record and replay

A module can be *recorded*: the engine keeps enough to re-execute its
transactions and check that they produce the same writes. This serves
auditing (is the state what the code produces?), debugging (re-run with
logging or fixed code) and time travel (§11.4).

**The boundary rule.** The log holds everything that *enters* the module, so
the module can be replayed alone, with no other module running.

| Reproduced by replay (not logged) | Logged |
|---|---|
| Reads and writes of its own tables | The reducer input: encoded context and arguments |
| `time_now_ms` and random draws (from the `Tx`) | The `Tx` |
| Blobs it creates; `log` | The response of every other host call its root frame makes (reducer and query calls, schedules, foreign-table reads, authority calls, current node id), with a digest of the request |
| | Blobs it read from its caller's scope |
| | Writes that reach its tables without one of its reducers (replica changes) |

Notes on the boundary:
- A query call is logged even when the target is the module itself: during
  replay nothing but the recorded module exists.
- Host calls made inside a nested query frame are part of that query, whose
  result is logged at the boundary.

**Recording** (`Module::recorder`, off by default):
- When recording starts, the module's tables are cloned as the *baseline*,
  under the table write lock. The clone keeps rows in order, indexes and
  auto_inc counters.
- Each commit into the module's tables appends a record under the same write
  lock, so log order is commit order. A record holds the origin and the
  committed writes. The origin is a reducer (name, input, `Tx`, logged
  responses, inherited blobs) or an external write (a replica change).
- Only commits are recorded; failed or rejected transactions changed nothing.
- The log is a window of at most N records (default 10 000). Evicting the
  oldest applies its writes to the baseline, so baseline + log always
  rebuilds the live state.

**Replay** runs on its own thread, against a *copy* of the module built from
its wasm (`Module::from_bytes`). The copy is never registered, so it has no
subscriptions, persistence or events. The log and the live tables are
captured together, under the read lock. Then each record is replayed in
order:
- External write: apply its writes.
- Reducer: run it with the recorded input and `Tx`. The frame is in replay
  mode: each logged host call checks its kind and request digest, then
  returns the recorded response and performs nothing (no draw, no call, no
  packet, no schedule). Its parent blob scope holds the recorded inherited
  blobs. The writes it produces are compared with the recorded ones
  (encoded bytes), then applied.
- A transaction that started before recording began (or under an earlier
  recording) has no reducer record at commit. Its commit is logged as an
  external write, which is still exact for state.
- At the end, the copy's tables are compared with the captured live tables:
  rows and their order.

The report gives the number of commits replayed and the first divergence:
the record, the reducer, and why (request mismatch, a missing or extra host
call, different writes, or a final state mismatch).

**Blob ids are per transaction:** `(tx.id << 24) | n`, where `n` counts the
transaction's blobs. They are unique because `Tx` ids are, and identical on
replay, so a request carrying a blob id has the same digest.

**Control** goes through the Module authority:
- `ModuleCall::Record { module_name, enabled }` and
  `ModuleCall::Replay { module_name }`. Replay answers `Ok`, or `Err` with
  the divergence. Both are logged to the node log.
- The `module_manager` module wraps them as `record_start(module)`,
  `record_stop(module)` and `replay(module)` reducers. From the CLI:
  `call_reducer <node> module_manager record_start <module>`, then
  `call_reducer <node> module_manager replay <module>`.
- Verified on `agar-server` (ticks with random draws and clock, plus
  joins): 316 commits replayed with state matching, and again after the
  window wrapped (baseline eviction).

**Why this causes no regressions:**
- Recording off costs one flag check per commit and one thread-local check
  per dispatched host call.
- Blob ids only need uniqueness along a scope chain, and guests treat them
  as opaque.
- Encoding reducer input in the runtime instead of the instance produces the
  same bytes.
- A read of `Other(own_module)` now resolves to the frame's module instead of
  the registry: the same module in live mode, the copy in replay.

**Status / limits:**
- The log is in memory only. It is lost on reload or restart, because the
  recorder lives on the `Module`.
- Instance memory that isn't a cache of table state shows up as a divergence.
  That is the audit working, not a bug.
- A commit that fails halfway leaves its earlier ops applied (a
  pre-existing hazard). Replay reports it as a final-state mismatch.

### 6.8 Time travel

A table declared with `timeline = N` keeps its commit history as a tree. A
reducer can move the table to any point of that tree. The next commit after
the move starts a new *branch*, and the future it left stays reachable. Uses:
undo and redo, game rewind and "ghost" effects, what-if branches.

**The tree** (`Table::timeline`, in memory):
- Each commit that changes the table adds a node: its id is the
  transaction id, plus its parent, the `Tx` timestamp, and its ops.
- An op holds before and after images: `Insert(row)`, `Update(old, new)`,
  `Delete(old)`. A clear becomes one `Delete` per row.
- Node `0` is the *base*: the oldest state still reachable. Transaction ids
  start at 1, so no real node is `0`.
- `head` is the node the table's current rows correspond to.
- Only the head's state is stored (the table itself). Any other node's state
  is computed by undoing ops from the head up to the common ancestor, then
  redoing ops down to the target.

**Points** (`TimelinePoint`), resolved against the committed head:
- `Commit(id)`: that node, or `0` for the base. An error if it isn't in the
  tree.
- `Back(n)`: `n` parents up from the head, clamped to the base.
- `Forward(n)`: `n` steps down, each to the most recently committed
  child, stopping at a leaf.
- `Time(ms)`: the deepest ancestor of the head (the head included) whose
  timestamp is `<= ms`, else the base. This is "the table as it was at `ms`
  on this branch".
- `Base`.

**Calls** (`HostCall::Timeline`, on the module's own tables only):
- `info(table)` gives the committed head and every node (id, parent,
  timestamp, op count). Needs read access.
- `rows_at(table, point)` gives the rows at a point without moving.
  Needs read access.
- `travel(table, point, mode)` moves the table, in a reducer only. It needs
  insert, update and delete access, because it writes all three.

**How a travel commits.** It is an ordinary write, so the transaction stays
atomic and everything downstream works unchanged:
1. Compute the rows at the target (above), then diff them by primary key
   against the reducer's current view (committed rows plus its earlier
   writes).
2. Stage the diff through the usual overlay: deletes, then updates, then
   inserts, each in primary-key order. Subscribers get normal
   insert/update/delete events, persistence writes the rows, replicas
   follow.
3. Stage a marker, `Transaction::Travel { table, to, truncate }`. It changes
   no rows.
4. At commit, the table's ops are collected per table. A marker discards the
   ops collected before it for that table: they were the reducer's earlier
   writes plus the diff, whose net effect is exactly the target state. The
   marker then sets `head = to`. Ops after the marker become this
   transaction's node, as a child of `to`. That is the new branch.

**Modes** (`TravelMode`):
- `Branch` (the default) keeps everything.
- `Truncate` drops the branch left behind: the subtree, under the common
  ancestor of `to` and the old head, that holds the old head. This is undo
  without redo. Travelling forward drops nothing.

**Rules:**
- One travel per table per transaction; a second one is an error. Writes
  before and after it are fine.
- A travel's diff is computed against one committed head. If the table
  changed before the commit (the batch is stale), a batch containing a
  marker is rejected as a conflict instead of being revalidated.
- A diff that can't be staged (a unique index clash while rows swap values)
  fails the call, and nothing of it is staged: the diff is validated on a
  copy of the overlay first.
- The table's overlay exists after a travel even when the diff is empty,
  so a concurrent commit to the table is still detected.
- Rows keep their primary keys on travel, auto-inc ones included. Counters
  only move up.

**Retention.** After a commit, while the tree holds more than `N` nodes, the
base is folded forward:
- If the head isn't the base: drop every subtree of the base except the one
  holding the head; if that is not enough, merge the base's child on the
  head's path into the base. That child's ops are forgotten, and its
  children become the base's.
- If the head is the base: drop the base's oldest child subtree.

The oldest history goes first, and branches forked before the window go with
it. Every remaining node stays reachable from the head.

**SDK.** `#[table(timeline)]` (1024 commits) or `#[table(timeline = N)]`;
not with `event`. The table handle then has `timeline()` and
`rows_at(point)` (with `CanRead`), and `travel(point)` and
`travel_with(point, mode)` (with `CanInsert + CanUpdate + CanDelete`). The
`hello` example keeps undoable notes this way (`note`, `erase`, `undo`,
`redo`, `rewind`, `show_notes`).

**Record and replay** (§6.7):
- The timeline belongs to the table, so the record baseline (a table clone)
  carries it.
- Timeline calls read the module's own state, so they are reproduced, not
  logged.
- Commits are recorded with their transaction id and timestamp, and
  `apply_writes` runs the same per-commit bookkeeping as a live commit
  (`TimelineCommit`). Baseline, replay copy and live tree therefore evolve
  identically.

**Why this causes no regressions:**
- Tables without `timeline` skip all of it: one `Option` check per write at
  commit.
- `TableSchema` gains a `timeline` field, so ABI 10 (every module rebuilds).
- Transaction ids start at 1 instead of 0. They are only used as seeds and
  blob-id prefixes, which only need uniqueness.
- The `Travel` marker is new, so existing batches are unchanged. Replica
  application never sees it.

**Status / limits:**
- History is in memory only. After a restart, the timeline starts from the
  loaded state.
- Replicas of a timeline table don't carry its timeline.
- `rows_at` and `travel` cost O(table rows + ops on the path).

---

## 7. Cross-module and cross-node interaction

### 7.1 Calls

| From → To | Semantics |
|---|---|
| Reducer → local reducer | **Deferred message** (§6.6). The call is recorded and released only if the caller commits. All calls of one commit run as one scheduled batch, in issue order, each callee as its own transaction. The result returned to the caller covers dispatch only (unknown module or reducer). |
| Reducer → remote reducer | Deferred the same way, then fire-and-forget over the network. |
| Reducer or query → local query | Synchronous, on the target module's query instance. |
| Reducer or query → remote query | Synchronous and blocking. ⚠ No timeout, and an error reply carries no request id, so the caller can hang forever. |
| Query → reducer | Refused. |

**Re-entrancy:** a query call into a module whose query is already on the
call stack is refused. Reducer calls are deferred, so they cannot re-enter, and
a module may call its own reducers.

**Caller identity:** every context carries `caller_node_id` and `caller_module_name`.
- `caller_module_name` is empty for jobs the runtime originates (events, timers,
  render) and for network calls.
- Co-located modules share a `caller_node_id`, so `caller_module_name` is the
  routing key.

**Bulk data (blobs):** a reducer or query parameter typed `Blob` takes a handle
to an immutable byte buffer held by the host (`runtime/blob.rs`), instead of
the bytes themselves.
- `Blob::new(&bytes)` copies once into the host. The blob belongs to the
  current call's `BlobScope` (§6.6). A deferred call batch keeps its caller's
  scope alive, and a callee resolves ids through its scope's parent chain.
  A blob therefore lives until the last call that can see it has run.
- A query shares its caller's scope, so the blobs it returns stay valid for the
  caller. An outermost query (host or remote request) inlines them as bytes.
- The GPU authority consumes blobs directly (`GpuCall::WriteTextureBlob`,
  `WriteBufferBlob`): the host passes the buffer to the wgpu queue, with no
  copy through the callee's memory. `graphics.create_texture` /
  `update_texture` take `Blob`. A 4.2 MB browser frame upload: 10–22 ms
  (median ~15) with `Vec<u8>` (guest → host → callee guest → host GPU call),
  1–3 ms (median ~2) as a blob.
- Across nodes a blob travels inline as `Bytes`; the receiving guest's `Blob`
  conversion re-registers it, so `Blob` parameters work for remote callers.
- Blobs are call-scoped: a table whose fields contain `Blob` is rejected at
  load, and a `Blob` kept in an instance cache is dead on the next call.

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

   If any step fails, the authorities claimed in step 2 are released, so a
   retry is not refused with "authority already taken". An unreachable node
   dependency (step 5) is such a failure: the load returns an error naming
   the node instead of panicking the worker.

   A `load` host call (Module authority) compiles and loads in the background
   and returns at once; the browser's 36 MB wasm takes about a minute to
   compile in a debug build. When it finishes, the runtime sends
   `ModuleEvent::LoadResult` to the requesting module's `module_load_result`
   subscription, and to no other module. Like every event, it enters the
   requester as an input, so its recording and replay see it (§6.7). Remote
   loads (`NodeSelection::Other`) get no result event yet.
3. **Unload.** Tear down subscriptions and replicas, release authorities and set
   `loaded = false`. The data is kept.
4. **Remove.** Unload, then delete the module directory.
5. **Node start.** Loads every persisted module with `loaded = true`, in
   dependency order.

---

## 10. ABI, SDK and CLI

**ABI** (`crates/interstice-abi`, `ABI_VERSION = 9`):
- Everything is postcard-encoded.
- Byte vectors travel as `IntersticeValue::Bytes(Vec<u8>)` (v8), a single
  length + raw bytes, not one `U8` value per byte. It validates as
  `Vec(U8)`, and `Vec<u8>` converts to and from both forms, so old stored
  values still load. A 4 MB texture upload took ~250 ms before and ~12 ms after.
- Bulk data between modules travels as a **blob** (v9): `IntersticeValue::Blob(id)`
  / `IntersticeType::Blob`, a handle to an immutable host-side buffer (§7.1).
- Pointer and length are packed into an `i64` (ptr high, len low).

| Item | Detail |
|---|---|
| Guest exports | `interstice_describe() -> i64`, `alloc`, `dealloc`, `__interstice_reducer_wrapper_<name>(ptr, len)`, `__interstice_query_wrapper_<name>(ptr, len) -> i64` |
| Host import `interstice_host_call` | One generic import carrying a `HostCall` enum: CallReducer, Schedule, CallQuery, TableScan, TableGetByPrimaryKey, TableIndexScan, Gpu, Audio, File, Module, Network, CurrentNodeId |
| Host imports, fast path | `interstice_log`, `interstice_time`, `interstice_random`, `interstice_insert_row`, `interstice_update_row`, `interstice_delete_row`, `interstice_clear_table`, `interstice_get_by_pk`, `interstice_blob_create`, `interstice_blob_len`, `interstice_blob_read` |
| Arguments in | `(RawReducerContext or RawQueryContext, IntersticeValue::Vec(args))` |
| Types | `IntersticeType`: Void, U8, U32, U64, I32, I64, F32, F64, Bool, String, Vec, Option, Tuple, Named, Blob. Structs and enums appear as `Named` with type definitions in the schema. |

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

**Done** (§4.1, §6.6): reads inside a reducer see its own pending writes,
commit is atomic, and queries called from the reducer see those pending writes
too.

### 11.3 Atomic multi-module transactions

**Decided: message passing with a transactional outbox** (§6.6). A reducer's
calls are released only if it commits; each callee is its own transaction,
scheduled with its own access set.

Nested transactions were rejected:
- Reducer schemas don't declare which reducers they call, so the scheduler
  could not admit the combined access set up front.
- Growing a running job's access set at call time either deadlocks or needs
  abort-and-retry, and retrying is unsafe once authority calls (GPU, audio,
  files) have run.
- Cross-module atomicity of *writes* is rarely needed. When it is, the callee
  can do the dependent work in its own transaction, triggered by the call.

### 11.4 Replay and time travel

The building block is in place: transactions are deterministic given their
inputs (§6.5), record and replay is done (§6.7), and so are table
timelines with branches (§6.8).

Open: travel kinds that keep *inputs* for re-simulation (rewind, then
re-run recorded reducers with a change), and persisting timelines.

### 11.5 Schema evolution

Today nothing checks persisted rows against a new table schema. A migration
story is needed: versioned table schemas plus a `migrate` hook or automatic
additive changes.

---

## 12. Examples: designing modules the Interstice way

### 12.1 The browser

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

### 12.2 The desktop: an OS made of modules

`modules/examples/desktop` is the showcase that ties the other examples
together. Nothing in it is special-cased by the engine: the desktop is an
ordinary module that holds no authority. It gets its powers by being the
**compositor** of `graphics`, the **input router** of `input`, and a client
of `module_manager`.

| Piece | Owner | What it shows |
|---|---|---|
| Surfaces, `draw_surface` | `graphics` (compositor claim) | each app renders into its own offscreen surface; the desktop composites them into windows |
| Input routing | `input` (router claim) | pointer and keyboard focus per app, in surface-local coordinates |
| App registry | `module_manager` | apps are registered wasm blobs, loaded and unloaded at runtime |
| Record / replay | `module_manager` → Module authority | a window's ● button records its app; a second click replays it on a detached copy and reports the outcome |
| Time travel | `hello` + `notes` | undo, redo and branches of a `#[table(timeline)]` table, driven from a UI |

**Apps.** The desktop bakes the example wasms (`include_bytes!`, since bytes
can't travel through a CLI string argument) and registers each one with an icon.
An app is a *window* or a *service*:
- Windows: `ui-example` (the UI library), `notes` (time travel),
  `browser-example` (Blitz + JS + the network broker), `agar-client`
  (cross-node replication; needs `example agar-server` running).
- Services have no window; their tile toggles load and unload. They are
  `hello-example` (the backend of `notes`), `http-get-example` (the network
  broker) and `audio-example` (the Audio authority: a 440 Hz tone).
- An app can declare requirements: opening `notes` first loads
  `hello-example`. A service cannot be stopped while a loaded app requires it.
- Left out: `graphics-example` drives the GPU directly. On a composited node
  the Gpu authority belongs to `graphics`, and one authority has one holder
  per node. `caller-example` needs a second node, as agar does, and adds
  nothing agar doesn't already show. `benchmark-workload` is not an app.

**Compositor identity.** `claim_compositor` and the surface reducers key on
`caller_module_name`. They used to key on `caller_node_id`, which every
co-located module shares, so any module could manage surfaces.

**Input routing (the `input` module).** Apps used to read the global
`MouseState`, `MouseButton`, `KeyState` and `TextInputBuffer` tables. Inside a
window that meant screen coordinates, so clicks landed at the wrong offset, and
every app received every keystroke. Routing follows Wayland: the compositor
says where each app is, and input tells each app what concerns it.
- Tables (private to `input`):
  - `InputRouter{id: 0, module_name, capture, keyboard}`: the router claim, a
    pointer capture flag, and the keyboard-focus module (`""` for none).
  - `InputRoute{module_name, x, y, w, h, z}`: an app's content rect in screen
    pixels, plus its stacking order.
  - `PointerGrab{id: 0, active, module}`: the implicit grab.
- Reducers. `claim_input_router()` lets the first caller win; the others are
  router-only:
  - `set_input_route(module, x, y, w, h, z)` and `remove_input_route(module)`;
  - `set_keyboard_focus(module)`;
  - `set_pointer_capture(bool)`: the router owns the pointer, e.g. while the
    launcher is open.
- **Pointer owner:** the grabbed module while a grab is active. Otherwise no
  app while captured; otherwise the highest-`z` route that contains the
  pointer, else no app.
- **Grab:** when a button goes down with none down before, `on_input` stores
  the pointer owner. When the last button goes up, the grab is released. A drag
  that starts in a window keeps going to that window even when the pointer
  leaves it. A drag that starts on desktop chrome never reaches an app.
- Query `input_view() -> InputView{position, wheel_delta, left, right,
  middle, pointer_inside, keyboard_focus}`, resolved from `caller_module_name`:
  - No router (every standalone example): global position, raw buttons, and
    both flags true. This is exactly the old behaviour.
  - The router itself: global position and raw buttons. `pointer_inside`
    means no app owns the pointer; `keyboard_focus` means no app has the
    keyboard.
  - A routed app: its position relative to its route's origin;
    `pointer_inside = (owner == caller)`; buttons are reported only while
    inside; `keyboard_focus = (keyboard == caller)`.
  - Any other module, while a router exists: global position, no buttons and
    no focus. A module without a window gets no input, as in Wayland.
  - `wheel_delta` stays the cumulative total. Consumers diff it every frame,
    but act on the difference only while `pointer_inside`.
- Apps read the view and ignore the raw tables. Key and button *events* are
  still emitted to every subscriber, so event reducers check `input_view()`
  before queueing. Routing is **cooperative** for now: the raw tables stay
  public, much as `network` results are filtered by an `owner` column, until
  row-level visibility exists (§13.3).
- No regression in theory: without a router every app sees exactly what the
  tables held. `input_view` is a query on tables that apps read before anyway,
  so it runs under the same pinned snapshot (§6.6).

**App registry (`module_manager`).** `App` rows used to hold the wasm bytes,
so `list_apps`, which the launcher calls every frame, copied every binary (the
browser alone is 36 MB).
- The bytes now live in `AppBinary{id, bin}` beside
  `App{id, name, icon, hash, loaded}`.
- `register_app(name, wasm: Blob, icon)` takes a call-scoped Blob, and skips
  rewriting the binary when its `hash` is unchanged.
- Lookups by name use the unique index.
- `load_app` sets `loaded` when it sends the request; the load itself is
  asynchronous (§9). `module_manager` subscribes to `module_load_result`. On
  a failure it clears `loaded` for that app, and on every result it inserts
  a row into the public event table `LoadReport{seq, module, ok, message}`
  (`event = 32`). A compile failure has no module name, so it is only
  reported, and attributed to no app.
- The desktop subscribes to `LoadReport` inserts. On a failure it closes the
  app's window, as the close box would, and shows the message as a toast.
  Example: `agar-client` opened while `example agar-server` isn't running
  fails at "connect to node dependencies" with a clear error.
- A reducer job queued for a module that has been unloaded meanwhile (for
  example, frames already scheduled when a window closes) is dropped with a
  warning, not logged as an error.

**Replay reports.**
- The Module call `Replay` now returns the report text
  (`ModuleCallResponse::Report`, ABI 11), and the SDK's `replay` returns
  `Result<String, String>`.
- `module_manager.replay` no longer fails on a divergence. It inserts a row
  into the public event table `ReplayReport{seq, module, ok, message}`
  (`event = 32`) and returns `Ok`.
- The desktop subscribes to the insert and shows the report as a toast in the
  top bar.
- Recording an app needs every input of the app to come through its boundary
  (§6.7). So `ui_subsystem!` keeps its last-seen wheel total in a table
  (`UiPointer`), not in a `static`. The static was instance memory, which a
  replay starts without, so the first replayed frame used to scroll and
  diverge.

**Notes (time travel from a UI).** `modules/examples/notes` is a front-end
with no table of its own beyond its UI. It binds to `hello-example` (a local
module dependency):
- It reads `Notes` through the binding and history through a new hello query
  `note_history() -> NoteHistory{head, commits: [NoteCommit{id, parent}]}`.
- It calls `note`, `erase`, `undo`, `redo` and `rewind(id, "branch")`.
- Hello keeps the timeline. Undo and redo are travels of hello's table, and a
  click on a commit chip travels to that commit, keeping the future as a
  branch.
- The UI is rebuilt only when a signature of the notes and history changes.
  The signature is stored in a `NotesView` row.

**Window manager state** is in desktop tables: `Window` (rect, focus order,
surface, recording flag), `DesktopState` (launcher, focus, pointer capture,
`pending_open`), `Interaction` (move/resize), `PrevMouse` (click edges) and
`Toast`. The reducer `open(app)` sets `pending_open`, and the next frame opens
the app exactly as a launcher click would; it is the scriptable entry point
(`interstice call_reducer desktop-example desktop-example open notes`). The writeback updates only the rows
that changed, and on a change it updates the window's input route.

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
- [x] **Isolation and ordering** (§6.6), done as one design:
  - [x] **Cross-module transactions** (§11.3): reducer calls are deferred
        messages in a transactional outbox, released only on commit.
  - [x] **Scheduler sees nested calls**: call batches are jobs whose access
        set is the union of the callees' sets plus a per-channel key.
  - [x] **Queries from reducers see pending writes** (§11.2).
  - [x] **Consistent snapshot per call**: reducers by scheduling (replica
        writes are jobs too), queries by pinning read locks.
  - [x] **Event order = commit order**: commits emit under the write lock.
  - [x] Blob scopes follow the call batch (`BlobScope`), and scheduler
        access keys are normalised (§6.3).
- [x] **First-class event tables**: `#[table(event = N)]` (§3.1), replacing
      `network`'s hand-written `emit!` trimming. The browser's `DocEvent`
      stays app-compacted on purpose: it is a replay log, and dropping its
      oldest rows would change the replayed document, so compaction folds
      it into a snapshot event instead.
- [x] **Determinism** (§6.5): a per-transaction `Tx` (id, timestamp, seed)
      shared by every call in the transaction; schedules go through the
      outbox.
- [x] **Record and replay** (§6.7): an opt-in log of everything that enters a
      module; replay on a detached copy that checks writes and final state.
- [x] **Time travel** (§6.8): `#[table(timeline = N)]` keeps a commit tree;
      `travel` / `rows_at` / `info`, with branches and truncation.
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
- [x] Generated read methods return `Result` instead of panicking: `scan`,
      `get`, `scan_by_*`, `get_by_*` (local and bindings) go through
      `sdk-core/reads.rs`; row decode is `TryFrom<Row>`; the panicking
      `IntoIterator` on table handles is gone. Reducers may return
      `Result<(), E>` and queries `Result<T, E>`; `Err` aborts the call.
- [x] A shorthand for "full access to these tables": `CanAll<T>`, tuple rows
      in `where` clauses, and `caps!` named sets (§5). The browser's
      engine reducers share `BrowserCaps`.
- [ ] Benchmarks for the overlay / re-validation path and pool affinity.
- [x] Bulk data between modules: call-scoped `Blob` handles (§7.1). The
      GPU authority uploads straight from the host buffer; a 4 MB texture
      went from ~15 ms to ~2 ms.

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
- [x] **Desktop**: the window manager is a module (§12.2): compositor,
      input router, app launcher over `module_manager`, services with
      requirements, record/replay from a window, time travel in `notes`,
      load-failure reports.
- [ ] Polish agar, ui, audio and graphics examples into short, readable
      showcases of one feature each.
