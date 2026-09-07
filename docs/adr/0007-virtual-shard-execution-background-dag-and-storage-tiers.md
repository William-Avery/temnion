# ADR 0007: Virtual-shard execution, background task DAGs, and storage hierarchy

Status: accepted implementation contract for M8 (Virtual-shard execution), M9 (Background task DAG), and M10 (Storage hierarchy).

## Problem and Context

As event volume and query throughput scale up, monolithic single-thread or globally locked storage engines
experience severe lock contention, unbounded memory growth, and foreground query latency degradation
caused by competing background maintenance operations.

Following Temnion Architecture §6, §10, and §11:
1. **Virtual-Shard Execution (M8):** Decouples logical shard identities (`ShardId`) from physical
   execution partitions (`VirtualShardId`), providing single-writer partition ownership and core-scaled
   concurrency without global hot locks.
2. **Background Task DAG (M9):** Structures all deferred maintenance (segment sealing, lossless
   compression, index construction, summary generation) as a directed acyclic graph with explicit
   prerequisites, prioritized scheduling, and adaptive pressure-based yielding.
3. **Storage Hierarchy & Tiers (M10):** Organizes memory and media into distinct tiers (`HotDram`,
   `WarmMapped`, `ColdMedia`) with capacity budgets, LRU eviction, and access-frequency promotion.

## Invariants and Guarantees

1. **No global mutable hot database lock:** Writers append to their assigned virtual shard partition
   with local sequence progression.
2. **Deterministic multi-shard merge order:** Fanout queries executed across multiple virtual shards
   merge results into a strictly deterministic total order `(valid.clock, valid.ticks, source, epoch, sequence)`.
3. **DAG integrity and cycle prevention:** Background task dependencies are verified acyclic prior to
   scheduling; cyclic task definitions are detected and rejected immediately.
4. **Foreground latency protection:** Background tasks must monitor foreground queue depth and write
   pressure. Low-priority maintenance yields under moderate pressure; only critical segment sealing
   runs under high pressure; all background tasks pause under critical pressure.
5. **Bounded memory footprints:** Hot and warm tiers enforce explicit capacity byte limits, evicting
   least recently used segments downwards to prevent memory exhaustion.

## Architecture and Data Structures

### Virtual-Shard Execution (`temnion-runtime` - M8)

- `VirtualShardId(pub u32)`: Physical partition handle.
- `ShardRouter`: Distributes logical `ShardId` across partitions using either `Modular(u32)` or
  `Explicit(HashMap<ShardId, VirtualShardId>)` policies.
- `VirtualShardPartition`: Single-writer execution context maintaining shard-local event sequences.
- `VirtualShardCoordinator`: Coordinates write routing, partition storage, and deterministic fanout
  queries.

### Background Task DAG (`temnion-runtime` - M9)

- `TaskClass`: Priority hierarchy:
  - `Seal` (priority weight 100): Sealing full WAL batches into immutable segments.
  - `Compress` (priority weight 80): Lossless compression candidate evaluation.
  - `Index` (priority weight 60): Building alternate projections (`ProjectionIndex`).
  - `Summary` (priority weight 40): Generating hierarchical summaries (`SegmentSummary`).
  - `Maintenance` (priority weight 20): Periodic checkpoints, GC, and file maintenance.
- `TaskDag`: Manages tasks, tracks dependencies, and resolves `ready_tasks()` ordered by task priority.
- `PressureController`: Categorizes foreground load into `Normal`, `Moderate`, `High`, and `Critical`
  pressure levels, controlling `should_throttle()` and `should_yield(class)`.

### Storage Hierarchy & Tiers (`temnion-runtime` - M10)

- `StorageTier`:
  - `HotDram`: Active mutable write buffers and live state slabs.
  - `WarmMapped`: Frequently accessed read-only segments and secondary indexes.
  - `ColdMedia`: Persistent archival disk segments.
- `TieredSegment`: Tracks segment byte size, current tier, access frequency, and LRU step counters.
- `TieredStorageManager`: Enforces memory budgets (`hot_capacity_bytes`, `warm_capacity_bytes`) using
  LRU demotion, and promotes cold segments to warm on repeated access.
