<a id="top"></a>

# rs-engine — A Technical Whitepaper

> **A from-scratch Rust reimplementation of a build-225 RuneScape 2 game server.**
> This document is its complete engineering reference: the architecture, the algorithms, the wire formats, the unsafe
> surface, the performance engineering, and the rationale behind every load-bearing decision.

|                     |                                                                                                                   |
|---------------------|-------------------------------------------------------------------------------------------------------------------|
| **Project**         | `rs-engine` (workspace root: `rs-majula`)                                                                         |
| **Version**         | 0.1.0                                                                                                             |
| **Language**        | Rust, **edition 2024**, MSRV **1.95** (`stable` toolchain)                                                        |
| **License**         | MIT                                                                                                               | |
| **Execution model** | Single-threaded game loop — `Engine::cycle()` runs **13 ordered phases** every **600 ms**                         |
| **Target scale**    | ~2,000 concurrent players per world node                                                                          |
| **Protocol target** | RuneScape 2, **build 225** (circa 2004) — bit-for-bit compatible with the stock client and original cache content |
| **Host**            | `tokio` async shell (network/DB/ether I/O) wrapping a single-threaded deterministic simulation                    |

---

## Abstract

`rs-engine` is a complete, production-oriented game server for the *build 225* revision of RuneScape 2, written from
scratch in Rust. It is functionally modeled on the canonical open-source TypeScript reference server (the
*LostCity / 2004scape* lineage), but rebuilt around three uncompromising goals: **byte-identical protocol and content
emulation** (the stock, unmodified client must connect and play against unmodified cache data), **single-threaded
deterministic simulation** (one world advances in lockstep, 600 ms per tick), and **mechanical sympathy** (every data
layout, allocation, and hot path is chosen for cache locality and a bounded per-tick CPU budget at two thousand
players).

The system is a Cargo workspace of nineteen crates organized by responsibility: a host crate (`rs-engine`) that owns the
mutable world and the tick, fourteen narrow leaf libraries (coordinates, zones, entities, the RuneScript virtual
machine, inventories, info-block encoding, vars/stats/timers/queues/hero/camera, and intrusive data structures), a
content/cache compiler (`rs-pack`), a logic-free wire codec (`rs-protocol`), and the async binary (`rs-server`). The
architectural keystone is a strict acyclic dependency graph enforced by the compiler, broken out of the reference
server's monolithic object soup by inverting the entity↔VM relationship through traits and an ambient `with_engine`
bridge. This whitepaper documents all of it — from the `u32`-packed coordinate system and the slab-allocated entity
registries up through the stack-based RuneScript interpreter, the single-encode-broadcast info pipeline, the
channel-isolated I/O boundary, and the `catch_unwind` resilience model — together with the performance philosophy and
the unsafe invariants that make a `!Sync`, globally-reachable `Engine` sound.

## About This Whitepaper

