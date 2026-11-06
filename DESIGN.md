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
- **History.** A logged table's log is its history. A reducer can read the
  table at any past point and restore it there (§6.7). `history = "7d"` limits
  how far back the log is kept; the default, `"all"`, keeps everything.

### 3.2 Persistence modes

| Mode | Intended use | Storage |
|---|---|---|
| **Logged** | Authoritative data whose history matters, the standard SpacetimeDB model | Append-only log in segments (`logs/<table>/<first_seq>.log`) plus checkpoints (`snapshots/<table>/<seq>-<tx>-<ts>.snap`). Nothing is compacted away: the log is the table's history, kept as long as `history` says (§6.7) |
| **Stateful** | Durable current state where history is irrelevant (settings, open tabs) | One file per row: `stateful/<table>/<hex(pk)>.row` |
| **Ephemeral** | Runtime-only state (input state, in-flight requests, frame data) | Memory only; empty after every load |

Everything on disk is postcard-encoded. A node's data directory looks like this:

```
<data_dir>/nodes/<uuid>/
  node.log, node.toml, identity.key, known_peers.toml
  modules/<module>/
    module.wasm, schema.bin (the ModuleSchema the data is in), state.toml (loaded = bool),
    sequences.toml (auto-increment ceilings)
    logs/<table>/*.log  snapshots/<table>/*.snap  stateful/<table>/*.row
```

**On restore** (a module reload):
- logged tables load the newest checkpoint and then replay the log entries after it;
- stateful tables read every row file;
- ephemeral tables start empty.

**Status:**
- **Durability is asynchronous.** Commits are acknowledged before they reach the
  disk. A background thread fdatasyncs the WAL and flushes stateful rows every
  10 ms, so a crash can lose about 10 ms of commits.
- **One write per commit.** A commit's log entries are buffered per log file
  and handed to the OS in one `write` at the end of the commit, so a process
  crash loses no committed transaction (only an OS crash or power loss can,
  within the sync interval). They used to be two `write`s per entry. The
  transfer benchmark went from 59k to 83k tx/s, and the commit phase from
  4.8 to 2.2 µs.
- **Stateful rows** are flushed by the same thread: each row file is synced
  before it is renamed over the old one, and each directory is synced once after
  its renames and removals. A crash can lose the last ~10 ms of row changes, but
  never leaves a torn row.
