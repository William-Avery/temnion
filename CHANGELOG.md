# Changelog

## Unreleased - Durable source-log increment

- Added strict checksummed WAL/batch and raw TSF v1 binary codecs.
- Added immutable scalar schemas, bit-exact values and canonical sparse mutation
  encoding with atomic in-memory apply.
- Added OS-locked source logs, synchronized durable receipts, restart-stable
  identity/sequence, explicit tail recovery and bounded disk-backed history.
- Added non-overwriting TSF exports that retain the authoritative WAL.
- Extended `tem` with persistent initialization, append, inspection, history,
  explicit recovery, sealing and standalone segment validation.
- Added a separately selected synchronized batch/reference workload.

Manifest-based WAL retirement, compression, reconstruction/checkpoints, branches
and the remaining application interfaces are still unfinished. This increment
does not complete the database or any production/performance gate.

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
