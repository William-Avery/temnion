---
title: "Chronicle + EKS Architecture Specification"
subtitle: "For Tzeentch and future model-agnostic consumers"
author: "Architecture Baseline v0.1"
date: "2026-09-07"
---

# Table of Contents

- 1. Executive Summary
- 2. Architectural Charter
- 3. Decision Classes
- 4. System Context
- 5. Proposed Repository and Crate Layout
- 6. Coding and Engineering Standards
- 7. Schema and Logical Data Model
- 8. Live State: Packed StateSlab
- 9. Event Chronicle and Delta History
- 10. Deterministic Reconstruction
- 11. Immutable Segment Format
- 12. Compression and Encoding
- 13. N-Dimensional Storage, Chunking, and Locality
- 14. Projections and Local Indexes
- 15. Filters, Summaries, and Progressive Pruning
- 16. Hierarchical Summaries / Data Mipmaps
- 17. Temporal Model
- 18. Causal History
- 19. Branching Timelines and Structural Sharing
- 20. Concurrency Model
- 21. Background Execution
- 22. Storage Hierarchy and I/O
- 23. Query Architecture
- 24. ChronQL: Human Query Language
- 25. Compact ChronQL and AI Token Efficiency
- 26. Query Budgets and Continuations
- 27. Cost Estimation and EXPLAIN
- 28. Data Ingestion and Subscriptions
- 29. Connectivity and Transport Architecture
- 30. Self-Description and Capability Negotiation
- 31. Model-Agnostic Integration
- 32. Chronicle Workbench / Studio
- 33. EKS: Evolutionary Knowledge Substrate
- 34. Provenance and Truth Maintenance
- 35. Predictive Knowledge
- 36. Knowledge Consolidation and Active-Memory Scaling
- 37. Transformation Engine
- 38. Evolution Engine
- 39. Adaptive Physical Memory
- 40. Meta-Evolution
- 41. Immutable Constitution
- 42. Scaling Strategy
- 43. Retention and Data Lifecycle
- 44. Metrics and Observability
- 45. Initial Performance Targets
- 46. Canonical Benchmark Program
- 47. Testing and Verification
- 48. Security and Operational Boundaries
- 49. Documentation and Developer Experience
- 50. Tzeentch Integration
- 51. Milestone Roadmap
- 52. Hard Gates
- 53. Risk Register
- 54. Key Architectural Metrics for Long-Term Success
- 55. Final Architectural Position
- Appendix A. Provisional Public API Shapes
- Appendix B. Provisional Compact Query Examples
- Appendix C. Candidate Telemetry Record
- Appendix D. Decision Summary

# 1. Executive Summary

Chronicle is a proposed high-performance, model-agnostic state and history engine designed first for Tzeentch, but intentionally usable by other AI agents, reinforcement-learning systems, world models, simulations, digital twins, temporal analytics systems, and conventional applications.

The core design goal is not to create a universal replacement for PostgreSQL, Redis, vector databases, or scientific array databases. It is to specialize in a different problem:

> Record an evolving system as compactly as practical, preserve exact and causal history, reconstruct state deterministically where possible, retrieve only the information actually needed, and allow higher layers to derive, transform, test, and evolve knowledge without corrupting the immutable record of what happened.

The complete architecture is divided into five conceptual layers:

1. **Chronicle / exact memory:** current state, immutable events, snapshots, replay, branches, causality, temporal history.
2. **EKS / knowledge:** observations, claims, beliefs, concepts, rules, world models, skills, confidence, uncertainty, and provenance.
3. **Transformation:** abstraction, rewriting, consolidation, summarization, synthesis, model conversion, and representation changes.
4. **Evolution:** champion/challenger candidate generation, evaluation, promotion, rollback, and lineage across storage, knowledge, models, skills, and transformations.
5. **Immutable Constitution:** integrity, provenance semantics, ABI contracts, sandboxing, rollback, resource limits, and promotion rules that the evolvable layers may not rewrite.

Chronicle is intended to be a standalone Rust project. Tzeentch is its first major consumer, not a hard dependency. The primary public interfaces are a native Rust API, C ABI plus Arrow interoperability, local IPC/shared memory, Arrow Flight for remote high-volume transfer, an official MCP server for AI discovery/control/querying, SQL compatibility where useful, and a human-facing Chronicle Workbench.

The project is benchmark-first. Every optimization is required to demonstrate a measurable benefit. Elegant mechanisms that do not outperform simpler incumbents under representative workloads are removed or disabled.

---

# 2. Architectural Charter

## 2.1 Goals

Chronicle shall:

- Keep live state near the packed theoretical minimum in RAM.
- Store history as events/deltas rather than repeated full rows wherever semantics permit.
- Avoid storing data that can be deterministically reconstructed.
- Preserve exact ground-truth history separately from interpretations derived from it.
- Support sparse N-dimensional logical state without assuming one global physical layout.
- Treat time as a first-class concept, while recognizing that world time, observation time, knowledge time, and causal order are not identical.
- Make typical query cost depend primarily on relevant data, not total database size.
- Support billions and eventually trillions of historical events without a giant global row index.
- Scale across cores using ownership rather than shared mutable hot state.
- Offload expensive adaptive, compression, index, knowledge, and evolutionary work to background workers operating primarily on immutable data.
- Provide human-readable and model-efficient query interfaces over the same typed logical IR.
- Be self-describing so clients can discover schemas, capabilities, query operators, model adapters, and result formats.
- Allow multiple model families and runtimes without coupling the core to Tzeentch, transformers, PyTorch, CUDA, or any one inference framework.
- Expose complete provenance: why a belief exists, what evidence supports it, which transformation created it, which mutation changed it, and which model/organ consumed it.
- Make storage layout, codec, indexes, checkpoints, cache policy, and other implementation strategies candidates behind stable interfaces.
- Keep security/integrity boundaries non-evolvable.

## 2.2 Non-goals

Chronicle is not intended to:

- Replace PostgreSQL for arbitrary relational transactions, business joins, constraints, or administrative workloads.
- Replace Redis for every trivial key/value cache use case.
- Replace a vector database for semantic nearest-neighbor search; vector retrieval is a complementary projection.
- Replace rasdaman/TileDB-style array algebra for arbitrary scientific array computation.
- Make MCP the high-throughput database transport.
- Execute arbitrary model/plugin code inside the database core by default.
- Make all historical data lossy automatically.
- Guarantee that learned indexes, Hilbert layouts, ALP, `io_uring`, CXL, ZNS, or any other research technique will be enabled merely because it is sophisticated.

## 2.3 Core optimization law

The system should never compute, retrieve, transform, evolve, transfer, or visualize information at a greater resolution than the consumer requires.

This principle applies to all layers:

- exact event vs. summary;
- full historical state vs. replay;
- raw evidence vs. consolidated knowledge;
- full result vs. token/byte/event budget;
- all candidate mutations vs. cheap screening;
- 100 million events vs. a few thousand visualization buckets.

---

# 3. Decision Classes

Every architectural choice belongs to one of three classes.

## 3.1 Locked architectural decisions

These define the system and should change only through an explicit architecture revision:

- Rust-first core.
- Model-agnostic Chronicle core.
- Exact Chronicle below derived knowledge.
- Current state physically separated from historical state.
- Append/event-oriented history rather than full-state rows by default.
- Deterministic reconstruction when feasible.
- Immutable history after segment sealing.
- Virtual shards with single-writer ownership.
- No globally shared mutable hot database object.
- Separate foreground and background execution classes.
- One canonical typed Query IR.
- Multiple thin query/transport front ends over that IR.
- MCP as an AI-facing control/discovery plane, not the bulk data plane.
- High-throughput native/IPC/Arrow-oriented data paths.
- Provenance and lineage are first-class data.
- Evolution uses candidate -> evaluate -> compare -> promote/rollback.
- Immutable Constitution beneath all evolvable behavior.
- Documentation, tests, benchmarks, and examples are part of feature completion.

## 3.2 Benchmark-gated implementation choices

These are candidates, not dogma:

- Morton vs. Hilbert vs. grid vs. adaptive/piecewise SFC.
- Per-shard vs. dedicated `io_uring` I/O workers.
- mmap vs. direct I/O for specific tiers/workloads.
- ALP vs. XOR vs. FOR vs. other float/integer codecs.
- Binary Fuse vs. Ribbon vs. Bloom-like filters.
- Learned indexes vs. SIMD trees vs. simple sorted arrays.
- Tokio vs. custom reactor for non-critical surfaces; Tokio is not assumed for the hot storage path.
- Segment size and checkpoint spacing.
- Number of virtual shards.
- CPU pool allocation between foreground, I/O, compression, indexing, maintenance, and evolution.

