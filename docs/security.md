# Security

The engine is the **kernel** of an operating system made of modules: it
proves who is calling, decides what each call may reach, and confines what
each authority can touch. *Decisions*, such as loading a module, granting an
authority or admitting a peer, belong to the module that holds the `Module`
authority (`module_manager` by default). Without one, the kernel decides from
the node's owners and asks at the CLI. The model follows SpacetimeDB's; the
details are in [DESIGN.md §7.4](../DESIGN.md#74-security-the-kernel-and-the-module-manager).

## Identities

Every node and every CLI has its own key pair (`identity.key`,
`cli_identity.key`), and **its id is derived from its public key**, so no one
can claim an id without the key.
- Connections use mutual TLS 1.3: both sides prove their key, and nothing
  secret crosses the wire.
- A reducer sees its caller as `ctx.caller_node_id`, and as
  `ctx.caller_module_name` when the caller is a module on the same node.
- Losing a key file loses the identity. There is no key rotation yet.

## Who can connect

- **A node listens on `127.0.0.1`** by default. To open it to the network:
  `interstice node expose <node> on`, or `node create … --expose`.
- **Admission.** The module manager admits every valid key except those listed
  in its `BlockedPeer` table: `block_peer` and `unblock_peer`, which are
  `local` reducers.

## Owners

Owners are the identities allowed to administer a node, listed in its
`node.toml`.
- The CLI that creates or starts a local node adds itself.
- Others are added with `interstice node owner add <node> <id>`; `interstice
  identity` prints a CLI's id.
- Owners may load, unload and remove modules, and call any reducer or query.

## What each call may reach

| Visibility | Callable by |
|---|---|
| `public` | anyone connected |
| `local` | modules on the same node |
| `internal` | the module itself |

Defaults:
- **Event handlers** are `internal`.
- **Other functions** are `public` in a `Public` module and `local` in a
  `Private` one.

Data:
- **Tables:** other nodes may read and subscribe to a table only if both the
  module and the table are public. Private tables are closed to other
  modules, which read them through **views**.
- **Views** decide row by row what each reader sees, from the reader's id
  (`viewer.node`) and, for a module on the same node, its name
  (`viewer.module`).
- **Schemas:** a node shows a non-owner only what that non-owner may use.

## Authorities

Authorities are privileged host functions: `Gpu`, `Input`, `Audio`, `File`,
`Network` and `Module`. Each is held by at most one module.
- **Claiming one needs a grant.** On a CLI `load`, the CLI asks (or `--grant`
  answers). Starting an example or a bundle grants what it contains, since
  running it is the owner's consent. A load by the module manager is granted
  by the manager's decision. Grants are remembered per module.
- **`File`** is confined to the node's file root: `<node>/files`, or
  `files_root` in `node.toml`. Paths outside it, `..`, and symlinks out of it
  are refused.
- **`Module`** can only be granted by an owner's load or a bundle, and its
  holder can't be replaced remotely.

## Not covered yet

- **Key rotation and revocation:** refusing a stolen key is up to the
  admission policy (`block_peer`).
- **Per-user identities and roles** beyond owners.
- **Modules on one node trust each other.** Any of them may call the module
  manager's `local` reducers. Finer rules belong in the manager, through
  `ctx.caller_module_name`.
