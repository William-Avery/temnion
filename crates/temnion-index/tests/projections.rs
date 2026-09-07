// SPDX-License-Identifier: AGPL-3.0-only
use temnion_core::{ClockId, EntityId, SchemaId, ShardId, Timestamp};
use temnion_index::{
    BoundingBox2D, BoundingBox3D, EntityProjection, GridChunker2D, GridChunker3D, IndexError,
    ProjectionIndex, SchemaBitmapProjection, SpatialMortonProjection, TemporalProjection,
    morton_decode_2d, morton_decode_3d, morton_encode_2d, morton_encode_3d,
};

#[test]
fn morton_2d_roundtrip_and_extremes() {
    assert_eq!(morton_encode_2d(0, 0), 0);
    assert_eq!(morton_decode_2d(0), (0, 0));

    assert_eq!(morton_encode_2d(1, 0), 1);
    assert_eq!(morton_decode_2d(1), (1, 0));

    assert_eq!(morton_encode_2d(0, 1), 2);
    assert_eq!(morton_decode_2d(2), (0, 1));

    assert_eq!(morton_encode_2d(1, 1), 3);
    assert_eq!(morton_decode_2d(3), (1, 1));

    // Maximum coordinates
    let max_code = morton_encode_2d(u32::MAX, u32::MAX);
    assert_eq!(max_code, u64::MAX);
    assert_eq!(morton_decode_2d(max_code), (u32::MAX, u32::MAX));

    // Diverse coordinates
    for x in [
        0,
        1,
        2,
        3,
        5,
        7,
        16,
        255,
        1024,
        65535,
        1_000_000,
        u32::MAX - 1,
    ] {
        for y in [0, 1, 2, 4, 8, 42, 256, 4096, 65536, 2_000_000, u32::MAX] {
            let code = morton_encode_2d(x, y);
            assert_eq!(morton_decode_2d(code), (x, y), "failed for ({x}, {y})");
        }
    }
}

#[test]
fn morton_3d_roundtrip_and_extremes() {
    assert_eq!(morton_encode_3d(0, 0, 0), 0);
    assert_eq!(morton_decode_3d(0), (0, 0, 0));

    assert_eq!(morton_encode_3d(1, 0, 0), 1);
    assert_eq!(morton_decode_3d(1), (1, 0, 0));

    assert_eq!(morton_encode_3d(0, 1, 0), 2);
    assert_eq!(morton_decode_3d(2), (0, 1, 0));

    assert_eq!(morton_encode_3d(0, 0, 1), 4);
    assert_eq!(morton_decode_3d(4), (0, 0, 1));

    // Maximum 21-bit coordinate (0x1f_ffff = 2,097,151)
    let max_21 = 0x001f_ffff;
    let max_code = morton_encode_3d(max_21, max_21, max_21);
    assert_eq!(morton_decode_3d(max_code), (max_21, max_21, max_21));

    for x in [0, 1, 2, 7, 63, 1023, 32767, max_21] {
        for y in [0, 1, 3, 8, 64, 2048, max_21] {
            for z in [0, 1, 5, 9, 128, 4096, max_21] {
                let code = morton_encode_3d(x, y, z);
                assert_eq!(
                    morton_decode_3d(code),
                    (x, y, z),
                    "failed for ({x}, {y}, {z})"
                );
            }
        }
    }
}

#[test]
fn bounding_box_and_chunker_morton_intervals() {
    let chunker2d = GridChunker2D::new(10).unwrap();
    assert_eq!(chunker2d.chunk_coords(0, 0), (0, 0));
    assert_eq!(chunker2d.chunk_coords(9, 9), (0, 0));
    assert_eq!(chunker2d.chunk_coords(10, 0), (1, 0));
    assert_eq!(chunker2d.chunk_coords(15, 25), (1, 2));

    let bbox = BoundingBox2D::new(5, 5, 25, 25);
    assert!(bbox.contains_point(5, 5));
    assert!(bbox.contains_point(25, 25));
    assert!(bbox.contains_point(15, 20));
    assert!(!bbox.contains_point(4, 5));
    assert!(!bbox.contains_point(26, 25));

    let bbox_disjoint = BoundingBox2D::new(30, 30, 40, 40);
    assert!(!bbox.intersects(&bbox_disjoint));
    let bbox_overlap = BoundingBox2D::new(20, 20, 30, 30);
    assert!(bbox.intersects(&bbox_overlap));

    // Bounding box [5..=25] spans chunks [0..=2] in x and y
    // Total chunks: 3 x 3 = 9 chunks
    let intervals = bbox.morton_intervals_chunked(&chunker2d);
    assert!(!intervals.is_empty());
    for (start, end) in &intervals {
        assert!(start <= end);
    }

    // 3D chunker and bounding box
    let chunker3d = GridChunker3D::new(10).unwrap();
    let bbox3d = BoundingBox3D::new(5, 5, 5, 15, 15, 15);
    assert!(bbox3d.contains_point(10, 10, 10));
    assert!(!bbox3d.contains_point(20, 10, 10));

    let intervals3d = bbox3d.morton_intervals_chunked(&chunker3d);
    assert!(!intervals3d.is_empty());
}

