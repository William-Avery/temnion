# Changelog

## Unreleased - N-D chunking/layouts and alternate projections (M6, M7)

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