## 3.3 Future-capability hooks

The architecture should leave clean interfaces for:

- CXL memory tiers.
- Zoned Namespace SSDs.
- GPU/device-resident Arrow buffers.
- Multi-node/distributed Chronicle.
- Learned prefetch and placement policies.
- Error-bounded semantic-aware cold compression.
- Learned multidimensional indexes on immutable segments.

---

# 4. System Context

```text
                         TZEENTCH / OTHER CONSUMERS
                                    |
             +----------------------+----------------------+
             |                      |                      |
         Rust API                 MCP/AI              Workbench
             |                      |                      |
             +----------------------+----------------------+
                                    |
                             Typed Query IR
                                    |
                    +---------------+---------------+
                    |                               |
                Chronicle                         EKS
             exact state/history            derived knowledge
                    |                               |
                    +---------------+---------------+
                                    |
                       Transformation / Evolution
                                    |
                       Immutable Constitution
                                    |
                        Rust storage/runtime core
```

Chronicle is responsible for what happened. EKS is responsible for interpretations, beliefs, models, skills, and knowledge derived from what happened. Transformation and evolution are allowed to replace representations and derived artifacts, but they may not rewrite the underlying historical evidence or the Constitution.

---

# 5. Proposed Repository and Crate Layout

A likely workspace layout is:

```text
chronicle/
├── Cargo.toml
├── crates/
│   ├── chronicle-core/          # common IDs, schemas, errors, capability model
│   ├── chronicle-format/        # stable segment/binary format
│   ├── chronicle-state/         # packed live StateSlab
│   ├── chronicle-events/        # append event representation / mutable epoch
│   ├── chronicle-replay/        # deterministic reconstruction/checkpoints
│   ├── chronicle-storage/       # segments, blocks, tiers, placement
│   ├── chronicle-codec/         # compression/encoding candidates
│   ├── chronicle-index/         # projections, filters, bitmaps, SFC layouts
│   ├── chronicle-runtime/       # shard reactors, messaging, scheduling
│   ├── chronicle-query-ir/      # canonical logical/physical query IR
│   ├── chronicle-query/         # optimizer/executor
│   ├── chronicle-chronql/       # human and compact syntax parser
│   ├── chronicle-sql/           # optional DataFusion/SQL surface
│   ├── chronicle-ffi/           # stable C ABI
│   ├── chronicle-ipc/           # local pipe/socket + shared-memory data plane
│   ├── chronicle-flight/        # Arrow Flight remote transport
│   ├── chronicle-mcp/           # official AI-facing MCP server
│   ├── chronicle-model/         # model manifest/worker/adapters
│   ├── chronicle-eks/           # knowledge/provenance/truth maintenance
│   ├── chronicle-transform/     # first-class transformations
│   ├── chronicle-evolution/     # candidate/promotion/lineage engine
│   ├── chronicle-tzeentch/      # Tzeentch-specific adapter only
│   └── chronicle-bench/         # canonical workloads / competitors
├── apps/
│   ├── chronicle-server/
│   ├── chronicle-cli/
│   └── chronicle-workbench/
├── docs/
└── tests/
```

The exact crate boundaries may be merged during implementation if compile-time and maintenance overhead exceeds the value of separation. Stable public interfaces matter more than a maximal number of crates.

---

# 6. Coding and Engineering Standards

## 6.1 Language and safety

- Rust is the primary implementation language.
- The hot storage/query path must not depend on a garbage collector.
- `unsafe` code should be isolated to small, audited modules required for FFI, mmap, SIMD, or low-level zero-copy operations.
- Public APIs do not panic on untrusted input; failures return typed errors.
- Binary formats define endianness explicitly.
- Serialization formats are versioned.
- External input is validated before entering trusted internal representations.

## 6.2 Hot-path rules

Avoid on latency-critical paths:

- JSON.
- Dynamic strings where stable numeric IDs suffice.
- General-purpose allocation per event/query.
- Global mutexes or `Arc<RwLock<Database>>`.
- Global sequence counters requiring atomic read-modify-write per event.
- Arbitrary background work on foreground shard reactors.

Prefer:

- narrow integers;
- bitsets;
- enums;
- fixed-point where appropriate;
- SoA layouts;
- arena/slab allocation;
- zero-copy views;
- preallocated bounded queues;
- typed IDs;
- prepared queries;
- immutable structures after publication.

## 6.3 Feature completion rule

A public feature is not complete until it has:

1. implementation;
2. tests;
3. benchmark where performance-relevant;
4. documentation;
5. at least one runnable example.

## 6.4 Compatibility

- Binary segment versions remain readable after later schema/engine versions where practical.
- Public C ABI and transport protocols are versioned.
- Model and organ interfaces use stable contracts and explicit capability negotiation.
- Internal implementation detail may evolve freely behind those contracts.

---

# 7. Schema and Logical Data Model

Chronicle exposes typed logical state rather than raw untyped blobs.

A schema may contain:

- dimensions;
- entities;
- fields/attributes;
- time semantics;
- units;
- exactness/precision policy;
- retention policy;
- mutability;
- indexing hints;
- reconstruction semantics.

Example:

```text
entity.position:
    type: vec3<f32>
    unit: meters
    mutable: true
    temporal: true
    recent_exact: true
    cold_max_error: 0.01m

entity.health:
    type: u16
    exact: true
    temporal: true

entity.visible:
    type: bool
    temporal: true
```

Logical N-dimensional access may be represented as:

```text
D(d0, d1, d2, ... dn) -> typed value/event/state
```

However, logical dimensional equality does not imply a symmetric physical layout. Time and entity timelines may receive dedicated physical treatment.

---

# 8. Live State: Packed StateSlab

Live state is not reconstructed from history on every decision. It has its own optimized representation.

## 8.1 Preferred form

For dense/reusable entity IDs:

```text
EntityId -> base + entity_index * stride -> packed state
```

Use generation IDs when IDs are recycled to prevent stale references.

## 8.2 Representation

Depending on field access patterns, use either packed AoS or SoA/hybrid layouts. Frequently filtered fields should prefer columnar/bitmap representation; fields usually consumed together may share compact structs.

Potential Rust concepts:

```rust
pub struct EntityId {
    pub slot: u32,
    pub generation: u16,
}

pub trait StateSchema {
    type View<'a> where Self: 'a;
    type Mutation<'a> where Self: 'a;
}

pub struct StateSlab<S: StateSchema> {
    // shard-local, packed storage
    _schema: core::marker::PhantomData<S>,
}
```

## 8.3 Target

Cached live-state lookup target:

- aspirational: 50-500 ns for dense direct-offset access;
- acceptance target: under 1 us for normal hot lookups on target hardware.

These are benchmark targets, not guarantees.

---

# 9. Event Chronicle and Delta History

Chronicle stores changes rather than repeated rows by default.

A conceptual event encoding is:

```text
[change mask] [delta time] [entity/local id] [changed values...]
```

Potential event fields:

- event ID;
- epoch;
- source/shard local sequence;
- world/valid time;
- observation time;
- knowledge/commit time where applicable;
- causal parent/reference IDs;
- changed-field mask;
- payload.

Prefer:

- ZigZag for signed deltas;
- variable-width integers where useful;
- bit-packed low-cardinality values;
- delta-of-delta timestamps;
- XOR or ALP-like float encoding candidates;
- sparse event masks.

Chronicle may coalesce redundant intermediate materialization inside a mutable window only when doing so does not destroy required semantics or replay fidelity.

---

# 10. Deterministic Reconstruction

When state can be reconstructed from prior state plus external inputs, actions, and RNG state, Chronicle should not persist repeated copies of the reconstructible state.

Model:

```text
S(t+1) = F(S(t), E(t), A(t), RNG(t))
```

Chronicle stores:

```text
checkpoint + external events + actions + deterministic RNG state
```

rather than every complete state frame.

## 10.1 Replay API

```rust
pub trait ReplayEngine {
    type State;
    type Error;

    fn reconstruct(&self, branch: BranchId, at: WorldTime)
        -> Result<Self::State, Self::Error>;
}
```

## 10.2 Adaptive checkpoints

Checkpoint frequency is not permanently fixed. The eventual policy minimizes a cost function incorporating:

- replay latency;
- mutation density;
- query frequency;
- storage cost;
- branch frequency;
- importance.

Frequently queried or expensive-to-replay histories receive denser checkpoints. Cold deterministic history receives fewer.

## 10.3 Determinism requirement

