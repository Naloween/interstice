# Interstice

**Interstice** is a modular execution substrate for running sandboxed WASM modules that cooperate through shared, versioned data rather than ad‑hoc APIs.

Interstice sits *between* a database, a runtime, and an operating‑system‑like substrate — without fully being any of them. Its core goal is to provide a **minimal, well‑defined foundation** on top of which higher‑level systems (graphics, input, games, tools, services) can be built *as modules*, not as privileged hard‑coded features.

---

## Motivation

Most systems that aim for extensibility eventually collapse under one of two failures:

1. **Unstructured APIs** — implicit contracts, global state, callbacks everywhere
2. **Over‑centralization** — the core grows endlessly as features demand special cases

Interstice takes a different approach:

* All module interfaces are **data‑defined**
* All authority is **explicit and capability‑based**
* The core remains **small and boring**

The ambition is not to replace an OS or a game engine, but to define a substrate that *could* grow toward those roles without architectural rewrites.

---

## Core Concepts

### Modules

A **module** is a sandboxed WebAssembly component executed by the Interstice core.

Each module:

* owns its own state
* exposes a public interface
* may call into other modules (if authorized)
* may subscribe to state changes

Modules are isolated by default. Interaction is explicit.

---

### Tables

**Tables** are the primary state abstraction.

* Structured, typed collections of rows
* Owned by a single module
* Optionally marked as public

Tables are not just storage — they are *reactive state*. Changes to tables drive execution.

Supported operations (via host calls):

* insert row
* update row
* delete row
* query by key or filter

---

### Reducers

A **reducer** is a pure, deterministic function that:

* is defined by a module
* receives structured input
* performs controlled mutations on tables

Reducers form the **API surface** of a module.

Other modules may call a reducer *only if*:

* the reducer is public
* the versioned interface matches
* the caller has the required capability

Reducers are intentionally constrained:

* no direct OS access
* no hidden side effects
* bounded execution

This keeps execution analyzable and replayable.

---

### Versioned Interfaces

Each module exposes a versioned interface consisting of:

* table schemas
* reducer signatures

Callers must explicitly target a compatible version.

This allows:

* safe evolution of modules
* coexistence of multiple versions
* reproducible execution

---

## Reactivity and Subscriptions

Polling is avoided by design.

Modules may **subscribe** to:

* table changes (insert/update/delete)
* reducer invocations

When a subscribed event occurs, the core schedules the appropriate reducer.

This yields:

* event‑driven execution
* zero idle CPU usage
* deterministic ordering

---

## Capabilities and Authority

Interstice uses an explicit **capability model**.

A module has no authority unless granted.

Capabilities include (non‑exhaustive):

* table read / write
* reducer invocation
* subscriptions
* time access
* logging
* graphics / input / hardware access

Capabilities are:

* granted at load time or dynamically
* scoped
* revocable

This allows fine‑grained control without a bloated core API.

---

## The Core

The **Interstice core** is intentionally minimal. It is responsible for:

* loading and sandboxing WASM modules
* scheduling reducer execution
* managing tables and subscriptions
* enforcing capabilities
* mediating module‑to‑module calls

The core does *not*:

* define graphics APIs
* define file systems
* define networking
* define game logic

Those are implemented as modules.

---

## Privileged Modules

Some modules may be granted elevated capabilities.

Examples:

### Graphics Module

A privileged graphics module may:

* create windows
* receive input events
* submit GPU command buffers

Other modules interact with graphics **only through its tables and reducers**.

The core never exposes the GPU directly.

---

### IO / Platform Modules

Similarly, IO, audio, networking, or asset streaming are:

* implemented as modules
* capability‑gated
* replaceable

This keeps the core stable while allowing experimentation.

---

## Execution Model

1. A module updates a table or calls a reducer
2. The core records the change
3. Subscriptions are resolved
4. Dependent reducers are scheduled
5. Execution proceeds deterministically

Parallelism and optimization are **implementation details**, not part of the semantic model.

---

## Non‑Goals (for now)

Interstice deliberately does *not* attempt to:

* be a full operating system
* replace existing databases
* provide high‑level graphics APIs
* solve distributed consensus

These may be layered on top later — or not.

---

## Philosophy

Interstice is designed around a few guiding principles:

* **Structure over convenience**
* **Explicit authority**
* **Data before control flow**
* **Minimal core, powerful composition**

The name *Interstice* reflects this intent: it is the space *between* systems, where coordination happens.

---

## Status

Early design / foundation phase.

Expect:

* breaking changes
* incomplete features
* experimentation

The architecture is the product.

---

## License

TBD
