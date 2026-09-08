# Changelog

## Unreleased - Standalone daemon and operational lifecycle (Post-R6)

- Added Standalone Daemon Application (`apps/temniond`):
  - **Multi-Protocol Daemon Binary (`temniond`):** Always-on background database service hosting high-performance binary TNP wire protocol over TCP sockets (`127.0.0.1:9180`), Arrow Flight analytical service (`127.0.0.1:9181`), and Model Context Protocol (MCP) control-plane endpoints.
  - **Thread-Safe Architecture:** Pure safe Rust concurrency model under `#![forbid(unsafe_code)]` with `Arc<Mutex<TnpServer>>`, worker connection threads, atomic shutdown coordination, and background maintenance worker for periodic checkpointing.
  - **Daemon Commands:** `run` (foreground / service execution with graceful `Ctrl+C` termination and WAL flush), `init` (store creation and configuration template generation), `status` (zero-downtime TNP health and statistics probe), and `version`.
  - **Configuration Management:** Robust line-by-line TOML configuration parser and generator (`DaemonConfig`, `temnion.example.toml`).
- Service Definitions and Production Hardening (`services/`):
  - **Linux systemd Unit (`services/systemd/temniond.service`):** Production service definition with `Type=simple`, `Restart=on-failure`, `LimitNOFILE=65536`, and strict sandboxing (`ProtectSystem=strict`, `ProtectHome=true`, `ReadWritePaths=/var/lib/temnion`, `NoNewPrivileges=true`).
  - **Windows Service Automation (`services/windows/`):** PowerShell administration scripts (`install-service.ps1` and `uninstall-service.ps1`) for registering, configuring automatic failure recovery, and managing `temniond.exe` as a managed Windows Service.
- Distribution Packaging (`scripts/`):
  - Added release packaging automation (`scripts/package-release.ps1`) building release binaries (`tem`, `temniond`), collecting service units and configuration templates, and creating distribution archives with NIST SHA-256 checksums.
- Published ADR 0015 (`docs/adr/0015-standalone-daemon-and-operational-lifecycle.md`) documenting daemon architecture, multi-protocol hosting, and operational lifecycle.

## Unreleased - Tzeentch integration and release qualification (M39–M44, R6)

- Added Tzeentch Adapter crate (`temnion-adapter`) (M39–M40):
  - **Decoupled Model-Agnostic Adapter (M39):** Pure safe NIST FIPS 180-4 SHA-256 content-addressed media references (`MediaRef`) keeping heavy binaries out of the latency-critical write-ahead log.
  - **Action/Intention Separation (M39):** Distinct domain entity representations for `TzeentchPercept`, `TzeentchIntention`, `TzeentchAction`, `TzeentchOutcome`, and `TzeentchBeliefUpdate` mapped to canonical typed schemas (`101..=106`).
  - **Zero Silent Drops & Dual Writing (M39):** Migration modes (`LegacyOnly`, `ShadowMirror`, `TemnionAuthoritative`, `TemnionOnly`) and bounded queue `MirrorWriter` returning explicit backpressure errors and preserving 100% event delivery without silent loss.
  - **Multi-Cadence Timing Scheduler (M40):** Independent desynchronized clocks for `Fast` (120 Hz reflex/motor), `Medium` (20 Hz organ integration), `Slow` (1 Hz deliberate planning/beliefs), and `Background` (0.1 Hz epistemic consolidation).
  - **Causal Action Tracer (M40):** Transitive causal lineage traversal reconstructing `Percept -> Organ/Cell -> Belief -> Prediction -> Decision -> Action -> Outcome` with explicit `SourceGap` nodes for uninstrumented steps and strict verification of zero future-knowledge leakage.
- Desktop Studio Tzeentch Explorer (`apps/temnion-studio`) (M41):
  - Added native Tauri backend IPC commands (`get_tzeentch_summary`, `get_tzeentch_cadence_stats`, `inspect_tzeentch_action_trace`, `set_tzeentch_migration_mode`).
  - Added interactive TanStack React `TzeentchPanel` featuring live multi-cadence frequency gauges, zero-drop dual-write migration toggle, and a causal action trace lineage inspector.