Replay tests should prove bit-for-bit equivalence where the modeled system permits it. Non-deterministic external dependencies must be captured explicitly if exact replay is required.

---

# 11. Immutable Segment Format

Mutable events accumulate in shard-local epochs. When an epoch closes it is sealed, encoded, validated, and published as immutable history.

Conceptual layout:

```text
SEGMENT
├── Header
│   ├── magic/version
│   ├── schema version
│   ├── epoch/branch
│   ├── dimension/time bounds
│   ├── record count
│   └── layout/codec descriptors
├── Chunk/Block Directory
├── Statistics and Summaries
├── Membership/Occupancy Filters
├── Entity/Time/Spatial Projections
├── Time Stream
├── Entity Stream
├── Coordinate Streams
├── Change Masks
├── Attribute Streams
├── Causal Adjacency
├── Checksums
└── Footer
```

Properties:

- immutable after publication;
- independently readable;
- checksummed;
- versioned;
- mmap-friendly;
- zero-copy-friendly where possible;
- content-addressable where useful;
- branch/dedup compatible.

Segment IDs may incorporate a cryptographic or strong content hash when content addressing or integrity requires it.

---

# 12. Compression and Encoding

No universal codec is assumed.

Candidate codecs include:

```text
Raw
BitPack
Delta
DeltaDelta
Frame-of-Reference
XOR
ALP-style float encoding
LZ4
Zstd
```

Possible future candidates include specialized sparse and error-bounded codecs.

## 12.1 Per-stream selection

Example:

```text
time        -> delta-delta
entity      -> bitpack/FOR
position    -> delta/FOR
flags       -> bitmap
velocity    -> ALP or XOR candidate
cold blob   -> Zstd
```

## 12.2 Candidate scoring

Each sealed segment may sample candidate codecs and score them by a weighted objective such as:

```text
score = storage_bytes
      + alpha * decode_cpu
      + beta  * encode_cpu
      + gamma * expected_query_cost
```

Selection occurs off the foreground hot path. The simpler incumbent remains if the candidate does not measurably win.

## 12.3 Query compressed data when possible

Filters, bitmaps, and compatible integer encodings should support predicate evaluation with minimal full decoding. The engine optimizes bytes moved and bytes decoded, not just instruction count.

---

# 13. N-Dimensional Storage, Chunking, and Locality

The logical model is N-dimensional, but the physical design specializes around the workload.

A preferred initial ordering for evolving world/state data is:

```text
Epoch -> Coarse Region -> Local Layout -> Time Delta
```

rather than a global symmetric 4D Morton key over `(x,y,z,t)`.

## 13.1 Physical layout candidates

Per segment/region:

- grid;
- Morton/Z-order;
- Hilbert;
- adaptive/piecewise SFC;
- entity ordering;
- temporal ordering;
- no SFC when a simpler layout wins.

## 13.2 Physical polymorphism

Each immutable segment records its physical strategy:

```text
SegmentDescriptor
├── layout
├── codecs
├── indexes
├── filters
├── summaries
└── placement tier
```

Different segments may use different representations under one logical schema.

---

# 14. Projections and Local Indexes

One physical order cannot optimize every access path. Chronicle therefore stores the payload once and builds compact alternate projections.

Initial projections:

- spatial -> segment/block/offset;
- entity -> timeline references;
- time -> segment ranges;
- event type -> compressed bitmap;
- causal -> compact adjacency;
- knowledge -> Chronicle evidence references;
- optional semantic/vector -> episode/model references.

Indexes are local to segments/shards wherever possible. Avoid one giant global B-tree containing every event ever recorded.

## 14.1 Low-cardinality filtering

Fields such as:

- visible;
- enemy;
- attacking;
- status;
- class/type;

should preferentially use compressed bitmaps instead of conventional per-row tree indexes.

Example:

```text
visible AND enemy AND attacking
```

becomes bitmap intersection before payload decoding.

---

# 15. Filters, Summaries, and Progressive Pruning

A query should descend only into branches that may contain an answer.

Hierarchy:

```text
Global Directory
  -> Node (future distributed)
    -> Virtual Shard
      -> Epoch
        -> Segment
          -> Block
            -> Compressed Stream
              -> Event
```

At every level, metadata attempts to prove irrelevance.

Candidate metadata:

- time bounds;
- dimension/spatial bounds;
- min/max;
- count;
- event-type mask;
- entity membership filter;
- occupancy bitmap;
- change density;
- first/last event;
- summary aggregates.

Binary Fuse, Ribbon, or similar static membership filters are candidates for immutable segments. Selection remains benchmark-gated.

---

# 16. Hierarchical Summaries / Data Mipmaps

Chronicle stores multi-resolution summaries to avoid reading raw history for overview queries or visualization.

Conceptually:

```text
Level 0: raw events
Level 1: 16x summary
Level 2: 256x summary
Level 3: 4096x summary
...
```

Summary fields may include:

- count;
- min/max;
- event masks;
- first/last time;
- change density;
- state ranges;
- region occupancy;
- approximate aggregate statistics.

The query planner chooses the minimum resolution satisfying the consumer's requested precision/budget.

---

# 17. Temporal Model

Chronicle must not collapse all time semantics into one timestamp.

At minimum support:

1. **World/valid time:** when something was true in the modeled world.
2. **Observation time:** when a sensor/source observed or captured it.
3. **Knowledge/commit time:** when the organism/system knew or committed the information.
4. **Logical causal order:** source sequence / logical clock establishing happens-before relationships.

This allows replay to distinguish:

- what was true;
- what was observable;
- what Tzeentch knew at the time.

This distinction prevents future-information leakage during training/replay.

---

# 18. Causal History

Chronicle records optional causal edges between events and derived operations.

A compact immutable representation may use CSR-like arrays:

```text
event_offsets[]
cause_ids[]
```

Queries include:

```text
CAUSES(event)
EFFECTS(event)
TRACE CAUSES DEPTH n
TRACE EFFECTS UNTIL t
```

For Tzeentch, a desired trace is:

```text
world event
 -> observation
 -> perception/organ processing
 -> retrieved knowledge
 -> prediction/belief
 -> decision
 -> action
 -> outcome
 -> prediction error/update
```

---

# 19. Branching Timelines and Structural Sharing

Counterfactual and RL workflows require cheap forks.

Chronicle history forms a persistent DAG:

```text
A -> B -> C
          +-> D1 -> E1
          +-> D2 -> E2
          +-> D3 -> E3
```

Branches share all immutable ancestors. Creating a branch should be approximately O(1) metadata until divergence occurs.

Branch uses:

- policy comparison;
- counterfactual simulation;
- rollback;
- training forks;
- world-model experiments.

Branch lifecycle states should include temporary, candidate, important/promoted, and discardable. Experimental branches are garbage-collected when no longer useful while minimal lineage metadata may be retained.

---

# 20. Concurrency Model

Chronicle does not copy Redis's single command-execution core and does not use a conventional globally shared mutable database with many worker threads.

The selected model is:

> **Virtual-shard ownership with a single foreground writer/reactor per executing shard group.**

## 20.1 Virtual shards

Use substantially more logical partitions than physical CPU cores, e.g. an initial benchmark candidate around 1024 virtual shards.

Virtual shards are remappable to cores and later to machines without changing persistent identity.

## 20.2 Single writer

Only the owning foreground reactor mutates a shard's live state, mutable epoch, allocator, and hot indexes.

Hot-path goals:

- zero global mutexes;
- near-zero atomic RMW operations;
- shard-local caches;
- shard-local arenas/slabs;
- bounded message queues for cross-shard requests.

## 20.3 Sharding keys

History should prefer locality-sensitive ownership such as coarse spatial/state region plus epoch rather than a simple global random hash when locality matters.

Live current state may use entity-oriented ownership when that better matches the policy's access pattern.

Different projections bridge the two access patterns without copying payloads.

## 20.4 Cross-shard reads

Wide queries fan out to relevant shards in parallel and merge results:

```text
query -> shard set -> parallel local execution -> merge
```

Latency approximates the slowest participating shard plus merge cost rather than sum of all shard latencies.

## 20.5 Avoid global counters

Event IDs should be independently generated from components such as:

```text
[epoch | shard | local_sequence]
```

rather than a globally contended atomic increment.

---

# 21. Background Execution

Background threads are a core architectural component, but they are not allowed to directly mutate foreground-owned live state.

## 21.1 Work classes

```text
Foreground   - live reads/writes and latency-sensitive query work
I/O          - NVMe submission/completion
Seal         - finalize mutable epochs
Compression  - encode/compress immutable blocks
Index        - projections, filters, learned/static indexes
Summary      - hierarchical aggregates
Maintenance  - archive, checksum, lifecycle work
Evolution    - candidate experiments and promotion evaluation
ML           - embeddings/model-related background work
```

