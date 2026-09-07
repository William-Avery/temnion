# Temnion architecture

**Status:** maintained design and implementation boundaries for the initial
foundation. Sections marked *planned* are contracts and roadmap direction, not
available APIs. The [roadmap](ROADMAP.md) tracks every source M0–M44 milestone.

## 1. Authority and names

The [original Markdown and DOCX](architecture/source/README.md) are preserved
unchanged as the historical architecture baseline. Their Chronicle/ChronQL names,
provisional APIs, performance targets and example defaults are not the current
product contract. This document, accepted ADRs and versioned implementation
documentation carry current decisions; changes must retain source traceability.

| Component | Required name |
| --- | --- |
| Database | **Temnion** |
| Server daemon | `temniond` |
| CLI | `tem` |
| Human query language | **TemQL** |
| Compact query syntax | **Tem**, with `tn:` prefix |
| Desktop application | **Temnion Studio** — Tauri 2 + Rust + React/TypeScript; TanStack where useful |
| Rust package family | `temnion-*` |
| MCP server | `temnion-mcp` |
| Native protocol | **Temnion Protocol / TNP** |
| Storage format | **Temnion Segment Format / TSF** |

The current packages implement live state, scalar schemas, source-log persistence
and a CLI. Names do not reserve unimplemented capabilities. WAL/batch and TSF v1
magic/layouts are specified in [BINARY_FORMAT](BINARY_FORMAT.md). Future MCP names
use `temnion.*` and resources use `temnion://`; protocol framing and query grammar
still require their own specifications and compatibility fixtures.

## 2. Purpose and non-goals

Record an evolving system's state and exact history without conflating evidence
with interpretations. Retrieve only the resolution and relevant blocks needed,
while preserving exactness and provenance. Enable standalone simulations,
temporal applications, agents and world models without requiring any particular
consumer, model family, GPU, or inference framework.

The complete architecture has five layers:

1. **Exact memory:** live state, immutable events, checkpoints, reconstruction,
   temporal queries, causal history and branches.
2. **EKS — Evolutionary Knowledge Substrate:** evidence-linked observations,
   claims, beliefs, concepts, rules, models and skills.
3. **Transformations:** versioned abstraction, consolidation, rewriting and
   representation changes.
4. **Evolution:** isolated champion/challenger evaluation, measured promotion,
   lineage and rollback.
5. **Immutable Constitution:** non-evolvable integrity and operational rules
   constraining every layer above them.

Temnion is not a general PostgreSQL replacement, universal Redis substitute,
vector database replacement or arbitrary scientific array engine. MCP is not a
bulk transport. Approximate history, arbitrary in-process model code,
distributed transactions and speculative optimizations are not defaults.

## 3. Implemented foundation

```text
tem / embedded examples / baseline runner
                |
       +--------+---------+
       |                  |
temnion-state      temnion-events
 StateSlab<T>       EventLog<T>
       |                  |
       +--------+---------+
                |
          temnion-core
       typed IDs and time
                |
      Rust standard library
```

The diagram shows the original volatile components. The current workspace also
has `temnion-schema`, `temnion-format`, and `temnion-storage` beneath the CLI.
It uses edition 2024, resolver 3, MSRV 1.85 and pinned Rust 1.85.0. Project source
forbids unsafe code. Explicit checksum/OS-lock/entropy dependencies support disk
storage; there is no async runtime, network stack, GUI or model runtime.

- `StateSlab<T>` stores dense live values with a separate slot directory,
  generation checks and explicit capacity. Swap removal can change iteration
  order without changing surviving entity IDs.
- `EventLog<T>` owns one source/epoch's bounded event sequence. Append and batch
  validation return volatile receipts; accepted storage has no mutable event API.
  Payload ownership and any interior mutability remain the caller's responsibility.
- Valid, optional observed, and known timestamps have explicit clock domains.
  Event identity is source/epoch/sequence, independent of entity identity.
- History filters support entity, a clock-scoped half-open time range, and an
  inclusive known-as-of cutoff. Pages bound records returned and records scanned.
  Queries use linear scans, not an index. Continuations retain an in-process
  snapshot prefix; they are not TNP, serialized or authenticated cursors.