- Scale Benchmarks & Retention Lifecycle (`crates/temnion-bench`, `crates/temnion-storage`) (M42–M43):
  - Added scale benchmark harness (`crates/temnion-bench/src/bin/scale.rs`) evaluating 4K, 64K, and 1M+ active records with linear throughput and sub-millisecond latencies.
  - Added retention management (`RetentionPolicy`) with strict causal reference holds (`ReferenceHold`) protecting pinned sequences from background eviction.
  - Added branch lifecycle GC and point-in-time CRC32-verified backup and restore manager (`BackupManager`).
- CLI Integrations (`apps/temnion-cli`):
  - Added `tem tzeentch [demo | status | trace <name>]` and `tem benchmark scale [records]` commands.
  - Updated capabilities to advertise `"tzeentch_adapter": true`, `"causal_action_trace": true`, `"scale_qualified": true`, and `"retention_holds": true`.
- Release Qualification & Architectural Documentation (M44):
  - Published ADR 0014 documenting Tzeentch integration, multi-cadence timing, and release qualification.
  - Published formal Release Qualification Report (`docs/reports/release-qualification-m39-m44.md`) certifying conformance across Gates A, B, and C with honest empirical findings.

## Unreleased - Isolated measured evolution and immutable Constitution (M31–M38)

- Added Isolated Measured Evolution Engine (`temnion-evolution`) (M31–M38):
  - **Champion/Challenger Evolution Engine (M31):** Versioned candidate registry (`CandidateRecord`, `CandidateKind`, `CandidateStatus`, `CandidateLineage`), sandboxed isolation budget (`IsolationBudget`), holistic net-benefit scoring, mandatory manual approval before promotion, and instantaneous atomic rollback upon detected regression.
  - **Adaptive Physical Memory (M32):** Representation candidate evaluation over sealed segments comparing codecs/layouts/indexes against static incumbents using holistic storage fitness (accounting for bytes, scan latency, CPU cycles, and read amplification).
  - **Adaptive Lifecycle Policies (M33):** Dynamic tuning of checkpoint intervals (balancing recovery replay latency against snapshot serialization overhead) and cache tier placement thresholds.
  - **Semantic and Vector Projections (M34):** Fixed-dimension dense vector embeddings (`VectorEmbedding`) with cosine similarity, dot product, and euclidean distance, plus `SemanticProjectionIndex` for associative nearest-neighbor retrieval strictly segregated from authoritative exact storage.
  - **Knowledge Mutation (M35):** Generation and evaluation of candidate beliefs and inferential rules against empirical evidence, penalizing contradiction rates and Brier score regressions.
  - **Transformation Mutation (M36):** Mutation and validation of candidate transformation manifests under constitutional budget limits with verified semantic equivalence.
  - **Meta-Evolution (M37):** Mutation policy evolution (`MutationPolicy`) gated strictly behind Gate C net adaptive value verification, with an unconditional runtime hard off-switch.
  - **Immutable Constitution Hardening (M38):** Runtime verification and enforcement of 8 non-evolvable constitutional axioms (`ConstitutionalAxiom`), blocking any attempt to mutate storage evidence, rewrite the Constitution, or bypass promotion gates, backed by a comprehensive audit framework (`ConstitutionAudit`).
- Interface Integrations:
  - Added `evolution_status`, `candidate_evaluate`, and `constitution_audit` tools to Model Context Protocol server (`temnion-mcp`).
  - Added `tem evolve status`, `tem evolve audit`, and `tem evolve demo` CLI commands to `apps/temnion-cli`, and updated capabilities to advertise `"evolution": true`, `"constitution": true`, and `"semantic_projections": true`.
- Published ADR 0013 documenting isolated measured evolution, candidate lifecycle, Gate C criteria, and the 8 immutable constitutional axioms.

## Unreleased - Epistemic Knowledge Store (EKS) and transformation engine (M25–M30)