## 21.2 Immutable-work rule

Foreground owns mutable state. Background workers primarily own immutable/sealed work.

A background task publishes a finished immutable candidate, validates it, then performs a version/pointer publication rather than exposing partially constructed structures.

## 21.3 Pressure-sensitive scheduling

Background work backs off based on foreground service pressure, not simply CPU utilization.

Monitored pressure should include:

- foreground p99/p99.9 latency;
- request queue depth;
- memory bandwidth/cache misses;
- I/O queue depth;
- storage bandwidth;
- segment-seal backlog;
- CPU saturation.

Evolution and archival experiments are the first classes throttled under pressure.

## 21.4 Resumable task graph

Long work is decomposed into bounded resumable tasks, such as per-block compression, so the scheduler can pause/reprioritize work.

Sealed segment task DAG example:

```text
seal
 +-> checksum
 +-> statistics -> codec selection -> compression
                              +-> spatial index
                              +-> entity index
                              +-> bitmaps/filters
                              +-> summaries
                                      -> validate -> publish
```

## 21.5 NUMA and heterogeneous CPUs

On NUMA systems, background processing should prefer the NUMA node owning the segment memory. On heterogeneous P/E-core systems, latency-sensitive work should prefer faster cores while compression/evolution/maintenance may use throughput/efficient cores.

---

# 22. Storage Hierarchy and I/O

Logical segment identity is independent of placement.

Initial tiers:

```text
Hot:     DRAM
Warm:    mmap/page cache and local NVMe
Cold:    highly compressed NVMe/archive
Archive: optional external/remote storage
```

Future hooks:

```text
CXL memory
ZNS NVMe
remote/distributed object/archive storage
```

## 22.1 I/O strategy

Linux should support benchmarkable backends such as:

- mmap/page cache;
- asynchronous I/O;
- `io_uring`;
- optional direct I/O for specific workloads.

Do not assume `io_uring` is automatically faster. Per-shard vs. dedicated I/O-core submission/completion remains benchmark-gated.

## 22.2 Prefetch

Prefetch should use obvious sequential/locality patterns first. Learned/workload-aware prefetch is optional later. Prefetch is charged against background resource budgets so it cannot harm foreground latency.

---

# 23. Query Architecture

There is one canonical typed query system.

```text
Rust API --------+
ChronQL ---------+
Compact ChronQL -+--> Logical Query IR -> Optimizer -> Physical IR -> Executor
Prepared Query --+
Visual Builder ---+
MCP --------------+
SQL compatibility +
```

No front end owns independent execution semantics.

## 23.1 Logical operators

Initial IR should express at least:

- current state;
- temporal range;
- N-D/spatial range;
- entity history;
- causal walk;
- replay/reconstruction;
- branch operations;
- predicates;
- projection;
- aggregation/summary resolution;
- knowledge queries;
- provenance/explanation;
- similarity/vector query where enabled;
- budgets/cursors.

Conceptual Rust:

```rust
pub enum Query {
    Current(CurrentQuery),
    Range(RangeQuery),
    EntityHistory(EntityHistoryQuery),
    CausalWalk(CausalWalkQuery),
    Replay(ReplayQuery),
    Branch(BranchQuery),
    Knowledge(KnowledgeQuery),
    Similarity(SimilarityQuery),
}
```

## 23.2 Optimizer

The optimizer may choose:

- projection;
- shard fanout;
- segment/block set;
- summary level;
- physical layout;
- filter/index;
- compressed predicate path;
- storage tier;
- prefetch;
- exact vs. approved approximate resolution.

---

# 24. ChronQL: Human Query Language

ChronQL is the human-readable native language.

Example:

```text
FROM chronicle
ENTITY 928
TIME valid 10s..20s
KNOWN_AS_OF 20.5s
WHERE visible AND enemy
SELECT position, health
LIMIT 32
```

Spatial example:

```text
FROM chronicle
SPACE x 100..200, y 500..600
TIME 10s..12s
WHERE enemy AND visible
SELECT entity, position
```

Causal example:

```text
EVENT 18272
TRACE CAUSES DEPTH 8
```

Knowledge example:

```text
FROM knowledge
MATCH enemy.attack
AT knowledge_time 14.2s
CONFIDENCE > 0.8
WHY
```

Branch example:

```text
BRANCH FROM event:728
TRY left, right, forward
RUN 5s
COMPARE survival, damage_taken, reward
```

The parser lowers immediately into typed IR. Query semantics never live only in parser code.

---

# 25. Compact ChronQL and AI Token Efficiency

A second canonical shorthand is intended for LLMs and other text-generating agents.

Provisional compact examples:

```text
c:#928@v10..20@k20.5?vis&enemy>pos,hp!32
```

```text
c:$18272<-8
```

Potential symbol vocabulary:

```text
c:   Chronicle
e:   Evolution
k:   Knowledge
#    entity
$    event
@v   valid/world time
@k   known time
[]   dimensional range
?    predicate
>    projection
!    limit
~    similarity
<-   causes
->   effects
```

This grammar is provisional until tokenized against target model tokenizers. Character count is not the optimization metric; actual model-token count is.

## 25.1 Prepared queries

Prepared queries are expected to be the most token-efficient mode for repetitive agent behavior.

```text
PREPARE q7 = visible enemies around entity $1 over previous $2
```

Then the model emits:

```text
q7 928 -5
```

A prepared query also fixes an output schema, eliminating repeated field names.

## 25.2 Result formats

Clients may request:

```text
Arrow
Binary
Tensor-oriented view
JSON
Compact text
Table
Summary
```

For prepared text queries, a schema may be declared once and subsequent rows returned positionally.

---

# 26. Query Budgets and Continuations

Every expensive query can carry resource/information budgets.

Supported budget concepts should include:

- tokens;
- bytes;
- events;
- wall-clock/deadline;
- I/O bytes;
- result rows/items;
- resolution/exactness.

Example:

```text
BUDGET TOKENS 512
RESOLUTION AUTO
```

or an equivalent compact form.

The planner may answer with a coarser summary plus a continuation when exact full detail would violate a declared budget.

Pagination must use stable continuation cursors, not SQL-style large `OFFSET` scans.

A cursor references a stable logical execution position such as segment/block/offset/query-version information.

---

# 27. Cost Estimation and EXPLAIN

The optimizer estimates cost before execution:

- candidate segments;
- candidate blocks;
- estimated bytes read;
- estimated bytes decoded;
- estimated events;
- cross-shard fanout;
- storage tier accesses;
- reconstruction depth;
- expected latency.

`EXPLAIN` should reveal both logical and physical plans.

Example:

```text
EntityTimelineProjection
 -> 185 candidate segments
 -> membership filter: 7
 -> time bounds: 2
 -> compressed scan
 -> 43 results

Estimated bytes: 12.4 KB
Cross-shard fanout: 0
```

A runaway query may be summarized, budget-limited, rejected by policy, or require explicit override depending on client type.

---

# 28. Data Ingestion and Subscriptions

Chronicle supports both pull and push workflows.

## 28.1 Ingestion forms

- direct Rust API batch/append;
- C ABI/Arrow batches;
- shared-memory stream;
- Arrow Flight stream;
- file/import tooling;
- application/model adapter;
- sensor/event adapter.

## 28.2 Push subscriptions

Models/applications may subscribe to predicates rather than poll continuously.

Example:

```text
SUBSCRIBE
WHERE enemy.visible
AND distance < 20m
```

Subscriptions compile to IR and execute against the same projections/filters as ordinary queries.

## 28.3 Backpressure

All streaming ingestion paths are bounded and expose backpressure rather than allowing unbounded buffering.

Policies may choose block, shed approved low-value data, aggregate, or spill to WAL/staging according to schema guarantees.

---

# 29. Connectivity and Transport Architecture

Chronicle uses one protocol/capability model across several transports.

## 29.1 Native Rust API

Primary Tzeentch integration and fastest in-process access.

No socket, JSON, or RPC overhead.

## 29.2 C ABI + Arrow

A deliberately small stable C ABI exposes lifecycle, ingestion, query, subscription, and buffer/result handling.

Arrow-compatible buffers are preferred for bulk columnar interchange with Python, C++, Julia, Go, Java, and ML runtimes.

## 29.3 Local IPC

For separate processes on one machine:

- control plane: Unix domain socket on Unix-like systems, named pipe on Windows;
- data plane: shared memory plus bounded ring buffers/descriptor queues.

