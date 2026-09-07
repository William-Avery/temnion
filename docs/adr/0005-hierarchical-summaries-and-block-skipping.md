# ADR 0005: Hierarchical summaries, zone maps, Bloom filters, and predicate pushdown

Status: accepted implementation contract for M11 (Filters & Compressed Execution) and M12 (Hierarchical Summaries & Block Skipping).

## Problem and Context

Historical querying against append-only source logs and segment files requires scanning sequential
event batches. Without summary metadata, evaluating a query with an entity or temporal filter requires
seeking to every batch frame, reading bytes from disk, verifying frame headers, and deserializing every
record payload before predicate evaluation.

To achieve high-throughput queries over massive event streams without redundant I/O or decoding overhead,
Temnion implements hierarchical summaries (`temnion-index`) enabling conservative, zero-false-negative
block skipping and predicate pushdown.

## Invariants and Guarantees

1. **Zero false negatives:** A block or segment is skipped during query execution if and only if it is
   mathematically impossible for any record contained within that block to satisfy the query predicate.
2. **Clock domain separation:** In accordance with ADR 0001, timestamps across distinct clock domains
   (`ClockId`) cannot be compared linearly. Zone maps track intervals within explicit clock domains;
   cross-clock comparisons conservatively return `SkipDecision::Evaluate` rather than falsely skipping.
3. **Deterministic verification:** Summary metadata files (`.tsm`) are framed with `TNSM` magic,
   versioning, and CRC32C integrity checksums. Any byte corruption or truncation is detected immediately.
4. **Zero runtime mutation of segments:** Companion summary files are generated during segment sealing
   (`Store::seal`), stored alongside `.tsf` files as `{:020}-{:020}.tsm`, and remain immutable.

## Architecture and Data Structures

### Zone Maps

- `ZoneMap<T: Copy + Ord>`: Tracks inclusive `[min, max]` scalar intervals (e.g. sequence numbers and
  packed entity IDs). Provides `contains(val)` and `overlaps(min, max)` predicates.
- `TimestampZoneMap`: Tracks intervals within a specific clock domain:
  - `clock: ClockId`
  - `min_ticks: u64`
  - `max_ticks: u64`
  - If a query predicate targets a differing `ClockId`, the zone map cannot rule out matching records
    and conservatively yields `SkipDecision::Evaluate`.

### Entity Bloom Filter

- `EntityBloomFilter`: A 512-bit (64-byte) bitset utilizing 4 deterministic FNV-1a hash seeds to index
  entity keys `(shard << 32) | slot`.
- Provides an $O(1)$ fast rejection test: if `contains_entity(e)` returns `false`, the entity is
  provably absent from the block with zero false negatives.

### Hierarchical Summaries

- `BlockSummary`: Maintained per batch frame in the WAL or segment. Records:
  - `batch_index`: Index of the frame within the log or segment.
  - `record_count` and `byte_length`: Size metrics.
  - `sequence_range`: `ZoneMap<u64>`.
  - `valid_time_range`: `TimestampZoneMap`.
  - `known_time_range`: `TimestampZoneMap`.
  - `entity_range`: `ZoneMap<EntityId>`.
  - `entity_bloom`: `EntityBloomFilter`.
- `SegmentSummary`: Segment-level aggregation encapsulating:
  - Aggregate record count and sequence/time/entity bounds across all blocks.
  - Ordered list of all constituent `BlockSummary` records.

### Binary Format (`.tsm`)

Companion segment summary files use a versioned binary format:
- Magic: `b"TNSM"` (4 bytes)
- Version: `u16` (currently 1)
- Flags: `u16`
- CRC32C: `u32` (checksum computed over the entire serialized payload following the header)
- Payload:
  - Source ID (`u64`) and Epoch (`u64`)
  - Total records (`u64`)
  - Global sequence range (`min: u64`, `max: u64`)
  - Valid time range (`clock: u64`, `min_ticks: u64`, `max_ticks: u64`)
  - Known time range (`clock: u64`, `min_ticks: u64`, `max_ticks: u64`)
  - Entity ID range (`shard: u32`, `slot: u32`, `gen: u32` for min and max)
  - Block count (`u32`)
  - Array of serialized block summaries (with 64-byte Bloom filter bitsets).

## Query Pushdown in Storage

In `temnion-storage::Store`:
1. In-memory `summaries: Vec<BlockSummary>` are maintained alongside `frames: Vec<FrameOffset>`.
2. When executing `Store::history(filter, budget)`:
   - For each frame in `frames`, the corresponding `BlockSummary` is checked against `HistoryFilter`:
     - Entity filter: Bloom filter check `entity_bloom.contains_entity(entity)` and `entity_range.contains(entity)`.
     - Valid time filter: `valid_time_range.overlaps_range(min, max)`.
     - Known as of filter: `known_time_range.min_ticks <= as_of_ticks`.
   - If `summary.matches_filter(filter) == SkipDecision::Skip`, the entire batch frame is skipped
     without reading from disk or running decompression/deserialization.
3. Upon segment seal (`Store::seal`), both `{:020}-{:020}.tsf` and `{:020}-{:020}.tsm` are atomically
   persisted.