- `temnion-schema` provides exact scalar values, immutable field metadata,
  sparse mutations and bounded deterministic binary encoding. It is not a
  persistent schema registry or a tensor/N-D schema engine.
- `Store` locks one database directory, validates and synchronizes complete WAL
  batches, recovers source-local identity/order, and exposes bounded disk-backed
  history. Ordinary open rejects incomplete tails; authorized recovery reports
  discarded bytes. TSF exports are immutable and the WAL remains authoritative.
- `tem` provides truthful capabilities plus initialization, low-level append,
  inspection, bounded history, explicit recovery and segment export/validation.
  `durable` and `tsf` are true; `server`, `temql`, `tnp`, `mcp` and `studio` remain
  false. Its original demo still uses volatile arrival-order state, not replay.
- In-memory and explicitly selected durable batch baselines do not establish a
  performance gate.

`StateSlab` and `EventLog` are independent components: state changes and appends
are separate calls, with no atomic combined state/history transaction. Capacity
limits count slots/records, not bytes in arbitrary generic payloads. Callers must
scope unique shard IDs and source/epoch incarnations for volatile components.
`Store` separately allocates a persistent DatabaseId and resumes its durable
source sequence. There is still no automatic reducer/materializer, external
input deduplication, replay, persistent schema registry, shared query IR,
virtual-shard scheduler or production security boundary.
See [ADR 0001](adr/0001-foundation-contracts.md) for the original volatile
contracts and [ADR 0002](adr/0002-durable-foundation.md) /
[STORAGE](STORAGE.md) for the persistent increment.

## 4. Locked boundaries

These decisions require an explicit architecture revision to change:

- Rust-first, model-agnostic core; no GC-dependent storage/query hot path.
- Mutable current state physically separate from append-oriented historical
  evidence; sealed history immutable.
- Exact evidence below derived knowledge; knowledge references evidence IDs.
- Tzeentch is a consumer through an adapter, never a core dependency. No Tzeentch
  source is copied into this repository.
- Virtual-shard identity independent of thread/node placement, with one foreground
  writer per owned shard group; no global shared mutable hot database object.
- Bounded admission and background queues with visible backpressure/errors;
  no silent event drops. A virtual shard does not imply one OS thread.
- One canonical typed logical/physical query IR, shared by thin frontends.
- Foreground execution separated from bounded, resumable background work.
- Integrity, authorization, resources, retention, promotion and rollback enforced
  below evolvable implementations from their introduction, not bolted on at M38.
- Benchmarks, tests, documentation and runnable examples are part of completion.

Current exclusive `&mut` APIs express single-owner mutation. They do not implement
a multi-shard runtime, shared-memory transport, global snapshot, or queueing system.

## 5. Exact history and durability — implemented increment and remaining design

### Identity, schema and temporal semantics

Specify database/source/shard/entity/event/branch/schema namespaces, restart
uniqueness, source-epoch persistence, overflow and schema evolution **before
freezing persistent encodings**. Rust struct layout is neither TSF nor a C ABI.
Schema contracts will cover typed fields, units, dimensions and exactness.

Keep four meanings distinct: world/valid time, observation/capture time,
known/commit time, and logical causal order. Ticks require a declared origin and
unit; clock mappings require explicit evidence, uncertainty and versioning.
Source sequence is not a global wall clock or a cross-source causal graph.
Late arrivals and corrections append evidence; they do not rewrite what was
known previously. Future known-as-of reconstruction must exclude future input.

### Durable publication and recovery

The current durable acknowledgment follows `File::sync_all`, not enqueueing,
copying to RAM, or an unsynchronized write. Append I/O failure poisons the writer
and explicitly reports an unknown commit outcome. The current exporter implements
checked non-overwriting TSF publication; manifest activation and WAL retirement
in the full lifecycle below remain unimplemented.

The planned lifecycle is:

```text
validate / admit
 -> framed, checksummed WAL / mutable epoch
 -> synchronize before durable acknowledgment
 -> bounded seal into new TSF objects
 -> validate / checksum / synchronize objects
 -> durably publish versioned manifest
 -> retire WAL only when recovery no longer needs it
```