This is preferred for high-rate local model workers that should not share the Chronicle process.

## 29.4 Remote high-throughput transport

Arrow Flight is the preferred remote bulk transfer candidate for:

- event ingestion;
- query result streams;
- replay streams;
- columnar/tensor-like feature transfer.

Remote transport requires authentication and TLS in production deployments.

## 29.5 MCP

Chronicle ships an official MCP server, but MCP is not the bulk data plane.

MCP is used for:

- AI discovery;
- schema/capability inspection;
- query execution;
- explanation/provenance;
- replay/branch control;
- model/adapter inspection;
- documentation resources.

Potential tools:

```text
chron.describe
chron.query
chron.explain
chron.replay
chron.trace
chron.branch
chron.compare
chron.knowledge
```

Potential resources:

```text
chronicle://schema
chronicle://capabilities
chronicle://query-language
chronicle://models
chronicle://namespaces
chronicle://examples
```

## 29.6 SQL compatibility

SQL is useful for analytics/tool compatibility but is not the canonical native semantic language.

A Rust/Arrow engine such as DataFusion is a strong candidate for exposing selected Chronicle datasets as tables without forcing Chronicle's causal/branch/knowledge semantics into SQL.

## 29.7 HTTP administration

A simple HTTP surface may exist for health, metrics, administration, and basic integrations. It is not intended as the highest-performance ingestion/query path.

---

# 30. Self-Description and Capability Negotiation

Any client can discover the server without source-code knowledge.

Examples:

```text
DESCRIBE
DESCRIBE entity
DESCRIBE CAPABILITIES
DESCRIBE TRANSPORTS
DESCRIBE MODELS
DESCRIBE QUERY
```

The machine-readable capability response should describe:

- namespaces;
- schemas/fields/dimensions;
- time types;
- query operators;
- result encodings;
- storage/query capabilities;
- model adapter types;
- optional vector/causal/branch/evolution features;
- protocol/version information.

Workbench, MCP, SDKs, and AI agents consume the same capability data.

---

# 31. Model-Agnostic Integration

Chronicle does not assume a specific model architecture.

Supported roles may include:

```text
policy
predictor
embedder
reranker
encoder
world_model
classifier
value_model
custom
```

A model manifest describes:

```text
ModelManifest
├── id/version
├── role/capabilities
├── input schemas
├── output schemas
├── device/runtime
├── batching requirements
├── determinism
├── latency class
├── statefulness
└── resource limits
```

Potential consumers:

- Tzeentch cells/organs;
- RL policies/value functions;
- LLM agents;
- vision/audio models;
- world models;
- GNNs;
- forecasting models;
- classical ML;
- ONNX/TensorRT services;
- remote API models.

## 31.1 Model worker isolation

Arbitrary Python/model/plugin code should run out of process by default.

Trusted Rust modules may opt into in-process execution when required for latency and after compatibility/security review.

A model worker protocol may expose:

```text
describe
load
infer
embed
observe/update
health
unload
```

Capabilities are explicit; not every worker implements every operation.

---

# 32. Chronicle Workbench / Studio

A first-party graphical application is required for adoption and debugging.

## 32.1 Connection manager

Supports:

- embedded/local;
- IPC;
- remote Flight/TLS;
- server health;
- MCP status;
- authentication/profile settings.

## 32.2 Schema explorer

Tree view of:

```text
WORLD
ENTITIES
EVENTS
KNOWLEDGE
EVOLUTION
MODELS
BRANCHES
```

Field details include type, dimensions, units, retention, exactness, codec/layout, index/projection, and consumers.

## 32.3 Query editor

Features:

- ChronQL syntax highlighting;
- autocomplete;
- schema-aware validation;
- compact-query preview;
- model token-count estimate;
- prepared-query creation;
- query history/bookmarks;
- EXPLAIN plan;
- live cost/bytes estimates.

## 32.4 Query plan explorer

Visualize pruning and execution:

```text
query
 -> projection
 -> segment candidates
 -> filters
 -> block candidates
 -> compressed predicates
 -> decode survivors
 -> result
```

Show actual/estimated:

- latency;
- segments/blocks;
- bytes read/decoded;
- cache hit;
- cross-shard messages;
- reconstruction depth.

## 32.5 Visualization modes

Typed result IR maps to:

- table;
- timeline;
- 2D space;
- 3D space with time scrubber;
- causal graph;
- knowledge graph;
- branch tree;
- evolution lineage tree;
- raw event stream;
- model activity.

Visualization is progressive. A 2000-pixel timeline requests a few thousand summary buckets, not 100 million raw events.

## 32.6 Model manager

Workbench displays connected model workers, input/output schemas, device/runtime, health, query rates, inference latency, Chronicle bandwidth, and active subscriptions.

## 32.7 Ingestion wizard

Users can map source fields to Chronicle schema fields for files, streams, sensors, applications, models, or external databases, then generate adapter/config code.

---

# 33. EKS: Evolutionary Knowledge Substrate

Chronicle stores evidence. EKS stores derived interpretation.

Knowledge types:

```text
Observation
Claim
Belief
Concept
Rule
Model
Skill
```

A conceptual knowledge node contains:

```text
id/type
representation
confidence
uncertainty
valid time
learned time
evidence[]
contradictions[]
dependencies[]
parent/version
transformation
predictions/outcomes
fitness
```

Knowledge objects reference Chronicle evidence rather than copying historical payloads.

---

# 34. Provenance and Truth Maintenance

A belief is not only a value; it records why it is currently supported.

```text
Belief
├── evidence
├── assumptions
├── contradictions
└── dependent beliefs
```

When supporting evidence changes or is invalidated, dependent belief confidence/state is reevaluated.

Historical belief versions are retained according to lineage/retention policy rather than overwritten as if the old belief never existed.

Queries should answer:

- Why is this believed?
- When was it learned?
- Which evidence supports it?
- What contradicts it?
- What depended on it?
- Which transformation generated it?
- What predictions did it make?
- How accurate was it?

---

# 35. Predictive Knowledge

Knowledge quality should be measurable where possible.

A central form is:

```text
P(S(t+1) | S(t), A(t), context)
```

EKS records:

```text
prediction
observed outcome
prediction error
confidence adjustment
context/generalization
```

Knowledge fitness may combine:

- predictive accuracy;
- generalization;
- calibration/uncertainty quality;
- memory cost;
- inference cost;
- stability;
- task reward where applicable.

This prevents the knowledge layer from becoming an unmeasured collection of symbolic assertions.

---

# 36. Knowledge Consolidation and Active-Memory Scaling

Chronicle history may grow indefinitely under retention policy. Active cognition must not.

Repeated episodes are consolidated:

```text
raw episodes -> episode cluster -> pattern -> concept/rule/model
```

Knowledge tiers:

```text
ACTIVE       frequently used/current
REFERENCE    valid but rarely accessed
ARCHIVED     superseded/low-value lineage
RAW EVIDENCE Chronicle
```

A high-level query retrieves the most consolidated representation first. `WHY` or increased requested resolution descends to rules, episodes, and exact Chronicle evidence.

---

# 37. Transformation Engine

Transformations are first-class versioned objects, not permanently hardcoded invisible functions.

Examples:

- episodes -> pattern;
- patterns -> rule;
- rules -> generalized rule;
- history -> summary;
- model -> compressed/specialized model;
- query expression -> optimized expression;
- representation A -> equivalent representation B.

Transformation metadata includes:

- input/output types;
- preconditions;
- implementation ID/version;
- historical success;
- CPU/memory cost;
- correctness constraints;
- parent/mutation lineage.

Initial transformations are ordinary handwritten Rust implementations behind stable interfaces.

E-graphs/equality saturation may be used selectively for expression/rule equivalence exploration if benchmarks demonstrate value. They are not a universal representation.

---

# 38. Evolution Engine

The same champion/challenger protocol is generalized across storage and knowledge.

Lifecycle:

```text
incumbent
 -> mutate/recombine/generate candidate
 -> isolate
 -> evaluate
 -> compare
 -> promote or discard
 -> retain lineage/metrics
 -> rollback if regression detected
```

Candidate classes may include:

- codec;
- segment layout;
- index/filter;
- checkpoint policy;
- prefetch/cache policy;
- shard placement;
- knowledge rule;
- retrieval policy;
- transformation;
- model/skill;
- Tzeentch organ;
- mutation operator.

Candidates operate through stable interfaces and cannot bypass validation.

---

# 39. Adaptive Physical Memory

Chronicle may optimize its own physical representations using observed workload telemetry.

A sealed segment can compare:

