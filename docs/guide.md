# Writing modules

A module is a Rust crate compiled to WebAssembly. It declares **tables**
(its data), **reducers** (functions that change the data, in transactions),
**queries** (read-only functions that return values) and **views** (read-only
relations other modules and nodes can read and subscribe to). The engine runs
it in a sandbox, persists its tables, schedules its reducers and enforces
what it may touch.

For the design behind each section, see [DESIGN.md](../DESIGN.md).

- [Set up](#set-up)
- [Tables](#tables)
- [Types](#types)
- [Reducers](#reducers)
- [Queries](#queries)
- [Capabilities](#capabilities)
- [Who may call what](#who-may-call-what)
- [Events and subscriptions](#events-and-subscriptions)
- [Time, randomness and scheduling](#time-randomness-and-scheduling)
- [Views](#views)
- [Other modules and nodes](#other-modules-and-nodes)
- [Bulk data](#bulk-data-blob)
- [Time travel](#time-travel)
- [Build, load and call](#build-load-and-call)
- [From outside: clients and embedding](#from-outside-clients-and-embedding)

## Set up

```bash
rustup target add wasm32-unknown-unknown
interstice init my-module && cd my-module
```

`init` writes a hello-world module:
- `Cargo.toml`, with `crate-type = ["cdylib"]` and `interstice-sdk`;
- `build.rs`, which generates bindings;
- `.cargo/config.toml`, which builds for `wasm32-unknown-unknown`;
- `src/lib.rs`.

`src/lib.rs` starts with the module declaration:

```rust
use interstice_sdk::*;

interstice_module!(visibility: Public, authorities: [Gpu, Input]);
```

- **The name** is the crate's name in `Cargo.toml`.
- **`visibility`**: `Public` modules can be reached from other nodes,
  `Private` ones (the default) only from modules on the same node.
- **`authorities`** claim privileged host functions (the GPU, input, audio,
  files, the network, module management). See
  [security.md](security.md#authorities).

## Tables

```rust
#[table]
pub struct Message {
    #[primary_key(auto_inc)]
    pub id: u64,
    #[index(btree)]
    pub sent_at: u64,
    #[index(hash, unique)]
    pub slug: String,
    pub text: String,
}
```

- **`#[primary_key]` is required.** `auto_inc` assigns integers: insert with
  `0` and read the assigned value from the returned row. An id is never
  handed out twice, even across restarts.
- **`#[index(hash)]` and `#[index(btree)]`** add secondary indexes, with
  `unique` to enforce uniqueness. A btree index also answers ranges.
- **Persistence:** one keyword per table.

  | Keyword | Storage | After a restart | For |
  |---|---|---|---|
  | (none): logged | an append-only log, which is the table's history | checkpoint + replay | authoritative data, anything to rewind |
  | `stateful` | one file per row | read back | large rows that change often |
  | `ephemeral` | memory only | empty | caches, session state |
  | `event = N` | memory, the `N` newest rows | empty | results, messages, notifications |

  Commits are acknowledged before they reach the disk. A crash can lose about
  the last 10 ms of commits.
- **Visibility:** `#[table(public)]` lets other modules and nodes read and
  subscribe to the table. Tables are private by default, and other modules
  can then only read them through [views](#views).
- **History:** `#[table(history = "7d")]` bounds how long a logged table's log
  is kept (default `"all"`).

Generated methods on `ctx.current.tables.<table>()`:
- `insert(row)`, which returns the inserted row (with its generated values);
- `update(row)` (by primary key), `delete(pk)`, `clear()`;
- `get(pk)`, `scan()`, and per index `scan_by_<field>_eq(v)` / `get_by_<field>(v)`.

All of them return `Result`; nothing panics.

**Changing a table** later is a migration. The engine diffs the old and new
schemas when the module is loaded:
- **Additive changes** apply at once: a new column with
  `#[default(...)]`, a new table, a new index.
- **A change that loses data** asks first (`interstice load` prompts).
- **A change that can't be applied** is refused, with the reason.

## Types

Columns and arguments can be:
- `u8`, `u32`, `u64`, `i32`, `i64`, `f32`, `f64`, `bool`, `String`;
- `Vec<T>`, `Option<T>`, tuples;
- structs and enums declared with `#[interstice_type]`.

```rust
#[interstice_type]
pub enum Shape {
    Empty,
    Circle(f64),
    Rect { width: u32, height: u32 },
}
```

A type that can't cross the module boundary is a compile error at that type,
with what to use instead: `u16` → `u32`, `&str` → `String`, `[T; N]` →
`Vec<T>`, a map → `Vec<(K, V)>`. A `#[table]` struct is also a type.

## Reducers

A reducer is a transaction: all of its writes commit together, or none do.

```rust
#[reducer]
pub fn post<Caps>(ctx: ReducerContext<Caps>, slug: String, text: String) -> Result<(), String>
where
    Caps: CanInsert<Message>,
{
    if text.is_empty() {
        return Err("empty message".into());
    }
    let sent_at = ctx.time_now_ms()?;
    ctx.current.tables.message().insert(Message { id: 0, sent_at, slug, text })?;
    Ok(())
}
```

- **The first parameter is the context.** `ReducerContext<Caps>`, where
  `Caps` declares the tables the reducer touches (see
  [Capabilities](#capabilities)).
- **Returning `Err`** (any `E: Display`) aborts the reducer: none of its
  writes commit, and the caller gets the message.
- **Reads see the reducer's own pending writes.**
- **Calls to other reducers are messages.** They run after the caller
  commits, each as its own transaction, in the order they were made, and
  never if the caller fails. So a caller can't read a callee's effects in the
  same reducer; react to them with a subscription instead.
- **Calls to queries** run at once, and see the caller's pending writes.

## Queries

```rust
#[query]
pub fn recent<Caps>(ctx: QueryContext<Caps>, limit: u32) -> Result<Vec<Message>, String>
where
    Caps: CanRead<Message>,
{
    let mut rows = ctx.current.tables.message().scan()?;
    rows.sort_by_key(|m| std::cmp::Reverse(m.sent_at));
    rows.truncate(limit as usize);
    Ok(rows)
}
```

A query reads one consistent snapshot and returns a value. It may call other
queries, but not reducers.

## Capabilities

Every table a function touches is declared in its signature, so the engine
knows each function's reads and writes before running it. That is what lets
reducers that don't conflict run in parallel.

- `CanRead<T>`, `CanInsert<T>`, `CanUpdate<T>`, `CanDelete<T>`.
- `CanAll<T>`: all four.
- Tuples: `CanRead<(A, B)>`.
- Named sets, for reducers that share one:

  ```rust
  caps! {
      pub struct AppCaps {
          all: [Session, Tab],
          read: [MouseState],
          write: [Log],   // insert + update + delete
      }
  }

  #[reducer]
  pub fn step(ctx: ReducerContext<AppCaps>) { /* … */ }
  ```

Narrow bounds are better where they fit: two reducers conflict only if they
touch the same table and one of them writes it.

## Who may call what

| Visibility | Callable by |
|---|---|
| `public` | anyone connected, from any node |
| `local` | modules on the same node |
| `internal` | the module itself |

- **The default:**
  - event handlers (`on = "…"`) are `internal`;
  - other functions are `public` in a `Public` module, and `local` in a
    `Private` one.
- **Override** it per function: `#[reducer(local)]`, `#[query(public)]`.
- **The node's owners**, and the engine itself, may call anything.

The caller is in the context: `ctx.caller_node_id`, and `ctx.caller_module_name`
for a module on the same node. A remote caller is identified by its node only:
its key is proven, and the module it names isn't.

## Events and subscriptions

A reducer can run on an event instead of being called:

```rust
#[reducer(on = "message.insert")]            // a table of this module
fn on_post(ctx: ReducerContext, row: Message) { /* … */ }

#[reducer(on = "hello.greetings.update")]    // another module's table or view
fn on_greeting(ctx: ReducerContext, old: Greetings, new: Greetings) { /* … */ }

#[reducer(on = "hello-node.hello.greetings.delete")]   // another node's
fn on_gone(ctx: ReducerContext, row: Greetings) { /* … */ }
```

- **Table events:** `insert` and `delete` pass the row; `update` passes the
  old and the new row.
- **`sync`** fires when a [replicated table](#other-modules-and-nodes)
  finished its initial copy.
- **Engine events:**
  - `init`: the module's first load ever;
  - `load`: every load;
  - `connect` and `disconnect`: with the peer's `node_id: String`;
  - authority events, such as `render` and `input`.

## Time, randomness and scheduling

- **`ctx.time_now_ms()`** is fixed for the whole transaction, and never
  decreases from one transaction to the next.
- **`ctx.deterministic_random_u64()`** draws from a seed fixed by the
  transaction.

So a reducer is reproducible from its inputs.

`ctx.schedule("tick", 1000)` runs the module's reducer `tick` (which takes
only the context) 1000 ms after this transaction commits. It doesn't run if
the transaction fails. A reducer can reschedule itself for periodic work.

## Views

A view is a read-only relation a module publishes: other modules and nodes
read it, subscribe to it and replicate it like a table, and each reader may
see different rows.

**Filter views** are one table and a condition, evaluated by the engine:

```rust
#[view(local)]
fn my_responses(r: &HttpResponse, viewer: &Viewer) -> bool {
    r.owner == viewer.module
}
```

- **The condition** may use the row's fields, `viewer.node` and
  `viewer.module`, literals, comparisons, `&&`, `||` and `!`. Anything else
  is a compile error at that spot.
- **Without a `viewer` parameter**, everyone sees the same rows.

**Function views** are arbitrary read-only code:

```rust
#[view(public, key = name)]
fn leaderboard<Caps>(ctx: ViewContext<Caps>) -> Vec<Entry>
where
    Caps: CanRead<Player>,
{ /* … */ }
```

- **When it re-runs:** when a table it reads changes.
- **Events:** readers get the difference by `key`.

A reader uses a view like a table:
- `ctx.network().views.my_responses().scan()`;
- `#[reducer(on = "network.my_responses.insert")]`.

The engine fills in `Viewer`, so a reader can't pretend to be someone else.
Private tables can only be read by other modules through views. This is how
the `network` module gives every app only its own results.

## Other modules and nodes

Dependencies are schema files in `src/bindings/`, fetched from a running node:

```bash
interstice bindings add module <node> <module>   # a module on the same node
interstice bindings add node <node>              # another node
```

`build.rs` turns them into typed handles:
- `ctx.hello().reducers.hello("you".into())?`;
- `ctx.hello().queries.list()?`;
- `ctx.hello().tables.greetings().scan()?`.

**Replication:** `interstice_module!(replicated_tables: ["node.module.table"])`
keeps a local, read-only copy of another node's public table or view, which
reducers read like their own.

## Bulk data: `Blob`

To pass a large buffer (pixels, audio) to another module, take a `Blob`
parameter instead of a `Vec<u8>`:
- `Blob::new(&bytes)` copies the bytes once into the host;
- callees read the blob or pass it on, and the GPU authority uploads straight
  from it;
- a blob lives as long as the call that made it and the calls it issued, so it
  can't be stored in a table.

## Time travel

A logged table's log is its history.
- `commits(limit)` lists the transactions that changed the table.
- `rows_at(HistoryPoint::Tx(id) | Time(ms) | Back(n))` reads a past state.
- `restore(point)` writes a past state back as a new transaction. Undo and
  redo are restores.

Restoring several tables to the same transaction gives a consistent state.
See the `notes` example.

## Build, load and call

```bash
cargo build --release                              # target from .cargo/config.toml
interstice node create dev 8100
interstice node start dev                          # in another terminal
interstice load dev .                              # builds and loads the crate in .
interstice call_reducer dev my-module post hi "hello there"
interstice call_query dev my-module recent 10
```

- **Arguments are typed from the function's signature.** Plain text for
  numbers, booleans and strings; JSON for structs, enums, lists and options,
  for example `'{"width": 2, "height": 3}'` or `'{"Rect": {…}}'`.
- **`call_reducer`** prints `committed`, or the error the reducer returned.
- **`load` asks before a migration that loses data**, and before granting
  authorities.

See [cli.md](cli.md) for every command.

## From outside: clients and embedding

**`interstice-client`** is for a program that isn't a module (a game client, a
tool, a test). It connects to a node with its own key:

```rust
let identity = LocalIdentity::load_or_create("client.key".as_ref())?;
let client = Client::connect("127.0.0.1:8100", &identity, &KnownPeers::load(None)).await?;
client.call_reducer("my-module", "post", IntersticeValue::Vec(vec![
    IntersticeValue::String("hi".into()),
    IntersticeValue::String("hello".into()),
])).await?;                                        // Ok once committed
let pending = client.submit_reducer(/* … */);       // many calls in flight
let rows = client.call_query("my-module", "recent", IntersticeValue::Vec(vec![10u32.into()])).await?;
```

**Embedding** runs a node inside your own program, without a window:

```rust
let node = Node::new(data_dir, 8100, "127.0.0.1:8100".into())?;
let handle = node.start_headless(&[&wasm]).await?;
handle.call_reducer("my-module", "post", args).await?;
```

The handle calls as the engine itself: no visibility checks, and no network.
