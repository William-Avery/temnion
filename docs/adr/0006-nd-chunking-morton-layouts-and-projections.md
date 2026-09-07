# ADR 0006: N-dimensional chunking, Morton space-filling curves, and zero-duplication alternate projections

Status: accepted implementation contract for M6 (N-D chunking/layouts) and M7 (Alternate projections).

## Problem and Context

Exact-state and historical querying often involves multi-axis predicates: queries may target specific
entities, temporal intervals, schema types, or spatial/multi-dimensional coordinate ranges.

Traditional databases solve multi-axis retrieval by copying entire record rows into secondary indexes,
which leads to severe write amplification, bloated storage consumption, and memory footprint explosion.
Furthermore, multi-dimensional coordinates (2D space, 3D simulation/voxel extents) suffer from the
"curse of dimensionality" when indexed as independent scalar dimensions.

Following Temnion Architecture §6 & §9:
1. **Space-Filling Curves (M6):** Morton (Z-order curve) encoding maps multi-dimensional coordinates
   into 1D scalar space while preserving spatial locality, allowing spatial bounding boxes to be
   decomposed into discrete 1D intervals.
2. **Zero-Duplication Projections (M7):** Secondary projections store strictly lightweight index keys
   and monotonic sequence numbers (`sequence: u64`), completely eliminating payload duplication.

## Invariants and Guarantees

1. **Zero payload duplication:** Projections store sequence pointers referencing authoritative WAL or
   TSF records. No event payloads, schema records, or delta bytes are ever duplicated.
2. **Deterministic bi-directional Morton mapping:** Morton encoding and decoding roundtrip bit-for-bit
   across all 32-bit (2D) and 21-bit (3D) coordinates.
3. **Multi-clock isolation:** Temporal projections index timestamps alongside their explicit `ClockId`.
   Cross-clock comparisons are never fabricated or linearly ordered.
4. **Short-circuiting composite evaluation:** When querying across multiple projection axes
   (entity, schema, time, space), the engine performs linear sorted merges and short-circuits to an
   empty result the moment any predicate yields an empty candidate set.
5. **CRC32C framing:** Serialized projection indexes (`TNPR`) enforce version 1 framing and CRC32C
   checksumming across their payload.

## Architecture and Data Structures

### Space-Filling Curves & Grid Chunking (`temnion-index`)

- `morton_encode_2d(x: u32, y: u32) -> u64` and `morton_decode_2d(code: u64) -> (u32, u32)`:
  Interleaves bits of 32-bit coordinate pairs using bit-spread dilation.
- `morton_encode_3d(x: u32, y: u32, z: u32) -> u64` and `morton_decode_3d(code: u64) -> (u32, u32, u32)`:
  Interleaves 21-bit 3D coordinates into a 63-bit code within a `u64`.
- `BoundingBox2D` & `BoundingBox3D`: Axis-aligned bounding boxes providing `contains_point` and
  `intersects`.
- `GridChunker2D` & `GridChunker3D`: Divides continuous space into uniform tiles/voxels of size
  `chunk_size`.
- `morton_intervals_chunked`: Decomposes a bounding box in chunk space into a minimal, sorted list of
  contiguous Morton code intervals `[(start, end)]`.

### Alternate Projections (`temnion-index`)

1. **`EntityProjection`**: `BTreeMap<EntityId, Vec<u64>>` providing $O(\log E)$ point entity lookups.
2. **`TemporalProjection`**: `BTreeMap<(ClockId, u64), Vec<u64>>` providing clock-scoped range scans.
3. **`SpatialMortonProjection`**: `BTreeMap<u64, Vec<u64>>` mapping Morton chunk codes to sequence numbers.
   Evaluates spatial box queries via `sequences_in_box`.
4. **`SchemaBitmapProjection`**: `BTreeMap<SchemaId, Vec<u64>>` indexing events by schema type.
5. **`ProjectionIndex`**: Composite index aggregating all four projections, providing:
   - `index_event(sequence, entity, schema, valid_time, spatial_coords, chunker)`
   - `query_intersect(entity, schema, time_range, spatial_intervals) -> Vec<u64>`
   - `encode() -> Vec<u8>`: Binary serialization with `TNPR` magic and CRC32C framing.
   - `decode(bytes) -> Result<ProjectionIndex, IndexError>`: Full integrity validation.