- **`auto_inc` counters persist** (after SpacetimeDB's sequences). A counter of a
  stateful or logged table reserves blocks of 4 096 ids. Before handing out the
  first id of a block, it saves the block's ceiling in
  `modules/<module>/sequences.toml` (`table.field = ceiling`, written
  atomically and synced). On load the counter continues from the saved ceiling,
  so ids of deleted rows are never handed out again, even after a crash. The
  price is a gap of up to one block per restart. A restore (time travel,
  replica sync) keeps the table's counters. Counters of ephemeral tables stay in
  memory.

### 3.3 Schema versioning and migrations

Updating a module must not lose or garble its data. This follows
SpacetimeDB's *automatic migrations*: each load compares the schema the data
was written in with the new module's schema and builds a **migration plan**.
Safe plans are applied without asking. Anything that would lose data, or
change the row layout that other modules are compiled against, is refused
until someone decides.

**Old schema.** Each successful load writes `schema.bin` (postcard
`ModuleSchema`) next to the data. On a reload the old schema is that file.
For data written before `schema.bin` existed, the old `module.wasm` is
described if it still instantiates. Otherwise the old schema is unknown: the
plan is safe only if every restored row passes `validate_row` against the new
schema.

**Rows are positional** (`Row.entries` follows `TableSchema.fields`), and
dependents decode them positionally from their bindings, ignoring extra
trailing entries. Columns are matched **by name**, and types are compared
structurally, including the named struct and enum definitions they reference.

| Change | Plan |
|---|---|
| Reducer, query or subscription added, removed or changed; any code change | safe (a removed reducer or query is listed: callers may break) |
| Table added; **empty** table removed | safe |
| Column appended **at the end** with a default (`#[default(expr)]`, or `Option<T>` → `None`) | safe; existing rows get the default |
| Index added or removed; `auto_inc` toggled; visibility, `history` or `retention` changed | safe |
| `unique` added, and the current rows satisfy it | safe |
| Any change to an **ephemeral** table, or a table becoming ephemeral while empty | safe (there is no persisted data) |
| Column removed (a rename is a removal plus an addition) | **asks**: the column's values are dropped |
| Columns reordered, or a column inserted mid-table | **asks**: lossless, but modules built against the old layout must be rebuilt |
| Column type changed | **asks**: the column is dropped and the new one filled with its default; with no default, the table is cleared |
| Column added without a default | **asks**: the table is cleared |
| Non-empty table removed | **asks**: its rows are dropped |
| Primary key renamed or retyped | **asks**: the table is cleared |
| `unique` added that the current rows violate | **asks**: the table is cleared |
| Persistence changed (logged ↔ stateful, or to ephemeral with data) | **asks**: the table is cleared |

The plan is per table, one of: *keep* (layout unchanged), *map* (each new
column is taken from an old position or filled with a default), *clear*,
*drop* (removed table), or *create*. Applying a plan rewrites the data in
the new layout: stateful row files, **every log entry and checkpoint** of
logged tables. History survives a migration, so time travel to a point
before it shows the old rows in the new layout.

**Deciding.** A load carries a `MigrationPolicy`:
- `Safe` (the default for every path): apply safe plans; refuse the rest
  with `MigrationRequired(plan)`. Nothing changes: the old module keeps
  running and its data and `module.wasm` are untouched.
- `AllowDataLoss`: apply the plan as listed, including the drops and clears.
- `DeleteData`: discard this module's data and load it as new (`init` runs).

`interstice load <node> <path>` asks the node for the plan, prints it, and
when the plan needs a decision prompts `[a]bort / [m]igrate (apply as
listed) / [d]elete this module's data`. `--allow-data-loss` answers the
prompt in advance; `--delete-data` always discards the module's data, as
SpacetimeDB's flag does. Without a terminal and without a flag, the CLI
aborts. The node answers every load with
`ModuleLoadResult` (loaded with plan, decision needed with plan, failed, or
forwarded to the Module authority).

**Updating a loaded module.** A `load` of a module that is already loaded is
an **update**, not a remove-and-reload. The plan is checked against the
running schema first; only if it passes is the old version unloaded, keeping
its data, and the new one loaded as a reload (no `init`; `load` fires).
Node startup, `module_manager` and the `ModuleCall::Load` host call use
`Safe`. A refused plan at node startup leaves the module unloaded, with its
data, and logs the plan.

**Crash safety.** The new data is written into `<module>/.migrate/`, then
`.migrate/READY` is written, then each of `stateful/`, `logs/` and
`snapshots/` is swapped in, then `schema.bin`. At startup, a `.migrate/`
without `READY` is deleted (the old data is intact). One with `READY` is
finished. Every mapped row is checked with `validate_row` before `READY`; a
failure aborts the migration and changes nothing.

**Status:**
- **Remote dependents are not checked.** Modules on other nodes that
  replicate a table see the new layout at their next sync.
- **Loaded local dependents keep running.** They are compiled against the
  old layout: an appended column is fine, anything listed as *asks* needs a
  rebuild. Dependency versions must still match exactly (§7.2).
- **No custom migration hook yet.** For a transformation the plan can't
  express, use SpacetimeDB's incremental pattern: add `note_v2`, copy or
  convert from `note` in the `load` reducer, then remove `note` once it is
  empty.
- **`interstice example` always starts from a fresh node**, so migrations
  matter for `interstice node` plus `load`.

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

**Calls** are checked by visibility, not capabilities (§7.4). Schedule only
targets the module's own reducers; time and random are not checked.

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
                                    scheduler (shared, behind a lock):
                               the ingress thread admits new jobs; a
                               worker completes its own job and keeps
                               the first job that releases
                                                       │ ready
                                     worker pool: min(cores, 8) threads
                                       each takes an instance from the
                                       module's pool of 8 WASM instances
```

Workers complete their own jobs under the scheduler lock. A conflicting job
released by a completion therefore runs on the same worker at once, instead of
going through a scheduler thread, a channel and another worker's wake-up. The
admission rules are unchanged. Serial (conflicting) work went from 83k to
140k transfers/s in process, and from 44.5k to ~98k over TLS.

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
- When a job completes, a waiting job is promoted unless it conflicts with a
  running job or with an earlier job that is still blocked.

The scheduler keeps **one FIFO queue per access key** (table or channel),
holding the running and waiting jobs that touch it, in arrival order, each
with the set of operations it performs on that key. A job is *clear* on a key
when it conflicts with no earlier entry of that key's queue; it runs when it
is clear on every key, so it keeps a count of the keys it is still blocked on.

Two operation sets are compatible only if both are `{Read}` or both are
`{Insert}`, and compatibility is then an equivalence. So in every queue the
clear entries form a **prefix**: an entry blocked by an earlier one blocks all
later ones. That makes both steps local:
- **enqueue**: per key, the new entry is clear iff the queue is empty or its
  last entry is clear and compatible with it: O(keys).
- **complete**: per key of the finished job, remove it and walk the queue from
  the front, marking entries clear until the first one that conflicts with an
  earlier entry; a job whose blocked count reaches zero is promoted. Promoted
  jobs are returned in arrival order. The cost is the clear prefix, which is
  bounded by the running jobs and the jobs it releases, not by the queue.

The previous scheduler rescanned the whole waiting queue on every enqueue and
completion, which is O(n²) under contention: 20 000 queued updates cost
0.9 ms each in scheduling alone. The admission rule, and so the guarantee
below, is unchanged.

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
| Admission rescanned the whole waiting queue | O(n²) under contention: 590 tx/s with 20 000 queued updates | *closed*: per-key queues (§6.2), 38 700 tx/s |
| `send_packet` spawned one task per packet | Two packets to the same peer could overtake each other | *closed* for reducer workers: packets go straight into the peer's channel, and a worker waits when it is full; only an event-loop sender facing a full channel still spawns |

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

Logged tables keep each entry's `tx` and timestamp, so a table can be read
and restored as of any past transaction (§6.7).

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


### 6.7 Time travel

Time travel works as in SpacetimeDB: a **logged** table's log *is* its
history. Any past state of the table can be rebuilt from the log, read, and
restored. There is no separate recorder and no in-memory history. Every
logged table has a history, and its retention is configurable.

**The log** (§3.2) holds one entry per operation:
`LogEntry{seq, tx, timestamp_ms, op}`.
- `seq` counts the table's entries.
- `tx` and `timestamp_ms` come from the transaction that wrote the entry (§6.5).
- `op` is one of `Insert(row)`, `Update(row)`, `Delete(pk)`, `Clear` or
  `Restore{to}`.

The entries of one transaction are contiguous, because a commit holds the
module's table write lock (§6.6). A *commit* of the table is a maximal run of
entries with the same `tx`.

**Every write has a `tx`.**
- Reducer commits use their own `Tx`.
- A logged write that doesn't come from a reducer draws a fresh
  `(id, timestamp)` at apply time, under the table lock. Replica changes are
  never logged, so today only reducers write to logs.
- Ids and timestamps are drawn together, under one small lock, so
  `timestamp_ms` never decreases as `tx` grows. A cut by time is therefore
  also a cut by `tx`.
- On restart, the runtime reads the largest `tx` and timestamp in the
  restored logs and continues above them. Along a table's log, `tx` therefore
  grows strictly, across restarts too.

**Points** (`HistoryPoint`). The state *at* a point is the table after every
entry the point covers:

| Point | Covers |
|---|---|
| `Tx(id)` | entries with `tx <= id`. `id` need not be a commit of this table |
| `Time(ms)` | entries with `timestamp_ms <= ms` |
| `Back(n)` | the table's `n`-th most recent commit and everything before it. `Back(0)` is the current state |
| `Start` | the oldest state still retained |

A point older than the retained history is an error. With nothing pruned, the
oldest state is the empty table, so any point resolves.

**A consistent cut across tables.** The same `Tx(id)` (or `Time(ms)`) applied
to several tables of a module gives a state that a serial execution actually
reached. Proof:
- Commit order is a valid serial order (§6.5).
- If transaction `a` conflicts with an earlier-committed `b`, the scheduler
  admitted `a` only after `b` committed (§6.6), so `b.id < a.id`.
- So the set `{tx <= id}` is closed under "committed before and conflicting",
  which makes it a prefix of a valid serial order.

Restoring several tables to the same point in one reducer is therefore
coherent.

**Calls** (`HostCall::History`, on the module's own logged tables only):
- `commits(table, limit)`: the newest `limit` commits, newest first. Each one
  is `HistoryCommit{tx, timestamp_ms, ops, restored_to}`, where `restored_to`
  is set when the commit was a restore. Needs read access.
- `rows_at(table, point)`: the rows at a point, in primary-key order, without
  changing anything. Needs read access. Allowed in queries.
- `restore(table, point)`: move the table back to a point, in a reducer only.
  It returns the `tx` of the last entry the point covers (`0` for an empty
  start). It needs insert, update and delete access, since it may do all
  three.

**How a restore commits.** A restore is an ordinary write, so the transaction
stays atomic, and subscribers, replicas and persistence work unchanged:
1. Rebuild the rows at the point (below). Diff them by primary key against
   the reducer's current view: committed rows plus its earlier writes.
2. Stage the diff through the usual overlay: deletes, then updates, then
   inserts, each in primary-key order. The diff is validated on a copy of the
   overlay first, so a diff that can't be staged (for example a unique index
   clash while rows swap values) fails the call and stages nothing.
3. Stage a marker, `Transaction::Restore{table, to}`. At commit it becomes a
   `Restore{to}` log entry, which applies nothing but tags the commit.

History stays linear: a restore is a new commit on top, and the commits it
undid stay in the log. A restore can itself be restored away.

**Rules:**
- One restore per table per transaction. Other writes before or after it are
  allowed.
- A restore's diff is computed against one committed state. If the table
  changed before the commit (the batch is stale), a batch with a restore
  marker is rejected as a conflict instead of being revalidated.
- Rows keep their primary keys, auto-inc ones included. Counters only move up.

**Undo, redo and branches** follow from the `restored_to` tags, with no
extra state. This is app logic over `commits`; the `hello` example implements it.
- The *position* after a prefix of the log is `to` if the prefix's last
  commit is `Restore{to}`, otherwise that last commit (or `0` if the prefix is
  empty).
- The *parent* of an ordinary commit `c` is the position just before `c`.
  Ordinary commits and their parents form a tree, and restores add no node.
  So the linear log still shows the branches an editor needs.
- *Undo* restores to the parent of the current position. *Redo* restores to
  the newest child of the current position, while one exists.
- Example: `A`, `B`, undo (restore to `A`), `C`. Then `C`'s parent is `A`,
  undo goes to `A` and then to the empty start, and redo from `A` goes back to
  `C`. `B` stays reachable as a branch from `A`.

**Rebuilding a past state:**
1. Pick the newest checkpoint whose last entry the point covers (or the empty
   start).
2. Replay later entries, in `seq` order, while the point covers them.

A checkpoint can sit in the middle of a transaction (it is taken after a
given entry). It is usable when its last entry is covered: replay then
finishes the transaction. It is never used for a point before that
transaction. The cost is reading one checkpoint plus at most one checkpoint
interval of entries.

**Storage:**
```
logs/<table>/<first_seq>.log         segments, rotated at each checkpoint
snapshots/<table>/<seq>-<tx>-<ts>.snap   checkpoints: {seq, tx, timestamp_ms, rows};
                                         the header is in the name, so choosing one reads none
```
- A checkpoint is taken after entry `S` once the entries since the last one
  reach `max(256, rows the table had at the last checkpoint)`. A checkpoint
  costs at most as many rows as the entries before it, so checkpoints cost
  O(1) rows per write, amortised, and take at most about as much disk as the
  log. (Measuring against the rows *now* would stop checkpoints for a table
  that only grows.)
- The writer moves to segment `S + 1` as soon as the checkpoint is planned,
  under the table lock. The rows are cloned under the same lock, as now, and
  the file is written after the lock is released.
- A crash before that write leaves a segment with no checkpoint before it.
  Readers then fall back to an older checkpoint and replay through it.
- The old segment's writer is synced by the background thread before it is
  closed.
- Restore on load: the newest checkpoint plus every later entry. This is the
  same as before, with the log split into segments.
- Legacy layout (`logs/<t>.log`, `snapshots/<t>.snap`): the table is restored
  the old way and a first checkpoint is written in the new layout. The legacy
  files are then removed, so the history starts at that checkpoint.

**Retention:**
- `#[table(history = "7d")]` (units `s`, `m`, `h`, `d`), or
  `history = "all"`, the default. It goes in `TableSchema.history`, in ms.
- It applies only to logged tables; anything else is a compile error.
- After each checkpoint is written, the horizon is `now - history`. The newest
  checkpoint at or before the horizon becomes the base. Older checkpoints, and
  segments whose entries are all before the base, are deleted. Every
  checkpoint ends a segment (the writer rotates when it is planned), so these
  are exactly the segments that start at or before the base.

**Determinism** (§6.5): history calls read the module's own op history, so
they are a deterministic function of it, like any table read.

**Why this causes no regressions:**
- Restore on load still produces the same state: the newest checkpoint plus
  the later entries.
- The write path does the same work: one appended entry per op, two more
  `u64`s per entry.
- Rotation replaces compaction, which used to rewrite the whole log. Fewer
  checkpoints are taken (an interval that grows with the table, instead of
  every 256 entries), so large tables do much less snapshot work.
- Non-logged tables are untouched.
- Transaction ids stay unique. They are now also monotone across restarts,
  where before they restarted at 1.
- `TableSchema` drops `timeline` and gains `history`, and `HostCall` loses
  `Timeline` and gains `History`. So the ABI goes to 12, and every module
  rebuilds.

**Status / limits:**
- A node without a data directory (in-memory) keeps no history. History
  calls then return an error.
- Replica tables are never persisted, so they have no history. The source
  table's history lives on its own node.
- `rows_at` and `restore` read from disk: one checkpoint plus up to one
  interval of entries.

---

### 6.8 Phase breakdown (profiling)

`runtime/profile.rs` keeps a count, a total and a maximum per phase of a call:
- `blocked` (in the scheduler) and `ready` (waiting for a worker);
- `prepare` (security, frame, encoding) and `instance wait`;
- `wasm` (host calls included), and each host call by kind: insert, update,
  delete, get, scan, other;
- `commit`, and the `persist` work inside it;
- `reply`.

It is on with `INTERSTICE_PROFILE=1`, in which case the node prints the table
to stderr every 10 s. An embedder turns it on with `profile::enable()`. When
it is off, each probe costs one relaxed atomic load. Totals are summed over
threads, so they are CPU time per phase, not wall time.

## 7. Cross-module and cross-node interaction

### 7.1 Calls

| From → To | Semantics |
|---|---|
| Reducer → local reducer | **Deferred message** (§6.6). The call is recorded and released only if the caller commits. All calls of one commit run as one scheduled batch, in issue order, each callee as its own transaction. The result returned to the caller covers dispatch only (unknown module or reducer). |
| Reducer → remote reducer | Deferred the same way, then fire-and-forget over the network. |
| Reducer or query → local query | Synchronous, on the target module's query instance. |
| Reducer or query → remote query | Synchronous and blocking, up to 30 s. The reply carries the request id with the value or the error. |
| Query → reducer | Refused. |
| Client → reducer (network) | Queued like any job. A `ReducerCall` with a `request_id` gets a `ReducerResult`: `Ok` once its writes are committed, or the error that rolled it back (unknown module or reducer, refused by visibility, `Err` returned, trap, commit conflict). "Committed" means applied and handed to the logger; the disk write follows within the flush interval (§3.2). Without a `request_id` the call is fire-and-forget. |

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

### 7.4 Security: the kernel and the module manager

The node core is the **kernel** of an operating system made of modules.

- **The kernel provides the mechanisms and enforces them:**
  - who a peer is (authentication);
  - what each call may reach (visibility);
  - which module holds which authority;
  - where the file authority may go.
- **Decisions are policy, and policy belongs to the module that holds the
  `Module` authority** (`module_manager` by default). Decisions here mean
  loading, unloading or removing a module, granting an authority, and
  admitting a peer.
- **Without such a module, the kernel decides on its own**, from the
  owners list and a CLI prompt. That is the "simple command program" mode.

**Identities and transport, after SpacetimeDB.**

SpacetimeDB is the guideline. Its identity is a hash of a credential: an
ES256 JWT's issuer and subject, signed by the server's `id_ecdsa` key or
by an OIDC provider. Reducers see that identity as `ctx.sender`, and the
database owner is an identity too. Its server listens on localhost, and
TLS is left to a reverse proxy. Interstice is peer to peer, with no
server to issue tokens and no proxy in front, so:

| SpacetimeDB | Interstice |
|---|---|
| Identity = hash of (issuer, subject) of an ES256 JWT | Identity = hash of the peer's own ECDSA P-256 public key: every node and CLI is its own issuer |
| The server keeps its signing key pair (`id_ecdsa`) on disk; losing it loses the identities | Every node (`<node>/identity.key`) and CLI (`cli_identity.key`) keeps its key pair on disk |
| A bearer token is sent with each connection | Nothing secret crosses the wire: a peer proves it holds the key in a mutual TLS 1.3 handshake |
| TLS from a reverse proxy; the server listens on localhost | TLS built in, since there is no proxy between peers; still localhost by default |
| `ctx.sender` | `ctx.caller_node_id` |
| The database owner publishes, updates and deletes | Owners (§7.4 Administration) |
| `client_connected` may return an error to refuse a client | The Module holder's `connect` handler may return an error to refuse a peer |
| Tables private by default; RLS and views for rows | Same defaults; rows: future work |

- **The identity is the id.**
  - A new node's or CLI's id is a UUID made from the first 16 bytes of
    SHA-256 over its public key (SubjectPublicKeyInfo), with the version
    bits of UUID v8. So an id is **self-certifying**: no one else can
    present it without the key.
  - Nodes created before keys keep their random UUID and get a key at
    their next start. Their peers remember that key on first use
    (`known_peers.toml`, like SSH's `known_hosts`), and refuse another
    key for that id afterwards.
- **Handshake.**
  1. TCP, then TLS 1.3 with client authentication. Both sides present a
     self-signed certificate for their key, generated at start. The
     verifiers check every handshake signature but no CA chain: trust is
     the identity, not a certificate authority, as in libp2p's TLS
     transport.
  2. Inside the encrypted channel, a `Handshake { node_id, address }`.
     The receiver checks that `node_id` matches the key: derived from it,
     or remembered for it.
  3. The CLI also checks that the node answering is the one its
     registry names.
- **Owners** are the identities allowed to administer a node, listed
  by id in `node.toml` in the node directory.
  - The CLI that creates or starts a local node adds itself.
  - Others are added with `node owner add <node> <id>`;
    `interstice identity` prints this CLI's id.
  - No secret is needed: an id can't be claimed without its key. The
    tokens of the previous design are gone.
- **Admission.**
  - When a peer connects, the `connect` event runs as before.
  - If the Module holder's `connect` handler returns `Err`, the kernel
    disconnects that peer, like SpacetimeDB's `client_connected`.
  - `module_manager` refuses peers listed in its `BlockedPeer` table
    (`block_peer` / `unblock_peer`, both `local`), and admits everyone
    else, as SpacetimeDB admits any valid identity.
  - Without a holder, everyone with a valid key is admitted. What a peer
    may then do is the visibility rules' business.

**Listening.**
- A node listens on `127.0.0.1` by default, so only this machine can
  connect.
- `listen = "0.0.0.0"` in `node.toml` (`interstice node expose <node>
  on|off`, or `node create … --expose`) opens it to the network.

**Who may call what.** Every reducer and query has an effective visibility:

| Visibility | Callable by |
|---|---|
| `public` | anyone connected, from any node |
| `local` | modules on the same node |
| `internal` | its own module only |

- **The default** depends on the module and the function:
  - a reducer with `on = "…"` (an event handler) is `internal`;
  - otherwise `public` in a `Public` module, and `local` in a `Private`
    module.
- **Override** a single function with `#[reducer(public|local|internal)]`
  or `#[query(…)]`. Visibility travels in the schema; this bumped the ABI
  to 14.
- **The kernel itself is never limited.** Event handlers, `init`/`load`,
  `render`, timers and schedules (a module schedules only its own
  reducers) are not affected by visibility.
- **Owners are never limited either**: an owner's CLI may call anything,
  for administration and debugging.
- **Where the check runs:** at the one entry each kind of call goes
  through, `Runtime::call_reducer` and `Runtime::call_query`. It runs
  before any transaction starts. The caller is classified from what every
  job already carries:
  - local node and no module: the kernel;
  - local node and a module: that module;
  - another id: a peer, owner or not.
  A refused call fails with `PermissionDenied` and runs nothing. A call
  from a module to another module on the same node is also checked when
  it is made, so the caller's `call_reducer` returns the error at once
  instead of the deferred call failing later.
- **Remote reads of tables follow the same rule.**
  - A subscription or table sync from a non-owner needs both the module
    and the table to be `Public`.
  - Until now subscriptions were not checked at all; only table sync
    checked the table.
- **Schema requests.** A non-owner gets `NodeSchema::to_remote()`: public
  modules only, private tables removed, and only the reducers and queries
  it may call. An owner gets everything, which bindings generation relies
  on. (`to_public()`, used for bindings, keeps `local` functions, because
  a module's bindings to a module on its own node need them.)

**Administration.** `load`, `unload` and `remove` arrive from the network:
- **With a `Module` holder:** the kernel forwards the request as a
  `ModuleEvent`, with the authenticated `caller` and `owner: bool`, and
  answers `Forwarded`. The holder decides.
  - `module_manager` accepts requests from owners: it loads (and
    registers the app), unloads or removes.
  - A load through the holder (`ModuleCall::Load`) uses the same update
    path as the CLI, with the safe policy. Loading the bytes that are
    already running does nothing, so the desktop can call `load_app`
    for an app that is already open.
  - It refuses everyone else, and logs the refusal.
  - Its own admin reducers are `local`, so a remote peer can't go around
    it by calling `load` directly.
- **Without a holder:** the kernel accepts owners and refuses everyone
  else.

**Authorities.**
- Claiming an authority (§8) needs a grant.
- **The grants already given** are stored with each module (`granted` in
  `state.toml`):
  - a reload or update that asks for nothing new needs no new grant;
  - a module written before grants existed counts as granted.
- **Who grants:**
  - **A `load` from the CLI** with no holder: the kernel answers
    `NeedsGrant { authorities }`, and the CLI asks
    `Grant File, Network to 'x'? [y/N]`. `--grant` answers in advance,
    and without a terminal the answer is no. Same flow as the migration
    prompt (§3.3).
  - **A bundle, an example, or a node start**: running it is the owner's
    consent. The kernel logs every grant it gives.
  - **A load by the `Module` holder** (`ModuleCall::Load`): the holder
    decided, so the grant is given.
- **The kernel bounds what any grant can reach.** The File authority is
  confined to the node's file root:
  - `<node>/files` by default, or `files_root` in `node.toml`;
  - `interstice example` sets it to the directory the example runs in;
  - relative paths are taken from the root;
  - absolute paths outside it, `..`, and symlinks leading out of it are
    refused.
  - File watches are confined the same way, and report paths relative
    to the root.
- **Only the owner chooses who holds `Module`.** A module that claims the
  Module authority can only be granted by an owner's load or a bundle,
  never by a non-owner's request, and the holder can't be replaced
  remotely.

**Status:**
- **Admission runs after the handshake.** The peer is registered and its
  `connect` event delivered, but its connection reads nothing until the
  holder's handler has decided (30 s at most, then it is dropped), so a
  refused peer's requests never run.
- **Losing a key file loses the identity**, as in SpacetimeDB: a node
  that regenerates its key gets a new id (a legacy id keeps its id, but
  its peers will refuse the new key).
- **No key rotation or revocation yet.** Refusing a stolen key is up to
  the admission policy (`block_peer`).
- **A refused CLI sees "connection reset"**, not a reason, because the
  node closes the connection after its handshake reply.
- **The cost of TLS on the benchmark path is not measured yet.**
- **An `Err` from a reducer is logged as a panic with a backtrace** (the
  SDK aborts the call that way), which makes a refusal look alarming in
  the log.
- **Local modules trust each other.** Any module on the node may call the
  module manager's `local` admin reducers. Per-caller rules (for example
  "only the desktop may load apps") are the manager's business, through
  `ctx.caller_module_name`.
- **No per-user identities or roles beyond owners.**
- **Loads forwarded to the manager lose the migration prompt.** The
  manager loads with the safe policy: a load whose plan needs a decision
  is refused, and the refusal is reported in its `LoadReport`.

---

### 7.5 Views

A **view** is a read-only relation that a module publishes. Its rows have
a primary key, and they are defined from the module's tables, possibly
depending on who reads them, the **viewer**. A table is stored and
observable; a query is defined by code but answers once. A view is
defined by code and observable: readers scan it, get by key, subscribe
to its insert, update and delete events, and replicate it across nodes,
exactly like a table they can't write. It follows SpacetimeDB's views,
which it also plans to use in place of row-level security.

**The viewer** is `Viewer { node, module }`, supplied by the kernel and
never by the reader:
- A module on the same node: `node` is this node, `module` the calling
  module. The kernel vouches for both.
- Another node: `node` is the peer's identity (§7.4), and `module` is
  empty. The remote node only claims which of its modules is asking, so
  that claim is not trusted. As in SpacetimeDB, the identity (the node,
  i.e. the user) authorizes; a remote node's modules act for the same
  user, as one SpacetimeDB identity's several connections do.

**What is published.** From outside a module (other modules, other
nodes) only **public tables and views** can be read or subscribed to.
A module reads its own tables directly. `#[table(public)]` stays: it is
the trivial view, every row to everyone. Private tables are refused to
other modules for reads and subscriptions; until now only the declared
access was checked locally. The convention: **a module's views are its
API, its tables its implementation**. A view can keep its shape while
the tables behind it change, so dependents don't break on a migration
(§3.3).

**Two kinds**, the same split SpacetimeDB makes between query-builder
views (incremental) and procedural views (re-run).

1. **Filter views.** One table and a row condition:

   ```rust
   #[view(public, from = HttpResponse)]
   fn my_responses(r: &HttpResponse, viewer: &Viewer) -> bool {
       r.owner == viewer.module
   }
   ```

   - The body is a restricted closure: column fields of `r`,
     `viewer.node` / `viewer.module`, literals (strings, integers,
     booleans), `== != < <= > >=`, `&& || !` and parentheses.
   - The macro rejects anything else with an error at that span. It also
     emits the function unchanged, so rustc type-checks the condition.
     It records the condition as an expression in the schema
     (`ViewExpr`), which the kernel evaluates natively.
   - Rows are the table's rows, and the key is the table's key. The
     `viewer` parameter is optional; without it the view is the same for
     everyone (anonymous).
   - A view is a filter view when it takes `&Row` and returns `bool`. The
     row parameter names the table, so `from = Table` is optional; when
     given, it must match.
   - **Cost:**
     - **No copy:** nothing is stored.
     - **No WebAssembly:** the kernel evaluates the condition itself.
     - **Reads:** a scan of the table, filtered.
     - **Deltas:** for each change of the table, the condition is tested
       on the old and new row for each subscriber:
       - true → true is an update;
       - false → true is an insert;
       - true → false is a delete;
       - false → false sends nothing.
2. **Function views.** Arbitrary code with declared reads, like a query:

   ```rust
   #[view(public, key = name)]
   fn leaderboard<Caps>(ctx: ViewContext<Caps>) -> Vec<Entry>
   where Caps: CanRead<Player> { … }
   ```

   - `ViewContext` is a `QueryContext`: read-only and deterministic. An
     optional second parameter `viewer: &Viewer` makes the view per
     viewer. `key = field` names the row type's primary key, which is
     required: without a key, an update can't be told from a delete and
     an insert.
   - **How it is run:** the macro turns the function into an internal
     query (`view_fn_<name>`), so it runs on the query path, against a
     consistent snapshot.
   - **When it is re-run:** whenever a table it declared changes. Its
     result is then compared with the previous one by key, which yields
     the inserts, updates and deletes.

**Caches.**
- Filter views have none.
- A function view keeps one result per (view, viewer) that has
  subscribers. An anonymous view keeps one result shared by all of
  them, which is why anonymous views are preferred.
- The cache is in memory, owned by the kernel, and never persisted. It
  is derived data: the tables and their logs hold the truth and the
  history.
- It is filled when a subscriber appears (a module loads, or a peer
  subscribes) and when the owner loads. It is dropped with the
  subscriber.
- Plain reads of a function view run the function. They don't use the
  cache, so they see the reader's snapshot.

**Visibility.** Views take `public`, `local` or `internal` like
reducers. The default is `public` in a public module and `local` in a
private one. Remote readers need a `public` view in a `Public` module;
owners and the kernel are never limited (§7.4).

**SDK, reader side.** The bindings generate views next to tables, as
read-only handles. Each view gets its own row struct, named after it
(`MyResponses`):
- reads: `ctx.network().views.my_responses().scan()` and `.get(pk)`;
- subscriptions: `#[reducer(on = "network.my_responses.insert")]`;
- replicas: `replicated_tables: ["node.module.my_responses"]`.

Reading is declared like a table (`CanRead<MyResponses>`).

**Kernel.**
- **Schema.** `ModuleSchema.views: Vec<ViewSchema { name, visibility,
  row_type, primary_key, per_viewer, kind: Filter { table, predicate } |
  Function { query, reads } }>` (ABI 15).
  `ModuleSchema::view_table_schema` gives a view the shape of a table,
  which bindings, replicas and table sync reuse.
- **Names.** A view shares the module's table namespace, so the read host
  calls (`TableScan`, `TableGetByPrimaryKey`, `TableIndexScan`) take a
  view name where they take a table name.
- **Scheduling.** Reading a view counts, for the scheduler, as reading
  the tables behind it.
- **Subscriptions.** After each local commit to table `T` of module `M`,
  the kernel derives the events of `M`'s filter views over `T` for each
  subscriber, local or remote, and re-runs the function views (of any
  module) that read `M.T`. Derived events go only to the subscriber
  they were computed for, and after the commit's own events.
- **Replicas and remote subscriptions.** A replica of a view is synced
  and kept current with the peer as viewer.
- **Migrations.** The plan lists added and removed views, like
  reducers: a removed view is "readers may break".

**The network module** is the first user. Its result tables
(`Connection`, `Inbound`, `ConnClosed`, `Resolved`, `HttpResponse`) are
private, published as `local` views `my_connections`, `my_inbound`,
`my_closed`, `my_resolved` and `my_responses`, filtered by `owner ==
viewer.module`. An app no longer sees, or filters out, other apps'
traffic. The browser and http-get subscribe to
`network.my_responses.insert`.

**Status:**
- **No views with arguments yet** (SpacetimeDB's
  `players_at_level(2)`).
- **No column projection, and no joins in filter views.**
- **No time travel on views.** A filter view at time t is simply the
  filter applied to the table at t, but it is not exposed yet.
- **Function views are re-run on any change of a table they read.** No
  finer read-set tracking.
- **Function views over replicas aren't re-run on replica changes.**
- **A function view's events arrive slightly after the commit's own
  events.** It is re-run off the event loop.
- **Reading a function view inside a reducer runs it.** It sees committed
  data, not the reducer's pending writes; filter views see both.

---

## 8. Authorities and default modules

Each authority is held by at most one module per node, claimed at load time
once it is granted (§7.4).

| Authority | Grants | Default module | Surface exposed to other modules |
|---|---|---|---|
| Gpu | ~40 wgpu-like host calls, the `render` event and window creation | `graphics` | Layers, textures, meshes, pipelines and 2D drawing reducers (`draw_rect`, `draw_text`, `draw_image`, …); public `FrameTick` and `SurfaceInfo` tables |
| Input | The `input` event | `input` | Public ephemeral tables `KeyState` (physical code + the layout's logical `key` and typed `text`), `MouseState`, `MouseButton`, `TextInputBuffer`; stateful `KeyboardLayout` (`system` default, `us`, `fr`; reducer `set_keyboard_layout`) |
| Network | TCP, UDP, host-side TLS and system DNS | `network` | Reducers `connect`, `send`, `close`, `resolve`, `http_get`; result tables `Connection`, `Inbound`, `ConnClosed`, `Resolved`, `HttpResponse`, each stamped with an `owner` |
| File | File-system host calls and file-watch events, confined to the node's file root (§7.4) | – (claimed directly) | – |
| Module | Load, unload and remove of modules; decides remote administration requests (§7.4) | `module_manager` | `App` table and `local` app lifecycle reducers |
| Audio | Output and input streams | `audio` (stub) | – |

`crates/interstice-ui` is a library that modules link against (`ui_subsystem!`);
it is not a default module. It also holds the brand (`interstice_ui::theme`:
colour tokens, type scale, baked brand textures; reference `brand/BRAND.md`)
and the UI faces, Space Grotesk and IBM Plex Mono (DejaVu and Droid Sans
Fallback for missing glyphs), which graphics rasterizes from identical bytes.

**Colour space.** Colours given to graphics draw calls are sRGB, as in CSS,
and the 2D compositor blends in sRGB space like a browser, so hex values and
alpha tints look as designed. The engine prefers a plain-unorm swapchain for
this. A texture whose bytes are sRGB (decoded pages, baked brand textures) is
created `rgba8unorm_srgb`; on a unorm target graphics stores it as
`rgba8unorm` and samples it raw. If a platform only offers an sRGB swapchain,
graphics keeps the texture formats and decodes draw colours in the vertex
stage, so colours stay right and only blending shifts to linear light.

**Status:**
- **The Module holder decides administration** (§7.4). Remote load,
  unload and remove requests reach it as `ModuleEvent`s carrying the
  caller and whether it is an owner. `module_manager` carries out owners'
  requests and refuses the rest.
- **Without a Module holder, `load` of an existing module updates it** with a
  migration plan (§3.3) and asks for new authority grants; only owners may
  load, unload or remove.
- **File watchers are never removed.**
- **Keyboard layout comes from the host.** The engine forwards winit's
  `logical_key` and `text` with each `InputEvent::Key` (empty when it can't
  tell); with `KeyboardLayout = "system"` the input module trusts them, and
  only uses its built-in tables when they are empty or a layout is forced.
  A different host may resolve layouts differently (or not at all), so this
  part of the ABI may still change. Dead keys / IME composition are not
  forwarded.
- **Mouse capture is a host concern.** Modules only ever see relative
  `MouseMotion` deltas and draw their own cursor (the desktop, agar), so
  the host's pointer and the module's cursor drift apart unless the host
  pointer is held. The CLI window starts *released*: motion and button
  events are not forwarded (raw device motion arrives even when the window
  is unfocused). A left click inside the window *captures*: the pointer is
  locked (falling back to confined where the platform can't lock) and
  hidden, and that click is swallowed so it doesn't land wherever the
  module's cursor happens to be. **Ctrl+Alt+G** releases (that key press
  is swallowed too), as does losing focus. Keys and the wheel are forwarded
  in both states. The window title says which state it is in. No module
  state is involved, so replay and time travel are unaffected.
- **Wheel direction is a module setting.** The host forwards wheel deltas
  as the OS reports them; the input module's persisted `WheelSettings`
  row (`invert`, default `true`) negates them before they accumulate in
  `MouseState.wheel_delta`, so every consumer agrees. Change it with
  `set_wheel_direction inverted|host`. Scripted browser scrolls bypass it.

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
   requester as an input, like any other event. Remote
   loads (`NodeSelection::Other`) get no result event yet.
3. **Unload.** Tear down subscriptions and replicas, release authorities and set
   `loaded = false`. The data is kept.
4. **Remove.** Unload, then delete the module directory.
5. **Node start.** Loads every persisted module with `loaded = true`, in
   dependency order.

### 9.1 Bundles: a whole node in one file

A bundle is a node you can hand to someone: a set of modules, and
optionally their data, in one file (`.itb`). It might hold the default
modules plus a game, the whole desktop, or a node captured as it is. It
can also become a **program**: a copy of the `interstice` binary with the
bundle appended, which runs that node when started.

**What a bundle holds:**
- A manifest: `name` (the node's name in the registry), `version` and
  `description` (informational), an optional default `port`, and the
  `abi_version` it was built for.
- Modules, each with its wasm, its name, and optionally its **seed data**:
  the files of its module directory (`state.toml`, `schema.bin`,
  `stateful/`, `logs/`, `snapshots/`). The history of logged tables comes
  with it, so time travel works on the shipped data.
- Never the node's identity: no node id, `peer_tokens.toml` or
  `node.log`. Every install is a new node with its own id.

**File format.**
- An `.itb` file is the magic `ITBUNDLE`, a `u32` format version (1),
  then a zstd-compressed, postcard-encoded `Bundle`. zstd is already
  built for wasmtime's cache.
- A program is the `interstice` binary, then the `.itb` bytes, then a
  trailer: the bundle's length as a `u64`, and the magic `ITBEXE01`. At
  start the binary reads its own last 16 bytes. If it finds the trailer it
  runs the bundle; otherwise it behaves as the normal CLI. Making a program
  out of a program replaces the old bundle.

**Making one** (`interstice bundle create <out.itb> ...`):
- `<bundle.toml>`: a manifest whose `modules` list entries like
  `builtin:input` (the default modules shipped in the CLI), a `.wasm` file,
  or a module project directory (built with cargo, like `load`).
- `--node <node> [--with-data]`: the node's loaded modules, as they are
  now. This includes apps the desktop installed at run time. With
  `--with-data` their data comes too. The node must be stopped: its files
  are read directly, and a running node may be in the middle of writing
  them.
- `--example <name>`: the modules an example runs.
- Every module is described at creation time. A module whose ABI differs
  from the CLI's, or a dependency that is missing from the bundle, is an
  error then, not at install.

`interstice bundle inspect <file>` lists the manifest and the modules.
`interstice bundle exe <file.itb> <out>` makes a program.

**Running one** (`interstice bundle run <file> [--name n] [--port p]
[--allow-data-loss|--delete-data]`, or running the program):
- **Fresh install** (no node with that name in the registry): a new node
  is created on `--port`, else the manifest's port, else a free port. The
  modules are installed with their seed data, and the node starts.
  - A module with seed data has a `state.toml`, so it loads as a reload:
    its tables are restored and `init` does not run again.
  - A module without seed data runs `init`.
- **Update** (the node exists): every module in the bundle is loaded over
  the node's data through the §3.3 migration plan, with the same rules and
  the same prompt as `load`.
  - Seed data is used only for modules that the node does not have yet. A
    module the node already has keeps its own data, and is migrated rather
    than overwritten.
  - **Abort** keeps the module's old version, from its `module.wasm` on
    disk.
  - Modules the node has that are not in the bundle stay as they are
    (apps the user installed, for example).
  - A module that the previous install of this bundle had and the new one
    drops is **unloaded, not removed**: its data stays, and `interstice
    remove` deletes it.
- Bundle modules are always loaded, even if one was unloaded by hand
  before: the bundle defines the node.
- `--delete-data` removes the node and its data, then installs fresh,
  seed data included.
- The node is then an ordinary node: `interstice node start`, `load` and
  `call_reducer` all work on it by name. `interstice node remove <name>`
  uninstalls it.
- The install records `bundle.toml` in the node directory: the bundle's
  name and version, and the modules it installed. An update reads it to
  find the modules that were dropped.
- The run refuses if the node is already running.

**In core:**
- `interstice_core::bundle` holds the format, and reading modules and
  data from a node directory.
- `describe_wasm` reads a module's schema without a node: every import is
  stubbed as a trap, since `interstice_describe` calls no host function.
- `Node::start_with(modules, decide)` is the one start path:
  - it merges the persisted modules with the given ones (the given wasm
    wins) and loads them all in dependency order;
  - each given module goes through `load_module_with(Safe)`;
  - on `MigrationRequired(plan)` it calls `decide(module, plan)` and
    retries with the policy returned, or keeps the old version on
    `Safe`.
- `Node::start(&[..])` and `example` use the same path with no decision
  (a plan that asks fails the start). An example is an in-memory bundle
  that is always installed fresh.

**Status:**
- No signing or checksums: a bundle is trusted like a module you `load`.
- Modules of the bundle are not checked against modules already on the
  node that depend on them (§3.3).
- A program carries the whole CLI, with every example embedded: about
  185 MB in a release build, before the bundle. The desktop as a program is
  209 MB; its bundle alone is 24 MB, zstd-compressed from 71 MB. A smaller
  runtime-only binary is future work.

---

## 10. ABI, SDK and CLI

**ABI** (`crates/interstice-abi`, `ABI_VERSION = 16`, loads `MIN_ABI_VERSION = 15` and up):
- **Compatibility.** A host loads any module whose ABI is in
  `MIN_ABI_VERSION..=ABI_VERSION`. A version that only adds host imports or
  optional fields keeps the minimum, so existing modules keep running. Only
  a breaking change raises the minimum and forces a rebuild.
- **Call failures (v16).** The SDK's panic hook passes the message to
  `interstice_fail` before the trap, and a reducer's `Err` panics with its
  message. The caller gets `CallFailed(message)` instead of the trap and its
  backtrace. A v15 module still reports the trap.
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
| Host imports, fast path | `interstice_log`, `interstice_fail` (v16), `interstice_time`, `interstice_random`, `interstice_insert_row`, `interstice_update_row`, `interstice_delete_row`, `interstice_clear_table`, `interstice_get_by_pk`, `interstice_blob_create`, `interstice_blob_len`, `interstice_blob_read` |
| Arguments in | `(RawReducerContext or RawQueryContext, IntersticeValue::Vec(args))` |
| Types | `IntersticeType`: Void, U8, U32, U64, I32, I64, F32, F64, Bool, String, Vec, Option, Tuple, Named, Blob. Structs and enums appear as `Named` with type definitions in the schema. |

**SDK** (`interstice-sdk`, `-sdk-core`, `-sdk-macros`):
- Attribute macros: `#[table]`, `#[reducer]`, `#[query]`, `#[interstice_type]`, `#[init]`.
  `#[reducer(public|local|internal)]` and `#[query(…)]` set who may call
  a function (§7.4).
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
  files; `scripts/build_modules.sh` refreshes those. An example is an in-memory
  bundle (§9.1).
- `bundle create|inspect|run|exe`: whole nodes as files or programs (§9.1).
- `call_reducer` waits for the `ReducerResult` (§7.1) and reports
  "committed" or the error.
- `call_reducer` and `call_query` type their arguments from the node's schema
  (`cli/src/args.rs`): plain text for numbers, booleans and strings, JSON for
  structs, enums, lists, options and tuples. A wrong count or type is
  reported against the signature.
- The CLI talks to nodes only through `interstice-client`.

**Embedding** (`interstice_core::NodeHandle`): `Node::start_headless(modules)`
boots everything but the window (config, network, runtime loop, modules) and
returns once the modules are loaded. The handle's `call_reducer` resolves when
the call has committed, `submit_reducer` queues without waiting (for many calls
in flight), and `call_query` runs a query. Calls go through the scheduler as
the kernel would, so visibility does not apply. The benchmark suite drives
nodes this way, and so would a host that runs Interstice as its kernel.

**Clients** (`crates/interstice-client`): programs that are not modules
connect with their own key over mutual TLS, like a node.
- `call_reducer` waits for the `ReducerResult`.
- `submit_reducer` pipelines calls, sent in order, each resolving to its
  outcome.
- `call_query` and `schema`.
- One task routes replies by request id. Another writes packets and
  flushes only when its queue is empty, so pipelined calls share TLS
  records. The node's writer for each peer batches flushes the same way.

**SDK macros and `macro_rules!`**: a type that reaches a macro through a `$t:ty`
fragment is wrapped in an invisible group. Every attribute macro unwraps those
first, so generated tables and reducers type-check like hand-written ones.

**Benchmarks** (`crates/interstice-bench`, `modules/benchmarks/bench`): see
`benchmarks/README.md`.
- The micro matrix is SpacetimeDB's `crates/bench`:
  - three targets: the raw table store, the module called in process, SQLite;
  - schemas `u32_u64_str` and `u32_u64_u64`; indexes `unique_0` and
    `btree_each_column`; memory or disk;
  - empty, insert_bulk, update_bulk, iterate and filter, one transaction per
    sample;
  - Criterion.
- The keynote transfer load: Zipf-picked account pairs and many calls in
  flight. Only committed calls count, at the client, per second, with p50, p95
  and p99 latency, and the total balance is checked afterwards.

### 10.1 SDK types and errors

All three known SDK bugs are fixed.
- **Types are converted at compile time** (`sdk-macros/src/types.rs`). Table
  fields, reducer and query parameters, query results, and struct and enum
  fields map straight to `IntersticeType`. A type that cannot cross the
  boundary is a compile error at the type, with what to use instead:
  - `u16`, `usize`, `i8`… → the supported integer sizes;
  - `&str` or `&[T]` → owned values;
  - arrays → `Vec`;
  - maps → `Vec` of tuples;
  - other generics.

  The macros then emit only those errors, without glue code that would add
  errors of its own (`crates/interstice-sdk/tests/ui`). This replaces parsing
  type strings when the module describes itself, which turned `u16` into
  `Named("u16")` and panicked on paths.
- **Named-field enum variants** keep their names. The payload is a struct type
  `Enum::Variant` in the schema, values use that name (the old bare `Variant`
  still decodes), and bindings rebuild `Variant { field: T }`. Such enums can
  now be stored in tables; their values used to fail validation, because the
  schema said tuple while values were structs.
- **`[node].[module].[table].sync` subscriptions compile.** The arm called
  `to_string` on a `ModuleSelection`.

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

Transactions are deterministic given their inputs (§6.5). Logged tables keep
their history with transaction ids and timestamps, and can be read and
restored at any point (§6.7).

Open: restores that re-run later reducers with a change (re-simulation, which
would need the reducer inputs logged too), and a module-wide restore call
(today: restore each table to the same `Tx`, which is consistent, §6.7).

### 11.5 Schema evolution

Automatic migrations are designed in §3.3. Open: a `migrate` hook for
transformations the plan can't express, and checking dependents (local and
remote) before an update.

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
| Time travel | `hello` + `notes` | undo, redo, branches and restore of a logged table's history, driven from a UI |

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

**Notes (time travel from a UI).** `modules/examples/notes` is a front-end
with no table of its own beyond its UI. It binds to `hello-example` (a local
module dependency):
- It reads `Notes` through the binding, and the history through the hello
  query `note_history() -> NoteHistory{head, commits: [NoteCommit{id, parent}]}`.
  That query derives the tree of ordinary commits and their parents from the
  table's log (§6.7).
- It calls `note`, `erase`, `undo`, `redo` and `restore(id)`.
- `Notes` is an ordinary logged table. Undo, redo and a click on a commit chip
  are all restores, so the history survives restarts.
- The UI is rebuilt only when a signature of the notes and history changes.
  The signature is stored in a `NotesView` row.

**Window manager state** is in desktop tables: `Window` (rect, focus order,
surface), `DesktopState` (launcher, focus, pointer capture,
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
- [x] **Time travel** (§6.7): logged tables keep their log as history (with
      `tx` and timestamps, in segments and checkpoints, `history` retention);
      `commits` / `rows_at` / `restore`. It replaced an in-memory recorder and
      per-table timelines, which duplicated what the log already holds.
- [x] **Scans without copies**: a scan or index scan with no pending writes
      over it encodes its reply straight from the table's rows (borrowed,
      the same postcard bytes), instead of cloning every row first. The host
      side of a 256-row scan went from 33 to 18 µs, and of an index lookup
      of ~256 rows from 59 to 21 µs. Streaming rows to the guest one at a
      time would need the typed value encoding (§10, proposal).

### 13.2 Storage
- [x] Persist `auto_inc` counters: blocks of ids with a saved ceiling (§3.2).
- [x] Sync stateful row files and their directories; the ~10 ms durability
      window is documented (§3.2).
- [x] Schema versioning and automatic migrations (§3.3), SpacetimeDB-style.
- [x] Remove dead `PersistenceConfig` / `LogRotator` (and the other dead code
      the compiler warned about).

### 13.3 Security and distribution
- [x] Visibility checks on reducer/query calls, subscriptions, table sync and
      schema requests; owners; localhost by default; administration and
      authority grants decided by the Module holder, else by the kernel
      (§7.4).
- [x] Key-pair identities (self-certifying ids), mutual TLS 1.3 between
      peers, admission by the Module holder's `connect` handler (§7.4),
      after SpacetimeDB's identity model.
- [x] Views (§7.5): filter and function views, per viewer or anonymous;
      private tables closed to other modules.
- [ ] Key rotation; views with arguments.
- [ ] Authority model for the "OS" use case: which module may grant which
      authority to whom; per-user identities.
- [x] Replica writes and remote queries through the scheduler (§6.3: replica
      jobs; queries pin a snapshot).
- [x] Row-level visibility (e.g. per-app filtering of `network` results,
      which today rely on an `owner` column filtered client-side).
- [x] Bundles (§9.1): a node's modules and data in one `.itb` file, or as
      a program; install and update go through the §3.3 migration plan.

### 13.4 SDK / tooling
- [x] The SDK bugs of §10.1, and compile errors at the offending type.
- [x] Generated read methods return `Result` instead of panicking: `scan`,
      `get`, `scan_by_*`, `get_by_*` (local and bindings) go through
      `sdk-core/reads.rs`; row decode is `TryFrom<Row>`; the panicking
      `IntoIterator` on table handles is gone. Reducers may return
      `Result<(), E>` and queries `Result<T, E>`; `Err` aborts the call.
- [x] A shorthand for "full access to these tables": `CanAll<T>`, tuple rows
      in `where` clauses, and `caps!` named sets (§5). The browser's
      engine reducers share `BrowserCaps`.
- [x] Benchmark suite (`benchmarks/README.md`): SpacetimeDB's micro matrix
      (raw / module / SQLite) and keynote transfer load, a phase breakdown
      (§6.8), results recorded.
- [x] Reducer results for network callers (§7.1); `interstice-client` for
      programs that are not modules; headless embedding (`NodeHandle`).
- [x] CI (`.github/workflows/ci.yml`), on every push and pull request:
  - every module built for WASM;
  - all workspace tests, including the SDK compile-error tests;
  - a benchmark smoke run;
  - the browser tests on a virtual display (informational).
- [x] Benchmark comparison (`.github/workflows/bench.yml`): the base commit and
      the head on the same runner, compared by Criterion and summarised in
      the job. Run by hand, or by labelling a PR `bench`.
- [ ] Optimise from the breakdown: per-host-call and per-row boundary cost,
      one log append per transaction, row-level conflicts.
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
      requirements, time travel in `notes`,
      load-failure reports.
- [x] **Brand** (`brand/BRAND.md`, from the user's brand board): one theme
      in `interstice_ui::theme`, Space Grotesk + IBM Plex Mono in the
      engine, exact-size glyph atlases, sRGB colour space (§8). Applied to
      the desktop (wallpaper hero, top bar, windows, launcher, cursor),
      notes, the UI demo (a component sheet), agar, the browser chrome and
      the graphics example.
- [ ] Polish audio into a short, readable showcase of one feature.
