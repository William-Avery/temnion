# Temnion roadmap

This roadmap covers the **entire** Rust-first standalone exact-history -> EKS ->
transformations -> safe-evolution architecture. It is not a list of features
already shipped. Source milestone identifiers M0–M44 remain traceable to the
[unchanged architecture](architecture/source/README.md); delivery tasks T00–T21
reorder prerequisites to settle identity, time, recovery and integrity early.

## Current delivery

**M0 foundation, M1 packed state, and a volatile M2 event-history subset.**

- Repository/Rust/licensing/documentation/CI foundations and a core contract ADR.
- Typed entity/source/event/clock identities and generic dense generational state.
- Bounded per-source typed in-memory history, atomic batch admission, clock-scoped
  filters, known-as-of selection and bounded snapshot pagination.
- `tem version`, `describe`, scripted `demo`, and embedded examples.
- A small seeded A/B/D baseline runner with simple reference paths.

This does **not** complete R0 or R1: most T01 specifications, most A–L workloads,
schema/delta systems, durable storage, recovery and replay are future work.
It does not complete M14 temporal reconstruction or M18 protocol negotiation.
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
| T03 — Typed state and events | M1 and volatile M2 foundation | Generation-safe packed state; independent source event IDs and clock domains; later typed schemas/units/dimensions, deltas/change masks and complete input semantics. Test overflow, recycling, late/duplicate input and reference reads. |
| T04 — WAL, TSF and recovery | Future | Durable receipts, raw lossless streams, checksummed/versioned formats, bounded sealing and recoverable manifest publication. Inject crashes at every append/sync/seal/publish boundary; reject corruption/truncation without unbounded allocation or loss of acknowledged data. |
| T05 — Replay, branches and causality | Future | Checkpoints, deterministic providers with captured inputs/RNG, temporal reconstruction, causal edges and structurally shared branch manifests. Test exact supported replay, explicit nondeterminism boundaries, branch lifecycle and reader pins. |
| T06 — Canonical query and languages | Future | Typed logical/physical IR, reference executor, predicates/projections/aggregations, subscriptions, prepared queries, budgets/cancellation/cursors, capability discovery and EXPLAIN. TemQL/Tem parsers must be equivalent to native execution and errors. |

Exit: ingest -> durable receipt -> crash/restart -> historical query ->
checkpoint/replay -> branch/causal trace, with explicit precision and limitations.
Cross-shard transactions are not silently inferred from source-local atomicity.

## R2 — Efficient standalone database and first embedded consumer

| Task | Status | Deliverable and acceptance |
| --- | --- | --- |
| T07 — Efficient immutable representations | Future; candidates gated | Useful lossless codecs, grid/Morton candidates, entity/time/spatial projections, event bitmaps, bounds/membership filters, compressed predicates and hierarchical summaries. One logical payload; compare every optimized result to the reference and keep simpler winners. |
| T08 — Virtual-shard runtime and tiers | Future | Bounded queues, owned shard groups, local allocators, consistent cross-shard cuts, bounded fanout/merge and resumable background DAGs. Portable DRAM/page-cache/NVMe first; exercise skew, saturation, storage pressure and interrupted work. |
| T09 — Embedded Tzeentch integration | Future | Separate feature-gated adapter and consumer change: fixtures/import, idempotency, bounded mirrored writes, drain/error reporting, historical-read parity and a rollback switch. Keep existing recorder/memory behavior authoritative until gates pass. |
| T10 — Operations and enforcement | Future | Health/metrics, policy/authorization hooks, exact no-auto-delete retention, reference-aware GC, backup/restore, inspection/repair, schema/format migration and auditable operations. Validate recovery and foreground resource protection. |

**Gate A** follows: establish equivalent correctness/durability and repeatable
target-workload value before EKS. Exit: a useful durable standalone exact-memory
database and embedded consumer path. Static Temnion remains useful even if later
adaptive techniques are rejected.

## R3 — Daemon, interoperability, AI control and Studio v1

| Task | Status | Deliverable and acceptance |
| --- | --- | --- |
| T11 — Daemon, full CLI, TNP, C/Arrow and IPC | Future beyond demo CLI | `temniond`/`tem` use the same engine; capability/version/error contracts, timeouts, cancellation, batch/file ingest, resumable bounded subscriptions and local OS access control. C/Arrow ownership and portable pipes/sockets precede measured shared-memory optimization. |
| T12 — Optional remote/MCP/SQL | Future | Authenticated TLS Arrow Flight, `temnion-mcp` with the official Rust SDK and optional DataFusion. Expose only implemented operations, bounded pages or authorized bulk handles, common authorization and budgets. |
| T13 — Studio v1 and adoption docs | Future | Tauri 2 + Rust + React/TypeScript, TanStack where useful. Connections, schemas, TemQL editor, compact/prepared preview, history/bookmarks, EXPLAIN, progressive table/timeline/2D/3D views and ingestion wizard. Real backend flows plus install/API/interoperability and compact AI guides. |

Exit: users can install, ingest, inspect, query, explain and visualize through
supported interfaces without reading implementation code. Headless builds stay
free of GUI/npm requirements; native Linux ARM64 Studio is required, not a
desktop-remote-only substitute.