Specify group commit, retry/idempotency, torn records, directory/manifest
synchronization, allocation bounds and Windows/Linux filesystem differences.
WAL, TSF, manifests and catalogs are all versioned persistent contracts.
Corruption, missing committed segments, unsupported versions, disk-full and
partial publication must be explicit errors, not silently skipped records.
Crash injection must cover every append/sync/seal/publish/retirement boundary.

Start with a simple lossless representation and explicit manifest. A metadata
store may be adopted only with demonstrated recovery/measurement benefit; it
does not silently replace the specialized history engine.

### Reconstruction, branches and causality

Capture checkpoints, explicit RNG state, versioned reducers/models and all
required external inputs for deterministic providers. Prove bit-for-bit replay
against recorded outcomes. Recorded state hashes alone do not recreate an
uncaptured external world. Report unsupported reconstruction rather than invent
missing state.

Branches form a persistent DAG sharing immutable history until divergence.
Queries bind to an explicit branch/snapshot. Causal projections link events and
retain gaps where instrumentation is absent. Cross-shard reads require a defined
consistent cut/fence; per-source batch atomicity does not imply a cross-shard
relational transaction.

## 6. Physical organization and runtime — planned

Use a directory of shard/epoch/segment/block ranges, not a giant global row table.
One payload representation can have compact entity, time, spatial/N-dimensional
and event-type projections. Bounds, membership filters, bitmaps and summaries
progressively prune candidate blocks before reading/decompression.

Lossless per-stream codecs start with raw/simple incumbents; bitpack, delta,
FOR, XOR, LZ4/Zstd and alternatives are candidates where semantics fit.
Grid/Morton are initial locality candidates, not universally optimal layouts.
Hierarchical summaries/data mipmaps serve explicit overview requests without
silently substituting approximation for exact answers.

Virtual shards allow ownership and placement to change without changing identity.
Use owner-local state/allocators, bounded fanout and deterministic merge order.
Background DAG tasks include sealing, compression, indexing, summaries,
maintenance and eventually evaluation. Tasks must yield/resume and throttle on
foreground pressure, memory, queue or I/O limits.

Start with portable DRAM/page-cache/local-NVMe paths. Tune shard count, segment
size, checkpoint spacing, NUMA placement and CPU allocation from measurements.
No promise is made that Hilbert/SFC variants, learned indexes, ALP, Binary Fuse,
Ribbon, SIMD trees, mmap/direct I/O, `io_uring`, or a custom reactor will win.
CXL, ZNS, device-resident Arrow buffers and multi-node operation are later hooks,
not dependencies of the initial database.

## 7. Query contracts and interfaces — planned

All frontends lower to one typed logical IR, validated against schemas and
capabilities, then a typed physical plan. The reference executor defines
semantics; optimized plans must match it.

TemQL is the human language; Tem with `tn:` is compact syntax. Both, native Rust,
prepared queries, MCP, optional SQL, IPC and Studio must agree on results, errors,
precision and authorization. The historical compact examples are provisional,
not accepted CLI input. Grammar/EBNF and actual tokenizer measurements precede
language compatibility claims.

Specify ordering and tie-breaking, snapshots/branches, cancellation, errors,
budgets and partial results. Future ceilings include rows, bytes, scanned events,
memory, I/O, deadline, fanout and optional model tokens. Token budgets identify
the tokenizer/accounting policy; character counts are not exact model tokens.
Summary fallback requires explicit permission and precision/truncation metadata.

Persistent or remote continuations bind query/schema version, stable snapshot,
branch, ordering, authorization and expiry. Retention/compaction must preserve
their logical position or return a documented error. Predicate subscriptions
share the IR and specify snapshot-to-live handoff, delivery/resume order,
disconnect recovery, overflow, cancellation and reference holds.

Interface order:

1. **Embedded Rust**, then `temniond`/`tem` around the same engine.
2. **TNP**, capability/version/error/stream semantics; local IPC over Windows
   named pipes or Unix sockets. Shared memory follows only after ownership,
   bounds, synchronization, crash recovery and release rules are proven.
