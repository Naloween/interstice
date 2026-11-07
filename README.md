<p align="center"><img src="brand/wordmark.svg" alt="interstice" width="420"></p>
<p align="center"><em>A space for things to interact.</em></p>

# Interstice

Interstice runs sandboxed WebAssembly **modules** that cooperate through typed
tables and transactional reducers, on one machine or across many. It is built
like an operating system:
- **The engine is the kernel.** It stores tables, schedules reducers in
  parallel when they don't conflict, enforces who may call and read what, and
  proves who is calling.
- **Everything else is a module**, from the drivers (GPU, input, audio, files,
  network) to the module manager and the apps.

- **Tables** are typed, indexed and persistent. Logged tables keep their
  history, so state can be read at, and restored to, any past transaction.
  Schemas migrate automatically.
- **Reducers** are atomic, deterministic transactions. They react to table
  changes, input, timers and other nodes.
- **Views** publish read-only relations that each reader may see differently.
- **Nodes** talk over mutual TLS. Their identities are their keys, and the
  security model follows SpacetimeDB's.
- **Bundles** ship a whole node (modules and data) as one file, or as a
  program.

## Install

From crates.io:

```bash
cargo install interstice-cli
```

On Linux this needs the ALSA headers (`sudo apt install libasound2-dev
pkg-config` on Debian and Ubuntu).

Prebuilt binaries for Linux, macOS and Windows are attached to the
[GitHub releases](https://github.com/Naloween/interstice/releases).

From a checkout (built modules are not in git):

```bash
rustup target add wasm32-unknown-unknown
scripts/build_modules.sh               # the default modules and the examples
cargo build -p interstice-cli --release
```

`cargo test --workspace` also builds the browser example natively, which
needs `libfontconfig1-dev`.

## Try it

```bash
interstice example desktop     # a desktop made of modules; click Apps
interstice example hello       # a minimal module
interstice example caller      # with hello running: calls across nodes
```

An example's modules are downloaded from the release the first time.

## Write a module

```bash
rustup target add wasm32-unknown-unknown
interstice init counter && cd counter
```

```rust
use interstice_sdk::*;

interstice_module!(visibility: Public);

#[table]
pub struct Counter {
    #[primary_key]
    pub name: String,
    pub value: u64,
}

#[reducer]
pub fn bump<Caps>(ctx: ReducerContext<Caps>, name: String) -> Result<(), String>
where
    Caps: CanRead<Counter> + CanInsert<Counter> + CanUpdate<Counter>,
{
    let counters = ctx.current.tables.counter();
    match counters.get(name.clone())? {
        Some(mut c) => {
            c.value += 1;
            counters.update(c)?;
        }
        None => {
            counters.insert(Counter { name, value: 1 })?;
        }
    }
    Ok(())
}
```

```bash
interstice node create dev 8100 && interstice node start dev   # keep running
interstice load dev .                                          # in another terminal
interstice call_reducer dev counter bump clicks                # → committed
```

## Performance

For reference, on an i7-1360P laptop (16 threads, Linux, release build,
2026-11-07, version 0.6.0). The methods are SpacetimeDB's, so the numbers
compare with theirs; [benchmarks/README.md](benchmarks/README.md) has the
full matrix, how to run it and the history.

| Benchmark | Interstice | SQLite (in memory) |
|---|---:|---:|
| Empty reducer call | 8.1 µs | – |
| Insert 256 rows in a reducer | 0.59–0.84 ms | 64–273 µs |
| Update 256 rows by id | 0.81–0.91 ms | 76–77 µs |
| Scan 256 rows | 37–57 µs | 14 µs |
| Index lookup, ~256 rows out of 2048 | 47–69 µs | 34 µs |

SpacetimeDB's transfer load (100 000 accounts, 2 560 calls in flight, every
call committed and confirmed):

| Mode | Transfers/s | p50 latency |
|---|---:|---:|
| In process | 142 000 | 17 ms |
| Over TLS, 64 connections | 94 000–99 000 | 23 ms |

The tables alone match SQLite; inside a module, the cost per row of crossing
the WASM boundary dominates. Transfers run one at a time because they all
write one table: SpacetimeDB reports about 280 000/s for this load on an
i9-14900K.

## Documentation

| | |
|---|---|
| [docs/guide.md](docs/guide.md) | Writing modules: tables, reducers, queries, views, events, capabilities, calls between modules and nodes, clients |
| [docs/cli.md](docs/cli.md) | Every `interstice` command: nodes, modules, calls, examples, bundles |
| [docs/security.md](docs/security.md) | Identities, owners, visibility, authorities |
| [benchmarks/README.md](benchmarks/README.md) | Benchmarks comparable to SpacetimeDB's, and the latest results |
| [DESIGN.md](DESIGN.md) | How the engine works, design decisions, and the roadmap (§13) |
| [CHANGELOG.md](CHANGELOG.md), [RELEASING.md](RELEASING.md) | Releases |
| [brand/BRAND.md](brand/BRAND.md) | The visual identity, implemented in `interstice_ui::theme` |

## Repository

| Path | What it holds |
|---|---|
| `crates/interstice-core` | The engine: runtime, scheduler, tables, persistence, network, security |
| `crates/interstice-abi`, `interstice-abi-macros` | The types and calls shared by the engine and modules |
| `crates/interstice-sdk`, `-sdk-core`, `-sdk-macros` | The Rust SDK for modules |
| `crates/interstice-client` | A client library for programs that aren't modules |
| `crates/interstice-cli` | The `interstice` command |
| `crates/interstice-ui` | A UI engine for modules |
| `crates/interstice-bench` | The benchmark suite |
| `modules/defaults` | The default modules: `graphics`, `input`, `audio`, `network`, `module_manager` |
| `modules/examples` | Example modules, from `hello` to the desktop and the browser |
| `modules/benchmarks` | Benchmark workloads |
| `scripts`, `vendor` | Build and benchmark scripts; patched dependencies |

## License

MIT; see [LICENSE](LICENSE).
