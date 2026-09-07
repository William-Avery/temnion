# ADR 0014: Tzeentch Integration, Multi-Cadence Timing, and Release Qualification

Status: accepted implementation contract for M39–M44 (Tzeentch Integration and Release Qualification), delivering Release R6 (Tasks T20 & T21).

## Problem and Context

Temnion serves as the foundational, deterministic temporal-epistemic substrate for continuous, long-lived autonomous agents and cognitive systems. Tzeentch represents an active, multi-scale consumer architecture comprising distributed organs, cognitive cells, predictive policies, and motor effectors operating concurrently across disparate temporal scales.

Integrating such an advanced consumer without compromising Temnion's architectural guarantees introduces critical challenges:
1. **Zero Internal Coupling Principle:** Temnion's deterministic core (`temnion-core`, `temnion-storage`, `temnion-causal`) must remain completely model-agnostic. It must never import consumer-specific dependencies, heavy machine learning runtimes (e.g., ONNX, LibTorch), or neural weights.
2. **Action/Intention Separation:** In cognitive systems, what an organism intended to do (and why) must be cleanly decoupled from what motor action was executed, and what empirical outcome was observed. Conflating these leads to uninterpretable causal loops.
3. **Heavy Media Integrity:** Perceptual streams (e.g., high-resolution camera frames, raw audio buffers, point clouds) must not pollute the high-throughput, latency-critical write-ahead log (WAL) or columnar time-series format (TSF). Instead, media must be content-addressed via cryptographic hashes with verifiable integrity.
4. **Zero Silent Drops Under Dual Writing:** Migrating a live consumer from legacy persistence to Temnion requires bounded queueing. Under backpressure, silent event dropping is fatal to causal provenance. Dropped events must be surfaced as explicit backpressure errors or explicit gap events.
5. **Multi-Timescale Cadence Semantics:** An organism operates at reflex loops (60–120+ Hz), organ-level attention (5–20 Hz), deliberate planning (0.5–2 Hz), and background consolidation (0.1 Hz). Events cannot be forced onto a single monolithic clock without causal distortion.
6. **Causal Action Trace Introspection:** When an agent takes an action, investigators and operators must be able to introspect the full chain of justification (`Percept -> Organ/Cell -> Belief -> Prediction -> Decision -> Action -> Outcome`). Missing lineage must appear as explicit `SourceGap` nodes, and future evidence must never leak backward into past decision contexts.
7. **Retention Holds and Lifecycle Stress:** Long-running systems require automated retention policies, but critical historical milestones, causal root causes, and benchmark branches must be protected from garbage collection via explicit reference holds.

---

## Architectural Decisions

### 1. Dedicated Consumer Adapter Layer (`crates/temnion-adapter`, M39)

A dedicated, isolated crate (`temnion-adapter`) bridges consumer domain abstractions to Temnion's core schemas without polluting core storage engines:
- **`MediaRef`**: Implements content-addressed external media references storing URI, mime type, byte size, and pure safe NIST FIPS 180-4 SHA-256 content hashes (`sha256_digest`). Heavy media remains in external object stores or filesystems; Temnion stores and verifies the tamper-proof cryptographic reference.
- **Action/Intention Domain Entities**:
  - `TzeentchPercept`: Raw sensory capture tagged with source organ, sensor ID, cadence tier, optional `MediaRef`, and numerical feature vectors.
  - `TzeentchIntention`: Cognitive plan specifying goal label, policy ID, target feature expectations, and deliberate justification.
  - `TzeentchAction`: Executed motor command with concrete parameter vector and optional intention reference.
  - `TzeentchOutcome`: Empirical observation post-action, capturing reward signals, state deltas, and feedback vectors.
  - `TzeentchBeliefUpdate`: Epistemic state modification tracking prior and posterior confidence scores.
- **Canonical Schemas**: Assigned fixed, deterministic schema IDs (`SCHEMA_PERCEPT = 101`, `SCHEMA_INTENTION = 102`, `SCHEMA_ACTION = 103`, `SCHEMA_OUTCOME = 104`, `SCHEMA_BELIEF_UPDATE = 105`, `SCHEMA_SOURCE_GAP = 106`).
- **Migration Modes & Zero-Drop Mirroring (`MirrorWriter`)**:
  - `LegacyOnly`: All writes route to legacy system; Temnion is dormant.
  - `ShadowMirror`: Dual writes where legacy is authoritative and events are mirrored to Temnion through a bounded queue. Queue overflow returns `AdapterError::QueueBackpressure`; silent dropping is forbidden.
  - `TemnionAuthoritative`: Temnion handles primary writes; legacy receives shadow writes.
  - `TemnionOnly`: Full cutover; Temnion is the sole, authoritative source of truth.

### 2. Multi-Cadence Timing & Causal Action Tracer (`crates/temnion-adapter`, M40)