```text
incumbent: Morton + Delta + LZ4
candidate: adaptive SFC + BitPack + Ribbon
```

against a representative shadow workload.

A possible storage fitness objective incorporates:

```text
latency
RAM
storage bytes
CPU
read amplification
write amplification
background cost
```

Different segments may settle on different winners.

The system does not assume there is a universal best index or codec.

---

# 40. Meta-Evolution

After ordinary mutation is proven safe and beneficial, mutation policies themselves may become candidates.

Example:

```text
MutationPolicy v1
40% representation
30% rule
20% parameter
10% exploratory
```

may be replaced by a policy with a different distribution because its historical candidate yield is superior.

Meta-evolution remains subject to the Immutable Constitution and may be disabled entirely if it fails the benchmark/behavior gate.

---

# 41. Immutable Constitution

The following are non-evolvable at runtime:

- event/history integrity rules;
- provenance semantics;
- segment format validation;
- resource accounting/enforcement;
- candidate isolation;
- rollback mechanism;
- promotion protocol;
- ABI/capability compatibility checks;
- lineage audit requirements;
- authentication/authorization boundaries;
- retention policy enforcement semantics;
- safety limits on destructive operations.

Evolution can propose artifacts above this boundary, but it cannot rewrite the rules that determine whether those artifacts may be trusted or promoted.

---

# 42. Scaling Strategy

Chronicle is designed so database size is not normally on the critical path.

The governing metric is:

> **How many bytes and blocks must Chronicle touch to answer the typical query?**

A database growing from 1 TB to 100 TB is acceptable if a typical local query still reads approximately the same small number of relevant blocks.

## 42.1 Scale hierarchy

```text
Global Directory
 -> Node (future)
 -> Virtual Shard
 -> Epoch
 -> Segment
 -> Block
 -> Event
```

Every level has pruning metadata.

## 42.2 No giant global row table

History is naturally partitioned into immutable segments. Queries locate a small candidate set by time, locality, entity projection, and summaries before disk reads/decompression.

## 42.3 Local indexes

Indexes are segment/shard local and compact. Global metadata points to ranges/partitions, not individual events.

## 42.4 Automatic repartitioning

Hot virtual regions may split; cold sparse regions may merge. Placement can adapt based on:

- data density;
- mutation rate;
- query rate;
- cross-shard traffic;
- NUMA topology;
- memory pressure.

## 42.5 Adaptive segment sizing

High-mutation combat-like regions may seal small temporal segments. Quiet areas may use larger segments. Cold history may be packed into larger archive supersegments.

## 42.6 Lifecycle compaction, not endless LSM rewriting

The preferred path is:

```text
mutable epoch -> seal -> encode -> immutable
```

with occasional lifecycle/archive rewrite rather than repeated overlapping-level compaction.

## 42.7 Multi-node future

Virtual shard IDs and transport-independent protocol semantics are chosen from day one so a later distributed implementation can assign shard ranges to machines without changing the logical data model.

---

# 43. Retention and Data Lifecycle

Retention is explicit, schema-aware, auditable, and never silently destructive.

## 43.1 Temperature tiers

```text
HOT      recent/high-use, DRAM/full indexes/dense checkpoints
WARM     recent NVMe, lossless compressed, useful indexes
COLD     old/low-use, stronger compression, sparse checkpoints/indexes
ARCHIVE  very old/low-use, large segments, minimal active metadata
```

Temperature is a function of more than age:

```text
recency + query frequency + importance + dependency count + replay cost
```

Old but frequently referenced evidence may warm automatically.

## 43.2 Per-field/per-source retention

Example policy:

```text
raw_video:       7 days
visual_features: 90 days
semantic_events: forever
important_branches: policy-defined
failed candidate artifacts: short-lived
candidate lineage metadata: long-lived
```

These are examples; actual defaults must be chosen per deployment.

## 43.3 Exactness classes

Schemas declare whether data must remain exact or may become bounded-error in cold storage.

Example:

```text
health: exact forever
entity_id: exact forever
collision_state: invariant
position: exact recent, <= 1 cm cold error allowed
```

Lossy/error-bounded storage is opt-in and must preserve declared semantic invariants. Exact Chronicle evidence is never approximated when the schema forbids it.

## 43.4 Deletion and legal/operational policy

Retention/deletion operations are audited. Destructive actions obey permissions and branch/reference dependency rules. A referenced immutable object cannot be physically removed until reference/lifecycle policy permits it.

## 43.5 Branch/candidate garbage collection

Most temporary counterfactual branches and failed evolutionary artifacts are removed after evaluation. Keep compact lineage metadata (ID, parent, score, rejection reason, hashes) without retaining every large artifact indefinitely.

---

# 44. Metrics and Observability

Metrics are part of the architecture, not a debugging afterthought.

## 44.1 Latency and throughput

- p50 / p95 / p99 / p99.9 latency;
- operations per second;
- ingest events/s;
- replay events/s;
- branch creation/divergence cost;
- background task completion/backlog.

## 44.2 Storage efficiency

- bytes/event;
- bytes/entity live state;
- compression ratio by stream/codec;
- index RAM / data size;
- metadata bytes / data bytes;
- checkpoint bytes / event bytes;
- branch sharing ratio;
- knowledge bytes / Chronicle evidence bytes.

## 44.3 Amplification and movement

- read amplification;
- write amplification;
- bytes read/query;
- bytes decoded/query;
- bytes transferred across shards;
- NVMe bytes/query;
- cache line/cache miss counters where available.

## 44.4 Query health

- segments/query;
- blocks/query;
- filter rejection ratio;
- bitmap selectivity;
- summary hit ratio;
- cache hit ratio;
- reconstruction depth;
- cross-shard messages/query;
- query continuation frequency;
- budget truncation/summarization frequency.

## 44.5 Shard/runtime health

- per-shard CPU;
- per-shard queue depth;
- virtual-shard skew;
- NUMA remote access indicators;
- background pressure/throttling;
- I/O queue depth;
- allocator/arena utilization.

## 44.6 Knowledge/evolution health

- knowledge nodes per Chronicle event;
- active vs. archived knowledge;
- prediction accuracy/calibration;
- contradictions/revisions;
- candidate generation rate;
- candidate promotion rate;
- regression/rollback rate;
- mutation operator yield;
- candidate evaluation cost.

---

# 45. Initial Performance Targets

These are engineering targets for representative local hardware, not promises. They must be measured on the actual Tzeentch target machines and benchmark servers.

| Operation | Initial target |
|---|---:|
| Packed current-state hot lookup | < 1 us; aspirational 0.05-0.5 us |
| Cached historical point | ~1-5 us |
| Cached local ~1K-event range | ~5-50 us |
| Cold NVMe point | ~50-200 us (device dominated) |
| Cached replay ~10K events | ~30-300 us |
| NVMe replay ~10K events | ~0.2-2 ms |
| Buffered ingest | initial 1-5M changes/s per CPU group; stretch 10M+ |
| Foreground global locks | effectively zero |

For the previously modeled 100M-change synthetic workload, a long-term compact-history target of roughly 1-2.5 GB remains an aspirational design target, highly dependent on data entropy, event sparsity, checkpoint policy, and exactness requirements.

---

# 46. Canonical Benchmark Program

Before advanced intelligence/evolution is accepted, Chronicle must prove itself against simpler baselines.

## 46.1 Workloads

A. Current packed state.  
B. Event append stream.  
C. Spatial/N-D local history.  
D. Entity history.  
E. Deterministic replay.  
F. Branch/counterfactual forks.  
G. Mixed Tzeentch-like fast/medium/slow/background workload.  
H. Billion-event scale.  
I. Skew/hotspot workload.  
J. Background-pressure workload.  
K. Cold-storage/archive workload.  
L. Query-budget/token-oriented AI workload.

## 46.2 Baselines

Where practical:

- direct Vec/HashMap baseline;
- redb;
- Fjall;
- RocksDB;
- SQLite;
- PostgreSQL;
- Redis;
- TileDB;
- vector store for similarity-specific comparisons only.

Comparisons must match equivalent semantics. Do not compare a semantic ANN search with an exact coordinate lookup as if they were the same operation.

## 46.3 Hardware dimensions

Test at minimum:

- desktop x86-64;
- Jetson-class ARM64 where Tzeentch runs;
- local NVMe;
- constrained memory mode;
- multiple core counts (1/2/4/8/16/... where available).

---

# 47. Testing and Verification

## 47.1 Correctness

- property tests for encoding/decoding;
- deterministic replay property tests;
- branch structural-sharing correctness;
- temporal/bitemporal semantics;
- causal graph consistency;
- schema compatibility;
- prepared-query equivalence to full ChronQL;
- result budget/continuation correctness.

