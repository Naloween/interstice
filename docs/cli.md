# The `interstice` command

`interstice help` lists every command. A node is named by its registry name or
its id. The registry, the CLI's key and local nodes' data live in the OS data
directory (`~/.local/share/interstice` on Linux):

```
nodes.toml                the registry: names, addresses, ids
cli_identity.key          this CLI's key; its id is shown by `interstice identity`
cli_identity.chain        its key handovers, once rotated
known_peers.toml          the newest key handover seen from each node
nodes/<id>/               a local node: node.toml, identity.key, identity.chain, node.log, modules/
```

## Nodes

| Command | What it does |
|---|---|
| `node create <name> <port> [--expose]` | Create a local node. It listens on localhost unless `--expose`. |
| `node start <name>` | Run it; this CLI becomes one of its owners. |
| `node add <name> <address>` | Register a remote node. |
| `node list`, `node show <name>` | The registry. |
| `node rename <old> <new>` | Rename a registry entry. |
| `node remove <name>` | Remove the entry; for a local node, **also delete its data**. |
| `node ping <name>` | Connect, check the node's key and record its id. |
| `node schema <name> [out]` | Print or save the schema the node shows you. |
| `node expose <name> on\|off` | Listen on every interface, or on localhost only. |
| `node owner add <name> <id>`, `node owner list <name>` | Owners may administer the node and call anything. |
| `node rotate-key <name>` | Move a local node to a new key; its id stays ([security](security.md#rotating-a-key)). |
| `identity` | This CLI's id, to make it an owner of another node. |
| `identity rotate` | Move this CLI to a new key; its id stays. |

## Modules

| Command | What it does |
|---|---|
| `init` | Start a module project in the current directory. |
| `load <node> <project> [--grant] [--allow-data-loss\|--delete-data]` | Build the project and load or update it. |
| `unload <node> <module>` | Stop the module and keep its data; loading it again resumes. |
| `remove <node> <module>` | Stop it and delete its data. |
| `bindings add module <node> <module> [project]` | Write a module's schema into `src/bindings/`. |
| `bindings add node <node> [project]` | Write a node's schema, for calls to another node. |

**When `load` asks.** An update whose migration would lose data asks first;
`--allow-data-loss` answers yes in advance, and `--delete-data` starts over
empty. A module that claims authorities asks before they are granted;
`--grant` answers yes. Without a terminal the answer is no, and nothing
changes on the node.

If the node runs a module manager (it holds the `Module` authority), loads go
to it and the CLI prints `forwarded`. `module_manager` accepts loads from
owners.

## Calls

```bash
interstice call_reducer <node> <module> <reducer> [args...]
interstice call_query   <node> <module> <query>   [args...]
```

- **Arguments are typed** by the function's signature in the node's schema:
  - plain text for numbers, booleans and strings;
  - JSON for the rest: `'{"x":1,"y":2}'`, `'"Unit"'`, `'{"Variant": 3}'`,
    `'[1,2,3]'`, `null`.

  A wrong count or type is reported with the signature.
- **`call_reducer`** waits for the call and prints `committed`, or the error
  the reducer returned.
- **`call_query`** prints the result.

## Examples

`interstice example <name>` recreates and starts a node with an example's
modules. The default modules are part of the CLI; an example's own modules
are downloaded from the GitHub release of the CLI's version the first time
(and cached), or taken from `dist/examples/` when the CLI is built from a
checkout after `scripts/build_modules.sh`:

| Example | Port | What it shows |
|---|---:|---|
| `desktop` | 8089 | The showcase: a window manager made of modules, which launches the others as apps. |
| `hello` | 8080 | Tables, reducers, queries, time travel. |
| `caller` | 8081 | Calls and subscriptions to another node (start `hello` first). |
| `notes` | 8092 | A UI over `hello`'s notes, with undo, redo and rewind. |
| `browser` | 8091 | A web browser: Blitz layout and Boa JavaScript, over the shared `network` module. |
| `http-get` | 8090 | Fetching through the `network` module, reading the results through a view. |
| `graphics`, `audio`, `ui` | 8082, 8083, 8088 | The GPU, audio and UI layers. |
| `agar-server`, `agar-client` | 8086, 8084 | A small multiplayer game across two nodes. |

Stop a running example (Ctrl+C) before starting it again.

## Bundles

A bundle is a whole node, its modules and optionally their data, in one
`.itb` file:

```toml
# bundle.toml
name = "my-app"
version = "1.0.0"
port = 8200
modules = ["builtin:input", "builtin:graphics", "../my-module", "extra.wasm"]
```

| Command | What it does |
|---|---|
| `bundle create <out.itb> <bundle.toml>` | Build the listed modules into a bundle. |
| `bundle create <out.itb> --node <node> [--with-data]` | A stopped node, as it is. |
| `bundle create <out.itb> --example <name>` | An example. |
| `bundle inspect <file>` | Its manifest and modules. |
| `bundle run <file> [--name n] [--port p]` | Install it as a node, or update that node, and start it. |
| `bundle exe <file.itb> <out>` | A program that runs the bundle when started. |

Updating a node from a newer bundle goes through the same migrations as
`load`.

## Benchmarks

See [benchmarks/README.md](../benchmarks/README.md).
