# ADR 0008: Canonical typed query IR, TemQL, and AI-compact Tem shorthand

Status: accepted implementation contract for M16 (Typed Query IR) and M17 (TemQL & Compact Tem).

## Problem and Context

Temnion supports multiple diverse entry points and consumers: interactive human developers, CLI tools, AI agents, IPC adapters, SQL frontends, and the desktop Studio interface. Without a canonical intermediate representation, query semantics, operator capabilities, and error conditions would splinter across individual language implementations.

Following Temnion Architecture §7, §23, §24, and §25:
1. **Typed Query IR (M16):** Establishes a single unified logical operator tree (`LogicalPlan`) and expression model (`Expr`). No frontend or client protocol owns independent execution semantics.
2. **TemQL (M17):** Provides a human-readable query language for relational, temporal, spatial, and causal queries.
3. **Compact Tem (M17):** Provides a token-efficient shorthand starting with `tn:` designed for LLM prompts and model generation, lowering into the exact same logical IR as TemQL.
4. **Physical Optimization & EXPLAIN (M16):** Translates logical plans into physical execution trees (`PhysicalPlan`), evaluating predicate pushdown, Bloom filters, and zone map skipping before disk access.
5. **Reference Execution & Resource Budgets (M16):** Evaluates physical plans against authoritative durable storage and causal DAGs while strictly enforcing resource ceilings (`QueryBudget`: max rows, max events scanned, max bytes).

## Invariants and Guarantees

1. **Frontend Lowering Equivalence:** All frontends (TemQL, compact `tn:`, CLI, and future protocols) lower into the same strongly-typed `LogicalPlan`. Equivalent human and compact queries yield identical `LogicalPlan` structures.
2. **Deterministic Query Semantics:** Execution ordering across all scans is strictly deterministic, preserving clock domain separation and total sequence order.
3. **Zero False Negatives in Skipping:** Block and segment skipping via zone maps and Bloom filters never prune matching events.
4. **Strict Resource Budget Enforcement:** Query execution respects hard limits on returned rows, scanned events, and read bytes. If exceeded, results are safely truncated with `truncated: true` and metrics reported.
5. **Memory Safety & No Unsafe Code:** `temnion-query` strictly enforces `#![forbid(unsafe_code)]`.

## Architecture and Data Structures

### Canonical Typed Query IR (`temnion-query` - M16)

- `LogicalPlan`:
  - `Scan`: Relational and temporal scans across entities, valid time ranges, known-as-of cutoffs, filter predicates, field projections, and row limits.
  - `CausalTrace`: Causal graph traversal (`Causes` or `Effects`) from an origin event up to a max depth.
  - `SpatialScan`: Morton-indexed bounding box ranges across 2D/3D coordinate spaces.
- `Expr`:
  - `Literal`: Typed values (`Int`, `Float`, `String`, `Bool`).
  - `Field(String)`: Field identifier.
  - `Binary`: Comparisons (`Eq`, `NotEq`, `Lt`, `Lte`, `Gt`, `Gte`) and boolean logic (`And`, `Or`).
  - `Not`: Unary boolean negation.
- `QueryBudget`:
  - `max_rows`: Maximum result records emitted.
  - `max_events_scanned`: Upper bound on scanned records before early termination.
  - `max_bytes`: Maximum read bytes allowed from storage.

### Physical Planning and EXPLAIN (`temnion-query` - M16)

- `PhysicalPlan`:
  - `StorageScan`: Configured with predicate pushdown flags (`use_zone_maps`, `use_bloom`) for storage block skipping.
  - `CausalWalk`: Transitive traversal using CSR-packed causal graphs (`CausalGraph`).
  - `SpatialIndexLookup`: Evaluates decomposed Morton intervals.
- `ExplainPlan`:
  - Produces formatted tree representation with operator names, estimated costs, selected indices, and pushdown filters.

### Query Parsers (`temnion-query` - M17)

- `parse_temql(input: &str) -> Result<LogicalPlan, QueryError>`:
  - Parses human-readable syntax:
    ```text
    FROM temnion
    ENTITY 0:1:0
    TIME valid 10..20
    KNOWN_AS_OF 20
    WHERE health > 50
    SELECT position, health
    LIMIT 32
    ```
  - Parses causal statements: `EVENT 1:1:100 TRACE CAUSES DEPTH 8 LIMIT 16`.
- `parse_compact_tem(input: &str) -> Result<LogicalPlan, QueryError>`:
  - Parses AI token shorthand prefixed with `tn:`:
    - Entity scan: `tn:#0:1:0@v10..20@k20?health>50>position,health!32`
    - Causal trace: `tn:$1:1:100<-8` (causes) or `tn:$1:1:100->8` (effects)
  - Disambiguates `>` comparison operators within expressions from `>` projection markers.
