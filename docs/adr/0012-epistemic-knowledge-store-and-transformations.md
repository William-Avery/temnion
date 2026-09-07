# ADR 0012: Epistemic Knowledge Store (EKS), truth maintenance, and deterministic transformations

Status: accepted implementation contract for M25–M30 (Epistemic Knowledge Store and Transformations).

## Problem and Context

Exact-history storage records authoritative ground truth events, but cognitive reasoning, agent perception, predictive modeling, and query planning require higher-level abstractions:
1. **Separation of Evidence and Beliefs:** As mandated by Temnion Architecture §9, derived hypotheses, perceptions, and predictions must live in an Epistemic layer (`temnion-eks`) distinct from authoritative immutable storage (`temnion-storage`). Derived beliefs are fallible, revisable, and non-destructive; ground truth events in the WAL/TSF are never overwritten.
2. **Non-Destructive Truth Maintenance:** Agent perceptions can contradict each other or be superseded by newer observations. When an underlying premise or assumption is revoked, the system must cascade retractions non-destructively through the justification dependency DAG without mutating history.
3. **Auditability and the `WHY` Query:** Every derived belief must trace its lineage back to explicit supporting WAL `EventId`s, applied inference rules, intermediate premises, and defeasible assumptions.
4. **Predictive Calibration and Zero Future Leakage:** When evaluating predictive calibration (e.g. Brier scores) at known-as-of time $T_k$, predictions, outcomes, or observations recorded after $T_k$ must be strictly filtered out.
5. **Deterministic Transformations with Resource Metering:** Derived transformations (feature extraction, episode pattern consolidation, belief derivation) must declare typed signatures, verify preconditions, strictly meter CPU steps and memory allocations, and log immutable lineage records.
6. **Canonical Query Optimization via E-Graphs:** To optimize complex canonical query IR expressions and plans without hardcoded heuristic passes, equality saturation over e-graphs allows exploring equivalent plan spaces and extracting optimal minimal ASTs with guaranteed semantic equivalence.

## Invariants and Guarantees

1. **Ground Truth Immutability:** Derived knowledge objects never mutate or delete underlying WAL/TSF events.
2. **Defeasibility and Non-Destructive Retraction:** Retracting a belief transitions its status to `Retracted` and cascades status changes to dependent beliefs, while preserving all historical records for forensic introspection.
3. **Strict Zero Future Leakage:** Queries with a known-as-of cutoff $T_k$ exclude any predictions, outcomes, or beliefs learned after $T_k$.
4. **Guaranteed Bounded Resource Consumption:** Transformations enforce step and memory budgets, emitting failure receipts if ceilings are exceeded.
5. **Cycle-Safe E-Graph Extraction:** E-graph cost extraction employs iterative relaxation over equivalence classes, immune to cycles introduced by symmetric rewrite rules (e.g. $X \iff \neg\neg X$).
6. **Strict Safe Code Mandate:** Pure safe Rust enforcing `#![forbid(unsafe_code)]` with zero compiler warnings under `-D warnings`.

## Architecture and Data Structures

### 1. Knowledge Primitives (`crates/temnion-eks`)

- `KnowledgeId(pub u64)`: Global unique identifier for epistemic items.
- `Confidence(f64)`: Bounded score in $[0.0, 1.0]$ with `CERTAIN`, `UNCERTAIN`, and `IMPOSSIBLE` constants.
- `KnowledgeStatus`: `Active`, `Contradicted`, `Retracted`, `Superseded`.
- `Observation`: Raw empirical perception referencing authoritative storage `EventId`.
- `Claim`: Asserted proposition from an external source or agent with validity intervals.
- `Belief`: Inferred proposition with confidence and explicit `Justification`.
- `Concept`: Consolidated abstract entity or category.
- `Rule`: Inferential antecedent-to-consequent production rule.
- `ModelManifest`: Versioned model metadata specifying runtime, memory budget, and statefulness.
- `Skill`: Reusable procedural capability or recipe.

### 2. Truth Maintenance and Provenance (`TruthMaintenanceSystem`)

- Tracks observations, claims, beliefs, rules, concepts, models, skills, and detected contradictions.
- `infer_belief`: Validates consistency, detects contradictions with existing active propositions, and inserts new beliefs with justification DAG links.
- `retract_belief`: Cascades non-destructive retraction recursively to all beliefs relying on the retracted item as a premise.
- `why(target_id) -> Result<WhyTrace, String>`: Traverses justification DAG back to direct storage `EventId`s, applied rules, intermediate premise traces, assumptions, and contradiction records.
- `active_beliefs_known_as_of(cutoff)`: Retrieves active beliefs strictly learned at or before $T_k$.

### 3. Predictive Knowledge (`PredictionLedger`)

- `Prediction`: Records predicted entity, valid horizon, predicted value, model reference, confidence, and emission time.
- `record_outcome`: Matches or refutes pending predictions with empirical observations.
- `compute_brier_score(cutoff) -> Result<f64, String>`: Calculates mean squared prediction error over resolved predictions strictly known at or before `cutoff`.

### 4. Tiered Consolidation (`TieredKnowledgeStore`)

- Partitions knowledge into `Active` (working memory), `Reference` (consolidated patterns/concepts), and `Archive` (historical superseded/retracted items).
- Enforces active item capacity limits using access-frequency eviction to `Archive`.
- `consolidate_episode(episode, signature)`: Promotes episode observations to `Reference` tier and mines generalized `Pattern` records.

### 5. Transformation Engine (`crates/temnion-transform`)

- `TransformationManifest`: Versioned metadata with `TransformationKind`, description, and `TransformationBudget`.
- `ResourceMeter`: Tracks CPU step consumption and allocated memory against budget ceilings.
- `TransformationEngine`: Executes deterministic feature extraction and episode consolidation, logging immutable `LineageRecord`s with execution receipts.

### 6. E-Graphs and Canonical IR Rewrites (`EGraph`)

- Canonical equality saturation over `Expr` and `LogicalPlan` nodes using union-find with path compression.
- Algebraic rewrite rules:
  - Boolean identities: $X \land \text{true} \to X$, $X \land \text{false} \to \text{false}$, $X \lor \text{true} \to \text{true}$, $X \lor \text{false} \to X$, $\neg\neg X \to X$, $X \land X \to X$, $X \lor X \to X$, $X = X \to \text{true}$.
  - Constant folding for comparisons and logical operations.
  - Plan rewrites: `Scan(entity, filter = true) => Scan(entity, filter = None)`, `Scan(entity, filter = false) => EmptyRelation`.
- `extract_best_expr`: Cost-based plan extraction using iterative Bellman-Ford relaxation to extract minimal ASTs.

### 7. Interface Integrations

- **MCP (`temnion-mcp`):** Exposes `why_trace` and `rewrite_expr` tools.
- **CLI (`temnion-cli`):** Provides `tem why-demo` and `tem rewrite-demo <expr>` commands; advertises `"eks": true`, `"transformations": true`, and `"e-graphs": true`.
- **Query Parser (`temnion-query`):** Exposes `pub fn parse_expr` for general recursive-descent scalar expression parsing with operator precedence.