- **`CadenceTier` & `CadenceScheduler`**:
  - `Fast` (120 Hz, `CLOCK_FAST`): Reflexes, sensory acquisition, motor actuation.
  - `Medium` (20 Hz, `CLOCK_MEDIUM`): Sensory filtering, organ-level state integration.
  - `Slow` (1 Hz, `CLOCK_SLOW`): High-level deliberation, goal planning, belief revisions.
  - `Background` (0.1 Hz, `CLOCK_BACKGROUND`): Epistemic consolidation, e-graph optimization, GC.
- **`TzeentchActionTracer`**:
  - Reconstructs transitive causal lineage for any executed action by traversing causal edges in chronological order.
  - Emits typed trace nodes: `Percept`, `CellProcessing`, `RetrievedBelief`, `Prediction`, `Intention`, `Action`, `Outcome`.
  - **Explicit Source Gaps**: Missing instrumented causal steps produce explicit `ActionTraceNode::SourceGap { step_name, expected_time }` nodes rather than silently omitting intervals.
  - **Zero Future-Knowledge Leakage**: Strictly enforces that no causal ancestor event possesses a known-time tick greater than the target action's execution tick (`AdapterError::FutureKnowledgeLeakage`).

### 3. Desktop Studio Tzeentch Explorer (`apps/temnion-studio`, M41)

Integrates deep organism observability into the Tauri desktop application:
- **Backend IPC Commands**:
  - `get_tzeentch_summary`: Queries organism ID, active organs, active cells, migration mode, and mirror queue statistics.
  - `get_tzeentch_cadence_stats`: Returns real-time frequencies (Hz), accumulated ticks, queue pressure, and drop counts across all four cadence tiers.
  - `inspect_tzeentch_action_trace`: Traces the causal action chain for any event sequence, surfacing nodes, edges, justifications, and future-leakage verification.
  - `set_tzeentch_migration_mode`: Dynamically adjusts migration mode with instant telemetry reflection.
- **Modern TanStack React UI (`TzeentchPanel`)**:
  - Multi-cadence live status gauges with color-coded frequency indicators.
  - Migration mode switch panel displaying zero-drop guarantees.
  - Interactive Causal Action Trace Inspector visualizing the complete perception-to-outcome pipeline with gap badges and integrity status.

### 4. Scale Benchmarking Harness (`crates/temnion-bench`, M42)

- Dedicated scale harness (`src/bin/scale.rs`) executing synthetic multi-cadence workloads from 4,000 to 64,000 to 1,000,000+ active records.
- Measures bulk ingestion throughput, point-lookup latency, bounded range scan performance, and depth-32 causal graph lineage traversal.

### 5. Retention Policies, Reference Holds & Backups (`crates/temnion-storage`, M43)

- **`ReferenceHold`**: Pinned range `[start_seq, end_seq]` identified by label and timestamp.
- **`RetentionPolicy`**: Calculates candidate sequence evictions while strictly exempting any sequence covered by an active `ReferenceHold`. Held records can never be pruned by background compaction.
- **`BackupManager`**:
  - Creates consistent point-in-time snapshots of the active store (`events.wal` and segment directory).
  - Computes and embeds per-file CRC32 checksums into a tamper-evident `backup.manifest.json`.
  - Verifies backup integrity and restores cleanly into fresh store directories, refusing restoration if checksums mismatch.

---

## Qualification Gates (M44)

Release R6 requires passing all three qualification gates with documented empirical results:
- **Gate A (Deterministic Foundations):** Zero unsafe code (`#![forbid(unsafe_code)]`), 100% reproducible crash-recovery across process exits, bit-identical reconstruction, and zero future leakage.
- **Gate B (Multi-Cadence & Protocol Integration):** Native local IPC, Arrow Flight streaming, MCP stdio protocol, zero silent drops in dual writing, and sub-millisecond multi-cadence action tracing.
- **Gate C (Measurable Autonomous Quality):** Candidate evaluation inside strict isolation envelopes, holistic net benefit scoring, certified constitutional compliance across all 8 axioms, and zero regressions.

---

## Consequences

### Positive
- **Architectural Purity:** Temnion's storage engine remains lean, fast, and completely decoupled from consumer-specific ML dependencies.
- **Provable Provenance:** Every action taken by a cognitive agent can be audited back to sensory perceptions with verified temporal integrity.
- **Operational Safety:** Dual-write migration guarantees zero lost events, while reference holds ensure critical historical evidence is never pruned.
- **Cross-Platform Inspection:** Developers and operators can introspect live organism traces in both CLI and desktop Studio interfaces.

### Neutral / Trade-offs
- External media must be managed in companion object stores; Temnion verifies SHA-256 hashes but does not store large raw binary objects directly in the log.
- Mirror queue backpressure must be explicitly handled by consumer application loops.