- Added Epistemic Knowledge Store (`temnion-eks`) (M25–M28):
  - **Knowledge Primitives (M25):** Typed epistemic representations (`Observation`, `Claim`, `Belief`, `Concept`, `Rule`, `ModelManifest`, `Skill`) referencing authoritative storage `EventId`s with bounded confidence scores in $[0.0, 1.0]$.
  - **Truth Maintenance System & Provenance (M26):** Non-destructive justification network, contradiction detection between competing propositions, recursive non-destructive retraction cascading, and `WHY` provenance query traversal (`WhyTrace`) tracing beliefs back to ground truth WAL events and applied rules.
  - **Predictive Knowledge & Calibration (M27):** `PredictionLedger` recording entity prediction horizons, empirical outcome resolution (`Matched`, `Refuted`, `Inconclusive`), and Brier score calibration with strict zero-future-leakage guarantees (filtering out any predictions or observations after $T_k$).
  - **Tiered Knowledge Store (M28):** Three-tier memory management (`Active`, `Reference`, `Archive`) with access-frequency eviction and episode pattern consolidation.
- Added Deterministic Transformation Engine and Canonical IR E-Graphs (`temnion-transform`) (M29–M30):
  - **Transformation Engine (M29):** Versioned manifests (`TransformationManifest`), declared signatures, precondition enforcement, CPU step and memory allocation metering (`ResourceMeter`), and immutable lineage logging (`LineageRecord`) with execution receipts.
  - **Canonical IR E-Graphs (M30):** Equality saturation over canonical query IR (`Expr` and `LogicalPlan`), algebraic boolean identities, constant folding, scan filter simplification, and cycle-safe cost-based plan extraction using iterative Bellman-Ford relaxation.
- Query and Parser Enhancements (`temnion-query`):
  - Added recursive-descent expression parser `pub fn parse_expr` handling operator precedence (`OR`, `AND`, comparisons, `NOT`, literals, parenthesized subexpressions).
  - Implemented `fmt::Display` and `Hash` for scalar query `Expr` and `BinaryOp`.
- Interface Integrations:
  - Added `why_trace` and `rewrite_expr` tools to Model Context Protocol server (`temnion-mcp`).
  - Added `tem why-demo` and `tem rewrite-demo <expr>` commands to CLI (`temnion-cli`), and updated capabilities to advertise `"eks": true`, `"transformations": true`, and `"e-graphs": true`.
- Published ADR 0012 documenting the Epistemic Knowledge Store, truth maintenance, and deterministic transformations.

## Unreleased - initial Temnion Studio desktop slice (M23/M24 increment)

- Replaced the static Studio mockup with a buildable React/TypeScript frontend.
- Added TanStack Query for native command state and cache invalidation, and
  TanStack Table for bounded query/history result rendering.
- Added a Tauri 2 native host with typed, bounded commands for local database
  create/open/disconnect, canonical TemQL/compact Tem/SQL query execution,
  EXPLAIN, durable history, OS-synchronized append, branch inspection and causal
  tracing.
- Added a valid desktop entry point, Tauri build script/configuration, generated
  application icons, npm/Cargo lockfiles and a Windows Studio CI job.
- Added native parser and durable store/query round-trip tests.
- Published `docs/STUDIO.md` with setup, operating boundaries, current limits
  and an explicit list of work still required before full M23 completion.

## Unreleased - SQL compatibility frontend lowering into canonical query IR (M22)

- Added standard relational SQL compatibility frontend to `temnion-query` (M22):
  - `parse_sql`: Parses standard relational and bi-temporal queries (`SELECT ... FROM ... WHERE ... [LIMIT n]`) directly lowering into canonical typed query IR (`LogicalPlan::Scan`).
  - Supports compound bi-temporal constraints: `entity = '#shard:slot:gen'`, `valid_time >= t1 AND valid_time < t2`, `known_as_of = tk`, `known_time <= tk`.
  - Supports relational value comparisons (`=`, `==`, `!=`, `<>`, `<=`, `>=`, `<`, `>`) lowered into strongly-typed `Expr::Binary` AST nodes.
  - Verified canonical plan equivalence across SQL, TemQL, and Compact Tem shorthand syntax.
- Integrated SQL query frontend across all interfaces:
  - Added `QueryFormat::Sql = 3` to wire protocol framing in `temnion-protocol` and enabled `"sql"` capability negotiation.
  - Added SQL query support in safe C ABI (`temnion_c_query_execute`).
  - Added SQL query execution in Arrow Flight remote service `do_get` (`temnion-flight`).
  - Added SQL query and EXPLAIN execution in Model Context Protocol tools (`temnion-mcp`).
  - Updated CLI `tem query` and `tem explain` to support standard SQL syntax, updated `tem describe` to report `"sql": true`, and added `"sql"` to implemented capabilities list.