## R4 — EKS, isolated model workers and transformations

| Task | Status | Deliverable and acceptance |
| --- | --- | --- |
| T14 — Knowledge, provenance and prediction | Future, after Gate A | Versioned Observation/Claim/Belief/Concept/Rule/Model/Skill, evidence references, dependencies/contradictions, confidence, valid/learned time, truth maintenance, predictions/outcomes/errors and WHY. Model manifests plus bounded isolated workers, deterministic fixture worker and at least one real adapter. |
| T15 — Consolidation and transformations | Future; e-graphs candidate | Active/reference/archive knowledge, episode-to-pattern/concept/rule transformations, evidence-linked summaries and bounded working memory. Versioned types/preconditions/costs; handwritten Rust first. Adopt e-graphs only for a demonstrated equivalence task. |

**Gate B** follows: held-out, time-correct evidence of consumer benefit with complete
provenance and accounted active-memory/runtime cost. Exit: Tzeentch and a second
model-agnostic example retrieve justified knowledge available at historical time.

## R5 — Isolated, evidence-driven evolution

| Task | Status | Deliverable and acceptance |
| --- | --- | --- |
| T16 — Champion/challenger control | Future, after Gate B | Versioned candidates/lineage, platform-appropriate isolation, bounded reproducible evaluation, comparison, manual approval, atomic promotion and rollback. Candidates cannot change evidence or the Constitution. |
| T17 — Adaptive physical memory/lifecycle | Future candidates | Shadow-test codec/layout/index/filter and checkpoint/segment/cache/prefetch/tier/shard policies against static incumbents. Count background costs, foreground tails, amplification and rollback. Preserve content and compatible readers. |
| T18 — Associative and knowledge/transformation mutation | Future candidates | Optional semantic/vector projections remain distinct from exact queries. Evaluate rules, retrieval, models/skills and transformations through the same isolation/provenance/promotion path. |
| T19 — Meta-evolution | Future, only after Gate C | Evaluate mutation policies as artifacts behind the unchanged Constitution. Require value over a fixed policy; preserve an off switch. Publish negative results and keep losing candidates disabled. |

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
| M2 | Event history/deltas | T03 | Volatile typed append/query subset |
| M3 | Deterministic reconstruction | T05 | Future |
| M4 | Immutable segment format | T04 | Future |
| M5 | Compression framework | T07 | Future |
| M6 | N-dimensional chunking/layouts | T07 | Future candidates |
| M7 | Alternate projections | T07 | Future |
| M8 | Virtual-shard execution | T08 | Future |
| M9 | Background task DAG | T08 | Future |
| M10 | Storage hierarchy | T08 | Future |
| M11 | Filters/compressed execution | T07 | Future |
| M12 | Hierarchical summaries | T07 | Future |
| M13 | Branching timelines | T05 | Future |
| M14 | Multi-time semantics | T03/T06 | Typed clocks/filter subset; reconstruction future |
| M15 | Causal history | T05/T20 | Future |
| M16 | Typed Query IR | T01/T06 | Future; foundation contract decision only |
| M17 | TemQL and compact Tem | T01/T06 | Future |
| M18 | Protocol/capabilities | T01/T06/T11 | CLI capability report only; protocol future |
| M19 | Native/C/Arrow/local IPC | T09/T11 | Foundational Rust access only; full interfaces future |
| M20 | Arrow Flight | T12 | Future |
| M21 | MCP server | T12 | Future |
| M22 | SQL compatibility | T12 | Future |
| M23 | Studio v1 | T13 | Future |
| M24 | Documentation v1 | T00–T21, especially T13 | Foundation docs; grows with implemented features |
| M25 | Knowledge primitives | T14 | Future after Gate A |
| M26 | Provenance/truth maintenance | T14 | Future |
| M27 | Predictive knowledge | T14 | Future |
| M28 | Knowledge consolidation | T15 | Future |
| M29 | Transformation engine | T15 | Future |
| M30 | E-graphs/rewrite experiments | T15 | Future benchmark-gated candidate |
| M31 | Champion/challenger evolution | T16 | Future after Gate B |
| M32 | Adaptive physical memory | T17 | Future candidates |
| M33 | Adaptive lifecycle policies | T17 | Future candidates |
| M34 | Semantic/vector projection | T18 | Future optional candidate |
| M35 | Knowledge mutation | T18 | Future candidates |
| M36 | Transformation mutation | T18 | Future candidates |
| M37 | Meta-evolution | T19 | Future, only after Gate C |
| M38 | Immutable Constitution hardening | T01/T04/T10/T16; continuous | Boundary accepted; feature enforcement/hardening staged |
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
| Consumer and Studio | Architecture and milestone requirements | Adapter mappings/parity/replay limits; native desktop setup and real end-to-end flows, T09/T13/T20 |
| Measurement | [BENCHMARKING](BENCHMARKING.md) | Full A–L fixtures, predeclared Gates A/B/C and native platform reports |

Every feature requires implementation, targeted tests, documentation and a
runnable example; performance-sensitive work needs relevant evidence. Optional
research may be rejected after a real evaluation. Empty crates, unsupported
capabilities, mocked UI success, speculative numbers and cross-check-only
“native support” do not meet completion criteria.