#[test]
fn projections_zero_duplication_and_individual_queries() {
    let mut entities = EntityProjection::new();
    let e1 = EntityId {
        shard: ShardId(0),
        slot: 1,
        generation: 1,
    };
    let e2 = EntityId {
        shard: ShardId(0),
        slot: 2,
        generation: 1,
    };

    entities.insert(e1, 10);
    entities.insert(e1, 15);
    entities.insert(e2, 20);

    assert_eq!(entities.sequences_for_entity(e1), &[10, 15]);
    assert_eq!(entities.sequences_for_entity(e2), &[20]);
    let e_missing = EntityId {
        shard: ShardId(0),
        slot: 99,
        generation: 1,
    };
    assert!(entities.sequences_for_entity(e_missing).is_empty());

    // Temporal
    let mut temporal = TemporalProjection::new();
    temporal.insert(ClockId(1), 100, 1);
    temporal.insert(ClockId(1), 150, 2);
    temporal.insert(ClockId(1), 200, 3);
    temporal.insert(ClockId(2), 150, 4); // Different clock

    assert_eq!(
        temporal.sequences_in_range(ClockId(1), 100, 150),
        vec![1, 2]
    );
    assert_eq!(
        temporal.sequences_in_range(ClockId(1), 150, 200),
        vec![2, 3]
    );
    assert_eq!(temporal.sequences_in_range(ClockId(2), 100, 200), vec![4]);
    assert!(temporal.sequences_in_range(ClockId(3), 100, 200).is_empty());

    // Schema
    let mut schemas = SchemaBitmapProjection::new();
    schemas.insert(SchemaId(1), 1);
    schemas.insert(SchemaId(1), 3);
    schemas.insert(SchemaId(2), 2);
    assert_eq!(schemas.sequences_for_schema(SchemaId(1)), &[1, 3]);
    assert_eq!(schemas.sequences_for_schema(SchemaId(2)), &[2]);
    assert!(schemas.sequences_for_schema(SchemaId(99)).is_empty());

    // Spatial Morton
    let chunker = GridChunker2D::new(10).unwrap();
    let mut spatial = SpatialMortonProjection::new();
    let code_a = chunker.chunk_morton(5, 5); // chunk (0, 0)
    let code_b = chunker.chunk_morton(15, 15); // chunk (1, 1)
    spatial.insert(code_a, 1);
    spatial.insert(code_b, 2);

    let bbox_a = BoundingBox2D::new(0, 0, 9, 9);
    assert_eq!(spatial.sequences_in_box(&bbox_a, &chunker), vec![1]);
    let bbox_both = BoundingBox2D::new(0, 0, 20, 20);
    assert_eq!(spatial.sequences_in_box(&bbox_both, &chunker), vec![1, 2]);
}