- Published ADR 0011 documenting SQL compatibility frontend lowering into canonical query IR.

## Arrow Flight and Model Context Protocol (MCP) server (M20, M21)

- Added authenticated Arrow Flight remote analytical transport to `temnion-flight` (M20):
  - Flight message types (`FlightDescriptor`, `Ticket`, `FlightInfo`, `FlightEndpoint`, `FlightData`).
  - Binary packet framing with magic `b"FLGT"`, CRC32C checksums, and 32 MB payload ceilings.
  - Handshake session authentication (`FlightHandshakeRequest`, `FlightHandshakeResponse`).
  - `FlightService` dispatcher implementing `get_flight_info`, streaming `do_get` columnar vector batches (`ColumnarBatch`), and `do_action` controls.
- Added Model Context Protocol (MCP) JSON-RPC 2.0 control-plane server to `temnion-mcp` (M21):
  - Lightweight, safe, self-contained JSON DOM parser and serializer.
  - MCP protocol handling (`initialize`, `ping`).
  - MCP tool catalog: `query`, `explain`, `inspect`, `branch_list`, `causal_trace` with bounded limits and truncation reporting.
  - MCP resource providers: `temnion://database/capabilities`, `temnion://database/branches`, `temnion://database/summaries`.
  - MCP prompt templates: `causal-investigation`, `timeline-audit`.
  - `run_stdio` line-delimited stdio transport for direct CLI and AI agent attachment.
- Updated `apps/temnion-cli`:
  - Added `tem mcp [database-dir]` command for interactive MCP agent hosting.
  - Registered capabilities `flight` and `mcp` in `tem describe` and updated `"mcp": true`.
- Published ADR 0010 documenting Arrow Flight and Model Context Protocol (MCP).

## TNP wire protocol, local IPC, Arrow columnar layout, and C ABI (M18, M19)

- Added canonical Temnion Network Protocol (TNP) framing, capability negotiation, and query streaming to `temnion-protocol` (M18):
  - Binary packet framing (`TnpPacket`) with magic `b"TNPP"`, version 1, 16-byte fixed header, stream multiplexing identifiers, 16 MB maximum packet length, and trailing CRC32C checksum validation.
  - Connection handshake payloads (`HandshakeRequest`, `HandshakeResponse`) with explicit protocol version verification and bitflag capability negotiation.
  - Streaming query protocol emitting `QueryResponse` header metadata, streamed `StreamRecord` frames, and `StreamEnd` terminator.
  - Connection liveness checks (`Ping`, `Pong`) and capability introspection (`DescribeRequest`, `DescribeResponse`).
- Added full-duplex local IPC transport, Arrow columnar batch layout, and safe C ABI to `temnion-protocol` (M19):
  - `TnpChannel<R, W>` transport adapter for duplex pipe and socket byte streams with partial read buffering.
  - `TnpServer` dispatcher processing handshake, describe, ping, and query execution against durable database stores.
  - `ColumnarBatch` memory layout compatible with Apache Arrow columnar formats, supporting lossless bidirectional conversions with `QueryRow` records.
  - Panic-safe, handle-based foreign function interface (`temnion_c_store_open`, `temnion_c_store_close`, `temnion_c_query_execute`, `temnion_c_result_row_count`, `temnion_c_result_free`) protected by thread-safe `HandleRegistry` and `catch_unwind` boundaries.
- Updated `apps/temnion-cli`:
  - Added `temnion-protocol` dependency.
  - Registered capabilities `tnp`, `local-ipc`, `arrow-columnar`, and `c-abi` in `tem describe` and updated `"tnp": true`.
- Published ADR 0009 documenting TNP framing, negotiation, local IPC transport, Arrow columnar layout, and C ABI.

## Typed Query IR, TemQL, and AI-compact Tem shorthand (M16, M17)

- Added canonical typed query IR, physical planner, and reference executor to `temnion-query` (M16):
  - Unified `LogicalPlan` representing relational scans, entity histories, valid/known time ranges, causal DAG traces, and 2D/3D spatial Morton intervals.
  - Strongly-typed `Expr` AST with literals (`Int`, `Float`, `String`, `Bool`), field references, binary comparisons, boolean logic, and unary negation.
  - `PhysicalPlan` optimizer applying predicate pushdown flags (`use_zone_maps`, `use_bloom`) for block skipping during storage scans.
  - Human-readable `ExplainPlan` formatted operator tree output.
  - Reference `QueryExecutor` evaluating physical plans against storage and CSR causal graphs while enforcing `QueryBudget` (max rows, max scanned events, max read bytes).
