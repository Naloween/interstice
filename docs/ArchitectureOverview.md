# Interstice – Architecture Overview

This document describes the internal architecture of **Interstice**, the rationale behind its major design choices, and the concrete API surface exposed by the core to modules.

It is intended as a *design reference*, not a user tutorial.

---

## High-Level Architecture

Interstice is organized around a **minimal trusted core** and a set of **sandboxed WASM modules**.

```
+-----------------------------+
|         Interstice          |
|            Core             |
|                             |
|  +-----------------------+  |
|  |  Scheduler / Reactor  |  |
|  +-----------------------+  |
|  |  Table Store          |  |
|  +-----------------------+  |
|  |  Capability Manager   |  |
|  +-----------------------+  |
|  |  WASM Runtime         |  |
|  |  (wasmtime)           |  |
|  +-----------------------+  |
+-------------+---------------+
              |
              | Host Calls
              |
+-------------v---------------+
|        WASM Modules          |
|                              |
|  - Tables                    |
|  - Reducers                  |
|  - Subscriptions             |
|                              |
+------------------------------+
```

The core owns *execution, scheduling, state, and authority*. Modules own *logic*.

---

## Design Principles

### 1. Minimal Trusted Computing Base

The core must be:

* small
* auditable
* deterministic

All non-essential functionality (graphics, IO, networking, asset loading) lives in modules.

---

### 2. Data-Oriented Interfaces

Modules communicate exclusively via:

* tables (state)
* reducers (state transitions)

There are no callbacks, shared memory, or implicit global services.

---

### 3. Explicit Authority

Every action that escapes pure computation requires a **capability**.

There is no ambient authority.

---

### 4. Deterministic Execution

Given:

* the same module versions
* the same reducer calls
* the same initial table state

Execution order and results are deterministic.

This enables replay, debugging, and testing.

---

## Core Responsibilities

The Interstice core is responsible for:

* Loading and instantiating WASM modules
* Enforcing sandboxing and memory isolation
* Managing tables and schemas
* Scheduling reducer execution
* Tracking subscriptions
* Enforcing capability checks
* Mediating all cross-module interaction

The core does *not* perform domain-specific work.

---

## Modules

A module consists of:

* Table definitions (schema)
* Reducer definitions
* Optional subscriptions

Modules cannot:

* spawn threads
* perform syscalls
* access hardware

Unless explicitly granted via capabilities.

---

## Tables

Tables are:

* owned by exactly one module
* strongly typed
* versioned as part of the module interface

Tables are identified by:

```
(module_id, table_name, version)
```

### Table Operations

All table mutations occur inside reducers.

Supported primitives:

* insert
* update
* delete
* query

---

## Reducers

Reducers are:

* deterministic
* side-effect constrained
* bounded in execution

Reducers may:

* read/write owned tables
* read authorized external tables
* call other reducers (if authorized)

Reducers may *not*:

* block
* sleep
* perform IO directly

---

## Subscriptions

Subscriptions express *reactive dependencies*.

A module may subscribe to:

* table mutations
* reducer invocations

When triggered, the subscribed reducer is scheduled by the core.

This avoids polling and idle execution.

---

## Capability System

Capabilities are explicit tokens granted to modules.

Capabilities are:

* typed
* scoped
* revocable

The core validates capabilities on every host call.

---

## Core API (Host Calls)

The host API exposed to WASM modules is intentionally narrow.

It is grouped by **authority level**.

---

### 1. Pure / Always-Available

No authority required.

* `core.log(level, message)`
* `core.now()`
* `core.module_id()`
* `core.module_version()`

---

### 2. Table Authority

Requires table-specific capability.

* `table.insert(table_id, row)`
* `table.update(table_id, key, row)`
* `table.delete(table_id, key)`
* `table.query(table_id, filter)`

---

### 3. Reducer Authority

Requires reducer-invocation capability.

* `reducer.call(module_id, reducer_name, version, args)`

Calls are synchronous from the caller’s perspective, but scheduled deterministically by the core.

---

### 4. Subscription Authority

Requires subscription capability.

* `subscribe.table(table_id, event_kind, reducer)`
* `subscribe.reducer(module_id, reducer_name, event_kind, reducer)`

---

### 5. Module Introspection Authority

* `module.list()`
* `module.interface(module_id, version)`

---

### 6. Privileged / Platform Authority

Granted only to trusted modules.

Examples:

#### Graphics

* `gfx.create_window(...)`
* `gfx.submit_commands(...)`
* `gfx.present(...)`

#### Input

* `input.subscribe(...)`

#### IO / Assets

* `io.request_asset(...)`
* `io.subscribe_completion(...)`

The core treats these as opaque capabilities.

---

## Graphics as a Module

Graphics is not a built-in feature.

Instead:

* a privileged graphics module owns GPU access
* other modules interact via its tables and reducers

This keeps the core independent of rendering APIs.

---

## Scheduling Model

Execution is event-driven.

1. A reducer executes
2. Tables are mutated
3. Subscriptions are resolved
4. Dependent reducers are queued

Scheduling policy is an implementation detail.

---

## Parallelism

Parallel execution is allowed *only if*:

* no conflicting table writes occur
* determinism is preserved

This is enforced by the core.

---

## Future Extensions (Non-Core)

Explicitly out of scope for the core:

* Job systems
* Unsafe fast paths
* Bulk memory access
* Distributed execution

These may be layered later.

---

## Summary

Interstice defines a small but expressive substrate:

* Data-defined interfaces
* Deterministic execution
* Explicit authority
* Replaceable subsystems

The core remains stable. Everything else is a module.
