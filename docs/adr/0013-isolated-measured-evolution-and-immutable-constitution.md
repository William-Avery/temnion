# ADR 0013: Isolated Measured Evolution and Immutable Constitution

Status: accepted implementation contract for M31–M38 (Isolated Measured Evolution).

## Problem and Context

Static database engineering settles on fixed codecs, layouts, index structures, checkpoint cadences, and inference rules. However, diverse real-world event workloads exhibit varying sparsity, write skew, and query patterns. Allowing runtime self-optimization and evolution introduces profound systemic risks:
1. **Risk of Evidence or Semantic Mutation:** An autonomous candidate or rogue model might alter authoritative historical records, delete past events, or redefine ground truth to report artificial improvements.
2. **Risk of Self-Modification of Evaluation Rules:** An evolutionary agent might alter promotion criteria, disable safety tests, or rewrite the Constitution itself to promote non-performing or broken candidates.
3. **Risk of Unmeasured Complexity and Background Degradation:** An adaptation might reduce query latency by 5% while quadrupling background CPU/RAM consumption, starving foreground transactions, or causing severe storage amplification.
4. **Risk of Irreversible Regressions:** A promoted candidate might encounter an unseen workload distribution in production and cause catastrophic latency spikes or lockouts without an immediate, safe rollback path.
5. **Associative Drift:** Semantic and vector embeddings might be treated as ground truth, corrupting deterministic replay and causal provenance.

To address these risks, Temnion Architecture §10, §41, and §52 (Gate C) mandate an **Immutable Constitution** beneath all evolvable behavior, paired with an **Isolated, Measured Champion/Challenger Evolution Engine** (`temnion-evolution`).

---

## The 8 Immutable Constitutional Axioms (M38)

The Constitution cannot evolve at runtime. It governs all candidate generation, evaluation, promotion, and execution:

1. **Evidence Immutability (`ConstitutionalAxiom::EvidenceImmutability`):**
   - Authoritative storage events and historical WAL/TSF files cannot be rewritten, truncated, or removed by evolvable mechanisms.
2. **Provenance Preservation (`ConstitutionalAxiom::ProvenancePreservation`):**
   - All derived knowledge, candidate artifacts, and mutation policies must maintain explicit, verifiable provenance lineage back to parent candidates, applied operators, and justification rationale.
3. **Non-Self-Modification (`ConstitutionalAxiom::NonSelfModification`):**
   - Evolvable candidates, models, or meta-policies cannot mutate the Constitution, evaluation gates, or promotion rules.
4. **Mandatory Rollback (`ConstitutionalAxiom::MandatoryRollback`):**
   - Any promoted candidate exhibiting measured regression must be reversibly rolled back to the prior known-good incumbent without data loss or downtime.
5. **Resource Boundedness (`ConstitutionalAxiom::ResourceBoundedness`):**
   - All candidate evaluations, shadow testing, and background evolution tasks must run inside bounded, metered CPU step and memory envelopes (`IsolationBudget`).
6. **Gate Prerequisite (`ConstitutionalAxiom::GatePrerequisite`):**
   - Evolution cannot proceed unless predeclared gate prerequisites (Gate A, B, C) are satisfied with recorded empirical evidence. Evolution is disabled by default.
7. **Exact Memory Primacy (`ConstitutionalAxiom::ExactMemoryPrimacy`):**
   - Associative and semantic vector projections are strictly secondary indexes and cannot mask, overwrite, or replace exact history or deterministic query execution.
8. **Reader Compatibility (`ConstitutionalAxiom::ReaderCompatibility`):**
   - Representation and physical layout adaptations must maintain bit-level, deterministic reader compatibility.

---

## Architecture and Data Structures

### 1. Champion/Challenger Evolution Engine (`crates/temnion-evolution`, M31)

