# Interstice – WASM ABI Specification (Draft)

This document specifies the **Application Binary Interface (ABI)** between the Interstice core (host) and WASM modules.

The ABI is intentionally:

* minimal
* explicit
* stable

It avoids embedding policy decisions or high-level semantics in the ABI itself.

---

## Design Constraints

The ABI must:

1. Work with standard WASM runtimes (Wasmtime)
2. Be language-agnostic
3. Support versioned module interfaces
4. Enforce capability checks at the host boundary
5. Remain forward-compatible

The ABI is **not** optimized for human ergonomics; higher-level SDKs are expected.

---

## Execution Model

* Modules export reducers as WASM functions
* The core invokes reducers synchronously
* All side effects occur via host calls

Reducers must be:

* deterministic
* non-blocking
* bounded in execution time

---

## Memory Model

Each module owns a linear WASM memory.

The ABI uses:

* caller-allocated buffers
* explicit pointers and lengths

No shared memory exists between modules.

---

## Core Imports

All host functionality is exposed via imported functions under the `interstice` namespace.

Example (conceptual):

```
(import "interstice" "table_insert" (func ...))
```

---

## ABI Types

All complex values are encoded using **canonical binary layouts**.

### Primitive Types

* `u32`, `u64`
* `i32`, `i64`
* `f32`, `f64`

### Handles

Opaque identifiers managed by the core:

* `TableId : u64`
* `ReducerId : u64`
* `ModuleId : u64`
* `SubscriptionId : u64`

Handles are not forgeable.

---

## Data Encoding

Structured data is encoded as:

* length-prefixed binary blobs
* schema-defined by the module interface

The ABI does **not** mandate JSON, protobuf, or any specific format.

A canonical encoding (e.g. flat binary or CBOR) is recommended but not required.

---

## Module Interface Declaration

Each module must export a well-known function:

```
export fn interstice_describe(ptr: u32, len: u32) -> u32
```

The core:

1. allocates a buffer
2. calls `interstice_describe`
3. receives a serialized interface description

The description includes:

* module name
* module version
* table schemas
* reducer signatures
* subscription declarations

---

## Reducer Exports

Each reducer is exported as a WASM function:

```
export fn reducer_<name>(ptr: u32, len: u32) -> u32
```

* `ptr/len` point to the encoded input arguments
* return value is a pointer to encoded output (or 0 if void)

The core controls allocation and deallocation.

---

## Host Calls by Authority

### 1. Logging and Introspection

```
log(level: u32, ptr: u32, len: u32)
now() -> u64
module_id() -> ModuleId
module_version() -> u32
```

---

### 2. Table Operations

```
table_insert(table: TableId, ptr: u32, len: u32)
table_update(table: TableId, key_ptr: u32, key_len: u32, ptr: u32, len: u32)
table_delete(table: TableId, key_ptr: u32, key_len: u32)

table_query(table: TableId, query_ptr: u32, query_len: u32, out_ptr: u32) -> u32
```

All mutations are validated by the core.

---

### 3. Reducer Invocation

```
reducer_call(
  module: ModuleId,
  reducer: ReducerId,
  version: u32,
  ptr: u32,
  len: u32
) -> u32
```

Cross-module calls are capability-checked.

---

### 4. Subscriptions

```
subscribe_table(
  table: TableId,
  event: u32,
  reducer: ReducerId
) -> SubscriptionId

unsubscribe(id: SubscriptionId)
```

---

### 5. Capability Queries (Optional)

```
has_capability(cap: u32) -> bool
```

Modules may adapt behavior based on granted authority.

---

## Error Handling

Host calls report errors via:

* reserved return codes
* or explicit error buffers

Reducers must handle failures gracefully.

Panics trap the module.

---

## Determinism Guarantees

The core guarantees:

* ordered reducer execution
* consistent table state
* reproducible results

Non-deterministic sources (time, input) are mediated.

---

## Versioning

ABI versions are explicit.

Breaking changes require:

* a new ABI version
* explicit opt-in by modules

---

## Non-Goals

The ABI does not:

* expose threads
* expose raw syscalls
* expose shared memory
* guarantee zero-copy semantics

These may be layered later.

---

## Status

Draft / subject to iteration.