## 47.2 Durability

- crash injection during append/seal/publish;
- WAL/staging recovery;
- partial/truncated segment detection;
- checksum corruption tests;
- restart/rebuild of indexes from immutable evidence where supported.

## 47.3 Fuzzing

Fuzz:

- binary segment parser;
- ChronQL parser;
- C ABI inputs;
- transport frames;
- schema negotiation;
- malformed model manifests;
- continuation cursors.

## 47.4 Concurrency

- shard ownership invariant tests;
- message-order/race tests;
- background publication consistency;
- pressure throttling;
- NUMA/shard migration where implemented.

## 47.5 Evolution safety

- candidate isolation;
- benchmark reproducibility;
- promotion atomicity;
- rollback;
- no mutation of immutable evidence;
- lineage completeness.

---

# 48. Security and Operational Boundaries

Chronicle is not initially a public Internet database, but the architecture must not preclude secure deployment.

Requirements:

- TLS for remote production transports;
- authenticated clients;
- namespace/schema-level authorization;
- separate read/query, ingest, branch, mutation/evolution, and administration permissions;
- resource quotas/query budgets;
- model worker isolation;
- no arbitrary plugin execution in the core by default;
- audit log for destructive retention, promotion, schema changes, and administrative operations;
- cryptographic or strong integrity verification where required;
- safe handling of MCP/model-initiated high-cost queries through budgets and policy.

The Immutable Constitution owns these enforcement semantics.

---

# 49. Documentation and Developer Experience

Extensive documentation is mandatory for adoption.

Recommended documentation tree:

```text
docs/
├── 00-start-here/
├── 01-concepts/
├── 02-install/
├── 03-ingestion/
├── 04-query/
├── 05-models/
├── 06-rust/
├── 07-python/
├── 08-remote/
├── 09-mcp/
├── 10-storage/
├── 11-evolution/
├── 12-tzeentch/
├── 13-workbench/
├── 14-security/
├── 15-performance/
├── 16-internals/
├── 17-format-spec/
└── ai/
```

Core specifications:

```text
ARCHITECTURE.md
QUERY_LANGUAGE.md
BINARY_FORMAT.md
MODEL_ADAPTER_SPEC.md
TRANSPORT_SPEC.md
MCP.md
BENCHMARKING.md
SECURITY.md
RETENTION.md
CONTRIBUTING.md
```

## 49.1 AI-specific documentation

AI consumers should not require hundreds of pages in context.

Ship a compact machine-facing bundle:

```text
docs/ai/primer.md           # target ~1-2K tokens
docs/ai/query-compact.md
docs/ai/schema.json
docs/ai/capabilities.json
docs/ai/examples.txt
```

MCP exposes these as resources.

## 49.2 Formal specifications

The query language and protocol require:

- EBNF/reference grammar;
- typed Rust AST/IR;
- machine-readable schemas;
- canonical examples;
- compatibility/version rules.

---

# 50. Tzeentch Integration

Tzeentch consumes Chronicle through a dedicated adapter crate. Chronicle never imports Tzeentch internals.

Conceptual boundaries:

```rust
pub trait ChronicleSource {
    fn observe(&mut self, event: &ObservationEvent) -> Result<(), ChronicleError>;
}

pub trait ChronicleMemory {
    fn current(&self, query: CurrentQuery) -> Result<CurrentResult, ChronicleError>;
    fn history(&self, query: Query) -> Result<QueryResult, ChronicleError>;
    fn replay(&self, query: ReplayQuery) -> Result<ReplayResult, ChronicleError>;
}

pub trait KnowledgeProvider {
    fn retrieve(&self, query: KnowledgeQuery) -> Result<KnowledgeResult, ChronicleError>;
}

pub trait EvolutionProvider {
    fn submit_candidate(&self, candidate: Candidate) -> Result<CandidateId, ChronicleError>;
}
```

Interfaces will be refined to avoid unnecessary ownership/copying and to support async/streaming variants where appropriate.

## 50.1 Timing integration

Chronicle must respect different Tzeentch timescales rather than dumping the entire organism state every tick.

Conceptual cadence:

```text
FAST       60-120+ Hz sensory/motor/event capture
MEDIUM     5-20 Hz attention/memory consolidation
SLOW       0.5-5 Hz planning/world-model work
BACKGROUND compression/evolution/learning
```

Actual rates remain workload-dependent. Ingestion is event-driven whenever practical.

## 50.2 Tzeentch causal introspection

Selecting an action should allow the full trace:

```text
world -> perception -> cell/organ -> knowledge -> prediction -> decision -> action -> outcome
```

This is a core research/debugging capability, not merely logging.

---

# 51. Milestone Roadmap

## Phase I - Foundation

**M0 Specification + benchmark harness**  
Lock workloads, metrics, competitors, reproducibility, and project conventions.

**M1 Packed live state**  
StateSlab, narrow types, generation IDs, direct-offset benchmark.

**M2 Event Chronicle**  
Append-only deltas/change masks and basic entity/time retrieval.

**M3 Deterministic reconstruction**  
Checkpoints, RNG/external input capture, replay property testing.

**M4 Immutable segment format**  
Versioned/checksummed zero-copy-friendly persistence.

**M5 Compression framework**  
Raw/bitpack/delta/FOR/XOR/LZ4/Zstd and candidate selection.

## Phase II - Chronicle

**M6 N-D chunking/layouts**  
Grid/Morton first; other layouts as candidates.

**M7 Alternate projections**  
Entity, spatial, temporal, event bitmap paths without payload duplication.

**M8 Virtual-shard execution**  
Single-writer ownership, core scaling, shard-local allocators.

**M9 Background task DAG**  
Seal/compression/index/summary/maintenance/evolution classes and pressure control.

**M10 Storage hierarchy**  
DRAM/mmap/NVMe tiers, cache and I/O backend benchmarks.

**M11 Filters/compressed execution**  
Membership filters, min/max, bitmaps, predicate pruning.

**M12 Hierarchical summaries**  
Multi-resolution overview/visualization support.

**M13 Branching timelines**  
Persistent DAG and structural sharing.

**M14 Multi-time semantics**  
Valid/observed/known/logical causal time.

**M15 Causal history**  
Compact event causal projection and tracing.

## Phase III - Interfaces and Adoption

**M16 Typed Query IR**  
Canonical logical/physical query representations.

**M17 ChronQL + Compact ChronQL**  
Human grammar, compact grammar, token benchmark, prepared queries.

**M18 Protocol/capability layer**  
Self-description, cursors, budgets, schemas.

**M19 Native + C/Arrow + local IPC**  
High-performance local model/application integration.

**M20 Arrow Flight remote transport**  
Remote bulk data/query streams.

**M21 MCP server**  
AI discovery, querying, explanation, replay, branching, docs resources.

**M22 SQL compatibility**  
Optional DataFusion-based analytics surface.

**M23 Workbench v1**  
Connections, schema explorer, query editor, EXPLAIN, table/timeline/space views.

**M24 Documentation v1**  
Start-here, APIs, query language, protocols, model integration, AI primer.

## Phase IV - Knowledge

**M25 Knowledge primitives**  
Observation/claim/belief/concept/rule/model/skill.

**M26 Provenance + truth maintenance**  
Evidence/dependencies/contradictions and revision.

**M27 Predictive knowledge**  
Prediction/outcome/error/calibration fitness.

**M28 Knowledge consolidation**  
Episode cluster -> pattern -> concept/rule/model and active/reference/archive tiers.

**M29 Transformation engine**  
First-class versioned transformations.

**M30 E-graph/rewrite experiments**  
Only where benchmarked useful.

## Phase V - Evolution

**M31 Champion/challenger evolution engine**  
Candidate isolation, evaluation, promotion, rollback, lineage.

**M32 Adaptive Physical Memory**  
Per-segment codec/layout/index candidates.

**M33 Adaptive lifecycle policies**  
Checkpoint, segment size, summaries, cache, tier, shard placement.

**M34 Semantic/vector projection**  
Selected episodes/models only; exact and associative memory remain distinct.

**M35 Knowledge mutation**  
Candidate beliefs/rules/models evaluated against Chronicle.

**M36 Transformation mutation**  
Transformations themselves become candidate artifacts.

**M37 Meta-evolution**  
Mutation-policy evolution after prior layers prove safe/valuable.

**M38 Immutable Constitution hardening**  
Explicit security/resource/provenance/promotion invariants and audits.

## Phase VI - Tzeentch and Scale

**M39 Tzeentch adapter**  
Stable Chronicle/EKS/provider interfaces.