- **`CandidateKind`**: `Codec`, `Layout`, `Index`, `CheckpointPolicy`, `PrefetchPolicy`, `ShardPlacement`, `KnowledgeRule`, `RetrievalPolicy`, `Transformation`, `ModelSkill`, `MutationPolicy`.
- **`CandidateStatus`**: `Proposed` -> `Sandboxed` -> `Evaluating` -> `Promoted` | `Rejected` | `RolledBack`.
- **`CandidateLineage`**: Immutable record capturing parent candidate ID, target domain, generation timestamp, applied mutation operator, and rationale.
- **`IsolationBudget`**: Limits maximum CPU steps (e.g. 1M steps), memory allocations (e.g. 64MB), and wall-clock execution timeouts.
- **`FitnessMetrics` & Holistic Net Benefit**:
  $$\text{Net Benefit} = 0.35 \cdot \Delta_{\text{latency}} + 0.25 \cdot \Delta_{\text{RAM}} + 0.25 \cdot \Delta_{\text{storage}} + 0.15 \cdot \Delta_{\text{amp}} - 0.10 \cdot \text{Cost}_{\text{bg}}$$
  Evaluates net improvement over static baseline while penalizing background evaluation overhead and write/read amplification.
- **Manual Promotion First**: Autonomous promotion is strictly disabled by default; promotion requires explicit human manual review and verification.
- **Instantaneous Rollback**: Prior incumbents are retained in an immutable historical stack. Rollback restores the prior incumbent immediately and marks the regressing candidate as `RolledBack { reason }`.

### 2. Adaptive Physical Memory (`AdaptivePhysicalMemory`, M32)

- Shadow-tests alternative representation candidates (e.g. adaptive SFC layout, BitPack/Delta codecs, Bloom/ZoneMap index structures) against static incumbents on sealed segments.
- Calculates storage fitness balancing compression ratio, scan latency, decompression CPU cycles, and read amplification without altering canonical event bytes.

### 3. Adaptive Lifecycle Policies (`AdaptiveLifecycleEvaluator`, M33)

- Dynamically tunes checkpoint intervals to balance recovery replay latency against snapshot serialization overhead.
- Evaluates segment seal thresholds and cache tier placement boundaries.

### 4. Semantic and Vector Projections (`VectorEmbedding`, `SemanticProjectionIndex`, M34)

- High-dimensional dense vector representation ($N$-dimensional float embeddings) supporting cosine similarity, dot product, and euclidean distance.
- `SemanticProjectionIndex` provides approximate nearest-neighbor retrieval over selected episodes and concepts.
- **Exact Memory Primacy**: Vector search results are explicitly tagged with similarity scores; exact queries continue to execute deterministically against canonical storage without approximate degradation.

### 5. Knowledge & Transformation Mutation (M35, M36)

- `KnowledgeMutationEvaluator`: Generates and evaluates candidate inferential rules and beliefs against support evidence, penalizing contradiction occurrences and Brier score regressions.
- `TransformationCandidateEvaluator`: Evaluates candidate transformations for throughput and memory efficiency while enforcing strict output equivalence and constitutional limits.

### 6. Meta-Evolution (`MetaEvolutionManager`, M37)

- Evolves mutation operator probability distributions (`MutationPolicy`) based on historical promotion yield.
- **Strict Gate C Enclosure**: Blocked unless Gate C demonstrates repeatable net adaptive value.
- **Hard Off-Switch**: Includes an unconditional runtime kill switch (`meta_evolution_enabled: false`).

### 7. Constitution Audit Framework (`ConstitutionAudit`, M38)

- Evaluates system state against all 8 axioms, verifying that all candidates maintain provenance, all incumbents are validly promoted, and no unauthorized modifications exist.

---

## Tooling and Interfaces

- **MCP Integration (`temnion-mcp`)**: Exposes `evolution_status`, `candidate_evaluate`, and `constitution_audit` tools.
- **CLI Integration (`temnion-cli`)**:
  - `tem evolve status`: Reports active incumbents and candidate lineage.
  - `tem evolve audit`: Executes full constitutional audit report.
  - `tem evolve demo`: Demonstrates shadow evaluation, net-benefit calculation, manual promotion, and active incumbent replacement.
  - `tem describe`: Advertises `"evolution": true`, `"constitution": true`, and `"semantic_projections": true`.

---

## Verification and Safety

- 100% safe Rust: `#![forbid(unsafe_code)]` enforced across all crates.
- Complete integration test suite verifying candidate sandboxing, manual promotion, instantaneous rollback, constitutional violation rejection, and meta-evolution gating.
- Zero compiler warnings under `clippy -- -D warnings`.