3. **C ABI and Arrow C Data/Stream**, with opaque handles, explicit release,
   panic containment and typed status. No implicit Rust-layout compatibility.
4. **Optional Arrow Flight**, authenticated/TLS-protected remote bulk transport.
5. **`temnion-mcp`**, using the official Rust SDK on the control plane for
   discovery, bounded query/explain/replay/trace/branch/knowledge operations only
   as implemented. Large results use authorized bounded pages or bulk handles.
6. **Optional DataFusion/SQL**, for applicable table-like data without pretending
   SQL captures every temporal, causal or knowledge operation.

Tokio may serve control-plane work, but networking/JSON/MCP/GUI/model dependencies
must not become mandatory in storage/query hot paths. No remote listener is
enabled by default.

## 8. Temnion Studio and model workers — planned

Studio uses **Tauri 2 + Rust + React/TypeScript**, with TanStack Query/Table/Router
where useful and a lightweight frontend build. No SSR/server stack is required
merely to deliver a desktop UI.

The Rust backend owns connections, credentials, engine validation and filesystem/
process permissions. The webview receives bounded typed view models, not millions
of JSON events or a second database implementation. Use narrow Tauri commands,
capabilities, CSP and untrusted-content handling.

Studio v1 covers connections, schemas, TemQL editing/completion, compact/prepared
previews, history/bookmarks, EXPLAIN with cost/actual metrics, paged tables,
timeline/2D/3D views and a field-mapping ingestion wizard. Later views cover
branches, causal/knowledge graphs, lineage, model health and Tzeentch introspection.

Native Linux ARM64 Studio is a required future deliverable, not remote-only UI.
Qualify Tauri/WebKitGTK 4.1, graphics, packaging and desktop integration on a
supported ARM64 image, targeting AGX Orin / JetPack 6.2.2 / L4T 36.5 / Ubuntu 22.04.
Headless operation remains independent. Do not replace the NVIDIA image to hide
userspace incompatibility; cross-compilation proves neither native execution
nor desktop feasibility.

Model manifests describe identity/version, runtime, schemas/capabilities,
statefulness, inputs/outputs and resources. Bounded out-of-process workers
provide model independence. Start with a deterministic test worker and a real
adapter; no model framework or accelerator is required by the core.

## 9. EKS and transformations — planned, after Gate A

EKS stores versioned `Observation`, `Claim`, `Belief`, `Concept`, `Rule`, `Model`
and `Skill` records with evidence IDs, assumptions, contradictions, dependencies,
confidence/uncertainty, valid/learned times and parent/transformation lineage.
It must not relabel inferred tracks or beliefs as measured ground truth.

Truth maintenance reevaluates affected interpretations without erasing old
versions. `WHY` traverses support and contradictions back to exact evidence.
Predictions connect to outcomes, errors and calibration. Historical evaluation
must only see evidence and learned knowledge available at the requested time.

Active/reference/archive tiers bound working knowledge while retaining
provenance. Consolidation moves episodes through patterns into concepts/rules/
models; higher-level retrieval can descend to evidence when requested.

Transformations have input/output types, preconditions, implementation versions,
correctness constraints, measured CPU/memory cost and lineage. Start with
handwritten Rust transformations. E-graphs/equality saturation remain a focused,
benchmark-gated experiment, not a universal representation.

Gate B requires held-out, time-correct evidence that justified knowledge improves
consumer outcomes or explanatory/memory value before evolutionary complexity.

## 10. Safe evolution and immutable Constitution — planned

Evolution is disabled until its prerequisites and gates pass. The lifecycle is:

```text
static incumbent -> candidate -> isolate -> evaluate -> compare
                  -> reject or manually approve / atomically promote
                  -> monitor -> rollback on regression
```

Pin datasets, seeds, models, environment and query snapshots. Count evaluation
CPU/RAM/I/O, foreground tail effects, storage amplification and rollback costs.
Candidate code requires platform-appropriate isolation; a child process alone is
not a sandbox. Promotion starts with manual approval, not autonomous replacement.

