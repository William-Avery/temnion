# Temnion roadmap

This roadmap covers the **entire** Rust-first standalone exact-history -> EKS ->
transformations -> safe-evolution architecture. It is not a list of features
already shipped. Source milestone identifiers M0–M44 remain traceable to the
[unchanged architecture](architecture/source/README.md); delivery tasks T00–T21
reorder prerequisites to settle identity, time, recovery and integrity early.

## Current delivery

**M0/M1 foundation through M22 interfaces are implemented in staged increments; M23 now has an initial functional Windows desktop slice and M24 documentation continues alongside delivery.**

- Repository/Rust/licensing/documentation/CI foundations and a core contract ADR.
- Typed entity/source/event/clock identities and generic dense generational state.
- Bounded per-source typed in-memory history, atomic batch admission, clock-scoped
  filters, known-as-of selection and bounded snapshot pagination.
- `tem version`, `describe`, scripted `demo`, and embedded examples.
- A small seeded A/B/D baseline runner with simple reference paths.
- Immutable validated scalar schemas, sparse mutations and canonical encodings.
- Checksummed WAL batches, OS-synchronized receipts, process-restart recovery,
  explicit incomplete-tail repair and standalone raw TSF v1 exports.
- Periodic checksummed checkpoints (`TNCP`), SplitMix64 step-counted PRNG tracking,
  and verified bit-for-bit deterministic replay equivalence (`temnion-replay`).
- Strictly lossless codec framework (`temnion-codec`) with Raw, RLE, BitPack, Delta-FOR,
  and XOR compression, CRC32C framing, and dynamic candidate scoring.
- Persistent timeline branching and structural sharing (`temnion-branch`) with $O(1)$
  zero-duplication forks, persistent manifests (`TNBM`), and interval timeline resolution.
- First-class causal graph representation (`temnion-causal`) with CSR-packed index (`TNCG`),
  bidirectional immediate queries, transitive causal/effect cone tracing, topological sort,
  and cycle detection.
- Hierarchical summaries and block skipping (`temnion-index`) with zero-false-negative zone maps,
  clock-scoped timestamp bounds, entity Bloom filters, predicate pushdown during query execution
  (`temnion-storage`), and companion `.tsm` segment summary exports.
- N-dimensional chunking and Morton space-filling curves (`temnion-index`), with 2D/3D bit-dilation,
  bounding box range decomposition, and uniform grid chunkers.
- Zero-payload-duplication alternate projections (`temnion-index`), providing inverted entity,
  temporal, spatial Morton, and schema bitmap indexing with multi-predicate intersection and
  CRC32C-framed binary persistence (`TNPR`).
- Virtual-shard execution (`temnion-runtime`) with single-writer partition ownership and deterministic
  multi-shard sequence merge ordering.
- Background task DAG (`temnion-runtime`) with priority classes (Seal, Compress, Index, Summary,
  Maintenance), cycle prevention, and adaptive foreground pressure throttling.
- Storage hierarchy & tiers (`temnion-runtime`) organizing Hot DRAM, Warm Mapped, and Cold Media with
  LRU-bounded capacity eviction and auto-promotion.
- Canonical typed query IR (`temnion-query`) with unified `LogicalPlan`, strongly-typed `Expr` AST,
  resource budgets (`QueryBudget`), physical operator planner, and human-readable `EXPLAIN`.
- Human-readable TemQL and token-efficient compact Tem (`tn:`) parsers (`temnion-query`), with
  verified canonical lowering equivalence.
- TNP framing/capability negotiation, local IPC, Arrow-compatible columns, safe C
  handles, authenticated Arrow Flight, bounded MCP tools and SQL compatibility
  lowering through the canonical query IR (M18–M22).
- Initial Temnion Studio desktop slice using Tauri 2, Rust, React/TypeScript,
  TanStack Query and TanStack Table for real bounded local open/create, query,
  EXPLAIN, history, append, branch inspection and causal tracing flows.