#[test]
fn composite_projection_index_multi_predicate_intersection() {
    let mut index = ProjectionIndex::new();
    let chunker = GridChunker2D::new(10).unwrap();

    let e1 = EntityId {
        shard: ShardId(0),
        slot: 1,
        generation: 1,
    };
    let e2 = EntityId {
        shard: ShardId(0),
        slot: 2,
        generation: 1,
    };

    // Event 0: e1, schema 1, clock 1 tick 100, pos (5, 5)
    index.index_event(
        0,
        e1,
        SchemaId(1),
        Timestamp::new(ClockId(1), 100),
        Some((5, 5)),
        Some(&chunker),
    );

    // Event 1: e1, schema 2, clock 1 tick 110, pos (15, 15)
    index.index_event(
        1,
        e1,
        SchemaId(2),
        Timestamp::new(ClockId(1), 110),
        Some((15, 15)),
        Some(&chunker),
    );

    // Event 2: e2, schema 1, clock 1 tick 120, pos (5, 5)
    index.index_event(
        2,
        e2,
        SchemaId(1),
        Timestamp::new(ClockId(1), 120),
        Some((5, 5)),
        Some(&chunker),
    );

    // Event 3: e2, schema 1, clock 1 tick 130, pos (25, 25)
    index.index_event(
        3,
        e2,
        SchemaId(1),
        Timestamp::new(ClockId(1), 130),
        Some((25, 25)),
        Some(&chunker),
    );

    assert_eq!(index.total_indexed, 4);

    // Query 1: Filter by Entity e1 only -> sequences [0, 1]
    assert_eq!(
        index.query_intersect(Some(e1), None, None, None),
        vec![0, 1]
    );

    // Query 2: Filter by Entity e1 AND Schema 1 -> sequence [0]
    assert_eq!(
        index.query_intersect(Some(e1), Some(SchemaId(1)), None, None),
        vec![0]
    );

    // Query 3: Filter by Schema 1 only -> sequences [0, 2, 3]
    assert_eq!(
        index.query_intersect(None, Some(SchemaId(1)), None, None),
        vec![0, 2, 3]
    );

    // Query 4: Filter by Schema 1 AND Time in 100..=120 -> sequences [0, 2]
    assert_eq!(
        index.query_intersect(None, Some(SchemaId(1)), Some((ClockId(1), 100, 120)), None),
        vec![0, 2]
    );

    // Query 5: Filter by Schema 1 AND Spatial box [0..=10, 0..=10] (chunk (0, 0)) -> sequences [0, 2]
    let bbox = BoundingBox2D::new(0, 0, 10, 10);
    let intervals = bbox.morton_intervals_chunked(&chunker);
    assert_eq!(
        index.query_intersect(None, Some(SchemaId(1)), None, Some(&intervals)),
        vec![0, 2]
    );

    // Query 6: Filter by Entity e2 AND Spatial box [0..=10, 0..=10] -> sequence [2]
    assert_eq!(
        index.query_intersect(Some(e2), None, None, Some(&intervals)),
        vec![2]
    );

    // Query 7: Disjoint filters -> empty result
    assert!(
        index
            .query_intersect(Some(e1), Some(SchemaId(99)), None, None)
            .is_empty()
    );
}

#[test]
fn projection_index_encode_decode_roundtrip_and_tampering() {
    let mut index = ProjectionIndex::new();
    let chunker = GridChunker2D::new(10).unwrap();

    let e = EntityId {
        shard: ShardId(0),
        slot: 5,
        generation: 2,
    };
    index.index_event(
        42,
        e,
        SchemaId(10),
        Timestamp::new(ClockId(1), 500),
        Some((15, 25)),
        Some(&chunker),
    );

    let bytes = index.encode();
    assert_eq!(&bytes[0..4], b"TNPR");

    let decoded = ProjectionIndex::decode(&bytes).expect("successful decode");
    assert_eq!(decoded.total_indexed, 1);
    assert_eq!(decoded.entities.sequences_for_entity(e), &[42]);
    assert_eq!(decoded.schemas.sequences_for_schema(SchemaId(10)), &[42]);
    assert_eq!(
        decoded.temporal.sequences_in_range(ClockId(1), 400, 600),
        vec![42]
    );

    // Checksum tamper detection
    let mut corrupted = bytes.clone();
    let last = corrupted.len() - 1;
    corrupted[last] ^= 0x55;
    assert_eq!(
        ProjectionIndex::decode(&corrupted).unwrap_err(),
        IndexError::ChecksumMismatch
    );

    // Header truncation detection (< 12 bytes)
    assert_eq!(
        ProjectionIndex::decode(&bytes[..8]).unwrap_err(),
        IndexError::Format("projection index truncated")
    );

    // Inner truncation with recomputed checksum
    use crc32fast::Hasher;
    let truncated_body = &bytes[12..bytes.len() - 5];
    let mut hasher = Hasher::new();
    hasher.update(truncated_body);
    let new_checksum = hasher.finalize();
    let mut truncated_msg = bytes[..12].to_vec();
    truncated_msg[8..12].copy_from_slice(&new_checksum.to_le_bytes());
    truncated_msg.extend_from_slice(truncated_body);
    assert_eq!(
        ProjectionIndex::decode(&truncated_msg).unwrap_err(),
        IndexError::Format("unexpected EOF reading u64")
    );

    // Magic mismatch detection
    let mut bad_magic = bytes.clone();
    bad_magic[0] = b'X';
    assert_eq!(
        ProjectionIndex::decode(&bad_magic).unwrap_err(),
        IndexError::InvalidMagic
    );
}