- Added human-readable TemQL and AI-compact `tn:` shorthand parsers to `temnion-query` (M17):
  - `parse_temql` supporting relational, temporal, field projection, limit, and causal queries (`EVENT <id> TRACE CAUSES/EFFECTS`).
  - `parse_compact_tem` supporting AI-token efficient shorthand (`tn:#<entity>@v<start>..<end>@k<tick>?<filter>><proj>!<limit>`, `tn:$<event><-<depth>`, `tn:$<event>-><depth>`).
  - Verified lowering equivalence guaranteeing identical `LogicalPlan` trees from both human and compact grammars.
- Added `tem query <directory> <query-str>` and `tem explain <query-str>` commands to `apps/temnion-cli`.
- Registered capabilities `query-ir`, `temql`, and `compact-tem` in `tem describe` and updated `"temql": true`.
- Published ADR 0008 documenting Typed Query IR, TemQL, and AI-compact Tem shorthand.

## Virtual shards, background task DAG, and storage hierarchy (M8, M9, M10)

- Added single-writer virtual shards and deterministic merge coordinator to `temnion-runtime` (M8):
  - Strongly-typed `VirtualShardId` and routing policies (`ShardRoutingPolicy::Modular`, `ShardRoutingPolicy::Explicit`).
  - Thread-safe `VirtualShardPartition` with isolated partition logs and zero global mutable database write locks.
  - Multi-shard query coordinator (`VirtualShardCoordinator`) providing deterministic total order sequence merging across partitions.
- Added priority background task DAG and adaptive pressure throttling to `temnion-runtime` (M9):
  - Priority classes (`TaskClass::Seal` weight 100, `Compress` weight 80, `Index` weight 60, `Summary` weight 40, `Maintenance` weight 20).
  - Directed acyclic graph (`TaskDag`) with Kahn's cycle detection and priority-ordered execution scheduling (`ready_tasks`).
  - `PressureController` monitoring memory and write queue depth, adaptively throttling or yielding background work under foreground pressure.
- Added three-tier storage hierarchy to `temnion-runtime` (M10):
  - Multi-tier classification (`StorageTier::HotDram`, `WarmMapped`, `ColdMedia`).
  - `TieredStorageManager` tracking segment byte sizes and capacities, enforcing automated LRU eviction from hot to cold tiers, and promoting hot-accessed cold segments.
- Registered capabilities `virtual-shards`, `background-dag`, and `storage-hierarchy` in `tem describe`.
- Published ADR 0007 documenting virtual shards, background task DAG, and storage hierarchy.

## N-D chunking/layouts and alternate projections (M6, M7)

- Added space-filling curve and N-dimensional chunking layouts to `temnion-index` (M6):
  - 2D and 3D Morton (Z-order curve) bit-dilation encoding and decoding (`morton_encode_2d`, `morton_decode_2d`, `morton_encode_3d`, `morton_decode_3d`).
  - Axis-aligned spatial bounding boxes (`BoundingBox2D`, `BoundingBox3D`) with `contains_point` and `intersects`.
  - Uniform grid chunking (`GridChunker2D`, `GridChunker3D`) and bounding box Morton interval range decomposition (`morton_intervals_chunked`).
- Added zero-payload-duplication alternate projections to `temnion-index` (M7):
  - Inverted entity projection (`EntityProjection`), clock-scoped temporal projection (`TemporalProjection`), spatial Morton projection (`SpatialMortonProjection`), and schema/event-type bitmap projection (`SchemaBitmapProjection`).
  - Composite multi-predicate index (`ProjectionIndex`) evaluating multi-axis query intersections (`query_intersect`) with early short-circuiting.
  - Binary projection index serialization (`TNPR` magic, version 1, CRC32C framing).
- Registered capabilities `nd-layouts` and `alternate-projections` in `tem describe`.
- Published ADR 0006 documenting N-D chunking, Morton space-filling curves, and zero-duplication alternate projections.

## Hierarchical summaries and block skipping (M11, M12)