**Audience.** Engineers who want to understand, extend, or learn from the engine: contributors, emulation researchers,
systems-programming students, and anyone studying how to push a soft-real-time simulation to its performance ceiling in
Rust. The document assumes fluency in Rust and general systems concepts; it does *not* assume prior knowledge of
RuneScape internals — domain jargon is defined inline and collected in the [Glossary](docs/part-10-reference.md#sec-30).

**How it is organized.** The whitepaper is divided into ten parts that progress from motivation, to architecture, to
each subsystem in depth, to cross-cutting engineering concerns, to reference material:

- **Part I — Foundations & Motivation:** what the engine is, the design philosophy, and why it is written in Rust rather
  than reusing the TypeScript reference.
- **Part II — Architecture & the Tick:** the crate graph and data flow, the 600 ms cycle, the thirteen phases, the
  engine core, and the intrusive data structures beneath it.
- **Part III — The Spatial World & Entities:** coordinates, zones, the four entity kinds, and collision/pathfinding.
- **Part IV — The RuneScript Engine:** the virtual machine, the opcode instruction set, and the trigger/scheduling
  model.
- **Part V — Player State & Items:** inventories and the per-player sub-systems.
- **Part VI — Networking & the Wire:** the info-block encoder, the network protocol, and the input handlers.
- **Part VII — Content, Persistence & Distribution:** the cache pipeline, the database, and the multi-world ether.
- **Part VIII — Runtime & Host:** the async I/O boundary and the server binary.
- **Part IX — Engineering Deep-Dives:** performance, memory safety and the unsafe inventory, emulation fidelity, and the
  build/toolchain.
- **Part X — Reference:** a glossary and the forward roadmap.

**Conventions.** Every factual claim about behavior is anchored to the source as `path/to/file.rs:line` — these are
clickable in most editors. Diagrams are rendered with [Mermaid](https://mermaid.js.org/) (GitHub renders them natively);
wire/byte layouts are given as ASCII tables. Code excerpts are short and illustrative, not exhaustive copies. Where a
statement is an architectural inference rather than a directly-stated fact in the source, it is marked as such. Line
numbers reflect the codebase at the time of writing and may drift as the tree evolves; the surrounding identifiers
remain the durable reference.

**A note on completeness.** This is a deliberately exhaustive document. It is meant to be read in parts or consulted by
section, not necessarily front-to-back in one sitting. Readers who want the fastest possible mental model should read
Part II in
full ([Architecture](docs/part-02-architecture-and-the-tick.md#sec-04) → [The Game Tick](docs/part-02-architecture-and-the-tick.md#sec-05) → [The Thirteen Phases](docs/part-02-architecture-and-the-tick.md#sec-06)),
then dip into the subsystem they care about.

---

## Read This Whitepaper

This whitepaper is published in two forms:

- **By part (recommended — renders on GitHub, diagrams and all):** the ten parts below each live in their own file
  under [`docs/`](docs/).
- **As one file:** the complete document is also preserved verbatim as [
  `docs/whitepaper-full.md`](docs/whitepaper-full.md) (~840 KB). It exceeds GitHub's in-page render limit, so read it in
  an IDE/editor or offline; on GitHub, prefer the per-part files.

---

## Table of Contents

### [Part I · Foundations & Motivation](docs/part-01-foundations-and-motivation.md)

- [1. Introduction](docs/part-01-foundations-and-motivation.md#sec-01)
- [2. Design Philosophy & Goals](docs/part-01-foundations-and-motivation.md#sec-02)
- [3. Why Rust Over the TypeScript Reference Engine](docs/part-01-foundations-and-motivation.md#sec-03)

### [Part II · Architecture & the Tick](docs/part-02-architecture-and-the-tick.md)

- [4. System Architecture — Topology, Crate Graph & Data Flow](docs/part-02-architecture-and-the-tick.md#sec-04)
- [5. The Game Tick — Cycle Orchestration](docs/part-02-architecture-and-the-tick.md#sec-05)
- [6. The Thirteen Phases in Detail](docs/part-02-architecture-and-the-tick.md#sec-06)
- [7. The Engine Core — State Container, Registries & World Mutation](docs/part-02-architecture-and-the-tick.md#sec-07)
- [8. Core Data Structures — Intrusive Lists & Open-Addressed Tables](docs/part-02-architecture-and-the-tick.md#sec-08)

### [Part III · The Spatial World & Entities](docs/part-03-spatial-world-and-entities.md)

- [9. The Coordinate System & Spatial Addressing](docs/part-03-spatial-world-and-entities.md#sec-09)
- [10. Zones — Spatial Partitioning & Event Broadcasting](docs/part-03-spatial-world-and-entities.md#sec-10)
- [11. Entities — Players, NPCs, Locs & Objs](docs/part-03-spatial-world-and-entities.md#sec-11)
- [12. Collision & Pathfinding](docs/part-03-spatial-world-and-entities.md#sec-12)

### [Part IV · The RuneScript Engine](docs/part-04-the-runescript-engine.md)

- [13. The RuneScript Virtual Machine — Architecture & Execution Model](docs/part-04-the-runescript-engine.md#sec-13)
- [14. The RuneScript Instruction Set — Opcode Catalog](docs/part-04-the-runescript-engine.md#sec-14)
- [15. Triggers, Scheduling & the World Queue](docs/part-04-the-runescript-engine.md#sec-15)

### [Part V · Player State & Items](docs/part-05-player-state-and-items.md)

- [16. Inventories & Items](docs/part-05-player-state-and-items.md#sec-16)
- [17. Player Sub-Systems — Vars, Stats, Timers, Queues, Hero, Camera](docs/part-05-player-state-and-items.md#sec-17)

### [Part VI · Networking & the Wire](docs/part-06-networking-and-the-wire.md)

- [18. Player & NPC Info Blocks — The Wire-Encoding Pipeline](docs/part-06-networking-and-the-wire.md#sec-18)
- [19. The Network Protocol & Packet Model](docs/part-06-networking-and-the-wire.md#sec-19)
- [20. Input Handlers — From Client Packet to Game Action](docs/part-06-networking-and-the-wire.md#sec-20)

### [Part VII · Content, Persistence & Distribution](docs/part-07-content-persistence-and-distribution.md)

- [21. The Game Cache & Content Pipeline](docs/part-07-content-persistence-and-distribution.md#sec-21)
- [22. Persistence — Player Saves & the Database Client](docs/part-07-content-persistence-and-distribution.md#sec-22)
- [23. Multi-World & the Ether](docs/part-07-content-persistence-and-distribution.md#sec-23)

### [Part VIII · Runtime & Host](docs/part-08-runtime-and-host.md)

- [24. The Async I/O Boundary & Client Lifecycle](docs/part-08-runtime-and-host.md#sec-24)
- [25. The Server Binary — Bootstrap, HTTP & TUI](docs/part-08-runtime-and-host.md#sec-25)

### [Part IX · Engineering Deep-Dives](docs/part-09-engineering-deep-dives.md)

- [26. Performance Engineering — The Optimization Playbook](docs/part-09-engineering-deep-dives.md#sec-26)
- [27. Memory Safety & the Unsafe Inventory](docs/part-09-engineering-deep-dives.md#sec-27)
- [28. Emulation Fidelity — Java Semantics & Byte-Identical Wire Format](docs/part-09-engineering-deep-dives.md#sec-28)
- [29. Build System, Toolchain & Observability](docs/part-09-engineering-deep-dives.md#sec-29)

### [Part X · Reference](docs/part-10-reference.md)

- [30. Glossary of Domain & Engine Terms](docs/part-10-reference.md#sec-30)
- [31. Conclusion & Roadmap](docs/part-10-reference.md#sec-31)