**M40 Tzeentch timing + causal introspection**  
Fast/medium/slow/background integration and complete action trace.

**M41 Workbench Tzeentch Explorer**  
World/state/cells/memory/knowledge/causality/evolution/branch views.

**M42 Scale tests**  
4K -> 64K -> 1M -> 10M+ active entities/cells and billion-event histories.

**M43 Retention/lifecycle stress**  
Hot/warm/cold/archive, branch GC, candidate GC, long-term compaction.

**M44 Static vs. adaptive evaluation**  
Prove adaptive Chronicle and EKS/evolution beat simpler static incumbents or remove non-winning complexity.

---

# 52. Hard Gates

## Gate A: Is Chronicle actually better for its target workload?

If the packed/event/segment engine fails to beat simpler baselines in the intended state-history workload, fix/simplify it before EKS.

## Gate B: Does knowledge improve behavior and debuggability?

If EKS does not improve prediction, decision quality, explanation, or memory efficiency, simplify it before adding more evolutionary complexity.

## Gate C: Does adaptation/evolution outperform static engineering?

If adaptive codecs/layouts/knowledge mutations do not produce repeatable gains after accounting for background cost and regressions, disable/remove them.

The project does not keep complexity merely because it is philosophically appealing.

---

# 53. Risk Register

## Complexity explosion

**Risk:** Chronicle + EKS + evolution becomes several projects at once.  
**Mitigation:** strict phases and gates; Chronicle v1 must stand alone before knowledge/evolution.

## False performance comparisons

**Risk:** comparing unlike semantics produces misleading claims.  
**Mitigation:** canonical equivalent workloads and publish full benchmark methodology.

## Background work destroys tail latency

**Risk:** compression/evolution consumes cache/memory/I/O resources.  
**Mitigation:** pressure-aware scheduling; foreground wins; resumable tasks.

## Metadata/index growth

**Risk:** alternate projections reproduce the same billion-row scaling problem.  
**Mitigation:** local indexes, hierarchical summaries, compressed bitmaps, membership filters, lifecycle tiers.

## Replay cost explosion

**Risk:** overaggressive event-only storage makes arbitrary history reconstruction slow.  
**Mitigation:** adaptive checkpoints driven by measured replay/query cost.

## Branch/candidate explosion

**Risk:** counterfactual/evolution artifacts consume unbounded storage.  
**Mitigation:** structural sharing plus explicit lifecycle/GC and compact retained lineage.

## Knowledge graph explosion

**Risk:** every event becomes a permanent high-level node.  
**Mitigation:** Chronicle references, consolidation, active/reference/archive tiers.

## Self-optimization regressions

**Risk:** adaptive representation harms performance/correctness.  
**Mitigation:** shadow evaluation, stable contracts, atomic publication, rollback, Constitution.

## Model/plugin instability

**Risk:** arbitrary model code crashes or corrupts the DB.  
**Mitigation:** out-of-process workers by default; explicit trusted in-process path.

## Query language complexity

**Risk:** ChronQL becomes harder than SQL and opaque to models.  
**Mitigation:** small typed IR, human syntax, compact syntax, prepared IDs, self-description, AI primer, Workbench visual builder.

---

# 54. Key Architectural Metrics for Long-Term Success

The project should continuously answer these questions:

1. **How many bytes must Chronicle touch for the median and p99 query?**
2. **How many historical bytes are stored per meaningful state change?**
3. **How much RAM is metadata/index versus useful hot state?**
4. **How many cross-shard messages does a typical query require?**
5. **How deep is typical replay reconstruction?**
6. **How much foreground latency is caused by background work?**
7. **How much active knowledge must enter cognition for the next decision?**
8. **How much knowledge is copied versus referenced/consolidated?**
9. **How often do evolutionary candidates actually beat incumbents after total cost?**
10. **Can every meaningful Tzeentch decision be traced to evidence and knowledge available at the time?**

These are more important than raw database row count.

---

# 55. Final Architectural Position

The final system is not simply a 4D database.

It is a layered architecture:

```text
Storage
  -> exact memory
    -> knowledge
      -> transformation
        -> evolution
          -> meta-evolution
```

with an immutable evidence and integrity layer underneath it.

Chronicle provides:

> **What actually happened?**

EKS provides:

> **What does the system currently believe it means, and why?**

Transformation provides:

> **Can that representation be made simpler, more general, or more useful?**

Evolution provides:

> **Which candidate change measurably improves the system?**

Tzeentch provides:

> **How should the organism perceive, predict, decide, act, learn, and evolve using those capabilities?**

The common operational loop is:

```text
Observe
 -> Remember
 -> Abstract
 -> Predict
 -> Act
 -> Measure
 -> Transform
 -> Mutate
 -> Evaluate
 -> Promote / Roll Back
```

while Chronicle preserves an auditable, exact or explicitly-policy-bounded record of the underlying experience.

This document is the architecture baseline. Implementation prompts and coding-agent instructions should derive from this specification rather than introducing parallel architecture decisions without an explicit revision.

---

# Appendix A. Provisional Public API Shapes

These are illustrative rather than frozen ABI definitions.

```rust
pub struct Chronicle {
    // model-agnostic engine handle
}

pub struct QueryContext {
    pub budget: QueryBudget,
    pub consistency: ConsistencyMode,
    pub branch: BranchId,
}

pub struct QueryBudget {
    pub max_bytes: Option<u64>,
    pub max_events: Option<u64>,
    pub max_tokens: Option<u32>,
    pub deadline_ns: Option<u64>,
    pub max_io_bytes: Option<u64>,
}

pub trait QueryExecutor {
    fn execute(&self, ctx: &QueryContext, query: &Query)
        -> Result<QueryResult, ChronicleError>;
}

pub trait IngestSink {
    fn append(&mut self, batch: EventBatch<'_>) -> Result<AppendReceipt, ChronicleError>;
}

pub trait Projection {
    fn kind(&self) -> ProjectionKind;
    fn estimate(&self, query: &Query) -> CostEstimate;
}
```

---

# Appendix B. Provisional Compact Query Examples

| Intent | Human ChronQL | Compact candidate |
|---|---|---|
| Entity history | `ENTITY 928 TIME 10s..20s` | `c:#928@10..20` |
| Visible enemies | `WHERE visible AND enemy` | `?vis&enemy` |
| Select pos/hp | `SELECT position, health` | `>pos,hp` |
| Causes depth 8 | `EVENT 18272 TRACE CAUSES DEPTH 8` | `c:$18272<-8` |
| Known-as-of | `KNOWN_AS_OF 20.5s` | `@k20.5` |
| Limit | `LIMIT 32` | `!32` |

The compact syntax is not final until model-token benchmarks demonstrate that it actually reduces token consumption and remains reliably learnable.

---

# Appendix C. Candidate Telemetry Record

```text
QueryTelemetry
├── query_id
├── logical_kind
├── prepared_query_id?
├── shard_count
├── cross_shard_messages
├── segments_considered
├── segments_read
├── blocks_considered
├── blocks_read
├── bytes_read
├── bytes_decoded
├── cache_hits/misses
├── reconstruction_events
├── summary_level
├── result_items
├── elapsed_ns
└── background_pressure_snapshot
```

---

# Appendix D. Decision Summary

| Area | Decision |
|---|---|
| Core language | Rust |
| Primary abstraction | Evolving sparse state/history, not rows |
| Live state | Packed separate StateSlab |
| History | Event/delta + replay + adaptive checkpoints |
| Persistence | Immutable versioned segments |
| N-D layout | Logical N-D; physical per-segment adaptive candidates |
| Time | Valid/world + observed + known/commit + causal order |
| Concurrency | Virtual shards, single writer per owned shard |
| Background | Separate priority/task classes on immutable work |
| Query | One typed IR; ChronQL + compact + prepared + API |
| AI efficiency | Prepared queries, compact output, token budgets |
| Local integration | Rust, C ABI/Arrow, IPC/shared memory |
| Remote bulk | Arrow Flight candidate |
| AI control | Official MCP server |
| SQL | Compatibility surface, not native semantics |
| UI | First-party Chronicle Workbench |
| Models | Model-agnostic manifest/worker adapters |
| Knowledge | EKS references Chronicle evidence |
| Evolution | Champion/challenger with rollback/lineage |
| Safety | Immutable Constitution |
| Scaling | Hierarchical pruning/local indexes/tiers/repartitioning |
| Retention | Explicit tiered, schema-aware, audited |
| Lossy data | Opt-in bounded-error only with semantic constraints |
| Success criterion | Cost proportional to relevant data, not total history |