Candidate classes include codecs/layouts/indexes/filters; checkpoint, segment,
cache/prefetch/tier/shard policies; rules/retrieval/models/skills; and
transformations. Semantic/vector projections are optional associative access,
not substitutes for exact state/history. Physical rewrites publish new immutable
representations with preserved logical content and compatible readers.

**The Constitution cannot evolve at runtime.** Its protected rules include:

- event/history integrity and provenance semantics;
- segment/metadata format validation and ABI/capability compatibility;
- authentication/authorization and destructive-operation safety;
- resource accounting/enforcement and candidate isolation;
- retention/reference enforcement;
- promotion, rollback and lineage/audit requirements.

Enforce each boundary when its feature is introduced. M38 hardens and audits
them; it does not postpone correctness/security until late in the roadmap.
Candidates cannot change the rules used to trust or promote themselves.
Constitution changes require an explicit human-reviewed architecture/software
revision outside runtime evolution.

Gate C requires repeatable net improvement over a static incumbent with no
integrity/compatibility or foreground-SLO regression. Losing paths stay disabled
or are removed. Meta-evolution of mutation policies is considered only after
Gate C and remains subject to the same unchanged Constitution.

## 11. Lifecycle, scale and operational evidence — planned

Default to exact data and no automatic deletion or lossy conversion. Hot/warm/
cold/archive placement reflects access, importance, dependencies and replay cost,
not age alone. Optional bounded-error storage requires explicit per-schema/field
permission, preserved invariants, and precision-aware query results.

Reference accounting protects snapshots, branches, readers, cursors and knowledge
evidence during rewrite/GC. Authorized audited deletion distinguishes logical
visibility from physical reclamation and reports holds. Preserve compact candidate
lineage even when failed candidate payloads expire. Storage exhaustion returns
actionable errors/backpressure.

Production readiness requires health/metrics, backup/restore, integrity inspection,
safe repair/index rebuild, schema/format migration, upgrade tests and crash recovery.
Measure relevant bytes/blocks touched, not just database size. Qualify 4K through
10M+ active records and billion-event histories only on provisioned hardware.
Include skew, fanout, cold/archive reads, queue saturation, long-running ingestion,
background contention, branch/candidate GC and consumer rollback.

## 12. Tzeentch integration — planned, separate ownership

Provide a standalone, feature-gated adapter and a separately reviewed consumer
change. Keep consumer types, source formats, clock mappings and model assumptions
out of the core; a non-Tzeentch example must use the same engine APIs.

Start with versioned, rights-cleared fixtures and idempotent imports. Preserve
original artifacts and independent intention/action/percept/hash/keyframe streams.
Namespace source identities by recording/episode and preserve clock domains.
External media references require independent validation.

Mirror bounded writes alongside the existing recorder before changing read
defaults. Queue admission is not durable completion; drain/failure and source-gap
reporting must be explicit. Preserve existing memory/recording behavior until
parity, latency and rollback gates pass. Retrieval affecting decisions becomes a
recorded input pinned to historical known-time and memory snapshot.

Eventually trace observation -> processing -> retrieved knowledge -> prediction
-> decision -> action -> outcome -> update, preserving missing instrumentation as
gaps. Respect fast/medium/slow/background scheduling rather than blocking motor
loops on synchronization or dumping the entire state each tick.

Tzeentch's declared MIT/Apache licensing remains separate. Temnion's AGPL or
commercial terms do not resolve rights for a combined distribution.

## 13. Completion and maintenance

[Gates A/B/C](ROADMAP.md#gates) govern added complexity.
[BENCHMARKING](BENCHMARKING.md) defines equivalent comparisons and required
reporting; source performance targets are aspirations, not results.
[COMPATIBILITY](COMPATIBILITY.md) and [ADR 0001](adr/0001-foundation-contracts.md)
define the pre-persistence contract work.

Each implemented feature requires relevant tests, documented limitations and a
runnable example. Later parsers/formats need round-trip, differential, fuzz and
fault-injection coverage; concurrency and unsafe/FFI tooling are added only when
those surfaces are justified. Studio requires real backend/end-to-end tests, not
mock-only screenshots. Publish native platform evidence before claiming support.