- Added hierarchical summary and indexing engine (`temnion-index`), featuring `ZoneMap<T>`,
  clock-scoped `TimestampZoneMap`, 512-bit `EntityBloomFilter` with 4 deterministic FNV-1a hash
  seeds, `BlockSummary`, and `SegmentSummary` with binary serialization (`TNSM`) and CRC32C framing.
- Integrated predicate pushdown into `temnion-storage`: `Store::history` evaluates block summaries
  against query filters (entity, valid time, known-as-of) and skips unmatching batch frames without
  reading from disk or performing frame decompression, mathematically guaranteeing zero false negatives.
- Enhanced `Store::seal` to export companion `.tsm` summary files (`{:020}-{:020}.tsm`) alongside `.tsf`
  immutable segments.
- Added CLI command `inspect-summary` to `tem` to validate `.tsm` summary files and print zone map bounds,
  and registered capability `hierarchical-summaries`.
- Published ADR 0005 documenting hierarchical summaries, clock domain isolation, Bloom filters, and
  predicate pushdown block skipping.

## Timeline branching and causal DAG tracing (M13, M14, M15)

- Added structurally shared branching timelines and persistent DAG manifests (`temnion-branch`),
  featuring $O(1)$ zero-payload-duplication forks, atomic `.tmp` to rename manifest updates (`TNBM`),
  branch lifecycle states (Active, Temporary, Candidate, Promoted, Retired), and interval-based
  timeline resolution (`resolve_timeline`) with ancestor chain traversal.
- Added first-class causal graph engine (`temnion-causal`), featuring compact CSR-packed index
  representation (`TNCG`), bidirectional immediate queries (`immediate_causes`, `immediate_effects`),
  bounded transitive DAG tracing (`trace_causes`, `trace_effects`), topological sorting, cycle detection,
  and causal ancestry checks.
- Added CLI commands `branch-create`, `branch-list`, and `causal-trace` to `tem`, supported optional
  causes in `tem append`, and registered capabilities `branching-timelines` and `causal-graph`.
- Published ADR 0004 documenting timeline branching, persistent DAG manifests, and causal DAG tracing.

## Deterministic replay and lossless codecs increment (M3 & M5)

- Added deterministic state reconstruction and replay engine (`temnion-replay`) with
  periodic checksummed checkpoints (`TNCP`), SplitMix64 step-counted PRNG tracking,
  and verified bit-for-bit replay equivalence between checkpointed resume and fresh playback.
- Added lossless codec framework (`temnion-codec`) featuring `RawCodec`, `RleCodec`,
  `BitPackCodec`, `DeltaForCodec`, and `XorCodec` (`TNCX`), with CRC32C framing,
  strictly lossless verification, and dynamic cost scoring with raw fallback.
- Added CLI commands `checkpoint`, `reconstruct`, and `evaluate-codecs` to `tem`, and
  registered capabilities `deterministic-reconstruction` and `lossless-codecs`.
- Published ADR 0003 documenting deterministic replay invariants and candidate codec scoring.

Manifest-based WAL retirement, branches, causal graph queries, and the remaining
application interfaces are still unfinished. This increment does not complete the
database or any production/performance gate.

## 0.1.0 - Foundation

Initial Rust implementation of the Temnion architecture's first milestones.

### Implemented

- Model-agnostic shard, source, epoch, entity and event identifiers.
- Separate valid, observation and known-time clock domains.
- Dense, bounded live state with generation-safe handles and slot reuse.
- Typed, append-only in-memory history with atomic batch admission.
- Entity and temporal filters, known-as-of cutoffs, scan/result budgets and
  snapshot-pinned in-process pagination.
- `tem help`, `tem version`, `tem describe` and a runnable `tem demo`.
- State/history examples and seeded A/B/D reference workloads.
- The supplied architecture originals, licensing documents and implementation
  roadmap.

### Scope

This is a **volatile foundation**, not a production database release. There is
no durable acknowledgment, WAL, TSF persistence, replay engine, combined
state/history transaction, daemon, TemQL parser, TNP transport, MCP server,
EKS or Studio yet. The benchmark runner is an initial reference harness, not
evidence that the full architecture's performance gates have passed.

Future work is tracked in the repository's milestone issues and
[`docs\ROADMAP.md`](docs/ROADMAP.md).