- Epistemic Knowledge Store (`temnion-eks`) with knowledge primitives (`Observation`,
  `Claim`, `Belief`, `Concept`, `Rule`, `ModelManifest`, `Skill`), truth maintenance,
  cascading non-destructive retraction, `WHY` provenance traversal, predictive
  knowledge ledger with Brier score calibration, strict zero future leakage, and
  tiered memory consolidation (`Active`, `Reference`, `Archive`).
- Deterministic transformation engine and canonical IR e-graph optimizer (`temnion-transform`)
  with versioned manifests, CPU/memory resource metering, immutable lineage logs,
  equality saturation over query expressions and plans, algebraic rewrites, constant
  folding, and cycle-safe cost-based plan extraction (M25–M30).
- Durable CLI operations, checkpoint/reconstruct, evaluate-codecs, branch-create/branch-list,
  causal-trace, inspect-summary, query, explain, why-demo, and rewrite-demo commands.

This does **not** complete R0, R1 or R3: most T01 specifications, most A–L workloads,
full N-D schemas, manifest-based lifecycle, `temniond`, complete Studio v1 flows,
native installers and Linux/ARM64 desktop qualification remain future work.
No Gates A/B/C have passed. Native ARM64/Jetson execution has not been qualified.
See [README](../README.md) for the implemented package inventory.

Status terms:

- **Foundation:** useful implemented subset, not completion of the larger task.
- **Future:** not implemented.
- **Candidate:** a real experiment must demonstrate value; rejection with evidence
  is acceptable, but an unimplemented placeholder is not an experiment.

## GitHub tracking epics

These epics group the original source milestones. The delivery sequence below
moves prerequisite contract and enforcement work earlier; epic numbering does
not override those dependencies or indicate completion.

