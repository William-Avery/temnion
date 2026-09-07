# ADR 0004: Timeline branching, structural sharing, and causal DAG tracing

Status: accepted implementation contract for M13, M14, and M15. Later format changes
require new version identifiers and compatibility fixtures, not reinterpretation.

## Timeline branching and structural sharing model

Following Temnion Architecture §19, historical events form a persistent DAG of branched
timelines:
```text
A -> B -> C (root: main)
          +-> D1 -> E1 (branch 1: candidate fork at seq C)
          +-> D2 -> E2 (branch 2: candidate fork at seq C)
```

Invariants:
1. Zero payload duplication on fork: creating a branch is an $O(1)$ metadata operation.
   Child branches share all immutable ancestor segments (TSF) and WAL records up to the
   fork sequence.
2. Independent child evolution: writes appended to a branch are isolated in the branch's
   local store directory without mutating or polluting ancestor branches.
3. Transparent timeline resolution: queries or reconstructions executed against a child
   branch resolve intervals backwards along the ancestor chain:
   - $[0, \text{fork\_seq}]$ resolved through parent branches.
   - $(\text{fork\_seq}, \dots]$ resolved through the child branch's local log.
4. Branch lifecycle tracking:
   - `Active`: standard writable branch.
   - `Temporary`: ephemeral scratch or speculative branch.
   - `Candidate`: proposed branch under evaluation or review.
   - `Promoted`: accepted branch replacing or merging into canonical lineage.
   - `Retired`: archived read-only branch, no further appends allowed.

## Persistent branch manifest specification

Branches are tracked in an authoritative manifest (`branches.manifest`) located in the
database root directory:
- Binary format: `TNBM` magic (4 bytes), version 1 (2 bytes), CRC32C checksum (4 bytes),
  database ID (16 bytes), branch count (4 bytes), followed by packed branch metadata records.
- Each branch record encodes:
  - Branch ID (`u64`, 8 bytes)
  - UTF-8 name length (`u16`, 2 bytes) and UTF-8 name bytes
  - Parent flag (`u8`, 1 byte): 0 if root branch, 1 if child fork
  - Optional parent branch ID (`u64`, 8 bytes) and fork sequence (`u64`, 8 bytes)
  - Lifecycle state tag (`u8`, 1 byte)
  - Creation timestamp (`u64` milliseconds, 8 bytes)
- Atomic publication: branch manifest updates write to a temporary file (`.tmp-<id>.tbm`),
  flush and sync to OS media, and are committed atomically via filesystem rename.

## Causal graph representation and bidirectional DAG tracing

Following Temnion Architecture §15, causality is first-class and tracked explicitly on
each event record through its `causes: Vec<EventId>` field.

Graph representations:
1. `CausalGraph`: dynamic adjacency representation mapping each event ID to its immediate
   upstream causes (`parents`) and downstream effects (`children`).
2. `CsrCausalGraph`: static Compressed Sparse Row representation packing causal relationships
   into flat, contiguous vectors (`event_offsets[]`, `cause_ids[]`):
   - Cache-friendly, optimal memory layout for large event graphs.
   - Binary serialization format (`TNCG` magic, version 1, CRC32C framing).

Bidirectional querying capabilities:
- Immediate queries: `immediate_causes(event)` and `immediate_effects(event)`.
- Transitive causal cones:
  - `trace_causes(event, max_depth)` traverses upstream causes using bounded BFS,
    computing the causal ancestry cone and shortest causal path depths.
  - `trace_effects(event, max_depth)` traverses downstream effects using bounded BFS,
    computing the blast radius / effect cone and shortest impact depths.
- Topological analysis:
  - `topological_sort()` produces a causal execution order respecting all dependencies.
  - Cycle detection: guarantees that causal graphs are directed acyclic graphs (DAGs);
    cyclic inputs are detected and rejected.
  - Ancestry testing: `is_ancestor_of(a, b)` determines whether event $a$ causally
    preceded event $b$.