| Epic | Source milestones | Scope |
| --- | --- | --- |
| [#1 — Rust foundation](https://github.com/William-Avery/temnion/issues/1) | M0–M2 | Foundation contracts, packed state and volatile history; the A/B/D harness is not the complete M0 benchmark program |
| [#2 — Durable replay and segment storage](https://github.com/William-Avery/temnion/issues/2) | M3–M5 | Reconstruction, durable storage/recovery and compression |
| [#3 — Exact-memory engine and scale-up](https://github.com/William-Avery/temnion/issues/3) | M6–M15 | Layouts, projections, ownership/runtime, storage tiers, summaries, branches, time and causality |
| [#4 — Interfaces and Temnion Studio](https://github.com/William-Avery/temnion/issues/4) | M16–M24 | Shared query IR, languages, protocols, interoperability, Studio and adoption documentation |
| [#5 — EKS and transformations](https://github.com/William-Avery/temnion/issues/5) | M25–M30 | Knowledge, provenance, prediction, consolidation and versioned transformations |
| [#6 — Isolated measured evolution](https://github.com/William-Avery/temnion/issues/6) | M31–M38 | Gated candidates, adaptive policies, meta-evolution and Constitution hardening |
| [#7 — Tzeentch integration and release qualification](https://github.com/William-Avery/temnion/issues/7) | M39–M44 | Consumer integration/introspection, scale/lifecycle tests and static/adaptive comparison |

Remaining Gate A workloads and equivalent durable-engine comparisons stay in
T02/T04–T10 and [Gates](#gates). Foundation tests on Windows do not complete the
canonical A–L program or establish all-platform/native ARM64 support.

## R0 — Repository, contracts and baseline

| Task | Status | Deliverable and acceptance |
| --- | --- | --- |
| T00 — Repository and licensing | Foundation | Rust workspace, approved AGPL/commercial documents, contribution-rights policy, useful docs, pinned CI and dependency inventory. No automatic publishing or unverified platform claim. |
| T01 — Invariants and compatibility | Foundation ADR only | Settle identity/time/schema/consistency, Constitution, query/TSF/TNP/WAL/manifest, retention and recovery contracts before freezes. Publish numeric resource limits and compatibility fixtures as implemented. Qualify an early Tauri feasibility prototype on supported desktops, including native ARM64. |
| T02 — Benchmark/reference fixtures | A/B/D foundation only | Seeded A–L workloads, simple independent references, deterministic toy world and rights-cleared consumer-format fixtures. Publish complete hardware/durability/cost metadata and equivalent comparator settings. |

Exit: reproducible development and honest reference evidence. Full specifications,
all workloads and Studio feasibility are not claimed complete by the initial CLI.

## R1 — Durable exact-memory foundation

| Task | Status | Deliverable and acceptance |
| --- | --- | --- |
| T03 — Typed state and events | Packed state, scalar schemas and mutation encoding | Generation-safe state and typed scalar/nullable/unit metadata with sparse atomic apply; N-D schemas, persistent schema registration and full input semantics remain unfinished. |
| T04 — WAL, TSF and recovery | Durable source-log increment | OS-synchronized batches, bounded checksummed WAL/TSF v1, strict recovery and immutable export are implemented. WAL remains authoritative; manifest activation/retirement and its fault matrix remain unfinished. |
| T05 — Replay, branches and causality | Replay, checkpoints, branching (M13), and causal DAG tracing (M15) implemented | Checkpoints, deterministic providers with captured inputs/RNG, temporal reconstruction, causal edges and structurally shared branch manifests. Replay equivalence between checkpoint resume and fresh playback is verified. Branch manifests with O(1) zero-duplication fork and CSR-packed causal DAG tracing are implemented. |
| T06 — Canonical query and languages | Typed Query IR (M16), TemQL and compact Tem parsers (M17), physical planner, EXPLAIN, and reference executor implemented | Unified LogicalPlan, strongly-typed Expr tree, resource budgets (QueryBudget), physical planning with predicate pushdown (PhysicalPlan), human-readable EXPLAIN (ExplainPlan), reference QueryExecutor, human TemQL parser, and AI-compact tn: shorthand parser (temnion-query). Prepared query caching, subscription streaming, and protocol negotiation remain future work. |

Exit: ingest -> durable receipt -> crash/restart -> historical query ->
checkpoint/replay -> branch/causal trace, with explicit precision and limitations.
Cross-shard transactions are not silently inferred from source-local atomicity.

## R2 — Efficient standalone database and first embedded consumer

| Task | Status | Deliverable and acceptance |
| --- | --- | --- |
| T07 — Efficient immutable representations | Lossless codecs (M5), Morton N-D layouts (M6), alternate projections (M7), block skipping (M11), and hierarchical summaries (M12) implemented; candidates gated | Lossless codecs (Raw, RLE, BitPack, Delta-FOR, XOR) with dynamic candidate scoring and raw fallback. Morton 2D/3D space-filling curve encoding, grid chunking, and bounding box interval decomposition. Zero-duplication alternate projections (entity, temporal, spatial, schema) with multi-predicate intersection. Hierarchical summaries (`temnion-index`) with clock-scoped zone maps, entity Bloom filters, and zero-false-negative predicate pushdown block skipping in storage. Hilbert/ALP candidates remain future work. |
| T08 — Virtual-shard runtime and tiers | Virtual-shard execution (M8), background task DAG (M9), and tiered storage hierarchy (M10) implemented | Single-writer virtual partition ownership, core scaling, explicit/modular shard routing, and deterministic total order merge fanout (`temnion-runtime`). Priority-weighted background task DAG (Seal, Compress, Index, Summary, Maintenance), cycle prevention via Kahn's algorithm, and adaptive foreground pressure throttling. Three-tier storage hierarchy (Hot DRAM, Warm Mapped, Cold Media) with LRU eviction and auto-promotion. |
| T09 — Embedded Tzeentch integration | Future | Separate feature-gated adapter and consumer change: fixtures/import, idempotency, bounded mirrored writes, drain/error reporting, historical-read parity and a rollback switch. Keep existing recorder/memory behavior authoritative until gates pass. |
| T10 — Operations and enforcement | Future | Health/metrics, policy/authorization hooks, exact no-auto-delete retention, reference-aware GC, backup/restore, inspection/repair, schema/format migration and auditable operations. Validate recovery and foreground resource protection. |

**Gate A** follows: establish equivalent correctness/durability and repeatable
target-workload value before EKS. Exit: a useful durable standalone exact-memory
database and embedded consumer path. Static Temnion remains useful even if later
adaptive techniques are rejected.

## R3 — Daemon, interoperability, AI control and Studio v1

| Task | Status | Deliverable and acceptance |
| --- | --- | --- |
| T11 — Daemon, full CLI, TNP, C/Arrow and IPC | Implemented for TNP wire protocol, local IPC, Arrow columnar layout, and C ABI (M18, M19); temniond daemon future | Checksummed binary packet framing (TNP), handshake negotiation, streaming query execution, duplex pipe/socket IPC transport (TnpChannel, TnpServer), Arrow-compatible columnar batching (ColumnarBatch), and safe handle-based C ABI (temnion-protocol). Standalone temniond daemon service remains future work. |
| T12 — Optional remote/MCP/SQL | Implemented for Arrow Flight, MCP server, and SQL compatibility (M20, M21, M22) | Authenticated Arrow Flight remote analytical transport (`temnion-flight`), Model Context Protocol server (`temnion-mcp`) with `tem mcp` CLI command, and SQL compatibility frontend (`parse_sql`) lowering directly to canonical Query IR across TNP, Flight, MCP, and CLI (`temnion-query`). |
| T13 — Studio v1 and adoption docs | Initial Windows desktop slice | Tauri 2 + Rust + React/TypeScript with TanStack Query/Table now provides real bounded local open/create, query, EXPLAIN, history/timeline, append, branch inspection and causal tracing. Persistent schema workflows, prepared/continuation UI, branch mutation, richer 2D/3D views, installers and native Linux x64/ARM64 qualification remain future work. See [Studio guide](STUDIO.md). |

Exit: users can install, ingest, inspect, query, explain and visualize through
supported interfaces without reading implementation code. Headless builds stay
free of GUI/npm requirements; native Linux ARM64 Studio is required, not a
desktop-remote-only substitute.

## R4 — EKS, isolated model workers and transformations

| Task | Status | Deliverable and acceptance |
| --- | --- | --- |
| T14 — Knowledge, provenance and prediction | Implemented (`temnion-eks`) | Versioned Observation/Claim/Belief/Concept/Rule/Model/Skill, evidence references, dependencies/contradictions, confidence, valid/learned time, non-destructive truth maintenance, prediction ledger with Brier score calibration, strict zero future leakage, and recursive WHY provenance traversal. |
| T15 — Consolidation and transformations | Implemented (`temnion-eks`, `temnion-transform`) | Active/reference/archive tiered knowledge with capacity eviction, episode-to-pattern consolidation, metered deterministic transformations with CPU/memory tracking and lineage logs, and canonical query IR e-graph optimizer with equality saturation, constant folding, and cycle-safe plan extraction. |

**Gate B** follows: held-out, time-correct evidence of consumer benefit with complete
provenance and accounted active-memory/runtime cost. Exit: Tzeentch and a second
model-agnostic example retrieve justified knowledge available at historical time.

## R5 — Isolated, evidence-driven evolution

| Task | Status | Deliverable and acceptance |
| --- | --- | --- |
| T16 — Champion/challenger control | Implemented (`temnion-evolution`) | Versioned candidate registry (`CandidateRecord`, `CandidateLineage`), sandboxed isolation budget (`IsolationBudget`), net-benefit evaluation scoring, mandatory manual approval before promotion, and instantaneous atomic rollback upon regression. Candidates cannot alter evidence or the Constitution. |
| T17 — Adaptive physical memory/lifecycle | Implemented (`temnion-evolution`) | Shadow-tests candidate segment codecs/layouts/indexes against static incumbents using holistic storage fitness (accounting for bytes, scan latency, CPU cycles, and read amplification). Evaluates dynamic checkpoint intervals (replay recovery latency vs snapshot overhead) and tier boundaries. |
| T18 — Associative and knowledge/transformation mutation | Implemented (`temnion-evolution`) | Optional semantic vector embeddings (`VectorEmbedding`) and similarity index (`SemanticProjectionIndex`) strictly segregated from exact queries. Evaluates candidate beliefs, inferential rules, and transformation manifests against contradiction penalties and calibration fitness. |
| T19 — Meta-evolution | Implemented (`temnion-evolution`) | Evaluates candidate mutation operator distributions (`MutationPolicy`) gated strictly behind Gate C. Requires demonstrated yield improvement over active policy; preserves an unconditional runtime hard off-switch. |

**Gate C occurs before T19**: repeatable net adaptive benefit with no protected
invariant or foreground-SLO regression. Exit: any enabled adaptation is measured,
isolated, auditable, reversible and unable to rewrite its evaluation rules.

## R6 — Full consumer introspection, scale and release

| Task | Status | Deliverable and acceptance |
| --- | --- | --- |
| T20 — Tzeentch causal integration and Explorer | Future | Observation -> processing -> retrieval -> prediction -> decision -> action -> outcome -> update, explicit source gaps, branch/counterfactual and causal/knowledge/lineage/model views. Preserve fast/medium/slow/background timing and bounded UI requests. |
| T21 — Scale, lifecycle, compatibility and releases | Future | Qualify 4K -> 64K -> 1M -> 10M+ active records and billion-event history on provisioned hardware; skew, archive, retention holds, branch/candidate GC, long-running ingest, upgrades, backup/restore and consumer rollback. Publish static/adaptive evidence and native platform release qualification. |

Exit: mandatory interfaces have real examples/tests/docs, losing research
candidates are explicitly classified, and supported platform/release statements
are backed by execution evidence.

## Gates

Gate criteria, datasets, comparator settings, statistical method, meaningful
improvements and allowed regressions must be declared **before** running the
evaluation. Do not choose favorable metrics after inspecting results without a
documented gate revision.

| Gate | Prerequisite and evidence | Failure action |
| --- | --- | --- |
| **A — Exact-memory value** | Durable exact-history correctness/recovery and equivalent semantics; repeatable advantage on target workloads against simple incumbents. Publish absolute distributions and complete storage/metadata/WAL costs. Required before EKS. | Improve or simplify the engine; do not hide durability cost or advance on aspirations. |
| **B — Knowledge value** | Held-out, time-correct fixtures with no future-information leakage; predeclared prediction/calibration, retrieval/decision or explanatory value plus active-memory and total runtime cost; complete provenance. Required before evolution. | Simplify knowledge rather than add evolutionary machinery without demonstrated benefit. |
| **C — Net adaptive value** | Repeatable improvement over static engineering after all evaluation/background CPU/RAM/I/O, foreground-tail, amplification and rollback costs; no integrity/compatibility/SLO regression. Required before meta-evolution. | Disable/remove losing adaptive paths; keep the static baseline and rollback procedure. |

Passing one workload does not establish every source target or every platform.
None of these gates has been passed by the foundation benchmark.

## Complete source milestone traceability

The source names below are restated using current product terminology; the
original files retain their historical names unchanged.

| Source | Capability | Delivery tasks | Current state |
| --- | --- | --- | --- |
| M0 | Specification and benchmark harness | T00–T02 | Foundation; full specs/A–L incomplete |
| M1 | Packed live state | T03 | Implemented foundation |
| M2 | Event history/deltas | T03 | Volatile history plus typed scalar sparse mutation encoding; N-D engine/schema registry remains |
| M3 | Deterministic reconstruction | T05 | Checkpoints, SplitMix64 PRNG tracking, and verified replay equivalence (`temnion-replay`) |
| M4 | Immutable segment format | T04 | Bounded raw TSF v1 and synchronized WAL/recovery; manifest lifecycle remains |
| M5 | Compression framework | T07 | Lossless codec framework (Raw, RLE, BitPack, Delta-FOR, XOR) with dynamic candidate scoring (`temnion-codec`) |
| M6 | N-dimensional chunking/layouts | T07 | Morton 2D/3D SFC bit-dilation, bounding box decomposition, and uniform grid chunking (`temnion-index`) |
| M7 | Alternate projections | T07 | Zero-duplication entity, temporal, spatial Morton, and schema projections with multi-predicate intersection (`temnion-index`) |
| M8 | Virtual-shard execution | T08 | Single-writer partition ownership, modular/explicit routing, core scaling, and deterministic multi-shard merge ordering (`temnion-runtime`) |
| M9 | Background task DAG | T08 | Priority-weighted background task DAG (Seal, Compress, Index, Summary, Maintenance), Kahn cycle detection, and foreground pressure throttling (`temnion-runtime`) |
| M10 | Storage hierarchy | T08 | Three-tier storage management (HotDram, WarmMapped, ColdMedia), capacity limits, LRU eviction, and access-frequency promotion (`temnion-runtime`) |
| M11 | Filters/compressed execution | T07 | Zone maps, entity Bloom filters, and storage predicate pushdown block skipping (`temnion-index`, `temnion-storage`) |
| M12 | Hierarchical summaries | T07 | Block and segment summaries, binary TNSM framing, and companion TSM segment exports (`temnion-index`, `temnion-storage`) |
| M13 | Branching timelines | T05 | Structurally shared timeline branching and persistent DAG manifests (`temnion-branch`) |
| M14 | Multi-time semantics | T03/T06 | Typed clocks/filter subset; valid/known time preserved across branches and intervals |
| M15 | Causal history | T05/T20 | CSR-packed causal graph, bidirectional traversal, and transitive cone tracing (`temnion-causal`) |
| M16 | Typed Query IR | T01/T06 | Canonical LogicalPlan, Expr AST, physical planner, EXPLAIN, and reference executor with resource budgets (`temnion-query`) |
| M17 | TemQL and compact Tem | T01/T06 | Human-readable TemQL grammar and token-efficient compact tn: shorthand with canonical IR lowering equivalence (`temnion-query`) |
| M18 | Protocol/capabilities | T01/T06/T11 | Checksummed TNP binary packet framing, version handshake negotiation, capability self-description, streaming query execution, and continuation cursors (`temnion-protocol`) |
| M19 | Native/C/Arrow/local IPC | T09/T11 | Full-duplex local IPC channel over pipes/sockets (`TnpChannel`, `TnpServer`), Arrow columnar batch layout (`ColumnarBatch`), and panic-safe handle-based C ABI (`temnion-protocol`) |
| M20 | Arrow Flight | T12 | Authenticated Arrow Flight remote analytical transport (`FlightDescriptor`, `Ticket`, `FlightInfo`, streaming `FlightData`, `ColumnarBatch`) (`temnion-flight`) |
| M21 | MCP server | T12 | Model Context Protocol JSON-RPC 2.0 control-plane server (query, explain, inspect, branch_list, causal_trace tools; resources; prompts; stdio transport) (`temnion-mcp`) |
| M22 | SQL compatibility | T12 | Canonical query IR lowering for standard SELECT/WHERE/LIMIT queries; equivalent to TemQL and Compact Tem across TNP, Flight, MCP, and CLI (`temnion-query`) |
| M23 | Studio v1 | T13 | Initial buildable Windows slice with real bounded native database flows; full v1 and platform qualification remain future |
| M24 | Documentation v1 | T00–T21, especially T13 | Foundation docs plus Studio setup/limits guide; grows with implemented features |
| M25 | Knowledge primitives | T14 | Knowledge primitives (`Observation`, `Claim`, `Belief`, `Concept`, `Rule`, `ModelManifest`, `Skill`) referencing authoritative storage `EventId`s with bounded confidence (`temnion-eks`) |
| M26 | Provenance/truth maintenance | T14 | Non-destructive justification network, contradiction detection, cascading retraction, and recursive `why(id)` provenance traversal returning `WhyTrace` (`temnion-eks`) |
| M27 | Predictive knowledge | T14 | `PredictionLedger` with empirical outcome matching, Brier score calibration, and strict zero-future-leakage guarantees (`temnion-eks`) |
| M28 | Knowledge consolidation | T15 | Three-tier knowledge management (`Active`, `Reference`, `Archive`) with access-frequency eviction and episode pattern consolidation (`temnion-eks`) |
| M29 | Transformation engine | T15 | Deterministic transformations with versioned manifests, typed signatures, preconditions, CPU/memory resource metering, and immutable lineage records (`temnion-transform`) |
| M30 | E-graphs/rewrite experiments | T15 | Canonical query IR equality saturation (`EGraph`), algebraic boolean simplification, constant folding, scan filter simplification, and cycle-safe cost-based plan extraction (`temnion-transform`) |
| M31 | Champion/challenger evolution | T16 | Candidate isolation, shadow evaluation, holistic net-benefit scoring, manual approval, atomic promotion, and instantaneous rollback (`temnion-evolution`) |
| M32 | Adaptive physical memory | T17 | Per-segment representation adaptation, shadow-testing codecs/layouts/indexes against static incumbents with holistic storage fitness accounting (`temnion-evolution`) |
| M33 | Adaptive lifecycle policies | T17 | Adaptive checkpoint interval tuning balancing recovery replay latency against snapshot overhead, and dynamic tier thresholds (`temnion-evolution`) |
| M34 | Semantic/vector projection | T18 | Dense vector embeddings (`VectorEmbedding`) and approximate nearest-neighbor index (`SemanticProjectionIndex`) strictly segregated from exact storage (`temnion-evolution`) |
| M35 | Knowledge mutation | T18 | Generation and evaluation of candidate beliefs and rules against empirical support and contradiction penalties (`temnion-evolution`) |
| M36 | Transformation mutation | T18 | Generation and evaluation of candidate transformation manifests under constitutional budget limits (`temnion-evolution`) |
| M37 | Meta-evolution | T19 | Evolvable mutation policy distributions (`MutationPolicy`) gated strictly behind Gate C with runtime hard off-switch (`temnion-evolution`) |
| M38 | Immutable Constitution hardening | T01/T04/T10/T16 | Runtime enforcement of 8 non-evolvable constitutional axioms (`ConstitutionalAxiom`) and comprehensive audit framework (`ConstitutionAudit`) (`temnion-evolution`) |
| M39 | Tzeentch adapter | T02/T09 | Future; no consumer code copied |
| M40 | Tzeentch timing/introspection | T09/T20 | Future |
| M41 | Studio Tzeentch Explorer | T20 | Future |
| M42 | Scale tests | T21 | Future |
| M43 | Retention/lifecycle stress | T10/T21 | Future |
| M44 | Static/adaptive evaluation | Gates A–C/T21 | Future; no gate result claimed |

## Specification and validation map

Publish detailed interface documents as implementation contracts are settled,
not placeholder specifications that imply working interfaces.

| Surface | Current authority | Required future artifacts |
| --- | --- | --- |
| Identity/time/state/events | [ADR 0001](adr/0001-foundation-contracts.md) | Persistent identity, schemas/units/deltas and reference fixtures in T01/T03 |
| Compatibility/Constitution | [Architecture](ARCHITECTURE.md), [compatibility policy](COMPATIBILITY.md) | Versioned enforcement tests and compatibility matrix at feature introduction |
| Durability | ADR pre-format checklist | TSF/WAL/manifest and recovery specifications with fault injection, T04 |
| Query | Architecture's shared-IR requirements | TemQL/Tem EBNF, typed IR, budgets/order/cursors/subscriptions and frontend equivalence, T06 |
| Transport/FFI | Compatibility policy | TNP, C/Arrow ownership, IPC and negotiation specs/tests, T11 |
| Models/MCP/SQL | Architecture's optional boundaries | Versioned worker manifests/protocol, capabilities/resources and access policy, T12/T14 |
| Lifecycle | Architecture's exact/no-auto-delete defaults | Retention/reference/GC, backup/restore and migration specifications, T10/T21 |
| Consumer and Studio | Architecture, milestone requirements and [Studio guide](STUDIO.md) | Adapter mappings/parity/replay limits; complete native packaging and platform-qualified end-to-end flows, T09/T13/T20 |
| Measurement | [BENCHMARKING](BENCHMARKING.md) | Full A–L fixtures, predeclared Gates A/B/C and native platform reports |

Every feature requires implementation, targeted tests, documentation and a
runnable example; performance-sensitive work needs relevant evidence. Optional
research may be rejected after a real evaluation. Empty crates, unsupported
capabilities, mocked UI success, speculative numbers and cross-check-only
“native support” do not meet completion criteria.
